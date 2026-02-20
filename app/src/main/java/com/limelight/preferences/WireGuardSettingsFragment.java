package com.limelight.preferences;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.SharedPreferences;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Base64;
import android.util.Log;
import android.widget.Toast;

import androidx.annotation.Nullable;
import androidx.appcompat.app.AlertDialog;
import androidx.preference.EditTextPreference;
import androidx.preference.Preference;
import androidx.preference.PreferenceDataStore;
import androidx.preference.PreferenceFragmentCompat;
import androidx.preference.SwitchPreferenceCompat;

import com.google.android.material.textfield.TextInputEditText;
import com.limelight.R;
import com.limelight.binding.wireguard.WireGuardManager;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Fragment for configuring WireGuard VPN tunnel settings using PreferenceFragment.
 * Uses MMKV for storage via MMKVPreferenceManager.
 */
public class WireGuardSettingsFragment extends PreferenceFragmentCompat {

    private static final String TAG = "WireGuardSettingsFrag";

    // Preference keys
    public static final String PREF_NAME = "wireguard_config";
    public static final String PREF_ENABLED = "wg_enabled";
    public static final String PREF_PRIVATE_KEY = "wg_private_key";
    public static final String PREF_TUNNEL_ADDRESS = "wg_tunnel_address";
    public static final String PREF_PEER_PUBLIC_KEY = "wg_peer_public_key";
    public static final String PREF_PEER_ENDPOINT = "wg_peer_endpoint";
    public static final String PREF_PRESHARED_KEY = "wg_preshared_key";
    public static final String PREF_MTU = "wg_mtu";

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ExecutorService executor = Executors.newSingleThreadExecutor();

    private SwitchPreferenceCompat enabledPref;
    private Preference statusPref;
    private Preference testConnectionPref;
    private EditTextPreference privateKeyPref;
    private Preference generateKeysPref;
    private Preference publicKeyPref;
    private EditTextPreference tunnelAddressPref;
    private EditTextPreference peerPublicKeyPref;
    private EditTextPreference peerEndpointPref;
    private EditTextPreference presharedKeyPref;
    private EditTextPreference mtuPref;
    private Preference importPref;
    private Preference exportPref;
    
    private PreferenceDataStore dataStore;

    @Override
    public void onCreatePreferences(@Nullable Bundle savedInstanceState, @Nullable String rootKey) {
        // Migrate old SharedPreferences data to MMKV (one-time migration)
        migrateFromSharedPreferences();
        
        // Set MMKV as the preference data store before loading preferences
        dataStore = MMKVPreferenceManager.getPreferenceDataStore(requireActivity());
        getPreferenceManager().setPreferenceDataStore(dataStore);
        
        setPreferencesFromResource(R.xml.wireguard_preferences, rootKey);

        initPreferences();
        setupListeners();
        updatePublicKey();
        updateStatusUI();
    }

