package com.limelight.binding.video.decoder;

import android.media.MediaCodecInfo;
import android.util.Range;

import com.limelight.LimeLog;
import com.limelight.binding.video.MediaCodecHelper;
import com.limelight.preferences.PreferenceConfiguration;

import java.util.List;

/**
 * Handles discovery and capability checking of video decoders.
 * <p>
 * This class is responsible for:
 * - Finding available AVC (H.264), HEVC (H.265), and AV1 decoders
 * - Checking if decoders can meet specified performance requirements
 * - Determining reference frame invalidation support
 * - Calculating optimal slices per frame
 */
public class DecoderCapabilityChecker {

    private final MediaCodecInfo avcDecoder;
    private final MediaCodecInfo hevcDecoder;
    private final MediaCodecInfo av1Decoder;

    private final boolean refFrameInvalidationAvc;
    private final boolean refFrameInvalidationHevc;
    private final boolean refFrameInvalidationAv1;
    private final boolean directSubmit;
    private final byte optimalSlicesPerFrame;

    public DecoderCapabilityChecker(PreferenceConfiguration prefs, boolean requestedHdr, int consecutiveCrashCount) {
        // Find AVC decoder
        avcDecoder = findAvcDecoder();
        if (avcDecoder != null) {
            LimeLog.info("Selected AVC decoder: " + avcDecoder.getName());
        } else {
            LimeLog.warning("No AVC decoder found");
        }

        // Find HEVC decoder
        hevcDecoder = findHevcDecoder(prefs, requestedHdr);
        if (hevcDecoder != null) {
            LimeLog.info("Selected HEVC decoder: " + hevcDecoder.getName());
        } else {
            LimeLog.info("No HEVC decoder found");
        }

        // Find AV1 decoder
        av1Decoder = findAv1Decoder(prefs);
        if (av1Decoder != null) {
            LimeLog.info("Selected AV1 decoder: " + av1Decoder.getName());
        } else {
            LimeLog.info("No AV1 decoder found");
        }

        // Initialize decoder capabilities
        int avcOptimalSlicesPerFrame = 0;
        int hevcOptimalSlicesPerFrame = 0;
        boolean tempDirectSubmit = false;
        boolean tempRefFrameInvalidationAvc = false;
        boolean tempRefFrameInvalidationHevc = false;
        boolean tempRefFrameInvalidationAv1 = false;

        if (avcDecoder != null) {
            tempDirectSubmit = MediaCodecHelper.decoderCanDirectSubmit(avcDecoder.getName());
            tempRefFrameInvalidationAvc = MediaCodecHelper.decoderSupportsRefFrameInvalidationAvc(avcDecoder.getName(), prefs.height);
            avcOptimalSlicesPerFrame = MediaCodecHelper.getDecoderOptimalSlicesPerFrame(avcDecoder.getName());

            if (tempDirectSubmit) {
                LimeLog.info("Decoder " + avcDecoder.getName() + " will use direct submit");
            }
            if (tempRefFrameInvalidationAvc) {
                LimeLog.info("Decoder " + avcDecoder.getName() + " will use reference frame invalidation for AVC");
            }
            LimeLog.info("Decoder " + avcDecoder.getName() + " wants " + avcOptimalSlicesPerFrame + " slices per frame");
        }

        if (hevcDecoder != null) {
            tempRefFrameInvalidationHevc = MediaCodecHelper.decoderSupportsRefFrameInvalidationHevc(hevcDecoder);
            hevcOptimalSlicesPerFrame = MediaCodecHelper.getDecoderOptimalSlicesPerFrame(hevcDecoder.getName());

            if (tempRefFrameInvalidationHevc) {
                LimeLog.info("Decoder " + hevcDecoder.getName() + " will use reference frame invalidation for HEVC");
            }
            LimeLog.info("Decoder " + hevcDecoder.getName() + " wants " + hevcOptimalSlicesPerFrame + " slices per frame");
        }

        if (av1Decoder != null) {
            tempRefFrameInvalidationAv1 = MediaCodecHelper.decoderSupportsRefFrameInvalidationAv1(av1Decoder);

            if (tempRefFrameInvalidationAv1) {
                LimeLog.info("Decoder " + av1Decoder.getName() + " will use reference frame invalidation for AV1");
            }
        }

        // Disable RFI if we've had consecutive crashes (odd crash count triggers this)
        if (consecutiveCrashCount % 2 == 1) {
            tempRefFrameInvalidationAvc = false;
            tempRefFrameInvalidationHevc = false;
            LimeLog.warning("Disabling RFI due to previous crash");
        }

        this.directSubmit = tempDirectSubmit;
        this.refFrameInvalidationAvc = tempRefFrameInvalidationAvc;
        this.refFrameInvalidationHevc = tempRefFrameInvalidationHevc;
        this.refFrameInvalidationAv1 = tempRefFrameInvalidationAv1;
        this.optimalSlicesPerFrame = (byte) Math.max(avcOptimalSlicesPerFrame, hevcOptimalSlicesPerFrame);

        LimeLog.info("Requesting " + optimalSlicesPerFrame + " slices per frame");
    }

