//! Callback implementations for moonlight-common-c
//!
//! This module provides callback functions for video decoding, audio rendering,
//! and connection events that bridge between moonlight-common-c and the JNI layer.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use crate::ffi::*;
use crate::jni_helpers::*;
use crate::opus::*;
use libc::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use log::{info, error, debug};

// Global state for callbacks
static OPUS_DECODER: AtomicPtr<OpusMSDecoder> = AtomicPtr::new(ptr::null_mut());
static mut OPUS_CONFIG: Option<OPUS_MULTISTREAM_CONFIGURATION> = None;

// Flag to indicate if JNI callbacks are enabled
static JNI_CALLBACKS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable JNI callbacks
#[inline]
pub fn set_jni_callbacks(enabled: bool) {
    JNI_CALLBACKS_ENABLED.store(enabled, Ordering::Release);
    info!("JNI callbacks {}", if enabled { "enabled" } else { "disabled" });
}

/// Check if JNI callbacks are enabled
#[inline]
pub fn jni_callbacks_enabled() -> bool {
    JNI_CALLBACKS_ENABLED.load(Ordering::Acquire)
}

// ============================================================================
// Video Decoder Callbacks
// ============================================================================

pub extern "C" fn bridge_dr_setup(
    video_format: c_int,
    width: c_int,
    height: c_int,
    redraw_rate: c_int,
    _context: *mut c_void,
    _dr_flags: c_int,
) -> c_int {
    info!("Video decoder setup: format={}, {}x{} @ {}Hz", video_format, width, height, redraw_rate);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return -1,
    };

    let method = get_dr_setup_method();
    if method.is_null() {
        error!("DR setup method not found");
        return -1;
    }

    let args = [
        JValue::int(video_format),
        JValue::int(width),
        JValue::int(height),
        JValue::int(redraw_rate),
    ];

    let err = call_static_int_method(env, method, &args);
    if check_exception(env) {
        return -1;
    }

    if err != 0 {
        return err;
    }

    // Create a 32K frame buffer that will increase if needed
    let buffer = new_byte_array(env, 32768);
    if buffer.is_null() {
        return -1;
    }
    let global_buffer = new_global_ref(env, buffer);
    delete_local_ref(env, buffer);
    set_decoded_frame_buffer(global_buffer);

    0
}

