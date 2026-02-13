package com.limelight.binding.audio;

import android.content.Context;
import android.content.Intent;
import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.media.audiofx.AudioEffect;
import android.util.Log;

import com.limelight.nvstream.av.audio.AudioRenderer;
import com.limelight.nvstream.jni.MoonBridge;

public class AndroidAudioRenderer implements AudioRenderer {

    private static final String TAG = "AndroidAudioRenderer";
    private final Context context;
    private final boolean enableAudioFx;

    private AudioTrack track;

    // Audio latency control constants
    private static final int MAX_PENDING_AUDIO_MS = 120;  // Maximum pending audio before dropping
    private static final int TARGET_PENDING_AUDIO_MS = 80; // Target latency for recovery
    private static final int MIN_BUFFER_MS = 20;           // Minimum buffer to maintain
    private int consecutiveDrops = 0;

    // Fade parameters for smooth audio transitions (reduces pops/clicks)
    private static final int FADE_SAMPLES = 64;  // Number of samples for fade in/out
    private boolean needsFadeIn = false;
    private short[] lastAudioFrame = null;

    public AndroidAudioRenderer(Context context, boolean enableAudioFx) {
        this.context = context;
        this.enableAudioFx = enableAudioFx;
    }

    private AudioTrack createAudioTrack(int channelConfig, int sampleRate, int bufferSize, boolean lowLatency) {
        AudioAttributes.Builder attributesBuilder = new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_GAME);
        AudioFormat format = new AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(sampleRate)
                .setChannelMask(channelConfig)
                .build();

        AudioTrack.Builder trackBuilder = new AudioTrack.Builder()
                .setAudioFormat(format)
                .setAudioAttributes(attributesBuilder.build())
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setBufferSizeInBytes(bufferSize);

