//! Connection Listener Callbacks
//!
//! This module provides callback functions for connection events that bridge
//! between moonlight-common-c and the JNI layer.

#![allow(static_mut_refs)]

use crate::ffi::*;
use crate::jni_helpers::*;
use libc::c_int;
use std::ptr;
use log::{info, error, debug};

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
// Static Callback Structure
// ============================================================================

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

