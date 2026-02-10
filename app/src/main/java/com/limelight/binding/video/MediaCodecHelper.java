package com.limelight.binding.video;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileReader;
import java.util.Collections;
import java.util.LinkedList;
import java.util.List;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import android.annotation.SuppressLint;
import android.app.ActivityManager;
import android.content.Context;
import android.content.pm.ConfigurationInfo;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaCodecInfo.CodecCapabilities;
import android.media.MediaCodecInfo.CodecProfileLevel;
import android.media.MediaFormat;
import android.os.Build;
import android.util.Log;

import com.limelight.LimeLog;
import com.limelight.preferences.PreferenceConfiguration;

@SuppressWarnings("RedundantSuppression")
public class MediaCodecHelper {

    private static final String TAG = "MediaCodecHelper";
    private static final List<String> preferredDecoders;

    private static final List<String> blacklistedDecoderPrefixes;
    private static final List<String> spsFixupBitstreamFixupDecoderPrefixes;
    private static final List<String> blacklistedAdaptivePlaybackPrefixes;
    private static final List<String> baselineProfileHackPrefixes;
    private static final List<String> directSubmitPrefixes;
    private static final List<String> constrainedHighProfilePrefixes;
    private static final List<String> whitelistedHevcDecoders;
    private static final List<String> refFrameInvalidationAvcPrefixes;
    private static final List<String> refFrameInvalidationHevcPrefixes;
    private static final List<String> useFourSlicesPrefixes;
    private static final List<String> qualcommDecoderPrefixes;
    private static final List<String> kirinDecoderPrefixes;
    private static final List<String> exynosDecoderPrefixes;
    private static final List<String> amlogicDecoderPrefixes;
    private static final List<String> knownVendorLowLatencyOptions;

    public static final boolean SHOULD_BYPASS_SOFTWARE_BLOCK =
            Build.HARDWARE.equals("ranchu") || Build.HARDWARE.equals("cheets") || Build.BRAND.equals("Android-x86");

    private static boolean isLowEndSnapdragon = false;
    private static boolean isAdreno620 = false;
    private static boolean initialized = false;

    static {
        directSubmitPrefixes = new LinkedList<>();


        // All Codec2 decoders
        directSubmitPrefixes.add("c2.");
    }

    static {
        refFrameInvalidationAvcPrefixes = new LinkedList<>();

        refFrameInvalidationHevcPrefixes = new LinkedList<>();
        refFrameInvalidationHevcPrefixes.add("c2.exynos");

        // Qualcomm and NVIDIA may be added at runtime
    }

    static {
        preferredDecoders = new LinkedList<>();
    }

    static {
        blacklistedDecoderPrefixes = new LinkedList<>();

        // Blacklist all OMX decoders - only support C2
        blacklistedDecoderPrefixes.add("omx.");
        blacklistedDecoderPrefixes.add("OMX.");

        // Blacklist software decoders that don't support H264 high profile except on systems
        // that are expected to only have software decoders (like emulators).
        if (!SHOULD_BYPASS_SOFTWARE_BLOCK) {
            blacklistedDecoderPrefixes.add("AVCDecoder");

            // We want to avoid ffmpeg decoders since they're usually software decoders,
            // but we'll defer to the Android 10 isSoftwareOnly() API on newer devices
            // to determine if we should use these or not.
        }
    }

    static {
        // If a decoder qualifies for reference frame invalidation,
        // these entries will be ignored for those decoders.
        spsFixupBitstreamFixupDecoderPrefixes = new LinkedList<>();

        baselineProfileHackPrefixes = new LinkedList<>();

        blacklistedAdaptivePlaybackPrefixes = new LinkedList<>();

        constrainedHighProfilePrefixes = new LinkedList<>();
    }

    static {
        whitelistedHevcDecoders = new LinkedList<>();

        // Allow software HEVC decoding in the official AOSP emulator
        if (Build.HARDWARE.equals("ranchu")) {
            whitelistedHevcDecoders.add("c2.android");
        }

        // Let's see if HEVC decoders are finally stable with C2
        whitelistedHevcDecoders.add("c2.");

        // Based on GPU attributes queried at runtime, the c2.qti prefix will be added
        // during initialization to avoid SoCs with broken HEVC decoders.
    }

