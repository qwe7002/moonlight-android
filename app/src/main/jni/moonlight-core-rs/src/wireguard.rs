//! WireGuard tunnel module using boringtun
//!
//! This module provides a userspace WireGuard tunnel that allows moonlight streaming
//! traffic to be routed through a WireGuard VPN without requiring a system TUN device.
//!
//! Architecture:
//! - Uses boringtun for WireGuard protocol (Noise handshake, encryption/decryption)
//! - Creates a real UDP socket to the WireGuard peer endpoint
//! - Provides UDP proxy sockets that tunnel traffic through WireGuard
//! - All moonlight streaming traffic (video, audio, control) goes through the tunnel

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::collections::HashMap;

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
        thread::Builder::new()
            .name(format!("wg-udp-fwd-{}", local_addr.port()))
            .spawn(move || {
                Self::udp_proxy_forward_loop(state, running, proxy_recv, target);
            })?;

        // Register the proxy for reverse traffic (WireGuard -> local)
        self.udp_proxies.lock().insert(target_addr.port(), UdpProxyEntry {
            proxy_socket,
            target_addr,
            local_addr,
        });

        Ok(local_addr)
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

                    if let Some((src_port, dst_port, payload)) = parse_udp_from_ip_packet(data) {
                        // Find the proxy for this source port (the remote server port)
                        let proxies = udp_proxies.lock();
                        if let Some(proxy) = proxies.get(&src_port) {
                            // Send decapsulated data back to moonlight-common-c via the local proxy
                            if let Err(e) = proxy.proxy_socket.send_to(payload, proxy.local_addr) {
                                debug!("Failed to forward decapsulated packet to proxy: {}", e);
                            }
                        } else {
                            debug!("No proxy found for source port {}", src_port);
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

        info!("WireGuard timer thread started");

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(250));

            let mut st = state.lock();
            match st.tunnel.update_timers(&mut dst_buf) {
                TunnResult::WriteToNetwork(data) => {
                    if let Err(e) = st.endpoint_socket.send(data) {
                        debug!("Failed to send timer packet: {}", e);
                    }
                }
                TunnResult::Err(e) => {
                    warn!("WireGuard timer error: {:?}", e);
                }
                TunnResult::Done => {}
                _ => {}
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
fn build_udp_ip_packet(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
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
// Global WireGuard tunnel instance
// ============================================================================

static GLOBAL_TUNNEL: Mutex<Option<WireGuardTunnel>> = Mutex::new(None);

/// Initialize and start the global WireGuard tunnel
pub fn wg_start_tunnel(config: WireGuardConfig) -> io::Result<()> {
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
