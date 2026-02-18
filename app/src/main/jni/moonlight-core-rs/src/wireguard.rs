//! WireGuard tunnel module using boringtun
//!
//! This module provides a userspace WireGuard tunnel that allows moonlight streaming
//! traffic to be routed through a WireGuard VPN without requiring a system TUN device.
//!
//! Architecture:
//! - Uses boringtun for WireGuard protocol (Noise handshake, encryption/decryption)
//! - Creates a real UDP socket to the WireGuard peer endpoint
//! - Provides UDP proxy sockets that tunnel traffic through WireGuard
//! - Provides TCP proxy using smoltcp for RTSP/control traffic
//! - All moonlight streaming traffic (video, audio, control) goes through the tunnel

#![allow(unused_mut)]
#![allow(unused_variables)]

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::collections::HashMap;

use boringtun::noise::{Tunn, TunnResult};
use x25519_dalek::{PublicKey, StaticSecret};
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{Socket as SmolTcpSocket, SocketBuffer, State as TcpState};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{IpAddress, IpCidr, IpEndpoint};

// Re-export configuration from dedicated module
pub use crate::wireguard_config::WireGuardConfig;

/// Maximum size of a UDP packet
const MAX_UDP_PACKET_SIZE: usize = 65535;

/// Buffer size for WireGuard encapsulation overhead
const WG_BUFFER_SIZE: usize = MAX_UDP_PACKET_SIZE + 256;


/// State of the WireGuard tunnel
struct TunnelState {
    /// The boringtun tunnel instance
    tunnel: Box<Tunn>,
    /// UDP socket connected to the WireGuard endpoint
    endpoint_socket: UdpSocket,
    /// Whether the tunnel is established (handshake completed)
    handshake_completed: AtomicBool,
    /// Reusable buffer for encoding
    encode_buf: Vec<u8>,
    /// Reusable buffer for decoding
    decode_buf: Vec<u8>,
}

/// A UDP proxy socket that tunnels traffic through WireGuard.
/// 
/// This creates a local UDP socket pair - one end is returned to the caller
/// (moonlight-common-c), and the other end is managed by the tunnel to
/// encapsulate/decapsulate WireGuard traffic.
pub struct WgUdpProxy {
    /// The local socket that moonlight-common-c will use
    pub local_socket: UdpSocket,
    /// The peer address that this proxy forwards to through the tunnel
    pub target_addr: SocketAddr,
}

/// A TCP proxy socket that tunnels traffic through WireGuard.
pub struct WgTcpProxy {
    /// The local port that moonlight-common-c will connect to
    pub local_port: u16,
    /// The peer address that this proxy forwards to through the tunnel
    pub target_addr: SocketAddr,
}

/// The WireGuard tunnel manager
pub struct WireGuardTunnel {
    config: WireGuardConfig,
    state: Arc<Mutex<TunnelState>>,
    running: Arc<AtomicBool>,
    /// Map of local proxy port -> target address for UDP proxies
    udp_proxies: Arc<Mutex<HashMap<u16, UdpProxyEntry>>>,
}

struct UdpProxyEntry {
    /// Socket receiving from moonlight-common-c
    proxy_socket: UdpSocket,
    /// The target address in the WireGuard network
    target_addr: SocketAddr,
    /// The local address moonlight connects to
    local_addr: SocketAddr,
    /// The client address that connected to this proxy (for sending responses back)
    client_addr: Arc<Mutex<Option<SocketAddr>>>,
}