    static {
        useFourSlicesPrefixes = new LinkedList<>();

        // Software decoders will use 4 slices per frame to allow for slice multithreading
        useFourSlicesPrefixes.add("c2.android");

        // Old Qualcomm decoders are detected at runtime
    }

    static {
        knownVendorLowLatencyOptions = new LinkedList<>();

        knownVendorLowLatencyOptions.add("vendor.qti-ext-dec-low-latency.enable");
        knownVendorLowLatencyOptions.add("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-req");
        knownVendorLowLatencyOptions.add("vendor.rtc-ext-dec-low-latency.enable");
        knownVendorLowLatencyOptions.add("vendor.low-latency.enable");
    }

    static {
        qualcommDecoderPrefixes = new LinkedList<>();

        qualcommDecoderPrefixes.add("c2.qti");
    }

    static {
        kirinDecoderPrefixes = new LinkedList<>();

        kirinDecoderPrefixes.add("c2.hisi"); // Unconfirmed
    }

    static {
        exynosDecoderPrefixes = new LinkedList<>();

        exynosDecoderPrefixes.add("c2.exynos");
    }

    static {
        amlogicDecoderPrefixes = new LinkedList<>();

        amlogicDecoderPrefixes.add("c2.amlogic"); // Unconfirmed
    }

    private static boolean isPowerVR(String glRenderer) {
        return glRenderer.toLowerCase().contains("powervr");
    }

    private static String getAdrenoVersionString(String glRenderer) {
        glRenderer = glRenderer.toLowerCase().trim();

        if (!glRenderer.contains("adreno")) {
            return null;
        }

        Pattern modelNumberPattern = Pattern.compile("(.*)([0-9]{3})(.*)");

        Matcher matcher = modelNumberPattern.matcher(glRenderer);
        if (!matcher.matches()) {
            return null;
        }

        String modelNumber = matcher.group(2);
        LimeLog.info("Found Adreno GPU: " + modelNumber);
        return modelNumber;
    }

    private static boolean isLowEndSnapdragonRenderer(String glRenderer) {
        String modelNumber = getAdrenoVersionString(glRenderer);
        if (modelNumber == null) {
            // Not an Adreno GPU
            return false;
        }

        // The current logic is to identify low-end SoCs based on a zero in the x0x place.
        return modelNumber.charAt(1) == '0';
    }

    private static int getAdrenoRendererModelNumber(String glRenderer) {
        String modelNumber = getAdrenoVersionString(glRenderer);
        if (modelNumber == null) {
            // Not an Adreno GPU
            return -1;
        }

        return Integer.parseInt(modelNumber);
    }

    // This is a workaround for some broken devices that report
    // only GLES 3.0 even though the GPU is an Adreno 4xx series part.
    // An example of such a device is the Huawei Honor 5x with the
    // Snapdragon 616 SoC (Adreno 405).
    private static boolean isGLES31SnapdragonRenderer(String glRenderer) {
        // Snapdragon 4xx and higher support GLES 3.1
        return getAdrenoRendererModelNumber(glRenderer) >= 400;
    }

