package com.limelight.preferences;


import android.content.Context;

import com.tencent.mmkv.MMKV;

public class VulkanPreferences {
    private static final String PREF_NAME = "VulkanPreferences";

    private static final String FINGERPRINT_PREF_STRING = "Fingerprint";
    private static final String GL_RENDERER_PREF_STRING = "Renderer";

    private final MMKV mmkv;
    public String VulkanRenderer;
    public String savedFingerprint;

    private VulkanPreferences(MMKV mmkv) {
        this.mmkv = mmkv;
    }

    public static VulkanPreferences readPreferences(Context context) {
        MMKV.initialize(context);
        MMKV mmkv = MMKV.mmkvWithID(PREF_NAME);
        VulkanPreferences glPrefs = new VulkanPreferences(mmkv);

        glPrefs.VulkanRenderer = mmkv.decodeString(GL_RENDERER_PREF_STRING, "");
        glPrefs.savedFingerprint = mmkv.decodeString(FINGERPRINT_PREF_STRING, "");

        return glPrefs;
    }

    public void writePreferences() {
        mmkv.encode(GL_RENDERER_PREF_STRING, VulkanRenderer);
        mmkv.encode(FINGERPRINT_PREF_STRING, savedFingerprint);
    }
}