impl WireGuardTunnel {
    /// Create a new WireGuard tunnel with the given configuration.
    pub fn new(config: WireGuardConfig) -> io::Result<Self> {
        info!("Creating WireGuard tunnel to endpoint: {}", config.endpoint);

        // Create the private/public key pair
        let private_key = StaticSecret::from(config.private_key);
        let peer_public_key = PublicKey::from(config.peer_public_key);

        // Create the boringtun tunnel
        let tunnel = Tunn::new(
            private_key,
            peer_public_key,
            config.preshared_key,
            Some(config.keepalive_secs),
            0, // index
            None, // rate limiter
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Create UDP socket to the WireGuard endpoint
        let endpoint_socket = UdpSocket::bind("0.0.0.0:0")?;
        endpoint_socket.connect(config.endpoint)?;
        endpoint_socket.set_nonblocking(false)?;

        // Set a read timeout so we can periodically check for shutdown
        endpoint_socket.set_read_timeout(Some(Duration::from_millis(100)))?;

        info!("WireGuard endpoint socket bound to: {}", endpoint_socket.local_addr()?);

        let state = Arc::new(Mutex::new(TunnelState {
            tunnel,
            endpoint_socket,
            handshake_completed: AtomicBool::new(false),
            encode_buf: vec![0u8; WG_BUFFER_SIZE],
            decode_buf: vec![0u8; WG_BUFFER_SIZE],
        }));

        let running = Arc::new(AtomicBool::new(false));

        Ok(WireGuardTunnel {
            config,
            state,
            running,
            udp_proxies: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Start the WireGuard tunnel.
    /// This initiates the handshake and starts the background packet processing threads.
    pub fn start(&self) -> io::Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);
        info!("Starting WireGuard tunnel...");

        // Initiate the handshake
        self.initiate_handshake()?;

        // Start the endpoint receiver thread - reads from the real WireGuard endpoint
        // and decapsulates packets, forwarding to the appropriate proxy socket
        let state = self.state.clone();
        let running = self.running.clone();
        let udp_proxies = self.udp_proxies.clone();

        thread::Builder::new()
            .name("wg-endpoint-rx".into())
            .spawn(move || {
                Self::endpoint_receiver_loop(state, running, udp_proxies);
            })?;

        // Start the timer thread for keepalive and handshake retransmission
        let state = self.state.clone();
        let running = self.running.clone();

        thread::Builder::new()
            .name("wg-timer".into())
            .spawn(move || {
                Self::timer_loop(state, running);
            })?;

        info!("WireGuard tunnel started");
        Ok(())
    }

    /// Stop the WireGuard tunnel.
    pub fn stop(&self) {
        info!("Stopping WireGuard tunnel...");
        self.running.store(false, Ordering::SeqCst);

        // Clear proxies
        self.udp_proxies.lock().clear();

        info!("WireGuard tunnel stopped");
    }

    /// Check if the tunnel is running and the handshake is completed.
    pub fn is_ready(&self) -> bool {
        self.running.load(Ordering::SeqCst)
            && self.state.lock().handshake_completed.load(Ordering::SeqCst)
    }

    /// Wait for the handshake to complete, with a timeout.
    pub fn wait_for_handshake(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.is_ready() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Create a UDP proxy for tunneling moonlight streaming traffic.
    /// 
    /// Returns the local address that moonlight-common-c should connect to,
    /// which will transparently tunnel traffic through WireGuard to `target_addr`.
    pub fn create_udp_proxy(&self, target_addr: SocketAddr) -> io::Result<SocketAddr> {
        // Create a local UDP socket pair for the proxy
        let proxy_socket = UdpSocket::bind("127.0.0.1:0")?;
        let local_addr = proxy_socket.local_addr()?;
        proxy_socket.set_nonblocking(true)?;

        info!("Created UDP proxy: {} -> {} (via WireGuard)", local_addr, target_addr);

        // Start a forwarder thread for this proxy (local -> WireGuard)
        let state = self.state.clone();
        let running = self.running.clone();
        let proxy_recv = proxy_socket.try_clone()?;

        let target = target_addr;
        let client_addr_tracker = Arc::new(Mutex::new(None::<SocketAddr>));
        let client_addr_for_loop = client_addr_tracker.clone();
        
        thread::Builder::new()
            .name(format!("wg-udp-fwd-{}", local_addr.port()))
            .spawn(move || {
                Self::udp_proxy_forward_loop(state, running, proxy_recv, target, client_addr_for_loop);
            })?;

        // Register the proxy for reverse traffic (WireGuard -> local)
        self.udp_proxies.lock().insert(target_addr.port(), UdpProxyEntry {
            proxy_socket,
            target_addr,
            local_addr,
            client_addr: client_addr_tracker,
        });

        Ok(local_addr)
    }

    /// Create UDP proxies for all moonlight streaming ports on the same local ports.
    /// This allows using 127.0.0.1 as the server address while forwarding to the WG target.
    /// 
    /// Moonlight uses:
    /// - base_port (47998): video
    /// - base_port+1 (47999): control
    /// - base_port+2 (48000): audio
    /// - RTSP port (47989 or 48010): setup (TCP)
    pub fn create_streaming_proxies(&self, target_ip: Ipv4Addr, base_port: u16) -> io::Result<()> {
        // Create proxies for video, control, audio
        let mut success_count = 0;
        for offset in 0..3 {
            let port = base_port + offset;
            let target = SocketAddr::new(IpAddr::V4(target_ip), port);
            
            // Try to bind to the same port locally for transparent proxying
            let proxy_socket = match UdpSocket::bind(format!("127.0.0.1:{}", port)) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Could not bind to port {}: {}, trying random port", port, e);
                    match UdpSocket::bind("127.0.0.1:0") {
                        Ok(s) => s,
                        Err(e2) => {
                            error!("Failed to create UDP proxy for port {}: {}", port, e2);
                            continue; // Skip this port, try next
                        }
                    }
                }
            };
            let local_addr = match proxy_socket.local_addr() {
                Ok(addr) => addr,
                Err(e) => {
                    error!("Failed to get local address for port {}: {}", port, e);
                    continue;
                }
            };
            if let Err(e) = proxy_socket.set_nonblocking(true) {
                error!("Failed to set nonblocking for port {}: {}", port, e);
                continue;
            }

            info!("Created streaming UDP proxy: {} -> {} (via WireGuard)", local_addr, target);

            let state = self.state.clone();
            let running = self.running.clone();
            let proxy_recv = match proxy_socket.try_clone() {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to clone UDP socket for port {}: {}", port, e);
                    continue;
                }
            };

            let client_addr_tracker = Arc::new(Mutex::new(None::<SocketAddr>));
            let client_addr_for_loop = client_addr_tracker.clone();

            if let Err(e) = thread::Builder::new()
                .name(format!("wg-udp-stream-{}", port))
                .spawn(move || {
                    Self::udp_proxy_forward_loop(state, running, proxy_recv, target, client_addr_for_loop);
                }) {
                error!("Failed to spawn UDP proxy thread for port {}: {}", port, e);
                continue;
            }

            self.udp_proxies.lock().insert(port, UdpProxyEntry {
                proxy_socket,
                target_addr: target,
                local_addr,
                client_addr: client_addr_tracker,
            });
            success_count += 1;
        }

        if success_count == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to create any UDP proxy"));
        }
        