    public static void initialize(Context context, String glRenderer) {
        if (initialized) {
            return;
        }

        ActivityManager activityManager =
                (ActivityManager) context.getSystemService(Context.ACTIVITY_SERVICE);
        ConfigurationInfo configInfo = activityManager.getDeviceConfigurationInfo();
        if (configInfo.reqGlEsVersion != ConfigurationInfo.GL_ES_VERSION_UNDEFINED) {
            LimeLog.info("OpenGL ES version: " + configInfo.reqGlEsVersion);

            isLowEndSnapdragon = isLowEndSnapdragonRenderer(glRenderer);
            isAdreno620 = getAdrenoRendererModelNumber(glRenderer) == 620;

            // Tegra K1 and later can do reference frame invalidation properly
            if (configInfo.reqGlEsVersion >= 0x30000) {
                LimeLog.info("Added c2.nvidia to reference frame invalidation support list");

                refFrameInvalidationAvcPrefixes.add("c2.nvidia"); // Unconfirmed
                refFrameInvalidationHevcPrefixes.add("c2.nvidia"); // Unconfirmed

                LimeLog.info("Added c2.qti to reference frame invalidation support list");
                refFrameInvalidationAvcPrefixes.add("c2.qti");
                refFrameInvalidationHevcPrefixes.add("c2.qti");
            }

            // Qualcomm's early HEVC decoders break hard on our HEVC stream. The best check to
            // tell the good from the bad decoders are the generation of Adreno GPU included:
            // 3xx - bad
            // 4xx - good
            //
            // The "good" GPUs support GLES 3.1, but we can't just check that directly
            // (see comment on isGLES31SnapdragonRenderer).
            //
            if (isGLES31SnapdragonRenderer(glRenderer)) {
                LimeLog.info("Added c2.qti to HEVC decoders based on GLES 3.1+ support");
                whitelistedHevcDecoders.add("c2.qti");
            } else {
                // These older decoders need 4 slices per frame for best performance
                useFourSlicesPrefixes.add("c2.qti");
            }

            // MediaTek SoCs with PowerVR GPUs have good HEVC support.
            if (isPowerVR(glRenderer)) {
                LimeLog.info("Added c2.mtk to HEVC decoders based on PowerVR GPU");
                whitelistedHevcDecoders.add("c2.mtk");

                // RFI on HEVC causes decoder hangs on the newer GE8100, GE8300, and GE8320 GPUs,
                // so we limit it to the Series6XT GPUs where we know it works.
                if (glRenderer.contains("GX6")) {
                    LimeLog.info("Added c2.mtk to RFI list for HEVC");
                    refFrameInvalidationHevcPrefixes.add("c2.mtk");
                }
            }
        }

        initialized = true;
    }

    private static boolean isDecoderInList(List<String> decoderList, String decoderName) {
        if (!initialized) {
            throw new IllegalStateException("MediaCodecHelper must be initialized before use");
        }

        for (String badPrefix : decoderList) {
            if (decoderName.length() >= badPrefix.length()) {
                String prefix = decoderName.substring(0, badPrefix.length());
                if (prefix.equalsIgnoreCase(badPrefix)) {
                    return true;
                }
            }
        }

        return false;
    }

    private static boolean decoderSupportsAndroidRLowLatency(MediaCodecInfo decoderInfo, String mimeType) {
        try {
            if (decoderInfo.getCapabilitiesForType(mimeType).isFeatureSupported(CodecCapabilities.FEATURE_LowLatency)) {
                LimeLog.info("Low latency decoding mode supported (FEATURE_LowLatency)");
                return true;
            }
        } catch (Exception e) {
            // Tolerate buggy codecs
            Log.e(TAG, "decoderSupportsAndroidRLowLatency: " + e.getMessage(), e);
        }

        return false;
    }

    private static boolean decoderSupportsKnownVendorLowLatencyOption(String decoderName) {
        // It's only possible to probe vendor parameters on Android 12 and above.
        MediaCodec testCodec = null;
        try {
            // Unfortunately we have to create an actual codec instance to get supported options.
            testCodec = MediaCodec.createByCodecName(decoderName);

            // See if any of the vendor parameters match ones we know about
            for (String supportedOption : testCodec.getSupportedVendorParameters()) {
                for (String knownLowLatencyOption : knownVendorLowLatencyOptions) {
                    if (supportedOption.equalsIgnoreCase(knownLowLatencyOption)) {
                        LimeLog.info(decoderName + " supports known low latency option: " + supportedOption);
                        return true;
                    }
                }
            }
        } catch (Exception e) {
            // Tolerate buggy codecs
            Log.e(TAG, "decoderSupportsKnownVendorLowLatencyOption: " + e.getMessage(), e);
        } finally {
            if (testCodec != null) {
                testCodec.release();
            }
        }
        return false;
    }

    private static boolean decoderSupportsMaxOperatingRate(String decoderName) {
        // Operate at maximum rate to lower latency as much as possible on
        // some Qualcomm platforms. We could also set KEY_PRIORITY to 0 (realtime)
        // but that will actually result in the decoder crashing if it can't satisfy
        // our (ludicrous) operating rate requirement. This seems to cause reliable
        // crashes on the Xiaomi Mi 10 lite 5G and Redmi K30i 5G on Android 10, so
        // we'll disable it on Snapdragon 765G and all non-Qualcomm devices to be safe.
        //
        // NB: Even on Android 10, this optimization still provides significant
        // performance gains on Pixel 2.
        return isDecoderInList(qualcommDecoderPrefixes, decoderName) && !isAdreno620;
    }

