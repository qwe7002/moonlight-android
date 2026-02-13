package com.limelight.preferences;

import android.content.Context;
import android.content.SharedPreferences;

import androidx.annotation.Nullable;
import androidx.preference.PreferenceDataStore;

import com.tencent.mmkv.MMKV;

import java.util.HashMap;
import java.util.Map;
import java.util.Set;

/**
 * A wrapper class that provides SharedPreferences-compatible interface using MMKV.
 * This allows seamless migration from Android's SharedPreferences to MMKV.
 */
public class MMKVPreferenceManager {
    private static final String DEFAULT_MMKV_ID = "default_preferences";
    private static final String DEFAULTS_INITIALIZED_KEY = "_mmkv_defaults_initialized";
    private static final String DEVICE_DEFAULTS_INITIALIZED_KEY = "_device_defaults_initialized";
    private static boolean initialized = false;
    private static MMKVPreferenceDataStore dataStoreInstance;

    public static void initialize(Context context) {
        if (!initialized) {
            MMKV.initialize(context);
            initialized = true;

            // Initialize default values on first run
            initializeDefaultValues();

            // Initialize device-specific defaults (resolution, FPS) based on display
            initializeDeviceDefaults(context);
        }
    }

    /**
     * Initialize device-specific default values based on display capabilities.
     * This detects the device's display resolution and refresh rate to set optimal defaults.
     */
    private static void initializeDeviceDefaults(Context context) {
        MMKV mmkv = MMKV.mmkvWithID(DEFAULT_MMKV_ID);

        // Only initialize once
        if (mmkv.decodeBool(DEVICE_DEFAULTS_INITIALIZED_KEY, false)) {
            return;
        }

        // Use PreferenceConfiguration to detect and set device-specific defaults
        PreferenceConfiguration.initializeDefaultsForDevice(context);

        // Mark device defaults as initialized
        mmkv.encode(DEVICE_DEFAULTS_INITIALIZED_KEY, true);
    }

    /**
     * Initialize all preference default values in MMKV.
     * This ensures that all preferences have proper default values on first run.
     */
    private static void initializeDefaultValues() {
        MMKV mmkv = MMKV.mmkvWithID(DEFAULT_MMKV_ID);

        // Check if defaults have already been initialized
        if (mmkv.decodeBool(DEFAULTS_INITIALIZED_KEY, false)) {
            return;
        }

        // Resolution and FPS
        initializeIfAbsent(mmkv, "list_resolution", "1920x1080");
        initializeIfAbsent(mmkv, "list_fps", "60");

        // Frame pacing
        initializeIfAbsent(mmkv, "frame_pacing", "latency");

        // Video settings
        initializeIfAbsent(mmkv, "checkbox_stretch_video", false);
        initializeIfAbsent(mmkv, "video_format", "auto");
        initializeIfAbsent(mmkv, "checkbox_enable_hdr", false);
        initializeIfAbsent(mmkv, "checkbox_full_range", false);

        // Audio settings
        initializeIfAbsent(mmkv, "list_audio_config", "2");
        initializeIfAbsent(mmkv, "checkbox_enable_audiofx", false);

        // Gamepad settings
        initializeIfAbsent(mmkv, "seekbar_deadzone", 1);
        initializeIfAbsent(mmkv, "checkbox_multi_controller", true);
        initializeIfAbsent(mmkv, "checkbox_usb_driver", true);
        initializeIfAbsent(mmkv, "checkbox_usb_bind_all", true);
        initializeIfAbsent(mmkv, "checkbox_mouse_emulation", true);
        initializeIfAbsent(mmkv, "analog_scrolling", "right");
        initializeIfAbsent(mmkv, "checkbox_vibrate_fallback", true);
        initializeIfAbsent(mmkv, "seekbar_vibrate_fallback_strength", 100);
        initializeIfAbsent(mmkv, "checkbox_flip_face_buttons", false);
        initializeIfAbsent(mmkv, "checkbox_gamepad_touchpad_as_mouse", false);
        initializeIfAbsent(mmkv, "checkbox_gamepad_motion_sensors", true);
        initializeIfAbsent(mmkv, "checkbox_gamepad_motion_fallback", false);

        // Input settings
        initializeIfAbsent(mmkv, "checkbox_touchscreen_trackpad", true);
        initializeIfAbsent(mmkv, "checkbox_mouse_nav_buttons", false);
        initializeIfAbsent(mmkv, "checkbox_absolute_mouse_mode", false);

        // On-screen controls
        initializeIfAbsent(mmkv, "checkbox_show_onscreen_controls", false);
        initializeIfAbsent(mmkv, "checkbox_vibrate_osc", true);
        initializeIfAbsent(mmkv, "checkbox_only_show_L3R3", false);
        initializeIfAbsent(mmkv, "checkbox_show_guide_button", true);
        initializeIfAbsent(mmkv, "seekbar_osc_opacity", 90);

        // Host settings
        initializeIfAbsent(mmkv, "checkbox_enable_sops", true);
        initializeIfAbsent(mmkv, "checkbox_host_audio", false);

        // UI settings
        initializeIfAbsent(mmkv, "checkbox_enable_pip", false);

        // Advanced settings
        initializeIfAbsent(mmkv, "checkbox_unlock_fps", false);
        initializeIfAbsent(mmkv, "checkbox_reduce_refresh_rate", false);
        initializeIfAbsent(mmkv, "checkbox_disable_warnings", false);
        initializeIfAbsent(mmkv, "checkbox_enable_perf_overlay", false);
        initializeIfAbsent(mmkv, "checkbox_enable_stats_notification", true);
        initializeIfAbsent(mmkv, "checkbox_enable_post_stream_toast", false);
        initializeIfAbsent(mmkv, "checkbox_enable_mdns", false);

        // Mark defaults as initialized
        mmkv.encode(DEFAULTS_INITIALIZED_KEY, true);
    }

