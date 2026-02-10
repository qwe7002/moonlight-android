package com.limelight.binding.video.decoder;

import android.media.MediaCodec.CodecException;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;
import android.os.Build;
import android.util.Range;

import com.limelight.BuildConfig;

import java.util.Objects;

/**
 * Custom exceptions for video decoder error handling and diagnostics.
 */
public class DecoderExceptions {

    /**
     * Exception thrown when the decoder appears to be hung.
     */
    @SuppressWarnings("NullableProblems")
    public static class DecoderHungException extends RuntimeException {
        private final int hangTimeMs;

        public DecoderHungException(int hangTimeMs) {
            this.hangTimeMs = hangTimeMs;
        }

        public int getHangTimeMs() {
            return hangTimeMs;
        }

        @Override
        public String toString() {
            return "Hang time: " + hangTimeMs + " ms" + DELIMITER + super.toString();
        }

        private static final String DELIMITER = BuildConfig.DEBUG ? "\n" : " | ";
    }

    /**
     * Exception with detailed renderer state information for debugging.
     */
    @SuppressWarnings("NullableProblems")
    public static class RendererException extends RuntimeException {
        private static final long serialVersionUID = 8985937536997012406L;
        public static final String DELIMITER = BuildConfig.DEBUG ? "\n" : " | ";

        private final String text;

        public RendererException(RendererState state, Exception e) {
            this.text = generateText(state, e);
        }

        @Override
        public String toString() {
            return text;
        }

        private String generateText(RendererState state, Exception originalException) {
            StringBuilder str = new StringBuilder();

            // Error phase
            if (state.numVpsIn == 0 && state.numSpsIn == 0 && state.numPpsIn == 0) {
                str.append("PreSPSError");
            } else if (state.numSpsIn > 0 && state.numPpsIn == 0) {
                str.append("PrePPSError");
            } else if (state.numPpsIn > 0 && state.numFramesIn == 0) {
                str.append("PreIFrameError");
            } else if (state.numFramesIn > 0 && state.outputFormat == null) {
                str.append("PreOutputConfigError");
            } else if (state.outputFormat != null && state.numFramesOut == 0) {
                str.append("PreOutputError");
            } else if (state.numFramesOut <= state.refreshRate * 30) {
                str.append("EarlyOutputError");
            } else {
                str.append("ErrorWhileStreaming");
            }

            str.append(DELIMITER);
            str.append("Format: ").append(String.format("%x", state.videoFormat)).append(DELIMITER);
            str.append("AVC Decoder: ").append(state.avcDecoderName != null ? state.avcDecoderName : "(none)").append(DELIMITER);
            str.append("HEVC Decoder: ").append(state.hevcDecoderName != null ? state.hevcDecoderName : "(none)").append(DELIMITER);
            str.append("AV1 Decoder: ").append(state.av1DecoderName != null ? state.av1DecoderName : "(none)").append(DELIMITER);

            if (state.avcDecoder != null) {
                appendDecoderCapabilities(str, state.avcDecoder, "video/avc", "AVC", state.initialWidth, state.initialHeight);
            }
            if (state.hevcDecoder != null) {
                appendDecoderCapabilities(str, state.hevcDecoder, "video/hevc", "HEVC", state.initialWidth, state.initialHeight);
            }
            if (state.av1Decoder != null) {
                appendDecoderCapabilities(str, state.av1Decoder, "video/av01", "AV1", state.initialWidth, state.initialHeight);
            }

            str.append("Configured format: ").append(state.configuredFormat).append(DELIMITER);
            str.append("Input format: ").append(state.inputFormat).append(DELIMITER);
            str.append("Output format: ").append(state.outputFormat).append(DELIMITER);
            str.append("Adaptive playback: ").append(state.adaptivePlayback).append(DELIMITER);
            str.append("GL Renderer: ").append(state.glRenderer).append(DELIMITER);
            str.append("SOC: ").append(Build.SOC_MANUFACTURER).append(" - ").append(Build.SOC_MODEL).append(DELIMITER);
            str.append("Performance class: ").append(Build.VERSION.MEDIA_PERFORMANCE_CLASS).append(DELIMITER);

            str.append("Consecutive crashes: ").append(state.consecutiveCrashCount).append(DELIMITER);
            str.append("RFI active: ").append(state.refFrameInvalidationActive).append(DELIMITER);
            str.append("Using modern SPS patching: true").append(DELIMITER);
            str.append("Fused IDR frames: ").append(state.fusedIdrFrame).append(DELIMITER);
            str.append("Video dimensions: ").append(state.initialWidth).append("x").append(state.initialHeight).append(DELIMITER);
            str.append("FPS target: ").append(state.refreshRate).append(DELIMITER);
            str.append("Bitrate: ").append(state.bitrate).append(" Kbps").append(DELIMITER);
            str.append("CSD stats: ").append(state.numVpsIn).append(", ").append(state.numSpsIn).append(", ").append(state.numPpsIn).append(DELIMITER);
            str.append("Frames in-out: ").append(state.numFramesIn).append(", ").append(state.numFramesOut).append(DELIMITER);
            str.append("Total frames received: ").append(state.totalFramesReceived).append(DELIMITER);
            str.append("Total frames rendered: ").append(state.totalFramesRendered).append(DELIMITER);
            str.append("Frame losses: ").append(state.framesLost).append(" in ").append(state.frameLossEvents).append(" loss events").append(DELIMITER);
            str.append("Average end-to-end client latency: ").append(state.avgEndToEndLatency).append("ms").append(DELIMITER);
            str.append("Average hardware decoder latency: ").append(state.avgDecoderLatency).append("ms").append(DELIMITER);
            str.append("Frame pacing mode: ").append(state.framePacing).append(DELIMITER);

            if (originalException instanceof CodecException) {
                CodecException ce = (CodecException) originalException;
                str.append("Diagnostic Info: ").append(ce.getDiagnosticInfo()).append(DELIMITER);
                str.append("Recoverable: ").append(ce.isRecoverable()).append(DELIMITER);
                str.append("Transient: ").append(ce.isTransient()).append(DELIMITER);
                str.append("Codec Error Code: ").append(ce.getErrorCode()).append(DELIMITER);
            }

            str.append(originalException.toString());

            return str.toString();
        }

