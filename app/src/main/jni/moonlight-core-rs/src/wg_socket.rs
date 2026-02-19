//! WgSocket - Direct TCP socket access through WireGuard via JNI
//!
//! This module provides JNI interfaces for WgSocket.java, enabling
//! direct TCP socket operations through WireGuard without local port proxying.
//!
//! Architecture:
//! ```text
//! Java OkHttp                           Rust WireGuard tunnel
//!   WgSocket.connect() ---JNI---> wg_socket_connect() ---> VirtualStack.tcp_connect()
//!   WgSocket.read()    ---JNI---> wg_socket_recv()    ---> channel.recv()
//!   WgSocket.write()   ---JNI---> wg_socket_send()    ---> VirtualStack.tcp_send()
//!   WgSocket.close()   ---JNI---> wg_socket_close()   ---> VirtualStack.tcp_close()
//! ```
//!
//! IMPORTANT: The global SOCKET_CONNECTIONS lock is only held briefly for map lookups.
//! Blocking I/O (recv_timeout) is done on Arc-wrapped per-connection state, outside the
//! global lock, to avoid deadlocking OkHttp's concurrent read/write threads.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use parking_lot::Mutex;

use crate::tun_stack::{TcpConnectionId, TcpState};
use crate::wg_http::{get_or_create_shared_proxy, GLOBAL_HTTP_CONFIG};

/// Handle counter for socket connections
static HANDLE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Per-connection receive buffer (protected by its own mutex, independent of global map)
struct RecvBuffer {
    data: Vec<u8>,
    pos: usize,
    /// EOF was received (e.g., consumed by wg_socket_has_data polling)
    eof: bool,
}

/// Active socket connection info.
/// Fields wrapped in Arc so they can be used outside the global map lock.
struct WgSocketConnection {
    conn_id: TcpConnectionId,
    /// Receiver channel - wrapped in Arc<Mutex> so recv can block without holding global lock
    receiver: Arc<Mutex<Receiver<Vec<u8>>>>,
    /// Per-connection recv buffer - wrapped in Arc<Mutex> for the same reason
    recv_buf: Arc<Mutex<RecvBuffer>>,
    _created_at: Instant,
}

/// Global map of socket handles to connections.
/// IMPORTANT: This lock must only be held briefly for map lookups, never during blocking I/O.
static SOCKET_CONNECTIONS: Mutex<Option<HashMap<u64, WgSocketConnection>>> = Mutex::new(None);

/// Initialize the socket connections map if needed
fn ensure_connections_map() {
    let mut map = SOCKET_CONNECTIONS.lock();
    if map.is_none() {
        *map = Some(HashMap::new());
    }
}

/// Look up a connection and clone its Arc-wrapped fields for use outside the lock.
fn get_connection_arcs(handle: u64) -> Option<(TcpConnectionId, Arc<Mutex<Receiver<Vec<u8>>>>, Arc<Mutex<RecvBuffer>>)> {
    let map = SOCKET_CONNECTIONS.lock();
    let connections = map.as_ref()?;
    let conn = connections.get(&handle)?;
    Some((conn.conn_id, conn.receiver.clone(), conn.recv_buf.clone()))
}