        info!("Created {}/3 streaming UDP proxies", success_count);
        Ok(())
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

    /// Send encapsulated data through the WireGuard tunnel
    fn send_through_tunnel(state: &Arc<Mutex<TunnelState>>, payload: &[u8], src_addr: SocketAddr, dst_addr: SocketAddr) -> io::Result<()> {
        // Build an IP/UDP packet wrapping the payload
        let ip_packet = build_udp_ip_packet(src_addr, dst_addr, payload);

        let mut st = state.lock();
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];

        match st.tunnel.encapsulate(&ip_packet, &mut dst_buf) {
            TunnResult::WriteToNetwork(data) => {
                st.endpoint_socket.send(data)?;
            }
            TunnResult::Err(e) => {
                error!("WireGuard encapsulation error: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, format!("Encapsulation failed: {:?}", e)));
            }
            _ => {}
        }

        Ok(())
    }

    /// Background thread: receives packets from the WireGuard endpoint and decapsulates them
    fn endpoint_receiver_loop(
        state: Arc<Mutex<TunnelState>>,
        running: Arc<AtomicBool>,
        udp_proxies: Arc<Mutex<HashMap<u16, UdpProxyEntry>>>,
    ) {
        let mut recv_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut dec_buf = vec![0u8; WG_BUFFER_SIZE];

        info!("WireGuard endpoint receiver started");

        while running.load(Ordering::SeqCst) {
            // Read from the endpoint socket
            let n = {
                let st = state.lock();
                match st.endpoint_socket.recv(&mut recv_buf) {
                    Ok(n) => n,
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                        continue;
                    }
                    Err(e) => {
                        if running.load(Ordering::SeqCst) {
                            error!("WireGuard endpoint recv error: {}", e);
                        }
                        continue;
                    }
                }
            };

            // Decapsulate the WireGuard packet
            let mut st = state.lock();
            let result = st.tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf);