        private void appendDecoderCapabilities(StringBuilder str, MediaCodecInfo decoder, String mimeType, String name, int width, int height) {
            Range<Integer> widthRange = Objects.requireNonNull(decoder.getCapabilitiesForType(mimeType).getVideoCapabilities()).getSupportedWidths();
            str.append(name).append(" supported width range: ").append(widthRange).append(DELIMITER);
            try {
                Range<Double> fpsRange = Objects.requireNonNull(decoder.getCapabilitiesForType(mimeType).getVideoCapabilities()).getAchievableFrameRatesFor(width, height);
                str.append(name).append(" achievable FPS range: ").append(fpsRange).append(DELIMITER);
            } catch (IllegalArgumentException e) {
                str.append(name).append(" achievable FPS range: UNSUPPORTED!").append(DELIMITER);
            }
        }
    }

    /**
     * State information for generating detailed exception reports.
     */
    public static class RendererState {
        public int videoFormat;
        public int initialWidth;
        public int initialHeight;
        public int refreshRate;
        public int bitrate;
        public int framePacing;
        public int consecutiveCrashCount;

        public MediaCodecInfo avcDecoder;
        public MediaCodecInfo hevcDecoder;
        public MediaCodecInfo av1Decoder;
        public String avcDecoderName;
        public String hevcDecoderName;
        public String av1DecoderName;

        public MediaFormat configuredFormat;
        public MediaFormat inputFormat;
        public MediaFormat outputFormat;

        public boolean adaptivePlayback;
        public boolean refFrameInvalidationActive;
        public boolean fusedIdrFrame;

        public String glRenderer;

        public int numVpsIn;
        public int numSpsIn;
        public int numPpsIn;
        public int numFramesIn;
        public int numFramesOut;

        public int totalFramesReceived;
        public int totalFramesRendered;
        public int framesLost;
        public int frameLossEvents;
        public int avgEndToEndLatency;
        public int avgDecoderLatency;
    }
}