pub extern "C" fn bridge_dr_start() {
    debug!("Video decoder start");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_dr_start_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_dr_stop() {
    debug!("Video decoder stop");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_dr_stop_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_dr_cleanup() {
    debug!("Video decoder cleanup");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    // Delete global frame buffer reference
    let buffer = get_decoded_frame_buffer();
    if !buffer.is_null() {
        delete_global_ref(env, buffer);
        set_decoded_frame_buffer(ptr::null_mut());
    }

    let method = get_dr_cleanup_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_dr_submit_decode_unit(decode_unit: *mut DECODE_UNIT) -> c_int {
    if decode_unit.is_null() {
        return DR_OK;
    }

    let env = match get_thread_env() {
        Some(e) => e,
        None => return DR_OK,
    };

    let method = get_dr_submit_decode_unit_method();
    if method.is_null() {
        return DR_OK;
    }

    let du = unsafe { &*decode_unit };
    let frame_buffer = get_decoded_frame_buffer();

    if frame_buffer.is_null() {
        return DR_OK;
    }

    // Increase the size of our frame data buffer if our frame won't fit
    let current_length = get_array_length(env, frame_buffer);
    if current_length < du.fullLength {
        delete_global_ref(env, frame_buffer);
        let new_buffer = new_byte_array(env, du.fullLength);
        if new_buffer.is_null() {
            return DR_OK;
        }
        let global_buffer = new_global_ref(env, new_buffer);
        delete_local_ref(env, new_buffer);
        set_decoded_frame_buffer(global_buffer);
    }

    let frame_buffer = get_decoded_frame_buffer();

    let mut current_entry = du.bufferList;
    let mut offset: c_int = 0;

    while !current_entry.is_null() {
        let entry = unsafe { &*current_entry };

        // Submit parameter set NALUs separately from picture data
        if entry.bufferType != BUFFER_TYPE_PICDATA {
            // Use the beginning of the buffer each time since this is a separate
            // invocation of the decoder each time.
            set_byte_array_region(env, frame_buffer, 0, entry.length, entry.data as *const i8);

            let args = [
                JValue::object(frame_buffer),
                JValue::int(entry.length),
                JValue::int(entry.bufferType),
                JValue::int(du.frameNumber),
                JValue::int(du.frameType),
                JValue::char(du.frameHostProcessingLatency),
                JValue::long(du.receiveTimeUs as i64),
                JValue::long(du.enqueueTimeUs as i64),
            ];

            let ret = call_static_int_method(env, method, &args);
            if check_exception(env) {
                detach_current_thread();
                return DR_OK;
            } else if ret != DR_OK {
                return ret;
            }
        } else {
            set_byte_array_region(env, frame_buffer, offset, entry.length, entry.data as *const i8);
            offset += entry.length;
        }

        current_entry = entry.next;
    }

    // Submit the picture data
    let args = [
        JValue::object(frame_buffer),
        JValue::int(offset),
        JValue::int(BUFFER_TYPE_PICDATA),
        JValue::int(du.frameNumber),
        JValue::int(du.frameType),
        JValue::char(du.frameHostProcessingLatency),
        JValue::long(du.receiveTimeUs as i64),
        JValue::long(du.enqueueTimeUs as i64),
    ];

    let ret = call_static_int_method(env, method, &args);
    if check_exception(env) {
        detach_current_thread();
        return DR_OK;
    }

    ret
}

// ============================================================================
// Audio Renderer Callbacks
// ============================================================================

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
// Connection Listener Callbacks
// ============================================================================

pub extern "C" fn bridge_cl_stage_starting(stage: c_int) {
    debug!("Connection stage starting: {}", stage);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_stage_starting_method();
    if !method.is_null() {
        let args = [JValue::int(stage)];
        call_static_void_method(env, method, &args);
    }
}

pub extern "C" fn bridge_cl_stage_complete(stage: c_int) {
    debug!("Connection stage complete: {}", stage);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_stage_complete_method();
    if !method.is_null() {
        let args = [JValue::int(stage)];
        call_static_void_method(env, method, &args);
    }
}

pub extern "C" fn bridge_cl_stage_failed(stage: c_int, error_code: c_int) {
    error!("Connection stage failed: stage={}, error={}", stage, error_code);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_stage_failed_method();
    if !method.is_null() {
        let args = [JValue::int(stage), JValue::int(error_code)];
        call_static_void_method(env, method, &args);
    }
}

pub extern "C" fn bridge_cl_connection_started() {
    info!("Connection started successfully");

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_connection_started_method();
    if !method.is_null() {
        call_static_void_method(env, method, &[]);
    }
}

pub extern "C" fn bridge_cl_connection_terminated(error_code: c_int) {
    error!("Connection terminated: error={}", error_code);

    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_connection_terminated_method();
    if !method.is_null() {
        let args = [JValue::int(error_code)];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

pub extern "C" fn bridge_cl_rumble(
    controller_number: libc::c_ushort,
    low_freq_motor: libc::c_ushort,
    high_freq_motor: libc::c_ushort,
) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_rumble_method();
    if !method.is_null() {
        // The seemingly redundant short casts are required to convert unsigned short to signed short
        let args = [
            JValue::short(controller_number as i16),
            JValue::short(low_freq_motor as i16),
            JValue::short(high_freq_motor as i16),
        ];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

pub extern "C" fn bridge_cl_connection_status_update(connection_status: c_int) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_connection_status_update_method();
    if !method.is_null() {
        let args = [JValue::int(connection_status)];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

pub extern "C" fn bridge_cl_set_hdr_mode(enabled: bool) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_set_hdr_mode_method();
    if method.is_null() {
        return;
    }

    // Check if HDR metadata was provided
    let hdr_metadata_array: JByteArray = if enabled {
        let mut hdr_metadata = SS_HDR_METADATA::default();
        let has_metadata = unsafe { LiGetHdrMetadata(&mut hdr_metadata) };

        if has_metadata {
            let metadata_size = std::mem::size_of::<SS_HDR_METADATA>() as i32;
            let array = new_byte_array(env, metadata_size);
            if !array.is_null() {
                set_byte_array_region(
                    env,
                    array,
                    0,
                    metadata_size,
                    &hdr_metadata as *const _ as *const i8,
                );
                array
            } else {
                ptr::null_mut()
            }
        } else {
            ptr::null_mut()
        }
    } else {
        ptr::null_mut()
    };

    let args = [
        JValue::boolean(enabled),
        JValue::object(hdr_metadata_array),
    ];
    call_static_void_method(env, method, &args);

    // Clean up local reference
    if !hdr_metadata_array.is_null() {
        delete_local_ref(env, hdr_metadata_array);
    }

    if check_exception(env) {
        detach_current_thread();
    }
}

pub extern "C" fn bridge_cl_rumble_triggers(
    controller_number: libc::c_ushort,
    left_trigger: libc::c_ushort,
    right_trigger: libc::c_ushort,
) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_rumble_triggers_method();
    if !method.is_null() {
        let args = [
            JValue::short(controller_number as i16),
            JValue::short(left_trigger as i16),
            JValue::short(right_trigger as i16),
        ];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

pub extern "C" fn bridge_cl_set_motion_event_state(
    controller_number: libc::c_ushort,
    motion_type: libc::c_uchar,
    report_rate_hz: libc::c_ushort,
) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_set_motion_event_state_method();
    if !method.is_null() {
        let args = [
            JValue::short(controller_number as i16),
            JValue::byte(motion_type as i8),
            JValue::short(report_rate_hz as i16),
        ];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

pub extern "C" fn bridge_cl_set_controller_led(
    controller_number: libc::c_ushort,
    r: libc::c_uchar,
    g: libc::c_uchar,
    b: libc::c_uchar,
) {
    let env = match get_thread_env() {
        Some(e) => e,
        None => return,
    };

    let method = get_cl_set_controller_led_method();
    if !method.is_null() {
        // These jbyte casts are necessary to satisfy CheckJNI
        let args = [
            JValue::short(controller_number as i16),
            JValue::byte(r as i8),
            JValue::byte(g as i8),
            JValue::byte(b as i8),
        ];
        call_static_void_method(env, method, &args);
        if check_exception(env) {
            detach_current_thread();
        }
    }
}

// ============================================================================
// Static Callback Structures
// ============================================================================

pub static VIDEO_CALLBACKS: DECODER_RENDERER_CALLBACKS = DECODER_RENDERER_CALLBACKS {
    setup: Some(bridge_dr_setup),
    start: Some(bridge_dr_start),
    stop: Some(bridge_dr_stop),
    cleanup: Some(bridge_dr_cleanup),
    submitDecodeUnit: Some(bridge_dr_submit_decode_unit),
    capabilities: 0, // Will be set at runtime
};

pub static AUDIO_CALLBACKS: AUDIO_RENDERER_CALLBACKS = AUDIO_RENDERER_CALLBACKS {
    init: Some(bridge_ar_init),
    start: Some(bridge_ar_start),
    stop: Some(bridge_ar_stop),
    cleanup: Some(bridge_ar_cleanup),
    decodeAndPlaySample: Some(bridge_ar_decode_and_play_sample),
    capabilities: CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION,
};

pub static CONNECTION_CALLBACKS: CONNECTION_LISTENER_CALLBACKS = CONNECTION_LISTENER_CALLBACKS {
    stageStarting: Some(bridge_cl_stage_starting),
    stageComplete: Some(bridge_cl_stage_complete),
    stageFailed: Some(bridge_cl_stage_failed),
    connectionStarted: Some(bridge_cl_connection_started),
    connectionTerminated: Some(bridge_cl_connection_terminated),
    logMessage: None, // C variadic functions not supported in stable Rust
    rumble: Some(bridge_cl_rumble),
    connectionStatusUpdate: Some(bridge_cl_connection_status_update),
    setHdrMode: Some(bridge_cl_set_hdr_mode),
    rumbleTriggers: Some(bridge_cl_rumble_triggers),
    setMotionEventState: Some(bridge_cl_set_motion_event_state),
    setControllerLED: Some(bridge_cl_set_controller_led),
    setAdaptiveTriggers: None, // Not implemented yet
};

/// Check if platform has fast AES support
/// This is a simplified version - on aarch64 we assume hardware AES is available
pub fn has_fast_aes() -> bool {
    // On arm64-v8a (aarch64), most modern devices have hardware AES
    // This is a conservative implementation that returns true for arm64
    #[cfg(target_arch = "aarch64")]
    {
        true
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}
