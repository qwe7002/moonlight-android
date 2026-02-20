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

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crossbeam_channel::{self, Receiver, Sender, RecvTimeoutError, TrySendError};
use log::{debug, error, info, warn};
use parking_lot::Mutex;

// ============================================================================
// Constants
// ============================================================================

/// Default recv timeout matching UDP_RECV_POLL_TIMEOUT_MS from Limelight-internal.h
const DEFAULT_RECV_TIMEOUT_MS: u64 = 100;

/// Channel buffer size - large enough for burst video frames at high bitrate.
/// Using 4096 reduces packet drops during I-frame bursts.
const CHANNEL_BUFFER_SIZE: usize = 4096;

/// Maximum number of pending packets buffered per port before any channel is registered.
/// Protects against unbounded memory growth if a port is never registered.
const MAX_PENDING_PACKETS_PER_PORT: usize = 512;

/// Maximum UDP/IP packet size for thread-local buffer
const MAX_IP_PACKET_SIZE: usize = 65535 + 48; // IPv6 header (40) + UDP header (8) + max payload

/// Starting FD for virtual WG TCP sockets (high value to avoid conflicts)
const WG_TCP_FD_BASE: i32 = 100000;

// ============================================================================
// Global WG routing state
// ============================================================================

/// Whether WG zero-copy routing is active
static WG_ROUTING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Counter for virtual WG TCP socket FDs
static WG_TCP_FD_COUNTER: AtomicI32 = AtomicI32::new(WG_TCP_FD_BASE);

/// WG routing configuration (supports both IPv4 and IPv6)
struct WgRoutingConfig {
    /// Client's WG tunnel IP (e.g., 10.0.0.2 or fd00::2)
    tunnel_ip: IpAddr,
    /// Server's WG tunnel IP (e.g., 10.0.0.1 or fd00::1)
    server_ip: IpAddr,
}

static WG_CONFIG: Mutex<Option<WgRoutingConfig>> = Mutex::new(None);

/// Per-socket WG information
struct WgUdpSocketInfo {
    /// Sender side of the channel (cloned for port registration)
    sender: Sender<Vec<u8>>,
    /// Receiver side of the channel (used by recvUdpSocket)
    /// crossbeam Receiver is Send+Sync so no Mutex needed - eliminates lock on recv hot path
    receiver: Receiver<Vec<u8>>,
    /// Local bound port of this socket
    local_port: u16,
    /// Remote port this socket communicates with (set on first sendto)
    remote_port: Mutex<Option<u16>>,
}

/// Per-socket WG information (TCP)
/// Maps virtual FD to wg_socket handle
struct WgTcpSocketInfo {
    /// Handle returned by wg_socket_connect
    wg_handle: u64,
    /// Whether the connection is open
    is_open: AtomicBool,
}

/// Map from socket FD → WG UDP socket info
static WG_UDP_SOCKETS: LazyLock<Mutex<HashMap<i32, Arc<WgUdpSocketInfo>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Map from virtual FD → WG TCP socket info
static WG_TCP_SOCKETS: LazyLock<Mutex<HashMap<i32, Arc<WgTcpSocketInfo>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Map from remote server port → channel sender
/// This is how endpoint_receiver_loop routes decapsulated UDP data to the right socket
static WG_PORT_SENDERS: LazyLock<Mutex<HashMap<u16, Sender<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// Inject-mode socket tracking (for ENet and other direct socket() callers)
// ============================================================================

/// Info for auto-registered inject-mode sockets.
/// These sockets were created directly (e.g., by ENet) rather than via bindUdpSocket.
/// Incoming WG data is injected to the real socket via loopback sendto.
#[derive(Clone, Copy)]
struct WgInjectSocketInfo {
    _local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
}

/// Map from socket FD → inject info (for recvfrom address fixup)
static WG_INJECT_SOCKETS: LazyLock<Mutex<HashMap<i32, WgInjectSocketInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Map from remote server port → local port (for inject delivery routing)
static WG_INJECT_PORT_MAP: LazyLock<Mutex<HashMap<u16, u16>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Global inject socket FD (used to send data to local sockets via loopback)
static WG_INJECT_FD: Mutex<Option<i32>> = Mutex::new(None);

/// Map from socket FD → virtually connected peer address.
/// Used to track UDP sockets that called connect() to the WG server.
/// We skip the real connect() so the socket can receive loopback-injected data.
static WG_UDP_CONNECTED_PEERS: LazyLock<Mutex<HashMap<i32, SocketAddr>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Pending packets buffer for server ports not yet registered.
/// When WG decapsulates UDP data for a port that has no channel or inject mapping,
/// packets are queued here. They are flushed into the channel once wg_sendto()
/// registers the port → sender mapping.
static WG_PENDING_PACKETS: LazyLock<Mutex<HashMap<u16, VecDeque<Vec<u8>>>>> =
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

    /// Original connectTcpSocket from PlatformSockets.c (renamed via -D define)
    fn orig_connectTcpSocket(
        dstaddr: *mut libc::sockaddr_storage,
        addrlen: libc::socklen_t,
        port: libc::c_ushort,
        timeoutSec: libc::c_int,
    ) -> i32;

    /// Original shutdownTcpSocket from PlatformSockets.c (renamed via -D define)
    fn orig_shutdownTcpSocket(s: i32);

    /// Original pollSockets from PlatformSockets.c (renamed via -D define)
    fn orig_pollSockets(pollFds: *mut libc::pollfd, pollFdsCount: libc::c_int, timeoutMs: libc::c_int) -> libc::c_int;
}

