//! WireGuard tunnel module using boringtun
//!
//! This module provides a userspace WireGuard tunnel that allows moonlight streaming
//! traffic to be routed through a WireGuard VPN without requiring a system TUN device.
//!
//! Architecture:
//! - Uses boringtun for WireGuard protocol (Noise handshake, encryption/decryption)
//! - Creates a real UDP socket to the WireGuard peer endpoint
//! - Uses zero-copy channel delivery for UDP traffic (via platform_sockets)
//! - Uses VirtualStack for TCP traffic (via wg_http)
//! - All moonlight streaming traffic (video, audio, control) goes through the tunnel
//! - Supports both IPv4 and IPv6 tunnel addresses

use std::cell::RefCell;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boringtun::noise::{Tunn, TunnResult};
use x25519_dalek::{PublicKey, StaticSecret};
use log::{debug, error, info, warn};
use parking_lot::Mutex;

// Re-export configuration from dedicated module
pub use crate::wireguard_config::WireGuardConfig;

/// Maximum size of a UDP packet
const MAX_UDP_PACKET_SIZE: usize = 65535;

/// Buffer size for WireGuard encapsulation overhead
const WG_BUFFER_SIZE: usize = MAX_UDP_PACKET_SIZE + 256;

/// DDNS re-resolution timeout in seconds (same as WireGuard's reresolve-dns.sh)
const DDNS_RERESOLVE_TIMEOUT_SECS: u64 = 135;

/// Return the unspecified bind address matching the address family of `addr`.
/// IPv4 endpoints bind to `0.0.0.0:0`, IPv6 endpoints bind to `[::]:0`.
fn bind_addr_for(addr: &SocketAddr) -> &'static str {
    match addr {
        SocketAddr::V4(_) => "0.0.0.0:0",
        SocketAddr::V6(_) => "[::]:0",
    }
}


/// State of the WireGuard tunnel
struct TunnelState {
    /// The boringtun tunnel instance
    tunnel: Box<Tunn>,
    /// UDP socket connected to the WireGuard endpoint
    endpoint_socket: UdpSocket,
    /// Currently resolved endpoint address
    resolved_endpoint: SocketAddr,
    /// Whether the tunnel is established (handshake completed)
    handshake_completed: AtomicBool,
    /// Last successful handshake/packet timestamp for DDNS re-resolution
    last_handshake: Instant,
    /// Incremented each time endpoint_socket is replaced (e.g. DDNS re-resolution).
    /// Used by the receiver thread and send cache to detect stale socket clones.
    socket_generation: u64,
}

/// The WireGuard tunnel manager
pub struct WireGuardTunnel {
    config: WireGuardConfig,
    state: Arc<Mutex<TunnelState>>,
    running: Arc<AtomicBool>,
}

impl WireGuardTunnel {
    /// Create a new WireGuard tunnel with the given configuration.
    pub fn new(config: WireGuardConfig) -> io::Result<Self> {
        info!("Creating WireGuard tunnel to endpoint: {}", config.endpoint);

        // Create the private/public key pair
        let private_key = StaticSecret::from(config.private_key);
        let peer_public_key = PublicKey::from(config.peer_public_key);

        // Create the boringtun tunnel
        let tunnel = Box::new(Tunn::new(
            private_key,
            peer_public_key,
            config.preshared_key,
            None,
            0, // index
            None, // rate limiter
        ));

        // Resolve endpoint dynamically for DDNS support
        let endpoint_addr = config.resolve_endpoint()?;
        info!("Resolved endpoint '{}' -> {}", config.endpoint, endpoint_addr);

        // Create UDP socket to the WireGuard endpoint (address family must match)
        let endpoint_socket = UdpSocket::bind(bind_addr_for(&endpoint_addr))?;
        endpoint_socket.connect(endpoint_addr)?;
        endpoint_socket.set_nonblocking(false)?;

        // Set large socket buffers for high-throughput streaming
        // Video frames at high bitrate can burst many packets; large buffers prevent kernel drops
        Self::set_socket_buffer_sizes(&endpoint_socket);

        // Set a short read timeout for timer/handshake operations
        // Note: receiver thread clones this socket and sets its own timeout
        endpoint_socket.set_read_timeout(Some(Duration::from_millis(10)))?;

        info!("WireGuard endpoint socket bound to: {}", endpoint_socket.local_addr()?);

        let state = Arc::new(Mutex::new(TunnelState {
            tunnel,
            endpoint_socket,
            resolved_endpoint: endpoint_addr,
            handshake_completed: AtomicBool::new(false),
            last_handshake: Instant::now(),
            socket_generation: 0,
        }));

        let running = Arc::new(AtomicBool::new(false));

        Ok(WireGuardTunnel {
            config,
            state,
            running,
        })
    }

