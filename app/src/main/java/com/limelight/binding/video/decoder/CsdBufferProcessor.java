package com.limelight.binding.video.decoder;


import com.limelight.binding.video.MediaCodecHelper;

import java.nio.ByteBuffer;
import java.util.ArrayList;

/**
 * Handles Codec Specific Data (CSD) buffer processing for H.265/AV1.
 * <p>
 * This class manages:
 * - VPS (Video Parameter Set) for HEVC
 * - SPS (Sequence Parameter Set) for HEVC
 * - PPS (Picture Parameter Set) for HEVC
 */
public class CsdBufferProcessor {

    private final ArrayList<byte[]> vpsBuffers = new ArrayList<>();
    private final ArrayList<byte[]> spsBuffers = new ArrayList<>();
    private final ArrayList<byte[]> ppsBuffers = new ArrayList<>();

    // Stats
    private int numVpsIn;
    private int numSpsIn;
    private int numPpsIn;

    public CsdBufferProcessor() {
    }

    /**
     * Initialize the processor with decoder-specific settings.
     */
    public void initialize(String decoderName) {
        boolean constrainedHighProfile = MediaCodecHelper.decoderNeedsConstrainedHighProfile(decoderName);

        if (constrainedHighProfile) {
            LimeLog.info("Decoder " + decoderName + " needs constrained high profile");
        }
    }

    /**
     * Clear all CSD buffers.
     */
    public void clear() {
        vpsBuffers.clear();
        spsBuffers.clear();
        ppsBuffers.clear();
    }

    /**
     * Process VPS (HEVC) data.
     */
    public void processVps(byte[] decodeUnitData, int decodeUnitLength) {
        numVpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        vpsBuffers.add(naluBuffer);
    }

    /**
     * Process HEVC SPS data.
     */
    public void processHevcSps(byte[] decodeUnitData, int decodeUnitLength) {
        numSpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        spsBuffers.add(naluBuffer);
    }

    /**
     * Process PPS data.
     */
    public void processPps(byte[] decodeUnitData, int decodeUnitLength) {
        numPpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        ppsBuffers.add(naluBuffer);
    }


    /**
     * Write all CSD buffers to the given ByteBuffer.
     */
    public void writeCsdBuffers(ByteBuffer buffer) {
        for (byte[] vpsBuffer : vpsBuffers) {
            buffer.put(vpsBuffer);
        }
        for (byte[] spsBuffer : spsBuffers) {
            buffer.put(spsBuffer);
        }
        for (byte[] ppsBuffer : ppsBuffers) {
            buffer.put(ppsBuffer);
        }
    }

    // Stats getters
    public int getNumVpsIn() {
        return numVpsIn;
    }

    public int getNumSpsIn() {
        return numSpsIn;
    }

    public int getNumPpsIn() {
        return numPpsIn;
    }
}