    public static boolean setDecoderLowLatencyOptions(MediaFormat videoFormat, MediaCodecInfo decoderInfo, int tryNumber) {
        // Options here should be tried in the order of most to least risky. The decoder will use
        // the first MediaFormat that doesn't fail in configure().

        boolean setNewOption = false;

        if (tryNumber < 1) {
            // Official Android 11+ low latency option (KEY_LOW_LATENCY).
            videoFormat.setInteger("low-latency", 1);
            setNewOption = true;

            // If this decoder officially supports FEATURE_LowLatency, we will just use that alone
            // for try 0. Otherwise, we'll include it as best effort with other options.
            if (decoderSupportsAndroidRLowLatency(decoderInfo, videoFormat.getString(MediaFormat.KEY_MIME))) {
                return true;
            }
        }

        if (tryNumber < 2 &&
                (!Build.MANUFACTURER.equalsIgnoreCase("xiaomi"))) {
            // MediaTek decoders don't use vendor-defined keys for low latency mode. Instead, they have a modified
            // version of AOSP's ACodec.cpp which supports the "vdec-lowlatency" option. This option is passed down
            // to the decoder as OMX.MTK.index.param.video.LowLatencyDecode.
            //
            // This option is also plumbed for Amazon Amlogic-based devices like the Fire TV 3. Not only does it
            // reduce latency on Amlogic, it fixes the HEVC bug that causes the decoder to not output any frames.
            // Unfortunately, it does the exact opposite for the Xiaomi MITV4-ANSM0, breaking it in the way that
            // Fire TV was broken prior to vdec-lowlatency :(
            //
            // On Fire TV 3, vdec-lowlatency is translated to OMX.amazon.fireos.index.video.lowLatencyDecode.
            //
            // https://github.com/yuan1617/Framwork/blob/master/frameworks/av/media/libstagefright/ACodec.cpp
            // https://github.com/iykex/vendor_mediatek_proprietary_hardware/blob/master/libomx/video/MtkOmxVdecEx/MtkOmxVdecEx.h
            videoFormat.setInteger("vdec-lowlatency", 1);
            setNewOption = true;
        }

        if (tryNumber < 3) {
            if (MediaCodecHelper.decoderSupportsMaxOperatingRate(decoderInfo.getName())) {
                videoFormat.setInteger(MediaFormat.KEY_OPERATING_RATE, Short.MAX_VALUE);
                setNewOption = true;
            } else {
                videoFormat.setInteger(MediaFormat.KEY_PRIORITY, 0);
                setNewOption = true;
            }
        }

        // MediaCodec supports vendor-defined format keys using the "vendor.<extension name>.<parameter name>" syntax.
        // These allow access to functionality that is not exposed through documented MediaFormat.KEY_* values.
        // https://cs.android.com/android/platform/superproject/+/master:hardware/qcom/sdm845/media/mm-video-v4l2/vidc/common/inc/vidc_vendor_extensions.h;l=67
        //
        // MediaCodec vendor extension support was introduced in Android 8.0:
        // https://cs.android.com/android/_/android/platform/frameworks/av/+/01c10f8cdcd58d1e7025f426a72e6e75ba5d7fc2
        // Try vendor-specific low latency options
        //
        // NOTE: Update knownVendorLowLatencyOptions if you modify this code!
        if (isDecoderInList(qualcommDecoderPrefixes, decoderInfo.getName())) {
            // Examples of Qualcomm's vendor extensions for Snapdragon 845:
            // https://cs.android.com/android/platform/superproject/+/master:hardware/qcom/sdm845/media/mm-video-v4l2/vidc/vdec/src/omx_vdec_extensions.hpp
            // https://cs.android.com/android/_/android/platform/hardware/qcom/sm8150/media/+/0621ceb1c1b19564999db8293574a0e12952ff6c
            //
            // We will first try both, then try vendor.qti-ext-dec-low-latency.enable alone if that fails
            if (tryNumber < 4) {
                videoFormat.setInteger("vendor.qti-ext-dec-picture-order.enable", 1);
                setNewOption = true;
            }
            if (tryNumber < 5) {
                videoFormat.setInteger("vendor.qti-ext-dec-low-latency.enable", 1);
                setNewOption = true;
            }
        } else if (isDecoderInList(kirinDecoderPrefixes, decoderInfo.getName())) {
            if (tryNumber < 4) {
                // Kirin low latency options
                // https://developer.huawei.com/consumer/cn/forum/topic/0202325564295980115
                videoFormat.setInteger("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-req", 1);
                videoFormat.setInteger("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-rdy", -1);
                setNewOption = true;
            }
        } else if (isDecoderInList(exynosDecoderPrefixes, decoderInfo.getName())) {
            if (tryNumber < 4) {
                // Exynos low latency option for H.264 decoder
                videoFormat.setInteger("vendor.rtc-ext-dec-low-latency.enable", 1);
                setNewOption = true;
            }
        } else if (isDecoderInList(amlogicDecoderPrefixes, decoderInfo.getName())) {
            if (tryNumber < 4) {
                // Amlogic low latency vendor extension
                // https://github.com/codewalkerster/android_vendor_amlogic_common_prebuilt_libstagefrighthw/commit/41fefc4e035c476d58491324a5fe7666bfc2989e
                videoFormat.setInteger("vendor.low-latency.enable", 1);
                setNewOption = true;
            }
        }

        return setNewOption;
    }