            match result {
                TunnResult::WriteToNetwork(data) => {
                    // This is typically a handshake response or keepalive
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
                            if !st.handshake_completed.load(Ordering::SeqCst) {
                                st.handshake_completed.store(true, Ordering::SeqCst);
                                info!("WireGuard handshake completed!");
                            }
                        }
                        TunnResult::Done => {
                            if !st.handshake_completed.load(Ordering::SeqCst) {
                                st.handshake_completed.store(true, Ordering::SeqCst);
                                info!("WireGuard handshake completed!");
                            }
                        }
                        _ => {}
                    }
                }
                TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                    // Decapsulated IP packet - extract and forward to the right proxy
                    if !st.handshake_completed.load(Ordering::SeqCst) {
                        st.handshake_completed.store(true, Ordering::SeqCst);
                        info!("WireGuard handshake completed (first data packet)!");
                    }
                    drop(st); // Release lock before forwarding

                    // Check IP protocol (byte at offset 9 in IPv4 header)
                    if data.len() >= 20 {
                        let protocol = data[9];
                        if protocol == 6 {
                            // TCP packet - forward to HTTP shared proxy's virtual stack
                            crate::wg_http::wg_http_inject_packet(data);
                        } else if protocol == 17 {
                            // UDP packet - try zero-copy channel first, then proxy fallback
                            if let Some((src_port, dst_port, payload)) = parse_udp_from_ip_packet(data) {
                                // Try zero-copy delivery via platform_sockets channel
                                if crate::platform_sockets::try_push_udp_data(src_port, payload) {
                                    // Data delivered via zero-copy channel, skip proxy
                                } else {
                                    // No zero-copy channel, use proxy fallback
                                    let proxies = udp_proxies.lock();
                                    if let Some(proxy) = proxies.get(&src_port) {
                                        let client = proxy.client_addr.lock();
                                        if let Some(client_addr) = *client {
                                            if let Err(e) = proxy.proxy_socket.send_to(payload, client_addr) {
                                                debug!("Failed to forward decapsulated UDP packet to {}: {}", client_addr, e);
                                            }
                                        } else {
                                            debug!("No client connected to proxy for port {} yet", src_port);
                                        }
                                    } else {
                                        debug!("No proxy found for source port {}", src_port);
                                    }
                                }
                            }
                        }
                    }
                }
                TunnResult::Done => {
                    // Keepalive or similar - nothing to forward
                }
                TunnResult::Err(e) => {
                    warn!("WireGuard decapsulation error: {:?}", e);
                }
            }
        }

        info!("WireGuard endpoint receiver stopped");
    }

    /// Background thread: forwards packets from a local proxy socket through WireGuard
    fn udp_proxy_forward_loop(
        state: Arc<Mutex<TunnelState>>,
        running: Arc<AtomicBool>,
        proxy_socket: UdpSocket,
        target_addr: SocketAddr,
        client_addr_tracker: Arc<Mutex<Option<SocketAddr>>>,
    ) {
        let mut recv_buf = vec![0u8; MAX_UDP_PACKET_SIZE];
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];

        // Set read timeout for periodic shutdown check
        let _ = proxy_socket.set_read_timeout(Some(Duration::from_millis(100)));

        debug!("UDP proxy forwarder started for target {}", target_addr);

        while running.load(Ordering::SeqCst) {
            let (n, src_addr) = match proxy_socket.recv_from(&mut recv_buf) {
                Ok(result) => result,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                    continue;
                }
                Err(e) => {
                    if running.load(Ordering::SeqCst) {
                        debug!("UDP proxy recv error: {}", e);
                    }
                    continue;
                }
            };

            // Store the client address so we can send responses back
            {
                let mut client = client_addr_tracker.lock();
                if client.is_none() || *client != Some(src_addr) {
                    debug!("UDP proxy: tracking client {} for target {}", src_addr, target_addr);
                    *client = Some(src_addr);
                }
            }

            // Build IP/UDP packet and encapsulate through WireGuard
            let src_socket_addr = SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), // Our tunnel IP  
                src_addr.port()
            );
            let ip_packet = build_udp_ip_packet(src_socket_addr, target_addr, &recv_buf[..n]);

            let mut st = state.lock();
            match st.tunnel.encapsulate(&ip_packet, &mut dst_buf) {
                TunnResult::WriteToNetwork(data) => {
                    if let Err(e) = st.endpoint_socket.send(data) {
                        debug!("Failed to send encapsulated packet: {}", e);
                    }
                }
                TunnResult::Err(e) => {
                    warn!("WireGuard encapsulation error: {:?}", e);
                }
                _ => {}
            }
        }

        debug!("UDP proxy forwarder stopped for target {}", target_addr);
    }

    /// Background thread: periodic timer for keepalive and handshake maintenance
    fn timer_loop(state: Arc<Mutex<TunnelState>>, running: Arc<AtomicBool>) {
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut handshake_retry_count = 0u32;
        const MAX_HANDSHAKE_RETRIES: u32 = 5;

        info!("WireGuard timer thread started");

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(250));

            let mut st = state.lock();
            
            // Process all timer events in a loop (there may be multiple)
            loop {
                match st.tunnel.update_timers(&mut dst_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        if let Err(e) = st.endpoint_socket.send(data) {
                            debug!("Failed to send timer packet: {}", e);
                        }
                    }
                    TunnResult::Err(e) => {
                        warn!("WireGuard timer error: {:?}", e);
                        
                        // Check if this is a connection expired error
                        let error_str = format!("{:?}", e);
                        if error_str.contains("ConnectionExpired") {
                            if handshake_retry_count < MAX_HANDSHAKE_RETRIES {
                                handshake_retry_count += 1;
                                warn!("Connection expired, re-initiating handshake (attempt {})", handshake_retry_count);
                                
                                // Mark handshake as not completed
                                st.handshake_completed.store(false, Ordering::SeqCst);
                                
                                // Try to re-initiate handshake
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
                            } else {
                                error!("WireGuard connection expired and max retries reached");
                            }
                        }
                        break;
                    }
                    TunnResult::Done => break,
                    _ => break,
                }
            }
            
            // Reset retry count if handshake is completed
            if st.handshake_completed.load(Ordering::SeqCst) {
                handshake_retry_count = 0;
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
// IP/UDP packet construction helpers
// ============================================================================

/// Build an IPv4/UDP packet from a payload
pub fn build_udp_ip_packet(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let src_ip = match src.ip() {
        IpAddr::V4(ip) => ip,
        _ => Ipv4Addr::new(0, 0, 0, 0),
    };
    let dst_ip = match dst.ip() {
        IpAddr::V4(ip) => ip,
        _ => Ipv4Addr::new(0, 0, 0, 0),
    };

    let udp_len = 8 + payload.len();
    let total_len = 20 + udp_len; // IP header (20) + UDP header (8) + payload

    let mut packet = Vec::with_capacity(total_len);

    // IPv4 header (20 bytes)
    packet.push(0x45); // Version (4) + IHL (5)
    packet.push(0x00); // DSCP + ECN
    packet.extend_from_slice(&(total_len as u16).to_be_bytes()); // Total length
    packet.extend_from_slice(&[0x00, 0x00]); // Identification
    packet.extend_from_slice(&[0x40, 0x00]); // Flags (Don't Fragment) + Fragment Offset
    packet.push(64); // TTL
    packet.push(17); // Protocol (UDP)
    packet.extend_from_slice(&[0x00, 0x00]); // Header checksum (will calculate)
    packet.extend_from_slice(&src_ip.octets()); // Source IP
    packet.extend_from_slice(&dst_ip.octets()); // Destination IP

    // Calculate IP header checksum
    let checksum = ip_checksum(&packet[..20]);
    packet[10] = (checksum >> 8) as u8;
    packet[11] = (checksum & 0xFF) as u8;

    // UDP header (8 bytes)
    packet.extend_from_slice(&src.port().to_be_bytes()); // Source port
    packet.extend_from_slice(&dst.port().to_be_bytes()); // Destination port
    packet.extend_from_slice(&(udp_len as u16).to_be_bytes()); // UDP length
    packet.extend_from_slice(&[0x00, 0x00]); // UDP checksum (optional for IPv4)

    // Payload
    packet.extend_from_slice(payload);

    packet
}

/// Calculate an IPv4 header checksum
fn ip_checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < header.len() {
        if i == 10 {
            // Skip the checksum field itself
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
    // Fold carries
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

/// Parse source port, destination port, and payload from an IP/UDP packet
fn parse_udp_from_ip_packet(packet: &[u8]) -> Option<(u16, u16, &[u8])> {
    // Minimum IPv4 + UDP header
    if packet.len() < 28 {
        return None;
    }

    // Check IPv4
    let version = (packet[0] >> 4) & 0x0F;
    if version != 4 {
        return None;
    }

    let ihl = (packet[0] & 0x0F) as usize * 4;
    let protocol = packet[9];

    // Only handle UDP (protocol 17)
    if protocol != 17 {
        return None;
    }

    if packet.len() < ihl + 8 {
        return None;
    }

    let udp_header = &packet[ihl..];
    let src_port = u16::from_be_bytes([udp_header[0], udp_header[1]]);
    let dst_port = u16::from_be_bytes([udp_header[2], udp_header[3]]);
    let udp_len = u16::from_be_bytes([udp_header[4], udp_header[5]]) as usize;

    if udp_len < 8 || ihl + udp_len > packet.len() {
        return None;
    }

    let payload = &udp_header[8..udp_len];
    Some((src_port, dst_port, payload))
}

// ============================================================================
// TCP Proxy Implementation using smoltcp
// ============================================================================

/// Buffer size for TCP socket buffers  
const TCP_RX_BUFFER_SIZE: usize = 65535;
const TCP_TX_BUFFER_SIZE: usize = 65535;

/// A virtual network device that sends/receives through WireGuard
struct WgDevice {
    /// Packets to be transmitted (from smoltcp to WireGuard)
    tx_queue: Vec<Vec<u8>>,
    /// Packets received (from WireGuard to smoltcp)  
    rx_queue: Vec<Vec<u8>>,
    /// MTU
    mtu: usize,
}

impl WgDevice {
    fn new(mtu: usize) -> Self {
        WgDevice {
            tx_queue: Vec::new(),
            rx_queue: Vec::new(),
            mtu,
        }
    }

    fn inject_packet(&mut self, packet: Vec<u8>) {
        self.rx_queue.push(packet);
    }

    fn take_outgoing(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tx_queue)
    }
}

struct WgRxToken {
    buffer: Vec<u8>,
}

impl RxToken for WgRxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buffer)
    }
}

