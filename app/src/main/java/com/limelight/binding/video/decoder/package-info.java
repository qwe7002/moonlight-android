/**
 * Video decoder components for MediaCodec-based video decoding.
 * <p>
 * This package contains the refactored components of the video decoder system:
 * <ul>
 *   <li>{@link com.limelight.binding.video.decoder.DecoderCapabilityChecker} - Decoder discovery and capability checking</li>
 *   <li>{@link com.limelight.binding.video.decoder.CodecRecoveryManager} - Codec exception handling and recovery</li>
 *   <li>{@link com.limelight.binding.video.decoder.CsdBufferProcessor} - VPS/SPS/PPS processing</li>
 *   <li>{@link com.limelight.binding.video.decoder.PerformanceStatsManager} - Video statistics collection</li>
 *   <li>{@link com.limelight.binding.video.decoder.DecoderExceptions} - Custom exception classes</li>
 * </ul>
 *
 * @see com.limelight.binding.video.MediaCodecDecoderRenderer
 */
package com.limelight.binding.video.decoder;

