//! WireGuard TCP proxy infrastructure
//!
//! This module provides:
//! - WgHttpConfig for configuring WireGuard tunnels
//! - SharedTcpProxy for routing TCP connections through WireGuard
//! - Global configuration management (GLOBAL_HTTP_CONFIG)
//!
//! HTTP requests go through OkHttp + WgSocket -> wg_socket.rs -> SharedTcpProxy

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use parking_lot::Mutex;

use boringtun::noise::{Tunn, TunnResult};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::tun_stack::VirtualStack;

/// Maximum packet size for WireGuard
const MAX_PACKET_SIZE: usize = 65535;

/// WireGuard tunnel configuration
#[derive(Clone)]
pub struct WgHttpConfig {
    pub private_key: [u8; 32],
    pub peer_public_key: [u8; 32],
    pub preshared_key: Option<[u8; 32]>,
    /// Endpoint as "host:port" string - resolved dynamically on each connection for DDNS support
    pub endpoint: String,
    pub tunnel_ip: IpAddr,
    pub server_ip: IpAddr,
    pub mtu: u16,
}

/// Resolve endpoint string to a list of SocketAddrs (supports both IP:port and hostname:port).
/// Returns all resolved addresses with IPv6 addresses first (preferred).
fn resolve_endpoint_all(endpoint: &str) -> io::Result<Vec<SocketAddr>> {
    use std::net::ToSocketAddrs;

    let mut addrs: Vec<SocketAddr> = endpoint.to_socket_addrs()
        .map_err(|e| io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Failed to resolve endpoint '{}': {}", endpoint, e)
        ))?
        .collect();

    if addrs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("DNS resolution returned no addresses for '{}'", endpoint)
        ));
    }

    // Sort addresses: IPv6 first, then IPv4
    addrs.sort_by_key(|addr| match addr {
        SocketAddr::V6(_) => 0,
        SocketAddr::V4(_) => 1,
    });

    Ok(addrs)
}

/// Resolve endpoint string to a single SocketAddr, preferring addresses whose
/// address family is supported by the OS (tries to bind a UDP socket to verify).
fn resolve_endpoint(endpoint: &str) -> io::Result<SocketAddr> {
    let addrs = resolve_endpoint_all(endpoint)?;

    // Try each resolved address: pick the first one where we can actually bind a socket
    for addr in &addrs {
        match UdpSocket::bind(bind_addr_for(addr)) {
            Ok(_) => return Ok(*addr),
            Err(e) => {
                info!("Skipping resolved address {} for '{}': {}", addr, endpoint, e);
            }
        }
    }

    // Fallback: return the first address even though binding failed (caller will get the error)
    Ok(addrs[0])
}

/// Return the unspecified bind address matching the address family of `addr`.
fn bind_addr_for(addr: &SocketAddr) -> &'static str {
    match addr {
        SocketAddr::V4(_) => "0.0.0.0:0",
        SocketAddr::V6(_) => "[::]:0",
    }
}

