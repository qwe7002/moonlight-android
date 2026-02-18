//! WireGuard TCP proxy infrastructure
//!
//! This module provides:
//! - WgHttpConfig for configuring WireGuard tunnels
//! - SharedTcpProxy for routing TCP connections through WireGuard
//! - Global configuration management (GLOBAL_HTTP_CONFIG)
//!
//! HTTP requests go through OkHttp + WgSocket -> wg_socket.rs -> SharedTcpProxy

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use parking_lot::Mutex;

use boringtun::noise::{Tunn, TunnResult};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::tun_stack::{VirtualStack, TcpState as VirtualTcpState};

/// Maximum packet size for WireGuard
const MAX_PACKET_SIZE: usize = 65535;

/// Connection timeout
const CONNECTION_TIMEOUT_SECS: u64 = 10;

/// WireGuard tunnel configuration
#[derive(Clone)]
pub struct WgHttpConfig {
    pub private_key: [u8; 32],
    pub peer_public_key: [u8; 32],
    pub preshared_key: Option<[u8; 32]>,
    pub endpoint: SocketAddr,
    pub tunnel_ip: Ipv4Addr,
    pub server_ip: Ipv4Addr,
    pub keepalive_secs: u16,
    pub mtu: u16,
}

/// Create a WireGuard tunnel
fn create_tunnel(config: &WgHttpConfig) -> io::Result<(Box<Tunn>, UdpSocket)> {
    let private_key = StaticSecret::from(config.private_key);
    let peer_public_key = PublicKey::from(config.peer_public_key);

    let tunnel = Tunn::new(
        private_key,
        peer_public_key,
        config.preshared_key,
        Some(config.keepalive_secs),
        0,
        None,
    )
    .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Tunn::new failed: {}", e)))?;

    let endpoint_socket = UdpSocket::bind("0.0.0.0:0")?;
    endpoint_socket.connect(config.endpoint)?;

    Ok((tunnel, endpoint_socket))
}

/// Perform WireGuard handshake with proper continuation and logging
fn do_handshake(tunnel: &mut Tunn, socket: &UdpSocket) -> io::Result<()> {
    let mut buf = vec![0u8; MAX_PACKET_SIZE];

    // Initiate handshake
    match tunnel.format_handshake_initiation(&mut buf, false) {
        TunnResult::WriteToNetwork(data) => {
            debug!("WG handshake: sending initiation ({} bytes)", data.len());
            socket.send(data)?;
        }
        TunnResult::Err(e) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Handshake init failed: {:?}", e),
            ));
        }
        _ => {
            warn!("WG handshake: unexpected result from format_handshake_initiation");
        }
    }

    // Wait for response
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut recv_buf = vec![0u8; MAX_PACKET_SIZE];
    let mut dec_buf = vec![0u8; MAX_PACKET_SIZE];

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        match socket.recv(&mut recv_buf) {
            Ok(n) => {
                debug!("WG handshake: received {} bytes from endpoint", n);
                match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        debug!("WG handshake: sending response ({} bytes)", data.len());
                        socket.send(data)?;
                        // Process any follow-up results to complete tunnel setup
                        loop {
                            match tunnel.decapsulate(None, &[], &mut dec_buf) {
                                TunnResult::WriteToNetwork(data) => {
                                    debug!("WG handshake: sending follow-up ({} bytes)", data.len());
                                    socket.send(data)?;
                                }
                                _ => break,
                            }
                        }
                        // Flush timer events to finalize tunnel state
                        match tunnel.update_timers(&mut buf) {
                            TunnResult::WriteToNetwork(data) => {
                                debug!("WG handshake: flushing timer event ({} bytes)", data.len());
                                socket.send(data).ok();
                            }
                            _ => {}
                        }
                        debug!("WG handshake: completed successfully");
                        return Ok(());
                    }
                    TunnResult::Done => {
                        debug!("WG handshake: got Done, tunnel may already be established");
                        return Ok(());
                    }
                    TunnResult::Err(e) => {
                        warn!("WG handshake: decapsulate error: {:?}", e);
                    }
                    _ => {
                        debug!("WG handshake: unexpected decapsulate result, continuing");
                    }
                }
            },
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                debug!("WG handshake: timeout waiting for response, retrying initiation");
                match tunnel.format_handshake_initiation(&mut buf, false) {
                    TunnResult::WriteToNetwork(data) => {
                        socket.send(data)?;
                    }
                    _ => {}
                }
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