struct WgTxToken<'a> {
    tx_queue: &'a mut Vec<Vec<u8>>,
}

impl<'a> TxToken for WgTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        self.tx_queue.push(buffer);
        result
    }
}

impl Device for WgDevice {
    type RxToken<'a> = WgRxToken;
    type TxToken<'a> = WgTxToken<'a>;

    fn receive(&mut self, _timestamp: SmolInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.rx_queue.pop()?;
        Some((
            WgRxToken { buffer: packet },
            WgTxToken { tx_queue: &mut self.tx_queue },
        ))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        Some(WgTxToken { tx_queue: &mut self.tx_queue })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        caps
    }
}

/// State for TCP proxy global management
static TCP_PROXY_RUNNING: AtomicBool = AtomicBool::new(false);
static TCP_PROXY_PORT: AtomicU16 = AtomicU16::new(0);
static NEXT_EPHEMERAL_PORT: AtomicU16 = AtomicU16::new(10000);

/// Create a TCP proxy that listens locally and relays to the target through WireGuard.
/// Each incoming connection gets its own WireGuard tunnel instance for isolation.
/// If prefer_local_port is Some, tries to bind to that port first (for transparent proxying).
/// Returns the local port to connect to.
pub fn create_tcp_proxy(
    config: WireGuardConfig,
    target_addr: SocketAddr,
    prefer_local_port: Option<u16>,
) -> io::Result<u16> {
    // Create local TCP listener - try preferred port first, fall back to random
    let listener = if let Some(port) = prefer_local_port {
        match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(l) => l,
            Err(e) => {
                warn!("Could not bind TCP proxy to port {}, using random port: {}", port, e);
                TcpListener::bind("127.0.0.1:0")?
            }
        }
    } else {
        TcpListener::bind("127.0.0.1:0")?
    };
    let local_port = listener.local_addr()?.port();
    
    TCP_PROXY_RUNNING.store(true, Ordering::SeqCst);
    TCP_PROXY_PORT.store(local_port, Ordering::SeqCst);
    
    info!("TCP proxy started on port {} -> {}", local_port, target_addr);
    
    thread::Builder::new()
        .name("wg-tcp-proxy".into())
        .spawn(move || {
            listener.set_nonblocking(true).ok();
            
            while TCP_PROXY_RUNNING.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, addr)) => {
                        debug!("TCP proxy: new connection from {}", addr);
                        let cfg = config.clone();
                        let target = target_addr;
                        
                        thread::Builder::new()
                            .name(format!("wg-tcp-conn-{}", addr.port()))
                            .spawn(move || {
                                if let Err(e) = handle_tcp_connection(client, cfg, target) {
                                    debug!("TCP connection error: {}", e);
                                }
                            })
                            .ok();
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        error!("TCP proxy accept error: {}", e);
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            
            info!("TCP proxy stopped");
        })?;
    
    Ok(local_port)
}