    public static boolean decoderSupportsFusedIdrFrame(MediaCodecInfo decoderInfo, String mimeType) {
        // If adaptive playback is supported, we can submit new CSD together with a keyframe
        try {
            if (decoderInfo.getCapabilitiesForType(mimeType).
                    isFeatureSupported(CodecCapabilities.FEATURE_AdaptivePlayback)) {
                LimeLog.info("Decoder supports fused IDR frames (FEATURE_AdaptivePlayback)");
                return true;
            }
        } catch (Exception e) {
            // Tolerate buggy codecs
            Log.e(TAG, "decoderSupportsFusedIdrFrame: "+e.getMessage(), e);
        }

        return false;
    }

    public static boolean decoderSupportsAdaptivePlayback(MediaCodecInfo decoderInfo, String mimeType) {
        if (isDecoderInList(blacklistedAdaptivePlaybackPrefixes, decoderInfo.getName())) {
            LimeLog.info("Decoder blacklisted for adaptive playback");
            return false;
        }

        try {
            if (decoderInfo.getCapabilitiesForType(mimeType).
                    isFeatureSupported(CodecCapabilities.FEATURE_AdaptivePlayback)) {
                // This will make getCapabilities() return that adaptive playback is supported
                LimeLog.info("Adaptive playback supported (FEATURE_AdaptivePlayback)");
                return true;
            }
        } catch (Exception e) {
            // Tolerate buggy codecs
            Log.e(TAG, "decoderSupportsAdaptivePlayback: "+e.getMessage(), e);
        }

        return false;
    }

    public static boolean decoderNeedsConstrainedHighProfile(String decoderName) {
        return isDecoderInList(constrainedHighProfilePrefixes, decoderName);
    }

    public static boolean decoderCanDirectSubmit(String decoderName) {
        return isDecoderInList(directSubmitPrefixes, decoderName) && !isExynos4Device();
    }

    public static boolean decoderNeedsSpsBitstreamRestrictions(String decoderName) {
        return isDecoderInList(spsFixupBitstreamFixupDecoderPrefixes, decoderName);
    }

    public static boolean decoderNeedsBaselineSpsHack(String decoderName) {
        return isDecoderInList(baselineProfileHackPrefixes, decoderName);
    }

    public static byte getDecoderOptimalSlicesPerFrame(String decoderName) {
        if (isDecoderInList(useFourSlicesPrefixes, decoderName)) {
            // 4 slices per frame reduces decoding latency on older Qualcomm devices
            return 4;
        } else {
            // 1 slice per frame produces the optimal encoding efficiency
            return 1;
        }
    }

