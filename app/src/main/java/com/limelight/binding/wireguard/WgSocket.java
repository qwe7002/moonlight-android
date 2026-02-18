package com.limelight.binding.wireguard;

import android.util.Log;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.net.SocketAddress;
import java.net.SocketException;
import java.net.SocketTimeoutException;

/**
 * Custom Socket implementation that routes TCP traffic directly through WireGuard via JNI.
 * This eliminates the need for local TCP proxy ports.
 */
public class WgSocket extends Socket {
    private static final String TAG = "WgSocket";

    // Ensure native library is loaded
    static {
        try {
            System.loadLibrary("moonlight_core");
        } catch (UnsatisfiedLinkError e) {
            Log.e(TAG, "Failed to load moonlight_core native library", e);
        }
    }

    // Native connection handle (managed by Rust)
    private long nativeHandle = 0;

    // Connection state
    private boolean connected = false;
    private boolean closed = false;
    private boolean inputShutdown = false;
    private boolean outputShutdown = false;

    // Socket options
    private int soTimeout = 0;
    private boolean tcpNoDelay = true;
    private int sendBufferSize = 65536;
    private int receiveBufferSize = 65536;

    // Remote endpoint info
    private InetSocketAddress remoteAddress;
    private InetSocketAddress localAddress;

    // I/O streams (created lazily)
    private WgInputStream inputStream;
    private WgOutputStream outputStream;

    /**
     * Create an unconnected WgSocket
     */
    public WgSocket() {
        // Allocate a local port placeholder
        localAddress = new InetSocketAddress("0.0.0.0", 0);
    }

    @Override
    public void connect(SocketAddress endpoint) throws IOException {
        connect(endpoint, 0);
    }

    @Override
    public void connect(SocketAddress endpoint, int timeout) throws IOException {
        if (closed) {
            throw new SocketException("Socket is closed");
        }
        if (connected) {
            throw new SocketException("Already connected");
        }
        if (!(endpoint instanceof InetSocketAddress)) {
            throw new IllegalArgumentException("Unsupported address type");
        }

        InetSocketAddress inetEndpoint = (InetSocketAddress) endpoint;
        String host = inetEndpoint.getAddress() != null
                ? inetEndpoint.getAddress().getHostAddress()
                : inetEndpoint.getHostName();
        int port = inetEndpoint.getPort();

        Log.i(TAG, "Connecting to " + host + ":" + port + " via WireGuard (timeout=" + timeout + "ms)");

        // Create native connection through VirtualStack
        nativeHandle = nativeConnect(host, port, timeout > 0 ? timeout : 10000);

        if (nativeHandle == 0) {
            throw new IOException("Failed to establish WireGuard connection to " + host + ":" + port);
        }

        connected = true;
        remoteAddress = inetEndpoint;

        // Get the allocated local port from native
        int localPort = nativeGetLocalPort(nativeHandle);
        localAddress = new InetSocketAddress("10.0.0.2", localPort);

        Log.i(TAG, "Connected to " + host + ":" + port + " via WireGuard (handle=" + nativeHandle + ")");
    }

    @Override
    public InputStream getInputStream() throws IOException {
        if (closed) {
            throw new SocketException("Socket is closed");
        }
        if (inputShutdown) {
            throw new SocketException("Socket input is shutdown");
        }
        if (!connected) {
            throw new SocketException("Socket is not connected");
        }

        if (inputStream == null) {
            inputStream = new WgInputStream(this);
        }
        return inputStream;
    }

    @Override
    public OutputStream getOutputStream() throws IOException {
        if (closed) {
            throw new SocketException("Socket is closed");
        }
        if (outputShutdown) {
            throw new SocketException("Socket output is shutdown");
        }
        if (!connected) {
            throw new SocketException("Socket is not connected");
        }

        if (outputStream == null) {
            outputStream = new WgOutputStream(this);
        }
        return outputStream;
    }

    @Override
    public synchronized void close() throws IOException {
        if (closed) {
            return;
        }

        closed = true;
        inputShutdown = true;
        outputShutdown = true;

        if (nativeHandle != 0) {
            Log.i(TAG, "Closing WireGuard socket (handle=" + nativeHandle + ")");
            nativeClose(nativeHandle);
            nativeHandle = 0;
        }
    }

    @Override
    public void shutdownInput() throws IOException {
        if (closed) {
            throw new SocketException("Socket is closed");
        }
        inputShutdown = true;
    }

    @Override
    public void shutdownOutput() throws IOException {
        if (closed) {
            throw new SocketException("Socket is closed");
        }
        outputShutdown = true;
    }