    /**
     * Migrate old SharedPreferences data to MMKV.
     * This is a one-time migration for users upgrading from the old version.
     */
    private void migrateFromSharedPreferences() {
        SharedPreferences oldPrefs = requireContext().getSharedPreferences(PREF_NAME, Context.MODE_PRIVATE);
        PreferenceDataStore mmkvStore = MMKVPreferenceManager.getPreferenceDataStore(requireActivity());
        
        // Check if migration is needed (old prefs exist and MMKV doesn't have the data)
        String oldPrivateKey = null;
        try {
            oldPrivateKey = oldPrefs.getString(PREF_PRIVATE_KEY, null);
        } catch (Exception ignored) {}
        
        if (oldPrivateKey != null && !oldPrivateKey.isEmpty()) {
            String mmkvPrivateKey = mmkvStore.getString(PREF_PRIVATE_KEY, null);
            if (mmkvPrivateKey == null || mmkvPrivateKey.isEmpty()) {
                Log.i(TAG, "Migrating WireGuard preferences from SharedPreferences to MMKV");
                
                // Migrate all preferences
                try {
                    mmkvStore.putBoolean(PREF_ENABLED, oldPrefs.getBoolean(PREF_ENABLED, false));
                } catch (Exception ignored) {}
                
                mmkvStore.putString(PREF_PRIVATE_KEY, oldPrivateKey);
                
                try {
                    mmkvStore.putString(PREF_TUNNEL_ADDRESS, oldPrefs.getString(PREF_TUNNEL_ADDRESS, "10.0.0.2"));
                } catch (Exception ignored) {}
                
                try {
                    mmkvStore.putString(PREF_PEER_PUBLIC_KEY, oldPrefs.getString(PREF_PEER_PUBLIC_KEY, ""));
                } catch (Exception ignored) {}
                
                try {
                    mmkvStore.putString(PREF_PEER_ENDPOINT, oldPrefs.getString(PREF_PEER_ENDPOINT, ""));
                } catch (Exception ignored) {}
                
                try {
                    mmkvStore.putString(PREF_PRESHARED_KEY, oldPrefs.getString(PREF_PRESHARED_KEY, ""));
                } catch (Exception ignored) {}
                
                // MTU might be int or string in old data
                try {
                    mmkvStore.putString(PREF_MTU, oldPrefs.getString(PREF_MTU, "1420"));
                } catch (ClassCastException e) {
                    try {
                        int mtuInt = oldPrefs.getInt(PREF_MTU, 1420);
                        mmkvStore.putString(PREF_MTU, String.valueOf(mtuInt));
                    } catch (Exception ignored) {}
                } catch (Exception ignored) {}
                
                // Clear old preferences after migration
                oldPrefs.edit().clear().apply();
                Log.i(TAG, "Migration complete");
            }
        }
    }

    private void initPreferences() {
        enabledPref = findPreference(PREF_ENABLED);
        statusPref = findPreference("wg_connection_status");
        testConnectionPref = findPreference("wg_test_connection");
        privateKeyPref = findPreference(PREF_PRIVATE_KEY);
        generateKeysPref = findPreference("wg_generate_keys");
        publicKeyPref = findPreference("wg_public_key");
        tunnelAddressPref = findPreference(PREF_TUNNEL_ADDRESS);
        peerPublicKeyPref = findPreference(PREF_PEER_PUBLIC_KEY);
        peerEndpointPref = findPreference(PREF_PEER_ENDPOINT);
        presharedKeyPref = findPreference(PREF_PRESHARED_KEY);
        mtuPref = findPreference(PREF_MTU);
        importPref = findPreference("wg_import_config");
        exportPref = findPreference("wg_export_config");

        // Set up private key to show masked value
        if (privateKeyPref != null) {
            privateKeyPref.setSummaryProvider(preference -> {
                String value = ((EditTextPreference) preference).getText();
                if (value != null && !value.isEmpty()) {
                    return "••••••••••••••••";
                }
                return getString(R.string.summary_wg_private_key);
            });
        }

        // Set up preshared key to show masked value
        if (presharedKeyPref != null) {
            presharedKeyPref.setSummaryProvider(preference -> {
                String value = ((EditTextPreference) preference).getText();
                if (value != null && !value.isEmpty()) {
                    return "••••••••••••••••";
                }
                return getString(R.string.wireguard_preshared_key_hint);
            });
        }
    }