        // Use PERFORMANCE_MODE_LOW_LATENCY on O and later
        if (lowLatency) {
            trackBuilder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY);
        }

        return trackBuilder.build();
    }

    @Override
    public int setup(MoonBridge.AudioConfiguration audioConfiguration, int sampleRate, int samplesPerFrame) {
        int channelConfig;
        int bytesPerFrame;

        switch (audioConfiguration.channelCount)
        {
            case 2:
                channelConfig = AudioFormat.CHANNEL_OUT_STEREO;
                break;
            case 4:
                channelConfig = AudioFormat.CHANNEL_OUT_QUAD;
                break;
            case 6:
                channelConfig = AudioFormat.CHANNEL_OUT_5POINT1;
                break;
            case 8:
                // AudioFormat.CHANNEL_OUT_7POINT1_SURROUND isn't available until Android 6.0,
                // yet the CHANNEL_OUT_SIDE_LEFT and CHANNEL_OUT_SIDE_RIGHT constants were added
                // in 5.0, so just hardcode the constant so we can work on Lollipop.
                channelConfig = 0x000018fc; // AudioFormat.CHANNEL_OUT_7POINT1_SURROUND
                break;
            default:
                LimeLog.severe("Decoder returned unhandled channel count");
                return -1;
        }

        LimeLog.info("Audio channel config: "+String.format("0x%X", channelConfig));

        bytesPerFrame = audioConfiguration.channelCount * samplesPerFrame * 2;

        // We're not supposed to request less than the minimum
        // buffer size for our buffer, but it appears that we can
        // do this on many devices and it lowers audio latency.
        // We'll try the small buffer size first and if it fails,
        // use progressively larger buffer sizes.

        for (int i = 0; i < 6; i++) {
            boolean lowLatency;
            int bufferSize;

            // We will try:
            // 1) Small buffer (2 frames), low latency mode
            // 2) Medium buffer (4 frames), low latency mode
            // 3) Large buffer (min buffer), low latency mode
            // 4) Small buffer (2 frames), standard mode
            // 5) Medium buffer (4 frames), standard mode
            // 6) Large buffer (min buffer), standard mode

            switch (i) {
                case 0:
                case 1:
                case 2:
                    lowLatency = true;
                    break;
                case 3:
                case 4:
                case 5:
                    lowLatency = false;
                    break;
                default:
                    // Unreachable
                    throw new IllegalStateException();
            }

            switch (i) {
                case 0:
                case 3:
                    // Small buffer - 2 frames
                    bufferSize = bytesPerFrame * 2;
                    break;

                case 1:
                case 4:
                    // Medium buffer - 4 frames (better for avoiding crackling)
                    bufferSize = bytesPerFrame * 4;
                    break;

                case 2:
                case 5:
                    // Large buffer - use minimum recommended size
                    bufferSize = Math.max(AudioTrack.getMinBufferSize(sampleRate,
                            channelConfig,
                            AudioFormat.ENCODING_PCM_16BIT),
                            bytesPerFrame * 4);

                    // Round to next frame
                    bufferSize = (((bufferSize + (bytesPerFrame - 1)) / bytesPerFrame) * bytesPerFrame);
                    break;
                default:
                    // Unreachable
                    throw new IllegalStateException();
            }

            // Skip low latency options if hardware sample rate doesn't match the content
            if (AudioTrack.getNativeOutputSampleRate(AudioManager.STREAM_MUSIC) != sampleRate && lowLatency) {
                continue;
            }

            // Skip low latency options when using audio effects, since low latency mode
            // precludes the use of the audio effect pipeline (as of Android 13).
            if (enableAudioFx && lowLatency) {
                continue;
            }

            try {
                track = createAudioTrack(channelConfig, sampleRate, bufferSize, lowLatency);
                track.play();

                // Successfully created working AudioTrack. We're done here.
                LimeLog.info("Audio track configuration: "+bufferSize+" "+lowLatency);
                break;
            } catch (Exception e) {
                // Try to release the AudioTrack if we got far enough
                Log.e(TAG, "setup: "+e.getMessage(), e);
                try {
                    if (track != null) {
                        track.release();
                        track = null;
                    }
                } catch (Exception ignored) {}
            }
        }

        if (track == null) {
            // Couldn't create any audio track for playback
            return -2;
        }

        return 0;
    }

    @Override
    public void playDecodedAudio(short[] audioData) {
        int pendingMs = MoonBridge.getPendingAudioDuration();

        // Use a more lenient threshold to prevent audio flickering.
        // Only drop audio when we have excessive buffering (over threshold),
        // and implement gradual recovery to avoid choppy audio.
        if (pendingMs > MAX_PENDING_AUDIO_MS) {
            consecutiveDrops++;
            // Only log occasionally to avoid log spam
            if (consecutiveDrops == 1 || consecutiveDrops % 10 == 0) {
                LimeLog.info("Dropping audio frame, pending: " + pendingMs + " ms (drops: " + consecutiveDrops + ")");
            }
            // Mark that we need fade-in when resuming to avoid pops
            needsFadeIn = true;
            return;
        }

        // Apply fade-in if we're resuming after drops to prevent clicks/pops
        if (needsFadeIn && consecutiveDrops > 0) {
            applyFadeIn(audioData);
            needsFadeIn = false;
        }

        // Reset drop counter when we successfully write
        if (consecutiveDrops > 0 && pendingMs < TARGET_PENDING_AUDIO_MS) {
            LimeLog.info("Audio recovered after " + consecutiveDrops + " drops, pending: " + pendingMs + " ms");
            consecutiveDrops = 0;
        }

        // Store reference for potential crossfade (optional future enhancement)
        lastAudioFrame = audioData;

        // This will block until the write is completed.
        track.write(audioData, 0, audioData.length);
    }

    /**
     * Apply a smooth fade-in to avoid audio pops when resuming playback.
     * This modifies the audioData array in-place.
     */
    private void applyFadeIn(short[] audioData) {
        int samplesToFade = Math.min(FADE_SAMPLES, audioData.length);
        for (int i = 0; i < samplesToFade; i++) {
            // Linear fade from 0 to 1
            float fadeMultiplier = (float) i / samplesToFade;
            audioData[i] = (short) (audioData[i] * fadeMultiplier);
        }
    }

    @Override
    public void start() {
        // Reset audio state
        consecutiveDrops = 0;
        needsFadeIn = false;
        lastAudioFrame = null;

        if (enableAudioFx) {
            // Open an audio effect control session to allow equalizers to apply audio effects
            Intent i = new Intent(AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION);
            i.putExtra(AudioEffect.EXTRA_AUDIO_SESSION, track.getAudioSessionId());
            i.putExtra(AudioEffect.EXTRA_PACKAGE_NAME, context.getPackageName());
            i.putExtra(AudioEffect.EXTRA_CONTENT_TYPE, AudioEffect.CONTENT_TYPE_GAME);
            context.sendBroadcast(i);
        }
    }

    @Override
    public void stop() {
        if (enableAudioFx) {
            // Close our audio effect control session when we're stopping
            Intent i = new Intent(AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION);
            i.putExtra(AudioEffect.EXTRA_AUDIO_SESSION, track.getAudioSessionId());
            i.putExtra(AudioEffect.EXTRA_PACKAGE_NAME, context.getPackageName());
            context.sendBroadcast(i);
        }
    }

    @Override
    public void cleanup() {
        // Immediately drop all pending data
        track.pause();
        track.flush();

        track.release();
    }
}
