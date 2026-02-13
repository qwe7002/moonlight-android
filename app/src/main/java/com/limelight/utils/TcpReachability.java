package com.limelight.utils;

import com.limelight.nvstream.http.ComputerDetails;

import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;

/**
 * Utility class for checking TCP port reachability.
 * Uses TCP connect to verify if a host is reachable on a specific port.
 */
public class TcpReachability {

    // Default timeout for TCP connection check (milliseconds)
    public static final int DEFAULT_TCP_TIMEOUT_MS = 2000;

    // Quick check timeout for initial ping (milliseconds)
    public static final int QUICK_TCP_TIMEOUT_MS = 1000;

    /**
     * Result of a TCP ping test containing success status and latency.
     */
    public static class TcpPingResult {
        public final boolean success;
        public final long latencyMs;
        public final String address;
        public final int port;
        public final String errorMessage;

        public TcpPingResult(boolean success, long latencyMs, String address, int port, String errorMessage) {
            this.success = success;
            this.latencyMs = latencyMs;
            this.address = address;
            this.port = port;
            this.errorMessage = errorMessage;
        }
    }

    /**
     * Check if a host is reachable by attempting a TCP connection to the specified port.
     * Returns a result object containing success status and latency.
     *
     * @param host The hostname or IP address to check
     * @param port The port number to connect to
     * @param timeoutMs Connection timeout in milliseconds
     * @return TcpPingResult containing success status and latency in milliseconds
     */
    public static TcpPingResult tcpPing(String host, int port, int timeoutMs) {
        if (host == null || host.isEmpty() || port <= 0) {
            return new TcpPingResult(false, -1, host, port, "Invalid host or port");
        }

        long startTime = System.currentTimeMillis();
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress(host, port), timeoutMs);
            long latency = System.currentTimeMillis() - startTime;
            LimeLog.info("TCP ping successful for " + host + ":" + port + " - latency: " + latency + "ms");
            return new TcpPingResult(true, latency, host, port, null);
        } catch (IOException e) {
            long latency = System.currentTimeMillis() - startTime;
            String errorMsg = e.getMessage();
            LimeLog.info("TCP ping failed for " + host + ":" + port + " - " + errorMsg);
            return new TcpPingResult(false, latency, host, port, errorMsg);
        }
    }

    /**
     * Check if a host is reachable by attempting a TCP connection to the specified port.
     *
     * @param host The hostname or IP address to check
     * @param port The port number to connect to
     * @param timeoutMs Connection timeout in milliseconds
     * @return true if the connection was successful, false otherwise
     */
    public static boolean isTcpPortReachable(String host, int port, int timeoutMs) {
        if (host == null || host.isEmpty() || port <= 0) {
            return false;
        }

        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress(host, port), timeoutMs);
            return true;
        } catch (IOException e) {
            // Connection failed - host is not reachable
            LimeLog.info("TCP ping failed for " + host + ":" + port + " - " + e.getMessage());
            return false;
        }
    }

    /**
     * Check if a host is reachable by attempting a TCP connection to the specified port.
     * Uses the default timeout.
     *
     * @param host The hostname or IP address to check
     * @param port The port number to connect to
     * @return true if the connection was successful, false otherwise
     */
    public static boolean isTcpPortReachable(String host, int port) {
        return isTcpPortReachable(host, port, DEFAULT_TCP_TIMEOUT_MS);
    }

    /**
     * Check if an address tuple is reachable via TCP.
     *
     * @param address The address tuple containing host and port
     * @param timeoutMs Connection timeout in milliseconds
     * @return true if the connection was successful, false otherwise
     */
    public static boolean isAddressReachable(ComputerDetails.AddressTuple address, int timeoutMs) {
        if (address == null) {
            return false;
        }
        return isTcpPortReachable(address.address, address.port, timeoutMs);
    }

    /**
     * Check if an address tuple is reachable via TCP.
     * Uses the default timeout.
     *
     * @param address The address tuple containing host and port
     * @return true if the connection was successful, false otherwise
     */
    public static boolean isAddressReachable(ComputerDetails.AddressTuple address) {
        return isAddressReachable(address, DEFAULT_TCP_TIMEOUT_MS);
    }

    /**
     * Perform a quick TCP ping check with a shorter timeout.
     * This is useful for initial reachability checks before performing
     * more expensive HTTP requests.
     *
     * @param address The address tuple containing host and port
     * @return true if the connection was successful, false otherwise
     */
    public static boolean quickPing(ComputerDetails.AddressTuple address) {
        return isAddressReachable(address, QUICK_TCP_TIMEOUT_MS);
    }

    /**
     * Perform a TCP ping on an address tuple and return detailed results.
     *
     * @param address The address tuple containing host and port
     * @param timeoutMs Connection timeout in milliseconds
     * @return TcpPingResult containing success status and latency
     */
    public static TcpPingResult tcpPingAddress(ComputerDetails.AddressTuple address, int timeoutMs) {
        if (address == null) {
            return new TcpPingResult(false, -1, null, 0, "Address is null");
        }
        return tcpPing(address.address, address.port, timeoutMs);
    }

    /**
     * Perform a TCP ping on an address tuple with default timeout.
     *
     * @param address The address tuple containing host and port
     * @return TcpPingResult containing success status and latency
     */
    public static TcpPingResult tcpPingAddress(ComputerDetails.AddressTuple address) {
        return tcpPingAddress(address, DEFAULT_TCP_TIMEOUT_MS);
    }

    /**
     * Check if any of the provided addresses is reachable.
     * Returns the first reachable address or null if none are reachable.
     *
     * @param addresses Array of address tuples to check
     * @param timeoutMs Connection timeout in milliseconds for each address
     * @return The first reachable address tuple, or null if none are reachable
     */
    public static ComputerDetails.AddressTuple findFirstReachableAddress(
            ComputerDetails.AddressTuple[] addresses, int timeoutMs) {
        if (addresses == null) {
            return null;
        }

        for (ComputerDetails.AddressTuple address : addresses) {
            if (isAddressReachable(address, timeoutMs)) {
                LimeLog.info("Found reachable address: " + address);
                return address;
            }
        }
        return null;
    }
}

