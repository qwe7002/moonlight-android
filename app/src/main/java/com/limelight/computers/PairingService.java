package com.limelight.computers;

import android.annotation.SuppressLint;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.os.Binder;
import android.os.IBinder;
import android.util.Log;

import com.limelight.PcView;
import com.limelight.R;
import com.limelight.binding.PlatformBinding;
import com.limelight.binding.video.WireGuardManager;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.NvHTTP;
import com.limelight.nvstream.http.PairingManager;
import com.limelight.nvstream.http.PairingManager.PairState;

import java.io.FileNotFoundException;
import java.net.UnknownHostException;
import java.security.cert.X509Certificate;

public class PairingService extends Service {
    private static final String TAG = "PairingService";
    private static final String CHANNEL_ID = "pairing_channel";
    private static final int NOTIFICATION_ID = 2001;

    public static final String EXTRA_COMPUTER_UUID = "computer_uuid";
    public static final String EXTRA_COMPUTER_NAME = "computer_name";
    public static final String EXTRA_COMPUTER_ADDRESS = "computer_address";
    public static final String EXTRA_COMPUTER_HTTP_PORT = "computer_http_port";
    public static final String EXTRA_COMPUTER_HTTPS_PORT = "computer_https_port";
    public static final String EXTRA_SERVER_CERT = "server_cert";
    public static final String EXTRA_UNIQUE_ID = "unique_id";

    // Sunshine auto-pairing extras
    public static final String EXTRA_SUNSHINE_USERNAME = "sunshine_username";
    public static final String EXTRA_SUNSHINE_PASSWORD = "sunshine_password";

    public static final String ACTION_CANCEL_PAIRING = "com.limelight.CANCEL_PAIRING";

    private NotificationManager notificationManager;
    private final PairingBinder binder = new PairingBinder();
    private PairingListener listener;
    private Thread pairingThread;
    private volatile boolean cancelled = false;
    private String currentPin;
    
    // WireGuard proxy state
    private volatile boolean wgProxyStarted = false;
    private String wgServerAddress = null;

    public interface PairingListener {
        void onPairingSuccess(String computerUuid, X509Certificate serverCert);

        void onPairingFailed(String computerUuid, String message);
    }

    public class PairingBinder extends Binder {
        public void setListener(PairingListener listener) {
            PairingService.this.listener = listener;
        }

        @SuppressWarnings("unused")
        public void cancelPairing() {
            cancelled = true;
            if (pairingThread != null) {
                pairingThread.interrupt();
            }
            stopSelf();
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        notificationManager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        createNotificationChannel();
    }

    private void createNotificationChannel() {
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                getString(R.string.pairing_notification_channel_name),
                NotificationManager.IMPORTANCE_HIGH
        );
        channel.setDescription(getString(R.string.pairing_notification_channel_description));
        channel.setShowBadge(true);
        channel.enableVibration(true);
        notificationManager.createNotificationChannel(channel);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            stopSelf();
            return START_NOT_STICKY;
        }

        String action = intent.getAction();
        if (ACTION_CANCEL_PAIRING.equals(action)) {
            cancelled = true;
            if (pairingThread != null) {
                pairingThread.interrupt();
            }
            stopForeground(STOP_FOREGROUND_REMOVE);
            stopSelf();
            return START_NOT_STICKY;
        }

        final String computerUuid = intent.getStringExtra(EXTRA_COMPUTER_UUID);
        final String computerName = intent.getStringExtra(EXTRA_COMPUTER_NAME);
        final String computerAddress = intent.getStringExtra(EXTRA_COMPUTER_ADDRESS);
        final int httpPort = intent.getIntExtra(EXTRA_COMPUTER_HTTP_PORT, NvHTTP.DEFAULT_HTTP_PORT);
        final int httpsPort = intent.getIntExtra(EXTRA_COMPUTER_HTTPS_PORT, 0);
        final byte[] serverCertBytes = intent.getByteArrayExtra(EXTRA_SERVER_CERT);
        final String uniqueId = intent.getStringExtra(EXTRA_UNIQUE_ID);

        // Sunshine auto-pairing credentials
        final String sunshineUsername = intent.getStringExtra(EXTRA_SUNSHINE_USERNAME);
        final String sunshinePassword = intent.getStringExtra(EXTRA_SUNSHINE_PASSWORD);

        if (computerUuid == null || computerAddress == null || uniqueId == null) {
            stopSelf();
            return START_NOT_STICKY;
        }

