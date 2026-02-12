//! Opus multistream decoder wrapper using direct FFI bindings
//!
//! This module provides Opus decoding functionality using direct FFI bindings to libopus,
//! which is compiled using the cc crate in build.rs.

use libc::{c_int, c_uchar};
use log::error;
use std::ptr;
use std::sync::Mutex;

// Opus error codes
const OPUS_OK: c_int = 0;
#[allow(dead_code)]
const OPUS_BAD_ARG: c_int = -1;
#[allow(dead_code)]
const OPUS_BUFFER_TOO_SMALL: c_int = -2;
#[allow(dead_code)]
const OPUS_INTERNAL_ERROR: c_int = -3;
#[allow(dead_code)]
const OPUS_INVALID_PACKET: c_int = -4;
#[allow(dead_code)]
const OPUS_UNIMPLEMENTED: c_int = -5;
#[allow(dead_code)]
const OPUS_INVALID_STATE: c_int = -6;
#[allow(dead_code)]
const OPUS_ALLOC_FAIL: c_int = -7;

// Opaque decoder type
#[repr(C)]
pub struct OpusMSDecoder {
    _private: [u8; 0],
}

// FFI bindings to libopus
extern "C" {
    fn opus_multistream_decoder_create(
        fs: c_int,
        channels: c_int,
        streams: c_int,
        coupled_streams: c_int,
        mapping: *const c_uchar,
        error: *mut c_int,
    ) -> *mut OpusMSDecoder;

    fn opus_multistream_decoder_destroy(st: *mut OpusMSDecoder);

    fn opus_multistream_decode(
        st: *mut OpusMSDecoder,
        data: *const c_uchar,
        len: c_int,
        pcm: *mut i16,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
}

/// Opus multistream configuration matching the C structure
#[derive(Debug, Clone)]
pub struct OpusConfig {
    pub sample_rate: i32,
    pub channel_count: i32,
    pub streams: i32,
    pub coupled_streams: i32,
    pub samples_per_frame: i32,
    pub mapping: [u8; 8],
}

impl Default for OpusConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channel_count: 2,
            streams: 1,
            coupled_streams: 1,
            samples_per_frame: 240,
            mapping: [0, 1, 0, 0, 0, 0, 0, 0],
        }
    }
}

/// Opus multistream decoder using direct FFI to libopus
pub struct OpusMultistreamDecoder {
    decoder: *mut OpusMSDecoder,
    config: OpusConfig,
}

// Safety: OpusMSDecoder is thread-safe when accessed with proper synchronization
unsafe impl Send for OpusMultistreamDecoder {}

impl OpusMultistreamDecoder {
    /// Create a new multistream decoder
    pub fn new(config: &OpusConfig) -> Result<Self, String> {
        // Validate sample rate
        if ![8000, 12000, 16000, 24000, 48000].contains(&config.sample_rate) {
            return Err(format!("Unsupported sample rate: {}", config.sample_rate));
        }

        let mut error: c_int = 0;
        let decoder = unsafe {
            opus_multistream_decoder_create(
                config.sample_rate,
                config.channel_count,
                config.streams,
                config.coupled_streams,
                config.mapping.as_ptr(),
                &mut error,
            )
        };

        if error != OPUS_OK || decoder.is_null() {
            return Err(format!("Failed to create opus decoder, error code: {}", error));
        }

        Ok(Self {
            decoder,
            config: config.clone(),
        })
    }

    /// Decode opus data to PCM
    pub fn decode(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, String> {
        if self.decoder.is_null() {
            return Err("Decoder not initialized".to_string());
        }

        let frame_size = (output.len() / self.config.channel_count as usize) as c_int;

        let result = unsafe {
            opus_multistream_decode(
                self.decoder,
                data.as_ptr(),
                data.len() as c_int,
                output.as_mut_ptr(),
                frame_size,
                0, // decode_fec = false
            )
        };

        if result < 0 {
            error!("Opus decode error: {}", result);
            Err(format!("Decode error: {}", result))
        } else {
            Ok(result as usize)
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &OpusConfig {
        &self.config
    }
}

impl Drop for OpusMultistreamDecoder {
    fn drop(&mut self) {
        if !self.decoder.is_null() {
            unsafe {
                opus_multistream_decoder_destroy(self.decoder);
            }
            self.decoder = ptr::null_mut();
        }
    }
}

/// Thread-safe wrapper for OpusMultistreamDecoder
pub struct ThreadSafeOpusDecoder {
    inner: Mutex<Option<OpusMultistreamDecoder>>,
    config: Mutex<OpusConfig>,
}

impl ThreadSafeOpusDecoder {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            config: Mutex::new(OpusConfig::default()),
        }
    }

    pub fn initialize(&self, config: &OpusConfig) -> Result<(), String> {
        let decoder = OpusMultistreamDecoder::new(config)?;

        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(decoder);
        } else {
            return Err("Failed to lock decoder".to_string());
        }

        if let Ok(mut cfg_guard) = self.config.lock() {
            *cfg_guard = config.clone();
        }

        Ok(())
    }

    pub fn decode(&self, data: &[u8], output: &mut [i16]) -> Result<usize, String> {
        let mut guard = self.inner.lock().map_err(|_| "Lock poisoned")?;

        match guard.as_mut() {
            Some(decoder) => decoder.decode(data, output),
            None => Err("Decoder not initialized".to_string()),
        }
    }

    pub fn destroy(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }

    pub fn get_config(&self) -> OpusConfig {
        self.config.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl Default for ThreadSafeOpusDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_config_default() {
        let config = OpusConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channel_count, 2);
    }

    #[test]
    fn test_invalid_sample_rate() {
        let config = OpusConfig {
            sample_rate: 44100, // Invalid for Opus
            channel_count: 2,
            streams: 1,
            coupled_streams: 1,
            samples_per_frame: 240,
            mapping: [0, 1, 0, 0, 0, 0, 0, 0],
        };

        let decoder = OpusMultistreamDecoder::new(&config);
        assert!(decoder.is_err());
    }
}
