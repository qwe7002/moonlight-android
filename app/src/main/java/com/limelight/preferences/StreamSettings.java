package com.limelight.preferences;

import android.app.Activity;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.media.MediaCodecInfo;
import android.os.Bundle;
import android.os.Handler;
import android.os.Vibrator;
import android.util.DisplayMetrics;
import android.util.Range;
import android.view.Display;
import android.view.DisplayCutout;
import android.view.View;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.preference.CheckBoxPreference;
import androidx.preference.ListPreference;
import androidx.preference.Preference;
import androidx.preference.PreferenceCategory;
import androidx.preference.PreferenceFragmentCompat;
import androidx.preference.PreferenceScreen;

import com.limelight.R;
import com.limelight.binding.video.MediaCodecHelper;
import com.limelight.utils.Dialog;
import com.limelight.utils.UiHelper;

import java.util.Arrays;
import java.util.Objects;

public class StreamSettings extends AppCompatActivity {
    private int previousDisplayPixelCount;

    private static final int REQUEST_NOTIFICATION_PERMISSION = 1001;

    @SuppressWarnings("NullableProblems")
    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQUEST_NOTIFICATION_PERMISSION) {
            if (grantResults.length > 0 && grantResults[0] != PackageManager.PERMISSION_GRANTED) {
                // Permission denied, uncheck the checkbox
                SharedPreferences prefs = MMKVPreferenceManager.getDefaultSharedPreferences(this);
                prefs.edit().putBoolean("checkbox_enable_stats_notification", false).apply();
                // Reload settings to reflect the change
                reloadSettings();
            }
        }
    }

    // HACK for Android 9
    static DisplayCutout displayCutoutP;

    void reloadSettings() {
        Display.Mode mode = getDisplay().getMode();
        previousDisplayPixelCount = mode.getPhysicalWidth() * mode.getPhysicalHeight();
        getSupportFragmentManager().beginTransaction().replace(
                R.id.stream_settings, new SettingsFragment()
        ).commitAllowingStateLoss();
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        UiHelper.setLocale(this);

        setContentView(R.layout.activity_stream_settings);

        UiHelper.notifyNewRootView(this);
    }

    @Override
    public void onAttachedToWindow() {
        super.onAttachedToWindow();
        reloadSettings();
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);

        Display.Mode mode = getDisplay().getMode();

        // If the display's physical pixel count has changed, we consider that it's a new display
        // and we should reload our settings (which include display-dependent values).
        //
        // NB: We aren't using displayId here because that stays the same (DEFAULT_DISPLAY) when
        // switching between screens on a foldable device.
        if (mode.getPhysicalWidth() * mode.getPhysicalHeight() != previousDisplayPixelCount) {
            reloadSettings();
        }
    }