    @Override
    public boolean isConnected() {
        return connected;
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public boolean isInputShutdown() {
        return inputShutdown;
    }

    @Override
    public boolean isOutputShutdown() {
        return outputShutdown;
    }

    @Override
    public boolean isBound() {
        return localAddress != null;
    }

    @Override
    public InetAddress getInetAddress() {
        return remoteAddress != null ? remoteAddress.getAddress() : null;
    }

    @Override
    public int getPort() {
        return remoteAddress != null ? remoteAddress.getPort() : 0;
    }

    @Override
    public InetAddress getLocalAddress() {
        return localAddress != null ? localAddress.getAddress() : null;
    }

    @Override
    public int getLocalPort() {
        return localAddress != null ? localAddress.getPort() : -1;
    }

    @Override
    public SocketAddress getRemoteSocketAddress() {
        return remoteAddress;
    }

    @Override
    public SocketAddress getLocalSocketAddress() {
        return localAddress;
    }

    // Socket options
    @Override
    public void setSoTimeout(int timeout) throws SocketException {
        this.soTimeout = timeout;
    }

    @Override
    public int getSoTimeout() throws SocketException {
        return soTimeout;
    }

    @Override
    public void setTcpNoDelay(boolean on) throws SocketException {
        this.tcpNoDelay = on;
    }

    @Override
    public boolean getTcpNoDelay() throws SocketException {
        return tcpNoDelay;
    }

    @Override
    public void setSendBufferSize(int size) throws SocketException {
        this.sendBufferSize = size;
    }

    @Override
    public int getSendBufferSize() throws SocketException {
        return sendBufferSize;
    }

    @Override
    public void setReceiveBufferSize(int size) throws SocketException {
        this.receiveBufferSize = size;
    }

    @Override
    public int getReceiveBufferSize() throws SocketException {
        return receiveBufferSize;
    }

    @Override
    public void setKeepAlive(boolean on) throws SocketException {
        // No-op for WireGuard socket (WireGuard has its own keepalive)
    }

    @Override
    public boolean getKeepAlive() throws SocketException {
        return true; // WireGuard handles keepalive
    }

    @Override
    public void setReuseAddress(boolean on) throws SocketException {
        // No-op
    }

    @Override
    public boolean getReuseAddress() throws SocketException {
        return false;
    }

    @Override
    public void setSoLinger(boolean on, int linger) throws SocketException {
        // No-op
    }

    @Override
    public int getSoLinger() throws SocketException {
        return -1;
    }

    @Override
    public void setOOBInline(boolean on) throws SocketException {
        // No-op
    }

    @Override
    public boolean getOOBInline() throws SocketException {
        return false;
    }

    @Override
    public void setTrafficClass(int tc) throws SocketException {
        // No-op
    }

    @Override
    public int getTrafficClass() throws SocketException {
        return 0;
    }

    // ========================================================================
    // Package-private methods for I/O streams
    // ========================================================================

    /**
     * Read data from the native socket
     * @param buffer Buffer to read into
     * @param offset Start offset in buffer
     * @param length Maximum bytes to read
     * @return Number of bytes read, or -1 on EOF
     */
    int nativeRead(byte[] buffer, int offset, int length) throws IOException {
        if (closed || inputShutdown) {
            return -1;
        }

        int result = nativeRecv(nativeHandle, buffer, offset, length, soTimeout);

        if (result == -2) {
            throw new SocketTimeoutException("Read timed out");
        } else if (result < 0) {
            throw new IOException("Native read error: " + result);
        }

        return result;
    }

    /**
     * Write data to the native socket
     * @param buffer Data to write
     * @param offset Start offset in buffer
     * @param length Number of bytes to write
     */
    void nativeWrite(byte[] buffer, int offset, int length) throws IOException {
        if (closed || outputShutdown) {
            throw new IOException("Socket is closed or output shutdown");
        }

        int result = nativeSend(nativeHandle, buffer, offset, length);

        if (result < 0) {
            throw new IOException("Native write error: " + result);
        }
    }

    long getNativeHandle() {
        return nativeHandle;
    }

    // ========================================================================
    // Native methods (implemented in Rust)
    // ========================================================================

    /**
     * Create a TCP connection through WireGuard VirtualStack
     * @param host Target host IP
     * @param port Target port
     * @param timeoutMs Connection timeout in milliseconds
     * @return Native handle, or 0 on failure
     */
    private static native long nativeConnect(String host, int port, int timeoutMs);

    /**
     * Get the local port allocated for this connection
     */
    private static native int nativeGetLocalPort(long handle);

    /**
     * Receive data from the connection
     * @param handle Native handle
     * @param buffer Buffer to receive into
     * @param offset Offset in buffer
     * @param length Maximum bytes to receive
     * @param timeoutMs Read timeout (0 = no timeout)
     * @return Bytes received, 0 on EOF, -1 on error, -2 on timeout
     */
    private static native int nativeRecv(long handle, byte[] buffer, int offset, int length, int timeoutMs);

    /**
     * Send data through the connection
     * @param handle Native handle
     * @param buffer Data to send
     * @param offset Offset in buffer
     * @param length Number of bytes to send
     * @return Bytes sent, or negative on error
     */
    private static native int nativeSend(long handle, byte[] buffer, int offset, int length);

    /**
     * Close the connection
     */
    private static native void nativeClose(long handle);
}

/**
 * Input stream wrapper for WgSocket
 */
class WgInputStream extends InputStream {
    private final WgSocket socket;

    WgInputStream(WgSocket socket) {
        this.socket = socket;
    }

    @Override
    public int read() throws IOException {
        byte[] buf = new byte[1];
        int n = read(buf, 0, 1);
        return n == 1 ? (buf[0] & 0xFF) : -1;
    }

    @Override
    public int read(byte[] b) throws IOException {
        return read(b, 0, b.length);
    }

    @Override
    public int read(byte[] b, int off, int len) throws IOException {
        return socket.nativeRead(b, off, len);
    }

    @Override
    public int available() throws IOException {
        // We don't have a way to peek at available data without blocking
        return 0;
    }

    @Override
    public void close() throws IOException {
        socket.shutdownInput();
    }
}

/**
 * Output stream wrapper for WgSocket
 */
class WgOutputStream extends OutputStream {
    private final WgSocket socket;

    WgOutputStream(WgSocket socket) {
        this.socket = socket;
    }

    @Override
    public void write(int b) throws IOException {
        write(new byte[]{(byte) b}, 0, 1);
    }

    @Override
    public void write(byte[] b) throws IOException {
        write(b, 0, b.length);
    }

    @Override
    public void write(byte[] b, int off, int len) throws IOException {
        socket.nativeWrite(b, off, len);
    }

    @Override
    public void flush() throws IOException {
        // No buffering, data is sent immediately
    }

    @Override
    public void close() throws IOException {
        socket.shutdownOutput();
    }
}