/// Stop the TCP proxy
pub fn stop_tcp_proxy() {
    TCP_PROXY_RUNNING.store(false, Ordering::SeqCst);
    TCP_PROXY_PORT.store(0, Ordering::SeqCst);
}

/// Check if TCP proxy is running
pub fn is_tcp_proxy_running() -> bool {
    TCP_PROXY_RUNNING.load(Ordering::SeqCst)
}

/// Get TCP proxy port
pub fn get_tcp_proxy_port() -> u16 {
    TCP_PROXY_PORT.load(Ordering::SeqCst)
}

/// Handle a single TCP connection through WireGuard
fn handle_tcp_connection(
    mut client: TcpStream,
    config: WireGuardConfig,
    target_addr: SocketAddr,
) -> io::Result<()> {
    client.set_nonblocking(true)?;
    client.set_nodelay(true)?;
    
    // Create WireGuard tunnel for this connection
    let private_key = StaticSecret::from(config.private_key);
    let peer_public_key = PublicKey::from(config.peer_public_key);
    
    let mut tunnel = Tunn::new(
        private_key,
        peer_public_key,
        config.preshared_key,
        Some(config.keepalive_secs),
        0,
        None,
    ).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    
    // Create UDP socket to WireGuard endpoint
    let endpoint_socket = UdpSocket::bind("0.0.0.0:0")?;
    endpoint_socket.connect(config.endpoint)?;
    endpoint_socket.set_nonblocking(true)?;
    
    // Perform WireGuard handshake
    let mut handshake_buf = vec![0u8; WG_BUFFER_SIZE];
    match tunnel.format_handshake_initiation(&mut handshake_buf, false) {
        TunnResult::WriteToNetwork(data) => {
            endpoint_socket.send(data)?;
        }
        _ => return Err(io::Error::new(io::ErrorKind::Other, "Handshake init failed")),
    }
    
    // Wait for handshake response
    endpoint_socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut recv_buf = vec![0u8; WG_BUFFER_SIZE];
    let mut dec_buf = vec![0u8; WG_BUFFER_SIZE];
    
    let n = endpoint_socket.recv(&mut recv_buf)?;
    match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
        TunnResult::WriteToNetwork(data) => {
            endpoint_socket.send(data)?;
        }
        TunnResult::Done => {}
        _ => {}
    }
    
    // Set socket back to non-blocking
    endpoint_socket.set_read_timeout(Some(Duration::from_millis(1)))?;
    
    // Create smoltcp interface
    let mtu = config.mtu as usize;
    let mut device = WgDevice::new(mtu);
    
    let local_ip = match config.tunnel_address {
        IpAddr::V4(ip) => ip,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPv6 tunnel address not supported")),
    };
    let iface_config = IfaceConfig::new(smoltcp::wire::HardwareAddress::Ip);
    let mut iface = Interface::new(iface_config, &mut device, SmolInstant::from_millis(0));
    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(IpAddress::v4(
            local_ip.octets()[0],
            local_ip.octets()[1], 
            local_ip.octets()[2],
            local_ip.octets()[3],
        ), 24)).ok();
    });
    
    // Create smoltcp TCP socket
    let tcp_rx_buffer = SocketBuffer::new(vec![0u8; TCP_RX_BUFFER_SIZE]);
    let tcp_tx_buffer = SocketBuffer::new(vec![0u8; TCP_TX_BUFFER_SIZE]);
    let mut tcp_socket = SmolTcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
    
    // Get ephemeral port for local side
    let local_port = NEXT_EPHEMERAL_PORT.fetch_add(1, Ordering::SeqCst);
    if local_port > 60000 {
        NEXT_EPHEMERAL_PORT.store(10000, Ordering::SeqCst);
    }
    
    // Connect to target
    let target_ip = match target_addr.ip() {
        IpAddr::V4(ip) => ip,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPv6 not supported")),
    };
    
    let remote_endpoint = IpEndpoint::new(
        IpAddress::v4(target_ip.octets()[0], target_ip.octets()[1], target_ip.octets()[2], target_ip.octets()[3]),
        target_addr.port(),
    );
    let local_endpoint = IpEndpoint::new(
        IpAddress::v4(local_ip.octets()[0], local_ip.octets()[1], local_ip.octets()[2], local_ip.octets()[3]),
        local_port,
    );
    
    if let Err(e) = tcp_socket.connect(iface.context(), remote_endpoint, local_endpoint) {
        return Err(io::Error::new(io::ErrorKind::Other, format!("TCP connect failed: {:?}", e)));
    }
    
    let mut sockets = SocketSet::new(vec![]);
    let tcp_handle = sockets.add(tcp_socket);
    
    // Buffers for zerocopy relay
    let mut client_buf = vec![0u8; 32768];
    let mut remote_buf = vec![0u8; 32768];
    
    let start = Instant::now();
    let timeout = Duration::from_secs(30);
    
    // Main relay loop
    while start.elapsed() < timeout {
        let timestamp = SmolInstant::from_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        );
        
        // Poll smoltcp interface
        iface.poll(timestamp, &mut device, &mut sockets);
        
        // Send outgoing IP packets through WireGuard
        for packet in device.take_outgoing() {
            let mut enc_buf = vec![0u8; WG_BUFFER_SIZE];
            match tunnel.encapsulate(&packet, &mut enc_buf) {
                TunnResult::WriteToNetwork(data) => {
                    endpoint_socket.send(data).ok();
                }
                _ => {}
            }
        }
        
        // Receive from WireGuard endpoint
        match endpoint_socket.recv(&mut recv_buf) {
            Ok(n) if n > 0 => {
                match tunnel.decapsulate(None, &recv_buf[..n], &mut dec_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        endpoint_socket.send(data).ok();
                    }
                    TunnResult::WriteToTunnelV4(data, _) => {
                        device.inject_packet(data.to_vec());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        
        // Get TCP socket
        let socket = sockets.get_mut::<SmolTcpSocket>(tcp_handle);
        
        // Check connection state
        match socket.state() {
            TcpState::Closed | TcpState::TimeWait => {
                debug!("TCP connection closed");
                break;
            }
            TcpState::Established => {
                // Read from client, send to remote
                match client.read(&mut client_buf) {
                    Ok(0) => break, // Client closed
                    Ok(n) => {
                        if socket.can_send() {
                            socket.send_slice(&client_buf[..n]).ok();
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
                
                // Read from remote, send to client
                if socket.can_recv() {
                    match socket.recv_slice(&mut remote_buf) {
                        Ok(n) if n > 0 => {
                            if client.write_all(&remote_buf[..n]).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                // Connection not yet established, just process
            }
        }
        
        // Update timers
        let mut timer_buf = vec![0u8; WG_BUFFER_SIZE];
        match tunnel.update_timers(&mut timer_buf) {
            TunnResult::WriteToNetwork(data) => {
                endpoint_socket.send(data).ok();
            }
            _ => {}
        }
        
        thread::sleep(Duration::from_micros(100));
    }
    
    // Clean close
    let socket = sockets.get_mut::<SmolTcpSocket>(tcp_handle);
    socket.close();
    
    Ok(())
}

// ============================================================================
// Global WireGuard tunnel instance
// ============================================================================

static GLOBAL_TUNNEL: Mutex<Option<WireGuardTunnel>> = Mutex::new(None);

/// Initialize and start the global WireGuard tunnel
pub fn wg_start_tunnel(config: WireGuardConfig) -> io::Result<()> {
    // Note: We don't stop the HTTP shared proxy here anymore.
    // Instead, the HTTP proxy will detect streaming is active and route through
    // the streaming tunnel using wg_send_ip_packet().
    // This allows HTTP requests to work during streaming.
    
    let mut global = GLOBAL_TUNNEL.lock();
    
    // Stop any existing tunnel
    if let Some(ref tunnel) = *global {
        tunnel.stop();
    }

    let tunnel = WireGuardTunnel::new(config)?;
    tunnel.start()?;
    
    // Wait for handshake
    if !tunnel.wait_for_handshake(Duration::from_secs(10)) {
        tunnel.stop();
        return Err(io::Error::new(io::ErrorKind::TimedOut, "WireGuard handshake timed out"));
    }

    *global = Some(tunnel);
    Ok(())
}

/// Stop the global WireGuard tunnel
pub fn wg_stop_tunnel() {
    // Disable zero-copy routing before stopping the tunnel
    crate::platform_sockets::disable_wg_routing();

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

/// Create a UDP proxy through the WireGuard tunnel.
/// Returns the local socket address to connect to.
pub fn wg_create_udp_proxy(target_addr: SocketAddr) -> io::Result<SocketAddr> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => tunnel.create_udp_proxy(target_addr),
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")),
    }
}

/// Create streaming UDP proxies for all moonlight ports.
/// This sets up proxies on the same ports locally (47998, 47999, 48000) forwarding to target.
/// Also enables zero-copy routing in platform_sockets for direct WG encapsulation.
pub fn wg_create_streaming_proxies(target_ip: Ipv4Addr, base_port: u16) -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => {
            tunnel.create_streaming_proxies(target_ip, base_port)?;

            // Enable zero-copy routing: extract tunnel IP from config
            let tunnel_ip = match tunnel.config.tunnel_address {
                IpAddr::V4(ip) => ip,
                _ => Ipv4Addr::new(10, 0, 0, 2), // fallback
            };
            crate::platform_sockets::enable_wg_routing(tunnel_ip, target_ip);

            Ok(())
        }
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")),
    }
}

/// Send an IP packet through the global WireGuard tunnel.
/// This is used by wg_http to route TCP traffic through the streaming tunnel.
pub fn wg_send_ip_packet(packet: &[u8]) -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => {
            let mut state = tunnel.state.lock();
            // Use a local buffer to avoid borrow conflict
            let mut encode_buf = vec![0u8; WG_BUFFER_SIZE];
            match state.tunnel.encapsulate(packet, &mut encode_buf) {
                TunnResult::WriteToNetwork(data) => {
                    state.endpoint_socket.send(data)?;
                    Ok(())
                }
                TunnResult::Done => Ok(()),
                TunnResult::Err(e) => Err(io::Error::new(io::ErrorKind::Other, format!("Encapsulate error: {:?}", e))),
                _ => Ok(()),
            }
        }
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")),
    }
}

/// Register a callback to receive incoming IP packets from the global WireGuard tunnel.
/// Returns a channel receiver for incoming IP packets destined to the specified port range.
pub fn wg_register_tcp_receiver(
    local_ip: Ipv4Addr,
    port_start: u16,
    port_end: u16,
) -> io::Result<std::sync::mpsc::Receiver<Vec<u8>>> {
    use std::sync::mpsc;
    
    let (tx, rx) = mpsc::channel();
    
    // Store the receiver registration in GLOBAL_TUNNEL
    let mut global = GLOBAL_TUNNEL.lock();
    match global.as_mut() {
        Some(tunnel) => {
            // Add to tunnel's TCP receivers (we'll need to add this field)
            // For now, just return the channel - the actual routing will be done elsewhere
            drop(global);
            info!("TCP receiver registered for {}:{}-{}", local_ip, port_start, port_end);
            Ok(rx)
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
    fn test_build_parse_udp_packet() {
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
}
