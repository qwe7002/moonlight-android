//! Opus decoder FFI bindings
//!
//! This module provides FFI declarations for the Opus multistream decoder.

use libc::{c_int, c_uchar};

/// Opus multistream decoder opaque type
#[repr(C)]
pub struct OpusMSDecoder {
    _private: [u8; 0],
}

// Opus decoder control request codes
/// Set decoder gain in Q8 dB units (-32768 to 32767)
#[allow(dead_code)]
pub const OPUS_SET_GAIN: c_int = 4034;
/// Get the duration of the last decoded packet in samples
#[allow(dead_code)]
pub const OPUS_GET_LAST_PACKET_DURATION: c_int = 4039;

// External C functions from libopus
#[link(name = "opus")]
extern "C" {
    /// Create a multistream decoder
    pub fn opus_multistream_decoder_create(
        sample_rate: c_int,
        channels: c_int,
        streams: c_int,
        coupled_streams: c_int,
        mapping: *const c_uchar,
        error: *mut c_int,
    ) -> *mut OpusMSDecoder;

    /// Decode a multistream packet
    pub fn opus_multistream_decode(
        st: *mut OpusMSDecoder,
        data: *const c_uchar,
        len: c_int,
        pcm: *mut i16,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;

    /// Destroy a multistream decoder
    pub fn opus_multistream_decoder_destroy(st: *mut OpusMSDecoder);

    /// Control decoder settings
    #[allow(dead_code)]
    pub fn opus_multistream_decoder_ctl(st: *mut OpusMSDecoder, request: c_int, ...) -> c_int;
}

