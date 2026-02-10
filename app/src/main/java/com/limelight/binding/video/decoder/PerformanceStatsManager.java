package com.limelight.binding.video.decoder;

import android.content.Context;
import android.os.SystemClock;

import com.limelight.R;
import com.limelight.binding.video.PerfOverlayListener;
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

    private final Context context;
    private final PreferenceConfiguration prefs;
    private final PerfOverlayListener perfListener;

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
                String perfText = buildPerformanceStatsString(activeDecoderName);
                perfListener.onPerfUpdate(perfText);
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


    private String buildPerformanceStatsString(String activeDecoderName) {
        VideoStats lastTwo = new VideoStats();
        lastTwo.add(lastWindowVideoStats);
        lastTwo.add(activeWindowVideoStats);
        VideoStats.Fps fps = lastTwo.getFps();

        float decodeTimeMs = (float) lastTwo.decoderTimeMs / lastTwo.totalFramesReceived;
        long rttInfo = MoonBridge.getEstimatedRttInfo();

        StringBuilder sb = new StringBuilder();
        sb.append(context.getString(R.string.perf_overlay_streamdetails,
                initialWidth + "x" + initialHeight, fps.totalFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_decoder, activeDecoderName)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_incomingfps, fps.receivedFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_renderingfps, fps.renderedFps)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_netdrops,
                (float) lastTwo.framesLost / lastTwo.totalFrames * 100)).append('\n');
        sb.append(context.getString(R.string.perf_overlay_netlatency,
                (int) (rttInfo >> 32), (int) rttInfo)).append('\n');

        if (lastTwo.framesWithHostProcessingLatency > 0) {
            sb.append(context.getString(R.string.perf_overlay_hostprocessinglatency,
                    (float) lastTwo.minHostProcessingLatency / 10,
                    (float) lastTwo.maxHostProcessingLatency / 10,
                    (float) lastTwo.totalHostProcessingLatency / 10 / lastTwo.framesWithHostProcessingLatency)).append('\n');
        }

        sb.append(context.getString(R.string.perf_overlay_dectime, decodeTimeMs));
        return sb.toString();
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