// ============================================================================
// Public API for WG integration (called from wireguard.rs)
// ============================================================================

/// Enable WG zero-copy routing with the given tunnel and server IPs.
/// Called from wg_create_streaming_proxies after proxy creation.
pub fn enable_wg_routing(tunnel_ip: impl Into<IpAddr>, server_ip: impl Into<IpAddr>) {
    let tunnel_ip = tunnel_ip.into();
    let server_ip = server_ip.into();
    let mut config = WG_CONFIG.lock();
    *config = Some(WgRoutingConfig { tunnel_ip, server_ip });
    WG_ROUTING_ACTIVE.store(true, Ordering::Release);
    info!(
        "WG zero-copy routing enabled: tunnel_ip={}, server_ip={}",
        tunnel_ip, server_ip
    );
}

/// Disable WG zero-copy routing and clean up all tracked sockets.
/// Called from wg_stop_tunnel.
pub fn disable_wg_routing() {
    WG_ROUTING_ACTIVE.store(false, Ordering::Release);
    WG_CONFIG.lock().take();
    WG_UDP_SOCKETS.lock().clear();
    WG_TCP_SOCKETS.lock().clear();
    WG_PORT_SENDERS.lock().clear();
    WG_INJECT_SOCKETS.lock().clear();
    WG_INJECT_PORT_MAP.lock().clear();
    WG_UDP_CONNECTED_PEERS.lock().clear();
    WG_PENDING_PACKETS.lock().clear();
    // Close inject socket
    if let Some(fd) = WG_INJECT_FD.lock().take() {
        unsafe { libc::close(fd); }
    }
    // Reset TCP FD counter
    WG_TCP_FD_COUNTER.store(WG_TCP_FD_BASE, Ordering::Relaxed);
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
            Err(TrySendError::Full(_)) => {
                warn!(
                    "WG zero-copy channel full for port {} (dropping packet)",
                    src_port
                );
                // Channel full - packet dropped. This shouldn't happen normally
                // as the receiver should be draining fast enough.
                true // Still return true to avoid double-delivery through proxy
            }
            Err(TrySendError::Disconnected(_)) => {
                debug!("WG zero-copy channel disconnected for port {}", src_port);
                false
            }
        }
    } else {
        false
    }
}

/// Buffer a UDP packet for a server port that has no channel or inject mapping yet.
/// Called from the WG receiver thread when both try_push_udp_data and
/// try_inject_udp_data return false.
///
/// The packet is stored in WG_PENDING_PACKETS and will be flushed into the
/// appropriate channel once wg_sendto() registers the port mapping.
pub fn buffer_pending_udp_data(src_port: u16, data: &[u8]) {
    let mut pending = WG_PENDING_PACKETS.lock();
    let queue = pending.entry(src_port).or_insert_with(VecDeque::new);
    if queue.len() < MAX_PENDING_PACKETS_PER_PORT {
        queue.push_back(data.to_vec());
        debug!(
            "WG pending: buffered packet for port {} ({} bytes, queue_len={})",
            src_port,
            data.len(),
            queue.len()
        );
    } else {
        // Drop oldest packet to make room (ring-buffer style)
        queue.pop_front();
        queue.push_back(data.to_vec());
        debug!(
            "WG pending: buffer full for port {}, dropped oldest (queue_len={})",
            src_port,
            queue.len()
        );
    }
}

/// Flush pending packets for a server port into the given channel sender.
/// Called from wg_sendto() when a new port → sender mapping is registered.
fn flush_pending_udp_data(remote_port: u16, sender: &Sender<Vec<u8>>) {
    let mut pending = WG_PENDING_PACKETS.lock();
    if let Some(queue) = pending.remove(&remote_port) {
        let count = queue.len();
        let mut delivered = 0usize;
        for pkt in queue {
            match sender.try_send(pkt) {
                Ok(()) => delivered += 1,
                Err(TrySendError::Full(_)) => {
                    warn!(
                        "WG pending flush: channel full for port {} after {} packets",
                        remote_port, delivered
                    );
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    warn!("WG pending flush: channel disconnected for port {}", remote_port);
                    break;
                }
            }
        }
        if delivered > 0 {
            info!(
                "WG pending flush: delivered {}/{} buffered packets for port {}",
                delivered, count, remote_port
            );
        }
    }
}