/// Create a WireGuard tunnel.
/// Tries all resolved addresses for the endpoint, falling back to the next
/// address if binding/connecting fails (e.g. IPv6 not supported on device).
fn create_tunnel(config: &WgHttpConfig) -> io::Result<(Box<Tunn>, UdpSocket, SocketAddr)> {
    let private_key = StaticSecret::from(config.private_key);
    let peer_public_key = PublicKey::from(config.peer_public_key);

    let tunnel = Box::new(Tunn::new(
        private_key,
        peer_public_key,
        config.preshared_key,
        None,
        0,
        None,
    ));

    // Resolve endpoint dynamically for DDNS support - get all addresses
    let addrs = resolve_endpoint_all(&config.endpoint)?;
    info!("Resolved endpoint '{}' -> {:?}", config.endpoint, addrs);

    // Try each resolved address until one works
    let mut last_err = None;
    for addr in &addrs {
        match UdpSocket::bind(bind_addr_for(addr)) {
            Ok(socket) => {
                match socket.connect(addr) {
                    Ok(()) => {
                        info!("Connected to endpoint {} (from {} candidates)", addr, addrs.len());
                        return Ok((tunnel, socket, *addr));
                    }
                    Err(e) => {
                        info!("Failed to connect to {}: {}, trying next", addr, e);
                        last_err = Some(e);
                    }
                }
            }
            Err(e) => {
                info!("Failed to bind for {}: {}, trying next", addr, e);
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| io::Error::new(
        io::ErrorKind::AddrNotAvailable,
        format!("Could not connect to any resolved address for '{}'", config.endpoint)
    )))
}

/// Perform WireGuard handshake with proper continuation and logging
fn do_handshake(tunnel: &mut Tunn, socket: &UdpSocket) -> io::Result<()> {
    let mut buf = vec![0u8; MAX_PACKET_SIZE];

    // Initiate handshake with retry for connection refused
    let mut init_retries = 0;
    const MAX_INIT_RETRIES: i32 = 3;
    
    loop {
        match tunnel.format_handshake_initiation(&mut buf, false) {
            TunnResult::WriteToNetwork(data) => {
                match socket.send(data) {
                    Ok(_) => break,
                    Err(ref e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                        init_retries += 1;
                        if init_retries >= MAX_INIT_RETRIES {
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionRefused,
                                "WireGuard endpoint not reachable (connection refused after retries)",
                            ));
                        }
                        warn!("WG handshake init: connection refused, retry {}/{}", init_retries, MAX_INIT_RETRIES);
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            TunnResult::Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Handshake init failed: {:?}", e),
                ));
            }
            _ => {
                warn!("WG handshake: unexpected result from format_handshake_initiation");
                break;
            }
        }
    }

    // Wait for response
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut recv_buf = vec![0u8; MAX_PACKET_SIZE];
    let mut dec_buf = vec![0u8; MAX_PACKET_SIZE];

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        match socket.recv(&mut recv_buf) {
            Ok(n) => {
                match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        // Send handshake response - ignore transient errors, retry will handle it
                        if let Err(e) = socket.send(data) {
                            warn!("WG handshake: send response failed: {}, will retry", e);
                            continue;
                        }
                        // Process any follow-up results to complete tunnel setup
                        loop {
                            match tunnel.decapsulate(None, &[], &mut dec_buf) {
                                TunnResult::WriteToNetwork(data) => {
                                    socket.send(data).ok();
                                }
                                _ => break,
                            }
                        }
                        // Flush timer events to finalize tunnel state
                        match tunnel.update_timers(&mut buf) {
                            TunnResult::WriteToNetwork(data) => {
                                socket.send(data).ok();
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    TunnResult::Done => {
                        return Ok(());
                    }
                    TunnResult::Err(e) => {
                        warn!("WG handshake: decapsulate error: {:?}", e);
                    }
                    _ => {}
                }
            },
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                // Timeout waiting for response - retry handshake initiation
                match tunnel.format_handshake_initiation(&mut buf, false) {
                    TunnResult::WriteToNetwork(data) => {
                        socket.send(data).ok(); // Ignore send errors, retry will handle it
                    }
                    _ => {}
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                // EINTR - interrupted by signal, just retry
                continue;
            }
            Err(ref e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                // ECONNREFUSED - ICMP port unreachable received
                // This can happen if:
                // 1. The WireGuard server is not listening on the port
                // 2. A firewall sent an ICMP rejection
                // Retry by re-sending the handshake initiation
                warn!("WG handshake: connection refused (ICMP port unreachable), retrying...");
                match tunnel.format_handshake_initiation(&mut buf, false) {
                    TunnResult::WriteToNetwork(data) => {
                        socket.send(data).ok();
                    }
                    _ => {}
                }
                // Small delay before retry to avoid hammering
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "WireGuard handshake timed out",
    ))
}

// ============================================================================
// Global HTTP client configuration
// ============================================================================

pub static GLOBAL_HTTP_CONFIG: Mutex<Option<WgHttpConfig>> = Mutex::new(None);

/// Set the WireGuard HTTP client configuration
pub fn wg_http_set_config(config: WgHttpConfig) {
    *GLOBAL_HTTP_CONFIG.lock() = Some(config);
}