    /// Start the WireGuard tunnel.
    /// This initiates the handshake and starts the background packet processing threads.
    pub fn start(&self) -> io::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::Release);
        info!("Starting WireGuard tunnel...");

        // Initiate the handshake
        self.initiate_handshake()?;

        // Start the endpoint receiver thread - reads from the real WireGuard endpoint
        // and decapsulates packets, forwarding via zero-copy channels
        let state = self.state.clone();
        let running = self.running.clone();

        thread::Builder::new()
            .name("wg-endpoint-rx".into())
            .spawn(move || {
                Self::endpoint_receiver_loop(state, running);
            })?;

        // Start the timer thread for handshake retransmission and DDNS re-resolution
        let state = self.state.clone();
        let running = self.running.clone();
        let config = self.config.clone();

        thread::Builder::new()
            .name("wg-timer".into())
            .spawn(move || {
                Self::timer_loop(state, running, config);
            })?;

        info!("WireGuard tunnel started");
        Ok(())
    }

    /// Stop the WireGuard tunnel.
    pub fn stop(&self) {
        // Only log and act if actually running (avoids double-stop from Drop)
        if self.running.swap(false, Ordering::Release) {
            info!("Stopping WireGuard tunnel...");
            info!("WireGuard tunnel stopped");
        }
    }

    /// Set large send/receive buffer sizes on a UDP socket for streaming throughput.
    /// On Linux/Android, the kernel will cap at net.core.rmem_max / wmem_max.
    fn set_socket_buffer_sizes(socket: &UdpSocket) {
        use std::os::unix::io::AsRawFd;
        let fd = socket.as_raw_fd();
        // 2MB buffers - handles bursty video traffic (I-frames can be very large)
        let buf_size: libc::c_int = 2 * 1024 * 1024;
        let optlen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        unsafe {
            let rc = libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &buf_size as *const _ as *const libc::c_void,
                optlen,
            );
            if rc == 0 {
                let mut actual: libc::c_int = 0;
                let mut actual_len = optlen;
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &mut actual as *mut _ as *mut libc::c_void,
                    &mut actual_len,
                );
                info!("WG endpoint SO_RCVBUF set to {} (requested {})", actual, buf_size);
            }
            let rc = libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &buf_size as *const _ as *const libc::c_void,
                optlen,
            );
            if rc == 0 {
                let mut actual: libc::c_int = 0;
                let mut actual_len = optlen;
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    &mut actual as *mut _ as *mut libc::c_void,
                    &mut actual_len,
                );
                info!("WG endpoint SO_SNDBUF set to {} (requested {})", actual, buf_size);
            }
        }
    }

    /// Check if the tunnel is running and the handshake is completed.
    pub fn is_ready(&self) -> bool {
        self.running.load(Ordering::Relaxed)
            && self.state.lock().handshake_completed.load(Ordering::Acquire)
    }

    /// Wait for the handshake to complete, with a timeout.
    ///
    /// Actively re-initiates the handshake with exponential backoff to handle
    /// packet loss on unreliable networks (mobile, WiFi).
    pub fn wait_for_handshake(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut next_retry = start + Duration::from_millis(1000);
        let mut retry_interval = Duration::from_millis(1000);
        let max_retry_interval = Duration::from_secs(4);
        let mut retry_count = 0u32;

        while start.elapsed() < timeout {
            if self.is_ready() {
                if retry_count > 0 {
                    info!("WireGuard handshake completed after {} retries ({:?})",
                          retry_count, start.elapsed());
                }
                return true;
            }

            // Actively re-initiate handshake on a schedule.
            // This handles the common case where the first handshake initiation
            // packet was lost (UDP is unreliable). Without this, we'd have to
            // wait for boringtun's internal timer (~5s) which is too slow.
            let now = Instant::now();
            if now >= next_retry {
                retry_count += 1;
                info!("Re-initiating WireGuard handshake (attempt {}, {:?} elapsed)",
                      retry_count, start.elapsed());
                if let Err(e) = self.initiate_handshake() {
                    warn!("Handshake re-initiation failed: {}", e);
                }
                retry_interval = (retry_interval * 2).min(max_retry_interval);
                next_retry = now + retry_interval;
            }

            thread::sleep(Duration::from_millis(50));
        }

        warn!("WireGuard handshake timed out after {:?} ({} retries)",
              start.elapsed(), retry_count);
        false
    }

    /// Initiate the WireGuard handshake
    fn initiate_handshake(&self) -> io::Result<()> {
        let mut state = self.state.lock();
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];

        match state.tunnel.format_handshake_initiation(&mut dst_buf, false) {
            TunnResult::WriteToNetwork(data) => {
                info!("Sending WireGuard handshake initiation ({} bytes)", data.len());
                state.endpoint_socket.send(data)?;
            }
            TunnResult::Err(e) => {
                error!("Failed to create handshake initiation: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, format!("Handshake initiation failed: {:?}", e)));
            }
            other => {
                warn!("Unexpected result from handshake initiation: {:?}", format!("{:?}", other).chars().take(50).collect::<String>());
            }
        }

        Ok(())
    }



    /// Background thread: receives packets from the WireGuard endpoint and decapsulates them
    fn endpoint_receiver_loop(
        state: Arc<Mutex<TunnelState>>,
        running: Arc<AtomicBool>,
    ) {
        // CRITICAL PERFORMANCE FIX: Clone socket for receiving so we don't hold
        // the tunnel state lock during blocking recv(). Previously, the lock was
        // held for up to 100ms during recv timeout, blocking ALL send operations
        // (UDP streaming data, TCP ACKs) through the tunnel.
        let (mut recv_socket, mut current_socket_gen) = {
            let st = state.lock();
            let sock = st.endpoint_socket.try_clone()
                .expect("Failed to clone WG endpoint socket for receiver");
            (sock, st.socket_generation)
        };
        // Use short read timeout (10ms) - just enough to check shutdown flag
        recv_socket.set_read_timeout(Some(Duration::from_millis(10))).ok();

        // Pre-allocate buffers once - reused for every packet (zero allocation hot path)
        let mut recv_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut dec_buf = vec![0u8; WG_BUFFER_SIZE];

        info!("WireGuard endpoint receiver started");

        while running.load(Ordering::Relaxed) {
            // Read WITHOUT holding tunnel lock - allows concurrent sends
            let n = match recv_socket.recv(&mut recv_buf) {
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock 
                    || e.kind() == io::ErrorKind::TimedOut 
                    || e.kind() == io::ErrorKind::Interrupted
                    || e.kind() == io::ErrorKind::ConnectionRefused => {
                    // ConnectionRefused on UDP = ICMP port unreachable, just retry.
                    // Also check if the socket was replaced (DDNS re-resolution)
                    // so we start reading from the new socket.
                    let st = state.lock();
                    if st.socket_generation != current_socket_gen {
                        info!("WG receiver: socket replaced (gen {} -> {}), re-cloning",
                              current_socket_gen, st.socket_generation);
                        match st.endpoint_socket.try_clone() {
                            Ok(new_sock) => {
                                drop(st);
                                new_sock.set_read_timeout(Some(Duration::from_millis(10))).ok();
                                recv_socket = new_sock;
                                current_socket_gen = state.lock().socket_generation;
                            }
                            Err(e2) => {
                                warn!("WG receiver: failed to re-clone socket: {}", e2);
                            }
                        }
                    }
                    continue;
                }
                Err(e) => {
                    if running.load(Ordering::Relaxed) {
                        warn!("WireGuard endpoint recv error: {}", e);
                    }
                    continue;
                }
            };

            // Lock briefly for decapsulate only (fast crypto operation, ~microseconds)
            let mut st = state.lock();

            // Update last handshake time on any received packet
            st.last_handshake = Instant::now();

            let result = st.tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf);

            match result {
                TunnResult::WriteToNetwork(data) => {
                    // This is typically a handshake response
                    if let Err(e) = st.endpoint_socket.send(data) {
                        error!("Failed to send WireGuard response: {}", e);
                    }

                    // Check if there's more data to process (for handshake completion)
                    // After sending the response, try to get decapsulated data
                    let result2 = st.tunnel.decapsulate(None, &[], &mut dec_buf);
                    match result2 {
                        TunnResult::WriteToNetwork(data2) => {
                            if let Err(e) = st.endpoint_socket.send(data2) {
                                error!("Failed to send WireGuard followup: {}", e);
                            }
                            // Handshake likely completed
                            if !st.handshake_completed.load(Ordering::Relaxed) {
                                st.handshake_completed.store(true, Ordering::Release);
                                info!("WireGuard handshake completed!");
                            }
                        }
                        TunnResult::Done => {
                            if !st.handshake_completed.load(Ordering::Relaxed) {
                                st.handshake_completed.store(true, Ordering::Release);
                                info!("WireGuard handshake completed!");
                            }
                        }
                        _ => {}
                    }
                }
                TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                    // Decapsulated IP packet - extract and forward to the right proxy
                    if !st.handshake_completed.load(Ordering::Relaxed) {
                        st.handshake_completed.store(true, Ordering::Release);
                        info!("WireGuard handshake completed (first data packet)!");
                    }
                    drop(st); // Release lock before forwarding

                    // Determine IP version and extract protocol
                    if data.len() >= 20 {
                        let ip_version = (data[0] >> 4) & 0x0F;
                        let protocol = match ip_version {
                            4 => data[9],     // IPv4: protocol at offset 9
                            6 if data.len() >= 40 => data[6], // IPv6: next header at offset 6
                            _ => continue,
                        };

                        if protocol == 6 {
                            // TCP packet - forward to HTTP shared proxy's virtual stack
                            crate::wg_http::wg_http_inject_packet(data);
                        } else if protocol == 17 {
                            // UDP packet - deliver via zero-copy channel
                            if let Some((src_port, _dst_port, payload)) = parse_udp_from_ip_packet(data) {
                                // Try zero-copy delivery via platform_sockets channel
                                if crate::platform_sockets::try_push_udp_data(src_port, payload) {
                                    debug!("WG UDP: delivered via zero-copy channel (src_port={})", src_port);
                                } else if crate::platform_sockets::try_inject_udp_data(src_port, payload) {
                                    debug!("WG UDP: delivered via loopback injection (src_port={})", src_port);
                                } else {
                                    debug!("WG UDP: no channel found for src_port={}", src_port);
                                }
                            }
                        }
                    }
                }
                TunnResult::Done => {
                    // Nothing to forward
                }
                TunnResult::Err(e) => {
                    warn!("WireGuard decapsulation error: {:?}", e);
                }
            }
        }

        info!("WireGuard endpoint receiver stopped");
    }

    /// Background thread: periodic timer for DDNS re-resolution and handshake maintenance
    fn timer_loop(state: Arc<Mutex<TunnelState>>, running: Arc<AtomicBool>, config: WireGuardConfig) {
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut handshake_retry_count = 0u32;

        info!("WireGuard timer thread started");

        while running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(250));

            // Track whether we need to update the send cache after releasing the state lock.
            // This avoids a lock ordering deadlock: send path holds WG_SEND_CACHE then state,
            // so we must NOT hold state while locking WG_SEND_CACHE.
            let mut new_send_socket: Option<UdpSocket> = None;

            {
                let mut st = state.lock();

                // Check for DDNS re-resolution (same as WireGuard's reresolve-dns.sh)
                // If no successful packet in DDNS_RERESOLVE_TIMEOUT_SECS, re-resolve DNS
                let last_handshake_elapsed = st.last_handshake.elapsed();
                if last_handshake_elapsed > Duration::from_secs(DDNS_RERESOLVE_TIMEOUT_SECS) {
                    info!("DDNS: no handshake for {} seconds, re-resolving endpoint",
                          last_handshake_elapsed.as_secs());

                    match config.resolve_endpoint() {
                        Ok(new_addr) => {
                            if new_addr != st.resolved_endpoint {
                                info!("DDNS re-resolution: endpoint '{}' changed {} -> {}",
                                      config.endpoint, st.resolved_endpoint, new_addr);

                                // Create new socket and connect to new address (address family must match)
                                match UdpSocket::bind(bind_addr_for(&new_addr)) {
                                    Ok(new_socket) => {
                                        if let Err(e) = new_socket.connect(new_addr) {
                                            warn!("DDNS: failed to connect to new endpoint: {}", e);
                                        } else {
                                            new_socket.set_nonblocking(false).ok();
                                            new_socket.set_read_timeout(Some(Duration::from_millis(10))).ok();
                                            Self::set_socket_buffer_sizes(&new_socket);

                                            // Clone for send cache update (before moving into state)
                                            new_send_socket = new_socket.try_clone().ok();

                                            // Replace socket and address
                                            st.endpoint_socket = new_socket;
                                            st.resolved_endpoint = new_addr;
                                            // Bump generation so receiver thread re-clones
                                            st.socket_generation += 1;

                                            info!("DDNS: reconnected to new endpoint {} (socket gen={})",
                                                  new_addr, st.socket_generation);

                                            // Reset handshake state and retry count
                                            st.handshake_completed.store(false, Ordering::Release);
                                            handshake_retry_count = 0;
                                        }
                                    }
                                    Err(e) => {
                                        warn!("DDNS: failed to create new socket: {}", e);
                                    }
                                }
                            } else {
                                debug!("DDNS re-resolution: endpoint '{}' unchanged ({})",
                                       config.endpoint, new_addr);
                            }

                            // Update last handshake time to prevent immediate re-resolution loop
                            st.last_handshake = Instant::now();

                            // Initiate new handshake
                            match st.tunnel.format_handshake_initiation(&mut dst_buf, false) {
                                TunnResult::WriteToNetwork(data) => {
                                    if let Err(e) = st.endpoint_socket.send(data) {
                                        warn!("DDNS: failed to send handshake: {}", e);
                                    } else {
                                        info!("DDNS: initiated handshake after re-resolution");
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            warn!("DDNS re-resolution failed: {}", e);
                            // Still update timestamp to prevent rapid retry loop
                            st.last_handshake = Instant::now();
                        }
                    }
                }

                // Process all timer events in a loop (there may be multiple)
                loop {
                    match st.tunnel.update_timers(&mut dst_buf) {
                        TunnResult::WriteToNetwork(data) => {
                            if let Err(e) = st.endpoint_socket.send(data) {
                                // EPERM (os error 1) is common on Android when network state changes
                                // Only log non-EPERM errors to reduce log spam
                                if e.raw_os_error() != Some(1) {
                                    debug!("Failed to send timer packet: {}", e);
                                }
                            }
                        }
                        TunnResult::Err(e) => {
                            warn!("WireGuard timer error: {:?}", e);

                            // Check if this is a connection expired error
                            let error_str = format!("{:?}", e);
                            if error_str.contains("ConnectionExpired") {
                                handshake_retry_count += 1;
                                warn!("Connection expired, re-initiating handshake (attempt {})",
                                      handshake_retry_count);

                                // Mark handshake as not completed
                                st.handshake_completed.store(false, Ordering::Release);

                                // Always retry - WireGuard connections can recover after
                                // network changes, temporary outages, or NAT rebinding.
                                // A hard cap would permanently kill the tunnel.
                                match st.tunnel.format_handshake_initiation(&mut dst_buf, false) {
                                    TunnResult::WriteToNetwork(data) => {
                                        if let Err(e) = st.endpoint_socket.send(data) {
                                            warn!("Failed to send handshake re-initiation: {}", e);
                                        } else {
                                            info!("Sent handshake re-initiation");
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            break;
                        }
                        TunnResult::Done => break,
                        _ => break,
                    }
                }

                // Reset retry count if handshake is completed
                if st.handshake_completed.load(Ordering::Acquire) {
                    handshake_retry_count = 0;
                }
            } // state lock released here

            // Update send cache OUTSIDE the state lock to avoid deadlock.
            // Lock ordering: send path holds WG_SEND_CACHE -> state,
            // so we must NOT hold state -> WG_SEND_CACHE.
            if let Some(new_sock) = new_send_socket {
                let mut cache = WG_SEND_CACHE.lock();
                if let Some(ref mut c) = *cache {
                    c.send_socket = new_sock;
                    info!("DDNS: updated send cache with new socket");
                }
            }
        }

        info!("WireGuard timer thread stopped");
    }
}

impl Drop for WireGuardTunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

// ============================================================================
// IP/UDP packet construction helpers (IPv4 + IPv6, zero-alloc variants)
// ============================================================================

/// Build an IPv4 or IPv6 UDP packet into the provided buffer.
/// Returns the number of bytes written. Zero-allocation hot path.
pub fn build_udp_ip_packet_into(buf: &mut [u8], src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> usize {
    match (src.ip(), dst.ip()) {
        (IpAddr::V4(src_ip), IpAddr::V4(dst_ip)) => {
            build_udp_ipv4_packet_into(buf, src_ip, src.port(), dst_ip, dst.port(), payload)
        }
        (IpAddr::V6(src_ip), IpAddr::V6(dst_ip)) => {
            build_udp_ipv6_packet_into(buf, src_ip, src.port(), dst_ip, dst.port(), payload)
        }
        _ => 0, // Mismatched address families
    }
}

/// Build an IPv4/UDP packet into buf. Returns total bytes written.
fn build_udp_ipv4_packet_into(
    buf: &mut [u8],
    src_ip: Ipv4Addr, src_port: u16,
    dst_ip: Ipv4Addr, dst_port: u16,
    payload: &[u8],
) -> usize {
    let udp_len = 8 + payload.len();
    let total_len = 20 + udp_len;
    if buf.len() < total_len {
        return 0;
    }

    // IPv4 header (20 bytes)
    buf[0] = 0x45; // Version (4) + IHL (5)
    buf[1] = 0x00; // DSCP + ECN
    buf[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    buf[4..6].copy_from_slice(&[0x00, 0x00]); // Identification
    buf[6..8].copy_from_slice(&[0x40, 0x00]); // Flags (DF)
    buf[8] = 64; // TTL
    buf[9] = 17; // Protocol (UDP)
    buf[10..12].copy_from_slice(&[0x00, 0x00]); // Checksum placeholder
    buf[12..16].copy_from_slice(&src_ip.octets());
    buf[16..20].copy_from_slice(&dst_ip.octets());

    // Calculate IP header checksum
    let checksum = ip_checksum(&buf[..20]);
    buf[10] = (checksum >> 8) as u8;
    buf[11] = (checksum & 0xFF) as u8;

    // UDP header (8 bytes)
    buf[20..22].copy_from_slice(&src_port.to_be_bytes());
    buf[22..24].copy_from_slice(&dst_port.to_be_bytes());
    buf[24..26].copy_from_slice(&(udp_len as u16).to_be_bytes());
    buf[26..28].copy_from_slice(&[0x00, 0x00]); // UDP checksum (optional for IPv4)

    // Payload
    buf[28..28 + payload.len()].copy_from_slice(payload);

    total_len
}

/// Build an IPv6/UDP packet into buf. Returns total bytes written.
fn build_udp_ipv6_packet_into(
    buf: &mut [u8],
    src_ip: Ipv6Addr, src_port: u16,
    dst_ip: Ipv6Addr, dst_port: u16,
    payload: &[u8],
) -> usize {
    let udp_len = 8 + payload.len();
    let total_len = 40 + udp_len; // IPv6 header (40) + UDP
    if buf.len() < total_len {
        return 0;
    }

    // IPv6 header (40 bytes)
    buf[0] = 0x60; // Version (6) + Traffic Class high nibble
    buf[1] = 0x00; // Traffic Class low nibble + Flow Label high
    buf[2..4].copy_from_slice(&[0x00, 0x00]); // Flow Label low
    buf[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes()); // Payload length
    buf[6] = 17; // Next Header (UDP)
    buf[7] = 64; // Hop Limit
    buf[8..24].copy_from_slice(&src_ip.octets()); // Source
    buf[24..40].copy_from_slice(&dst_ip.octets()); // Destination

    // UDP header (8 bytes) at offset 40
    let udp_off = 40;
    buf[udp_off..udp_off + 2].copy_from_slice(&src_port.to_be_bytes());
    buf[udp_off + 2..udp_off + 4].copy_from_slice(&dst_port.to_be_bytes());
    buf[udp_off + 4..udp_off + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    buf[udp_off + 6..udp_off + 8].copy_from_slice(&[0x00, 0x00]); // Checksum placeholder

    // UDP checksum is mandatory for IPv6 - compute it
    let cksum = udp_checksum_ipv6(&src_ip, &dst_ip, src_port, dst_port, payload);
    buf[udp_off + 6] = (cksum >> 8) as u8;
    buf[udp_off + 7] = (cksum & 0xFF) as u8;

    // Payload
    buf[udp_off + 8..udp_off + 8 + payload.len()].copy_from_slice(payload);

    total_len
}

/// Allocating version for callers that need a Vec (backward compat)
pub fn build_udp_ip_packet(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let max_len = 40 + 8 + payload.len(); // IPv6 header max
    let mut buf = vec![0u8; max_len];
    let len = build_udp_ip_packet_into(&mut buf, src, dst, payload);
    buf.truncate(len);
    buf
}

/// Calculate an IPv4 header checksum
fn ip_checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < header.len() {
        if i == 10 {
            i += 2;
            continue;
        }
        let word = if i + 1 < header.len() {
            ((header[i] as u32) << 8) | (header[i + 1] as u32)
        } else {
            (header[i] as u32) << 8
        };
        sum += word;
        i += 2;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

/// Calculate UDP checksum for IPv6 (mandatory per RFC 2460)
fn udp_checksum_ipv6(src: &Ipv6Addr, dst: &Ipv6Addr, src_port: u16, dst_port: u16, payload: &[u8]) -> u16 {
    let udp_len = (8 + payload.len()) as u32;
    let mut sum: u32 = 0;

    // Pseudo-header: src addr (16 bytes)
    for chunk in src.octets().chunks(2) {
        sum += ((chunk[0] as u32) << 8) | (chunk[1] as u32);
    }
    // Pseudo-header: dst addr (16 bytes)
    for chunk in dst.octets().chunks(2) {
        sum += ((chunk[0] as u32) << 8) | (chunk[1] as u32);
    }
    // Pseudo-header: UDP length (4 bytes) + next header = 17 (4 bytes)
    sum += (udp_len >> 16) & 0xFFFF;
    sum += udp_len & 0xFFFF;
    sum += 17; // next header = UDP

    // UDP header
    sum += src_port as u32;
    sum += dst_port as u32;
    sum += udp_len & 0xFFFF;
    // checksum field = 0

    // Payload
    let mut i = 0;
    while i + 1 < payload.len() {
        sum += ((payload[i] as u32) << 8) | (payload[i + 1] as u32);
        i += 2;
    }
    if i < payload.len() {
        sum += (payload[i] as u32) << 8;
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let result = !sum as u16;
    if result == 0 { 0xFFFF } else { result } // 0 means no checksum in UDP; use 0xFFFF instead
}

/// Parse source port, destination port, and payload from an IPv4 or IPv6 UDP packet
fn parse_udp_from_ip_packet(packet: &[u8]) -> Option<(u16, u16, &[u8])> {
    if packet.is_empty() {
        return None;
    }

    let version = (packet[0] >> 4) & 0x0F;
    match version {
        4 => parse_udp_from_ipv4(packet),
        6 => parse_udp_from_ipv6(packet),
        _ => None,
    }
}

fn parse_udp_from_ipv4(packet: &[u8]) -> Option<(u16, u16, &[u8])> {
    if packet.len() < 28 {
        return None;
    }
    let ihl = (packet[0] & 0x0F) as usize * 4;
    if packet[9] != 17 || packet.len() < ihl + 8 {
        return None;
    }
    let udp = &packet[ihl..];
    let src_port = u16::from_be_bytes([udp[0], udp[1]]);
    let dst_port = u16::from_be_bytes([udp[2], udp[3]]);
    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || ihl + udp_len > packet.len() {
        return None;
    }
    Some((src_port, dst_port, &udp[8..udp_len]))
}

fn parse_udp_from_ipv6(packet: &[u8]) -> Option<(u16, u16, &[u8])> {
    if packet.len() < 48 { // 40 (IPv6) + 8 (UDP min)
        return None;
    }
    // Next Header at offset 6
    if packet[6] != 17 {
        return None; // Not UDP (extension headers not supported for now)
    }
    let udp = &packet[40..];
    let src_port = u16::from_be_bytes([udp[0], udp[1]]);
    let dst_port = u16::from_be_bytes([udp[2], udp[3]]);
    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || 40 + udp_len > packet.len() {
        return None;
    }
    Some((src_port, dst_port, &udp[8..udp_len]))
}

// ============================================================================
// Global WireGuard tunnel instance + performance-optimized send cache
// ============================================================================

static GLOBAL_TUNNEL: Mutex<Option<WireGuardTunnel>> = Mutex::new(None);

/// Cached state for hot-path packet sending.
/// Avoids double-lock on GLOBAL_TUNNEL and per-packet socket dup() syscall.
struct WgSendCache {
    state: Arc<Mutex<TunnelState>>,
    send_socket: UdpSocket, // pre-cloned once
}
static WG_SEND_CACHE: Mutex<Option<WgSendCache>> = Mutex::new(None);

// Thread-local encode buffer to avoid per-packet heap allocation (~65KB).
thread_local! {
    static ENCODE_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; WG_BUFFER_SIZE]);
}

/// Initialize and start the global WireGuard tunnel
pub fn wg_start_tunnel(config: WireGuardConfig) -> io::Result<()> {
    let mut global = GLOBAL_TUNNEL.lock();
    
    // Stop any existing tunnel
    if let Some(ref tunnel) = *global {
        tunnel.stop();
    }
    // Clear send cache
    *WG_SEND_CACHE.lock() = None;

    let tunnel = WireGuardTunnel::new(config)?;
    tunnel.start()?;
    
    // Wait for handshake with active retry (timeout allows ~4 retry attempts with backoff)
    if !tunnel.wait_for_handshake(Duration::from_secs(15)) {
        tunnel.stop();
        return Err(io::Error::new(io::ErrorKind::TimedOut, "WireGuard handshake timed out"));
    }

    // Populate send cache for hot-path
    {
        let state_arc = tunnel.state.clone();
        let send_socket = {
            let st = state_arc.lock();
            st.endpoint_socket.try_clone()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Socket clone for cache: {}", e)))?
        };
        *WG_SEND_CACHE.lock() = Some(WgSendCache {
            state: state_arc,
            send_socket,
        });
    }

    *global = Some(tunnel);
    Ok(())
}

/// Stop the global WireGuard tunnel
pub fn wg_stop_tunnel() {
    // Disable zero-copy routing before stopping the tunnel
    crate::platform_sockets::disable_wg_routing();

    // Clear send cache first
    *WG_SEND_CACHE.lock() = None;

    let mut global = GLOBAL_TUNNEL.lock();
    if let Some(ref tunnel) = *global {
        tunnel.stop();
    }
    *global = None;
}

/// Check if the WireGuard tunnel is active and ready
pub fn wg_is_tunnel_active() -> bool {
    let global = GLOBAL_TUNNEL.lock();
    global.as_ref().map_or(false, |t| t.is_ready())
}

/// Send an IP packet through the global WireGuard tunnel (hot path).
///
/// Performance: Uses cached `Arc<Mutex<TunnelState>>` and pre-cloned socket
/// to avoid double-lock and per-packet `dup()` syscall. Uses thread-local
/// encode buffer to avoid per-packet 65KB heap allocation.
pub fn wg_send_ip_packet(packet: &[u8]) -> io::Result<()> {
    let cache = WG_SEND_CACHE.lock();
    let c = cache.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")
    })?;

    ENCODE_BUF.with(|buf_cell| {
        let mut buf = buf_cell.borrow_mut();
        // Encapsulate under tunnel state lock (fast crypto, ~microseconds)
        // then send directly from the buffer - zero allocation hot path.
        // The encrypted `data` slice borrows `buf` (not the lock), so we can
        // send while still in the match arm without copying.
        let mut st = c.state.lock();
        match st.tunnel.encapsulate(packet, &mut buf) {
            TunnResult::WriteToNetwork(data) => {
                // Send directly from encode buffer - eliminates to_vec() heap allocation
                // Holding the tunnel lock during send() is acceptable: send() on a
                // connected UDP socket is a fast non-blocking syscall (~1µs), much
                // cheaper than a 1-64KB heap allocation + memcpy.
                let result = c.send_socket.send(data);
                drop(st);
                result.map(|_| ())
            }
            TunnResult::Done => {
                // encapsulate() returned Done — the tunnel has no active session keys
                // (e.g., right after handshake completion before timers flush, or
                // during a re-key transition). Flush pending timer events to advance
                // the tunnel state machine, then retry once.
                debug!("encapsulate returned Done, flushing timers and retrying");
                loop {
                    match st.tunnel.update_timers(&mut buf) {
                        TunnResult::WriteToNetwork(data) => {
                            c.send_socket.send(data).ok();
                        }
                        _ => break,
                    }
                }
                // Retry encapsulate after timer flush
                match st.tunnel.encapsulate(packet, &mut buf) {
                    TunnResult::WriteToNetwork(data) => {
                        let result = c.send_socket.send(data);
                        drop(st);
                        result.map(|_| ())
                    }
                    _ => {
                        drop(st);
                        warn!("encapsulate returned Done after timer flush — packet dropped");
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            "WireGuard tunnel not ready (no session keys)",
                        ))
                    }
                }
            }
            TunnResult::Err(e) => {
                drop(st);
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Encapsulate error: {:?}", e),
                ))
            }
            _ => {
                drop(st);
                Ok(())
            }
        }
    })
}

/// Batch-send multiple IP packets through the WireGuard tunnel.
/// Single lock acquisition for all packets, minimizing lock contention.
pub fn wg_send_ip_packets_batch(packets: &[Vec<u8>]) -> io::Result<()> {
    if packets.is_empty() {
        return Ok(());
    }

    let cache = WG_SEND_CACHE.lock();
    let c = cache.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")
    })?;

    ENCODE_BUF.with(|buf_cell| {
        let mut buf = buf_cell.borrow_mut();
        // Encrypt and send each packet under a single lock acquisition.
        // Sending directly from the encode buffer avoids per-packet to_vec() allocation.
        let mut st = c.state.lock();
        let mut timer_flushed = false;
        for pkt in packets {
            match st.tunnel.encapsulate(pkt, &mut buf) {
                TunnResult::WriteToNetwork(data) => {
                    if let Err(e) = c.send_socket.send(data) {
                        warn!("Batch send error: {}", e);
                    }
                }
                TunnResult::Done => {
                    // Flush timers once per batch to advance tunnel state,
                    // then retry this packet.
                    if !timer_flushed {
                        timer_flushed = true;
                        loop {
                            match st.tunnel.update_timers(&mut buf) {
                                TunnResult::WriteToNetwork(data) => {
                                    c.send_socket.send(data).ok();
                                }
                                _ => break,
                            }
                        }
                        // Retry after timer flush
                        match st.tunnel.encapsulate(pkt, &mut buf) {
                            TunnResult::WriteToNetwork(data) => {
                                if let Err(e) = c.send_socket.send(data) {
                                    warn!("Batch send error (retry): {}", e);
                                }
                            }
                            _ => {
                                warn!("Batch encapsulate: packet dropped (no session keys)");
                            }
                        }
                    } else {
                        warn!("Batch encapsulate: packet dropped (no session keys)");
                    }
                }
                TunnResult::Err(e) => {
                    warn!("Batch encapsulate error: {:?}", e);
                }
                _ => {}
            }
        }
        drop(st);
        Ok(())
    })
}

