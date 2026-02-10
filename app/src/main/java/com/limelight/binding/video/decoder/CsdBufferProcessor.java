package com.limelight.binding.video.decoder;

import org.jcodec.codecs.h264.H264Utils;
import org.jcodec.codecs.h264.io.model.SeqParameterSet;
import org.jcodec.codecs.h264.io.model.VUIParameters;

import com.limelight.LimeLog;
import com.limelight.binding.video.MediaCodecHelper;

import java.nio.ByteBuffer;
import java.util.ArrayList;

/**
 * Handles Codec Specific Data (CSD) buffer processing for H.264/H.265/AV1.
 * <p>
 * This class manages:
 * - VPS (Video Parameter Set) for HEVC
 * - SPS (Sequence Parameter Set) for H.264 and HEVC
 * - PPS (Picture Parameter Set) for H.264 and HEVC
 * - SPS patching and bitstream restrictions
 */
public class CsdBufferProcessor {

    private final ArrayList<byte[]> vpsBuffers = new ArrayList<>();
    private final ArrayList<byte[]> spsBuffers = new ArrayList<>();
    private final ArrayList<byte[]> ppsBuffers = new ArrayList<>();

    private boolean needsSpsBitstreamFixup;
    private boolean needsBaselineSpsHack;
    private boolean constrainedHighProfile;
    private boolean refFrameInvalidationActive;
    private SeqParameterSet savedSps;

    private int initialWidth;
    private int initialHeight;
    private int refreshRate;

    // Stats
    private int numVpsIn;
    private int numSpsIn;
    private int numPpsIn;

    public CsdBufferProcessor() {
    }

    /**
     * Initialize the processor with decoder-specific settings.
     */
    public void initialize(String decoderName, int width, int height, int refreshRate,
                          boolean refFrameInvalidationActive) {
        this.initialWidth = width;
        this.initialHeight = height;
        this.refreshRate = refreshRate;
        this.refFrameInvalidationActive = refFrameInvalidationActive;

        this.needsSpsBitstreamFixup = MediaCodecHelper.decoderNeedsSpsBitstreamRestrictions(decoderName);
        this.needsBaselineSpsHack = MediaCodecHelper.decoderNeedsBaselineSpsHack(decoderName);
        this.constrainedHighProfile = MediaCodecHelper.decoderNeedsConstrainedHighProfile(decoderName);

        if (needsSpsBitstreamFixup) {
            LimeLog.info("Decoder " + decoderName + " needs SPS bitstream restrictions fixup");
        }
        if (needsBaselineSpsHack) {
            LimeLog.info("Decoder " + decoderName + " needs baseline SPS hack");
        }
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
     * Process H.264 SPS data.
     */
    public byte[] processH264Sps(byte[] decodeUnitData, int decodeUnitLength) {
        numSpsIn++;

        ByteBuffer spsBuf = ByteBuffer.wrap(decodeUnitData);
        int startSeqLen = decodeUnitData[2] == 0x01 ? 3 : 4;

        // Skip to the start of the NALU data
        spsBuf.position(startSeqLen + 1);

        // Read and patch SPS
        SeqParameterSet sps = H264Utils.readSPS(spsBuf);

        patchSpsParameters(sps);
        addOrPatchBitstreamRestrictions(sps);
        handleBaselineSpsHack(sps);
        doProfileSpecificSpsPatching(sps);

        // Write the patched SPS
        ByteBuffer escapedNalu = H264Utils.writeSPS(sps, decodeUnitLength);
        byte[] naluBuffer = new byte[startSeqLen + 1 + escapedNalu.limit()];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, startSeqLen + 1);
        escapedNalu.get(naluBuffer, startSeqLen + 1, escapedNalu.limit());

        spsBuffers.add(naluBuffer);
        return naluBuffer;
    }

    /**
     * Process VPS (HEVC) data.
     */
    public byte[] processVps(byte[] decodeUnitData, int decodeUnitLength) {
        numVpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        vpsBuffers.add(naluBuffer);
        return naluBuffer;
    }

    /**
     * Process HEVC SPS data.
     */
    public byte[] processHevcSps(byte[] decodeUnitData, int decodeUnitLength) {
        numSpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        spsBuffers.add(naluBuffer);
        return naluBuffer;
    }