/// Create a TCP connection through WireGuard VirtualStack.
/// Returns a handle (>0) on success, 0 on failure.
pub fn wg_socket_connect(host: &str, port: u16, timeout_ms: u32) -> u64 {
    info!("wg_socket_connect: {}:{} (timeout={}ms)", host, port, timeout_ms);

    // Get config
    let config = match GLOBAL_HTTP_CONFIG.lock().clone() {
        Some(c) => c,
        None => {
            error!("wg_socket_connect: WireGuard HTTP not configured");
            return 0;
        }
    };

    // Parse host as IPv4 (WireGuard tunnel IP)
    let target_ip: Ipv4Addr = match host.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("wg_socket_connect: invalid host IP '{}': {}", host, e);
            return 0;
        }
    };

    // Get the shared proxy (handles WG tunnel creation/reuse)
    let proxy = match get_or_create_shared_proxy(&config) {
        Ok(p) => p,
        Err(e) => {
            error!("wg_socket_connect: failed to get shared proxy: {}", e);
            return 0;
        }
    };

    // Initiate TCP connection through virtual stack
    let (conn_id, rx) = proxy.virtual_stack.tcp_connect(target_ip, port);

    // Flush the SYN packet
    proxy.flush_outgoing();

    // Wait for connection establishment with SYN retransmission
    let connect_timeout = Duration::from_millis(timeout_ms as u64);
    let start = Instant::now();
    
    // SYN retransmission with exponential backoff: 500ms, 1s, 2s, 4s...
    let mut next_syn_retry = start + Duration::from_millis(500);
    let mut syn_retry_interval = Duration::from_millis(500);
    let max_syn_retry_interval = Duration::from_secs(4);

    while !proxy.virtual_stack.is_tcp_established(&conn_id) {
        let remaining = connect_timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            let state = proxy.virtual_stack.get_tcp_state(&conn_id);
            warn!("wg_socket_connect: timeout after {:?}, state: {:?}", start.elapsed(), state);
            proxy.virtual_stack.remove_tcp_connection(&conn_id);
            return 0;
        }

        // Check for connection reset/refused
        match proxy.virtual_stack.get_tcp_state(&conn_id) {
            Some(TcpState::Closed) | None => {
                warn!("wg_socket_connect: connection to {}:{} refused", target_ip, port);
                proxy.virtual_stack.remove_tcp_connection(&conn_id);
                return 0;
            }
            _ => {}
        }

        // Retransmit SYN if needed (in case initial SYN was lost)
        let now = Instant::now();
        if now >= next_syn_retry {
            if proxy.virtual_stack.resend_syn_if_pending(&conn_id) {
                proxy.flush_outgoing();
                // Exponential backoff for next retry
                syn_retry_interval = (syn_retry_interval * 2).min(max_syn_retry_interval);
                next_syn_retry = now + syn_retry_interval;
            }
        }

        // Wait for state change notification (with short timeout for safety)
        let wait_time = remaining.min(Duration::from_millis(100));
        proxy.virtual_stack.wait_for_state_change(wait_time);
    }

    // Connection established - create handle
    let handle = HANDLE_COUNTER.fetch_add(1, Ordering::SeqCst);

    let connection = WgSocketConnection {
        conn_id,
        receiver: Arc::new(Mutex::new(rx)),
        recv_buf: Arc::new(Mutex::new(RecvBuffer {
            data: Vec::new(),
            pos: 0,
            eof: false,
        })),
        _created_at: Instant::now(),
    };

    ensure_connections_map();
    SOCKET_CONNECTIONS.lock().as_mut().unwrap().insert(handle, connection);

    info!("wg_socket_connect: established connection to {}:{}, handle={}", target_ip, port, handle);
    handle
}

/// Get the local port allocated for this connection
pub fn wg_socket_get_local_port(handle: u64) -> u16 {
    let map = SOCKET_CONNECTIONS.lock();
    if let Some(ref connections) = *map {
        if let Some(conn) = connections.get(&handle) {
            return conn.conn_id.local_port;
        }
    }
    0
}

/// Receive data from a connection.
/// Returns bytes read, 0 on EOF, -1 on error, -2 on timeout.
///
/// CRITICAL: This function must NOT hold the global SOCKET_CONNECTIONS lock while blocking
/// on recv_timeout(), because OkHttp reads and writes on separate threads and both need
/// access to the connection map. We clone Arc references under the lock, release it,
/// then block only on the per-connection mutex.
pub fn wg_socket_recv(handle: u64, buffer: &mut [u8], timeout_ms: u32) -> i32 {
    // Step 1: Briefly lock global map to get Arc refs, then release
    let (receiver_arc, recv_buf_arc) = match get_connection_arcs(handle) {
        Some((_conn_id, rx, buf)) => (rx, buf),
        None => {
            error!("wg_socket_recv: invalid handle {}", handle);
            return -1;
        }
    };
    // Global lock is now released.

    // Step 2: Lock only the per-connection recv buffer
    let mut recv_buf = recv_buf_arc.lock();

    // First, drain any buffered data from previous partial read
    if recv_buf.pos < recv_buf.data.len() {
        let available = recv_buf.data.len() - recv_buf.pos;
        let to_copy = std::cmp::min(available, buffer.len());
        buffer[..to_copy].copy_from_slice(&recv_buf.data[recv_buf.pos..recv_buf.pos + to_copy]);
        recv_buf.pos += to_copy;

        // Clear buffer if fully consumed
        if recv_buf.pos >= recv_buf.data.len() {
            recv_buf.data.clear();
            recv_buf.pos = 0;
        }

        return to_copy as i32;
    }

    // Check if EOF was previously consumed (e.g., by wg_socket_has_data polling)
    if recv_buf.eof {
        return 0; // EOF
    }

    // Step 3: Lock per-connection receiver and block on channel recv
    // (recv_buf is still held, which is fine - only one reader at a time)
    let receiver = receiver_arc.lock();

    let timeout = if timeout_ms > 0 {
        Duration::from_millis(timeout_ms as u64)
    } else {
        Duration::from_secs(300) // Default 5 min timeout
    };

    match receiver.recv_timeout(timeout) {
        Ok(data) => {
            if data.is_empty() {
                recv_buf.eof = true;
                return 0; // EOF
            }

            let to_copy = std::cmp::min(data.len(), buffer.len());
            buffer[..to_copy].copy_from_slice(&data[..to_copy]);

            // Buffer remaining data if any
            if to_copy < data.len() {
                recv_buf.data = data[to_copy..].to_vec();
                recv_buf.pos = 0;
            }

            to_copy as i32
        }
        Err(RecvTimeoutError::Timeout) => {
            -2 // Timeout error code
        }
        Err(RecvTimeoutError::Disconnected) => {
            // Mark EOF so subsequent calls return immediately without logging again
            recv_buf.eof = true;
            debug!("wg_socket_recv: channel disconnected for handle {}", handle);
            0 // EOF
        }
    }
}

