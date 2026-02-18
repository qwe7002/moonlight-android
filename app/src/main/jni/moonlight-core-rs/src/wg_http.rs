//! Direct HTTP client through WireGuard tunnel
//!
//! This module provides a simple HTTP client that makes requests directly
//! through the WireGuard tunnel without going through OkHttp.
//! It also provides TCP proxy functionality for HTTPS traffic.
//!
//! Architecture:
//! - HTTP GET: per-request WireGuard tunnel with smoltcp TCP/IP stack
//! - TCP proxy: single shared WireGuard tunnel with manual TCP/IP stack
//!   (VirtualStack from tun_stack module, based on ssserver-wg's proven approach)
//!   This avoids smoltcp SYN-ACK processing issues and WG peer endpoint conflicts

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use parking_lot::Mutex;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer, State as TcpState};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{IpAddress, IpCidr};

use boringtun::noise::{Tunn, TunnResult};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::tun_stack::{VirtualStack, TcpState as VirtualTcpState};

/// Maximum packet size for WireGuard
const MAX_PACKET_SIZE: usize = 65535;

/// TCP buffer sizes
const TCP_RX_BUFFER_SIZE: usize = 65535;
const TCP_TX_BUFFER_SIZE: usize = 65535;

/// HTTP response buffer size
const HTTP_RESPONSE_BUFFER_SIZE: usize = 262144; // 256KB

/// Connection timeout
const CONNECTION_TIMEOUT_SECS: u64 = 10;

/// Read timeout
const READ_TIMEOUT_SECS: u64 = 15;

/// WireGuard HTTP client configuration
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

/// HTTP response from the WireGuard tunnel
#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: String,
}

/// Virtual network device for smoltcp over WireGuard
struct WgDevice {
    tx_queue: Arc<Mutex<Vec<Vec<u8>>>>,
    rx_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    mtu: usize,
}

impl WgDevice {
    fn new(mtu: usize) -> Self {
        WgDevice {
            tx_queue: Arc::new(Mutex::new(Vec::new())),
            rx_queue: Arc::new(Mutex::new(VecDeque::new())),
            mtu,
        }
    }

    fn inject_packet(&self, packet: Vec<u8>) {
        self.rx_queue.lock().push_back(packet);
    }

    fn take_outgoing(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.tx_queue.lock())
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

struct WgTxToken {
    tx_queue: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl TxToken for WgTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        self.tx_queue.lock().push(buffer);
        result
    }
}

impl Device for WgDevice {
    type RxToken<'a> = WgRxToken;
    type TxToken<'a> = WgTxToken;