    private void setupListeners() {
        // Enable switch
        if (enabledPref != null) {
            enabledPref.setOnPreferenceChangeListener((preference, newValue) -> {
                boolean enabled = (Boolean) newValue;
                if (enabled) {
                    if (validateConfig()) {
                        startTunnel();
                        return true;
                    } else {
                        return false;
                    }
                } else {
                    stopTunnel();
                    return true;
                }
            });
        }

        // Test connection
        if (testConnectionPref != null) {
            testConnectionPref.setOnPreferenceClickListener(preference -> {
                testConnection();
                return true;
            });
        }

        // Generate keys
        if (generateKeysPref != null) {
            generateKeysPref.setOnPreferenceClickListener(preference -> {
                new AlertDialog.Builder(requireContext())
                        .setTitle(R.string.wireguard_generate_keys)
                        .setMessage(R.string.wireguard_confirm_generate_keys)
                        .setPositiveButton(android.R.string.ok, (dialog, which) -> generateNewKeys())
                        .setNegativeButton(android.R.string.cancel, null)
                        .show();
                return true;
            });
        }

        // Copy public key
        if (publicKeyPref != null) {
            publicKeyPref.setOnPreferenceClickListener(preference -> {
                String publicKey = publicKeyPref.getSummary() != null ?
                        publicKeyPref.getSummary().toString() : "";
                if (!publicKey.isEmpty() && !publicKey.equals(getString(R.string.wireguard_public_key_tap_to_copy))) {
                    ClipboardManager clipboard = (ClipboardManager)
                            requireContext().getSystemService(Context.CLIPBOARD_SERVICE);
                    ClipData clip = ClipData.newPlainText("WireGuard Public Key", publicKey);
                    clipboard.setPrimaryClip(clip);
                    Toast.makeText(requireContext(), R.string.wireguard_public_key_copied, Toast.LENGTH_SHORT).show();
                }
                return true;
            });
        }

        // Import config
        if (importPref != null) {
            importPref.setOnPreferenceClickListener(preference -> {
                showImportDialog();
                return true;
            });
        }

        // Export config
        if (exportPref != null) {
            exportPref.setOnPreferenceClickListener(preference -> {
                exportConfig();
                return true;
            });
        }
        
        // Listen for private key changes to update public key
        if (privateKeyPref != null) {
            privateKeyPref.setOnPreferenceChangeListener((preference, newValue) -> {
                // Update public key after the value is saved
                mainHandler.post(this::updatePublicKey);
                return true;
            });
        }
    }

    @Override
    public void onResume() {
        super.onResume();
        updateStatusUI();
    }

    private void updatePublicKey() {
        if (privateKeyPref == null || publicKeyPref == null) return;

        String privateKeyB64 = privateKeyPref.getText();
        if (privateKeyB64 == null || privateKeyB64.isEmpty()) {
            publicKeyPref.setSummary(R.string.wireguard_public_key_tap_to_copy);
            return;
        }

        executor.execute(() -> {
            try {
                byte[] privateKey = Base64.decode(privateKeyB64, Base64.DEFAULT);
                if (privateKey.length == 32) {
                    byte[] publicKey = WireGuardManager.derivePublicKey(privateKey);
                    if (publicKey != null) {
                        String publicKeyB64 = Base64.encodeToString(publicKey, Base64.NO_WRAP);
                        mainHandler.post(() -> publicKeyPref.setSummary(publicKeyB64));
                        return;
                    }
                }
            } catch (Exception e) {
                Log.w(TAG, "Failed to derive public key", e);
            }
            mainHandler.post(() -> publicKeyPref.setSummary(R.string.wireguard_public_key_tap_to_copy));
        });
    }

