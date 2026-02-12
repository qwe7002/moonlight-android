//! Audio Renderer Callbacks
//!
//! This module provides callback functions for audio rendering that bridge
//! between moonlight-common-c and the JNI layer.

#![allow(static_mut_refs)]

use crate::ffi::*;
use crate::jni_helpers::*;
use crate::opus::*;
use libc::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use log::{info, error, debug};

// Global state for audio callbacks
static OPUS_DECODER: AtomicPtr<OpusMSDecoder> = AtomicPtr::new(ptr::null_mut());
static mut OPUS_CONFIG: Option<OPUS_MULTISTREAM_CONFIGURATION> = None;

pub extern "C" fn bridge_ar_init(
    audio_configuration: c_int,
    opus_config: *const OPUS_MULTISTREAM_CONFIGURATION,
    _context: *mut c_void,
    _flags: c_int,
) -> c_int {
    if opus_config.is_null() {
        return -1;
    }

    let config = unsafe { *opus_config };
    info!("Audio init: config={}, rate={}, channels={}, streams={}, coupled={}, samples_per_frame={}",
          audio_configuration, config.sampleRate, config.channelCount,
          config.streams, config.coupledStreams, config.samplesPerFrame);
    info!("Audio mapping: {:?}", &config.mapping[..config.channelCount as usize]);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return -1,
    };

    let method = get_ar_init_method();
    if method.is_null() {
        error!("AR init method not found");
        return -1;
    }

    let args = [
        JValue::int(audio_configuration),
        JValue::int(config.sampleRate),
        JValue::int(config.samplesPerFrame),
    ];

    let err = call_static_int_method(env, method, &args);
    if check_exception(env) {
        return -1;
    }

    if err != 0 {
        return err;
    }

    // Store config for later use
    unsafe {
        OPUS_CONFIG = Some(config);
    }

    // Create opus decoder
    let mut error: c_int = 0;
    let decoder = unsafe {
        opus_multistream_decoder_create(
            config.sampleRate,
            config.channelCount,
            config.streams,
            config.coupledStreams,
            config.mapping.as_ptr(),
            &mut error,
        )
    };

    if decoder.is_null() || error != 0 {
        error!("Failed to create Opus decoder: error={}", error);
        // Call cleanup on Java side
        let cleanup_method = get_ar_cleanup_method();
        if !cleanup_method.is_null() {
            call_static_void_method(env, cleanup_method, &[]);
        }
        return -1;
    }

    OPUS_DECODER.store(decoder, Ordering::SeqCst);

    // Pre-allocate the decoded audio buffer
    let buffer_size = config.channelCount * config.samplesPerFrame;
    let audio_buffer = new_short_array(env, buffer_size);
    if audio_buffer.is_null() {
        error!("Failed to create audio buffer");
        return -1;
    }
    let global_buffer = new_global_ref(env, audio_buffer);
    delete_local_ref(env, audio_buffer);
    set_decoded_audio_buffer(global_buffer);

    0
}

pub extern "C" fn bridge_ar_start() {
    debug!("Audio renderer start");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_ar_start_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_ar_stop() {
    debug!("Audio renderer stop");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_ar_stop_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_ar_cleanup() {
    debug!("Audio renderer cleanup");

    // Destroy opus decoder
    let decoder = OPUS_DECODER.swap(ptr::null_mut(), Ordering::SeqCst);
    if !decoder.is_null() {
        unsafe {
            opus_multistream_decoder_destroy(decoder);
        }
    }

    unsafe {
        OPUS_CONFIG = None;
    }

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    // Delete global audio buffer reference
    let buffer = get_decoded_audio_buffer();
    if !buffer.is_null() {
        delete_global_ref(env, buffer);
        set_decoded_audio_buffer(ptr::null_mut());
    }

    let method = get_ar_cleanup_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_ar_decode_and_play_sample(sample_data: *mut c_char, sample_length: c_int) {
    let decoder = OPUS_DECODER.load(Ordering::Acquire);
    if decoder.is_null() {
        return;
    }

    let config = unsafe {
        match OPUS_CONFIG.as_ref() {
            Some(c) => c,
            None => return,
        }
    };

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let audio_buffer = get_decoded_audio_buffer();
    if audio_buffer.is_null() {
        return;
    }

    // Use GetPrimitiveArrayCritical for direct access (same as original C code)
    let decoded_data = get_primitive_array_critical(env, audio_buffer) as *mut i16;
    if decoded_data.is_null() {
        return;
    }

    // When sample_data is NULL (sample_length == 0), this triggers Opus packet loss
    // concealment (PLC). The decoder will generate audio to fill the gap caused by
    // the missing packet. This is critical for smooth audio playback.
    let data_ptr = if sample_data.is_null() {
        ptr::null()
    } else {
        sample_data as *const u8
    };

    let decode_len = unsafe {
        opus_multistream_decode(
            decoder,
            data_ptr,
            sample_length,
            decoded_data,
            config.samplesPerFrame,
            0,
        )
    };

    if decode_len > 0 {

        // Release the array before making JNI calls (commit changes with mode 0)
        release_primitive_array_critical(env, audio_buffer, decoded_data as *mut c_void, 0);

        let method = get_ar_play_sample_method();
        if !method.is_null() {
            let args = [JValue::object(audio_buffer)];
            call_static_void_method(env, method, &args);
            if check_exception(env) {
                detach_current_thread();
            }
        }
    } else {
        error!("Opus decode failed: decode_len={}, sample_len={}", decode_len, sample_length);
        // Abort - don't copy back since no valid data
        release_primitive_array_critical(env, audio_buffer, decoded_data as *mut c_void, JNI_ABORT);
    }
}

// ============================================================================
// Static Callback Structure
// ============================================================================

pub static AUDIO_CALLBACKS: AUDIO_RENDERER_CALLBACKS = AUDIO_RENDERER_CALLBACKS {
    init: Some(bridge_ar_init),
    start: Some(bridge_ar_start),
    stop: Some(bridge_ar_stop),
    cleanup: Some(bridge_ar_cleanup),
    decodeAndPlaySample: Some(bridge_ar_decode_and_play_sample),
    capabilities: CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION,
};

