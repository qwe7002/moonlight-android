package com.limelight.binding.wireguard;

import android.util.Log;

import java.io.IOException;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.net.UnknownHostException;

import javax.net.SocketFactory;

/**
 * Custom SocketFactory that creates WgSocket instances for routing TCP traffic
 * directly through WireGuard via JNI.
 * 
 * Usage with OkHttp:
 * <pre>
 * OkHttpClient client = new OkHttpClient.Builder()
 *     .socketFactory(WgSocketFactory.getInstance())
 *     .build();
 * </pre>
 */
public class WgSocketFactory extends SocketFactory {
    private static final String TAG = "WgSocketFactory";
    
    private static volatile WgSocketFactory instance;
    
    private WgSocketFactory() {
    }
    
    /**
     * Get the singleton instance of WgSocketFactory.
     * Only use this when WireGuard HTTP is configured.
     */
    public static WgSocketFactory getInstance() {
        if (instance == null) {
            synchronized (WgSocketFactory.class) {
                if (instance == null) {
                    instance = new WgSocketFactory();
                }
            }
        }
        return instance;
    }
    
    /**
     * Check if WgSocketFactory should be used (i.e., WireGuard is configured).
     */
    public static boolean isAvailable() {
        return WireGuardManager.isHttpConfigured();
    }
    
    @Override
    public Socket createSocket() throws IOException {
        Log.d(TAG, "createSocket() - creating unconnected WgSocket");
        return new WgSocket();
    }
    
    @Override
    public Socket createSocket(String host, int port) throws IOException, UnknownHostException {
        Log.d(TAG, "createSocket(" + host + ", " + port + ")");
        WgSocket socket = new WgSocket();
        socket.connect(new InetSocketAddress(host, port));
        return socket;
    }
    
    @Override
    public Socket createSocket(String host, int port, InetAddress localHost, int localPort) 
            throws IOException, UnknownHostException {
        Log.d(TAG, "createSocket(" + host + ", " + port + ", localHost, " + localPort + ")");
        // Local address binding is handled internally by VirtualStack
        WgSocket socket = new WgSocket();
        socket.connect(new InetSocketAddress(host, port));
        return socket;
    }
    
    @Override
    public Socket createSocket(InetAddress host, int port) throws IOException {
        Log.d(TAG, "createSocket(" + host.getHostAddress() + ", " + port + ")");
        WgSocket socket = new WgSocket();
        socket.connect(new InetSocketAddress(host, port));
        return socket;
    }
    
    @Override
    public Socket createSocket(InetAddress address, int port, InetAddress localAddress, int localPort) 
            throws IOException {
        Log.d(TAG, "createSocket(" + address.getHostAddress() + ", " + port + ", localAddr, " + localPort + ")");
        // Local address binding is handled internally by VirtualStack
        WgSocket socket = new WgSocket();
        socket.connect(new InetSocketAddress(address, port));
        return socket;
    }
}