    private static void initializeIfAbsent(MMKV mmkv, String key, String value) {
        if (!mmkv.containsKey(key)) {
            mmkv.encode(key, value);
        }
    }

    private static void initializeIfAbsent(MMKV mmkv, String key, boolean value) {
        if (!mmkv.containsKey(key)) {
            mmkv.encode(key, value);
        }
    }

    private static void initializeIfAbsent(MMKV mmkv, String key, int value) {
        if (!mmkv.containsKey(key)) {
            mmkv.encode(key, value);
        }
    }

    /**
     * Get a SharedPreferences-compatible implementation backed by MMKV.
     * This method can be used as a drop-in replacement for PreferenceManager.getDefaultSharedPreferences().
     *
     * @param context The context to use for initialization
     * @return A SharedPreferences implementation backed by MMKV
     */
    public static SharedPreferences getDefaultSharedPreferences(Context context) {
        initialize(context);
        return new MMKVSharedPreferences(MMKV.mmkvWithID(DEFAULT_MMKV_ID));
    }

    /**
     * Get a PreferenceDataStore implementation backed by MMKV.
     * This can be used with PreferenceFragment/PreferenceFragmentCompat to store preferences in MMKV.
     *
     * @param context The context to use for initialization
     * @return A PreferenceDataStore implementation backed by MMKV
     */
    public static PreferenceDataStore getPreferenceDataStore(Context context) {
        initialize(context);
        if (dataStoreInstance == null) {
            dataStoreInstance = new MMKVPreferenceDataStore(MMKV.mmkvWithID(DEFAULT_MMKV_ID));
        }
        return dataStoreInstance;
    }

    /**
     * PreferenceDataStore implementation that wraps MMKV.
     * This allows PreferenceFragment to store preferences directly in MMKV.
     */
    public static class MMKVPreferenceDataStore extends PreferenceDataStore {
        private final MMKV mmkv;

        MMKVPreferenceDataStore(MMKV mmkv) {
            this.mmkv = mmkv;
        }

        @Override
        public void putString(String key, @Nullable String value) {
            if (value == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, value);
            }
        }

        @Override
        @Nullable
        public String getString(String key, @Nullable String defValue) {
            return mmkv.decodeString(key, defValue);
        }