    public static boolean decoderSupportsRefFrameInvalidationAvc(String decoderName, int videoHeight) {
        // Reference frame invalidation is broken on low-end Snapdragon SoCs at 1080p.
        if (videoHeight > 720 && isLowEndSnapdragon) {
            return false;
        }

        // This device seems to crash constantly at 720p, so try disabling
        // RFI to see if we can get that under control.
        if (Build.DEVICE.equals("b3") || Build.DEVICE.equals("b5")) {
            return false;
        }

        return isDecoderInList(refFrameInvalidationAvcPrefixes, decoderName);
    }

    public static boolean decoderSupportsRefFrameInvalidationHevc(MediaCodecInfo decoderInfo) {
        // HEVC decoders seem to universally support RFI, but it can have huge latency penalties
        // for some decoders due to the number of references frames being > 1. Old Amlogic
        // decoders are known to have this problem.
        //
        // If the decoder supports FEATURE_LowLatency or any vendor low latency option,
        // we will use that as an indication that it can handle HEVC RFI without excessively
        // buffering frames.
        if (decoderSupportsAndroidRLowLatency(decoderInfo, "video/hevc") ||
                decoderSupportsKnownVendorLowLatencyOption(decoderInfo.getName())) {
            LimeLog.info("Enabling HEVC RFI based on low latency option support");
            return true;
        }

        return isDecoderInList(refFrameInvalidationHevcPrefixes, decoderInfo.getName());
    }

    public static boolean decoderSupportsRefFrameInvalidationAv1(MediaCodecInfo decoderInfo) {
        // We'll use the same heuristics as HEVC for now
        if (decoderSupportsAndroidRLowLatency(decoderInfo, "video/av01") ||
                decoderSupportsKnownVendorLowLatencyOption(decoderInfo.getName())) {
            LimeLog.info("Enabling AV1 RFI based on low latency option support");
            return true;
        }

        return false;
    }

    public static boolean decoderIsWhitelistedForHevc(MediaCodecInfo decoderInfo) {
        // Software decoders are terrible and we never want to use them
        if (decoderInfo.getName().contains("sw")) {
            LimeLog.info("Disallowing HEVC on software decoder: " + decoderInfo.getName());
            return false;
        } else if (!decoderInfo.isHardwareAccelerated() || decoderInfo.isSoftwareOnly()) {
            LimeLog.info("Disallowing HEVC on software decoder: " + decoderInfo.getName());
            return false;
        }

        // If this device is media performance class 12 or higher, we will assume any hardware
        // HEVC decoder present is fast and modern enough for streaming.
        //
        // [5.3/H-1-1] MUST NOT drop more than 2 frames in 10 seconds (i.e less than 0.333 percent frame drop) for a 1080p 60 fps video session under load.
        LimeLog.info("Media performance class: " + Build.VERSION.MEDIA_PERFORMANCE_CLASS);
        if (Build.VERSION.MEDIA_PERFORMANCE_CLASS >= Build.VERSION_CODES.S) {
            LimeLog.info("Allowing HEVC based on media performance class");
            return true;
        }

        // If the decoder supports FEATURE_LowLatency, we will assume it is fast and modern enough
        // to be preferable for streaming over H.264 decoders.
        if (decoderSupportsAndroidRLowLatency(decoderInfo, "video/hevc")) {
            LimeLog.info("Allowing HEVC based on FEATURE_LowLatency support");
            return true;
        }

        // Otherwise, we use our list of known working HEVC decoders
        return isDecoderInList(whitelistedHevcDecoders, decoderInfo.getName());
    }

    public static boolean isDecoderWhitelistedForAv1(MediaCodecInfo decoderInfo) {
        // Software decoders are terrible and we never want to use them
        if (decoderInfo.getName().contains("sw")) {
            LimeLog.info("Disallowing AV1 on software decoder: " + decoderInfo.getName());
            return false;
        } else if (!decoderInfo.isHardwareAccelerated() || decoderInfo.isSoftwareOnly()) {
            LimeLog.info("Disallowing AV1 on software decoder: " + decoderInfo.getName());
            return false;
        }

        // Hardware-accelerated AV1 decoders are generally well-supported on modern devices
        // AV1 provides better compression efficiency than HEVC, so prefer it when available
        LimeLog.info("Whitelisting hardware AV1 decoder: " + decoderInfo.getName());
        return true;
    }

