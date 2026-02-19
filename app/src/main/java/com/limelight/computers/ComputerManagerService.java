package com.limelight.computers;

import java.io.IOException;
import java.io.OutputStream;
import java.io.StringReader;
import java.net.Inet4Address;
import java.util.HashSet;
import java.util.LinkedList;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.locks.Lock;
import java.util.concurrent.locks.ReentrantLock;

import com.limelight.binding.PlatformBinding;
import com.limelight.binding.wireguard.WireGuardManager;
import com.limelight.discovery.DiscoveryService;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.NvApp;
import com.limelight.nvstream.http.NvHTTP;
import com.limelight.nvstream.http.PairingManager;
import com.limelight.nvstream.mdns.MdnsComputer;
import com.limelight.nvstream.mdns.MdnsDiscoveryListener;
import com.limelight.preferences.PreferenceConfiguration;
import com.limelight.preferences.WireGuardSettingsActivity;
import com.limelight.utils.CacheHelper;
import com.limelight.utils.ServerHelper;

import android.app.Service;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.net.ConnectivityManager;
import android.net.Network;
import android.os.Binder;
import android.os.IBinder;
import android.os.SystemClock;
import android.util.Log;

import androidx.annotation.NonNull;

import org.xmlpull.v1.XmlPullParserException;

public class ComputerManagerService extends Service {
    private static final String TAG = "ComputerManagerService";
    private static final int SERVERINFO_POLLING_PERIOD_MS = 1500;
    private static final int APPLIST_POLLING_PERIOD_MS = 30000;
    private static final int APPLIST_FAILED_POLLING_RETRY_MS = 2000;
    private static final int MDNS_QUERY_PERIOD_MS = 1000;

    // Track if we started WireGuard HTTP in this service
    private boolean wgHttpStartedByService = false;
    // The HTTP config generation when we configured it, to detect external reconfiguration
    private int wgHttpConfigGeneration = -1;
    private static final int OFFLINE_POLL_TRIES = 3;
    private static final int INITIAL_POLL_TRIES = 2;
    private static final int EMPTY_LIST_THRESHOLD = 3;
    private static final int POLL_DATA_TTL_MS = 30000;

    private final ComputerManagerBinder binder = new ComputerManagerBinder();

    private ComputerDatabaseManager dbManager;
    private final AtomicInteger dbRefCount = new AtomicInteger(0);

    private IdentityManager idManager;
    private final LinkedList<PollingTuple> pollingTuples = new LinkedList<>();
    private ComputerManagerListener listener = null;
    private final AtomicInteger activePolls = new AtomicInteger(0);
    private boolean pollingActive = false;
    private final Lock defaultNetworkLock = new ReentrantLock();

    private ConnectivityManager.NetworkCallback networkCallback;

    private DiscoveryService.DiscoveryBinder discoveryBinder;
    private final ServiceConnection discoveryServiceConnection = new ServiceConnection() {
        public void onServiceConnected(ComponentName className, IBinder binder) {
            synchronized (discoveryServiceConnection) {
                DiscoveryService.DiscoveryBinder privateBinder = ((DiscoveryService.DiscoveryBinder) binder);

                // Set us as the event listener
                privateBinder.setListener(createDiscoveryListener());

                // Signal a possible waiter that we're all setup
                discoveryBinder = privateBinder;
                discoveryServiceConnection.notifyAll();
            }
        }

        public void onServiceDisconnected(ComponentName className) {
            discoveryBinder = null;
        }
    };

    // Returns true if the details object was modified
    private boolean runPoll(ComputerDetails details, boolean newPc, int offlineCount) throws InterruptedException {
        if (!getLocalDatabaseReference()) {
            return false;
        }

        final int pollTriesBeforeOffline = details.state == ComputerDetails.State.UNKNOWN ?
                INITIAL_POLL_TRIES : OFFLINE_POLL_TRIES;

        activePolls.incrementAndGet();

        // Poll the machine
        try {
            if (!pollComputer(details)) {
                if (!newPc && offlineCount < pollTriesBeforeOffline) {
                    // Return without calling the listener
                    releaseLocalDatabaseReference();
                    return false;
                }

                details.state = ComputerDetails.State.OFFLINE;
            }
        } catch (InterruptedException e) {
            releaseLocalDatabaseReference();
            throw e;
        } finally {
            activePolls.decrementAndGet();
        }

        // If it's online, update our persistent state
        if (details.state == ComputerDetails.State.ONLINE) {
            ComputerDetails existingComputer = dbManager.getComputerByUUID(details.uuid);

            // Check if it's in the database because it could have been
            // removed after this was issued
            if (!newPc && existingComputer == null) {
                // It's gone
                releaseLocalDatabaseReference();
                return false;
            }

            // If we already have an entry for this computer in the DB, we must
            // combine the existing data with this new data (which may be partially available
            // due to detecting the PC via mDNS) without the saved external address. If we
            // write to the DB without doing this first, we can overwrite our existing data.
            if (existingComputer != null) {
                existingComputer.update(details);
                dbManager.updateComputer(existingComputer);
            } else {
                dbManager.updateComputer(details);
            }
        }

        // Don't call the listener if this is a failed lookup of a new PC
        if ((!newPc || details.state == ComputerDetails.State.ONLINE) && listener != null) {
            listener.notifyComputerUpdated(details);
        }

        releaseLocalDatabaseReference();
        return true;
    }