    private MediaCodecInfo findAvcDecoder() {
        MediaCodecInfo decoder = MediaCodecHelper.findProbableSafeDecoder("video/avc", MediaCodecInfo.CodecProfileLevel.AVCProfileHigh);
        if (decoder == null) {
            decoder = MediaCodecHelper.findFirstDecoder("video/avc");
        }
        return decoder;
    }

    private MediaCodecInfo findHevcDecoder(PreferenceConfiguration prefs, boolean requestedHdr) {
        // Don't return anything if H.264 is forced
        if (prefs.videoFormat == PreferenceConfiguration.FormatOption.FORCE_H264) {
            return null;
        }

        MediaCodecInfo hevcDecoderInfo = MediaCodecHelper.findProbableSafeDecoder("video/hevc", -1);
        if (hevcDecoderInfo != null) {
            if (!MediaCodecHelper.decoderIsWhitelistedForHevc(hevcDecoderInfo)) {
                LimeLog.info("Found HEVC decoder, but it's not whitelisted - " + hevcDecoderInfo.getName());

                // Force HEVC enabled if the user asked for it
                if (prefs.videoFormat == PreferenceConfiguration.FormatOption.FORCE_HEVC) {
                    LimeLog.info("Forcing HEVC enabled despite non-whitelisted decoder");
                }
                // HDR implies HEVC forced on
                else if (requestedHdr) {
                    LimeLog.info("Forcing HEVC enabled for HDR streaming");
                }
                // > 4K streaming requires HEVC
                else if (prefs.width > 4096 || prefs.height > 4096) {
                    LimeLog.info("Forcing HEVC enabled for over 4K streaming");
                }
                // Use HEVC if AVC decoder can't meet performance point
                else if (avcDecoder != null && decoderCanMeetPerformancePointWithHevcAndNotAvc(hevcDecoderInfo, avcDecoder, prefs)) {
                    LimeLog.info("Using non-whitelisted HEVC decoder to meet performance point");
                } else {
                    return null;
                }
            }
        }

        return hevcDecoderInfo;
    }

    private MediaCodecInfo findAv1Decoder(PreferenceConfiguration prefs) {
        // Don't use AV1 if H.264 or HEVC is explicitly forced
        if (prefs.videoFormat == PreferenceConfiguration.FormatOption.FORCE_H264 ||
            prefs.videoFormat == PreferenceConfiguration.FormatOption.FORCE_HEVC) {
            return null;
        }

        MediaCodecInfo decoderInfo = MediaCodecHelper.findProbableSafeDecoder("video/av01", -1);
        if (decoderInfo != null) {
            if (!MediaCodecHelper.isDecoderWhitelistedForAv1(decoderInfo)) {
                LimeLog.info("Found AV1 decoder, but it's not whitelisted - " + decoderInfo.getName());

                // Force AV1 enabled if the user asked for it
                if (prefs.videoFormat == PreferenceConfiguration.FormatOption.FORCE_AV1) {
                    LimeLog.info("Forcing AV1 enabled despite non-whitelisted decoder");
                }
                // Use AV1 if HEVC decoder can't meet performance point
                else if (hevcDecoder != null && decoderCanMeetPerformancePointWithAv1AndNotHevc(decoderInfo, hevcDecoder, prefs)) {
                    LimeLog.info("Using non-whitelisted AV1 decoder to meet performance point");
                }
                // Use AV1 if AVC decoder can't meet performance point and no HEVC decoder
                else if (hevcDecoder == null && avcDecoder != null && decoderCanMeetPerformancePointWithAv1AndNotAvc(decoderInfo, avcDecoder, prefs)) {
                    LimeLog.info("Using non-whitelisted AV1 decoder to meet performance point");
                } else {
                    return null;
                }
            } else {
                LimeLog.info("Using whitelisted AV1 decoder: " + decoderInfo.getName());
            }
        }

        return decoderInfo;
    }

