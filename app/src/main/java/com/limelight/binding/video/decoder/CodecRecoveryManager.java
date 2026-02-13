package com.limelight.binding.video.decoder;

import android.media.MediaCodec.CodecException;
import android.util.Log;

import java.util.concurrent.atomic.AtomicInteger;

/**
 * Manages codec recovery operations when MediaCodec encounters errors.
 * <p>
 * This class handles:
 * - Thread quiescence coordination for safe codec operations
 * - Flush, restart, and reset recovery strategies
 * - Transient and non-transient exception handling
 * - Recovery attempt tracking and limiting
 */
public class CodecRecoveryManager {

    private static final String TAG = "CodecRecoveryManager";

    // Recovery type constants
    public static final int RECOVERY_TYPE_NONE = 0;
    public static final int RECOVERY_TYPE_FLUSH = 1;
    public static final int RECOVERY_TYPE_RESTART = 2;
    public static final int RECOVERY_TYPE_RESET = 3;

    // Thread quiescence flags
    public static final int FLAG_INPUT_THREAD = 0x1;
    public static final int FLAG_RENDER_THREAD = 0x2;
    public static final int FLAG_CHOREOGRAPHER = 0x4;
    public static final int FLAG_ALL = FLAG_INPUT_THREAD | FLAG_RENDER_THREAD | FLAG_CHOREOGRAPHER;

    private static final int MAX_RECOVERY_ATTEMPTS = 10;

    private final AtomicInteger recoveryType = new AtomicInteger(RECOVERY_TYPE_NONE);
    private final Object recoveryMonitor = new Object();
    private int threadQuiescedFlags = 0;
    private int recoveryAttempts = 0;

    private final CodecRecoveryCallback callback;
    private volatile boolean stopping = false;
    private boolean hasChoreographerThread = false;

    /**
     * Callback interface for codec recovery operations.
     */
    public interface CodecRecoveryCallback {
        void onFlushDecoder();
        void onRestartDecoder();
        void onResetDecoder();
        boolean onRecreateDecoder();
        void onRecoveryFailed(Exception e);
        void onClearBuffers();
    }

    public CodecRecoveryManager(CodecRecoveryCallback callback) {
        this.callback = callback;
    }

    public void setHasChoreographerThread(boolean hasChoreographerThread) {
        this.hasChoreographerThread = hasChoreographerThread;
    }

    public void setStopping(boolean stopping) {
        this.stopping = stopping;
    }

    public boolean isStopping() {
        return stopping;
    }

    public int getRecoveryType() {
        return recoveryType.get();
    }

    public int getRecoveryAttempts() {
        return recoveryAttempts;
    }

    public void resetRecoveryAttempts() {
        recoveryAttempts = 0;
    }

    /**
     * Called by threads that interact with the MediaCodec to participate in recovery.
     * Returns true if codec recovery was performed.
     */
    public boolean doRecoveryIfRequired(int quiescenceFlag) {
        if (recoveryType.get() == RECOVERY_TYPE_NONE) {
            return false;
        }

        synchronized (recoveryMonitor) {
            if (!hasChoreographerThread) {
                threadQuiescedFlags |= FLAG_CHOREOGRAPHER;
            }

            threadQuiescedFlags |= quiescenceFlag;

            if (threadQuiescedFlags == FLAG_ALL) {
                performRecovery();
            } else {
                waitForRecovery();
            }
        }

        return true;
    }

