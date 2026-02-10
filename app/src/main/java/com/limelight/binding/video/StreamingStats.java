package com.limelight.binding.video;

import android.content.Context;

import com.limelight.R;

/**
 * Structured streaming performance statistics data class.
 * Contains all key metrics from the video stream in a type-safe format.
 */
public class StreamingStats {

    // Video stream info
    public final String resolution;
    public final float totalFps;
    public final String decoderName;

    // Frame rates
    public final float receivedFps;
    public final float renderedFps;

    // Network stats
    public final float networkDropPercentage;
    public final int networkLatencyMs;
    public final int networkLatencyVarianceMs;

    // Host processing latency (in 0.1ms units)
    public final float minHostLatencyMs;
    public final float maxHostLatencyMs;
    public final float avgHostLatencyMs;
    public final boolean hasHostLatency;

    // Decoding stats
    public final float decodeTimeMs;

    // Gamepad info
    public final int gamepadCount;
    public final int gamepadVibrationCount;

    public StreamingStats(String resolution, float totalFps, String decoderName,
                          float receivedFps, float renderedFps,
                          float networkDropPercentage, int networkLatencyMs, int networkLatencyVarianceMs,
                          float minHostLatencyMs, float maxHostLatencyMs, float avgHostLatencyMs, boolean hasHostLatency,
                          float decodeTimeMs, int gamepadCount, int gamepadVibrationCount) {
        this.resolution = resolution;
        this.totalFps = totalFps;
        this.decoderName = decoderName;
        this.receivedFps = receivedFps;
        this.renderedFps = renderedFps;
        this.networkDropPercentage = networkDropPercentage;
        this.networkLatencyMs = networkLatencyMs;
        this.networkLatencyVarianceMs = networkLatencyVarianceMs;
        this.minHostLatencyMs = minHostLatencyMs;
        this.maxHostLatencyMs = maxHostLatencyMs;
        this.avgHostLatencyMs = avgHostLatencyMs;
        this.hasHostLatency = hasHostLatency;
        this.decodeTimeMs = decodeTimeMs;
        this.gamepadCount = gamepadCount;
        this.gamepadVibrationCount = gamepadVibrationCount;
    }

    /**
     * Get a simplified one-line summary for notification display.
     * Format: "DecoderName | 1920x1080 60.0 FPS | RTT 5 ms | Dec 2.5 ms | 🎮 2(1)"
     */
    public String toNotificationText(String videoCodec) {
        StringBuilder sb = new StringBuilder();

        // Add video codec if available
        if (videoCodec != null && !videoCodec.isEmpty() && !videoCodec.equals("Unknown")) {
            sb.append(videoCodec).append(" | ");
        }

        // Resolution and FPS
        sb.append(resolution).append(" ").append(String.format("%.1f", totalFps)).append(" FPS");

        // Network latency
        sb.append(" | RTT ").append(networkLatencyMs).append(" ms");

        // Decode time
        sb.append(" | Dec ").append(String.format("%.1f", decodeTimeMs)).append(" ms");

        // Gamepad info: show count and vibration support
        if (gamepadCount > 0) {
            sb.append(" | 🎮 ").append(gamepadCount);
            if (gamepadVibrationCount > 0) {
                sb.append("(").append(gamepadVibrationCount).append("📳)");
            }
        }

        return sb.toString();
    }

    /**
     * Get full performance overlay text with all details.
     */
    public String toFullDisplayText(Context context) {
        StringBuilder sb = new StringBuilder();

        sb.append(context.getString(R.string.perf_overlay_streamdetails,
                resolution, totalFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_decoder, decoderName)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_incomingfps, receivedFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_renderingfps, renderedFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_netdrops, networkDropPercentage)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_netlatency,
                networkLatencyMs, networkLatencyVarianceMs)).append('\n');

        if (hasHostLatency) {
            sb.append(context.getString(R.string.perf_overlay_hostprocessinglatency,
                    minHostLatencyMs, maxHostLatencyMs, avgHostLatencyMs)).append('\n');
        }

        sb.append(context.getString(R.string.perf_overlay_dectime, decodeTimeMs));
        return sb.toString();
    }

    /**
     * Builder class for creating StreamingStats instances.
     */
    public static class Builder {
        private String resolution = "";
        private float totalFps;
        private String decoderName = "";
        private float receivedFps;
        private float renderedFps;
        private float networkDropPercentage;
        private int networkLatencyMs;
        private int networkLatencyVarianceMs;
        private float minHostLatencyMs;
        private float maxHostLatencyMs;
        private float avgHostLatencyMs;
        private boolean hasHostLatency;
        private float decodeTimeMs;
        private int gamepadCount;
        private int gamepadVibrationCount;

        public Builder setResolution(int width, int height) {
            this.resolution = width + "x" + height;
            return this;
        }

        public Builder setFps(VideoStats.Fps fps) {
            this.totalFps = fps.totalFps;
            this.receivedFps = fps.receivedFps;
            this.renderedFps = fps.renderedFps;
            return this;
        }

        public Builder setDecoderName(String decoderName) {
            this.decoderName = decoderName;
            return this;
        }

        public Builder setNetworkStats(int totalFrames, int framesLost, long rttInfo) {
            if (totalFrames > 0) {
                this.networkDropPercentage = (float) framesLost / totalFrames * 100;
            }
            this.networkLatencyMs = (int) (rttInfo >> 32);
            this.networkLatencyVarianceMs = (int) rttInfo;
            return this;
        }

        public Builder setHostProcessingLatency(char minLatency, char maxLatency,
                                                 int totalLatency, int framesWithLatency) {
            this.hasHostLatency = framesWithLatency > 0;
            if (hasHostLatency) {
                this.minHostLatencyMs = (float) minLatency / 10;
                this.maxHostLatencyMs = (float) maxLatency / 10;
                this.avgHostLatencyMs = (float) totalLatency / 10 / framesWithLatency;
            }
            return this;
        }

        public Builder setDecodeTimeMs(long decoderTimeMs, int totalFramesReceived) {
            if (totalFramesReceived > 0) {
                this.decodeTimeMs = (float) decoderTimeMs / totalFramesReceived;
            }
            return this;
        }

        public Builder setGamepadInfo(int gamepadCount, int gamepadVibrationCount) {
            this.gamepadCount = gamepadCount;
            this.gamepadVibrationCount = gamepadVibrationCount;
            return this;
        }

        public StreamingStats build() {
            return new StreamingStats(
                    resolution, totalFps, decoderName,
                    receivedFps, renderedFps,
                    networkDropPercentage, networkLatencyMs, networkLatencyVarianceMs,
                    minHostLatencyMs, maxHostLatencyMs, avgHostLatencyMs, hasHostLatency,
                    decodeTimeMs, gamepadCount, gamepadVibrationCount
            );
        }
    }
}