    @SuppressWarnings("deprecation")
    @SuppressLint("NewApi")
    private static LinkedList<MediaCodecInfo> getMediaCodecList() {
        LinkedList<MediaCodecInfo> infoList = new LinkedList<>();

        MediaCodecList mcl = new MediaCodecList(MediaCodecList.REGULAR_CODECS);
        Collections.addAll(infoList, mcl.getCodecInfos());

        return infoList;
    }

    @SuppressWarnings("RedundantThrows")
    public static String dumpDecoders() throws Exception {
        StringBuilder str = new StringBuilder();
        for (MediaCodecInfo codecInfo : getMediaCodecList()) {
            // Skip encoders
            if (codecInfo.isEncoder()) {
                continue;
            }

            str.append("Decoder: ").append(codecInfo.getName()).append("\n");
            for (String type : codecInfo.getSupportedTypes()) {
                str.append("\t").append(type).append("\n");
                CodecCapabilities caps = codecInfo.getCapabilitiesForType(type);

                for (CodecProfileLevel profile : caps.profileLevels) {
                    str.append("\t\t").append(profile.profile).append(" ").append(profile.level).append("\n");
                }
            }
        }
        return str.toString();
    }

    private static MediaCodecInfo findPreferredDecoder() {
        // This is a different algorithm than the other findXXXDecoder functions,
        // because we want to evaluate the decoders in our list's order
        // rather than MediaCodecList's order

        if (!initialized) {
            throw new IllegalStateException("MediaCodecHelper must be initialized before use");
        }

        for (String preferredDecoder : preferredDecoders) {
            for (MediaCodecInfo codecInfo : getMediaCodecList()) {
                // Skip encoders
                if (codecInfo.isEncoder()) {
                    continue;
                }

                // Check for preferred decoders
                if (preferredDecoder.equalsIgnoreCase(codecInfo.getName())) {
                    LimeLog.info("Preferred decoder choice is " + codecInfo.getName());
                    return codecInfo;
                }
            }
        }

        return null;
    }

    private static boolean isCodecBlacklisted(MediaCodecInfo codecInfo) {
        // Use the new isSoftwareOnly() function on Android Q
        if (!SHOULD_BYPASS_SOFTWARE_BLOCK && codecInfo.isSoftwareOnly()) {
            LimeLog.info("Skipping software-only decoder: " + codecInfo.getName());
            return true;
        }

        // Check for explicitly blacklisted decoders
        if (isDecoderInList(blacklistedDecoderPrefixes, codecInfo.getName())) {
            LimeLog.info("Skipping blacklisted decoder: " + codecInfo.getName());
            return true;
        }

        return false;
    }

    public static MediaCodecInfo findFirstDecoder(String mimeType) {
        for (MediaCodecInfo codecInfo : getMediaCodecList()) {
            // Skip encoders
            if (codecInfo.isEncoder()) {
                continue;
            }

            // Skip compatibility aliases on Q+
            if (codecInfo.isAlias()) {
                continue;
            }

            // Find a decoder that supports the specified video format
            for (String mime : codecInfo.getSupportedTypes()) {
                if (mime.equalsIgnoreCase(mimeType)) {
                    // Skip blacklisted codecs
                    if (isCodecBlacklisted(codecInfo)) {
                        continue;
                    }

                    LimeLog.info("First decoder choice is " + codecInfo.getName());
                    return codecInfo;
                }
            }
        }

        return null;
    }

    public static MediaCodecInfo findProbableSafeDecoder(String mimeType, int requiredProfile) {
        // First look for a preferred decoder by name
        MediaCodecInfo info = findPreferredDecoder();
        if (info != null) {
            return info;
        }

        // Now look for decoders we know are safe
        try {
            // If this function completes, it will determine if the decoder is safe
            return findKnownSafeDecoder(mimeType, requiredProfile);
        } catch (Exception e) {
            // Some buggy devices seem to throw exceptions
            // from getCapabilitiesForType() so we'll just assume
            // they're okay and go with the first one we find
            return findFirstDecoder(mimeType);
        }
    }