/// Clear the WireGuard HTTP client configuration.
/// If streaming tunnel is active, keep the shared proxy running since it's
/// still needed to receive TCP packets injected by the streaming tunnel.
/// Only stop the proxy when streaming tunnel is not active.
pub fn wg_http_clear_config() {
    // Close all WgSocket connections first so they don't spin on dead channels
    crate::wg_socket::wg_socket_close_all();
    
    // Only stop the shared proxy if the streaming tunnel is NOT active.
    // When streaming tunnel is active, incoming TCP packets are routed through
    // wg_http_inject_packet and need the proxy's VirtualStack to process them.
    // Stopping the proxy during a streaming session would cause TCP packets to be dropped.
    if !crate::wireguard::wg_is_tunnel_active() {
        stop_shared_proxy();
    } else {
        info!("Streaming tunnel active - keeping shared proxy running for TCP routing");
    }
    
    *GLOBAL_HTTP_CONFIG.lock() = None;
}

/// Check if WireGuard HTTP client is configured
pub fn wg_http_is_configured() -> bool {
    GLOBAL_HTTP_CONFIG.lock().is_some()
}

/// Inject a received IP packet into the HTTP shared proxy's virtual stack.
/// This is called by the streaming tunnel when it receives TCP packets.
/// Cached Arc to avoid locking SHARED_TCP_PROXY on every injected packet.
static INJECT_PROXY_CACHE: Mutex<Option<Arc<SharedTcpProxy>>> = Mutex::new(None);

pub fn wg_http_inject_packet(packet: &[u8]) {
    // Fast path: try cached Arc first
    let proxy = {
        let cache = INJECT_PROXY_CACHE.lock();
        cache.clone()
    };
    let proxy = match proxy {
        Some(p) if p.running.load(Ordering::Relaxed) => p,
        _ => {
            // Cache miss or stale: refresh from SHARED_TCP_PROXY
            let shared = SHARED_TCP_PROXY.lock();
            match shared.as_ref() {
                Some(p) if p.running.load(Ordering::Relaxed) => {
                    let p = p.clone();
                    *INJECT_PROXY_CACHE.lock() = Some(p.clone());
                    p
                }
                _ => {
                    warn!("wg_http_inject_packet: no shared proxy configured");
                    return;
                }
            }
        }
    };
    proxy.virtual_stack.process_incoming_packet(packet);
    proxy.flush_outgoing();
}

// ============================================================================
// Shared WireGuard TCP stack (for HTTP/HTTPS and socket connections)
//
// Uses a SINGLE shared WireGuard tunnel with a manual TCP/IP stack
// (VirtualStack from tun_stack module). This avoids:
// 1. Multiple WG tunnels with the same key conflicting at the server
// 2. Routing issues by sharing the tunnel with streaming
// ============================================================================

/// DDNS re-resolution timeout in seconds (same as WireGuard's reresolve-dns.sh)
const DDNS_RERESOLVE_TIMEOUT_SECS: u64 = 135;

/// Shared WireGuard tunnel and virtual TCP stack for all TCP proxy connections.
/// Using a single tunnel avoids WG peer endpoint conflicts when multiple
/// connections use the same key pair.
pub struct SharedTcpProxy {
    /// boringtun tunnel instance (mutex for thread-safe access)
    tunnel: Mutex<Box<Tunn>>,
    /// UDP socket connected to WireGuard endpoint
    endpoint_socket: Mutex<UdpSocket>,
    /// Currently resolved endpoint address
    endpoint_addr: Mutex<SocketAddr>,
    /// Configuration for re-creating tunnel on DDNS re-resolution
    config: WgHttpConfig,
    /// Virtual TCP/IP stack
    pub virtual_stack: VirtualStack,
    /// Running flag for background threads
    running: Arc<AtomicBool>,
    /// Flag indicating receiver thread is ready
    receiver_ready: AtomicBool,
    /// Last successful handshake timestamp
    last_handshake: Mutex<Instant>,
    /// Condvar to wake receiver thread when packets are injected
    inject_notify: std::sync::Condvar,
    /// Mutex used with inject_notify
    inject_mutex: std::sync::Mutex<bool>,
}