        @Override
        public void putStringSet(String key, @Nullable Set<String> values) {
            if (values == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, values);
            }
        }

        @Override
        @Nullable
        public Set<String> getStringSet(String key, @Nullable Set<String> defValues) {
            return mmkv.decodeStringSet(key, defValues);
        }

        @Override
        public void putInt(String key, int value) {
            mmkv.encode(key, value);
        }

        @Override
        public int getInt(String key, int defValue) {
            return mmkv.decodeInt(key, defValue);
        }

        @Override
        public void putLong(String key, long value) {
            mmkv.encode(key, value);
        }

        @Override
        public long getLong(String key, long defValue) {
            return mmkv.decodeLong(key, defValue);
        }

        @Override
        public void putFloat(String key, float value) {
            mmkv.encode(key, value);
        }

        @Override
        public float getFloat(String key, float defValue) {
            return mmkv.decodeFloat(key, defValue);
        }

        @Override
        public void putBoolean(String key, boolean value) {
            mmkv.encode(key, value);
        }

        @Override
        public boolean getBoolean(String key, boolean defValue) {
            return mmkv.decodeBool(key, defValue);
        }
    }

    /**
     * SharedPreferences implementation that wraps MMKV.
     */
    private static class MMKVSharedPreferences implements SharedPreferences {
        private final MMKV mmkv;

        MMKVSharedPreferences(MMKV mmkv) {
            this.mmkv = mmkv;
        }

        @Override
        public Map<String, ?> getAll() {
            String[] keys = mmkv.allKeys();
            Map<String, Object> result = new HashMap<>();
            if (keys != null) {
                for (String key : keys) {
                    // MMKV doesn't have a type-safe getAll, so we return all as strings
                    // This is a limitation but works for most use cases
                    result.put(key, mmkv.decodeString(key));
                }
            }
            return result;
        }

        @Override
        public String getString(String key, String defValue) {
            return mmkv.decodeString(key, defValue);
        }

        @Override
        public Set<String> getStringSet(String key, Set<String> defValues) {
            return mmkv.decodeStringSet(key, defValues);
        }

        @Override
        public int getInt(String key, int defValue) {
            return mmkv.decodeInt(key, defValue);
        }

        @Override
        public long getLong(String key, long defValue) {
            return mmkv.decodeLong(key, defValue);
        }

        @Override
        public float getFloat(String key, float defValue) {
            return mmkv.decodeFloat(key, defValue);
        }

        @Override
        public boolean getBoolean(String key, boolean defValue) {
            return mmkv.decodeBool(key, defValue);
        }

        @Override
        public boolean contains(String key) {
            return mmkv.containsKey(key);
        }

        @Override
        public Editor edit() {
            return new MMKVEditor(mmkv);
        }

        @Override
        public void registerOnSharedPreferenceChangeListener(OnSharedPreferenceChangeListener listener) {
            // MMKV doesn't support change listeners in the same way as SharedPreferences
            // This is a no-op but won't cause issues for most use cases
        }

        @Override
        public void unregisterOnSharedPreferenceChangeListener(OnSharedPreferenceChangeListener listener) {
            // MMKV doesn't support change listeners in the same way as SharedPreferences
            // This is a no-op
        }
    }

    /**
     * SharedPreferences.Editor implementation that wraps MMKV.
     */
    private static class MMKVEditor implements SharedPreferences.Editor {
        private final MMKV mmkv;

        MMKVEditor(MMKV mmkv) {
            this.mmkv = mmkv;
        }

        @Override
        public SharedPreferences.Editor putString(String key, String value) {
            if (value == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, value);
            }
            return this;
        }

        @Override
        public SharedPreferences.Editor putStringSet(String key, Set<String> values) {
            if (values == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, values);
            }
            return this;
        }

        @Override
        public SharedPreferences.Editor putInt(String key, int value) {
            mmkv.encode(key, value);
            return this;
        }

        @Override
        public SharedPreferences.Editor putLong(String key, long value) {
            mmkv.encode(key, value);
            return this;
        }

        @Override
        public SharedPreferences.Editor putFloat(String key, float value) {
            mmkv.encode(key, value);
            return this;
        }

        @Override
        public SharedPreferences.Editor putBoolean(String key, boolean value) {
            mmkv.encode(key, value);
            return this;
        }

        @Override
        public SharedPreferences.Editor remove(String key) {
            mmkv.removeValueForKey(key);
            return this;
        }

        @Override
        public SharedPreferences.Editor clear() {
            mmkv.clearAll();
            return this;
        }

        @Override
        public boolean commit() {
            // MMKV writes synchronously by default
            return true;
        }

        @Override
        public void apply() {
            // MMKV writes immediately, no need to apply
        }
    }
}