    fn receive(&mut self, _timestamp: SmolInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.rx_queue.lock().pop_front()?;
        Some((
            WgRxToken { buffer: packet },
            WgTxToken { tx_queue: self.tx_queue.clone() },
        ))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        Some(WgTxToken { tx_queue: self.tx_queue.clone() })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        caps
    }
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

/// Make an HTTP GET request through the WireGuard tunnel
pub fn http_get(config: &WgHttpConfig, host: &str, port: u16, path: &str, https: bool) -> io::Result<HttpResponse> {
    debug!("WG HTTP GET: {}:{}{} (https={})", host, port, path, https);

    // For now, only support HTTP (not HTTPS) - HTTPS would require rustls
    if https {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "HTTPS not yet supported in direct WG HTTP client",
        ));
    }

    // Create WireGuard tunnel
    let (mut tunnel, endpoint_socket) = create_tunnel(config)?;

    // Perform handshake
    do_handshake(&mut tunnel, &endpoint_socket)?;
    debug!("WireGuard handshake completed for HTTP request");

    // Flush any pending timer events after handshake
    {
        let mut timer_buf = vec![0u8; MAX_PACKET_SIZE];
        match tunnel.update_timers(&mut timer_buf) {
            TunnResult::WriteToNetwork(data) => {
                debug!("WG HTTP: post-handshake timer flush ({} bytes)", data.len());
                endpoint_socket.send(data).ok();
            }
            _ => {}
        }
    }

    // Set up smoltcp
    let tunnel_ip = config.tunnel_ip;
    let server_ip = config.server_ip;
    let mtu = config.mtu as usize;
    let mut device = WgDevice::new(mtu);

    let iface_config = Config::new(smoltcp::wire::HardwareAddress::Ip);
    let mut iface = Interface::new(iface_config, &mut device, SmolInstant::now());
    iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::new(
                IpAddress::v4(
                    tunnel_ip.octets()[0],
                    tunnel_ip.octets()[1],
                    tunnel_ip.octets()[2],
                    tunnel_ip.octets()[3],
                ),
                0,
            ))
            .ok();
    });

    // Create TCP socket
    let rx_buffer = SocketBuffer::new(vec![0u8; TCP_RX_BUFFER_SIZE]);
    let tx_buffer = SocketBuffer::new(vec![0u8; TCP_TX_BUFFER_SIZE]);
    let mut tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);

    // Connect to target
    let remote_endpoint = smoltcp::wire::IpEndpoint::new(
        IpAddress::v4(
            server_ip.octets()[0],
            server_ip.octets()[1],
            server_ip.octets()[2],
            server_ip.octets()[3],
        ),
        port,
    );

    // Use random ephemeral port
    let local_port = (std::process::id() as u16 % 10000) + 50000;
    let local_endpoint = smoltcp::wire::IpEndpoint::new(
        IpAddress::v4(
            tunnel_ip.octets()[0],
            tunnel_ip.octets()[1],
            tunnel_ip.octets()[2],
            tunnel_ip.octets()[3],
        ),
        local_port,
    );

    tcp_socket
        .connect(iface.context(), remote_endpoint, local_endpoint)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("TCP connect failed: {:?}", e)))?;

    let mut sockets = SocketSet::new(vec![]);
    let tcp_handle = sockets.add(tcp_socket);

    // Buffers
    let mut wg_send_buf = vec![0u8; MAX_PACKET_SIZE];
    let mut wg_recv_buf = vec![0u8; MAX_PACKET_SIZE];
    let mut dec_buf = vec![0u8; MAX_PACKET_SIZE];

    endpoint_socket.set_nonblocking(true)?;

    // Wait for TCP connection
    let start = Instant::now();
    let timeout = Duration::from_secs(CONNECTION_TIMEOUT_SECS);
    let mut last_tcp_state = TcpState::Closed;

    loop {
        let now = SmolInstant::now();
        iface.poll(now, &mut device, &mut sockets);

        // Send outgoing packets through WireGuard
        let outgoing = device.take_outgoing();
        for packet in &outgoing {
            debug!("WG HTTP: sending {} byte IP packet through tunnel", packet.len());
            match tunnel.encapsulate(packet, &mut wg_send_buf) {
                TunnResult::WriteToNetwork(data) => {
                    if let Err(e) = endpoint_socket.send(data) {
                        warn!("WG HTTP: send encapsulated packet failed: {}", e);
                    }
                }
                TunnResult::Err(e) => {
                    warn!("WG HTTP: encapsulate error: {:?}", e);
                }
                _ => {
                    warn!("WG HTTP: encapsulate returned unexpected result");
                }
            }
        }

        // Receive all available packets from WireGuard
        let mut received_packets = false;
        loop {
            match endpoint_socket.recv(&mut wg_recv_buf) {
                Ok(n) => {
                    debug!("WG HTTP: received {} bytes from WG endpoint", n);
                    match tunnel.decapsulate(None, &wg_recv_buf[..n], &mut dec_buf) {
                        TunnResult::WriteToTunnelV4(data, _) => {
                            debug!("WG HTTP: decapsulated {} byte IPv4 packet", data.len());
                            device.inject_packet(data.to_vec());
                            received_packets = true;
                        }
                        TunnResult::WriteToTunnelV6(data, _) => {
                            debug!("WG HTTP: decapsulated {} byte IPv6 packet", data.len());
                            device.inject_packet(data.to_vec());
                            received_packets = true;
                        }
                        TunnResult::WriteToNetwork(data) => {
                            debug!("WG HTTP: decapsulate returned WriteToNetwork ({} bytes)", data.len());
                            endpoint_socket.send(data).ok();
                        }
                        TunnResult::Err(e) => {
                            warn!("WG HTTP: decapsulate error: {:?}", e);
                        }
                        TunnResult::Done => {
                            debug!("WG HTTP: decapsulate returned Done");
                        }
                    }
                    // Drain follow-up results from decapsulate
                    loop {
                        match tunnel.decapsulate(None, &[], &mut dec_buf) {
                            TunnResult::WriteToTunnelV4(data, _) => {
                                debug!("WG HTTP: drain: decapsulated {} byte IPv4 packet", data.len());
                                device.inject_packet(data.to_vec());
                                received_packets = true;
                            }
                            TunnResult::WriteToTunnelV6(data, _) => {
                                device.inject_packet(data.to_vec());
                                received_packets = true;
                            }
                            TunnResult::WriteToNetwork(data) => {
                                endpoint_socket.send(data).ok();
                            }
                            _ => break,
                        }
                    }
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        // Handle timer events
        match tunnel.update_timers(&mut wg_send_buf) {
            TunnResult::WriteToNetwork(data) => {
                endpoint_socket.send(data).ok();
            }
            TunnResult::Err(e) => {
                warn!("WG HTTP: update_timers error: {:?}", e);
            }
            _ => {}
        }

        // Re-poll smoltcp after injecting received packets so they are processed immediately
        if received_packets {
            let now = SmolInstant::now();
            iface.poll(now, &mut device, &mut sockets);
            // Send any response packets generated by the re-poll (e.g., TCP ACK for SYN-ACK)
            for packet in device.take_outgoing() {
                match tunnel.encapsulate(&packet, &mut wg_send_buf) {
                    TunnResult::WriteToNetwork(data) => {
                        endpoint_socket.send(data).ok();
                    }
                    _ => {}
                }
            }
        }

        let socket = sockets.get_mut::<TcpSocket>(tcp_handle);
        let current_state = socket.state();
        if current_state != last_tcp_state {
            debug!("WG HTTP: TCP state: {:?} -> {:?}", last_tcp_state, current_state);
            last_tcp_state = current_state;
        }

        if current_state == TcpState::Established {
            break;
        }

        if current_state == TcpState::Closed {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "Connection refused",
            ));
        }

        if start.elapsed() > timeout {
            warn!("WG HTTP: TCP connection timeout after {:?}, state: {:?}", start.elapsed(), current_state);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Connection timeout",
            ));
        }

        thread::sleep(Duration::from_millis(1));
    }

    debug!("TCP connection established for HTTP request");

    // Send HTTP request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: Moonlight-Android\r\n\r\n",
        path, host
    );

    let socket = sockets.get_mut::<TcpSocket>(tcp_handle);
    socket
        .send_slice(request.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Send failed: {:?}", e)))?;

    // Read response
    let mut response_data = Vec::with_capacity(HTTP_RESPONSE_BUFFER_SIZE);
    let read_start = Instant::now();
    let read_timeout = Duration::from_secs(READ_TIMEOUT_SECS);

    loop {
        let now = SmolInstant::now();
        iface.poll(now, &mut device, &mut sockets);

        // Send outgoing packets
        for packet in device.take_outgoing() {
            if let TunnResult::WriteToNetwork(data) = tunnel.encapsulate(&packet, &mut wg_send_buf)
            {
                endpoint_socket.send(data).ok();
            }
        }

        // Receive packets
        let mut received_packets = false;
        loop {
            match endpoint_socket.recv(&mut wg_recv_buf) {
                Ok(n) => {
                    match tunnel.decapsulate(None, &wg_recv_buf[..n], &mut dec_buf) {
                        TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                            device.inject_packet(data.to_vec());
                            received_packets = true;
                        }
                        TunnResult::WriteToNetwork(data) => {
                            endpoint_socket.send(data).ok();
                        }
                        _ => {}
                    }
                    // Drain follow-up results
                    loop {
                        match tunnel.decapsulate(None, &[], &mut dec_buf) {
                            TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                                device.inject_packet(data.to_vec());
                                received_packets = true;
                            }
                            TunnResult::WriteToNetwork(data) => {
                                endpoint_socket.send(data).ok();
                            }
                            _ => break,
                        }
                    }
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        // Handle timer events
        if let TunnResult::WriteToNetwork(data) = tunnel.update_timers(&mut wg_send_buf) {
            endpoint_socket.send(data).ok();
        }

        // Re-poll after receiving to process injected packets immediately
        if received_packets {
            let now = SmolInstant::now();
            iface.poll(now, &mut device, &mut sockets);
            for packet in device.take_outgoing() {
                if let TunnResult::WriteToNetwork(data) = tunnel.encapsulate(&packet, &mut wg_send_buf) {
                    endpoint_socket.send(data).ok();
                }
            }
        }

        let socket = sockets.get_mut::<TcpSocket>(tcp_handle);

        // Read available data
        if socket.can_recv() {
            socket
                .recv(|data| {
                    response_data.extend_from_slice(data);
                    (data.len(), ())
                })
                .ok();
        }

        // Check if connection closed (server sent all data)
        if socket.state() == TcpState::CloseWait
            || socket.state() == TcpState::Closed
            || socket.state() == TcpState::Closing
        {
            break;
        }

        // Check for complete HTTP response (Content-Length or chunked)
        if is_http_response_complete(&response_data) {
            break;
        }

        if read_start.elapsed() > read_timeout {
            if !response_data.is_empty() {
                break; // Return partial response
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Read timeout"));
        }

        thread::sleep(Duration::from_micros(100));
    }

    // Close connection gracefully
    let socket = sockets.get_mut::<TcpSocket>(tcp_handle);
    socket.close();

    // Poll to send FIN
    for _ in 0..5 {
        let now = SmolInstant::now();
        iface.poll(now, &mut device, &mut sockets);
        for packet in device.take_outgoing() {
            if let TunnResult::WriteToNetwork(data) = tunnel.encapsulate(&packet, &mut wg_send_buf)
            {
                endpoint_socket.send(data).ok();
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    // Parse HTTP response
    parse_http_response(&response_data)
}

/// Check if HTTP response is complete
fn is_http_response_complete(data: &[u8]) -> bool {
    // Find header end
    let header_end = match find_header_end(data) {
        Some(pos) => pos,
        None => return false,
    };

    let headers = match std::str::from_utf8(&data[..header_end]) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Check for Connection: close (response ends when server closes)
    if headers.to_lowercase().contains("connection: close") {
        return false; // Wait for server to close
    }

    // Check Content-Length
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(len_str) = line.split(':').nth(1) {
                if let Ok(content_length) = len_str.trim().parse::<usize>() {
                    let body_start = header_end + 4; // After \r\n\r\n
                    return data.len() >= body_start + content_length;
                }
            }
        }
    }

    // Check for chunked encoding - look for final chunk
    if headers.to_lowercase().contains("transfer-encoding: chunked") {
        // Look for 0\r\n\r\n which ends chunked response
        let body = &data[header_end + 4..];
        return body.windows(5).any(|w| w == b"0\r\n\r\n");
    }

    false
}

/// Find the end of HTTP headers
fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4)
        .position(|window| window == b"\r\n\r\n")
}

/// Parse HTTP response
fn parse_http_response(data: &[u8]) -> io::Result<HttpResponse> {
    let response_str = String::from_utf8_lossy(data);

    // Find status line
    let first_line = response_str
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty response"))?;

    // Parse status code (e.g., "HTTP/1.1 200 OK")
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid status line",
        ));
    }

    let status_code: u16 = parts[1]
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid status code"))?;

    // Find body
    let body = if let Some(pos) = find_header_end(data) {
        let body_start = pos + 4;
        if body_start < data.len() {
            String::from_utf8_lossy(&data[body_start..]).to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Handle chunked encoding
    let body = if response_str.to_lowercase().contains("transfer-encoding: chunked") {
        decode_chunked_body(&body)
    } else {
        body
    };

    Ok(HttpResponse { status_code, body })
}

/// Decode chunked transfer encoding
fn decode_chunked_body(body: &str) -> String {
    let mut result = String::new();
    let mut remaining = body;

    loop {
        // Find chunk size line
        let size_end = match remaining.find("\r\n") {
            Some(pos) => pos,
            None => break,
        };

        let size_str = &remaining[..size_end];
        let chunk_size = match usize::from_str_radix(size_str.trim(), 16) {
            Ok(s) => s,
            Err(_) => break,
        };

        if chunk_size == 0 {
            break;
        }

        // Get chunk data
        let data_start = size_end + 2;
        let data_end = data_start + chunk_size;

        if data_end <= remaining.len() {
            result.push_str(&remaining[data_start..data_end]);
            remaining = &remaining[data_end..];

            // Skip trailing \r\n
            if remaining.starts_with("\r\n") {
                remaining = &remaining[2..];
            }
        } else {
            break;
        }
    }

    result
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

/// Make an HTTP GET request using the stored configuration
pub fn wg_http_get(host: &str, port: u16, path: &str) -> io::Result<HttpResponse> {
    let config = GLOBAL_HTTP_CONFIG
        .lock()
        .clone()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "WG HTTP not configured"))?;

    http_get(&config, host, port, path, false)
}

/// Make an HTTP GET request and return just the body
pub fn wg_http_get_string(host: &str, port: u16, path: &str) -> io::Result<String> {
    let response = wg_http_get(host, port, path)?;

    if response.status_code >= 200 && response.status_code < 300 {
        Ok(response.body)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("HTTP error: {}", response.status_code),
        ))
    }
}

// ============================================================================
// Shared WireGuard TCP stack (for HTTP/HTTPS and socket connections)
//
// Uses a SINGLE shared WireGuard tunnel with a manual TCP/IP stack
// (VirtualStack from tun_stack module). This avoids:
// 1. smoltcp silently dropping SYN-ACK packets (checksum or other issues)
// 2. Multiple WG tunnels with the same key conflicting at the server
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

