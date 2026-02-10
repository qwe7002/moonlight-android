package com.limelight.preferences;

import android.content.Context;
import android.content.SharedPreferences;
import android.preference.PreferenceDataStore;


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
    private static boolean initialized = false;
    private static MMKVPreferenceDataStore dataStoreInstance;

    public static void initialize(Context context) {
        if (!initialized) {
            MMKV.initialize(context);
            initialized = true;
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
    public static class MMKVPreferenceDataStore implements PreferenceDataStore {
        private final MMKV mmkv;

        MMKVPreferenceDataStore(MMKV mmkv) {
            this.mmkv = mmkv;
        }

        @Override
        public void putString(String key,  String value) {
            if (value == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, value);
            }
        }

        @Override
        public String getString(String key,  String defValue) {
            return mmkv.decodeString(key, defValue);
        }

        @Override
        public void putStringSet(String key,  Set<String> values) {
            if (values == null) {
                mmkv.removeValueForKey(key);
            } else {
                mmkv.encode(key, values);
            }
        }

        @Override
        public Set<String> getStringSet(String key, Set<String> defValues) {
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