    /**
     * Check if a decoder can meet the specified performance point.
     */
    private boolean decoderCanMeetPerformancePoint(MediaCodecInfo.VideoCapabilities caps, PreferenceConfiguration prefs) {
        MediaCodecInfo.VideoCapabilities.PerformancePoint targetPerfPoint =
                new MediaCodecInfo.VideoCapabilities.PerformancePoint(prefs.width, prefs.height, prefs.fps);

        List<MediaCodecInfo.VideoCapabilities.PerformancePoint> perfPoints = caps.getSupportedPerformancePoints();
        if (perfPoints != null) {
            for (MediaCodecInfo.VideoCapabilities.PerformancePoint perfPoint : perfPoints) {
                if (perfPoint.covers(targetPerfPoint)) {
                    return true;
                }
            }
            return false;
        }

        // Try Android M API
        try {
            Range<Double> fpsRange = caps.getAchievableFrameRatesFor(prefs.width, prefs.height);
            if (fpsRange != null) {
                return prefs.fps <= fpsRange.getUpper();
            }
        } catch (IllegalArgumentException e) {
            return false;
        }

        // Last resort: areSizeAndRateSupported()
        return caps.areSizeAndRateSupported(prefs.width, prefs.height, prefs.fps);
    }

    private boolean decoderCanMeetPerformancePointWithHevcAndNotAvc(
            MediaCodecInfo hevcDecoderInfo, MediaCodecInfo avcDecoderInfo, PreferenceConfiguration prefs) {
        MediaCodecInfo.VideoCapabilities avcCaps = avcDecoderInfo.getCapabilitiesForType("video/avc").getVideoCapabilities();
        MediaCodecInfo.VideoCapabilities hevcCaps = hevcDecoderInfo.getCapabilitiesForType("video/hevc").getVideoCapabilities();
        return !decoderCanMeetPerformancePoint(avcCaps, prefs) && decoderCanMeetPerformancePoint(hevcCaps, prefs);
    }

    private boolean decoderCanMeetPerformancePointWithAv1AndNotHevc(
            MediaCodecInfo av1DecoderInfo, MediaCodecInfo hevcDecoderInfo, PreferenceConfiguration prefs) {
        MediaCodecInfo.VideoCapabilities av1Caps = av1DecoderInfo.getCapabilitiesForType("video/av01").getVideoCapabilities();
        MediaCodecInfo.VideoCapabilities hevcCaps = hevcDecoderInfo.getCapabilitiesForType("video/hevc").getVideoCapabilities();
        return !decoderCanMeetPerformancePoint(hevcCaps, prefs) && decoderCanMeetPerformancePoint(av1Caps, prefs);
    }

    private boolean decoderCanMeetPerformancePointWithAv1AndNotAvc(
            MediaCodecInfo av1DecoderInfo, MediaCodecInfo avcDecoderInfo, PreferenceConfiguration prefs) {
        MediaCodecInfo.VideoCapabilities avcCaps = avcDecoderInfo.getCapabilitiesForType("video/avc").getVideoCapabilities();
        MediaCodecInfo.VideoCapabilities av1Caps = av1DecoderInfo.getCapabilitiesForType("video/av01").getVideoCapabilities();
        return !decoderCanMeetPerformancePoint(avcCaps, prefs) && decoderCanMeetPerformancePoint(av1Caps, prefs);
    }

    // Getters

    public MediaCodecInfo getAvcDecoder() {
        return avcDecoder;
    }

    public MediaCodecInfo getHevcDecoder() {
        return hevcDecoder;
    }

    public MediaCodecInfo getAv1Decoder() {
        return av1Decoder;
    }

    public boolean isAvcSupported() {
        return avcDecoder != null;
    }

    public boolean isHevcSupported() {
        return hevcDecoder != null;
    }

    public boolean isAv1Supported() {
        return av1Decoder != null;
    }

    public boolean isHevcMain10Hdr10Supported() {
        if (hevcDecoder == null) {
            return false;
        }

        for (MediaCodecInfo.CodecProfileLevel profileLevel : hevcDecoder.getCapabilitiesForType("video/hevc").profileLevels) {
            if (profileLevel.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10) {
                LimeLog.info("HEVC decoder " + hevcDecoder.getName() + " supports HEVC Main10 HDR10");
                return true;
            }
        }

        return false;
    }

    public boolean isAv1Main10Supported() {
        if (av1Decoder == null) {
            return false;
        }

        for (MediaCodecInfo.CodecProfileLevel profileLevel : av1Decoder.getCapabilitiesForType("video/av01").profileLevels) {
            if (profileLevel.profile == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10HDR10) {
                LimeLog.info("AV1 decoder " + av1Decoder.getName() + " supports AV1 Main 10 HDR10");
                return true;
            }
        }

        return false;
    }

    public boolean isRefFrameInvalidationAvc() {
        return refFrameInvalidationAvc;
    }

    public boolean isRefFrameInvalidationHevc() {
        return refFrameInvalidationHevc;
    }

    public boolean isRefFrameInvalidationAv1() {
        return refFrameInvalidationAv1;
    }

    public boolean isDirectSubmit() {
        return directSubmit;
    }

    public byte getOptimalSlicesPerFrame() {
        return optimalSlicesPerFrame;
    }
}