        if (sunshineUsername == null || sunshinePassword == null) {
            stopSelf();
            return START_NOT_STICKY;
        }

        // Generate PIN
        currentPin = PairingManager.generatePinString();

        // Get device name for pairing
        final String deviceName = android.os.Build.MODEL;

        // Show notification
        showPairingNotification(computerName, deviceName);

        // Start pairing in background using Sunshine API
        cancelled = false;
        pairingThread = new Thread(() ->
                doSunshinePairing(computerUuid, computerName, computerAddress, httpPort, httpsPort,
                        serverCertBytes, uniqueId, currentPin, sunshineUsername, sunshinePassword, deviceName));
        pairingThread.start();

        return START_STICKY;
    }


    private void showPairingNotification(String computerName, String deviceName) {
        // Intent to open PcView
        Intent openIntent = new Intent(this, PcView.class);
        openIntent.setFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent openPendingIntent = PendingIntent.getActivity(
                this, 0, openIntent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        // Intent to cancel pairing
        Intent cancelIntent = new Intent(this, PairingService.class);
        cancelIntent.setAction(ACTION_CANCEL_PAIRING);
        PendingIntent cancelPendingIntent = PendingIntent.getService(
                this, 2, cancelIntent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        String title = getString(R.string.pairing_notification_title, computerName);
        String content = getString(R.string.pairing_notification_auto_content, deviceName);

        Notification.Builder builder = new Notification.Builder(this, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(title)
                .setContentText(content)
                .setStyle(new Notification.BigTextStyle().bigText(content))
                .setOngoing(true)
                .setAutoCancel(false)
                .setContentIntent(openPendingIntent)
                .addAction(new Notification.Action.Builder(
                        null, getString(android.R.string.cancel), cancelPendingIntent).build())
                .setCategory(Notification.CATEGORY_PROGRESS);

        startForeground(NOTIFICATION_ID, builder.build());
    }

    private void updateNotificationSuccess(String computerName) {
        Notification.Builder builder = new Notification.Builder(this, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(getString(R.string.pairing_notification_success_title))
                .setContentText(getString(R.string.pairing_notification_success_content, computerName))
                .setAutoCancel(true)
                .setTimeoutAfter(5000)
                .setCategory(Notification.CATEGORY_STATUS);

        notificationManager.notify(NOTIFICATION_ID + 1, builder.build());
    }

    private void updateNotificationFailed(String computerName, String reason) {
        Notification.Builder builder = new Notification.Builder(this, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(getString(R.string.pairing_notification_failed_title))
                .setContentText(getString(R.string.pairing_notification_failed_content, computerName, reason))
                .setAutoCancel(true)
                .setTimeoutAfter(10000)
                .setCategory(Notification.CATEGORY_ERROR);

        notificationManager.notify(NOTIFICATION_ID + 1, builder.build());
    }


    /**
     * Perform pairing using Sunshine's REST API with username/password authentication
     * Flow: pm.pair() sends pairing request -> server waits for PIN -> /api/pin submits PIN -> pairing completes
     */
    private void doSunshinePairing(String computerUuid, String computerName, String computerAddress,
                                   int httpPort, int httpsPort, byte[] serverCertBytes, String uniqueId,
                                   String pin, String username, String password, String deviceName) {
        String message = null;
        X509Certificate pairedCert = null;
        boolean success = false;

        // Setup WireGuard proxy if enabled
        setupWireGuardProxy();
        
        // Use effective address (WireGuard server address if enabled)
        String effectiveAddress = getEffectiveAddress(computerAddress);

        try {
            java.security.cert.X509Certificate serverCert = null;
            if (serverCertBytes != null) {
                java.security.cert.CertificateFactory cf = java.security.cert.CertificateFactory.getInstance("X.509");
                serverCert = (java.security.cert.X509Certificate) cf.generateCertificate(
                        new java.io.ByteArrayInputStream(serverCertBytes));
            }

            ComputerDetails.AddressTuple addressTuple = new ComputerDetails.AddressTuple(effectiveAddress, httpPort);

            NvHTTP httpConn = new NvHTTP(
                    addressTuple,
                    httpsPort, uniqueId, serverCert,
                    PlatformBinding.getCryptoProvider(this));

            if (httpConn.getPairState() == PairState.PAIRED) {
                success = true;
                pairedCert = httpConn.getPairingManager().getPairedCert();
            } else {
                // Step 1: Verify Sunshine credentials before starting pairing
                Log.i(TAG, "Verifying Sunshine credentials...");
                int verifyResult = verifySunshineCredentials(effectiveAddress, username, password);
                if (verifyResult == 401) {
                    Log.e(TAG, "Sunshine authentication failed - invalid credentials");
                    message = getString(R.string.sunshine_pairing_auth_failed);
                } else if (verifyResult != 200 && verifyResult != -2 && verifyResult != -1) {
                    // -2 means endpoint not found (older Sunshine), proceed with pairing
                    // -1 means network error (proxy issue, etc.), also proceed with pairing
                    Log.e(TAG, "Failed to verify Sunshine credentials, response code: " + verifyResult);
                    message = getString(R.string.pair_fail);
                } else {
                    if (verifyResult == -1) {
                        Log.w(TAG, "Sunshine credential verification had network error, proceeding with pairing anyway");
                    }
                    // Credentials verified or verification not supported, proceed with pairing
                    PairingManager pm = httpConn.getPairingManager();

                    // Use AtomicBoolean to capture PIN submission result from background thread
                    final java.util.concurrent.atomic.AtomicBoolean pinSubmitSuccess = new java.util.concurrent.atomic.AtomicBoolean(false);
                    final java.util.concurrent.atomic.AtomicBoolean pinSubmitAuthFailed = new java.util.concurrent.atomic.AtomicBoolean(false);

                    // Schedule PIN submission to run after a short delay (to ensure pm.pair() has started)
                    final String finalEffectiveAddress = effectiveAddress;
                    final Thread currentPairingThread = pairingThread;
                    Thread pinThread = new Thread(() -> {
                        try {
                            // Wait a bit for pm.pair() to start and send the initial pairing request
                            Thread.sleep(500);
                            Log.i(TAG, "Submitting PIN to Sunshine API...");
                            int result = sendPinToSunshine(finalEffectiveAddress, username, password, pin, deviceName);
                            if (result == 200) {
                                Log.i(TAG, "PIN submitted successfully");
                                pinSubmitSuccess.set(true);
                            } else if (result == 401) {
                                Log.e(TAG, "Authentication failed (401) - invalid credentials");
                                pinSubmitAuthFailed.set(true);
                                // Interrupt the pairing thread to stop waiting
                                if (currentPairingThread != null) {
                                    currentPairingThread.interrupt();
                                }
                            } else {
                                Log.e(TAG, "Failed to submit PIN to Sunshine API, response code: " + result);
                            }
                        } catch (InterruptedException e) {
                            Thread.currentThread().interrupt();
                        }
                    });
                    pinThread.start();

                    // Step 2: This call blocks until server receives PIN and completes pairing
                    try {
                        PairState pairState = pm.pair(pin);

                        if (pairState == PairState.PIN_WRONG) {
                            message = getString(R.string.pair_incorrect_pin);
                        } else if (pairState == PairState.FAILED) {
                            // Check if it was due to authentication failure
                            if (pinSubmitAuthFailed.get()) {
                                message = getString(R.string.sunshine_pairing_auth_failed);
                            } else {
                                message = getString(R.string.pair_fail);
                            }
                        } else if (pairState == PairState.ALREADY_IN_PROGRESS) {
                            message = getString(R.string.pair_already_in_progress);
                        } else if (pairState == PairState.PAIRED) {
                            success = true;
                            pairedCert = pm.getPairedCert();
                        }
                    } catch (Exception e) {
                        // Check if interrupted due to auth failure
                        if (pinSubmitAuthFailed.get()) {
                            message = getString(R.string.sunshine_pairing_auth_failed);
                        } else if (!cancelled) {
                            throw e;
                        }
                    }
                }
            }
        } catch (UnknownHostException e) {
            message = getString(R.string.error_unknown_host);
        } catch (FileNotFoundException e) {
            message = getString(R.string.error_404);
        } catch (Exception e) {
            //LimeLog.warning("Sunshine pairing failed: " + e.getMessage());
            Log.e(TAG, "Sunshine pairing failed: " + e.getMessage(), e);
            message = e.getMessage();
        }

        if (cancelled) {
            stopForeground(STOP_FOREGROUND_REMOVE);
            stopWireGuardProxy();
            stopSelf();
            return;
        }

        stopForeground(STOP_FOREGROUND_REMOVE);
        
        // Clean up WireGuard proxy 
        stopWireGuardProxy();

        if (success) {
            updateNotificationSuccess(computerName);
            if (listener != null) {
                listener.onPairingSuccess(computerUuid, pairedCert);
            }
        } else {
            updateNotificationFailed(computerName, message != null ? message : "Unknown error");
            if (listener != null) {
                listener.onPairingFailed(computerUuid, message);
            }
        }

        stopSelf();
    }

    /**
     * Ensure a TCP proxy for Sunshine API port 47990 exists.
     * If WireGuard is enabled but the proxy doesn't exist yet, try to create it.
     * @return Local proxy port (>0) if available, -1 if WG not enabled or proxy creation failed
     */
    private int ensureSunshineProxyPort() {
        Log.i(TAG, "ensureSunshineProxyPort called, wgProxyStarted=" + wgProxyStarted);
        if (!wgProxyStarted) {
            Log.w(TAG, "WireGuard proxy not started, cannot provide Sunshine proxy port");
            return -1;
        }
        
        int proxyPort = WireGuardManager.getTcpProxyPort(47990);
        Log.i(TAG, "WireGuardManager.getTcpProxyPort(47990) returned " + proxyPort);
        if (proxyPort > 0) {
            Log.i(TAG, "Sunshine API proxy already exists on port " + proxyPort);
            return proxyPort;
        }
        
        // Proxy not found - try to create it with retries
        Log.w(TAG, "Sunshine API proxy for port 47990 not found (getTcpProxyPort returned " + proxyPort + "), creating...");
        for (int attempt = 0; attempt < 3; attempt++) {
            if (attempt > 0) {
                Log.i(TAG, "Retrying Sunshine proxy creation (attempt " + (attempt + 1) + ")");
                try { Thread.sleep(100); } catch (InterruptedException ignored) {}
            }
            proxyPort = WireGuardManager.createTcpProxy(47990);
            if (proxyPort > 0) {
                Log.i(TAG, "Sunshine API proxy created on-the-fly on port " + proxyPort);
                return proxyPort;
            }
        }
        Log.e(TAG, "Failed to create Sunshine API proxy for port 47990 after retries");
        return proxyPort;
    }

    /**
     * Verify Sunshine credentials by calling a simple API endpoint
     *
     * @return HTTP response code (200 = success, 401 = auth failed, -2 = endpoint not found, -1 = error)
     */
    @SuppressLint("CustomX509TrustManager")
    private int verifySunshineCredentials(String computerAddress, String username, String password) {
        Log.i(TAG, ">>> verifySunshineCredentials CALLED: address=" + computerAddress);
        javax.net.ssl.HttpsURLConnection connection = null;
        try {
            // Build URL for Sunshine API - use /api/apps as a simple endpoint to verify auth
            String host = computerAddress;
            if (host.contains(":") && !host.startsWith("[")) {
                host = "[" + host + "]";
            }
            String url = "https://" + host + ":47990/api/apps";

            Log.i(TAG, "Verifying Sunshine credentials: " + url);

            // Create Basic Auth header
            String credentials = username + ":" + password;
            String basicAuth = "Basic " + android.util.Base64.encodeToString(
                    credentials.getBytes(java.nio.charset.StandardCharsets.UTF_8), android.util.Base64.NO_WRAP);

            // Create trust manager that accepts all certificates (for self-signed Sunshine certs)
            javax.net.ssl.TrustManager[] trustAllCerts = new javax.net.ssl.TrustManager[]{
                    new javax.net.ssl.X509TrustManager() {
                        public java.security.cert.X509Certificate[] getAcceptedIssuers() {
                            return new java.security.cert.X509Certificate[0];
                        }

                        @SuppressLint("TrustAllX509TrustManager")
                        public void checkClientTrusted(java.security.cert.X509Certificate[] certs, String authType) {
                        }

                        @SuppressLint("TrustAllX509TrustManager")
                        public void checkServerTrusted(java.security.cert.X509Certificate[] certs, String authType) {
                        }
                    }
            };

            // Create SSL context with trust-all manager
            javax.net.ssl.SSLContext sslContext = javax.net.ssl.SSLContext.getInstance("TLS");
            sslContext.init(null, trustAllCerts, new java.security.SecureRandom());
            javax.net.ssl.SSLSocketFactory sslSocketFactory = sslContext.getSocketFactory();
            javax.net.ssl.HostnameVerifier trustAllHostnames = (hostname, session) -> true;

            java.net.URL apiUrl = new java.net.URL(url);
            Log.i(TAG, "verifySunshineCredentials: initial URL=" + url + ", wgProxyStarted=" + wgProxyStarted);

            // Use TCP proxy through WireGuard if available
            int proxyPort = ensureSunshineProxyPort();
            Log.i(TAG, "verifySunshineCredentials: ensureSunshineProxyPort returned " + proxyPort);
            if (proxyPort > 0) {
                String proxyUrl = "https://127.0.0.1:" + proxyPort + "/api/apps";
                Log.i(TAG, "Using WireGuard TCP proxy for Sunshine API: " + proxyUrl);
                apiUrl = new java.net.URL(proxyUrl);
            } else if (wgProxyStarted) {
                Log.e(TAG, "WireGuard enabled but no proxy available for port 47990, direct connection will likely fail");
            } else {
                Log.i(TAG, "WireGuard not enabled, using direct connection to " + url);
            }

            connection = (javax.net.ssl.HttpsURLConnection) apiUrl.openConnection();
            Log.i(TAG, "verifySunshineCredentials: connecting to " + apiUrl.toString());

            connection.setSSLSocketFactory(sslSocketFactory);
            connection.setHostnameVerifier(trustAllHostnames);

            connection.setRequestMethod("GET");
            connection.setRequestProperty("Authorization", basicAuth);
            connection.setRequestProperty("Accept", "*/*");
            connection.setConnectTimeout(15000);
            connection.setReadTimeout(15000);

            int responseCode = connection.getResponseCode();
            Log.i(TAG, "Sunshine credentials verification response code: " + responseCode);

            if (responseCode == 200) {
                return 200;
            } else if (responseCode == 401) {
                Log.w(TAG, "Sunshine authentication failed (401)");
                return 401;
            } else if (responseCode == 404) {
                // Endpoint not found - older Sunshine version, proceed with pairing
                Log.i(TAG, "API endpoint not found, proceeding with pairing");
                return -2;
            } else {
                return responseCode;
            }
        } catch (java.io.FileNotFoundException e) {
            // 404 - endpoint not found
            Log.i(TAG, "API endpoint not found (FileNotFoundException), proceeding with pairing");
            return -2;
        } catch (Exception e) {
            Log.e(TAG, "Failed to verify Sunshine credentials: " + e.getMessage(), e);
            return -1;
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    /**
     * Send PIN to Sunshine server via its REST API
     *
     * @return HTTP response code (200 = success, 401 = auth failed, -1 = error)
     */
    private int sendPinToSunshine(String computerAddress, String username, String password,
                                      String pin, String deviceName) {
        javax.net.ssl.HttpsURLConnection connection = null;
        try {
            // Build URL for Sunshine API
            String host = computerAddress;
            if (host.contains(":") && !host.startsWith("[")) {
                host = "[" + host + "]";
            }
            String url = "https://" + host + ":47990/api/pin";

            //LimeLog.info("Sending PIN to Sunshine API: " + url);
            Log.i(TAG, "Sending PIN to Sunshine API: " + url);

            // Create JSON payload
            org.json.JSONObject jsonPayload = new org.json.JSONObject();
            jsonPayload.put("pin", pin);
            jsonPayload.put("name", deviceName);

            // Create Basic Auth header
            String credentials = username + ":" + password;
            String basicAuth = "Basic " + android.util.Base64.encodeToString(
                    credentials.getBytes(java.nio.charset.StandardCharsets.UTF_8), android.util.Base64.NO_WRAP);

            // Create trust manager that accepts all certificates (for self-signed Sunshine certs)
            @SuppressLint("CustomX509TrustManager") javax.net.ssl.TrustManager[] trustAllCerts = new javax.net.ssl.TrustManager[]{
                    new javax.net.ssl.X509TrustManager() {
                        public java.security.cert.X509Certificate[] getAcceptedIssuers() {
                            return new java.security.cert.X509Certificate[0];
                        }

                        @SuppressLint("TrustAllX509TrustManager")
                        public void checkClientTrusted(java.security.cert.X509Certificate[] certs, String authType) {
                        }

                        @SuppressLint("TrustAllX509TrustManager")
                        public void checkServerTrusted(java.security.cert.X509Certificate[] certs, String authType) {
                        }
                    }
            };

            // Create SSL context with trust-all manager
            javax.net.ssl.SSLContext sslContext = javax.net.ssl.SSLContext.getInstance("TLS");
            sslContext.init(null, trustAllCerts, new java.security.SecureRandom());
            javax.net.ssl.SSLSocketFactory sslSocketFactory = sslContext.getSocketFactory();
            javax.net.ssl.HostnameVerifier trustAllHostnames = (hostname, session) -> true;

            java.net.URL apiUrl = new java.net.URL(url);
            Log.i(TAG, "sendPinToSunshine: initial URL=" + url + ", wgProxyStarted=" + wgProxyStarted);

            // Use TCP proxy through WireGuard if available
            int proxyPort = ensureSunshineProxyPort();
            Log.i(TAG, "sendPinToSunshine: ensureSunshineProxyPort returned " + proxyPort);
            if (proxyPort > 0) {
                String proxyUrl = "https://127.0.0.1:" + proxyPort + "/api/pin";
                Log.i(TAG, "Using WireGuard TCP proxy for Sunshine PIN API: " + proxyUrl);
                apiUrl = new java.net.URL(proxyUrl);
            } else if (wgProxyStarted) {
                Log.e(TAG, "WireGuard enabled but no proxy available for port 47990, PIN submission will likely fail");
            } else {
                Log.i(TAG, "WireGuard not enabled, using direct connection for PIN API");
            }

            connection = (javax.net.ssl.HttpsURLConnection) apiUrl.openConnection();

            // Set SSL configuration on the connection (not globally)
            connection.setSSLSocketFactory(sslSocketFactory);
            connection.setHostnameVerifier(trustAllHostnames);

            connection.setRequestMethod("POST");
            connection.setRequestProperty("Authorization", basicAuth);
            connection.setRequestProperty("Content-Type", "application/json");
            connection.setRequestProperty("Accept", "*/*");
            connection.setDoOutput(true);
            connection.setConnectTimeout(15000);
            connection.setReadTimeout(30000);

            // Send request
            byte[] input = jsonPayload.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8);
            try (java.io.OutputStream os = connection.getOutputStream()) {
                os.write(input, 0, input.length);
                os.flush();
            }

            int responseCode = connection.getResponseCode();
            //LimeLog.info("Sunshine API response code: " + responseCode);
            Log.i(TAG, "Sunshine API response code: " + responseCode);
            // 200 OK means PIN was accepted
            if (responseCode == 200) {
                return 200;
            } else if (responseCode == 401) {
                //LimeLog.warning("Sunshine API authentication failed (401)");
                Log.w(TAG, "Sunshine API authentication failed (401)");
                return 401;
            } else {
                // Try to read error message
                java.io.InputStream errorStream = connection.getErrorStream();
                if (errorStream != null) {
                    try (java.io.BufferedReader br = new java.io.BufferedReader(
                            new java.io.InputStreamReader(errorStream, java.nio.charset.StandardCharsets.UTF_8))) {
                        StringBuilder response = new StringBuilder();
                        String line;
                        while ((line = br.readLine()) != null) {
                            response.append(line);
                        }
                        //LimeLog.warning("Sunshine API error response: " + response);
                        Log.w(TAG, "Sunshine API error response: " + response);
                    }
                }
                return responseCode;
            }
        } catch (javax.net.ssl.SSLHandshakeException e) {
            /*LimeLog.warning("SSL Handshake failed: " + e.getMessage());
            LimeLog.warning("Stack trace: " + android.util.Log.getStackTraceString(e));*/
            Log.e(TAG, "SSL Handshake failed: " + e.getMessage(), e);
            return -1;
        } catch (java.net.SocketTimeoutException e) {
            //LimeLog.warning("Connection timeout: " + e.getMessage());
            Log.w(TAG, "Connection timeout: " + e.getMessage(), e);
            return -1;
        } catch (Exception e) {
            /*imeLog.warning("Failed to send PIN to Sunshine: " + e.getMessage());
            LimeLog.warning("Stack trace: " + android.util.Log.getStackTraceString(e));*/
            Log.e(TAG, "Failed to send PIN to Sunshine: " + e.getMessage(), e);
            return -1;
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    @Override
    public IBinder onBind(Intent intent) {
        return binder;
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        cancelled = true;
        if (pairingThread != null) {
            pairingThread.interrupt();
        }
        
        // Stop WireGuard proxy if we started it
        stopWireGuardProxy();
    }
    
    /**
     * Set up WireGuard direct HTTP and TCP proxy for pairing if enabled in preferences
     */
    private void setupWireGuardProxy() {
        SharedPreferences wgPrefs = getSharedPreferences("wireguard_config", Context.MODE_PRIVATE);
        boolean wgEnabled = wgPrefs.getBoolean("wg_enabled", false);
        Log.i(TAG, "setupWireGuardProxy: wg_enabled=" + wgEnabled);
        
        if (!wgEnabled) {
            return;
        }
        
        wgServerAddress = wgPrefs.getString("wg_server_address", "");
        String wgPrivateKey = wgPrefs.getString("wg_private_key", "");
        String wgPeerPublicKey = wgPrefs.getString("wg_peer_public_key", "");
        String wgPresharedKey = wgPrefs.getString("wg_preshared_key", "");
        String wgEndpoint = wgPrefs.getString("wg_peer_endpoint", "");
        String wgTunnelAddress = wgPrefs.getString("wg_tunnel_address", "10.0.0.2");
        
        if (wgServerAddress.isEmpty() || wgPrivateKey.isEmpty() || wgPeerPublicKey.isEmpty() || wgEndpoint.isEmpty()) {
            Log.w(TAG, "WireGuard enabled but configuration incomplete");
            return;
        }
        
        try {
            WireGuardManager.Config wgConfig = new WireGuardManager.Config()
                    .setPrivateKeyBase64(wgPrivateKey)
                    .setPeerPublicKeyBase64(wgPeerPublicKey)
                    .setPresharedKeyBase64(wgPresharedKey.isEmpty() ? null : wgPresharedKey)
                    .setEndpoint(wgEndpoint)
                    .setTunnelAddress(wgTunnelAddress);
            
            // Configure direct WireGuard HTTP (bypasses OkHttp via JNI for HTTP,
            // TCP proxy through WireGuard for HTTPS)
            if (WireGuardManager.configureHttp(wgConfig, wgServerAddress)) {
                NvHTTP.setUseDirectWgHttp(true);
                
                // Create TCP proxy for Sunshine HTTPS API (port 47990) with retry
                int sunshineProxyPort = -1;
                for (int attempt = 0; attempt < 3 && sunshineProxyPort <= 0; attempt++) {
                    if (attempt > 0) {
                        Log.i(TAG, "Retrying TCP proxy creation for port 47990 (attempt " + (attempt + 1) + ")");
                        try { Thread.sleep(100); } catch (InterruptedException ignored) {}
                    }
                    sunshineProxyPort = WireGuardManager.createTcpProxy(47990);
                }
                
                if (sunshineProxyPort > 0) {
                    Log.i(TAG, "TCP proxy for Sunshine API created on port " + sunshineProxyPort);
                    // Verify the proxy is accessible
                    int verifyPort = WireGuardManager.getTcpProxyPort(47990);
                    Log.i(TAG, "Sunshine API proxy verification: getTcpProxyPort(47990) = " + verifyPort);
                    wgProxyStarted = true;
                    Log.i(TAG, "WireGuard configured for pairing (HTTP via JNI, HTTPS via TCP proxy)");
                } else {
                    Log.e(TAG, "Failed to create TCP proxy for Sunshine API port 47990 after retries");
                    // Still enable WireGuard for HTTP but warn about HTTPS
                    wgProxyStarted = true;
                    Log.w(TAG, "WireGuard HTTP enabled but HTTPS proxy may not work");
                }
            } else {
                Log.e(TAG, "Failed to configure WireGuard HTTP for pairing");
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to setup WireGuard for pairing", e);
        }
    }
    
    /**
     * Stop WireGuard HTTP and TCP proxies if we started them
     */
    private void stopWireGuardProxy() {
        if (wgProxyStarted) {
            // Clear direct HTTP config
            WireGuardManager.clearHttpConfig();
            NvHTTP.setUseDirectWgHttp(false);
            
            // Stop TCP proxies
            WireGuardManager.stopTcpProxies();
            
            wgProxyStarted = false;
            Log.i(TAG, "WireGuard stopped after pairing");
        }
    }
    
    /**
     * Get the address to use for connections (WireGuard server address if enabled, original otherwise)
     */
    private String getEffectiveAddress(String originalAddress) {
        if (wgProxyStarted && wgServerAddress != null && !wgServerAddress.isEmpty()) {
            return wgServerAddress;
        }
        return originalAddress;
    }
}
