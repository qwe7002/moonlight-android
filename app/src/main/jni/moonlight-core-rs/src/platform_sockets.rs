//! Platform Sockets - WireGuard zero-copy socket wrappers
//!
//! This module replaces key PlatformSockets.c functions with WG-aware versions:
//! - `bindUdpSocket`: creates real socket + registers WG receive channel
//! - `recvUdpSocket`: reads from WG channel when available (zero-copy receive)
//! - `closeSocket`: cleans up WG socket tracking
//! - `wg_sendto`: intercepts sendto calls to encapsulate directly through WG (zero-copy send)
//!
//! When WG is not active, all functions delegate to the original C implementations.
//!
//! Architecture:
//! ```text
//! [moonlight-common-c]                    [WireGuard tunnel]
//!   VideoStream                            endpoint_receiver_loop
//!     sendto(ping) → wg_sendto ----→ WG encapsulate → endpoint
//!     recvUdpSocket ← channel  ←---- WG decapsulate ← endpoint
//!   AudioStream
//!     sendto(ping) → wg_sendto ----→ WG encapsulate → endpoint
//!     recvUdpSocket ← channel  ←---- WG decapsulate ← endpoint
//! ```

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, RecvTimeoutError};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use log::{debug, info, warn};
use parking_lot::Mutex;

// ============================================================================
// Constants
// ============================================================================

/// Default recv timeout matching UDP_RECV_POLL_TIMEOUT_MS from Limelight-internal.h
const DEFAULT_RECV_TIMEOUT_MS: u64 = 100;

/// Channel buffer size - enough for ~1000 packets in flight
const CHANNEL_BUFFER_SIZE: usize = 1024;

// ============================================================================
// Global WG routing state
// ============================================================================

/// Whether WG zero-copy routing is active
static WG_ROUTING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// WG routing configuration
struct WgRoutingConfig {
    /// Client's WG tunnel IP (e.g., 10.0.0.2)
    tunnel_ip: Ipv4Addr,
    /// Server's WG tunnel IP (e.g., 10.0.0.1)
    server_ip: Ipv4Addr,
}

static WG_CONFIG: Mutex<Option<WgRoutingConfig>> = Mutex::new(None);

/// Per-socket WG information
struct WgUdpSocketInfo {
    /// Sender side of the channel (cloned for port registration)
    sender: SyncSender<Vec<u8>>,
    /// Receiver side of the channel (used by recvUdpSocket)
    receiver: Mutex<Receiver<Vec<u8>>>,
    /// Local bound port of this socket
    local_port: u16,
    /// Remote port this socket communicates with (set on first sendto)
    remote_port: Mutex<Option<u16>>,
}

/// Map from socket FD → WG UDP socket info
static WG_UDP_SOCKETS: LazyLock<Mutex<HashMap<i32, Arc<WgUdpSocketInfo>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Map from remote server port → channel sender
/// This is how endpoint_receiver_loop routes decapsulated UDP data to the right socket
static WG_PORT_SENDERS: LazyLock<Mutex<HashMap<u16, SyncSender<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// External C functions from PlatformSockets.c (compiled with renamed symbols)
// ============================================================================

extern "C" {
    /// Original recvUdpSocket from PlatformSockets.c (renamed via -D define)
    fn orig_recvUdpSocket(s: i32, buffer: *mut libc::c_char, size: i32, useSelect: bool) -> i32;

    /// Original bindUdpSocket from PlatformSockets.c (renamed via -D define)
    fn orig_bindUdpSocket(
        addressFamily: libc::c_int,
        localAddr: *mut libc::sockaddr_storage,
        addrLen: libc::socklen_t,
        bufferSize: libc::c_int,
        socketQosType: libc::c_int,
    ) -> i32;

    /// Original closeSocket from PlatformSockets.c (renamed via -D define)
    fn orig_closeSocket(s: i32);
}

// ============================================================================
// Public API for WG integration (called from wireguard.rs)
// ============================================================================

/// Enable WG zero-copy routing with the given tunnel and server IPs.
/// Called from wg_create_streaming_proxies after proxy creation.
pub fn enable_wg_routing(tunnel_ip: Ipv4Addr, server_ip: Ipv4Addr) {
    let mut config = WG_CONFIG.lock();
    *config = Some(WgRoutingConfig { tunnel_ip, server_ip });
    WG_ROUTING_ACTIVE.store(true, Ordering::SeqCst);
    info!(
        "WG zero-copy routing enabled: tunnel_ip={}, server_ip={}",
        tunnel_ip, server_ip
    );
}

/// Disable WG zero-copy routing and clean up all tracked sockets.
/// Called from wg_stop_tunnel.
pub fn disable_wg_routing() {
    WG_ROUTING_ACTIVE.store(false, Ordering::SeqCst);
    WG_CONFIG.lock().take();
    WG_UDP_SOCKETS.lock().clear();
    WG_PORT_SENDERS.lock().clear();
    info!("WG zero-copy routing disabled");
}

