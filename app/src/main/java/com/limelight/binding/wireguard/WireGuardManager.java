package com.limelight.binding.wireguard;

import android.util.Base64;
import android.util.Log;

import java.net.InetAddress;

/**
 * WireGuard tunnel manager that interfaces with the Rust native library.
 * Provides methods for configuring and managing the WireGuard VPN tunnel
 * used for streaming over the internet.
 */
public class WireGuardManager {
    private static final String TAG = "WireGuardManager";

    // Load the native library
    static {
        try {
            System.loadLibrary("moonlight_core");
        } catch (UnsatisfiedLinkError e) {
            Log.e(TAG, "Failed to load moonlight_core native library", e);
        }
    }

    /**
     * Configuration class for WireGuard tunnel
     */
    public static class Config {
        private byte[] privateKey;
        private byte[] peerPublicKey;
        private byte[] presharedKey; // nullable
        private String endpoint;
        private String tunnelAddress;
        private int keepaliveSecs;
        private int mtu;

        public Config() {
            this.keepaliveSecs = 25;
            this.mtu = 1420;
            this.tunnelAddress = "10.0.0.2";
        }

        public Config setPrivateKey(byte[] privateKey) {
            this.privateKey = privateKey;
            return this;
        }

        public Config setPrivateKeyBase64(String privateKeyB64) {
            this.privateKey = Base64.decode(privateKeyB64, Base64.DEFAULT);
            return this;
        }

        public Config setPeerPublicKey(byte[] peerPublicKey) {
            this.peerPublicKey = peerPublicKey;
            return this;
        }

        public Config setPeerPublicKeyBase64(String peerPublicKeyB64) {
            this.peerPublicKey = Base64.decode(peerPublicKeyB64, Base64.DEFAULT);
            return this;
        }

        public Config setPresharedKey(byte[] presharedKey) {
            this.presharedKey = presharedKey;
            return this;
        }

        public Config setPresharedKeyBase64(String presharedKeyB64) {
            if (presharedKeyB64 != null && !presharedKeyB64.isEmpty()) {
                this.presharedKey = Base64.decode(presharedKeyB64, Base64.DEFAULT);
            }
            return this;
        }

        public Config setEndpoint(String endpoint) {
            this.endpoint = endpoint;
            return this;
        }

        public Config setTunnelAddress(String tunnelAddress) {
            this.tunnelAddress = tunnelAddress;
            return this;
        }

        public Config setKeepaliveSecs(int keepaliveSecs) {
            this.keepaliveSecs = keepaliveSecs;
            return this;
        }

        public Config setMtu(int mtu) {
            this.mtu = mtu;
            return this;
        }

        public byte[] getPrivateKey() { return privateKey; }
        public byte[] getPeerPublicKey() { return peerPublicKey; }
        public byte[] getPresharedKey() { return presharedKey; }
        public String getEndpoint() { return endpoint; }
        public String getTunnelAddress() { return tunnelAddress; }
        public int getKeepaliveSecs() { return keepaliveSecs; }
        public int getMtu() { return mtu; }

        /**
         * Validate the configuration
         * @return null if valid, error message if invalid
         */
        public String validate() {
            if (privateKey == null || privateKey.length != 32) {
                return "Invalid private key (must be 32 bytes)";
            }
            if (peerPublicKey == null || peerPublicKey.length != 32) {
                return "Invalid peer public key (must be 32 bytes)";
            }
            if (presharedKey != null && presharedKey.length != 32) {
                return "Invalid preshared key (must be 32 bytes)";
            }
            if (endpoint == null || !endpoint.contains(":")) {
                return "Invalid endpoint format (use host:port)";
            }
            if (tunnelAddress == null || tunnelAddress.isEmpty()) {
                return "Invalid tunnel address";
            }
            if (mtu < 576 || mtu > 65535) {
                return "Invalid MTU (must be 576-65535)";
            }
            return null;
        }
    }

    /**
     * Callback interface for tunnel status updates
     */
    public interface StatusCallback {
        void onConnecting();
        void onConnected();
        void onDisconnected();
        void onError(String error);
    }

    private static StatusCallback statusCallback;
    private static volatile boolean isActive = false;

    /**
     * Set the status callback for tunnel events
     */
    public static void setStatusCallback(StatusCallback callback) {
        statusCallback = callback;
    }

    /**
     * Start the WireGuard tunnel with the given configuration
     * @param config The tunnel configuration
     * @return true if successful, false otherwise
     */
    public static boolean startTunnel(Config config) {
        String error = config.validate();
        if (error != null) {
            Log.e(TAG, "Invalid configuration: " + error);
            if (statusCallback != null) {
                statusCallback.onError(error);
            }
            return false;
        }

        if (statusCallback != null) {
            statusCallback.onConnecting();
        }

        try {
            // Parse endpoint
            String[] parts = config.endpoint.split(":");
            String host = parts[0];
            int port = Integer.parseInt(parts[1]);

            // Resolve hostname
            InetAddress addr = InetAddress.getByName(host);
            String resolvedEndpoint = addr.getHostAddress() + ":" + port;

            boolean result = nativeStartTunnel(
                config.privateKey,
                config.peerPublicKey,
                config.presharedKey,
                resolvedEndpoint,
                config.tunnelAddress,
                config.keepaliveSecs,
                config.mtu
            );

            if (result) {
                isActive = true;
                if (statusCallback != null) {
                    statusCallback.onConnected();
                }
                Log.i(TAG, "WireGuard tunnel started successfully");
            } else {
                if (statusCallback != null) {
                    statusCallback.onError("Failed to start tunnel");
                }
                Log.e(TAG, "Failed to start WireGuard tunnel");
            }

            return result;
        } catch (Exception e) {
            Log.e(TAG, "Failed to start tunnel", e);
            if (statusCallback != null) {
                statusCallback.onError(e.getMessage());
            }
            return false;
        }
    }

