package com.limelight.binding.video.decoder;

import android.content.Context;
import android.os.SystemClock;

import com.limelight.binding.input.ControllerHandler;
import com.limelight.binding.video.PerfOverlayListener;
import com.limelight.binding.video.StreamingStats;
import com.limelight.binding.video.VideoStats;
import com.limelight.nvstream.jni.MoonBridge;
import com.limelight.preferences.PreferenceConfiguration;

/**
 * Manages video performance statistics collection and reporting.
 * <p>
 * This class handles:
 * - Frame statistics (received, rendered, lost)
 * - Decoder timing statistics
 * - Host processing latency tracking
 * - Performance overlay text generation
 */
public class PerformanceStatsManager {

    private final VideoStats activeWindowVideoStats;
    private final VideoStats lastWindowVideoStats;
    private final VideoStats globalVideoStats;

    private final PreferenceConfiguration prefs;
    private final PerfOverlayListener perfListener;
    private final Context context;

    private int lastFrameNumber;
    private int initialWidth;
    private int initialHeight;

    public PerformanceStatsManager(Context context, PreferenceConfiguration prefs,
                                    PerfOverlayListener perfListener) {
        this.context = context;
        this.prefs = prefs;
        this.perfListener = perfListener;

        this.activeWindowVideoStats = new VideoStats();
        this.lastWindowVideoStats = new VideoStats();
        this.globalVideoStats = new VideoStats();
    }

    public void setVideoDimensions(int width, int height) {
        this.initialWidth = width;
        this.initialHeight = height;
    }

    public VideoStats getActiveWindowVideoStats() {
        return activeWindowVideoStats;
    }

    public VideoStats getGlobalVideoStats() {
        return globalVideoStats;
    }

    /**
     * Update frame statistics when receiving a new frame.
     * @return true if this is an IDR frame with a new frame number
     */
    public boolean updateFrameStats(int frameNumber, int frameType) {
        boolean isNewIdrFrame = false;

        if (lastFrameNumber == 0) {
            activeWindowVideoStats.measurementStartTimestamp = SystemClock.uptimeMillis();
        } else if (frameNumber != lastFrameNumber && frameNumber != lastFrameNumber + 1) {
            activeWindowVideoStats.framesLost += frameNumber - lastFrameNumber - 1;
            activeWindowVideoStats.totalFrames += frameNumber - lastFrameNumber - 1;
            activeWindowVideoStats.frameLossEvents++;
        }

        if (lastFrameNumber != frameNumber && frameType == MoonBridge.FRAME_TYPE_IDR) {
            isNewIdrFrame = true;
        }

        lastFrameNumber = frameNumber;
        return isNewIdrFrame;
    }

    /**
     * Update performance overlay if the stats window has elapsed.
     */
    public void updatePerformanceOverlay(String activeDecoderName) {
        if (SystemClock.uptimeMillis() >= activeWindowVideoStats.measurementStartTimestamp + 1000) {
            if (prefs.enablePerfOverlay || prefs.enableStatsNotification) {
                StreamingStats stats = buildStreamingStats(activeDecoderName);
                perfListener.onPerfUpdate(stats);
            }

            globalVideoStats.add(activeWindowVideoStats);
            lastWindowVideoStats.copy(activeWindowVideoStats);
            activeWindowVideoStats.clear();
            activeWindowVideoStats.measurementStartTimestamp = SystemClock.uptimeMillis();
        }
    }

    /**
     * Update host processing latency statistics.
     */
    public void updateHostProcessingLatency(char frameHostProcessingLatency) {
        if (frameHostProcessingLatency != 0) {
            if (activeWindowVideoStats.minHostProcessingLatency != 0) {
                activeWindowVideoStats.minHostProcessingLatency = (char) Math.min(
                        activeWindowVideoStats.minHostProcessingLatency, frameHostProcessingLatency);
            } else {
                activeWindowVideoStats.minHostProcessingLatency = frameHostProcessingLatency;
            }
            activeWindowVideoStats.framesWithHostProcessingLatency += 1;
        }
        activeWindowVideoStats.maxHostProcessingLatency = (char) Math.max(
                activeWindowVideoStats.maxHostProcessingLatency, frameHostProcessingLatency);
        activeWindowVideoStats.totalHostProcessingLatency += frameHostProcessingLatency;
    }


    private StreamingStats buildStreamingStats(String activeDecoderName) {
        VideoStats lastTwo = new VideoStats();
        lastTwo.add(lastWindowVideoStats);
        lastTwo.add(activeWindowVideoStats);

        long rttInfo = MoonBridge.getEstimatedRttInfo();

        // Get gamepad connection info
        ControllerHandler.GamepadInfo gamepadInfo = ControllerHandler.getGamepadInfo(context);

        return new StreamingStats.Builder()
                .setResolution(initialWidth, initialHeight)
                .setFps(lastTwo.getFps())
                .setDecoderName(activeDecoderName)
                .setNetworkStats(lastTwo.totalFrames, lastTwo.framesLost, rttInfo)
                .setHostProcessingLatency(
                        lastTwo.minHostProcessingLatency,
                        lastTwo.maxHostProcessingLatency,
                        lastTwo.totalHostProcessingLatency,
                        lastTwo.framesWithHostProcessingLatency)
                .setDecodeTimeMs(lastTwo.decoderTimeMs, lastTwo.totalFramesReceived)
                .setGamepadInfo(gamepadInfo.totalCount, gamepadInfo.vibrationSupportCount)
                .build();
    }

    public int getAverageEndToEndLatency() {
        if (globalVideoStats.totalFramesReceived == 0) {
            return 0;
        }
        return (int) (globalVideoStats.totalTimeMs / globalVideoStats.totalFramesReceived);
    }

    public int getAverageDecoderLatency() {
        if (globalVideoStats.totalFramesReceived == 0) {
            return 0;
        }
        return (int) (globalVideoStats.decoderTimeMs / globalVideoStats.totalFramesReceived);
    }
}