/// Global shared TCP proxy (single WG tunnel for all connections)
pub static SHARED_TCP_PROXY: Mutex<Option<Arc<SharedTcpProxy>>> = Mutex::new(None);

impl SharedTcpProxy {
    /// Create a new shared proxy with WG tunnel and handshake.
    /// If streaming tunnel is active, skip creating our own WG session -
    /// packets will be routed through the streaming tunnel instead.
    fn new(config: &WgHttpConfig) -> io::Result<Arc<Self>> {
        let streaming_active = crate::wireguard::wg_is_tunnel_active();

        // Only create our own tunnel if streaming is not active
        let (tunnel, endpoint_socket, endpoint_addr) = if streaming_active {
            info!("Streaming tunnel active - HTTP proxy will route through it");
            // When routing through the streaming tunnel, we don't need a real
            // endpoint socket.  Create a minimal boringtun Tunn (pure crypto,
            // no network I/O) and a dummy loopback socket to satisfy the struct.
            let private_key = StaticSecret::from(config.private_key);
            let peer_public_key = PublicKey::from(config.peer_public_key);
            let tun = Box::new(Tunn::new(
                private_key,
                peer_public_key,
                config.preshared_key,
                None,
                0,
                None,
            ));

            // Dummy socket — will never be used for real I/O
            let dummy_socket = UdpSocket::bind("127.0.0.1:0")?;
            let dummy_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            (tun, dummy_socket, dummy_addr)
        } else {
            // Create tunnel with handshake (create_tunnel handles endpoint resolution internally)
            let (mut tun, sock, endpoint_addr) = create_tunnel(config)?;
            info!("Initial endpoint resolution: '{}' -> {}", config.endpoint, endpoint_addr);

            // Perform handshake before wrapping in Mutex
            do_handshake(&mut tun, &sock)?;
            info!("Shared WG tunnel handshake completed");
            
            // Flush timer events after handshake
            {
                let mut timer_buf = vec![0u8; MAX_PACKET_SIZE];
                match tun.update_timers(&mut timer_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        sock.send(data).ok();
                    }
                    _ => {}
                }
            }
            (tun, sock, endpoint_addr)
        };

        // Extract IPv4 for VirtualStack (IPv6 VirtualStack is separate)
        let tunnel_ipv4 = match config.tunnel_ip {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(_) => {
                // For IPv6-only configs, use a placeholder; VirtualStack
                // will be extended for IPv6 in a follow-up
                Ipv4Addr::new(10, 0, 0, 2)
            }
        };

        let proxy = Arc::new(SharedTcpProxy {
            tunnel: Mutex::new(tunnel),
            endpoint_socket: Mutex::new(endpoint_socket),
            endpoint_addr: Mutex::new(endpoint_addr),
            config: config.clone(),
            virtual_stack: VirtualStack::new(tunnel_ipv4),
            running: Arc::new(AtomicBool::new(true)),
            receiver_ready: AtomicBool::new(false),
            last_handshake: Mutex::new(Instant::now()),
            inject_notify: std::sync::Condvar::new(),
            inject_mutex: std::sync::Mutex::new(false),
        });

        // Start packet receiver thread
        let proxy_rx = proxy.clone();
        thread::Builder::new()
            .name("wg-tcp-proxy-rx".into())
            .spawn(move || {
                Self::receiver_loop(proxy_rx);
            })?;

        // Start timer thread
        let proxy_timer = proxy.clone();
        thread::Builder::new()
            .name("wg-tcp-proxy-timer".into())
            .spawn(move || {
                Self::timer_loop(proxy_timer);
            })?;

        // Wait for receiver thread to be ready (up to 500ms)
        let start = Instant::now();
        while !proxy.receiver_ready.load(Ordering::Acquire) && start.elapsed() < Duration::from_millis(500) {
            thread::sleep(Duration::from_millis(5));
        }
        if !proxy.receiver_ready.load(Ordering::Acquire) {
            warn!("SharedTcpProxy: receiver thread not ready after 500ms, proceeding anyway");
        }

