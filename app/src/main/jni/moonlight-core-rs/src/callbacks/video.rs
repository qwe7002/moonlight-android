//! Video Decoder Callbacks
//!
//! This module provides callback functions for video decoding that bridge
//! between moonlight-common-c and the JNI layer.

#![allow(static_mut_refs)]

use crate::ffi::*;
use crate::jni_helpers::*;
use libc::{c_int, c_void};
use std::ptr;
use log::{info, error, debug};

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
// Static Callback Structure
// ============================================================================

pub static VIDEO_CALLBACKS: DECODER_RENDERER_CALLBACKS = DECODER_RENDERER_CALLBACKS {
    setup: Some(bridge_dr_setup),
    start: Some(bridge_dr_start),
    stop: Some(bridge_dr_stop),
    cleanup: Some(bridge_dr_cleanup),
    submitDecodeUnit: Some(bridge_dr_submit_decode_unit),
    capabilities: 0, // Will be set at runtime
};