/// Send data through a connection.
/// Returns bytes sent, or negative on error.
pub fn wg_socket_send(handle: u64, data: &[u8]) -> i32 {
    // Get config for proxy access
    let config = match GLOBAL_HTTP_CONFIG.lock().clone() {
        Some(c) => c,
        None => {
            error!("wg_socket_send: WireGuard HTTP not configured");
            return -1;
        }
    };

    // Briefly lock global map to get conn_id, then release
    let conn_id = match get_connection_arcs(handle) {
        Some((id, _, _)) => id,
        None => {
            error!("wg_socket_send: invalid handle {}", handle);
            return -1;
        }
    };

    // Get shared proxy and send data (no global lock held)
    let proxy = match get_or_create_shared_proxy(&config) {
        Ok(p) => p,
        Err(e) => {
            error!("wg_socket_send: failed to get shared proxy: {}", e);
            return -1;
        }
    };

    // Send through virtual stack
    if let Err(e) = proxy.virtual_stack.tcp_send(&conn_id, data) {
        error!("wg_socket_send: tcp_send failed: {}", e);
        return -1;
    }

    // Flush outgoing packets
    proxy.flush_outgoing();

    data.len() as i32
}

/// Close a connection
pub fn wg_socket_close(handle: u64) {
    info!("wg_socket_close: handle={}", handle);

    // Get config for proxy access
    let config = match GLOBAL_HTTP_CONFIG.lock().clone() {
        Some(c) => c,
        None => {
            // Just remove the connection entry
            let mut map = SOCKET_CONNECTIONS.lock();
            if let Some(ref mut connections) = *map {
                connections.remove(&handle);
            }
            return;
        }
    };

    // Get connection ID and remove from map
    let conn_id = {
        let mut map = SOCKET_CONNECTIONS.lock();
        let connections = match *map {
            Some(ref mut c) => c,
            None => return,
        };
        match connections.remove(&handle) {
            Some(conn) => conn.conn_id,
            None => return,
        }
    };
    // Global lock released here; the removed connection's Arcs will drop when we leave scope

    // Gracefully close the TCP connection.
    // Don't remove from virtual stack - let TCP teardown complete properly.
    // The connection will transition through FinWait/LastAck/TimeWait/Closed
    // and be cleaned up by cleanup_stale_connections.
    if let Ok(proxy) = get_or_create_shared_proxy(&config) {
        proxy.virtual_stack.tcp_close(&conn_id).ok();
        proxy.flush_outgoing();
    }
}

/// Close all socket connections (cleanup)
pub fn wg_socket_close_all() {
    info!("wg_socket_close_all");
    
    let handles: Vec<u64> = {
        let map = SOCKET_CONNECTIONS.lock();
        match *map {
            Some(ref connections) => connections.keys().cloned().collect(),
            None => return,
        }
    };

    for handle in handles {
        wg_socket_close(handle);
    }
}

/// Get the number of active socket connections
pub fn wg_socket_connection_count() -> usize {
    let map = SOCKET_CONNECTIONS.lock();
    match *map {
        Some(ref connections) => connections.len(),
        None => 0,
    }
}

/// Check if a connection has data available to read (non-blocking).
/// Returns true if data is buffered or available from the channel.
pub fn wg_socket_has_data(handle: u64) -> bool {
    // Get Arc refs without holding the global lock
    let (receiver_arc, recv_buf_arc) = match get_connection_arcs(handle) {
        Some((_conn_id, rx, buf)) => (rx, buf),
        None => return false,
    };

    // Check if there's buffered data or EOF from a previous read/poll
    {
        let recv_buf = recv_buf_arc.lock();
        if recv_buf.pos < recv_buf.data.len() || recv_buf.eof {
            return true;
        }
    }

    // Try to receive from channel without blocking
    let receiver = receiver_arc.lock();
    match receiver.try_recv() {
        Ok(data) => {
            let mut recv_buf = recv_buf_arc.lock();
            if data.is_empty() {
                // EOF signal - mark it so recv() returns 0 immediately
                recv_buf.eof = true;
            } else {
                // Data available - buffer it for later recv call
                recv_buf.data = data;
                recv_buf.pos = 0;
            }
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            // Channel closed - mark EOF so recv() returns 0 immediately
            let mut recv_buf = recv_buf_arc.lock();
            recv_buf.eof = true;
            true
        }
    }
}