/// Rebind the WireGuard endpoint socket.
///
/// When the network changes (e.g., WiFi → mobile or vice versa), the existing
/// UDP socket may be bound to an interface that is no longer available.
/// This function creates a new socket, connects it to the same endpoint,
/// and replaces the old socket so the tunnel can continue operating on the
/// new network path.  A fresh handshake is initiated automatically.
pub fn wg_rebind_endpoint() -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    let tunnel = global.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")
    })?;

    if !tunnel.running.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not running"));
    }

    // Build the new socket under the state lock, then update the send cache outside it.
    let new_send_socket: UdpSocket;
    {
        let mut st = tunnel.state.lock();
        let endpoint_addr = st.resolved_endpoint;

        info!("Rebinding WireGuard endpoint socket to {} (network change)", endpoint_addr);

        let new_socket = UdpSocket::bind(bind_addr_for(&endpoint_addr))?;
        new_socket.connect(endpoint_addr)?;
        new_socket.set_nonblocking(false)?;
        new_socket.set_read_timeout(Some(Duration::from_millis(10)))?;
        WireGuardTunnel::set_socket_buffer_sizes(&new_socket);

        // Clone for send cache update (before moving into state)
        new_send_socket = new_socket.try_clone()?;

        // Replace socket in tunnel state
        st.endpoint_socket = new_socket;
        st.socket_generation += 1;

        // Re-initiate handshake on the new socket
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];
        match st.tunnel.format_handshake_initiation(&mut dst_buf, false) {
            TunnResult::WriteToNetwork(data) => {
                if let Err(e) = st.endpoint_socket.send(data) {
                    warn!("Rebind: failed to send handshake initiation: {}", e);
                } else {
                    info!("Rebind: sent handshake initiation on new socket (gen={})", st.socket_generation);
                }
            }
            _ => {}
        }

        // Reset last_handshake so the timer thread doesn't immediately try DDNS re-resolution
        st.last_handshake = Instant::now();
    }

    // Update send cache OUTSIDE the state lock to avoid deadlock
    {
        let mut cache = WG_SEND_CACHE.lock();
        if let Some(ref mut c) = *cache {
            c.send_socket = new_send_socket;
            info!("Rebind: updated send cache with new socket");
        }
    }

    info!("WireGuard endpoint socket rebound successfully");
    Ok(())
}