    private void generateNewKeys() {
        executor.execute(() -> {
            byte[][] keyPair = WireGuardManager.generateKeyPair();
            if (keyPair != null) {
                String privateKeyB64 = Base64.encodeToString(keyPair[0], Base64.NO_WRAP);
                String publicKeyB64 = Base64.encodeToString(keyPair[1], Base64.NO_WRAP);

                mainHandler.post(() -> {
                    if (privateKeyPref != null) {
                        privateKeyPref.setText(privateKeyB64);
                    }
                    if (publicKeyPref != null) {
                        publicKeyPref.setSummary(publicKeyB64);
                    }
                    Toast.makeText(requireContext(), R.string.wireguard_keys_generated, Toast.LENGTH_SHORT).show();
                });
            } else {
                mainHandler.post(() -> {
                    Toast.makeText(requireContext(), R.string.wireguard_config_error, Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private boolean validateConfig() {
        return validateConfigInternal(true);
    }

    /**
     * Validate config without showing Toast messages.
     * Used for auto-reconnect on resume where user didn't trigger the action.
     */
    private boolean validateConfigSilent() {
        return validateConfigInternal(false);
    }

    private boolean validateConfigInternal(boolean showToast) {
        // Validate private key
        String privateKey = dataStore.getString(PREF_PRIVATE_KEY, "");
        if (privateKey.isEmpty() || !isValidBase64Key(privateKey)) {
            if (showToast) {
                Toast.makeText(requireContext(), R.string.wireguard_invalid_private_key, Toast.LENGTH_SHORT).show();
            }
            return false;
        }

        // Validate peer public key
        String peerPublicKey = dataStore.getString(PREF_PEER_PUBLIC_KEY, "");
        if (peerPublicKey.isEmpty() || !isValidBase64Key(peerPublicKey)) {
            if (showToast) {
                Toast.makeText(requireContext(), R.string.wireguard_invalid_public_key, Toast.LENGTH_SHORT).show();
            }
            return false;
        }

        // Validate endpoint
        String endpoint = dataStore.getString(PREF_PEER_ENDPOINT, "");
        if (endpoint.isEmpty() || !endpoint.contains(":")) {
            if (showToast) {
                Toast.makeText(requireContext(), R.string.wireguard_invalid_endpoint, Toast.LENGTH_SHORT).show();
            }
            return false;
        }

        // Validate tunnel address
        String tunnelAddress = dataStore.getString(PREF_TUNNEL_ADDRESS, "");
        if (tunnelAddress.isEmpty()) {
            if (showToast) {
                Toast.makeText(requireContext(), R.string.wireguard_invalid_address, Toast.LENGTH_SHORT).show();
            }
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
                    }
                });
            } catch (Exception e) {
                Log.e(TAG, "Failed to start tunnel", e);
                mainHandler.post(() -> {
                    updateStatus(Status.ERROR);
                    Toast.makeText(requireContext(),
                            getString(R.string.wireguard_config_error, e.getMessage()),
                            Toast.LENGTH_SHORT).show();
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

        if (testConnectionPref != null) {
            testConnectionPref.setEnabled(false);
        }
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
                    if (testConnectionPref != null) {
                        testConnectionPref.setEnabled(true);
                    }
                    if (success && isActive) {
                        Toast.makeText(requireContext(), R.string.wireguard_connection_success, Toast.LENGTH_SHORT).show();
                        updateStatus(Status.DISCONNECTED);
                    } else {
                        Toast.makeText(requireContext(),
                                getString(R.string.wireguard_connection_failed, "Handshake failed"),
                                Toast.LENGTH_SHORT).show();
                        updateStatus(Status.ERROR);
                    }
                });
            } catch (Exception e) {
                Log.e(TAG, "Connection test failed", e);
                mainHandler.post(() -> {
                    if (testConnectionPref != null) {
                        testConnectionPref.setEnabled(true);
                    }
                    updateStatus(Status.ERROR);
                    Toast.makeText(requireContext(),
                            getString(R.string.wireguard_connection_failed, e.getMessage()),
                            Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private WireGuardManager.Config buildConfig() {
        String privateKey = dataStore.getString(PREF_PRIVATE_KEY, "");
        String tunnelAddress = dataStore.getString(PREF_TUNNEL_ADDRESS, "10.0.0.2");
        String peerPublicKey = dataStore.getString(PREF_PEER_PUBLIC_KEY, "");
        String peerEndpoint = dataStore.getString(PREF_PEER_ENDPOINT, "");
        String presharedKey = dataStore.getString(PREF_PRESHARED_KEY, "");
        int mtu = 1420;

        try {
            String mtuStr = dataStore.getString(PREF_MTU, "1420");
            mtu = Integer.parseInt(mtuStr != null ? mtuStr : "1420");
        } catch (NumberFormatException ignored) {
        }

        return new WireGuardManager.Config()
                .setPrivateKeyBase64(privateKey)
                .setPeerPublicKeyBase64(peerPublicKey)
                .setPresharedKeyBase64(presharedKey.isEmpty() ? null : presharedKey)
                .setEndpoint(peerEndpoint)
                .setTunnelAddress(tunnelAddress)
                .setMtu(mtu);
    }

    private void showImportDialog() {
        final TextInputEditText input = new TextInputEditText(requireContext());
        input.setHint("Paste WireGuard config here...");
        input.setMinLines(10);

        new AlertDialog.Builder(requireContext())
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

            if (!privateKey.isEmpty()) {
                dataStore.putString(PREF_PRIVATE_KEY, privateKey);
                if (privateKeyPref != null) privateKeyPref.setText(privateKey);
            }
            if (!tunnelAddress.isEmpty()) {
                dataStore.putString(PREF_TUNNEL_ADDRESS, tunnelAddress);
                if (tunnelAddressPref != null) tunnelAddressPref.setText(tunnelAddress);
            }
            if (!peerPublicKey.isEmpty()) {
                dataStore.putString(PREF_PEER_PUBLIC_KEY, peerPublicKey);
                if (peerPublicKeyPref != null) peerPublicKeyPref.setText(peerPublicKey);
            }
            if (!peerEndpoint.isEmpty()) {
                dataStore.putString(PREF_PEER_ENDPOINT, peerEndpoint);
                if (peerEndpointPref != null) peerEndpointPref.setText(peerEndpoint);
            }
            if (!presharedKey.isEmpty()) {
                dataStore.putString(PREF_PRESHARED_KEY, presharedKey);
                if (presharedKeyPref != null) presharedKeyPref.setText(presharedKey);
            }

            updatePublicKey();

            Toast.makeText(requireContext(), R.string.wireguard_import_success, Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(requireContext(),
                    getString(R.string.wireguard_import_failed, e.getMessage()),
                    Toast.LENGTH_SHORT).show();
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
        String privateKey = dataStore.getString(PREF_PRIVATE_KEY, "");
        String tunnelAddress = dataStore.getString(PREF_TUNNEL_ADDRESS, "10.0.0.2");
        String peerPublicKey = dataStore.getString(PREF_PEER_PUBLIC_KEY, "");
        String peerEndpoint = dataStore.getString(PREF_PEER_ENDPOINT, "");
        String presharedKey = dataStore.getString(PREF_PRESHARED_KEY, "");

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
        ClipboardManager clipboard = (ClipboardManager)
                requireContext().getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData clip = ClipData.newPlainText("WireGuard Config", config.toString());
        clipboard.setPrimaryClip(clip);

        Toast.makeText(requireContext(), R.string.wireguard_export_success, Toast.LENGTH_SHORT).show();
    }

    private enum Status {
        DISCONNECTED,
        CONNECTING,
        CONNECTED,
        ERROR
    }

    private void updateStatus(Status status) {
        if (statusPref == null) return;

        switch (status) {
            case DISCONNECTED:
                statusPref.setSummary(R.string.wireguard_status_disconnected);
                break;
            case CONNECTING:
                statusPref.setSummary(R.string.wireguard_status_connecting);
                break;
            case CONNECTED:
                statusPref.setSummary(R.string.wireguard_status_connected);
                break;
            case ERROR:
                statusPref.setSummary(R.string.wireguard_status_error);
                break;
        }
    }

    private void updateStatusUI() {
        if (WireGuardManager.isTunnelActive()) {
            updateStatus(Status.CONNECTED);
        } else {
            // If switch is ON but tunnel is not active (e.g., after app restart),
            // try to auto-reconnect
            boolean switchOn = enabledPref != null && enabledPref.isChecked();
            if (switchOn && validateConfigSilent()) {
                startTunnel();
            } else if (switchOn) {
                // Config is incomplete/invalid, show error but keep switch ON
                updateStatus(Status.ERROR);
            } else {
                updateStatus(Status.DISCONNECTED);
            }
        }
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        executor.shutdown();
    }
}
