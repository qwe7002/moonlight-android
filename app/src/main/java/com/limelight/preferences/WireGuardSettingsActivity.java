package com.limelight.preferences;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.SharedPreferences;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.TextWatcher;
import android.util.Base64;
import android.util.Log;
import android.view.MenuItem;
import android.view.View;
import android.widget.Button;
import android.widget.TextView;
import android.widget.Toast;

import androidx.appcompat.app.AlertDialog;
import androidx.appcompat.app.AppCompatActivity;

import com.google.android.material.materialswitch.MaterialSwitch;
import com.google.android.material.textfield.TextInputEditText;
import com.google.android.material.textfield.TextInputLayout;
import com.limelight.R;
import com.limelight.binding.video.WireGuardManager;
import com.limelight.utils.UiHelper;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Activity for configuring WireGuard VPN tunnel settings.
 */
public class WireGuardSettingsActivity extends AppCompatActivity {
    private static final String TAG = "WireGuardSettings";

    // Preference keys
    private static final String PREF_NAME = "wireguard_config";
    private static final String PREF_ENABLED = "wg_enabled";
    private static final String PREF_PRIVATE_KEY = "wg_private_key";
    private static final String PREF_TUNNEL_ADDRESS = "wg_tunnel_address";
    private static final String PREF_PEER_PUBLIC_KEY = "wg_peer_public_key";
    private static final String PREF_PEER_ENDPOINT = "wg_peer_endpoint";
    private static final String PREF_PRESHARED_KEY = "wg_preshared_key";
    private static final String PREF_MTU = "wg_mtu";
    private static final String PREF_KEEPALIVE = "wg_keepalive";

    // UI elements
    private MaterialSwitch switchEnabled;
    private View statusIndicator;
    private TextView textStatus;
    private Button btnTestConnection;
    private TextInputEditText editPrivateKey;
    private TextInputEditText editPublicKey;
    private TextInputEditText editTunnelAddress;
    private TextInputEditText editPeerPublicKey;
    private TextInputEditText editPeerEndpoint;
    private TextInputEditText editPresharedKey;
    private TextInputEditText editMtu;
    private TextInputEditText editKeepalive;
    private TextInputLayout layoutPublicKey;
    private Button btnGenerateKeys;
    private Button btnImport;
    private Button btnExport;
    private Button btnSave;

    private SharedPreferences prefs;
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ExecutorService executor = Executors.newSingleThreadExecutor();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        UiHelper.setLocale(this);
        setContentView(R.layout.activity_wireguard_settings);

        // Enable back button in action bar
        if (getSupportActionBar() != null) {
            getSupportActionBar().setDisplayHomeAsUpEnabled(true);
            getSupportActionBar().setTitle(R.string.wireguard_settings_title);
        }

        prefs = getSharedPreferences(PREF_NAME, Context.MODE_PRIVATE);

        initViews();
        loadConfig();
        setupListeners();
        updateStatusUI();