/*    @Override
    // NOTE: This will NOT be called on Android 13+ with android:enableOnBackInvokedCallback="true"
    public void onBackPressed() {
        super.onBackPressed();
        finish();

        // Language changes are handled via configuration changes in Android 13+,
        // so manual activity relaunching is no longer required.
    }*/

    public static class SettingsFragment extends PreferenceFragmentCompat {
        private int nativeResolutionStartIndex = Integer.MAX_VALUE;
        private boolean nativeFramerateShown = false;

        private void setValue(String preferenceKey, String value) {
            ListPreference pref = findPreference(preferenceKey);
            if (pref != null) {
                pref.setValue(value);
            }
        }

        private void appendPreferenceEntry(ListPreference pref, String newEntryName, String newEntryValue) {
            CharSequence[] newEntries = Arrays.copyOf(pref.getEntries(), pref.getEntries().length + 1);
            CharSequence[] newValues = Arrays.copyOf(pref.getEntryValues(), pref.getEntryValues().length + 1);

            // Add the new option
            newEntries[newEntries.length - 1] = newEntryName;
            newValues[newValues.length - 1] = newEntryValue;

            pref.setEntries(newEntries);
            pref.setEntryValues(newValues);
        }

        private void addNativeResolutionEntry(int nativeWidth, int nativeHeight, boolean insetsRemoved, boolean portrait) {
            ListPreference pref = findPreference(PreferenceConfiguration.RESOLUTION_PREF_STRING);
            if (pref == null) return;

            String newName;

            if (insetsRemoved) {
                newName = getResources().getString(R.string.resolution_prefix_native_fullscreen);
            } else {
                newName = getResources().getString(R.string.resolution_prefix_native);
            }

            if (PreferenceConfiguration.isSquarishScreen(nativeWidth, nativeHeight)) {
                if (portrait) {
                    newName += " " + getResources().getString(R.string.resolution_prefix_native_portrait);
                } else {
                    newName += " " + getResources().getString(R.string.resolution_prefix_native_landscape);
                }
            }

            newName += " (" + nativeWidth + "x" + nativeHeight + ")";

            String newValue = nativeWidth + "x" + nativeHeight;

            // Check if the native resolution is already present
            for (CharSequence value : pref.getEntryValues()) {
                if (newValue.equals(value.toString())) {
                    // It is present in the default list, so don't add it again
                    return;
                }
            }

            if (pref.getEntryValues().length < nativeResolutionStartIndex) {
                nativeResolutionStartIndex = pref.getEntryValues().length;
            }
            appendPreferenceEntry(pref, newName, newValue);
        }

        @SuppressWarnings("SuspiciousNameCombination")
        private void addNativeResolutionEntries(int nativeWidth, int nativeHeight, boolean insetsRemoved) {
            if (PreferenceConfiguration.isSquarishScreen(nativeWidth, nativeHeight)) {
                addNativeResolutionEntry(nativeHeight, nativeWidth, insetsRemoved, true);
            }
            addNativeResolutionEntry(nativeWidth, nativeHeight, insetsRemoved, false);
        }

        private void addNativeFrameRateEntry(float framerate) {
            int frameRateRounded = Math.round(framerate);
            if (frameRateRounded == 0) {
                return;
            }

            ListPreference pref = findPreference(PreferenceConfiguration.FPS_PREF_STRING);
            if (pref == null) return;

            String fpsValue = Integer.toString(frameRateRounded);
            String fpsName = getResources().getString(R.string.resolution_prefix_native) +
                    " (" + fpsValue + " " + getResources().getString(R.string.fps_suffix_fps) + ")";

            // Check if the native frame rate is already present
            for (CharSequence value : pref.getEntryValues()) {
                if (fpsValue.equals(value.toString())) {
                    // It is present in the default list, so don't add it again
                    nativeFramerateShown = false;
                    return;
                }
            }

            appendPreferenceEntry(pref, fpsName, fpsValue);
            nativeFramerateShown = true;
        }

        private void removeValue(String preferenceKey, String value, Runnable onMatched) {
            int matchingCount = 0;

            ListPreference pref = findPreference(preferenceKey);
            if (pref == null) return;

            // Count the number of matching entries we'll be removing
            for (CharSequence seq : pref.getEntryValues()) {
                if (seq.toString().equalsIgnoreCase(value)) {
                    matchingCount++;
                }
            }

            // Create the new arrays
            CharSequence[] entries = new CharSequence[pref.getEntries().length - matchingCount];
            CharSequence[] entryValues = new CharSequence[pref.getEntryValues().length - matchingCount];
            int outIndex = 0;
            for (int i = 0; i < pref.getEntryValues().length; i++) {
                if (pref.getEntryValues()[i].toString().equalsIgnoreCase(value)) {
                    // Skip matching values
                    continue;
                }

                entries[outIndex] = pref.getEntries()[i];
                entryValues[outIndex] = pref.getEntryValues()[i];
                outIndex++;
            }

            if (pref.getValue().equalsIgnoreCase(value)) {
                onMatched.run();
            }

            // Update the preference with the new list
            pref.setEntries(entries);
            pref.setEntryValues(entryValues);
        }

        /*private void resetBitrateToDefault(SharedPreferences prefs, String res, String fps) {
            if (res == null) {
                res = prefs.getString(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.DEFAULT_RESOLUTION);
            }
            if (fps == null) {
                fps = prefs.getString(PreferenceConfiguration.FPS_PREF_STRING, PreferenceConfiguration.DEFAULT_FPS);
            }

            // Read the current video format setting
            PreferenceConfiguration.FormatOption videoFormat = PreferenceConfiguration.FormatOption.AUTO;
            String videoFormatStr = prefs.getString("video_format", "auto");
            switch (videoFormatStr) {
                case "auto":
                    videoFormat = PreferenceConfiguration.FormatOption.AUTO;
                    break;
                case "forceav1":
                    videoFormat = PreferenceConfiguration.FormatOption.FORCE_AV1;
                    break;
                case "forceh265":
                    videoFormat = PreferenceConfiguration.FormatOption.FORCE_HEVC;
                    break;
            }

            int newBitrate = PreferenceConfiguration.getDefaultBitrate(res, fps, videoFormat);

            prefs.edit()
                    .putInt(PreferenceConfiguration.BITRATE_PREF_STRING, newBitrate)
                    .apply();

            // Update the SeekBarPreference UI
            SeekBarPreference bitratePref = findPreference(PreferenceConfiguration.BITRATE_PREF_STRING);
            if (bitratePref != null) {
                bitratePref.setValue(newBitrate);
            }
        }*/

        @Override
        public void onViewCreated(@NonNull View view, @Nullable Bundle savedInstanceState) {
            super.onViewCreated(view, savedInstanceState);
            UiHelper.applyStatusBarPadding(view);
        }


        @Override
        public void onCreatePreferences(Bundle savedInstanceState, String rootKey) {
            // Set MMKV as the preference data store before loading preferences
            getPreferenceManager().setPreferenceDataStore(
                    MMKVPreferenceManager.getPreferenceDataStore(requireActivity()));

            setPreferencesFromResource(R.xml.preferences, rootKey);
            PreferenceScreen screen = getPreferenceScreen();

            // hide on-screen controls category on non touch screen devices
            if (!requireActivity().getPackageManager().hasSystemFeature(PackageManager.FEATURE_TOUCHSCREEN)) {
                PreferenceCategory category = findPreference("category_onscreen_controls");
                if (category != null) {
                    screen.removePreference(category);
                }
            }

            // Hide remote desktop mouse mode on pre-Oreo (which doesn't have pointer capture)
            // and NVIDIA SHIELD devices (which support raw mouse input in pointer capture mode)
            if (requireActivity().getPackageManager().hasSystemFeature("com.nvidia.feature.shield")) {
                PreferenceCategory category = findPreference("category_input_settings");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_absolute_mouse_mode"));
                }
            }

            // Hide gamepad motion sensor fallback option if the device has no gyro or accelerometer
            if (!requireActivity().getPackageManager().hasSystemFeature(PackageManager.FEATURE_SENSOR_ACCELEROMETER) &&
                    !requireActivity().getPackageManager().hasSystemFeature(PackageManager.FEATURE_SENSOR_GYROSCOPE)) {
                PreferenceCategory category = findPreference("category_gamepad_settings");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_gamepad_motion_fallback"));
                }
            }

            // Hide USB driver options on devices without USB host support
            if (!requireActivity().getPackageManager().hasSystemFeature(PackageManager.FEATURE_USB_HOST)) {
                PreferenceCategory category = findPreference("category_gamepad_settings");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_usb_bind_all"));
                    category.removePreference(findPreference("checkbox_usb_driver"));
                }
            }

            // Remove PiP mode on devices pre-Oreo, where the feature is not available (some low RAM devices),
            // and on Fire OS where it violates the Amazon App Store guidelines for some reason.
            if (!requireActivity().getPackageManager().hasSystemFeature("android.software.picture_in_picture") || requireActivity().getPackageManager().hasSystemFeature("com.amazon.software.fireos")) {
                PreferenceCategory category = findPreference("category_ui_settings");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_enable_pip"));
                }
            }

            PreferenceCategory category_gamepad_settings = findPreference("category_gamepad_settings");
            Vibrator deviceVibrator = (Vibrator) requireActivity().getSystemService(Context.VIBRATOR_SERVICE);

            // Remove the vibration options if the device can't vibrate or doesn't support amplitude control
            // Amplitude control is required for proper dual-motor rumble simulation
            if (!deviceVibrator.hasVibrator() || !deviceVibrator.hasAmplitudeControl()) {
                if (category_gamepad_settings != null) {
                    category_gamepad_settings.removePreference(findPreference("checkbox_vibrate_fallback"));
                    category_gamepad_settings.removePreference(findPreference("seekbar_vibrate_fallback_strength"));
                }
                // The entire OSC category may have already been removed by the touchscreen check above
                PreferenceCategory category = findPreference("category_onscreen_controls");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_vibrate_osc"));
                }
            }

            // Setup gamepad test preference click listener
            Preference gamepadTestPref = findPreference("pref_gamepad_test");
            if (gamepadTestPref != null) {
                gamepadTestPref.setOnPreferenceClickListener(preference -> {
                    android.content.Intent intent = new android.content.Intent(requireActivity(), GamepadTestActivity.class);
                    startActivity(intent);
                    return true;
                });
            }

            Display display = requireActivity().getDisplay();
            float maxSupportedFps = display.getRefreshRate();

            // Hide non-supported resolution/FPS combinations
            int maxSupportedResW = 0;

            // Add a native resolution with any insets included for users that don't want content
            // behind the notch of their display
            boolean hasInsets = false;
            DisplayCutout cutout;

            // Use the much nicer Display.getCutout() API on Android 10+
            cutout = display.getCutout();

            if (cutout != null) {
                int widthInsets = cutout.getSafeInsetLeft() + cutout.getSafeInsetRight();
                int heightInsets = cutout.getSafeInsetBottom() + cutout.getSafeInsetTop();

                if (widthInsets != 0 || heightInsets != 0) {
                    DisplayMetrics metrics = new DisplayMetrics();
                    display.getRealMetrics(metrics);

                    int width = Math.max(metrics.widthPixels - widthInsets, metrics.heightPixels - heightInsets);
                    int height = Math.min(metrics.widthPixels - widthInsets, metrics.heightPixels - heightInsets);

                    addNativeResolutionEntries(width, height, false);
                    hasInsets = true;
                }
            }

            // Always allow resolutions that are smaller or equal to the active
            // display resolution because decoders can report total non-sense to us.
            // For example, a p201 device reports:
            // AVC Decoder: OMX.amlogic.avc.decoder.awesome
            // HEVC Decoder: OMX.amlogic.hevc.decoder.awesome
            // AVC supported width range: 64 - 384
            // HEVC supported width range: 64 - 544
            for (Display.Mode candidate : display.getSupportedModes()) {
                // Some devices report their dimensions in the portrait orientation
                // where height > width. Normalize these to the conventional width > height
                // arrangement before we process them.

                int width = Math.max(candidate.getPhysicalWidth(), candidate.getPhysicalHeight());
                int height = Math.min(candidate.getPhysicalWidth(), candidate.getPhysicalHeight());

                // Add native resolution entries for all devices
                addNativeResolutionEntries(width, height, hasInsets);

                if ((width >= 3840 || height >= 2160) && maxSupportedResW < 3840) {
                    maxSupportedResW = 3840;
                } else if ((width >= 2560 || height >= 1440) && maxSupportedResW < 2560) {
                    maxSupportedResW = 2560;
                } else if ((width >= 1920 || height >= 1080) && maxSupportedResW < 1920) {
                    maxSupportedResW = 1920;
                }

                if (candidate.getRefreshRate() > maxSupportedFps) {
                    maxSupportedFps = candidate.getRefreshRate();
                }
            }

            // This must be called to do runtime initialization before calling functions that evaluate
            // decoder lists.
            MediaCodecHelper.initialize(getContext(), VulkanPreferences.readPreferences(getContext()).VulkanRenderer);

            MediaCodecInfo avcDecoder = MediaCodecHelper.findProbableSafeDecoder("video/avc", -1);
            MediaCodecInfo hevcDecoder = MediaCodecHelper.findProbableSafeDecoder("video/hevc", -1);

            if (avcDecoder != null) {
                Range<Integer> avcWidthRange = Objects.requireNonNull(avcDecoder.getCapabilitiesForType("video/avc").getVideoCapabilities()).getSupportedWidths();

                LimeLog.info("AVC supported width range: " + avcWidthRange.getLower() + " - " + avcWidthRange.getUpper());

                // If 720p is not reported as supported, ignore all results from this API
                if (avcWidthRange.contains(1280)) {
                    if (avcWidthRange.contains(3840) && maxSupportedResW < 3840) {
                        maxSupportedResW = 3840;
                    } else if (avcWidthRange.contains(1920) && maxSupportedResW < 1920) {
                        maxSupportedResW = 1920;
                    } else if (maxSupportedResW < 1280) {
                        maxSupportedResW = 1280;
                    }
                }
            }

            if (hevcDecoder != null) {
                Range<Integer> hevcWidthRange = Objects.requireNonNull(hevcDecoder.getCapabilitiesForType("video/hevc").getVideoCapabilities()).getSupportedWidths();

                LimeLog.info("HEVC supported width range: " + hevcWidthRange.getLower() + " - " + hevcWidthRange.getUpper());

                // If 720p is not reported as supported, ignore all results from this API
                if (hevcWidthRange.contains(1280)) {
                    if (hevcWidthRange.contains(3840) && maxSupportedResW < 3840) {
                        maxSupportedResW = 3840;
                    } else if (hevcWidthRange.contains(1920) && maxSupportedResW < 1920) {
                        maxSupportedResW = 1920;
                    } else if (maxSupportedResW < 1280) {
                        maxSupportedResW = 1280;
                    }
                }
            }

            LimeLog.info("Maximum resolution slot: " + maxSupportedResW);

            if (maxSupportedResW != 0) {
                if (maxSupportedResW < 3840) {
                    // 4K is unsupported
                    removeValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_4K, () -> {
                        setValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_1440P);
                    });
                }
                if (maxSupportedResW < 2560) {
                    // 1440p is unsupported
                    removeValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_1440P, () -> {
                        setValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_1080P);
                    });
                }
                if (maxSupportedResW < 1920) {
                    // 1080p is unsupported
                    removeValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_1080P, () -> {
                        setValue(PreferenceConfiguration.RESOLUTION_PREF_STRING, PreferenceConfiguration.RES_1080P);
                    });
                }
            }

            if (!PreferenceConfiguration.readPreferences(requireActivity()).unlockFps) {
                // We give some extra room in case the FPS is rounded down
                if (maxSupportedFps < 118) {
                    removeValue(PreferenceConfiguration.FPS_PREF_STRING, "120", () -> {
                        setValue(PreferenceConfiguration.FPS_PREF_STRING, "90");
                    });
                }
                if (maxSupportedFps < 88) {
                    // 90fps is unsupported
                    removeValue(PreferenceConfiguration.FPS_PREF_STRING, "90", () -> {
                        setValue(PreferenceConfiguration.FPS_PREF_STRING, "60");
                    });
                }
                // Never remove 30 FPS or 60 FPS
            }
            addNativeFrameRateEntry(maxSupportedFps);

            // Android L introduces the drop duplicate behavior of releaseOutputBuffer()
            // that the unlock FPS option relies on to not massively increase latency.
            Preference unlockFpsPref = findPreference(PreferenceConfiguration.UNLOCK_FPS_STRING);
            if (unlockFpsPref != null) {
                unlockFpsPref.setOnPreferenceChangeListener((preference, newValue) -> {
                    // HACK: We need to let the preference change succeed before reinitializing to ensure
                    // it's reflected in the new layout.
                    final Handler h = new Handler();
                    h.postDelayed(() -> {
                        // Ensure the activity is still open when this timeout expires
                        StreamSettings settingsActivity = (StreamSettings) getActivity();
                        if (settingsActivity != null) {
                            settingsActivity.reloadSettings();
                        }
                    }, 500);

                    // Allow the original preference change to take place
                    return true;
                });
            }

            // Request notification permission when stats notification is enabled
            Preference statsNotifPref = findPreference("checkbox_enable_stats_notification");
            if (statsNotifPref != null) {
                statsNotifPref.setOnPreferenceChangeListener((preference, newValue) -> {
                    if ((Boolean) newValue) {
                        // User is enabling stats notification, request permission
                        Activity activity = getActivity();
                        if (activity != null) {
                            if (activity.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                                activity.requestPermissions(
                                        new String[]{android.Manifest.permission.POST_NOTIFICATIONS},
                                        REQUEST_NOTIFICATION_PERMISSION
                                );
                            }
                        }
                    }
                    return true;
                });
            }

            Display.HdrCapabilities hdrCaps = display.getHdrCapabilities();

            // We must now ensure our display is compatible with HDR10
            boolean foundHdr10 = false;
            if (hdrCaps != null) {
                // getHdrCapabilities() returns null on Lenovo Lenovo Mirage Solo (vega), Android 8.0
                for (int hdrType : hdrCaps.getSupportedHdrTypes()) {
                    if (hdrType == Display.HdrCapabilities.HDR_TYPE_HDR10) {
                        foundHdr10 = true;
                        break;
                    }
                }
            }

            if (!foundHdr10) {
                LimeLog.info("Excluding HDR toggle based on display capabilities");
                PreferenceCategory category = findPreference("category_advanced_settings");
                if (category != null) {
                    category.removePreference(findPreference("checkbox_enable_hdr"));
                }
            }

            // Handle auto bitrate checkbox - disable manual bitrate seekbar when auto is enabled
            CheckBoxPreference autoBitratePref = findPreference("checkbox_auto_bitrate");
            SeekBarPreference bitratePref = findPreference(PreferenceConfiguration.BITRATE_PREF_STRING);
            if (autoBitratePref != null && bitratePref != null) {
                // Set initial state - disable seekbar when auto bitrate is enabled
                boolean autoBitrateEnabled = autoBitratePref.isChecked();
                bitratePref.setEnabled(!autoBitrateEnabled);

                autoBitratePref.setOnPreferenceChangeListener((preference, newValue) -> {
                    boolean autoEnabled = (Boolean) newValue;
                    bitratePref.setEnabled(!autoEnabled);
                    return true;
                });
            }

            // Add a listener to the FPS and resolution preference
            // so the bitrate can be auto-adjusted when auto bitrate is enabled
            Preference resPref = findPreference(PreferenceConfiguration.RESOLUTION_PREF_STRING);
            if (resPref != null) {
                resPref.setOnPreferenceChangeListener((preference, newValue) -> {
                    String valueStr = (String) newValue;

                    // Detect if this value is the native resolution option
                    CharSequence[] values = ((ListPreference) preference).getEntryValues();
                    boolean isNativeRes = true;
                    for (int i = 0; i < values.length; i++) {
                        // Look for a match prior to the start of the native resolution entries
                        if (valueStr.equals(values[i].toString()) && i < nativeResolutionStartIndex) {
                            isNativeRes = false;
                            break;
                        }
                    }

                    // If this is native resolution, show the warning dialog
                    if (isNativeRes) {
                        Dialog.displayDialog(getActivity(),
                                getResources().getString(R.string.title_native_res_dialog),
                                getResources().getString(R.string.text_native_res_dialog),
                                false);
                    }


                    // Allow the original preference change to take place
                    return true;
                });
            }

            Preference fpsPref = findPreference(PreferenceConfiguration.FPS_PREF_STRING);
            if (fpsPref != null) {
                fpsPref.setOnPreferenceChangeListener((preference, newValue) -> {
                    // If this is native frame rate, show the warning dialog
                    CharSequence[] values = ((ListPreference) preference).getEntryValues();
                    if (nativeFramerateShown && values[values.length - 1].toString().equals(newValue.toString())) {
                        Dialog.displayDialog(getActivity(),
                                getResources().getString(R.string.title_native_fps_dialog),
                                getResources().getString(R.string.text_native_res_dialog),
                                false);
                    }

                    // Allow the original preference change to take place
                    return true;
                });
            }

        }
    }
}