/// Try to deliver UDP data to a registered zero-copy channel.
/// Called from endpoint_receiver_loop when a UDP packet is decapsulated.
///
/// Returns true if data was delivered to a channel, false if no channel exists
/// for this port (fallback to proxy).
pub fn try_push_udp_data(src_port: u16, data: &[u8]) -> bool {
    let senders = WG_PORT_SENDERS.lock();
    if let Some(sender) = senders.get(&src_port) {
        match sender.try_send(data.to_vec()) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                warn!(
                    "WG zero-copy channel full for port {} (dropping packet)",
                    src_port
                );
                // Channel full - packet dropped. This shouldn't happen normally
                // as the receiver should be draining fast enough.
                true // Still return true to avoid double-delivery through proxy
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                debug!("WG zero-copy channel disconnected for port {}", src_port);
                false
            }
        }
    } else {
        false
    }
}

/// Check if WG routing is active (for use by other modules)
pub fn is_wg_routing_active() -> bool {
    WG_ROUTING_ACTIVE.load(Ordering::SeqCst)
}

// ============================================================================
// Socket wrapper functions (extern "C", called by moonlight-common-c)
// ============================================================================

/// WG-aware recvUdpSocket: reads from WG channel for tracked sockets.
///
/// When WG is active and this socket is registered, data is read directly from
/// the WG decapsulation channel, bypassing the kernel UDP stack entirely.
///
/// Returns: >0 bytes received, 0 timeout, <0 fatal error
#[no_mangle]
pub unsafe extern "C" fn recvUdpSocket(
    s: i32,
    buffer: *mut libc::c_char,
    size: i32,
    useSelect: bool,
) -> i32 {
    // Fast path: if WG routing not active, delegate immediately
    if !WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        return orig_recvUdpSocket(s, buffer, size, useSelect);
    }

    // Check if this socket is WG-tracked
    let socket_info = {
        let sockets = WG_UDP_SOCKETS.lock();
        sockets.get(&s).cloned()
    };

    if let Some(info) = socket_info {
        // WG zero-copy path: read from channel
        let receiver = info.receiver.lock();
        let timeout = Duration::from_millis(DEFAULT_RECV_TIMEOUT_MS);

        match receiver.recv_timeout(timeout) {
            Ok(data) => {
                let copy_len = std::cmp::min(data.len(), size as usize);
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    buffer as *mut u8,
                    copy_len,
                );
                copy_len as i32
            }
            Err(RecvTimeoutError::Timeout) => {
                // Timeout - same as original behavior (return 0)
                0
            }
            Err(RecvTimeoutError::Disconnected) => {
                // Channel closed - treat as error
                -1
            }
        }
    } else {
        // Not a WG socket, use original implementation
        orig_recvUdpSocket(s, buffer, size, useSelect)
    }
}

/// WG-aware bindUdpSocket: creates real socket + registers WG receive channel.
///
/// The real socket is still created (for sendto compatibility and as fallback),
/// but a zero-copy channel is also set up for the WG receive path.
#[no_mangle]
pub unsafe extern "C" fn bindUdpSocket(
    addressFamily: libc::c_int,
    localAddr: *mut libc::sockaddr_storage,
    addrLen: libc::socklen_t,
    bufferSize: libc::c_int,
    socketQosType: libc::c_int,
) -> i32 {
    // Always create the real socket via original implementation
    let fd = orig_bindUdpSocket(addressFamily, localAddr, addrLen, bufferSize, socketQosType);

    if fd < 0 {
        return fd; // Socket creation failed
    }

    // If WG routing is active, register this socket for zero-copy
    if WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        let local_port = get_socket_local_port(fd);

        // Create bounded channel for WG data delivery
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_BUFFER_SIZE);

        let info = Arc::new(WgUdpSocketInfo {
            sender,
            receiver: Mutex::new(receiver),
            local_port,
            remote_port: Mutex::new(None),
        });

        WG_UDP_SOCKETS.lock().insert(fd, info);
        debug!(
            "Registered WG zero-copy UDP socket: fd={}, local_port={}, qos={}",
            fd, local_port, socketQosType
        );
    }

    fd
}

/// WG-aware closeSocket: cleans up WG tracking before closing.
#[no_mangle]
pub unsafe extern "C" fn closeSocket(s: i32) {
    // Clean up WG tracking if active
    if WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        let removed = WG_UDP_SOCKETS.lock().remove(&s);
        if let Some(info) = removed {
            // Also remove the port → sender mapping
            if let Some(remote_port) = *info.remote_port.lock() {
                WG_PORT_SENDERS.lock().remove(&remote_port);
                debug!(
                    "Cleaned up WG zero-copy socket: fd={}, remote_port={}",
                    s, remote_port
                );
            }
        }
    }

    // Close the real socket
    orig_closeSocket(s);
}