        UiHelper.notifyNewRootView(this);
    }

    @Override
    public boolean onOptionsItemSelected(MenuItem item) {
        if (item.getItemId() == android.R.id.home) {
            finish();
            return true;
        }
        return super.onOptionsItemSelected(item);
    }

    private void initViews() {
        switchEnabled = findViewById(R.id.switch_wireguard_enabled);
        statusIndicator = findViewById(R.id.status_indicator);
        textStatus = findViewById(R.id.text_status);
        btnTestConnection = findViewById(R.id.btn_test_connection);
        editPrivateKey = findViewById(R.id.edit_private_key);
        editPublicKey = findViewById(R.id.edit_public_key);
        editTunnelAddress = findViewById(R.id.edit_tunnel_address);
        editPeerPublicKey = findViewById(R.id.edit_peer_public_key);
        editPeerEndpoint = findViewById(R.id.edit_peer_endpoint);
        editPresharedKey = findViewById(R.id.edit_preshared_key);
        editMtu = findViewById(R.id.edit_mtu);
        editKeepalive = findViewById(R.id.edit_keepalive);
        layoutPublicKey = findViewById(R.id.layout_public_key);
        btnGenerateKeys = findViewById(R.id.btn_generate_keys);
        btnImport = findViewById(R.id.btn_import_config);
        btnExport = findViewById(R.id.btn_export_config);
        btnSave = findViewById(R.id.btn_save);
    }

    private void loadConfig() {
        switchEnabled.setChecked(prefs.getBoolean(PREF_ENABLED, false));
        editPrivateKey.setText(prefs.getString(PREF_PRIVATE_KEY, ""));
        editTunnelAddress.setText(prefs.getString(PREF_TUNNEL_ADDRESS, "10.0.0.2"));
        editPeerPublicKey.setText(prefs.getString(PREF_PEER_PUBLIC_KEY, ""));
        editPeerEndpoint.setText(prefs.getString(PREF_PEER_ENDPOINT, ""));
        editPresharedKey.setText(prefs.getString(PREF_PRESHARED_KEY, ""));
        editMtu.setText(String.valueOf(prefs.getInt(PREF_MTU, 1420)));
        editKeepalive.setText(String.valueOf(prefs.getInt(PREF_KEEPALIVE, 25)));

        // Update public key if private key exists
        updatePublicKey();
    }

    private void setupListeners() {
        // Switch toggle
        switchEnabled.setOnCheckedChangeListener((buttonView, isChecked) -> {
            if (isChecked) {
                // Validate and try to connect
                if (validateConfig()) {
                    startTunnel();
                } else {
                    switchEnabled.setChecked(false);
                }
            } else {
                stopTunnel();
            }
        });

        // Private key changed - update public key
        editPrivateKey.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {}

            @Override
            public void afterTextChanged(Editable s) {
                updatePublicKey();
            }
        });

        // Generate keys button
        btnGenerateKeys.setOnClickListener(v -> {
            new AlertDialog.Builder(this)
                .setTitle(R.string.wireguard_generate_keys)
                .setMessage(R.string.wireguard_confirm_generate_keys)
                .setPositiveButton(android.R.string.ok, (dialog, which) -> generateNewKeys())
                .setNegativeButton(android.R.string.cancel, null)
                .show();
        });

        // Copy public key button (end icon click)
        layoutPublicKey.setEndIconOnClickListener(v -> {
            String publicKey = editPublicKey.getText() != null ? editPublicKey.getText().toString() : "";
            if (!publicKey.isEmpty()) {
                ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
                ClipData clip = ClipData.newPlainText("WireGuard Public Key", publicKey);
                clipboard.setPrimaryClip(clip);
                Toast.makeText(this, R.string.wireguard_public_key_copied, Toast.LENGTH_SHORT).show();
            }
        });

        // Test connection button
        btnTestConnection.setOnClickListener(v -> testConnection());

        // Import button
        btnImport.setOnClickListener(v -> showImportDialog());

        // Export button
        btnExport.setOnClickListener(v -> exportConfig());

        // Save button
        btnSave.setOnClickListener(v -> saveConfig());
    }

    private void updatePublicKey() {
        String privateKeyB64 = editPrivateKey.getText() != null ? editPrivateKey.getText().toString().trim() : "";
        if (privateKeyB64.isEmpty()) {
            editPublicKey.setText("");
            return;
        }

        executor.execute(() -> {
            try {
                byte[] privateKey = Base64.decode(privateKeyB64, Base64.DEFAULT);
                if (privateKey.length == 32) {
                    byte[] publicKey = WireGuardManager.derivePublicKey(privateKey);
                    if (publicKey != null) {
                        String publicKeyB64 = Base64.encodeToString(publicKey, Base64.NO_WRAP);
                        mainHandler.post(() -> editPublicKey.setText(publicKeyB64));
                        return;
                    }
                }
            } catch (Exception e) {
                Log.w(TAG, "Failed to derive public key", e);
            }
            mainHandler.post(() -> editPublicKey.setText(""));
        });
    }

    private void generateNewKeys() {
        executor.execute(() -> {
            byte[][] keyPair = WireGuardManager.generateKeyPair();
            if (keyPair != null) {
                String privateKeyB64 = Base64.encodeToString(keyPair[0], Base64.NO_WRAP);
                String publicKeyB64 = Base64.encodeToString(keyPair[1], Base64.NO_WRAP);

                mainHandler.post(() -> {
                    editPrivateKey.setText(privateKeyB64);
                    editPublicKey.setText(publicKeyB64);
                    Toast.makeText(this, R.string.wireguard_keys_generated, Toast.LENGTH_SHORT).show();
                });
            } else {
                mainHandler.post(() -> {
                    Toast.makeText(this, R.string.wireguard_config_error, Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private boolean validateConfig() {
        // Validate private key
        String privateKey = editPrivateKey.getText() != null ? editPrivateKey.getText().toString().trim() : "";
        if (privateKey.isEmpty() || !isValidBase64Key(privateKey)) {
            Toast.makeText(this, R.string.wireguard_invalid_private_key, Toast.LENGTH_SHORT).show();
            return false;
        }

        // Validate peer public key
        String peerPublicKey = editPeerPublicKey.getText() != null ? editPeerPublicKey.getText().toString().trim() : "";
        if (peerPublicKey.isEmpty() || !isValidBase64Key(peerPublicKey)) {
            Toast.makeText(this, R.string.wireguard_invalid_public_key, Toast.LENGTH_SHORT).show();
            return false;
        }

        // Validate endpoint
        String endpoint = editPeerEndpoint.getText() != null ? editPeerEndpoint.getText().toString().trim() : "";
        if (endpoint.isEmpty() || !endpoint.contains(":")) {
            Toast.makeText(this, R.string.wireguard_invalid_endpoint, Toast.LENGTH_SHORT).show();
            return false;
        }

        // Validate tunnel address
        String tunnelAddress = editTunnelAddress.getText() != null ? editTunnelAddress.getText().toString().trim() : "";
        if (tunnelAddress.isEmpty()) {
            Toast.makeText(this, R.string.wireguard_invalid_address, Toast.LENGTH_SHORT).show();
            return false;
        }

        return true;
    }

    private boolean isValidBase64Key(String keyB64) {
        try {
            byte[] key = Base64.decode(keyB64, Base64.DEFAULT);
            return key.length == 32;
        } catch (Exception e) {
            return false;
        }
    }

    private void saveConfig() {
        String privateKey = editPrivateKey.getText() != null ? editPrivateKey.getText().toString().trim() : "";
        String tunnelAddress = editTunnelAddress.getText() != null ? editTunnelAddress.getText().toString().trim() : "";
        String peerPublicKey = editPeerPublicKey.getText() != null ? editPeerPublicKey.getText().toString().trim() : "";
        String peerEndpoint = editPeerEndpoint.getText() != null ? editPeerEndpoint.getText().toString().trim() : "";
        String presharedKey = editPresharedKey.getText() != null ? editPresharedKey.getText().toString().trim() : "";
        int mtu = 1420;
        int keepalive = 25;

        try {
            mtu = Integer.parseInt(editMtu.getText() != null ? editMtu.getText().toString() : "1420");
        } catch (NumberFormatException e) {
            // Use default
        }

        try {
            keepalive = Integer.parseInt(editKeepalive.getText() != null ? editKeepalive.getText().toString() : "25");
        } catch (NumberFormatException e) {
            // Use default
        }

        prefs.edit()
            .putBoolean(PREF_ENABLED, switchEnabled.isChecked())
            .putString(PREF_PRIVATE_KEY, privateKey)
            .putString(PREF_TUNNEL_ADDRESS, tunnelAddress)
            .putString(PREF_PEER_PUBLIC_KEY, peerPublicKey)
            .putString(PREF_PEER_ENDPOINT, peerEndpoint)
            .putString(PREF_PRESHARED_KEY, presharedKey)
            .putInt(PREF_MTU, mtu)
            .putInt(PREF_KEEPALIVE, keepalive)
            .apply();

        Toast.makeText(this, R.string.wireguard_config_saved, Toast.LENGTH_SHORT).show();
    }

    private void startTunnel() {
        updateStatus(Status.CONNECTING);

        executor.execute(() -> {
            try {
                WireGuardManager.Config config = buildConfig();
                boolean success = WireGuardManager.startTunnel(config);

                mainHandler.post(() -> {
                    if (success) {
                        updateStatus(Status.CONNECTED);
                    } else {
                        updateStatus(Status.ERROR);
                        switchEnabled.setChecked(false);
                    }
                });
            } catch (Exception e) {
                Log.e(TAG, "Failed to start tunnel", e);
                mainHandler.post(() -> {
                    updateStatus(Status.ERROR);
                    switchEnabled.setChecked(false);
                    Toast.makeText(this, getString(R.string.wireguard_config_error, e.getMessage()), Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private void stopTunnel() {
        executor.execute(() -> {
            WireGuardManager.stopTunnel();
            mainHandler.post(() -> updateStatus(Status.DISCONNECTED));
        });
    }

    private void testConnection() {
        if (!validateConfig()) {
            return;
        }

        btnTestConnection.setEnabled(false);
        updateStatus(Status.CONNECTING);

        executor.execute(() -> {
            try {
                WireGuardManager.Config config = buildConfig();
                boolean success = WireGuardManager.startTunnel(config);

                // Wait a bit for handshake
                Thread.sleep(3000);

                boolean isActive = WireGuardManager.isTunnelActive();
                WireGuardManager.stopTunnel();

                mainHandler.post(() -> {
                    btnTestConnection.setEnabled(true);
                    if (success && isActive) {
                        Toast.makeText(this, R.string.wireguard_connection_success, Toast.LENGTH_SHORT).show();
                        updateStatus(Status.DISCONNECTED);
                    } else {
                        Toast.makeText(this, getString(R.string.wireguard_connection_failed, "Handshake failed"), Toast.LENGTH_SHORT).show();
                        updateStatus(Status.ERROR);
                    }
                });
            } catch (Exception e) {
                Log.e(TAG, "Connection test failed", e);
                mainHandler.post(() -> {
                    btnTestConnection.setEnabled(true);
                    updateStatus(Status.ERROR);
                    Toast.makeText(this, getString(R.string.wireguard_connection_failed, e.getMessage()), Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private WireGuardManager.Config buildConfig() {
        String privateKey = editPrivateKey.getText() != null ? editPrivateKey.getText().toString().trim() : "";
        String tunnelAddress = editTunnelAddress.getText() != null ? editTunnelAddress.getText().toString().trim() : "";
        String peerPublicKey = editPeerPublicKey.getText() != null ? editPeerPublicKey.getText().toString().trim() : "";
        String peerEndpoint = editPeerEndpoint.getText() != null ? editPeerEndpoint.getText().toString().trim() : "";
        String presharedKey = editPresharedKey.getText() != null ? editPresharedKey.getText().toString().trim() : "";

        int mtu = 1420;
        int keepalive = 25;

        try {
            mtu = Integer.parseInt(editMtu.getText() != null ? editMtu.getText().toString() : "1420");
        } catch (NumberFormatException ignored) {}

        try {
            keepalive = Integer.parseInt(editKeepalive.getText() != null ? editKeepalive.getText().toString() : "25");
        } catch (NumberFormatException ignored) {}

        return new WireGuardManager.Config()
            .setPrivateKeyBase64(privateKey)
            .setPeerPublicKeyBase64(peerPublicKey)
            .setPresharedKeyBase64(presharedKey.isEmpty() ? null : presharedKey)
            .setEndpoint(peerEndpoint)
            .setTunnelAddress(tunnelAddress)
            .setMtu(mtu)
            .setKeepaliveSecs(keepalive);
    }

    private void showImportDialog() {
        // Simple text input dialog for pasting WireGuard config
        final TextInputEditText input = new TextInputEditText(this);
        input.setHint("Paste WireGuard config here...");
        input.setMinLines(10);

        new AlertDialog.Builder(this)
            .setTitle(R.string.wireguard_import_title)
            .setView(input)
            .setPositiveButton(R.string.wireguard_import, (dialog, which) -> {
                String configText = input.getText() != null ? input.getText().toString() : "";
                importConfig(configText);
            })
            .setNegativeButton(android.R.string.cancel, null)
            .show();
    }

    private void importConfig(String configText) {
        try {
            // Parse WireGuard config format
            String privateKey = "";
            String tunnelAddress = "";
            String peerPublicKey = "";
            String peerEndpoint = "";
            String presharedKey = "";

            for (String line : configText.split("\n")) {
                line = line.trim();
                if (line.startsWith("PrivateKey")) {
                    privateKey = extractValue(line);
                } else if (line.startsWith("Address")) {
                    tunnelAddress = extractValue(line).split("/")[0]; // Remove CIDR
                } else if (line.startsWith("PublicKey")) {
                    peerPublicKey = extractValue(line);
                } else if (line.startsWith("Endpoint")) {
                    peerEndpoint = extractValue(line);
                } else if (line.startsWith("PresharedKey")) {
                    presharedKey = extractValue(line);
                }
            }

            if (!privateKey.isEmpty()) editPrivateKey.setText(privateKey);
            if (!tunnelAddress.isEmpty()) editTunnelAddress.setText(tunnelAddress);
            if (!peerPublicKey.isEmpty()) editPeerPublicKey.setText(peerPublicKey);
            if (!peerEndpoint.isEmpty()) editPeerEndpoint.setText(peerEndpoint);
            if (!presharedKey.isEmpty()) editPresharedKey.setText(presharedKey);

            Toast.makeText(this, R.string.wireguard_import_success, Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(this, getString(R.string.wireguard_import_failed, e.getMessage()), Toast.LENGTH_SHORT).show();
        }
    }

    private String extractValue(String line) {
        int idx = line.indexOf('=');
        if (idx >= 0 && idx < line.length() - 1) {
            return line.substring(idx + 1).trim();
        }
        return "";
    }

    private void exportConfig() {
        String privateKey = editPrivateKey.getText() != null ? editPrivateKey.getText().toString().trim() : "";
        String publicKey = editPublicKey.getText() != null ? editPublicKey.getText().toString().trim() : "";
        String tunnelAddress = editTunnelAddress.getText() != null ? editTunnelAddress.getText().toString().trim() : "";
        String peerPublicKey = editPeerPublicKey.getText() != null ? editPeerPublicKey.getText().toString().trim() : "";
        String peerEndpoint = editPeerEndpoint.getText() != null ? editPeerEndpoint.getText().toString().trim() : "";
        String presharedKey = editPresharedKey.getText() != null ? editPresharedKey.getText().toString().trim() : "";

        StringBuilder config = new StringBuilder();
        config.append("[Interface]\n");
        config.append("PrivateKey = ").append(privateKey).append("\n");
        config.append("Address = ").append(tunnelAddress).append("/32\n");
        config.append("\n[Peer]\n");
        config.append("PublicKey = ").append(peerPublicKey).append("\n");
        if (!presharedKey.isEmpty()) {
            config.append("PresharedKey = ").append(presharedKey).append("\n");
        }
        config.append("Endpoint = ").append(peerEndpoint).append("\n");
        config.append("AllowedIPs = 0.0.0.0/0\n");

        // Copy to clipboard
        ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData clip = ClipData.newPlainText("WireGuard Config", config.toString());
        clipboard.setPrimaryClip(clip);

        Toast.makeText(this, R.string.wireguard_export_success, Toast.LENGTH_SHORT).show();
    }

    private enum Status {
        DISCONNECTED,
        CONNECTING,
        CONNECTED,
        ERROR
    }

    private void updateStatus(Status status) {
        switch (status) {
            case DISCONNECTED:
                statusIndicator.setBackgroundResource(R.drawable.status_indicator_disconnected);
                textStatus.setText(R.string.wireguard_status_disconnected);
                break;
            case CONNECTING:
                statusIndicator.setBackgroundResource(R.drawable.status_indicator_connecting);
                textStatus.setText(R.string.wireguard_status_connecting);
                break;
            case CONNECTED:
                statusIndicator.setBackgroundResource(R.drawable.status_indicator_connected);
                textStatus.setText(R.string.wireguard_status_connected);
                break;
            case ERROR:
                statusIndicator.setBackgroundResource(R.drawable.status_indicator_disconnected);
                textStatus.setText(R.string.wireguard_status_error);
                break;
        }
    }

    private void updateStatusUI() {
        if (WireGuardManager.isTunnelActive()) {
            updateStatus(Status.CONNECTED);
        } else {
            updateStatus(Status.DISCONNECTED);
        }
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        executor.shutdown();
    }

    /**
     * Static method to load WireGuard config from preferences
     */
    public static WireGuardManager.Config loadConfig(Context context) {
        SharedPreferences prefs = context.getSharedPreferences(PREF_NAME, Context.MODE_PRIVATE);

        if (!prefs.getBoolean(PREF_ENABLED, false)) {
            return null;
        }

        String privateKey = prefs.getString(PREF_PRIVATE_KEY, "");
        String peerPublicKey = prefs.getString(PREF_PEER_PUBLIC_KEY, "");
        String peerEndpoint = prefs.getString(PREF_PEER_ENDPOINT, "");

        if (privateKey.isEmpty() || peerPublicKey.isEmpty() || peerEndpoint.isEmpty()) {
            return null;
        }

        return new WireGuardManager.Config()
            .setPrivateKeyBase64(privateKey)
            .setPeerPublicKeyBase64(peerPublicKey)
            .setPresharedKeyBase64(prefs.getString(PREF_PRESHARED_KEY, ""))
            .setEndpoint(peerEndpoint)
            .setTunnelAddress(prefs.getString(PREF_TUNNEL_ADDRESS, "10.0.0.2"))
            .setMtu(prefs.getInt(PREF_MTU, 1420))
            .setKeepaliveSecs(prefs.getInt(PREF_KEEPALIVE, 25));
    }

    /**
     * Check if WireGuard is enabled in preferences
     */
    public static boolean isEnabled(Context context) {
        SharedPreferences prefs = context.getSharedPreferences(PREF_NAME, Context.MODE_PRIVATE);
        return prefs.getBoolean(PREF_ENABLED, false);
    }
}