    /**
     * Stop the WireGuard tunnel
     */
    public static void stopTunnel() {
        nativeStopTunnel();
        isActive = false;
        if (statusCallback != null) {
            statusCallback.onDisconnected();
        }
        Log.i(TAG, "WireGuard tunnel stopped");
    }

    /**
     * Check if the tunnel is currently active
     */
    public static boolean isTunnelActive() {
        return isActive && nativeIsTunnelActive();
    }

    /**
     * Generate a new WireGuard private key
     * @return 32-byte private key, or null on error
     */
    public static byte[] generatePrivateKey() {
        return nativeGeneratePrivateKey();
    }

    /**
     * Derive the public key from a private key
     * @param privateKey 32-byte private key
     * @return 32-byte public key, or null on error
     */
    public static byte[] derivePublicKey(byte[] privateKey) {
        if (privateKey == null || privateKey.length != 32) {
            return null;
        }
        return nativeDerivePublicKey(privateKey);
    }

    /**
     * Generate a new key pair
     * @return array of [privateKey, publicKey], or null on error
     */
    public static byte[][] generateKeyPair() {
        byte[] privateKey = generatePrivateKey();
        if (privateKey == null) {
            return null;
        }
        byte[] publicKey = derivePublicKey(privateKey);
        if (publicKey == null) {
            return null;
        }
        return new byte[][] { privateKey, publicKey };
    }

    /**
     * Encode a key to Base64
     */
    public static String encodeKey(byte[] key) {
        return Base64.encodeToString(key, Base64.NO_WRAP);
    }

    /**
     * Decode a key from Base64
     */
    public static byte[] decodeKey(String keyB64) {
        try {
            return Base64.decode(keyB64, Base64.DEFAULT);
        } catch (Exception e) {
            return null;
        }
    }

    // Native methods implemented in Rust
    private static native boolean nativeStartTunnel(
        byte[] privateKey,
        byte[] peerPublicKey,
        byte[] presharedKey,
        String endpoint,
        String tunnelAddress,
        int keepaliveSecs,
        int mtu
    );

    private static native void nativeStopTunnel();
    private static native boolean nativeIsTunnelActive();
    private static native byte[] nativeGeneratePrivateKey();
    private static native byte[] nativeDerivePublicKey(byte[] privateKey);

    // ========================================================================
    // Direct HTTP through WireGuard (bypasses OkHttp)
    // ========================================================================

    private static volatile boolean httpConfigured = false;
    private static volatile String currentTunnelAddress = null;

    /**
     * Generation counter for HTTP config. Incremented each time configureHttp() is called.
     * Used by ComputerManagerService to detect if someone else reconfigured HTTP since
     * the service last set it up, avoiding inadvertent teardown of another owner's config.
     */
    private static volatile int httpConfigGeneration = 0;

    /**
     * Get the current HTTP config generation counter.
     */
    public static int getHttpConfigGeneration() {
        return httpConfigGeneration;
    }

    /**
     * Get the currently configured tunnel address.
     * @return The tunnel address (e.g. "10.0.0.2"), or null if not configured
     */
    public static String getCurrentTunnelAddress() {
        return currentTunnelAddress;
    }

    /**
     * Configure the WireGuard HTTP client for direct HTTP requests.
     * This allows making HTTP requests directly through WireGuard without OkHttp.
     *
     * @param config The WireGuard configuration
     * @param serverAddress The server IP address in the tunnel (e.g. "10.0.0.1")
     * @return true if configuration succeeded
     */
    public static boolean configureHttp(Config config, String serverAddress) {
        String error = config.validate();
        if (error != null) {
            Log.e(TAG, "Invalid configuration for HTTP: " + error);
            return false;
        }

        try {
            // Parse endpoint
            String[] parts = config.endpoint.split(":");
            String host = parts[0];
            int port = Integer.parseInt(parts[1]);

            // Resolve hostname
            java.net.InetAddress addr = java.net.InetAddress.getByName(host);
            String resolvedEndpoint = addr.getHostAddress() + ":" + port;

            boolean result = nativeHttpSetConfig(
                config.privateKey,
                config.peerPublicKey,
                config.presharedKey,
                resolvedEndpoint,
                config.tunnelAddress,
                serverAddress,
                config.keepaliveSecs,
                config.mtu
            );

            if (result) {
                httpConfigured = true;
                httpConfigGeneration++;
                currentTunnelAddress = config.tunnelAddress;
                Log.i(TAG, "WireGuard HTTP client configured, tunnel address: " + currentTunnelAddress + ", generation: " + httpConfigGeneration);
            }
            return result;
        } catch (Exception e) {
            Log.e(TAG, "Failed to configure WireGuard HTTP client", e);
            return false;
        }
    }

    /**
     * Clear the WireGuard HTTP client configuration.
     */
    public static void clearHttpConfig() {
        nativeHttpClearConfig();
        httpConfigured = false;
        currentTunnelAddress = null;
        Log.i(TAG, "WireGuard HTTP client configuration cleared");
    }

    /**
     * Check if the WireGuard HTTP client is configured.
     */
    public static boolean isHttpConfigured() {
        return httpConfigured && nativeHttpIsConfigured();
    }

    // Direct HTTP native methods (config only - actual HTTP now goes through OkHttp + WgSocket)
    private static native boolean nativeHttpSetConfig(
        byte[] privateKey,
        byte[] peerPublicKey,
        byte[] presharedKey,
        String endpoint,
        String tunnelAddress,
        String serverAddress,
        int keepaliveSecs,
        int mtu
    );
    private static native void nativeHttpClearConfig();
    private static native boolean nativeHttpIsConfigured();
}