    // We declare this method as explicitly throwing Exception
    // since some bad decoders can throw IllegalArgumentExceptions unexpectedly
    // and we want to be sure all callers are handling this possibility
    @SuppressWarnings("RedundantThrows")
    private static MediaCodecInfo findKnownSafeDecoder(String mimeType, int requiredProfile) throws Exception {
        // Some devices have multiple C2 decoders. On some Qualcomm devices (like Pixel 4),
        // there are separate low latency decoders (like c2.qti.hevc.decoder.low_latency)
        // that advertise FEATURE_LowLatency while the standard ones (like c2.qti.hevc.decoder)
        // do not. The decoders with FEATURE_LowLatency support are usually listed after the
        // standard ones, so we do two passes to prefer decoders with low latency support.
        for (int i = 0; i < 2; i++) {
            for (MediaCodecInfo codecInfo : getMediaCodecList()) {
                // Skip encoders
                if (codecInfo.isEncoder()) {
                    continue;
                }

                // Skip compatibility aliases on Q+
                if (codecInfo.isAlias()) {
                    continue;
                }

                // Find a decoder that supports the requested video format
                for (String mime : codecInfo.getSupportedTypes()) {
                    if (mime.equalsIgnoreCase(mimeType)) {
                        LimeLog.info("Examining decoder capabilities of " + codecInfo.getName() + " (round " + (i + 1) + ")");

                        // Skip blacklisted codecs
                        if (isCodecBlacklisted(codecInfo)) {
                            continue;
                        }

                        CodecCapabilities caps = codecInfo.getCapabilitiesForType(mime);

                        if (i == 0 && !decoderSupportsAndroidRLowLatency(codecInfo, mime)) {
                            LimeLog.info("Skipping decoder that lacks FEATURE_LowLatency for round 1");
                            continue;
                        }

                        if (requiredProfile != -1) {
                            for (CodecProfileLevel profile : caps.profileLevels) {
                                if (profile.profile == requiredProfile) {
                                    LimeLog.info("Decoder " + codecInfo.getName() + " supports required profile");
                                    return codecInfo;
                                }
                            }

                            LimeLog.info("Decoder " + codecInfo.getName() + " does NOT support required profile");
                        } else {
                            return codecInfo;
                        }
                    }
                }
            }
        }

        return null;
    }

    public static String readCpuinfo() throws Exception {
        StringBuilder cpuInfo = new StringBuilder();
        try (final BufferedReader br = new BufferedReader(new FileReader(new File("/proc/cpuinfo")))) {
            for (; ; ) {
                int ch = br.read();
                if (ch == -1)
                    break;
                cpuInfo.append((char) ch);
            }

            return cpuInfo.toString();
        }
    }

    private static boolean stringContainsIgnoreCase(String string, String substring) {
        return string.toLowerCase(Locale.ENGLISH).contains(substring.toLowerCase(Locale.ENGLISH));
    }

    public static boolean isExynos4Device() {
        try {
            // Try reading CPU info too look for 
            String cpuInfo = readCpuinfo();

            // SMDK4xxx is Exynos 4 
            if (stringContainsIgnoreCase(cpuInfo, "SMDK4")) {
                LimeLog.info("Found SMDK4 in /proc/cpuinfo");
                return true;
            }

            // If we see "Exynos 4" also we'll count it
            if (stringContainsIgnoreCase(cpuInfo, "Exynos 4")) {
                LimeLog.info("Found Exynos 4 in /proc/cpuinfo");
                return true;
            }
        } catch (Exception e) {
            Log.e(TAG, "isExynos4Device: "+e.getMessage(), e);
        }

        try {
            File systemDir = new File("/sys/devices/system");
            File[] files = systemDir.listFiles();
            if (files != null) {
                for (File f : files) {
                    if (stringContainsIgnoreCase(f.getName(), "exynos4")) {
                        LimeLog.info("Found exynos4 in /sys/devices/system");
                        return true;
                    }
                }
            }
        } catch (Exception e) {
            Log.e(TAG, "isExynos4Device: "+e.getMessage(), e);
        }

        return false;
    }
}