/// Enable direct WireGuard routing for UDP/TCP traffic.
pub fn wg_enable_direct_routing(server_ip: Ipv4Addr) -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => {
            let tunnel_ip = match tunnel.config.tunnel_address {
                IpAddr::V4(ip) => ip,
                IpAddr::V6(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "IPv6 tunnel address not yet supported for direct routing",
                    ));
                }
            };
            crate::platform_sockets::enable_wg_routing(tunnel_ip, server_ip);
            info!("Direct WireGuard routing enabled: tunnel_ip={}, server_ip={}", tunnel_ip, server_ip);
            Ok(())
        }
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_checksum() {
        let header: [u8; 20] = [
            0x45, 0x00, 0x00, 0x3c, 0x1c, 0x46, 0x40, 0x00,
            0x40, 0x06, 0x00, 0x00, 0xac, 0x10, 0x0a, 0x63,
            0xac, 0x10, 0x0a, 0x0c,
        ];
        let cksum = ip_checksum(&header);
        assert_ne!(cksum, 0);
    }

    #[test]
    fn test_build_parse_udp_ipv4_packet() {
        let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 12345);
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 47998);
        let payload = b"hello wireguard";

        let packet = build_udp_ip_packet(src, dst, payload);
        let parsed = parse_udp_from_ip_packet(&packet);

        assert!(parsed.is_some());
        let (src_port, dst_port, data) = parsed.unwrap();
        assert_eq!(src_port, 12345);
        assert_eq!(dst_port, 47998);
        assert_eq!(data, payload);
    }

    #[test]
    fn test_build_parse_udp_ipv6_packet() {
        let src = SocketAddr::new(
            IpAddr::V6("fd00::2".parse().unwrap()), 12345,
        );
        let dst = SocketAddr::new(
            IpAddr::V6("fd00::1".parse().unwrap()), 47998,
        );
        let payload = b"hello ipv6 wireguard";

        let packet = build_udp_ip_packet(src, dst, payload);
        assert!(!packet.is_empty());
        let parsed = parse_udp_from_ip_packet(&packet);
        assert!(parsed.is_some());
        let (src_port, dst_port, data) = parsed.unwrap();
        assert_eq!(src_port, 12345);
        assert_eq!(dst_port, 47998);
        assert_eq!(data, payload);
    }

    #[test]
    fn test_build_udp_ip_packet_into_zero_alloc() {
        let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 5000);
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 6000);
        let payload = b"test";
        let mut buf = [0u8; 256];
        let len = build_udp_ip_packet_into(&mut buf, src, dst, payload);
        assert_eq!(len, 20 + 8 + 4);
        let parsed = parse_udp_from_ip_packet(&buf[..len]);
        assert!(parsed.is_some());
        let (sp, dp, d) = parsed.unwrap();
        assert_eq!(sp, 5000);
        assert_eq!(dp, 6000);
        assert_eq!(d, payload);
    }
}