/// Flush pending packets for a server port via inject (loopback) delivery.
/// Called from wg_sendto() when a new inject socket mapping is registered.
fn flush_pending_inject_data(remote_port: u16, local_port: u16) {
    let mut pending = WG_PENDING_PACKETS.lock();
    if let Some(queue) = pending.remove(&remote_port) {
        let count = queue.len();
        let inject_fd = get_or_create_inject_fd();
        if inject_fd < 0 {
            warn!("WG pending inject flush: failed to create inject socket");
            return;
        }

        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
        addr.sin_port = local_port.to_be();

        let mut delivered = 0usize;
        for pkt in &queue {
            let result = unsafe {
                libc::sendto(
                    inject_fd,
                    pkt.as_ptr() as *const libc::c_void,
                    pkt.len(),
                    0,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            };
            if result >= 0 {
                delivered += 1;
            } else {
                warn!("WG pending inject flush: sendto failed for port {}", remote_port);
                break;
            }
        }
        if delivered > 0 {
            info!(
                "WG pending inject flush: delivered {}/{} buffered packets for port {}",
                delivered, count, remote_port
            );
        }
    }
}

/// Check if WG routing is active (for use by other modules)
pub fn is_wg_routing_active() -> bool {
    WG_ROUTING_ACTIVE.load(Ordering::Acquire)
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
        // WG zero-copy path: read from crossbeam channel (lock-free receive)
        let timeout = Duration::from_millis(DEFAULT_RECV_TIMEOUT_MS);

        match info.receiver.recv_timeout(timeout) {
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

        // Create bounded crossbeam channel for WG data delivery
        // crossbeam-channel is significantly faster than std::sync::mpsc
        // for both send (try_send ~40ns vs ~200ns) and recv (~50ns vs ~300ns)
        let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_BUFFER_SIZE);

        let info = Arc::new(WgUdpSocketInfo {
            sender,
            receiver,  // No Mutex needed - crossbeam Receiver is Sync
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
    // Check if this is a WG TCP socket (virtual FD >= WG_TCP_FD_BASE)
    if s >= WG_TCP_FD_BASE {
        let removed = WG_TCP_SOCKETS.lock().remove(&s);
        if let Some(info) = removed {
            info.is_open.store(false, Ordering::Release);
            crate::wg_socket::wg_socket_close(info.wg_handle);
            debug!("Closed WG TCP socket: virtual_fd={}, handle={}", s, info.wg_handle);
        }
        return; // Virtual FD, don't call orig_closeSocket
    }

    // Clean up WG UDP tracking if active
    if WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        let removed = WG_UDP_SOCKETS.lock().remove(&s);
        if let Some(info) = removed {
            // Also remove the port → sender mapping
            if let Some(remote_port) = *info.remote_port.lock() {
                WG_PORT_SENDERS.lock().remove(&remote_port);
                debug!(
                    "Cleaned up WG zero-copy UDP socket: fd={}, remote_port={}",
                    s, remote_port
                );
            }
        }
        
        // Clean up inject-mode socket tracking
        if let Some(info) = WG_INJECT_SOCKETS.lock().remove(&s) {
            // Also clean up the port map entry to prevent stale mappings
            // from capturing packets intended for a future socket on the
            // same remote port (e.g., ENet reconnection to port 47999).
            WG_INJECT_PORT_MAP.lock().remove(&info.remote_port);
            debug!("Cleaned up inject socket: fd={}, remote_port={}", s, info.remote_port);
        }
        
        // Clean up virtual UDP connection tracking
        WG_UDP_CONNECTED_PEERS.lock().remove(&s);
    }

    // Close the real socket
    orig_closeSocket(s);
}

/// WG-aware pollSockets: handles both real FDs and WG virtual TCP FDs.
///
/// This wraps the original pollSockets to support WireGuard virtual TCP sockets.
/// For virtual FDs (>= WG_TCP_FD_BASE), we check data availability using our
/// internal mechanisms. For real FDs, we delegate to the original implementation.
#[no_mangle]
pub unsafe extern "C" fn pollSockets(
    poll_fds: *mut libc::pollfd,
    poll_fds_count: libc::c_int,
    timeout_ms: libc::c_int,
) -> libc::c_int {
    if poll_fds.is_null() || poll_fds_count <= 0 {
        return orig_pollSockets(poll_fds, poll_fds_count, timeout_ms);
    }

    let fds = std::slice::from_raw_parts_mut(poll_fds, poll_fds_count as usize);
    
    // Separate virtual FDs from real FDs
    let mut has_virtual = false;
    let mut has_real = false;
    
    for pfd in fds.iter() {
        if pfd.fd >= WG_TCP_FD_BASE {
            has_virtual = true;
        } else if pfd.fd >= 0 {
            has_real = true;
        }
    }
    
    // If only real FDs, delegate entirely to original
    if !has_virtual {
        return orig_pollSockets(poll_fds, poll_fds_count, timeout_ms);
    }
    
    // If only virtual FDs, handle entirely in Rust
    if !has_real {
        return poll_virtual_only(fds, timeout_ms);
    }
    
    // Mixed case: poll both
    // First, check virtual FDs (non-blocking)
    let mut ready_count = 0;
    for pfd in fds.iter_mut() {
        pfd.revents = 0;
        
        if pfd.fd >= WG_TCP_FD_BASE {
            // Virtual WG TCP socket
            let tcp_info = WG_TCP_SOCKETS.lock().get(&pfd.fd).cloned();
            if let Some(info) = tcp_info {
                if !info.is_open.load(Ordering::Relaxed) {
                    // Socket closed - signal error/hangup
                    pfd.revents = libc::POLLHUP;
                    ready_count += 1;
                } else {
                    // Check for POLLIN (data available)
                    if (pfd.events & libc::POLLIN) != 0 {
                        if crate::wg_socket::wg_socket_has_data(info.wg_handle) {
                            pfd.revents |= libc::POLLIN;
                            ready_count += 1;
                        }
                    }
                    // Check for POLLOUT (always writable for our implementation)
                    if (pfd.events & libc::POLLOUT) != 0 {
                        pfd.revents |= libc::POLLOUT;
                        if pfd.revents == libc::POLLOUT as i16 {
                            ready_count += 1;
                        }
                    }
                }
            } else {
                // Invalid FD
                pfd.revents = libc::POLLNVAL;
                ready_count += 1;
            }
        }
    }
    
    // If virtual FDs are ready, return immediately
    if ready_count > 0 {
        return ready_count;
    }
    
    // Otherwise, poll real FDs with timeout, then check virtual again
    // Create a temporary array for real FDs only
    let real_count = fds.iter().filter(|p| p.fd >= 0 && p.fd < WG_TCP_FD_BASE).count();
    if real_count > 0 {
        // Poll real FDs with shorter timeout, then check virtual
        let poll_timeout = if timeout_ms > 0 { std::cmp::min(timeout_ms, 100) } else { 0 };
        
        let start = std::time::Instant::now();
        let total_timeout = if timeout_ms >= 0 {
            std::time::Duration::from_millis(timeout_ms as u64)
        } else {
            std::time::Duration::from_secs(86400) // Effectively infinite
        };
        
        loop {
            // Create temp array for real FDs
            let mut real_pfds: Vec<libc::pollfd> = fds
                .iter()
                .filter(|p| p.fd >= 0 && p.fd < WG_TCP_FD_BASE)
                .cloned()
                .collect();
            
            let result = orig_pollSockets(real_pfds.as_mut_ptr(), real_pfds.len() as i32, poll_timeout);
            
            // Copy revents back to real FDs
            let mut real_idx = 0;
            for pfd in fds.iter_mut() {
                if pfd.fd >= 0 && pfd.fd < WG_TCP_FD_BASE {
                    pfd.revents = real_pfds[real_idx].revents;
                    if pfd.revents != 0 {
                        ready_count += 1;
                    }
                    real_idx += 1;
                }
            }
            
            // Check virtual FDs again
            for pfd in fds.iter_mut() {
                if pfd.fd >= WG_TCP_FD_BASE {
                    let tcp_info = WG_TCP_SOCKETS.lock().get(&pfd.fd).cloned();
                    if let Some(info) = tcp_info {
                        if !info.is_open.load(Ordering::Relaxed) {
                            pfd.revents = libc::POLLHUP;
                            ready_count += 1;
                        } else {
                            if (pfd.events & libc::POLLIN) != 0 {
                                if crate::wg_socket::wg_socket_has_data(info.wg_handle) {
                                    pfd.revents |= libc::POLLIN;
                                    ready_count += 1;
                                }
                            }
                            if (pfd.events & libc::POLLOUT) != 0 {
                                pfd.revents |= libc::POLLOUT;
                                if pfd.revents == libc::POLLOUT as i16 {
                                    ready_count += 1;
                                }
                            }
                        }
                    } else {
                        pfd.revents = libc::POLLNVAL;
                        ready_count += 1;
                    }
                }
            }
            
            if ready_count > 0 || result < 0 {
                return if result < 0 && ready_count == 0 { result } else { ready_count };
            }
            
            if start.elapsed() >= total_timeout {
                return 0; // Timeout
            }
            
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    
    0
}

/// Poll only virtual WG TCP sockets (no real FDs)
unsafe fn poll_virtual_only(fds: &mut [libc::pollfd], timeout_ms: libc::c_int) -> libc::c_int {
    let start = std::time::Instant::now();
    let timeout = if timeout_ms >= 0 {
        std::time::Duration::from_millis(timeout_ms as u64)
    } else {
        std::time::Duration::from_secs(86400) // Effectively infinite
    };
    
    loop {
        let mut ready_count = 0;
        
        for pfd in fds.iter_mut() {
            pfd.revents = 0;
            
            if pfd.fd >= WG_TCP_FD_BASE {
                let tcp_info = WG_TCP_SOCKETS.lock().get(&pfd.fd).cloned();
                if let Some(info) = tcp_info {
                    if !info.is_open.load(Ordering::Relaxed) {
                        pfd.revents = libc::POLLHUP;
                        ready_count += 1;
                    } else {
                        if (pfd.events & libc::POLLIN) != 0 {
                            if crate::wg_socket::wg_socket_has_data(info.wg_handle) {
                                pfd.revents |= libc::POLLIN;
                                ready_count += 1;
                            }
                        }
                        if (pfd.events & libc::POLLOUT) != 0 {
                            pfd.revents |= libc::POLLOUT;
                            if pfd.revents == libc::POLLOUT as i16 {
                                ready_count += 1;
                            }
                        }
                    }
                } else {
                    pfd.revents = libc::POLLNVAL;
                    ready_count += 1;
                }
            }
        }
        
        if ready_count > 0 {
            return ready_count;
        }
        
        if start.elapsed() >= timeout {
            return 0; // Timeout
        }
        
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
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

    // Extract destination IP and port first (before socket lookup)
    // If dest_addr is NULL, check if we have a virtually connected peer
    let (dest_ip, dest_port): (IpAddr, u16) = if dest_addr.is_null() {
        // Check for virtually connected socket (we intercepted connect())
        let connected_peers = WG_UDP_CONNECTED_PEERS.lock();
        if let Some(peer) = connected_peers.get(&sockfd) {
            (peer.ip(), peer.port())
        } else {
            drop(connected_peers);
            // No virtual connection, pass through
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
    } else {
        match extract_addr_from_sockaddr(dest_addr) {
            Some(addr) => addr,
            None => {
                debug!("wg_sendto: fd={}, len={}, could not extract addr, fallback to real sendto", sockfd, len);
                return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
            }
        }
    };

    debug!("wg_sendto: fd={}, dest={}:{}, len={}", sockfd, dest_ip, dest_port, len);

    // Check if destination is the WG server
    let config = WG_CONFIG.lock();
    let cfg = match config.as_ref() {
        Some(cfg) => cfg,
        None => {
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
    };

    // Check if destination is the WG server
    let is_wg_target = dest_ip == cfg.server_ip;

    if !is_wg_target {
        debug!("wg_sendto: fd={}, dest={}:{} not WG target (server_ip={}), fallback",
               sockfd, dest_ip, dest_port, cfg.server_ip);
        drop(config);
        // Not targeting WG server (e.g., STUN), use real sendto
        return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
    }

    let tunnel_ip = cfg.tunnel_ip;
    let server_ip = cfg.server_ip;
    drop(config);

    // Check if this socket is in WG_UDP_SOCKETS (channel-based, created by bindUdpSocket)
    let socket_info = {
        let sockets = WG_UDP_SOCKETS.lock();
        sockets.get(&sockfd).cloned()
    };

    let local_port = if let Some(ref info) = socket_info {
        // Channel-based socket (created by bindUdpSocket) - register port → channel mapping
        let lp = info.local_port;
        {
            let mut remote_port_lock = info.remote_port.lock();
            if remote_port_lock.is_none() || *remote_port_lock != Some(dest_port) {
                *remote_port_lock = Some(dest_port);
                WG_PORT_SENDERS.lock().insert(dest_port, info.sender.clone());
                info!(
                    "WG zero-copy: registered port mapping fd={} local_port={} <-> remote_port={}",
                    sockfd, lp, dest_port
                );
                // Flush any packets that arrived before this channel was registered.
                // This fixes the race where the server starts sending on a port
                // (e.g., 47998) before the client has sent the first ping.
                flush_pending_udp_data(dest_port, &info.sender);
            }
        }
        lp
    } else {
        // Not a channel-based socket (e.g., ENet) - auto-register for inject delivery.
        // Data from WG will be injected to this socket via loopback sendto,
        // and recvfrom will fix the source address.
        let lp = get_socket_local_port(sockfd);
        if lp == 0 {
            warn!("wg_sendto: could not determine local port for fd={}, falling back", sockfd);
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }

        let mut inject_sockets = WG_INJECT_SOCKETS.lock();
        if !inject_sockets.contains_key(&sockfd) {
            inject_sockets.insert(sockfd, WgInjectSocketInfo {
                _local_port: lp,
                remote_ip: server_ip,
                remote_port: dest_port,
            });
            drop(inject_sockets);
            WG_INJECT_PORT_MAP.lock().insert(dest_port, lp);
            info!(
                "WG auto-registered inject socket: fd={}, local_port={}, remote={}:{}",
                sockfd, lp, server_ip, dest_port
            );
            // Flush any packets that arrived before inject registration
            flush_pending_inject_data(dest_port, lp);
        }
        lp
    };

    // Build UDP/IP packet and send through WireGuard
    // Use thread-local buffer to avoid per-packet heap allocation on the send hot path
    let payload = std::slice::from_raw_parts(buf as *const u8, len);
    let src_addr = SocketAddr::new(tunnel_ip, local_port);
    let dst_addr = SocketAddr::new(server_ip, dest_port);

    debug!("wg_sendto: sending {} bytes via WG: {} -> {} (fd={})", len, src_addr, dst_addr, sockfd);

    thread_local! {
        static IP_PKT_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; MAX_IP_PACKET_SIZE]);
    }

    IP_PKT_BUF.with(|pkt_buf| {
        let mut pkt_buf = pkt_buf.borrow_mut();
        let pkt_len = crate::wireguard::build_udp_ip_packet_into(&mut pkt_buf, src_addr, dst_addr, payload);
        if pkt_len == 0 {
            warn!("wg_sendto: failed to build IP packet (buffer too small?)");
            return libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen);
        }
        match crate::wireguard::wg_send_ip_packet(&pkt_buf[..pkt_len]) {
            Ok(()) => {
                debug!("wg_sendto: successfully sent {} bytes via WG fd={}", len, sockfd);
                len as libc::ssize_t
            }
            Err(e) => {
                warn!("wg_sendto: failed to send through WG: {} (fd={}, dst={})", e, sockfd, dst_addr);
                // On WG send failure, fall back to real sendto (goes through proxy)
                libc::sendto(sockfd, buf, len, flags, dest_addr, addrlen)
            }
        }
    })
}

/// WG-aware recvfrom: fixes source addresses for inject-mode sockets.
///
/// For sockets auto-registered by wg_sendto (e.g., ENet), incoming WG data
/// is injected to the real socket via loopback. This interceptor calls real
/// recvfrom, then replaces the localhost source address with the actual WG
/// server address so ENet's peer address matching works correctly.
///
/// For all other sockets, this is a transparent pass-through.
#[no_mangle]
pub unsafe extern "C" fn wg_recvfrom(
    sockfd: libc::c_int,
    buf: *mut libc::c_void,
    len: libc::size_t,
    flags: libc::c_int,
    src_addr: *mut libc::sockaddr,
    addrlen: *mut libc::socklen_t,
) -> libc::ssize_t {
    // Always call real recvfrom first
    let result = libc::recvfrom(sockfd, buf, len, flags, src_addr, addrlen);

    // Fix source address for inject-mode sockets
    if result > 0 && WG_ROUTING_ACTIVE.load(Ordering::Relaxed)
        && !src_addr.is_null() && !addrlen.is_null()
    {
        // Quick check: is this an inject-mode socket?
        let fix_info = {
            let inject = WG_INJECT_SOCKETS.lock();
            inject.get(&sockfd).copied()
        };

        if let Some(info) = fix_info {
            // Check if the source is localhost (our injected data)
            let family = (*src_addr).sa_family as i32;
            debug!("wg_recvfrom: fd={}, result={}, family={}, inject_info=(remote={}:{})",
                   sockfd, result, family, info.remote_ip, info.remote_port);
            if family == libc::AF_INET {
                let sin = &mut *(src_addr as *mut libc::sockaddr_in);
                let src_ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                let src_port = u16::from_be(sin.sin_port);
                debug!("wg_recvfrom: fd={}, AF_INET src={}:{}, is_loopback={}",
                       sockfd, src_ip, src_port, src_ip.is_loopback());
                if src_ip.is_loopback() {
                    // Replace with actual WG server address
                    match info.remote_ip {
                        IpAddr::V4(remote_v4) => {
                            sin.sin_addr.s_addr = u32::from(remote_v4).to_be();
                            sin.sin_port = info.remote_port.to_be();
                        }
                        IpAddr::V6(_) => {
                            // IPv6 remote but AF_INET socket - shouldn't happen normally
                            warn!("wg_recvfrom: IPv6 remote with AF_INET socket fd={}", sockfd);
                        }
                    }
                    debug!("wg_recvfrom: fd={}, fixed src to {}:{}",
                           sockfd, info.remote_ip, info.remote_port);
                }
            } else if family == libc::AF_INET6 {
                // Handle IPv4-mapped IPv6 loopback (::ffff:127.0.0.1)
                // This happens when an AF_INET6 dual-stack socket receives
                // our injected IPv4 loopback packet
                let sin6 = &mut *(src_addr as *mut libc::sockaddr_in6);
                let octets = sin6.sin6_addr.s6_addr;
                let is_v4_mapped_loopback =
                    octets[0..10] == [0; 10]
                    && octets[10] == 0xff && octets[11] == 0xff
                    && octets[12] == 127 && octets[13] == 0
                    && octets[14] == 0 && octets[15] == 1;
                let is_v6_loopback = octets == [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1];
                debug!("wg_recvfrom: fd={}, AF_INET6 is_v4_mapped_loopback={}, is_v6_loopback={}", sockfd, is_v4_mapped_loopback, is_v6_loopback);
                if is_v4_mapped_loopback || is_v6_loopback {
                    // Replace with WG server address
                    match info.remote_ip {
                        IpAddr::V4(remote_v4) => {
                            let ip_octets = remote_v4.octets();
                            sin6.sin6_addr.s6_addr = [
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff,
                                ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3],
                            ];
                        }
                        IpAddr::V6(remote_v6) => {
                            sin6.sin6_addr.s6_addr = remote_v6.octets();
                        }
                    }
                    sin6.sin6_port = info.remote_port.to_be();
                    debug!("wg_recvfrom: fd={}, fixed v6 src to {}:{}",
                           sockfd, info.remote_ip, info.remote_port);
                }
            } else {
                debug!("wg_recvfrom: fd={}, unexpected family={}, no fixup", sockfd, family);
            }
        } else if result > 0 {
            // Not an inject socket - log for debugging
            debug!("wg_recvfrom: fd={}, result={}, not inject-mode socket", sockfd, result);
        }
    } else if result < 0 && WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        let errno_val = *libc::__errno();
        debug!("wg_recvfrom: fd={}, error result={}, errno={}", sockfd, result, errno_val);
    }

    result
}

/// Try to deliver UDP data to an inject-mode socket (e.g., ENet).
/// Sends data via loopback to the real socket's local port, so that
/// poll()/select() on the real FD wakes up and recvfrom() receives the data.
///
/// Returns true if data was delivered, false if no inject socket exists for this port.
pub fn try_inject_udp_data(src_port: u16, data: &[u8]) -> bool {
    let local_port = {
        let port_map = WG_INJECT_PORT_MAP.lock();
        match port_map.get(&src_port) {
            Some(&port) => port,
            None => return false,
        }
    };

    // Get or create the inject socket
    let inject_fd = get_or_create_inject_fd();
    if inject_fd < 0 {
        warn!("try_inject_udp_data: failed to create inject socket");
        return false;
    }

    // Send to localhost:local_port (will be received by the real socket)
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
    addr.sin_port = local_port.to_be();

    let result = unsafe {
        libc::sendto(
            inject_fd,
            data.as_ptr() as *const libc::c_void,
            data.len(),
            0,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };

    if result < 0 {
        let err = std::io::Error::last_os_error();
        warn!("try_inject_udp_data: sendto failed for port {}: {} (inject_fd={}, local_port={})",
              local_port, err, inject_fd, local_port);
        false
    } else {
        info!("try_inject_udp_data: delivered {} bytes from server port {} to local port {} (inject_fd={})",
              data.len(), src_port, local_port, inject_fd);
        true
    }
}

/// Get or create the global inject UDP socket (used for loopback data injection)
fn get_or_create_inject_fd() -> i32 {
    let mut fd_guard = WG_INJECT_FD.lock();
    if let Some(fd) = *fd_guard {
        return fd;
    }

    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
    if fd < 0 {
        warn!("Failed to create inject socket: {}", std::io::Error::last_os_error());
        return -1;
    }
    *fd_guard = Some(fd);
    debug!("Created WG inject socket: fd={}", fd);
    fd
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

/// Extract IP address and port from a sockaddr pointer (supports IPv4 and IPv6)
fn extract_addr_from_sockaddr(addr: *const libc::sockaddr) -> Option<(IpAddr, u16)> {
    if addr.is_null() {
        return None;
    }
    unsafe {
        match (*addr).sa_family as i32 {
            libc::AF_INET => {
                let sin = &*(addr as *const libc::sockaddr_in);
                let port = u16::from_be(sin.sin_port);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                Some((IpAddr::V4(ip), port))
            }
            libc::AF_INET6 => {
                let sin6 = &*(addr as *const libc::sockaddr_in6);
                let port = u16::from_be(sin6.sin6_port);
                let octets = sin6.sin6_addr.s6_addr;
                // Check if it's a v4-mapped v6 address (::ffff:x.x.x.x)
                if octets[0..10] == [0; 10] && octets[10] == 0xff && octets[11] == 0xff {
                    let ip = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                    Some((IpAddr::V4(ip), port))
                } else {
                    // Native IPv6 address
                    let ip = std::net::Ipv6Addr::from(octets);
                    Some((IpAddr::V6(ip), port))
                }
            }
            _ => None,
        }
    }
}

/// Extract IP address from sockaddr_storage (supports IPv4 and IPv6)
fn extract_ip_from_sockaddr_storage(addr: *const libc::sockaddr_storage) -> Option<IpAddr> {
    if addr.is_null() {
        return None;
    }
    unsafe {
        match (*addr).ss_family as i32 {
            libc::AF_INET => {
                let sin = &*(addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                Some(IpAddr::V4(ip))
            }
            libc::AF_INET6 => {
                let sin6 = &*(addr as *const libc::sockaddr_in6);
                let octets = sin6.sin6_addr.s6_addr;
                // Check for v4-mapped v6 address
                if octets[0..10] == [0; 10] && octets[10] == 0xff && octets[11] == 0xff {
                    Some(IpAddr::V4(Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15])))
                } else {
                    Some(IpAddr::V6(std::net::Ipv6Addr::from(octets)))
                }
            }
            _ => None,
        }
    }
}

// ============================================================================
// TCP Socket Wrappers (WireGuard-aware)
// ============================================================================

/// WG-aware connectTcpSocket: routes through WireGuard virtual TCP stack when active.
///
/// When WG routing is active and the destination is the WG server IP,
/// creates a TCP connection through the WireGuard tunnel using the virtual stack.
/// Returns a virtual FD (>= WG_TCP_FD_BASE) that can be used with send/recv.
#[no_mangle]
pub unsafe extern "C" fn connectTcpSocket(
    dstaddr: *mut libc::sockaddr_storage,
    addrlen: libc::socklen_t,
    port: libc::c_ushort,
    timeoutSec: libc::c_int,
) -> i32 {
    // Fast path: if WG routing not active, use original
    if !WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        return orig_connectTcpSocket(dstaddr, addrlen, port, timeoutSec);
    }

    // Check if destination is the WG server
    let dest_ip = match extract_ip_from_sockaddr_storage(dstaddr) {
        Some(ip) => ip,
        None => {
            // Unknown address family, use original
            return orig_connectTcpSocket(dstaddr, addrlen, port, timeoutSec);
        }
    };

    let config = WG_CONFIG.lock();
    let is_wg_target = match config.as_ref() {
        Some(cfg) => dest_ip == cfg.server_ip,
        None => false,
    };
    drop(config);

    if !is_wg_target {
        // Not targeting WG server, use original
        return orig_connectTcpSocket(dstaddr, addrlen, port, timeoutSec);
    }

    // Route through WireGuard virtual TCP stack
    info!("connectTcpSocket: routing {}:{} through WireGuard", dest_ip, port);

    let timeout_ms = (timeoutSec as u32) * 1000;
    let host = dest_ip.to_string();
    let handle = crate::wg_socket::wg_socket_connect(&host, port, timeout_ms);

    if handle == 0 {
        error!("connectTcpSocket: WG connection failed to {}:{}", dest_ip, port);
        // Return INVALID_SOCKET (-1)
        return -1;
    }

    // Allocate a virtual FD for this connection
    let virtual_fd = WG_TCP_FD_COUNTER.fetch_add(1, Ordering::Relaxed);

    let info = Arc::new(WgTcpSocketInfo {
        wg_handle: handle,
        is_open: AtomicBool::new(true),
    });

    WG_TCP_SOCKETS.lock().insert(virtual_fd, info);

    info!(
        "connectTcpSocket: WG TCP connection established, virtual_fd={}, handle={}",
        virtual_fd, handle
    );

    virtual_fd
}

/// WG-aware shutdownTcpSocket: properly shuts down WG TCP connection.
#[no_mangle]
pub unsafe extern "C" fn shutdownTcpSocket(s: i32) {
    // Check if this is a WG TCP socket
    if s >= WG_TCP_FD_BASE {
        let tcp_sockets = WG_TCP_SOCKETS.lock();
        if let Some(info) = tcp_sockets.get(&s) {
            info.is_open.store(false, Ordering::Release);
            // Note: actual close happens in closeSocket
            debug!("shutdownTcpSocket: marked WG TCP socket {} for shutdown", s);
        }
        return;
    }

    // Real socket
    orig_shutdownTcpSocket(s);
}

/// WG-aware TCP send: sends data through WireGuard virtual TCP stack.
///
/// This function is called via the `send` macro redirect in wg_intercept.h.
#[no_mangle]
pub unsafe extern "C" fn wg_tcp_send(
    sockfd: libc::c_int,
    buf: *const libc::c_void,
    len: libc::size_t,
    flags: libc::c_int,
) -> libc::ssize_t {
    // Check if this is a WG TCP socket
    if sockfd >= WG_TCP_FD_BASE {
        let tcp_info = {
            let sockets = WG_TCP_SOCKETS.lock();
            sockets.get(&sockfd).cloned()
        };

        if let Some(info) = tcp_info {
            if !info.is_open.load(Ordering::Relaxed) {
                // Socket was shut down
                return -1;
            }

            let data = std::slice::from_raw_parts(buf as *const u8, len);
            let result = crate::wg_socket::wg_socket_send(info.wg_handle, data);

            if result < 0 {
                error!("wg_tcp_send: send failed, handle={}, result={}", info.wg_handle, result);
                return -1;
            }

            return result as libc::ssize_t;
        } else {
            error!("wg_tcp_send: invalid virtual FD {}", sockfd);
            return -1;
        }
    }

    // Regular socket, use libc send
    libc::send(sockfd, buf, len, flags)
}

/// WG-aware TCP recv: receives data from WireGuard virtual TCP stack.
///
/// This function is called via the `recv` macro redirect in wg_intercept.h.
#[no_mangle]
pub unsafe extern "C" fn wg_tcp_recv(
    sockfd: libc::c_int,
    buf: *mut libc::c_void,
    len: libc::size_t,
    flags: libc::c_int,
) -> libc::ssize_t {
    // Check if this is a WG TCP socket
    if sockfd >= WG_TCP_FD_BASE {
        let tcp_info = {
            let sockets = WG_TCP_SOCKETS.lock();
            sockets.get(&sockfd).cloned()
        };

        if let Some(info) = tcp_info {
            if !info.is_open.load(Ordering::Relaxed) {
                // Socket was shut down
                return 0; // EOF
            }

            let buffer = std::slice::from_raw_parts_mut(buf as *mut u8, len);
            // Use a reasonable timeout for recv (e.g., 5 seconds)
            let timeout_ms = 5000u32;
            let result = crate::wg_socket::wg_socket_recv(info.wg_handle, buffer, timeout_ms);

            if result == -2 {
                // Timeout - for blocking recv, we should retry
                // Set errno to EAGAIN and return -1
                *libc::__errno() = libc::EAGAIN;
                return -1;
            } else if result < 0 {
                error!("wg_tcp_recv: recv failed, handle={}, result={}", info.wg_handle, result);
                return -1;
            }

            return result as libc::ssize_t;
        } else {
            error!("wg_tcp_recv: invalid virtual FD {}", sockfd);
            return -1;
        }
    }

    // Regular socket, use libc recv
    libc::recv(sockfd, buf, len, flags)
}

// ============================================================================
// UDP connect interception
// ============================================================================

/// WG-aware UDP connect: intercepts connect() on UDP sockets.
///
/// When a UDP socket calls connect() to the WG server IP:
/// - We skip the real connect() call (which would filter incoming packets by source)
/// - Store the peer address in WG_UDP_CONNECTED_PEERS for use by wg_sendto
///
/// This allows loopback-injected data to be received by the socket.
/// For non-WG destinations, we pass through to the real connect().
#[no_mangle]
pub unsafe extern "C" fn wg_udp_connect(
    sockfd: libc::c_int,
    addr: *const libc::sockaddr,
    addrlen: libc::socklen_t,
) -> libc::c_int {
    // Check if WG routing is active
    if !WG_ROUTING_ACTIVE.load(Ordering::Relaxed) {
        return libc::connect(sockfd, addr, addrlen);
    }

    // Check socket type - only intercept UDP (SOCK_DGRAM)
    let mut sock_type: libc::c_int = 0;
    let mut optlen: libc::socklen_t = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    if libc::getsockopt(sockfd, libc::SOL_SOCKET, libc::SO_TYPE, 
                        &mut sock_type as *mut _ as *mut libc::c_void, &mut optlen) != 0 {
        return libc::connect(sockfd, addr, addrlen);
    }

    if sock_type != libc::SOCK_DGRAM {
        // Not a UDP socket, pass through to real connect
        return libc::connect(sockfd, addr, addrlen);
    }

    // Extract destination address
    let peer_addr = match extract_addr_from_sockaddr(addr) {
        Some((ip, port)) => SocketAddr::new(ip, port),
        None => {
            // Can't extract address, pass through
            return libc::connect(sockfd, addr, addrlen);
        }
    };

    // Check if this is the WG server IP
    let config = WG_CONFIG.lock();
    let server_ip = match config.as_ref() {
        Some(cfg) => cfg.server_ip,
        None => {
            drop(config);
            return libc::connect(sockfd, addr, addrlen);
        }
    };
    drop(config);

    if peer_addr.ip() == server_ip {
        // This is a UDP connect() to the WG server!
        // Store the peer address, skip the real connect()
        WG_UDP_CONNECTED_PEERS.lock().insert(sockfd, peer_addr);
        info!("wg_udp_connect: intercepted connect() to WG server fd={}, peer={}",
              sockfd, peer_addr);
        return 0; // Success, but socket remains unconnected
    }

    // Not WG server, pass through to real connect
    libc::connect(sockfd, addr, addrlen)
}
