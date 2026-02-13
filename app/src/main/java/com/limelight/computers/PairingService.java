package com.limelight.computers;

import android.annotation.SuppressLint;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.os.Binder;
import android.os.IBinder;

import com.limelight.PcView;
import com.limelight.R;
import com.limelight.binding.PlatformBinding;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.NvHTTP;
import com.limelight.nvstream.http.PairingManager;
import com.limelight.nvstream.http.PairingManager.PairState;

import java.io.FileNotFoundException;
import java.net.UnknownHostException;
import java.security.cert.X509Certificate;

public class PairingService extends Service {
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

        try {
            java.security.cert.X509Certificate serverCert = null;
            if (serverCertBytes != null) {
                java.security.cert.CertificateFactory cf = java.security.cert.CertificateFactory.getInstance("X.509");
                serverCert = (java.security.cert.X509Certificate) cf.generateCertificate(
                        new java.io.ByteArrayInputStream(serverCertBytes));
            }

            ComputerDetails.AddressTuple addressTuple = new ComputerDetails.AddressTuple(computerAddress, httpPort);

            NvHTTP httpConn = new NvHTTP(
                    addressTuple,
                    httpsPort, uniqueId, serverCert,
                    PlatformBinding.getCryptoProvider(this));

            if (httpConn.getPairState() == PairState.PAIRED) {
                success = true;
                pairedCert = httpConn.getPairingManager().getPairedCert();
            } else {
                PairingManager pm = httpConn.getPairingManager();

                // Step 1: Start pm.pair() which sends the pairing request and blocks waiting for PIN
                // We need to submit the PIN via /api/pin while pm.pair() is waiting
                // Schedule PIN submission to run after a short delay (to ensure pm.pair() has started)
                final String finalComputerAddress = computerAddress;
                new Thread(() -> {
                    try {
                        // Wait a bit for pm.pair() to start and send the initial pairing request
                        Thread.sleep(500);
                        LimeLog.info("Submitting PIN to Sunshine API...");
                        boolean pinSubmitted = sendPinToSunshine(finalComputerAddress, username, password, pin, deviceName);
                        if (pinSubmitted) {
                            LimeLog.info("PIN submitted successfully");
                        } else {
                            LimeLog.warning("Failed to submit PIN to Sunshine API");
                        }
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                    }
                }).start();

                // Step 2: This call blocks until server receives PIN and completes pairing
                PairState pairState = pm.pair(httpConn.getServerInfo(true), pin);

                if (pairState == PairState.PIN_WRONG) {
                    message = getString(R.string.pair_incorrect_pin);
                } else if (pairState == PairState.FAILED) {
                    message = getString(R.string.pair_fail);
                } else if (pairState == PairState.ALREADY_IN_PROGRESS) {
                    message = getString(R.string.pair_already_in_progress);
                } else if (pairState == PairState.PAIRED) {
                    success = true;
                    pairedCert = pm.getPairedCert();
                }
            }
        } catch (UnknownHostException e) {
            message = getString(R.string.error_unknown_host);
        } catch (FileNotFoundException e) {
            message = getString(R.string.error_404);
        } catch (Exception e) {
            LimeLog.warning("Sunshine pairing failed: " + e.getMessage());
            message = e.getMessage();
        }

        if (cancelled) {
            stopForeground(STOP_FOREGROUND_REMOVE);
            stopSelf();
            return;
        }

        stopForeground(STOP_FOREGROUND_REMOVE);

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
     * Send PIN to Sunshine server via its REST API
     * @return true if PIN was accepted
     */
    private boolean sendPinToSunshine(String computerAddress, String username, String password,
                                      String pin, String deviceName) {
        javax.net.ssl.HttpsURLConnection connection = null;
        try {
            // Build URL for Sunshine API
            String host = computerAddress;
            if (host.contains(":") && !host.startsWith("[")) {
                host = "[" + host + "]";
            }
            String url = "https://" + host + ":47990/api/pin";

            LimeLog.info("Sending PIN to Sunshine API: " + url);

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
                    public void checkClientTrusted(java.security.cert.X509Certificate[] certs, String authType) {}
                    @SuppressLint("TrustAllX509TrustManager")
                    public void checkServerTrusted(java.security.cert.X509Certificate[] certs, String authType) {}
                }
            };

            // Create SSL context with trust-all manager
            javax.net.ssl.SSLContext sslContext = javax.net.ssl.SSLContext.getInstance("TLS");
            sslContext.init(null, trustAllCerts, new java.security.SecureRandom());
            javax.net.ssl.SSLSocketFactory sslSocketFactory = sslContext.getSocketFactory();
            javax.net.ssl.HostnameVerifier trustAllHostnames = (hostname, session) -> true;

            java.net.URL apiUrl = new java.net.URL(url);
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
            LimeLog.info("Sunshine API response code: " + responseCode);

            // 200 OK means PIN was accepted
            if (responseCode == 200) {
                return true;
            } else if (responseCode == 401) {
                LimeLog.warning("Sunshine API authentication failed (401)");
                return false;
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
                        LimeLog.warning("Sunshine API error response: " + response);
                    }
                }
                return false;
            }
        } catch (javax.net.ssl.SSLHandshakeException e) {
            LimeLog.warning("SSL Handshake failed: " + e.getMessage());
            LimeLog.warning("Stack trace: " + android.util.Log.getStackTraceString(e));
            return false;
        } catch (java.net.SocketTimeoutException e) {
            LimeLog.warning("Connection timeout: " + e.getMessage());
            return false;
        } catch (Exception e) {
            LimeLog.warning("Failed to send PIN to Sunshine: " + e.getMessage());
            LimeLog.warning("Stack trace: " + android.util.Log.getStackTraceString(e));
            return false;
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
    }
}