        Ok(proxy)
    }

    /// Send queued outgoing IP packets through the WG tunnel.
    /// If the streaming tunnel is active, route through it instead to avoid two WG sessions.
    /// Uses batch send for streaming tunnel path to minimize lock contention.
    pub fn flush_outgoing(&self) {
        let packets = self.virtual_stack.take_outgoing_packets();
        if packets.is_empty() {
            return;
        }

        // Check if we should route through streaming tunnel
        if crate::wireguard::wg_is_tunnel_active() {
            // Batch send through streaming tunnel (single lock acquisition)
            if let Err(e) = crate::wireguard::wg_send_ip_packets_batch(&packets) {
                warn!("WG TCP proxy: batch send via streaming tunnel failed: {}", e);
            }
        } else {
            // Use our own tunnel
            let mut tunnel = self.tunnel.lock();
            let endpoint_socket = self.endpoint_socket.lock();
            let mut buf = vec![0u8; MAX_PACKET_SIZE + 200];
            let mut timer_flushed = false;

            for packet in &packets {
                match tunnel.encapsulate(packet, &mut buf) {
                    TunnResult::WriteToNetwork(data) => {
                        if let Err(e) = endpoint_socket.send(data) {
                            warn!("WG TCP proxy: send failed: {}", e);
                        }
                    }
                    TunnResult::Done => {
                        // Flush timers once to advance tunnel state, then retry
                        if !timer_flushed {
                            timer_flushed = true;
                            loop {
                                match tunnel.update_timers(&mut buf) {
                                    TunnResult::WriteToNetwork(data) => {
                                        endpoint_socket.send(data).ok();
                                    }
                                    _ => break,
                                }
                            }
                            if let TunnResult::WriteToNetwork(data) = tunnel.encapsulate(packet, &mut buf) {
                                if let Err(e) = endpoint_socket.send(data) {
                                    warn!("WG TCP proxy: send failed (retry): {}", e);
                                }
                            }
                        }
                    }
                    TunnResult::Err(e) => {
                        warn!("WG TCP proxy: encapsulate error: {:?}", e);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Background thread: receives WG packets, decapsulates, and dispatches to virtual stack
    fn receiver_loop(proxy: Arc<SharedTcpProxy>) {
        let mut recv_buf = vec![0u8; MAX_PACKET_SIZE];
        let mut dec_buf = vec![0u8; MAX_PACKET_SIZE];

        // Set read timeout for periodic checks
        {
            let endpoint_socket = proxy.endpoint_socket.lock();
            endpoint_socket.set_read_timeout(Some(Duration::from_millis(100))).ok();
        }

        // Signal that we're ready to receive packets
        proxy.receiver_ready.store(true, Ordering::Release);
        info!("WG TCP proxy receiver started");

        while proxy.running.load(Ordering::Relaxed) {
            // When streaming tunnel is active, packets are injected via wg_http_inject_packet
            // Skip socket operations to avoid receiving from wrong tunnel
            if crate::wireguard::wg_is_tunnel_active() {
                // Wait for inject notification or timeout for retransmissions
                {
                    let guard = proxy.inject_mutex.lock().unwrap();
                    let _ = proxy.inject_notify.wait_timeout(guard, Duration::from_millis(50));
                }
                // Check for TCP retransmissions
                proxy.virtual_stack.check_retransmissions();
                // Flush any outgoing packets generated by connection handling
                proxy.flush_outgoing();
                continue;
            }
            
            let recv_result = {
                let endpoint_socket = proxy.endpoint_socket.lock();
                endpoint_socket.recv(&mut recv_buf)
            };

            match recv_result {
                Ok(n) if n > 0 => {
                    // Update last handshake time on successful packet reception
                    *proxy.last_handshake.lock() = Instant::now();

                    // Decapsulate the WG packet(s)
                    let mut ip_packets = Vec::new();
                    {
                        let mut tunnel = proxy.tunnel.lock();
                        let endpoint_socket = proxy.endpoint_socket.lock();
                        match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
                            TunnResult::WriteToTunnelV4(data, _)
                            | TunnResult::WriteToTunnelV6(data, _) => {
                                ip_packets.push(data.to_vec());
                            }
                            TunnResult::WriteToNetwork(data) => {
                                endpoint_socket.send(data).ok();
                                // Drain follow-up results
                                loop {
                                    match tunnel.decapsulate(None, &[], &mut dec_buf) {
                                        TunnResult::WriteToTunnelV4(data, _)
                                        | TunnResult::WriteToTunnelV6(data, _) => {
                                            ip_packets.push(data.to_vec());
                                        }
                                        TunnResult::WriteToNetwork(data) => {
                                            endpoint_socket.send(data).ok();
                                        }
                                        _ => break,
                                    }
                                }
                            }
                            TunnResult::Err(e) => {
                                debug!("WG TCP proxy: decapsulate error: {:?}", e);
                            }
                            _ => {}
                        }
                    }

                    // Process IP packets through virtual stack (tunnel lock released)
                    for packet in ip_packets {
                        proxy.virtual_stack.process_incoming_packet(&packet);
                    }

                    // Flush any outgoing packets generated by processing (e.g., ACKs)
                    proxy.flush_outgoing();
                }
                Err(ref e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut
                        || e.kind() == io::ErrorKind::Interrupted
                        || e.kind() == io::ErrorKind::ConnectionRefused =>
                {
                    // WouldBlock/TimedOut: no data, check retransmissions and flush
                    // Interrupted (EINTR): interrupted by signal, retry
                    // ConnectionRefused: ICMP port unreachable, retry (server may be restarting)
                    proxy.virtual_stack.check_retransmissions();
                    proxy.flush_outgoing();
                }
                Err(e) => {
                    if proxy.running.load(Ordering::Relaxed) {
                        warn!("WG TCP proxy: recv error: {}", e);
                    }
                }
                _ => {}
            }
        }

        info!("WG TCP proxy receiver stopped");
    }

    /// Re-resolve DNS and reconnect to the new endpoint address.
    /// This implements the same logic as WireGuard's reresolve-dns.sh script.
    fn reresolve_endpoint(&self) -> io::Result<()> {
        let new_addr = resolve_endpoint(&self.config.endpoint)?;
        let mut current_addr = self.endpoint_addr.lock();

        if new_addr != *current_addr {
            info!("DDNS re-resolution: endpoint '{}' changed {} -> {}",
                  self.config.endpoint, *current_addr, new_addr);

            // Create new socket and connect to new address (address family must match)
            let new_socket = UdpSocket::bind(bind_addr_for(&new_addr))?;
            new_socket.connect(new_addr)?;
            new_socket.set_read_timeout(Some(Duration::from_millis(100)))?;

            // Replace socket and address
            let mut endpoint_socket = self.endpoint_socket.lock();
            *endpoint_socket = new_socket;
            *current_addr = new_addr;

            info!("DDNS: reconnected to new endpoint {}", new_addr);
        } else {
            debug!("DDNS re-resolution: endpoint '{}' unchanged ({})",
                   self.config.endpoint, new_addr);
        }

        Ok(())
    }

    /// Background thread: DDNS re-resolution, handshake maintenance, and stale connection cleanup
    fn timer_loop(proxy: Arc<SharedTcpProxy>) {
        let mut buf = vec![0u8; 256];
        let mut handshake_buf = vec![0u8; MAX_PACKET_SIZE];
        let mut handshake_retry_count = 0u32;
        const MAX_HANDSHAKE_RETRIES: u32 = 5;

        while proxy.running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(1));

            // Skip WG timer updates when streaming tunnel is active
            // (streaming tunnel handles its own timers, we just handle connection cleanup)
            if !crate::wireguard::wg_is_tunnel_active() {
                // Check for DDNS re-resolution (same as WireGuard's reresolve-dns.sh)
                // If no successful handshake in DDNS_RERESOLVE_TIMEOUT_SECS, re-resolve DNS
                let last_handshake_elapsed = proxy.last_handshake.lock().elapsed();
                if last_handshake_elapsed > Duration::from_secs(DDNS_RERESOLVE_TIMEOUT_SECS) {
                    info!("DDNS: no handshake for {} seconds, re-resolving endpoint",
                          last_handshake_elapsed.as_secs());

                    if let Err(e) = proxy.reresolve_endpoint() {
                        warn!("DDNS re-resolution failed: {}", e);
                    } else {
                        // Reset handshake retry count after re-resolution
                        handshake_retry_count = 0;

                        // Initiate new handshake after endpoint change
                        let mut tunnel = proxy.tunnel.lock();
                        let endpoint_socket = proxy.endpoint_socket.lock();
                        match tunnel.format_handshake_initiation(&mut handshake_buf, false) {
                            TunnResult::WriteToNetwork(data) => {
                                endpoint_socket.send(data).ok();
                                info!("DDNS: initiated handshake to new endpoint");
                            }
                            _ => {}
                        }
                        // Update last handshake time to prevent immediate re-resolution loop
                        *proxy.last_handshake.lock() = Instant::now();
                    }
                }

                // Update WG timers (handshake, etc.)
                {
                    let mut tunnel = proxy.tunnel.lock();
                    let endpoint_socket = proxy.endpoint_socket.lock();
                    loop {
                        match tunnel.update_timers(&mut buf) {
                            TunnResult::WriteToNetwork(data) => {
                                endpoint_socket.send(data).ok();
                            }
                            TunnResult::Err(e) => {
                                let error_str = format!("{:?}", e);
                                if error_str.contains("ConnectionExpired") {
                                    if handshake_retry_count < MAX_HANDSHAKE_RETRIES {
                                        handshake_retry_count += 1;
                                        warn!("WG TCP proxy: connection expired, re-initiating handshake (attempt {})",
                                              handshake_retry_count);

                                        // Try to re-initiate handshake
                                        match tunnel.format_handshake_initiation(&mut handshake_buf, false) {
                                            TunnResult::WriteToNetwork(data) => {
                                                endpoint_socket.send(data).ok();
                                            }
                                            _ => {}
                                        }
                                    }
                                } else {
                                    debug!("WG TCP proxy timer error: {:?}", e);
                                }
                                break;
                            }
                            _ => break,
                        }
                    }
                }
            } else {
                // When streaming is active, reset retry count
                handshake_retry_count = 0;
            }

            // Periodic stale connection cleanup (every ~15 seconds)
            static CLEANUP_COUNTER: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            let counter = CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed);
            if counter % 15 == 0 {
                let removed = proxy.virtual_stack.cleanup_stale_connections();
                if removed > 0 {
                    info!(
                        "Cleaned up {} stale TCP connections (active: {})",
                        removed,
                        proxy.virtual_stack.connection_count()
                    );
                }
            }

            // Check for TCP data retransmissions every second
            let retransmitted = proxy.virtual_stack.check_retransmissions();
            if retransmitted > 0 {
                proxy.flush_outgoing();
            }
        }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Release);
        // Wake receiver thread if blocked on inject_notify
        self.inject_notify.notify_all();
    }
}

/// Get or create the shared WG tunnel for TCP proxying.
/// When streaming tunnel is active, the shared proxy routes through it instead of
/// creating its own WG session.
pub fn get_or_create_shared_proxy(config: &WgHttpConfig) -> io::Result<Arc<SharedTcpProxy>> {
    let mut shared = SHARED_TCP_PROXY.lock();
    if let Some(ref proxy) = *shared {
        if proxy.running.load(Ordering::Relaxed) {
            return Ok(proxy.clone());
        }
    }

    info!("Creating shared WG tunnel for TCP proxy");
    let proxy = SharedTcpProxy::new(config)?;
    *shared = Some(proxy.clone());
    Ok(proxy)
}

/// Stop the shared WireGuard tunnel.
/// Called when WireGuard is disabled or when the streaming tunnel starts.
pub fn stop_shared_proxy() {
    // Clear inject cache first
    *INJECT_PROXY_CACHE.lock() = None;

    let mut shared = SHARED_TCP_PROXY.lock();
    if let Some(ref proxy) = *shared {
        proxy.stop();
        info!("Stopped shared WG TCP proxy tunnel");
    }
    *shared = None;
}