    /**
     * Process PPS data.
     */
    public byte[] processPps(byte[] decodeUnitData, int decodeUnitLength) {
        numPpsIn++;
        byte[] naluBuffer = new byte[decodeUnitLength];
        System.arraycopy(decodeUnitData, 0, naluBuffer, 0, decodeUnitLength);
        ppsBuffers.add(naluBuffer);
        return naluBuffer;
    }

    private void patchSpsParameters(SeqParameterSet sps) {
        if (!refFrameInvalidationActive) {
            if (initialWidth <= 720 && initialHeight <= 480 && refreshRate <= 60) {
                LimeLog.info("Patching level_idc to 31");
                sps.levelIdc = 31;
            } else if (initialWidth <= 1280 && initialHeight <= 720 && refreshRate <= 60) {
                LimeLog.info("Patching level_idc to 32");
                sps.levelIdc = 32;
            } else if (initialWidth <= 1920 && initialHeight <= 1080 && refreshRate <= 60) {
                LimeLog.info("Patching level_idc to 42");
                sps.levelIdc = 42;
            }
        }

        if (!refFrameInvalidationActive) {
            LimeLog.info("Patching num_ref_frames in SPS");
            sps.numRefFrames = 1;
        }
    }

    private void addOrPatchBitstreamRestrictions(SeqParameterSet sps) {
        if (sps.vuiParams == null) {
            LimeLog.info("Adding VUI parameters");
            sps.vuiParams = new VUIParameters();
        }

        if (sps.vuiParams.bitstreamRestriction == null) {
            LimeLog.info("Adding bitstream restrictions");
            sps.vuiParams.bitstreamRestriction = new VUIParameters.BitstreamRestriction();
            sps.vuiParams.bitstreamRestriction.motionVectorsOverPicBoundariesFlag = true;
            sps.vuiParams.bitstreamRestriction.maxBytesPerPicDenom = 2;
            sps.vuiParams.bitstreamRestriction.maxBitsPerMbDenom = 1;
            sps.vuiParams.bitstreamRestriction.log2MaxMvLengthHorizontal = 16;
            sps.vuiParams.bitstreamRestriction.log2MaxMvLengthVertical = 16;
            sps.vuiParams.bitstreamRestriction.numReorderFrames = 0;
        } else {
            LimeLog.info("Patching bitstream restrictions");
        }

        sps.vuiParams.bitstreamRestriction.maxDecFrameBuffering = sps.numRefFrames;
    }

    private void handleBaselineSpsHack(SeqParameterSet sps) {
        if (needsBaselineSpsHack) {
            LimeLog.info("Hacking SPS to baseline");
            sps.profileIdc = 66;
            savedSps = sps;
        }
    }

    private void doProfileSpecificSpsPatching(SeqParameterSet sps) {
        if (sps.profileIdc == 100 && constrainedHighProfile) {
            LimeLog.info("Setting constraint set flags for constrained high profile");
            sps.constraintSet4Flag = true;
            sps.constraintSet5Flag = true;
        } else {
            sps.constraintSet4Flag = false;
            sps.constraintSet5Flag = false;
        }
    }

    /**
     * Create the SPS replay buffer for baseline SPS hack.
     */
    public byte[] createSpsReplayBuffer() {
        if (savedSps == null) {
            return null;
        }

        // Switch the H264 profile back to high
        savedSps.profileIdc = 100;
        doProfileSpecificSpsPatching(savedSps);

        ByteBuffer escapedNalu = H264Utils.writeSPS(savedSps, 128);
        byte[] result = new byte[5 + escapedNalu.limit()];

        // Write the Annex B header
        result[0] = 0x00;
        result[1] = 0x00;
        result[2] = 0x00;
        result[3] = 0x01;
        result[4] = 0x67;

        escapedNalu.get(result, 5, escapedNalu.limit());

        savedSps = null;
        return result;
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

    public boolean isNeedsBaselineSpsHack() {
        return needsBaselineSpsHack;
    }

    public void setNeedsBaselineSpsHack(boolean needsBaselineSpsHack) {
        this.needsBaselineSpsHack = needsBaselineSpsHack;
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