/// Clear the WireGuard HTTP client configuration and stop the shared proxy
pub fn wg_http_clear_config() {
    // Stop the shared proxy first
    stop_shared_proxy();
    *GLOBAL_HTTP_CONFIG.lock() = None;
}

/// Check if WireGuard HTTP client is configured
pub fn wg_http_is_configured() -> bool {
    GLOBAL_HTTP_CONFIG.lock().is_some()
}

/// Inject a received IP packet into the HTTP shared proxy's virtual stack.
/// This is called by the streaming tunnel when it receives TCP packets.
pub fn wg_http_inject_packet(packet: &[u8]) {
    let shared = SHARED_TCP_PROXY.lock();
    if let Some(ref proxy) = *shared {
        if proxy.running.load(Ordering::SeqCst) {
            proxy.virtual_stack.process_incoming_packet(packet);
            // Flush any responses generated by processing this packet
            proxy.flush_outgoing();
        }
    }
}

// ============================================================================
// Shared WireGuard TCP stack (for HTTP/HTTPS and socket connections)
//
// Uses a SINGLE shared WireGuard tunnel with a manual TCP/IP stack
// (VirtualStack from tun_stack module). This avoids:
// 1. Multiple WG tunnels with the same key conflicting at the server
// 2. Routing issues by sharing the tunnel with streaming
// ============================================================================

/// Shared WireGuard tunnel and virtual TCP stack for all TCP proxy connections.
/// Using a single tunnel avoids WG peer endpoint conflicts when multiple
/// connections use the same key pair.
pub struct SharedTcpProxy {
    /// boringtun tunnel instance (mutex for thread-safe access)
    tunnel: Mutex<Box<Tunn>>,
    /// UDP socket connected to WireGuard endpoint
    endpoint_socket: UdpSocket,
    /// Virtual TCP/IP stack
    pub virtual_stack: VirtualStack,
    /// Running flag for background threads
    running: Arc<AtomicBool>,
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
        let (tunnel, endpoint_socket) = if streaming_active {
            info!("Streaming tunnel active - HTTP proxy will route through it");
            // Create a dummy tunnel and socket that won't be used
            // We still need them for SharedTcpProxy struct, but I/O will go through streaming
            let (tun, sock) = create_tunnel(config)?;
            // Don't do handshake - streaming tunnel is already handling WG session
            (tun, sock)
        } else {
            let (mut tun, sock) = create_tunnel(config)?;
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
            (tun, sock)
        };

