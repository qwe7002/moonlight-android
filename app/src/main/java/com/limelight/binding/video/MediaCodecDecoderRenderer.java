package com.limelight.binding.video;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Arrays;
import java.util.concurrent.LinkedBlockingQueue;

import com.limelight.LimeLog;
import com.limelight.binding.video.decoder.CodecRecoveryManager;
import com.limelight.binding.video.decoder.CsdBufferProcessor;
import com.limelight.binding.video.decoder.DecoderCapabilityChecker;
import com.limelight.binding.video.decoder.DecoderExceptions;
import com.limelight.binding.video.decoder.PerformanceStatsManager;
import com.limelight.nvstream.av.video.VideoDecoderRenderer;
import com.limelight.nvstream.jni.MoonBridge;
import com.limelight.preferences.PreferenceConfiguration;

import android.app.Activity;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;
import android.media.MediaCodec.BufferInfo;
import android.media.MediaCodec.CodecException;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Process;
import android.os.SystemClock;
import android.util.Log;
import android.view.Choreographer;
import android.view.SurfaceHolder;

@SuppressWarnings("deprecation")
public class MediaCodecDecoderRenderer extends VideoDecoderRenderer implements Choreographer.FrameCallback,
        CodecRecoveryManager.CodecRecoveryCallback {
    private static final String TAG = "MediaCodecDecoderRenderer";

    private static final boolean USE_FRAME_RENDER_TIME = false;
    private static final boolean FRAME_RENDER_TIME_ONLY = false;

    // Extracted components
    private final DecoderCapabilityChecker capabilityChecker;
    private final CsdBufferProcessor csdProcessor;
    private final PerformanceStatsManager statsManager;
    private final CodecRecoveryManager recoveryManager;

    private boolean submittedCsd;
    private byte[] currentHdrMetadata;

    private int nextInputBufferIndex = -1;
    private ByteBuffer nextInputBuffer;

    private final Activity activity;
    private MediaCodec videoDecoder;
    private Thread rendererThread;
    private boolean adaptivePlayback, fusedIdrFrame;
    private boolean refFrameInvalidationActive;
    private int initialWidth, initialHeight;
    private int videoFormat;
    private String activeDecoderName;
    private SurfaceHolder renderTarget;
    private volatile boolean stopping;
    private final CrashListener crashListener;
    private boolean reportedCrash;
    private final int consecutiveCrashCount;
    private final String glRenderer;

    private MediaFormat inputFormat;
    private MediaFormat outputFormat;
    private MediaFormat configuredFormat;

    private DecoderExceptions.RendererException initialException;
    private long initialExceptionTimestamp;
    private static final int EXCEPTION_REPORT_DELAY_MS = 3000;

    private long lastTimestampUs;
    private int refreshRate;
    private final PreferenceConfiguration prefs;

    private final LinkedBlockingQueue<Integer> outputBufferQueue = new LinkedBlockingQueue<>();
    private static final int OUTPUT_BUFFER_QUEUE_LIMIT = 2;
    private long lastRenderedFrameTimeNanos;
    private HandlerThread choreographerHandlerThread;
    private Handler choreographerHandler;

    private int numFramesIn;
    private int numFramesOut;


    public void setRenderTarget(SurfaceHolder renderTarget) {
        this.renderTarget = renderTarget;
    }

    public MediaCodecDecoderRenderer(Activity activity, PreferenceConfiguration prefs,
                                     CrashListener crashListener, int consecutiveCrashCount,
                                     boolean requestedHdr,
                                     String glRenderer, PerfOverlayListener perfListener) {
        this.activity = activity;
        this.prefs = prefs;
        this.crashListener = crashListener;
        this.consecutiveCrashCount = consecutiveCrashCount;
        this.glRenderer = glRenderer;

        // Initialize extracted components
        this.capabilityChecker = new DecoderCapabilityChecker(prefs, requestedHdr, consecutiveCrashCount);
        this.csdProcessor = new CsdBufferProcessor();
        this.statsManager = new PerformanceStatsManager(prefs, perfListener);
        this.recoveryManager = new CodecRecoveryManager(this);
    }

    public boolean isHevcSupported() {
        return capabilityChecker.isHevcSupported();
    }

    public boolean isAvcSupported() {
        return capabilityChecker.isAvcSupported();
    }

    public boolean isHevcMain10Hdr10Supported() {
        return capabilityChecker.isHevcMain10Hdr10Supported();
    }

    public boolean isAv1Supported() {
        return capabilityChecker.isAv1Supported();
    }

    public boolean isAv1Main10Supported() {
        return capabilityChecker.isAv1Main10Supported();
    }

    public int getPreferredColorSpace() {
        // Default to Rec 709 which is probably better supported on modern devices.
        //
        // We are sticking to Rec 601 on older devices unless the device has an HEVC decoder
        // to avoid possible regressions (and they are < 5% of installed devices). If we have
        // an HEVC decoder, we will use Rec 709 (even for H.264) since we can't choose a
        // colorspace by codec (and it's probably safe to say a SoC with HEVC decoding is
        // plenty modern enough to handle H.264 VUI colorspace info).
        return MoonBridge.COLORSPACE_REC_709;
    }

    public int getPreferredColorRange() {
        if (prefs.fullRange) {
            return MoonBridge.COLOR_RANGE_FULL;
        } else {
            return MoonBridge.COLOR_RANGE_LIMITED;
        }
    }

    public int getActiveVideoFormat() {
        return this.videoFormat;
    }

    public String getActiveDecoderName() {
        return this.activeDecoderName;
    }

    private MediaFormat createBaseMediaFormat(String mimeType) {
        MediaFormat videoFormat = MediaFormat.createVideoFormat(mimeType, initialWidth, initialHeight);

        // Avoid setting KEY_FRAME_RATE on Lollipop and earlier to reduce compatibility risk
        videoFormat.setInteger(MediaFormat.KEY_FRAME_RATE, refreshRate);

        // Populate keys for adaptive playback
        if (adaptivePlayback) {
            videoFormat.setInteger(MediaFormat.KEY_MAX_WIDTH, initialWidth);
            videoFormat.setInteger(MediaFormat.KEY_MAX_HEIGHT, initialHeight);
        }

        // Android 7.0 adds color options to the MediaFormat
        videoFormat.setInteger(MediaFormat.KEY_COLOR_RANGE,
                getPreferredColorRange() == MoonBridge.COLOR_RANGE_FULL ?
                        MediaFormat.COLOR_RANGE_FULL : MediaFormat.COLOR_RANGE_LIMITED);

        // If the stream is HDR-capable, the decoder will detect transitions in color standards
        // rather than us hardcoding them into the MediaFormat.
        if ((getActiveVideoFormat() & MoonBridge.VIDEO_FORMAT_MASK_10BIT) == 0) {
            // Set color format keys when not in HDR mode, since we know they won't change
            videoFormat.setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO);
            switch (getPreferredColorSpace()) {
                case MoonBridge.COLORSPACE_REC_601:
                    videoFormat.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT601_NTSC);
                    break;
                case MoonBridge.COLORSPACE_REC_709:
                    videoFormat.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709);
                    break;
                case MoonBridge.COLORSPACE_REC_2020:
                    videoFormat.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT2020);
                    break;
            }
        }

        return videoFormat;
    }

    private void configureAndStartDecoder(MediaFormat format) {
        // Set HDR metadata if present
        if (currentHdrMetadata != null) {
            ByteBuffer hdrStaticInfo = ByteBuffer.allocate(25).order(ByteOrder.LITTLE_ENDIAN);
            ByteBuffer hdrMetadata = ByteBuffer.wrap(currentHdrMetadata).order(ByteOrder.LITTLE_ENDIAN);

            // Create a HDMI Dynamic Range and Mastering InfoFrame as defined by CTA-861.3
            hdrStaticInfo.put((byte) 0); // Metadata type
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // RX
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // RY
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // GX
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // GY
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // BX
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // BY
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // White X
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // White Y
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // Max mastering luminance
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // Min mastering luminance
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // Max content luminance
            hdrStaticInfo.putShort(hdrMetadata.getShort()); // Max frame average luminance

            hdrStaticInfo.rewind();
            format.setByteBuffer(MediaFormat.KEY_HDR_STATIC_INFO, hdrStaticInfo);
        } else {
            format.removeKey(MediaFormat.KEY_HDR_STATIC_INFO);
        }

        LimeLog.info("Configuring with format: " + format);

        videoDecoder.configure(format, renderTarget.getSurface(), null, 0);

        configuredFormat = format;

        // After reconfiguration, we must resubmit CSD buffers
        submittedCsd = false;
        csdProcessor.clear();

        // This will contain the actual accepted input format attributes
        inputFormat = videoDecoder.getInputFormat();
        LimeLog.info("Input format: " + inputFormat);

        videoDecoder.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT);

        // Start the decoder
        videoDecoder.start();

    }

    private boolean tryConfigureDecoder(MediaCodecInfo selectedDecoderInfo, MediaFormat format, boolean throwOnCodecError) {
        boolean configured = false;
        try {
            videoDecoder = MediaCodec.createByCodecName(selectedDecoderInfo.getName());
            configureAndStartDecoder(format);
            LimeLog.info("Using codec " + selectedDecoderInfo.getName() + " for hardware decoding " + format.getString(MediaFormat.KEY_MIME));
            activeDecoderName = selectedDecoderInfo.getName();
            configured = true;
        } catch (IllegalArgumentException | IllegalStateException e) {
            Log.e(TAG, "tryConfigureDecoder: " + e.getMessage(), e);
            if (throwOnCodecError) {
                throw e;
            }
        } catch (IOException e) {
            Log.e(TAG, "tryConfigureDecoder: " + e.getMessage(), e);
            if (throwOnCodecError) {
                throw new RuntimeException(e);
            }
        } finally {
            if (!configured && videoDecoder != null) {
                videoDecoder.release();
                videoDecoder = null;
            }
        }
        return configured;
    }

    public int initializeDecoder(boolean throwOnCodecError) {
        String mimeType;
        MediaCodecInfo selectedDecoderInfo;

        if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_H264) != 0) {
            mimeType = "video/avc";
            selectedDecoderInfo = capabilityChecker.getAvcDecoder();

            if (selectedDecoderInfo == null) {
                LimeLog.severe("No available AVC decoder!");
                return -1;
            }

            if (initialWidth > 4096 || initialHeight > 4096) {
                LimeLog.severe("> 4K streaming only supported on HEVC");
                return -1;
            }

            // Initialize CSD processor with H.264 specific settings
            csdProcessor.initialize(selectedDecoderInfo.getName()
            );

            refFrameInvalidationActive = capabilityChecker.isRefFrameInvalidationAvc();
        } else if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_H265) != 0) {
            mimeType = "video/hevc";
            selectedDecoderInfo = capabilityChecker.getHevcDecoder();

            if (selectedDecoderInfo == null) {
                LimeLog.severe("No available HEVC decoder!");
                return -2;
            }

            refFrameInvalidationActive = capabilityChecker.isRefFrameInvalidationHevc();
        } else if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_AV1) != 0) {
            mimeType = "video/av01";
            selectedDecoderInfo = capabilityChecker.getAv1Decoder();

            if (selectedDecoderInfo == null) {
                LimeLog.severe("No available AV1 decoder!");
                return -2;
            }

            refFrameInvalidationActive = capabilityChecker.isRefFrameInvalidationAv1();
        } else {
            // Unknown format
            LimeLog.severe("Unknown format");
            return -3;
        }

        adaptivePlayback = MediaCodecHelper.decoderSupportsAdaptivePlayback(selectedDecoderInfo, mimeType);
        fusedIdrFrame = MediaCodecHelper.decoderSupportsFusedIdrFrame(selectedDecoderInfo, mimeType);

        // Set video dimensions for stats
        statsManager.setVideoDimensions(initialWidth, initialHeight);

        // Configure recovery manager
        recoveryManager.setHasChoreographerThread(prefs.framePacing == PreferenceConfiguration.FRAME_PACING_BALANCED);

        // Force low-latency decoder: Only try with all low-latency options enabled (tryNumber=0)
        // Do not allow fallback to non-low-latency options
        LimeLog.info("Decoder configuration: Forcing low-latency mode");

        MediaFormat mediaFormat = createBaseMediaFormat(mimeType);

        // Set all low latency options (tryNumber=0 means all options enabled)
        MediaCodecHelper.setDecoderLowLatencyOptions(mediaFormat, selectedDecoderInfo, 0);

        // Try to configure the decoder with low-latency options
        if (!tryConfigureDecoder(selectedDecoderInfo, mediaFormat, throwOnCodecError)) {
            LimeLog.severe("Low-latency decoder required but configuration failed. Aborting without fallback.");
            return -5;
        }

        if (USE_FRAME_RENDER_TIME) {
            videoDecoder.setOnFrameRenderedListener((mediaCodec, presentationTimeUs, renderTimeNanos) -> {
                long delta = (renderTimeNanos / 1000000L) - (presentationTimeUs / 1000);
                if (delta >= 0 && delta < 1000) {
                    if (USE_FRAME_RENDER_TIME) {
                        statsManager.getActiveWindowVideoStats().totalTimeMs += delta;
                    }
                }
            }, null);
        }

        return 0;
    }

    @Override
    public int setup(int format, int width, int height, int redrawRate) {
        this.initialWidth = width;
        this.initialHeight = height;
        this.videoFormat = format;
        this.refreshRate = redrawRate;

        return initializeDecoder(false);
    }

    // All threads that interact with the MediaCodec instance must call this function regularly!
    private boolean doCodecRecoveryIfRequired(int quiescenceFlag) {
        return recoveryManager.doRecoveryIfRequired(quiescenceFlag);
    }

    // CodecRecoveryManager.CodecRecoveryCallback implementation
    @Override
    public void onFlushDecoder() {
        videoDecoder.flush();
    }

    @Override
    public void onRestartDecoder() {
        videoDecoder.stop();
        configureAndStartDecoder(configuredFormat);
    }

    @Override
    public void onResetDecoder() {
        videoDecoder.reset();
        configureAndStartDecoder(configuredFormat);
    }

    @Override
    public boolean onRecreateDecoder() {
        videoDecoder.release();
        int err = initializeDecoder(true);
        return err == 0;
    }

    @Override
    public void onRecoveryFailed(Exception e) {
        if (!reportedCrash) {
            reportedCrash = true;
            crashListener.notifyCrash(e);
        }
        throw createRendererException(e);
    }

    @Override
    public void onClearBuffers() {
        nextInputBuffer = null;
        nextInputBufferIndex = -1;
        outputBufferQueue.clear();
    }

    // Returns true if the exception is transient
    private boolean handleDecoderException(IllegalStateException e) {
        // Eat decoder exceptions if we're in the process of stopping
        if (stopping) {
            return false;
        }

        if (e instanceof CodecException) {
            return handleCodecException((CodecException) e);
        } else {
            return handleIllegalStateException(e);
        }
    }

    private boolean handleCodecException(CodecException codecExc) {
        if (codecExc.isTransient()) {
            LimeLog.warning(codecExc.getDiagnosticInfo());
            return true;
        }

        LimeLog.severe(codecExc.getDiagnosticInfo());

        if (!recoveryManager.hasExceededMaxRecoveryAttempts()) {
            if (codecExc.isRecoverable()) {
                recoveryManager.scheduleRecoverableRecovery(codecExc);
            } else {
                recoveryManager.scheduleNonRecoverableRecovery(codecExc);
            }
            return false;
        }

        return handleExceptionAfterMaxRecoveryAttempts(codecExc);
    }

    private boolean handleIllegalStateException(IllegalStateException e) {
        if (!recoveryManager.hasExceededMaxRecoveryAttempts()) {
            recoveryManager.scheduleResetRecovery(e);
            return false;
        }

        return handleExceptionAfterMaxRecoveryAttempts(e);
    }

    private boolean handleExceptionAfterMaxRecoveryAttempts(IllegalStateException e) {
        if (recoveryManager.getRecoveryType() == CodecRecoveryManager.RECOVERY_TYPE_NONE) {
            handlePersistentException(e);
        }
        return false;
    }

    private void handlePersistentException(IllegalStateException e) {
        if (initialException != null) {
            if (SystemClock.uptimeMillis() - initialExceptionTimestamp >= EXCEPTION_REPORT_DELAY_MS) {
                if (!reportedCrash) {
                    reportedCrash = true;
                    crashListener.notifyCrash(initialException);
                }
                throw initialException;
            }
        } else {
            initialException = createRendererException(e);
            initialExceptionTimestamp = SystemClock.uptimeMillis();
        }
    }

    private DecoderExceptions.RendererException createRendererException(Exception e) {
        DecoderExceptions.RendererState state = new DecoderExceptions.RendererState();
        state.videoFormat = this.videoFormat;
        state.initialWidth = this.initialWidth;
        state.initialHeight = this.initialHeight;
        state.refreshRate = this.refreshRate;
        state.bitrate = this.prefs.bitrate;
        state.framePacing = this.prefs.framePacing;
        state.consecutiveCrashCount = this.consecutiveCrashCount;

        state.avcDecoder = capabilityChecker.getAvcDecoder();
        state.hevcDecoder = capabilityChecker.getHevcDecoder();
        state.av1Decoder = capabilityChecker.getAv1Decoder();
        state.avcDecoderName = state.avcDecoder != null ? state.avcDecoder.getName() : null;
        state.hevcDecoderName = state.hevcDecoder != null ? state.hevcDecoder.getName() : null;
        state.av1DecoderName = state.av1Decoder != null ? state.av1Decoder.getName() : null;

        state.configuredFormat = this.configuredFormat;
        state.inputFormat = this.inputFormat;
        state.outputFormat = this.outputFormat;

        state.adaptivePlayback = this.adaptivePlayback;
        state.refFrameInvalidationActive = this.refFrameInvalidationActive;
        state.fusedIdrFrame = this.fusedIdrFrame;
        state.glRenderer = this.glRenderer;

        state.numVpsIn = csdProcessor.getNumVpsIn();
        state.numSpsIn = csdProcessor.getNumSpsIn();
        state.numPpsIn = csdProcessor.getNumPpsIn();
        state.numFramesIn = this.numFramesIn;
        state.numFramesOut = this.numFramesOut;

        VideoStats globalStats = statsManager.getGlobalVideoStats();
        state.totalFramesReceived = globalStats.totalFramesReceived;
        state.totalFramesRendered = globalStats.totalFramesRendered;
        state.framesLost = globalStats.framesLost;
        state.frameLossEvents = globalStats.frameLossEvents;
        state.avgEndToEndLatency = statsManager.getAverageEndToEndLatency();
        state.avgDecoderLatency = statsManager.getAverageDecoderLatency();

        return new DecoderExceptions.RendererException(state, e);
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        // Do nothing if we're stopping
        if (stopping) {
            return;
        }

        frameTimeNanos -= activity.getWindowManager().getDefaultDisplay().getAppVsyncOffsetNanos();

        // Don't render unless a new frame is due. This prevents microstutter when streaming
        // at a frame rate that doesn't match the display (such as 60 FPS on 120 Hz).
        long actualFrameTimeDeltaNs = frameTimeNanos - lastRenderedFrameTimeNanos;
        long expectedFrameTimeDeltaNs = 800000000 / refreshRate; // within 80% of the next frame
        if (actualFrameTimeDeltaNs >= expectedFrameTimeDeltaNs) {
            // Render up to one frame when in frame pacing mode.
            //
            // NB: Since the queue limit is 2, we won't starve the decoder of output buffers
            // by holding onto them for too long. This also ensures we will have that 1 extra
            // frame of buffer to smooth over network/rendering jitter.
            Integer nextOutputBuffer = outputBufferQueue.poll();
            if (nextOutputBuffer != null) {
                try {
                    videoDecoder.releaseOutputBuffer(nextOutputBuffer, frameTimeNanos);

                    lastRenderedFrameTimeNanos = frameTimeNanos;
                    statsManager.getActiveWindowVideoStats().totalFramesRendered++;
                } catch (IllegalStateException ignored) {
                    try {
                        // Try to avoid leaking the output buffer by releasing it without rendering
                        videoDecoder.releaseOutputBuffer(nextOutputBuffer, false);
                    } catch (IllegalStateException e) {
                        // This will leak nextOutputBuffer, but there's really nothing else we can do
                        Log.e(TAG, "doFrame: " + e.getMessage(), e);
                        handleDecoderException(e);
                    }
                }
            }
        }

        // Attempt codec recovery even if we have nothing to render right now. Recovery can still
        // be required even if the codec died before giving any output.
        doCodecRecoveryIfRequired(CodecRecoveryManager.FLAG_CHOREOGRAPHER);

        // Request another callback for next frame
        Choreographer.getInstance().postFrameCallback(this);
    }

    private void startChoreographerThread() {
        if (prefs.framePacing != PreferenceConfiguration.FRAME_PACING_BALANCED) {
            // Not using Choreographer in this pacing mode
            return;
        }

        // We use a separate thread to avoid any main thread delays from delaying rendering
        choreographerHandlerThread = new HandlerThread("Video - Choreographer", Process.THREAD_PRIORITY_DEFAULT + Process.THREAD_PRIORITY_MORE_FAVORABLE);
        choreographerHandlerThread.start();

        // Start the frame callbacks
        choreographerHandler = new Handler(choreographerHandlerThread.getLooper());
        choreographerHandler.post(new Runnable() {
            @Override
            public void run() {
                Choreographer.getInstance().postFrameCallback(MediaCodecDecoderRenderer.this);
            }
        });
    }

    private void startRendererThread() {
        rendererThread = new Thread() {
            @Override
            public void run() {
                BufferInfo info = new BufferInfo();
                while (!stopping) {
                    try {
                        // Try to output a frame
                        int outIndex = videoDecoder.dequeueOutputBuffer(info, 50000);
                        if (outIndex >= 0) {
                            long presentationTimeUs = info.presentationTimeUs;
                            int lastIndex = outIndex;

                            numFramesOut++;

                            // Render the latest frame now if frame pacing isn't in balanced mode
                            if (prefs.framePacing != PreferenceConfiguration.FRAME_PACING_BALANCED) {
                                // Get the last output buffer in the queue
                                while ((outIndex = videoDecoder.dequeueOutputBuffer(info, 0)) >= 0) {
                                    videoDecoder.releaseOutputBuffer(lastIndex, false);

                                    numFramesOut++;

                                    lastIndex = outIndex;
                                    presentationTimeUs = info.presentationTimeUs;
                                }

                                if (prefs.framePacing == PreferenceConfiguration.FRAME_PACING_MAX_SMOOTHNESS ||
                                        prefs.framePacing == PreferenceConfiguration.FRAME_PACING_CAP_FPS) {
                                    // In max smoothness or cap FPS mode, we want to never drop frames
                                    // Use a PTS that will cause this frame to never be dropped
                                    videoDecoder.releaseOutputBuffer(lastIndex, 0);
                                } else {
                                    // Use a PTS that will cause this frame to be dropped if another comes in within
                                    // the same V-sync period
                                    videoDecoder.releaseOutputBuffer(lastIndex, System.nanoTime());
                                }

                                statsManager.getActiveWindowVideoStats().totalFramesRendered++;
                            } else {
                                // For balanced frame pacing case, the Choreographer callback will handle rendering.
                                // We just put all frames into the output buffer queue and let it handle things.

                                // Discard the oldest buffer if we've exceeded our limit.
                                //
                                // NB: We have to do this on the producer side because the consumer may not
                                // run for a while (if there is a huge mismatch between stream FPS and display
                                // refresh rate).
                                if (outputBufferQueue.size() == OUTPUT_BUFFER_QUEUE_LIMIT) {
                                    try {
                                        videoDecoder.releaseOutputBuffer(outputBufferQueue.take(), false);
                                    } catch (InterruptedException e) {
                                        // We're shutting down, so we can just drop this buffer on the floor
                                        // and it will be reclaimed when the codec is released.
                                        return;
                                    }
                                }

                                // Add this buffer
                                outputBufferQueue.add(lastIndex);
                            }

                            // Add delta time to the totals (excluding probable outliers)
                            long delta = SystemClock.uptimeMillis() - (presentationTimeUs / 1000);
                            if (delta >= 0 && delta < 1000) {
                                statsManager.getActiveWindowVideoStats().decoderTimeMs += delta;
                                if (!USE_FRAME_RENDER_TIME) {
                                    statsManager.getActiveWindowVideoStats().totalTimeMs += delta;
                                }
                            }
                        } else {
                            if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                                LimeLog.info("Output format changed");
                                outputFormat = videoDecoder.getOutputFormat();
                                LimeLog.info("New output format: " + outputFormat);
                            }
                        }
                    } catch (IllegalStateException e) {
                        handleDecoderException(e);
                    } finally {
                        doCodecRecoveryIfRequired(CodecRecoveryManager.FLAG_RENDER_THREAD);
                    }
                }
            }
        };
        rendererThread.setName("Video - Renderer (MediaCodec)");
        rendererThread.setPriority(Thread.NORM_PRIORITY + 2);
        rendererThread.start();
    }

    private boolean fetchNextInputBuffer() {
        long startTime;
        boolean codecRecovered;

        if (nextInputBuffer != null) {
            // We already have an input buffer
            return true;
        }

        startTime = SystemClock.uptimeMillis();

        try {
            // If we don't have an input buffer index yet, fetch one now
            while (nextInputBufferIndex < 0 && !stopping) {
                nextInputBufferIndex = videoDecoder.dequeueInputBuffer(10000);
            }

            // Get the backing ByteBuffer for the input buffer index
            if (nextInputBufferIndex >= 0) {
                // Using the new getInputBuffer() API on Lollipop allows
                // the framework to do some performance optimizations for us
                nextInputBuffer = videoDecoder.getInputBuffer(nextInputBufferIndex);
                if (nextInputBuffer == null) {
                    // According to the Android docs, getInputBuffer() can return null "if the
                    // index is not a dequeued input buffer". I don't think this ever should
                    // happen but if it does, let's try to get a new input buffer next time.
                    nextInputBufferIndex = -1;
                }
            }
        } catch (IllegalStateException e) {
            handleDecoderException(e);
            return false;
        } finally {
            codecRecovered = doCodecRecoveryIfRequired(CodecRecoveryManager.FLAG_INPUT_THREAD);
        }

        // If codec recovery is required, always return false to ensure the caller will request
        // an IDR frame to complete the codec recovery.
        if (codecRecovered) {
            return false;
        }

        int deltaMs = (int) (SystemClock.uptimeMillis() - startTime);

        if (deltaMs >= 20) {
            LimeLog.warning("Dequeue input buffer ran long: " + deltaMs + " ms");
        }

        if (nextInputBuffer == null) {
            // We've been hung for 5 seconds and no other exception was reported,
            // so generate a decoder hung exception
            if (deltaMs >= 5000 && initialException == null) {
                DecoderExceptions.DecoderHungException decoderHungException =
                        new DecoderExceptions.DecoderHungException(deltaMs);
                if (!reportedCrash) {
                    reportedCrash = true;
                    crashListener.notifyCrash(decoderHungException);
                }
                throw createRendererException(decoderHungException);
            }

            return false;
        }

        return true;
    }

    @Override
    public void start() {
        startRendererThread();
        startChoreographerThread();
    }

    // !!! May be called even if setup()/start() fails !!!
    public void prepareForStop() {
        // Let the decoding code know to ignore codec exceptions now
        stopping = true;
        recoveryManager.setStopping(true);

        // Halt the rendering thread
        if (rendererThread != null) {
            rendererThread.interrupt();
        }

        // Stop any active codec recovery operations
        recoveryManager.stopRecovery();

        // Post a quit message to the Choreographer looper (if we have one)
        if (choreographerHandler != null) {
            choreographerHandler.post(new Runnable() {
                @Override
                public void run() {
                    // Don't allow any further messages to be queued
                    choreographerHandlerThread.quit();

                    // Deregister the frame callback (if registered)
                    Choreographer.getInstance().removeFrameCallback(MediaCodecDecoderRenderer.this);
                }
            });
        }
    }

    @Override
    public void stop() {
        // May be called already, but we'll call it now to be safe
        prepareForStop();

        // Wait for the Choreographer looper to shut down (if we have one)
        if (choreographerHandlerThread != null) {
            try {
                choreographerHandlerThread.join();
            } catch (InterruptedException e) {
                Log.e(TAG, "stop: " + e.getMessage(), e);

                // InterruptedException clears the thread's interrupt status. Since we can't
                // handle that here, we will re-interrupt the thread to set the interrupt
                // status back to true.
                Thread.currentThread().interrupt();
            }
        }

        // Wait for the renderer thread to shut down
        try {
            rendererThread.join();
        } catch (InterruptedException e) {
            Log.e(TAG, "stop: " + e.getMessage(), e);

            // InterruptedException clears the thread's interrupt status. Since we can't
            // handle that here, we will re-interrupt the thread to set the interrupt
            // status back to true.
            Thread.currentThread().interrupt();
        }
    }

    @Override
    public void cleanup() {
        videoDecoder.release();
    }

    @Override
    public void setHdrMode(boolean enabled, byte[] hdrMetadata) {
        // HDR metadata is only supported in Android 7.0 and later, so don't bother
        // restarting the codec on anything earlier than that.
        if (currentHdrMetadata != null && (!enabled || hdrMetadata == null)) {
            currentHdrMetadata = null;
        } else if (enabled && hdrMetadata != null && !Arrays.equals(currentHdrMetadata, hdrMetadata)) {
            currentHdrMetadata = hdrMetadata;
        } else {
            // Nothing to do
            return;
        }

        // If we reach this point, we need to restart the MediaCodec instance to
        // pick up the HDR metadata change. This will happen on the next input
        // or output buffer.
        recoveryManager.scheduleRestartForHdrChange();
    }

    @SuppressWarnings("BooleanMethodIsAlwaysInverted")
    private boolean queueNextInputBuffer(long timestampUs, int codecFlags) {
        boolean codecRecovered;

        try {
            videoDecoder.queueInputBuffer(nextInputBufferIndex,
                    0, nextInputBuffer.position(),
                    timestampUs, codecFlags);

            // We need a new buffer now
            nextInputBufferIndex = -1;
            nextInputBuffer = null;
        } catch (IllegalStateException e) {
            if (handleDecoderException(e)) {
                // We encountered a transient error. In this case, just hold onto the buffer
                // (to avoid leaking it), clear it, and keep it for the next frame. We'll return
                // false to trigger an IDR frame to recover.
                nextInputBuffer.clear();
            } else {
                // We encountered a non-transient error. In this case, we will simply leak the
                // buffer because we cannot be sure we will ever succeed in queuing it.
                nextInputBufferIndex = -1;
                nextInputBuffer = null;
            }
            return false;
        } finally {
            codecRecovered = doCodecRecoveryIfRequired(CodecRecoveryManager.FLAG_INPUT_THREAD);
        }

        // If codec recovery is required, always return false to ensure the caller will request
        // an IDR frame to complete the codec recovery.
        if (codecRecovered) {
            return false;
        }

        // Fetch a new input buffer now while we have some time between frames
        // to have it ready immediately when the next frame arrives.
        //
        // We must propagate the return value here in order to properly handle
        // codec recovery happening in fetchNextInputBuffer(). If we don't, we'll
        // never get an IDR frame to complete the recovery process.
        return fetchNextInputBuffer();
    }

    @Override
    public int submitDecodeUnit(byte[] decodeUnitData, int decodeUnitLength, int decodeUnitType,
                                int frameNumber, int frameType, char frameHostProcessingLatency,
                                long receiveTimeMs, long enqueueTimeMs) {
        if (stopping) {
            return MoonBridge.DR_OK;
        }

        VideoStats activeStats = statsManager.getActiveWindowVideoStats();
        boolean isNewIdrFrame = statsManager.updateFrameStats(frameNumber, frameType);

        // Reset CSD data for each new IDR frame
        if (isNewIdrFrame) {
            csdProcessor.clear();
        }

        statsManager.updatePerformanceOverlay(activeDecoderName);

        boolean csdSubmittedForThisFrame = false;

        // IDR frames require special handling for CSD buffer submission
        if (frameType == MoonBridge.FRAME_TYPE_IDR) {
            int result = handleIdrFrameCsd(decodeUnitType, decodeUnitData, decodeUnitLength);
            if (result != MoonBridge.DR_OK) {
                return result;
            }

            // Check if CSD was submitted in handleIdrFrameCsd
            if (decodeUnitType == MoonBridge.BUFFER_TYPE_SPS ||
                decodeUnitType == MoonBridge.BUFFER_TYPE_VPS ||
                decodeUnitType == MoonBridge.BUFFER_TYPE_PPS) {
                return MoonBridge.DR_OK;
            }

            // Check if we submitted CSD for this frame
            csdSubmittedForThisFrame = (videoFormat & (MoonBridge.VIDEO_FORMAT_MASK_H264 | MoonBridge.VIDEO_FORMAT_MASK_H265)) != 0 &&
                (!submittedCsd || !fusedIdrFrame);
        }

        statsManager.updateHostProcessingLatency(frameHostProcessingLatency);

        activeStats.totalFramesReceived++;
        activeStats.totalFrames++;

        if (!FRAME_RENDER_TIME_ONLY) {
            // Count time from first packet received to enqueue time as receive time
            // We will count DU queue time as part of decoding, because it is directly
            // caused by a slow decoder.
            activeStats.totalTimeMs += enqueueTimeMs - receiveTimeMs;
        }

        return submitFrameData(decodeUnitData, decodeUnitLength, frameType, enqueueTimeMs, csdSubmittedForThisFrame);
    }

    private int handleIdrFrameCsd(int decodeUnitType, byte[] decodeUnitData, int decodeUnitLength) {if (decodeUnitType == MoonBridge.BUFFER_TYPE_VPS) {
            csdProcessor.processVps(decodeUnitData, decodeUnitLength);
            return MoonBridge.DR_OK;
        } else if (decodeUnitType == MoonBridge.BUFFER_TYPE_SPS) {
            // Only the HEVC SPS hits this path (H.264 is handled above)
            csdProcessor.processHevcSps(decodeUnitData, decodeUnitLength);
            return MoonBridge.DR_OK;
        } else if (decodeUnitType == MoonBridge.BUFFER_TYPE_PPS) {
            csdProcessor.processPps(decodeUnitData, decodeUnitLength);
            return MoonBridge.DR_OK;
        } else if ((videoFormat & (MoonBridge.VIDEO_FORMAT_MASK_H264 | MoonBridge.VIDEO_FORMAT_MASK_H265)) != 0) {
            return submitCsdBuffers();
        }

        return MoonBridge.DR_OK;
    }

    private int submitCsdBuffers() {
        // If this is the first CSD blob or we aren't supporting fused IDR frames,
        // submit the CSD blob in a separate input buffer for each IDR frame
        if (!submittedCsd || !fusedIdrFrame) {
            if (!fetchNextInputBuffer()) {
                return MoonBridge.DR_NEED_IDR;
            }

            // Submit all CSD when we receive the first non-CSD blob in an IDR frame
            csdProcessor.writeCsdBuffers(nextInputBuffer);

            if (!queueNextInputBuffer(0, MediaCodec.BUFFER_FLAG_CODEC_CONFIG)) {
                return MoonBridge.DR_NEED_IDR;
            }

            submittedCsd = true;
        }

        return MoonBridge.DR_OK;
    }

    private int submitFrameData(byte[] decodeUnitData, int decodeUnitLength,
                                 int frameType, long enqueueTimeMs, boolean csdSubmittedForThisFrame) {
        if (!fetchNextInputBuffer()) {
            return MoonBridge.DR_NEED_IDR;
        }

        int codecFlags = prepareCodecFlags(frameType, csdSubmittedForThisFrame);
        long timestampUs = calculateTimestampUs(enqueueTimeMs);

        if (!validateAndCopyDecodeUnit(decodeUnitData, decodeUnitLength)) {
            return MoonBridge.DR_NEED_IDR;
        }

        if (!queueNextInputBuffer(timestampUs, codecFlags)) {
            return MoonBridge.DR_NEED_IDR;
        }

        numFramesIn++;
        return MoonBridge.DR_OK;
    }

    private int prepareCodecFlags(int frameType, boolean csdSubmittedForThisFrame) {
        int codecFlags = 0;

        if (frameType == MoonBridge.FRAME_TYPE_IDR) {
            codecFlags |= MediaCodec.BUFFER_FLAG_SYNC_FRAME;

            // If we are using fused IDR frames, submit the CSD with each IDR frame
            if (fusedIdrFrame && !csdSubmittedForThisFrame) {
                csdProcessor.writeCsdBuffers(nextInputBuffer);
            }
        }

        return codecFlags;
    }

    private long calculateTimestampUs(long enqueueTimeMs) {
        long timestampUs = enqueueTimeMs * 1000;
        if (timestampUs <= lastTimestampUs) {
            // We can't submit multiple buffers with the same timestamp
            // so bump it up by one before queuing
            timestampUs = lastTimestampUs + 1;
        }
        lastTimestampUs = timestampUs;
        return timestampUs;
    }

    private boolean validateAndCopyDecodeUnit(byte[] decodeUnitData, int decodeUnitLength) {
        if (decodeUnitLength > nextInputBuffer.limit() - nextInputBuffer.position()) {
            IllegalArgumentException exception = new IllegalArgumentException(
                    "Decode unit length " + decodeUnitLength + " too large for input buffer " + nextInputBuffer.limit());
            if (!reportedCrash) {
                reportedCrash = true;
                crashListener.notifyCrash(exception);
            }
            throw createRendererException(exception);
        }

        // Copy data from our buffer list into the input buffer
        nextInputBuffer.put(decodeUnitData, 0, decodeUnitLength);
        return true;
    }


    @Override
    public int getCapabilities() {
        int capabilities = 0;

        // Request the optimal number of slices per frame for this decoder
        capabilities |= MoonBridge.CAPABILITY_SLICES_PER_FRAME(capabilityChecker.getOptimalSlicesPerFrame());

        // Enable reference frame invalidation on supported hardware
        if (capabilityChecker.isRefFrameInvalidationHevc()) {
            capabilities |= MoonBridge.CAPABILITY_REFERENCE_FRAME_INVALIDATION_HEVC;
        }
        if (capabilityChecker.isRefFrameInvalidationAv1()) {
            capabilities |= MoonBridge.CAPABILITY_REFERENCE_FRAME_INVALIDATION_AV1;
        }

        // Enable direct submit on supported hardware
        if (capabilityChecker.isDirectSubmit()) {
            capabilities |= MoonBridge.CAPABILITY_DIRECT_SUBMIT;
        }

        return capabilities;
    }

    public int getAverageEndToEndLatency() {
        return statsManager.getAverageEndToEndLatency();
    }

    public int getAverageDecoderLatency() {
        return statsManager.getAverageDecoderLatency();
    }
}