    private void performRecovery() {
        callback.onClearBuffers();

        // Try flush first
        if (recoveryType.get() == RECOVERY_TYPE_FLUSH) {
            LimeLog.warning("Flushing decoder");
            try {
                callback.onFlushDecoder();
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalStateException e) {
                Log.e(TAG, "Flush failed: " + e.getMessage(), e);
                recoveryType.set(RECOVERY_TYPE_RESTART);
            }
        }

        if (recoveryType.get() != RECOVERY_TYPE_NONE) {
            recoveryAttempts++;
            LimeLog.info("Codec recovery attempt: " + recoveryAttempts);
        }

        // Try restart
        if (recoveryType.get() == RECOVERY_TYPE_RESTART) {
            LimeLog.warning("Trying to restart decoder after CodecException");
            try {
                callback.onRestartDecoder();
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalArgumentException e) {
                Log.e(TAG, "Restart failed (surface invalid?): " + e.getMessage(), e);
                stopping = true;
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalStateException e) {
                Log.e(TAG, "Restart failed: " + e.getMessage(), e);
                recoveryType.set(RECOVERY_TYPE_RESET);
            }
        }

        // Try reset
        if (recoveryType.get() == RECOVERY_TYPE_RESET) {
            LimeLog.warning("Trying to reset decoder after CodecException");
            try {
                callback.onResetDecoder();
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalArgumentException e) {
                Log.e(TAG, "Reset failed (surface invalid?): " + e.getMessage(), e);
                stopping = true;
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalStateException e) {
                Log.e(TAG, "Reset failed: " + e.getMessage(), e);
            }
        }

        // Last resort: recreate
        if (recoveryType.get() == RECOVERY_TYPE_RESET) {
            LimeLog.warning("Trying to recreate decoder after CodecException");
            try {
                if (callback.onRecreateDecoder()) {
                    recoveryType.set(RECOVERY_TYPE_NONE);
                } else {
                    throw new IllegalStateException("Decoder recreation failed");
                }
            } catch (IllegalArgumentException e) {
                Log.e(TAG, "Recreation failed (surface invalid?): " + e.getMessage(), e);
                stopping = true;
                recoveryType.set(RECOVERY_TYPE_NONE);
            } catch (IllegalStateException e) {
                callback.onRecoveryFailed(e);
            }
        }

        threadQuiescedFlags = 0;
        recoveryMonitor.notifyAll();
    }

    private void waitForRecovery() {
        while (recoveryType.get() != RECOVERY_TYPE_NONE) {
            try {
                LimeLog.info("Waiting to quiesce decoder threads: " + threadQuiescedFlags);
                recoveryMonitor.wait(1000);
            } catch (InterruptedException e) {
                Log.e(TAG, "Interrupted while waiting for recovery: " + e.getMessage(), e);
                Thread.currentThread().interrupt();
                break;
            }
        }
    }

    public void scheduleFlush() {
        recoveryType.compareAndSet(RECOVERY_TYPE_NONE, RECOVERY_TYPE_FLUSH);
    }

    public void scheduleRecoverableRecovery(CodecException codecExc) {
        Log.e(TAG, "Scheduling recoverable recovery: " + codecExc.getMessage(), codecExc);
        if (recoveryType.compareAndSet(RECOVERY_TYPE_NONE, RECOVERY_TYPE_RESTART)) {
            LimeLog.info("Decoder requires restart for recoverable CodecException");
        } else if (recoveryType.compareAndSet(RECOVERY_TYPE_FLUSH, RECOVERY_TYPE_RESTART)) {
            LimeLog.info("Decoder flush promoted to restart for recoverable CodecException");
        }
    }

    public void scheduleNonRecoverableRecovery(CodecException codecExc) {
        Log.e(TAG, "Scheduling non-recoverable recovery: " + codecExc.getMessage(), codecExc);
        if (recoveryType.compareAndSet(RECOVERY_TYPE_NONE, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder requires reset for non-recoverable CodecException");
        } else if (recoveryType.compareAndSet(RECOVERY_TYPE_FLUSH, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder flush promoted to reset for non-recoverable CodecException");
        } else if (recoveryType.compareAndSet(RECOVERY_TYPE_RESTART, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder restart promoted to reset for non-recoverable CodecException");
        }
    }

    public void scheduleResetRecovery(IllegalStateException e) {
        Log.e(TAG, "Scheduling reset recovery: " + e.getMessage(), e);
        if (recoveryType.compareAndSet(RECOVERY_TYPE_NONE, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder requires reset for IllegalStateException");
        } else if (recoveryType.compareAndSet(RECOVERY_TYPE_FLUSH, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder flush promoted to reset for IllegalStateException");
        } else if (recoveryType.compareAndSet(RECOVERY_TYPE_RESTART, RECOVERY_TYPE_RESET)) {
            LimeLog.info("Decoder restart promoted to reset for IllegalStateException");
        }
    }

    public void scheduleRestartForHdrChange() {
        recoveryAttempts = 0;
        if (!recoveryType.compareAndSet(RECOVERY_TYPE_NONE, RECOVERY_TYPE_RESTART)) {
            recoveryType.compareAndSet(RECOVERY_TYPE_FLUSH, RECOVERY_TYPE_RESTART);
        }
    }

    public boolean hasExceededMaxRecoveryAttempts() {
        return recoveryAttempts >= MAX_RECOVERY_ATTEMPTS;
    }

    public void stopRecovery() {
        synchronized (recoveryMonitor) {
            recoveryType.set(RECOVERY_TYPE_NONE);
            recoveryMonitor.notifyAll();
        }
    }
}