        let proxy = Arc::new(SharedTcpProxy {
            tunnel: Mutex::new(tunnel),
            endpoint_socket,
            virtual_stack: VirtualStack::new(config.tunnel_ip),
            running: Arc::new(AtomicBool::new(true)),
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

        Ok(proxy)
    }

    /// Send queued outgoing IP packets through the WG tunnel.
    /// If the streaming tunnel is active, route through it instead to avoid two WG sessions.
    pub fn flush_outgoing(&self) {
        let packets = self.virtual_stack.take_outgoing_packets();
        if packets.is_empty() {
            return;
        }

        // Check if we should route through streaming tunnel
        if crate::wireguard::wg_is_tunnel_active() {
            // Route through streaming tunnel
            for packet in &packets {
                if let Err(e) = crate::wireguard::wg_send_ip_packet(packet) {
                    warn!("WG TCP proxy: send via streaming tunnel failed: {}", e);
                }
            }
        } else {
            // Use our own tunnel
            let tunnel = self.tunnel.lock();
            let mut buf = vec![0u8; MAX_PACKET_SIZE + 200];

            for packet in &packets {
                match tunnel.encapsulate(packet, &mut buf) {
                    TunnResult::WriteToNetwork(data) => {
                        if let Err(e) = self.endpoint_socket.send(data) {
                            warn!("WG TCP proxy: send failed: {}", e);
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
        proxy
            .endpoint_socket
            .set_read_timeout(Some(Duration::from_millis(5)))
            .ok();

        info!("WG TCP proxy receiver started");

        while proxy.running.load(Ordering::SeqCst) {
            // When streaming tunnel is active, packets are injected via wg_http_inject_packet
            // Skip socket operations to avoid receiving from wrong tunnel
            if crate::wireguard::wg_is_tunnel_active() {
                // Packets come via inject_packet, just sleep to avoid busy loop
                std::thread::sleep(Duration::from_millis(10));
                // Flush any outgoing packets generated by connection handling
                proxy.flush_outgoing();
                continue;
            }
            
            match proxy.endpoint_socket.recv(&mut recv_buf) {
                Ok(n) if n > 0 => {
                    // Decapsulate the WG packet(s)
                    let mut ip_packets = Vec::new();
                    {
                        let tunnel = proxy.tunnel.lock();
                        match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
                            TunnResult::WriteToTunnelV4(data, _)
                            | TunnResult::WriteToTunnelV6(data, _) => {
                                ip_packets.push(data.to_vec());
                            }
                            TunnResult::WriteToNetwork(data) => {
                                proxy.endpoint_socket.send(data).ok();
                                // Drain follow-up results
                                loop {
                                    match tunnel.decapsulate(None, &[], &mut dec_buf) {
                                        TunnResult::WriteToTunnelV4(data, _)
                                        | TunnResult::WriteToTunnelV6(data, _) => {
                                            ip_packets.push(data.to_vec());
                                        }
                                        TunnResult::WriteToNetwork(data) => {
                                            proxy.endpoint_socket.send(data).ok();
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
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    // Timeout - flush any pending outgoing packets from other threads
                    proxy.flush_outgoing();
                }
                Err(e) => {
                    if proxy.running.load(Ordering::SeqCst) {
                        debug!("WG TCP proxy: recv error: {}", e);
                    }
                }
                _ => {}
            }
        }

        info!("WG TCP proxy receiver stopped");
    }

    /// Background thread: WG keepalive and stale connection cleanup
    fn timer_loop(proxy: Arc<SharedTcpProxy>) {
        let mut buf = vec![0u8; 256];
        let mut handshake_buf = vec![0u8; MAX_PACKET_SIZE];
        let mut handshake_retry_count = 0u32;
        const MAX_HANDSHAKE_RETRIES: u32 = 5;

        while proxy.running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));

            // Skip WG timer updates when streaming tunnel is active
            // (streaming tunnel handles keepalives, we just handle connection cleanup)
            if !crate::wireguard::wg_is_tunnel_active() {
                // Update WG timers (keepalive, etc.)
                let tunnel = proxy.tunnel.lock();
                loop {
                    match tunnel.update_timers(&mut buf) {
                        TunnResult::WriteToNetwork(data) => {
                            proxy.endpoint_socket.send(data).ok();
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
                                            proxy.endpoint_socket.send(data).ok();
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
            } else {
                // When streaming is active, reset retry count
                handshake_retry_count = 0;
            }

            // Periodic stale connection cleanup (every ~15 seconds)
            static CLEANUP_COUNTER: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            if CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed) % 15 == 0 {
                let removed = proxy.virtual_stack.cleanup_stale_connections();
                if removed > 0 {
                    info!(
                        "Cleaned up {} stale TCP connections (active: {})",
                        removed,
                        proxy.virtual_stack.connection_count()
                    );
                }
            }
        }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Get or create the shared WG tunnel for TCP proxying.
/// When streaming tunnel is active, the shared proxy routes through it instead of
/// creating its own WG session.
pub fn get_or_create_shared_proxy(config: &WgHttpConfig) -> io::Result<Arc<SharedTcpProxy>> {
    let mut shared = SHARED_TCP_PROXY.lock();
    if let Some(ref proxy) = *shared {
        if proxy.running.load(Ordering::SeqCst) {
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
    let mut shared = SHARED_TCP_PROXY.lock();
    if let Some(ref proxy) = *shared {
        proxy.stop();
        info!("Stopped shared WG TCP proxy tunnel");
    }
    *shared = None;
}

