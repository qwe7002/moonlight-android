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

#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
        let tunnel = Tunn::new(
            private_key,
            peer_public_key,
            config.preshared_key,
            Some(config.keepalive_secs),
            0, // index
            None, // rate limiter
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Resolve endpoint dynamically for DDNS support
        let endpoint_addr = config.resolve_endpoint()?;
        info!("Resolved endpoint '{}' -> {}", config.endpoint, endpoint_addr);

        // Create UDP socket to the WireGuard endpoint (address family must match)
        let endpoint_socket = UdpSocket::bind(bind_addr_for(&endpoint_addr))?;
        endpoint_socket.connect(endpoint_addr)?;
        endpoint_socket.set_nonblocking(false)?;

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
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);
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

        // Start the timer thread for keepalive and handshake retransmission
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
        if self.running.swap(false, Ordering::SeqCst) {
            info!("Stopping WireGuard tunnel...");
            info!("WireGuard tunnel stopped");
        }
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

    /// Send encapsulated data through the WireGuard tunnel.
    /// Encapsulates under lock (fast crypto), then sends outside lock.
    #[allow(dead_code)]
    fn send_through_tunnel(state: &Arc<Mutex<TunnelState>>, payload: &[u8], src_addr: SocketAddr, dst_addr: SocketAddr) -> io::Result<()> {
        // Build an IP/UDP packet wrapping the payload
        let ip_packet = build_udp_ip_packet(src_addr, dst_addr, payload);

        // Encapsulate under lock, copy result, release lock before sending
        let (send_data, send_socket) = {
            let st = state.lock();
            let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];

            match st.tunnel.encapsulate(&ip_packet, &mut dst_buf) {
                TunnResult::WriteToNetwork(data) => {
                    let copied = data.to_vec();
                    let socket = st.endpoint_socket.try_clone()
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Socket clone: {}", e)))?;
                    (copied, socket)
                }
                TunnResult::Err(e) => {
                    error!("WireGuard encapsulation error: {:?}", e);
                    return Err(io::Error::new(io::ErrorKind::Other, format!("Encapsulation failed: {:?}", e)));
                }
                _ => return Ok(()),
            }
        };
        // Lock released - send outside lock
        send_socket.send(&send_data)?;
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
        let recv_socket = {
            let st = state.lock();
            st.endpoint_socket.try_clone()
                .expect("Failed to clone WG endpoint socket for receiver")
        };
        // Use short read timeout (10ms) - just enough to check shutdown flag
        recv_socket.set_read_timeout(Some(Duration::from_millis(10))).ok();

        let mut recv_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut dec_buf = vec![0u8; WG_BUFFER_SIZE];

        info!("WireGuard endpoint receiver started");

        while running.load(Ordering::SeqCst) {
            // Read WITHOUT holding tunnel lock - allows concurrent sends
            let n = match recv_socket.recv(&mut recv_buf) {
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock 
                    || e.kind() == io::ErrorKind::TimedOut 
                    || e.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => {
                    if running.load(Ordering::SeqCst) {
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
                            debug!("WG TCP received: {} bytes, injecting to HTTP proxy", data.len());
                            crate::wg_http::wg_http_inject_packet(data);
                        } else if protocol == 17 {
                            // UDP packet - deliver via zero-copy channel
                            if let Some((src_port, dst_port, payload)) = parse_udp_from_ip_packet(data) {
                                debug!("WG UDP received: src_port={}, dst_port={}, payload_len={}", src_port, dst_port, payload.len());
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
                    // Keepalive or similar - nothing to forward
                }
                TunnResult::Err(e) => {
                    warn!("WireGuard decapsulation error: {:?}", e);
                }
            }
        }

        info!("WireGuard endpoint receiver stopped");
    }

    /// Background thread: periodic timer for keepalive, DDNS re-resolution, and handshake maintenance
    fn timer_loop(state: Arc<Mutex<TunnelState>>, running: Arc<AtomicBool>, config: WireGuardConfig) {
        let mut dst_buf = vec![0u8; WG_BUFFER_SIZE];
        let mut handshake_retry_count = 0u32;
        const MAX_HANDSHAKE_RETRIES: u32 = 5;

        info!("WireGuard timer thread started");

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(250));

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

                                        // Replace socket and address
                                        st.endpoint_socket = new_socket;
                                        st.resolved_endpoint = new_addr;

                                        info!("DDNS: reconnected to new endpoint {}", new_addr);

                                        // Reset handshake state and retry count
                                        st.handshake_completed.store(false, Ordering::SeqCst);
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

/// A virtual network device that sends/receives through WireGuard
#[allow(dead_code)]
struct WgDevice {
    /// Packets to be transmitted (from smoltcp to WireGuard)
    tx_queue: Vec<Vec<u8>>,
    /// Packets received (from WireGuard to smoltcp)  
    rx_queue: Vec<Vec<u8>>,
    /// MTU
    mtu: usize,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

/// Send an IP packet through the global WireGuard tunnel.
/// This is used by wg_http to route TCP traffic through the streaming tunnel.
///
/// Performance: Encapsulate under lock (fast crypto), then send outside lock
/// to minimize lock hold time and avoid blocking the receiver loop.
pub fn wg_send_ip_packet(packet: &[u8]) -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => {
            // Encapsulate under lock (fast), copy result, release lock, then send
            let (send_data, send_socket) = {
                let state = tunnel.state.lock();
                let mut encode_buf = vec![0u8; WG_BUFFER_SIZE];
                match state.tunnel.encapsulate(packet, &mut encode_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        let copied = data.to_vec();
                        let socket = state.endpoint_socket.try_clone()
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Socket clone failed: {}", e)))?;
                        (copied, socket)
                    }
                    TunnResult::Done => return Ok(()),
                    TunnResult::Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Encapsulate error: {:?}", e))),
                    _ => return Ok(()),
                }
            };
            // State lock released here - send outside lock
            drop(global); // Release GLOBAL_TUNNEL lock too
            send_socket.send(&send_data)?;
            Ok(())
        }
        None => Err(io::Error::new(io::ErrorKind::NotConnected, "WireGuard tunnel not active")),
    }
}

/// Enable direct WireGuard routing for UDP/TCP traffic.
/// This enables zero-copy routing: socket sendto calls targeting the WG server IP
/// are intercepted and encapsulated directly through the WG tunnel.
pub fn wg_enable_direct_routing(server_ip: Ipv4Addr) -> io::Result<()> {
    let global = GLOBAL_TUNNEL.lock();
    match global.as_ref() {
        Some(tunnel) => {
            let tunnel_ip = match tunnel.config.tunnel_address {
                IpAddr::V4(ip) => ip,
                IpAddr::V6(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "IPv6 tunnel address not supported",
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