/// WG-aware sendto: encapsulates UDP directly through WireGuard for tracked sockets.
///
/// This function is called via the `sendto` macro redirect in wg_intercept.h.
/// For WG-tracked sockets targeting the WG server, data is encapsulated directly
/// into a WG packet, bypassing the kernel UDP stack.
///
/// On first call for a socket, also establishes the port → channel mapping
/// so that response data from the server is routed to the correct channel.
#[no_mangle]
pub unsafe extern "C" fn wg_sendto(
    sockfd: libc::c_int,
    buf: *const libc::c_void,
    len: libc::size_t,
    flags: libc::c_int,
    dest_addr: *const libc::sockaddr,
    addrlen: libc::socklen_t,
) -> libc::ssize_t {
    // Fast path: if WG routing not active, use real sendto
    if !WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
    }

    // Check if this socket is WG-tracked
    let socket_info = {
        let sockets = WG_UDP_SOCKETS.lock();
        sockets.get(&sockfd).cloned()
    };

    let info = match socket_info {
        Some(info) => info,
        None => {
            // Not a WG-tracked socket, use real sendto
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
    };

    // Extract destination IP and port
    let (dest_ip, dest_port) = match extract_addr_from_sockaddr(dest_addr) {
        Some(addr) => addr,
        None => {
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
    };

    // Only intercept traffic to the WG-proxied server (127.0.0.1 == proxy target)
    let config = WG_CONFIG.lock();
    let cfg = match config.as_ref() {
        Some(cfg) => cfg,
        None => {
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
    };

    // Check if destination is the proxy address (127.0.0.1) or the WG server
    let is_wg_target = dest_ip == Ipv4Addr::new(127, 0, 0, 1)
        || dest_ip == cfg.server_ip;

    if !is_wg_target {
        drop(config);
        // Not targeting WG server (e.g., STUN), use real sendto
        return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
    }

    let tunnel_ip = cfg.tunnel_ip;
    let server_ip = cfg.server_ip;
    let local_port = info.local_port;
    drop(config);

    // Register port → channel mapping on first sendto to this port
    {
        let mut remote_port_lock = info.remote_port.lock();
        if remote_port_lock.is_none() || *remote_port_lock != Some(dest_port) {
            *remote_port_lock = Some(dest_port);

            let mut senders = WG_PORT_SENDERS.lock();
            senders.insert(dest_port, info.sender.clone());
            info!(
                "WG zero-copy: registered port mapping fd={} local_port={} <-> remote_port={}",
                sockfd, local_port, dest_port
            );
        }
    }

    // Build UDP/IP packet and send through WireGuard
    let data = std::slice::from_raw_parts(buf as *const u8, len);
    let src_addr = SocketAddr::new(IpAddr::V4(tunnel_ip), local_port);
    let dst_addr = SocketAddr::new(IpAddr::V4(server_ip), dest_port);

    let ip_packet = crate::wireguard::build_udp_ip_packet(src_addr, dst_addr, data);

    match crate::wireguard::wg_send_ip_packet(&ip_packet) {
        Ok(()) => len as libc::ssize_t,
        Err(e) => {
            warn!("wg_sendto: failed to send through WG: {}", e);
            // On WG send failure, fall back to real sendto (goes through proxy)
            libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen)
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Get the local port of a bound socket via getsockname
fn get_socket_local_port(fd: i32) -> u16 {
    unsafe {
        let mut addr: libc::sockaddr_storage = std::mem::zeroed();
        let mut len: libc::socklen_t =
            std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getsockname(fd, &mut addr as *mut _ as *mut libc::sockaddr, &mut len) == 0 {
            match addr.ss_family as i32 {
                libc::AF_INET => {
                    let sin = &*(&addr as *const _ as *const libc::sockaddr_in);
                    u16::from_be(sin.sin_port)
                }
                libc::AF_INET6 => {
                    let sin6 = &*(&addr as *const _ as *const libc::sockaddr_in6);
                    u16::from_be(sin6.sin6_port)
                }
                _ => 0,
            }
        } else {
            0
        }
    }
}

/// Extract IPv4 address and port from a sockaddr pointer
fn extract_addr_from_sockaddr(addr: *const libc::sockaddr) -> Option<(Ipv4Addr, u16)> {
    if addr.is_null() {
        return None;
    }
    unsafe {
        match (*addr).sa_family as i32 {
            libc::AF_INET => {
                let sin = &*(addr as *const libc::sockaddr_in);
                let port = u16::from_be(sin.sin_port);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                Some((ip, port))
            }
            libc::AF_INET6 => {
                let sin6 = &*(addr as *const libc::sockaddr_in6);
                let port = u16::from_be(sin6.sin6_port);
                // Check if it's a v4-mapped v6 address (::ffff:x.x.x.x)
                let octets = sin6.sin6_addr.s6_addr;
                if octets[0..10] == [0; 10] && octets[10] == 0xff && octets[11] == 0xff {
                    let ip = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                    Some((ip, port))
                } else {
                    // Pure IPv6 - not WG-routable in our setup
                    None
                }
            }
            _ => None,
        }
    }
}