    private Thread createPollingThread(final PollingTuple tuple) {
        Thread t = new Thread() {
            @Override
            public void run() {

                int offlineCount = 0;
                while (!isInterrupted() && pollingActive && tuple.thread == this) {
                    try {
                        // Only allow one request to the machine at a time
                        synchronized (tuple.networkLock) {
                            // Check if this poll has modified the details
                            if (!runPoll(tuple.computer, false, offlineCount)) {
                                Log.w(TAG, tuple.computer.name + " is offline (try " + offlineCount + ")");
                                offlineCount++;
                            } else {
                                tuple.lastSuccessfulPollMs = SystemClock.elapsedRealtime();
                                offlineCount = 0;
                            }
                        }

                        // Wait until the next polling interval
                        //noinspection BusyWait
                        Thread.sleep(SERVERINFO_POLLING_PERIOD_MS);
                    } catch (InterruptedException e) {
                        break;
                    }
                }
            }
        };
        t.setName("Polling thread for " + tuple.computer.name);
        return t;
    }

    public class ComputerManagerBinder extends Binder {
        public void startPolling(ComputerManagerListener listener) {
            // Polling is active
            pollingActive = true;

            // Set the listener
            ComputerManagerService.this.listener = listener;

            // Configure WireGuard HTTP JNI if not already active.
            // onUnbind() tears it down when polling stops, so we need
            // to re-create a fresh socket when polling resumes.
            if (!wgHttpStartedByService) {
                configureWireGuardHttp();
            }

            // Start mDNS autodiscovery only if enabled in settings
            if (PreferenceConfiguration.isMdnsEnabled(ComputerManagerService.this)) {
                discoveryBinder.startDiscovery(MDNS_QUERY_PERIOD_MS);
            }

            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    // Enforce the poll data TTL
                    if (SystemClock.elapsedRealtime() - tuple.lastSuccessfulPollMs > POLL_DATA_TTL_MS) {
                        Log.i(TAG, "Timing out polled state for " + tuple.computer.name);
                        tuple.computer.state = ComputerDetails.State.UNKNOWN;
                    }

                    // Report this computer initially
                    listener.notifyComputerUpdated(tuple.computer);

                    // This polling thread might already be there
                    if (tuple.thread == null) {
                        tuple.thread = createPollingThread(tuple);
                        tuple.thread.start();
                    }
                }
            }
        }

        public void waitForReady() {
            synchronized (discoveryServiceConnection) {
                try {
                    while (discoveryBinder == null) {
                        // Wait for the bind notification
                        discoveryServiceConnection.wait(1000);
                    }
                } catch (InterruptedException e) {
                    Log.e(TAG, "waitForReady: " + e.getMessage(), e);

                    // InterruptedException clears the thread's interrupt status. Since we can't
                    // handle that here, we will re-interrupt the thread to set the interrupt
                    // status back to true.
                    Thread.currentThread().interrupt();
                }
            }
        }

        public void waitForPollingStopped() {
            while (activePolls.get() != 0) {
                try {
                    //noinspection BusyWait
                    Thread.sleep(250);
                } catch (InterruptedException e) {
                    Log.e(TAG, "waitForPollingStopped: " + e.getMessage(), e);

                    // InterruptedException clears the thread's interrupt status. Since we can't
                    // handle that here, we will re-interrupt the thread to set the interrupt
                    // status back to true.
                    Thread.currentThread().interrupt();
                }
            }
        }

        public boolean addComputerBlocking(ComputerDetails fakeDetails) throws InterruptedException {
            return ComputerManagerService.this.addComputerBlocking(fakeDetails);
        }

        public void removeComputer(ComputerDetails computer) {
            ComputerManagerService.this.removeComputer(computer);
        }

        public void stopPolling() {
            // Just call the unbind handler to cleanup
            ComputerManagerService.this.onUnbind(null);
        }

        public ApplistPoller createAppListPoller(ComputerDetails computer) {
            return new ApplistPoller(computer);
        }

        public String getUniqueId() {
            return idManager.getUniqueId();
        }

        public ComputerDetails getComputer(String uuid) {
            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    if (uuid.equals(tuple.computer.uuid)) {
                        return tuple.computer;
                    }
                }
            }

            return null;
        }

        public void invalidateStateForComputer(String uuid) {
            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    if (uuid.equals(tuple.computer.uuid)) {
                        // We need the network lock to prevent a concurrent poll
                        // from wiping this change out
                        synchronized (tuple.networkLock) {
                            tuple.computer.state = ComputerDetails.State.UNKNOWN;
                        }

                        // Notify the listener so the UI updates immediately
                        if (listener != null) {
                            listener.notifyComputerUpdated(tuple.computer);
                        }

                        // Interrupt the polling thread to force an immediate re-poll
                        if (tuple.thread != null) {
                            tuple.thread.interrupt();
                            tuple.thread = null;
                        }

                        // Start a new polling thread immediately
                        if (pollingActive) {
                            tuple.thread = createPollingThread(tuple);
                            tuple.thread.start();
                        }
                    }
                }
            }
        }
    }

    @Override
    public boolean onUnbind(Intent intent) {
        if (discoveryBinder != null) {
            // Stop mDNS autodiscovery
            discoveryBinder.stopDiscovery();
        }

        // Stop polling
        pollingActive = false;
        synchronized (pollingTuples) {
            for (PollingTuple tuple : pollingTuples) {
                if (tuple.thread != null) {
                    // Interrupt and remove the thread
                    tuple.thread.interrupt();
                    tuple.thread = null;
                }
            }
        }

        // Remove the listener
        listener = null;

        // Teardown WireGuard HTTP JNI so the socket is released while
        // polling is inactive. startPolling() will create a fresh one.
        teardownWireGuardHttp();

        return false;
    }

    private void populateExternalAddress(ComputerDetails details) {
        ConnectivityManager connMgr = (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);

        // Check if we're currently connected to a VPN which may send our
        // traffic through a different path than the default network.
        // In that case, we should skip external address population since
        // STUN results may not accurately reflect the PC's WAN address.
        Network activeNetwork = connMgr.getActiveNetwork();
        if (activeNetwork != null) {
            android.net.NetworkCapabilities caps = connMgr.getNetworkCapabilities(activeNetwork);
            if (caps != null && caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_VPN)) {
                Log.i(TAG, "VPN detected, skipping external address population");
                return;
            }
        }
    }

    private MdnsDiscoveryListener createDiscoveryListener() {
        return new MdnsDiscoveryListener() {
            @Override
            public void notifyComputerAdded(MdnsComputer computer) {
                ComputerDetails details = new ComputerDetails();

                // Populate the computer template with mDNS info
                if (computer.getLocalAddress() != null) {
                    details.localAddress = new ComputerDetails.AddressTuple(computer.getLocalAddress().getHostAddress(), computer.getPort());

                    // Since we're on the same network, we can use STUN to find
                    // our WAN address, which is also very likely the WAN address
                    // of the PC. We can use this later to connect remotely.
                    if (computer.getLocalAddress() instanceof Inet4Address) {
                        populateExternalAddress(details);
                    }
                }
                if (computer.getIpv6Address() != null) {
                    details.ipv6Address = new ComputerDetails.AddressTuple(computer.getIpv6Address().getHostAddress(), computer.getPort());
                }

                try {
                    // Kick off a blocking serverinfo poll on this machine
                    if (!addComputerBlocking(details)) {
                        Log.w(TAG, "Auto-discovered PC failed to respond: " + details);
                    }
                } catch (InterruptedException e) {
                    Log.e(TAG, "notifyComputerAdded: " + e.getMessage(), e);

                    // InterruptedException clears the thread's interrupt status. Since we can't
                    // handle that here, we will re-interrupt the thread to set the interrupt
                    // status back to true.
                    Thread.currentThread().interrupt();
                }
            }

            @Override
            public void notifyDiscoveryFailure(Exception e) {
                Log.e(TAG, "mDNS discovery failure: " + e.getMessage(), e);
            }
        };
    }

    private void addTuple(ComputerDetails details) {
        synchronized (pollingTuples) {
            for (PollingTuple tuple : pollingTuples) {
                // Check if this is the same computer
                if (tuple.computer.uuid.equals(details.uuid)) {
                    // Update the saved computer with potentially new details
                    tuple.computer.update(details);

                    // Start a polling thread if polling is active
                    if (pollingActive && tuple.thread == null) {
                        tuple.thread = createPollingThread(tuple);
                        tuple.thread.start();
                    }

                    // Found an entry so we're done
                    return;
                }
            }

            // If we got here, we didn't find an entry
            PollingTuple tuple = new PollingTuple(details, null);
            if (pollingActive) {
                tuple.thread = createPollingThread(tuple);
            }
            pollingTuples.add(tuple);
            if (tuple.thread != null) {
                tuple.thread.start();
            }
        }
    }

    public boolean addComputerBlocking(ComputerDetails fakeDetails) throws InterruptedException {
        // Block while we try to fill the details

        // We cannot use runPoll() here because it will attempt to persist the state of the machine
        // in the database, which would be bad because we don't have our pinned cert loaded yet.
        if (pollComputer(fakeDetails)) {
            // See if we have record of this PC to pull its pinned cert
            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    if (tuple.computer.uuid.equals(fakeDetails.uuid)) {
                        fakeDetails.serverCert = tuple.computer.serverCert;
                        break;
                    }
                }
            }

            // Poll again, possibly with the pinned cert, to get accurate pairing information.
            // This will insert the host into the database too.
            runPoll(fakeDetails, true, 0);
        }

        // If the machine is reachable, it was successful
        if (fakeDetails.state == ComputerDetails.State.ONLINE) {
            Log.i(TAG, "Adding new computer: " + fakeDetails);
            // Start a polling thread for this machine
            addTuple(fakeDetails);
            return true;
        } else {
            return false;
        }
    }

    public void removeComputer(ComputerDetails computer) {
        if (!getLocalDatabaseReference()) {
            return;
        }

        // Remove it from the database
        dbManager.deleteComputer(computer);

        synchronized (pollingTuples) {
            // Remove the computer from the computer list
            for (PollingTuple tuple : pollingTuples) {
                if (tuple.computer.uuid.equals(computer.uuid)) {
                    if (tuple.thread != null) {
                        // Interrupt the thread on this entry
                        tuple.thread.interrupt();
                        tuple.thread = null;
                    }
                    pollingTuples.remove(tuple);
                    break;
                }
            }
        }

        releaseLocalDatabaseReference();
    }

    @SuppressWarnings("BooleanMethodIsAlwaysInverted")
    private boolean getLocalDatabaseReference() {
        if (dbRefCount.get() == 0) {
            return false;
        }

        dbRefCount.incrementAndGet();
        return true;
    }

    private void releaseLocalDatabaseReference() {
        if (dbRefCount.decrementAndGet() == 0) {
            dbManager.close();
        }
    }

    private ComputerDetails tryPollIp(ComputerDetails details, ComputerDetails.AddressTuple address) {
        try {
            // If the current address's port number matches the active address's port number, we can also assume
            // the HTTPS port will also match. This assumption is currently safe because Sunshine sets all ports
            // as offsets from the base HTTP port and doesn't allow custom HttpsPort responses for WAN vs LAN.
            boolean portMatchesActiveAddress = details.state == ComputerDetails.State.ONLINE &&
                    details.activeAddress != null && address.port == details.activeAddress.port;

            NvHTTP http = new NvHTTP(address, portMatchesActiveAddress ? details.httpsPort : 0, idManager.getUniqueId(), details.serverCert,
                    PlatformBinding.getCryptoProvider(ComputerManagerService.this));

            // If this PC is currently online at this address, extend the timeouts to allow more time for the PC to respond.
            boolean isLikelyOnline = details.state == ComputerDetails.State.ONLINE && address.equals(details.activeAddress);

            ComputerDetails newDetails = http.getComputerDetails(isLikelyOnline);

            // Check if this is the PC we expected
            if (newDetails.uuid == null) {
                Log.i(TAG, "Polling returned no UUID!");
                return null;
            }
            // details.uuid can be null on initial PC add
            else if (details.uuid != null && !details.uuid.equals(newDetails.uuid)) {
                // We got the wrong PC!
                Log.i(TAG, "Polling returned the wrong PC!");
                return null;
            }

            return newDetails;
        } catch (XmlPullParserException e) {
            Log.e(TAG, "tryPollIp: " + e.getMessage(), e);
            return null;
        } catch (IOException e) {
            // Check if this was caused by thread interruption
            if (Thread.currentThread().isInterrupted()) {
                return null;
            }
            return null;
        } catch (Exception e) {
            // Catch any other unexpected exceptions to prevent crashes
            Log.w(TAG, "Unexpected exception in tryPollIp: " + e.getMessage(), e);
            return null;
        }
    }

    private static class ParallelPollTuple {
        public ComputerDetails.AddressTuple address;
        public ComputerDetails existingDetails;

        public boolean complete;
        public Thread pollingThread;
        public ComputerDetails returnedDetails;

        public ParallelPollTuple(ComputerDetails.AddressTuple address, ComputerDetails existingDetails) {
            this.address = address;
            this.existingDetails = existingDetails;
        }

        public void interrupt() {
            if (pollingThread != null) {
                pollingThread.interrupt();
            }
        }
    }

    private void startParallelPollThread(ParallelPollTuple tuple, HashSet<ComputerDetails.AddressTuple> uniqueAddresses) {
        // Don't bother starting a polling thread for an address that doesn't exist
        // or if the address has already been polled with an earlier tuple
        if (tuple.address == null || !uniqueAddresses.add(tuple.address)) {
            tuple.complete = true;
            tuple.returnedDetails = null;
            return;
        }

        tuple.pollingThread = new Thread() {
            @Override
            public void run() {
                ComputerDetails details = tryPollIp(tuple.existingDetails, tuple.address);

                synchronized (tuple) {
                    tuple.complete = true; // Done
                    tuple.returnedDetails = details; // Polling result

                    tuple.notify();
                }
            }
        };
        tuple.pollingThread.setName("Parallel Poll - " + tuple.address + " - " + tuple.existingDetails.name);
        tuple.pollingThread.start();
    }

    /**
     * Configure WireGuard HTTP JNI for direct HTTP requests through the tunnel.
     * This enables all HTTP polling requests to be routed through WireGuard.
     */
    private void configureWireGuardHttp() {
        PreferenceConfiguration prefConfig = PreferenceConfiguration.readPreferences(this);
        
        if (!prefConfig.wgEnabled) {
            Log.i(TAG, "WireGuard is not enabled, skipping HTTP configuration");
            return;
        }
        
        // Check if all required WireGuard settings are present
        if (prefConfig.wgPrivateKey.isEmpty() || prefConfig.wgPeerPublicKey.isEmpty() || 
            prefConfig.wgEndpoint.isEmpty() || prefConfig.wgTunnelAddress.isEmpty()) {
            Log.w(TAG, "WireGuard settings incomplete, skipping HTTP configuration");
            return;
        }
        
        try {
            // Build WireGuard config
            WireGuardManager.Config wgConfig = new WireGuardManager.Config()
                    .setPrivateKeyBase64(prefConfig.wgPrivateKey)
                    .setPeerPublicKeyBase64(prefConfig.wgPeerPublicKey)
                    .setPresharedKeyBase64(prefConfig.wgPresharedKey.isEmpty() ? null : prefConfig.wgPresharedKey)
                    .setEndpoint(prefConfig.wgEndpoint)
                    .setTunnelAddress(prefConfig.wgTunnelAddress);

            // Configure WireGuard HTTP routing
            // The serverAddress parameter is not actually used by the Rust implementation,
            // so we pass a placeholder IP. WgSocket connects to any target IP through the tunnel.
            if (WireGuardManager.configureHttp(wgConfig, "0.0.0.0")) {
                wgHttpStartedByService = true;
                wgHttpConfigGeneration = WireGuardManager.getHttpConfigGeneration();
                Log.i(TAG, "WireGuard HTTP routing configured for polling (gen=" + wgHttpConfigGeneration + ")");
            } else {
                Log.e(TAG, "Failed to configure WireGuard HTTP routing");
            }
        } catch (Exception e) {
            Log.e(TAG, "Exception configuring WireGuard HTTP", e);
        }
    }

    /**
     * Tear down WireGuard HTTP JNI if we started it.
     */
    private void teardownWireGuardHttp() {
        if (wgHttpStartedByService) {
            // Only clear if the config is still ours (same generation).
            // If Game.startWireGuard() reconfigured HTTP, the generation will differ
            // and we must NOT clear it (that would kill Game's active session).
            int currentGen = WireGuardManager.getHttpConfigGeneration();
            if (currentGen == wgHttpConfigGeneration) {
                WireGuardManager.clearHttpConfig();
                Log.i(TAG, "WireGuard HTTP JNI torn down (gen=" + wgHttpConfigGeneration + ")");
            } else {
                Log.i(TAG, "Skipping WG HTTP teardown - config was reconfigured externally (our gen=" +
                        wgHttpConfigGeneration + ", current gen=" + currentGen + ")");
            }
            wgHttpStartedByService = false;
            wgHttpConfigGeneration = -1;
        }
    }

    /**
     * Get WireGuard server address for polling.
     * Note: Removed - WireGuard target address is now dynamic per-connection.
     */
    private ComputerDetails.AddressTuple getWireGuardServerAddress() {
        // WireGuard server address is now dynamic per-connection
        // No static address to return for polling
        return null;
    }

    private ComputerDetails parallelPollPc(ComputerDetails details) throws InterruptedException {
        ParallelPollTuple localInfo = new ParallelPollTuple(details.localAddress, details);
        ParallelPollTuple manualInfo = new ParallelPollTuple(details.manualAddress, details);
        ParallelPollTuple remoteInfo = new ParallelPollTuple(details.remoteAddress, details);
        ParallelPollTuple ipv6Info = new ParallelPollTuple(details.ipv6Address, details);

        // When WireGuard is active, also poll the server's WireGuard tunnel address
        ComputerDetails.AddressTuple wgAddress = getWireGuardServerAddress();
        ParallelPollTuple wgInfo = new ParallelPollTuple(wgAddress, details);

        // These must be started in order of precedence for the deduplication algorithm
        // to result in the correct behavior.
        HashSet<ComputerDetails.AddressTuple> uniqueAddresses = new HashSet<>();
        startParallelPollThread(localInfo, uniqueAddresses);
        startParallelPollThread(manualInfo, uniqueAddresses);
        startParallelPollThread(wgInfo, uniqueAddresses);
        startParallelPollThread(remoteInfo, uniqueAddresses);
        startParallelPollThread(ipv6Info, uniqueAddresses);

        try {
            // Check local first
            //noinspection SynchronizationOnLocalVariableOrMethodParameter
            synchronized (localInfo) {
                while (!localInfo.complete) {
                    localInfo.wait();
                }

                if (localInfo.returnedDetails != null) {
                    localInfo.returnedDetails.activeAddress = localInfo.address;
                    return localInfo.returnedDetails;
                }
            }

            // Now manual
            //noinspection SynchronizationOnLocalVariableOrMethodParameter
            synchronized (manualInfo) {
                while (!manualInfo.complete) {
                    manualInfo.wait();
                }

                if (manualInfo.returnedDetails != null) {
                    manualInfo.returnedDetails.activeAddress = manualInfo.address;
                    return manualInfo.returnedDetails;
                }
            }

            // Now WireGuard tunnel address
            //noinspection SynchronizationOnLocalVariableOrMethodParameter
            synchronized (wgInfo) {
                while (!wgInfo.complete) {
                    wgInfo.wait();
                }

                if (wgInfo.returnedDetails != null) {
                    wgInfo.returnedDetails.activeAddress = wgInfo.address;
                    return wgInfo.returnedDetails;
                }
            }

            // Now remote IPv4
            //noinspection SynchronizationOnLocalVariableOrMethodParameter
            synchronized (remoteInfo) {
                while (!remoteInfo.complete) {
                    remoteInfo.wait();
                }

                if (remoteInfo.returnedDetails != null) {
                    remoteInfo.returnedDetails.activeAddress = remoteInfo.address;
                    return remoteInfo.returnedDetails;
                }
            }

            // Now global IPv6
            //noinspection SynchronizationOnLocalVariableOrMethodParameter
            synchronized (ipv6Info) {
                while (!ipv6Info.complete) {
                    ipv6Info.wait();
                }

                if (ipv6Info.returnedDetails != null) {
                    ipv6Info.returnedDetails.activeAddress = ipv6Info.address;
                    return ipv6Info.returnedDetails;
                }
            }
        } finally {
            // Stop any further polling if we've found a working address or we've been
            // interrupted by an attempt to stop polling.
            localInfo.interrupt();
            manualInfo.interrupt();
            wgInfo.interrupt();
            remoteInfo.interrupt();
            ipv6Info.interrupt();
        }

        return null;
    }

    private boolean pollComputer(ComputerDetails details) throws InterruptedException {
        // Poll all addresses in parallel to speed up the process
        Log.i(TAG, "Starting parallel poll for " + details.name + " (" + details.localAddress + ", " + details.remoteAddress + ", " + details.manualAddress + ", " + details.ipv6Address + ")");
        ComputerDetails polledDetails = parallelPollPc(details);
        Log.i(TAG, "Parallel poll for " + details.name + " returned address: " + details.activeAddress);

        if (polledDetails != null) {
            details.update(polledDetails);
            return true;
        } else {
            return false;
        }
    }

    @Override
    public void onCreate() {
        // Bind to the discovery service
        bindService(new Intent(this, DiscoveryService.class),
                discoveryServiceConnection, Service.BIND_AUTO_CREATE);

        // Lookup or generate this device's UID
        idManager = new IdentityManager(this);

        // Initialize the DB
        dbManager = new ComputerDatabaseManager(this);
        dbRefCount.set(1);

        // Grab known machines into our computer list
        if (!getLocalDatabaseReference()) {
            return;
        }

        for (ComputerDetails computer : dbManager.getAllComputers()) {
            // Add tuples for each computer
            addTuple(computer);
        }

        releaseLocalDatabaseReference();

        // Monitor for network changes to invalidate our PC state
        networkCallback = new ConnectivityManager.NetworkCallback() {
            @Override
            public void onAvailable(@NonNull Network network) {
                Log.i(TAG, "Resetting PC state for new available network");
                synchronized (pollingTuples) {
                    for (PollingTuple tuple : pollingTuples) {
                        tuple.computer.state = ComputerDetails.State.UNKNOWN;
                        if (listener != null) {
                            listener.notifyComputerUpdated(tuple.computer);
                        }
                    }
                }
            }

            @Override
            public void onLost(@NonNull Network network) {
                Log.i(TAG, "Offlining PCs due to network loss");
                synchronized (pollingTuples) {
                    for (PollingTuple tuple : pollingTuples) {
                        tuple.computer.state = ComputerDetails.State.OFFLINE;
                        if (listener != null) {
                            listener.notifyComputerUpdated(tuple.computer);
                        }
                    }
                }
            }
        };

        ConnectivityManager connMgr = (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);
        connMgr.registerDefaultNetworkCallback(networkCallback);

        // Configure WireGuard HTTP JNI if WireGuard is enabled and the tunnel is active.
        // This ensures polling requests go through the JNI WireGuard HTTP client
        // instead of OkHttp (which can't reach through the userspace tunnel).
        configureWireGuardHttp();

        // Monitor for WireGuard tunnel state changes to trigger re-polling
        // Since the userspace WireGuard tunnel doesn't trigger ConnectivityManager callbacks,
        // we need to explicitly handle tunnel state transitions.
        WireGuardManager.setStatusCallback(new WireGuardManager.StatusCallback() {
            @Override
            public void onConnecting() {
                // Nothing to do while connecting
            }

            @Override
            public void onConnected() {
                Log.i(TAG, "WireGuard tunnel connected, configuring HTTP JNI and resetting PC state");
                configureWireGuardHttp();
                synchronized (pollingTuples) {
                    for (PollingTuple tuple : pollingTuples) {
                        tuple.computer.state = ComputerDetails.State.UNKNOWN;
                        if (listener != null) {
                            listener.notifyComputerUpdated(tuple.computer);
                        }
                    }
                }
            }

            @Override
            public void onDisconnected() {
                Log.i(TAG, "WireGuard tunnel disconnected, disabling HTTP JNI and resetting PC state");
                teardownWireGuardHttp();
                synchronized (pollingTuples) {
                    for (PollingTuple tuple : pollingTuples) {
                        tuple.computer.state = ComputerDetails.State.UNKNOWN;
                        if (listener != null) {
                            listener.notifyComputerUpdated(tuple.computer);
                        }
                    }
                }
            }

            @Override
            public void onError(String error) {
                Log.w(TAG, "WireGuard tunnel error: " + error);
            }
        });
    }

    /**
     * Awaits termination of all polling threads with a timeout.
     * This prevents indefinite blocking when HttpURLConnection operations don't respond to interrupts.
     */
    private void awaitPollingTermination(@SuppressWarnings("SameParameterValue") long timeoutMs) {
        long startTime = SystemClock.elapsedRealtime();
        boolean allTerminated;

        do {
            allTerminated = true;
            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    if (tuple.thread != null && tuple.thread.isAlive()) {
                        allTerminated = false;
                        break;
                    }
                }
            }

            if (!allTerminated) {
                try {
                    Thread.sleep(100);
                } catch (InterruptedException e) {
                    Log.w(TAG, "Interrupted while awaiting polling termination");
                    Thread.currentThread().interrupt();
                    break;
                }
            }
        } while (!allTerminated && (SystemClock.elapsedRealtime() - startTime) < timeoutMs);

        if (!allTerminated) {
            Log.w(TAG, "Some polling threads did not terminate within " + timeoutMs + "ms timeout");
        } else {
            Log.i(TAG, "All polling threads terminated successfully");
        }
    }

    @Override
    public void onDestroy() {
        ConnectivityManager connMgr = (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);
        connMgr.unregisterNetworkCallback(networkCallback);

        // Clean up WireGuard HTTP JNI if we started it
        teardownWireGuardHttp();

        // Clear the WireGuard status callback to avoid leaking this service
        WireGuardManager.setStatusCallback(null);

        if (discoveryBinder != null) {
            // Unbind from the discovery service
            unbindService(discoveryServiceConnection);
        }

        // Stop polling if still active and wait for threads to terminate
        // Use a timeout to avoid blocking indefinitely due to HttpURLConnection timeout issues
        pollingActive = false;
        synchronized (pollingTuples) {
            for (PollingTuple tuple : pollingTuples) {
                if (tuple.thread != null) {
                    tuple.thread.interrupt();
                }
            }
        }

        // Wait up to 2 seconds for threads to terminate gracefully
        awaitPollingTermination(2000);

        // Remove the initial DB reference
        releaseLocalDatabaseReference();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return binder;
    }

    public class ApplistPoller {
        private Thread thread;
        private final ComputerDetails computer;
        private final Object pollEvent = new Object();
        private boolean receivedAppList = false;

        public ApplistPoller(ComputerDetails computer) {
            this.computer = computer;
        }

        public void pollNow() {
            synchronized (pollEvent) {
                pollEvent.notify();
            }
        }

        private boolean waitPollingDelay() {
            try {
                synchronized (pollEvent) {
                    if (receivedAppList) {
                        // If we've already reported an app list successfully,
                        // wait the full polling period
                        pollEvent.wait(APPLIST_POLLING_PERIOD_MS);
                    } else {
                        // If we've failed to get an app list so far, retry much earlier
                        pollEvent.wait(APPLIST_FAILED_POLLING_RETRY_MS);
                    }
                }
            } catch (InterruptedException e) {
                return false;
            }

            return thread != null && !thread.isInterrupted();
        }

        private PollingTuple getPollingTuple(ComputerDetails details) {
            synchronized (pollingTuples) {
                for (PollingTuple tuple : pollingTuples) {
                    if (details.uuid.equals(tuple.computer.uuid)) {
                        return tuple;
                    }
                }
            }

            return null;
        }

        public void start() {
            thread = new Thread(() -> {
                int emptyAppListResponses = 0;
                do {
                    // Can't poll if it's not online or paired
                    if (computer.state != ComputerDetails.State.ONLINE ||
                            computer.pairState != PairingManager.PairState.PAIRED) {
                        if (listener != null) {
                            listener.notifyComputerUpdated(computer);
                        }
                        continue;
                    }

                    // Can't poll if there's no UUID yet
                    if (computer.uuid == null) {
                        continue;
                    }

                    PollingTuple tuple = getPollingTuple(computer);

                    try {
                        NvHTTP http = new NvHTTP(ServerHelper.getCurrentAddressFromComputer(computer), computer.httpsPort, idManager.getUniqueId(),
                                computer.serverCert, PlatformBinding.getCryptoProvider(ComputerManagerService.this));

                        String appList;
                        if (tuple != null) {
                            // If we're polling this machine too, grab the network lock
                            // while doing the app list request to prevent other requests
                            // from being issued in the meantime.
                            synchronized (tuple.networkLock) {
                                appList = http.getAppListRaw();
                            }
                        } else {
                            // No polling is happening now, so we just call it directly
                            appList = http.getAppListRaw();
                        }

                        List<NvApp> list = NvHTTP.getAppListByReader(new StringReader(appList));
                        if (list.isEmpty()) {
                            Log.i(TAG, "Empty app list received from " + computer.uuid);

                            // The app list might actually be empty, so if we get an empty response a few times
                            // in a row, we'll go ahead and believe it.
                            emptyAppListResponses++;
                        }
                        if (!appList.isEmpty() &&
                                (!list.isEmpty() || emptyAppListResponses >= EMPTY_LIST_THRESHOLD)) {
                            // Open the cache file
                            try (final OutputStream cacheOut = CacheHelper.openCacheFileForOutput(
                                    getCacheDir(), "applist", computer.uuid)
                            ) {
                                CacheHelper.writeStringToOutputStream(cacheOut, appList);
                            } catch (IOException e) {
                                Log.e(TAG, "Failed to write app list cache for " + computer.uuid + ": " + e.getMessage(), e);
                            }

                            // Reset empty count if it wasn't empty this time
                            if (!list.isEmpty()) {
                                emptyAppListResponses = 0;
                            }

                            // Update the computer
                            computer.rawAppList = appList;
                            receivedAppList = true;

                            // Notify that the app list has been updated
                            // and ensure that the thread is still active
                            if (listener != null && thread != null) {
                                listener.notifyComputerUpdated(computer);
                            }
                        } else if (appList.isEmpty()) {
                            Log.w(TAG, "Null app list received from " + computer.uuid);
                        }
                    } catch (IOException e) {
                        Log.w(TAG, "IOException while polling app list for " + computer.uuid + ": " + e.getMessage(), e);
                    } catch (XmlPullParserException e) {
                        Log.w(TAG, "XmlPullParserException while polling app list for " + computer.uuid + ": " + e.getMessage(), e);
                    }
                } while (waitPollingDelay());
            });
            thread.setName("App list polling thread for " + computer.name);
            thread.start();
        }

        public void stop() {
            if (thread != null) {
                thread.interrupt();

                // Don't join here because we might be blocked on network I/O

                thread = null;
            }
        }
    }
}

class PollingTuple {
    public Thread thread;
    public final ComputerDetails computer;
    public final Object networkLock;
    public long lastSuccessfulPollMs;

    public PollingTuple(ComputerDetails computer, Thread thread) {
        this.computer = computer;
        this.thread = thread;
        this.networkLock = new Object();
    }
}

class ReachabilityTuple {
    public final String reachableAddress;
    public final ComputerDetails computer;

    public ReachabilityTuple(ComputerDetails computer, String reachableAddress) {
        this.computer = computer;
        this.reachableAddress = reachableAddress;
    }
}