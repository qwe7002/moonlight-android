//! Callback implementations for video/audio rendering and connection status
//!
//! Ported from callbacks.c
//!
//! This module uses Rust's audiopus crate for Opus decoding instead of C libopus.

use crate::ffi::*;
use crate::opus::{OpusConfig, ThreadSafeOpusDecoder};
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JShortArray, JValue};
use jni::sys::{jbyte, jchar, jlong, jshort};
use jni::JNIEnv;
use jni::JavaVM;
use libc::{c_char, c_int, c_void};
use log::{error, info};
use once_cell::sync::OnceCell;
use std::sync::Mutex;

// Global state for JNI callbacks
static JVM: OnceCell<JavaVM> = OnceCell::new();
static BRIDGE_CLASS_NAME: &str = "com/limelight/nvstream/jni/MoonBridge";

// Opus decoder state - now using Rust implementation
static OPUS_DECODER: OnceCell<ThreadSafeOpusDecoder> = OnceCell::new();

// Decoded buffers
static DECODED_FRAME_BUFFER: OnceCell<Mutex<Option<GlobalRef>>> = OnceCell::new();
static DECODED_AUDIO_BUFFER: OnceCell<Mutex<Option<GlobalRef>>> = OnceCell::new();

/// Initialize the callback system with JNI environment
pub fn init_callbacks(env: &mut JNIEnv, _class: &JClass) -> Result<(), jni::errors::Error> {
    // Store JavaVM
    let jvm = env.get_java_vm()?;
    let _ = JVM.set(jvm);

    // Initialize Opus decoder container
    let _ = OPUS_DECODER.set(ThreadSafeOpusDecoder::new());

    // Initialize buffer containers
    let _ = DECODED_FRAME_BUFFER.set(Mutex::new(None));
    let _ = DECODED_AUDIO_BUFFER.set(Mutex::new(None));

    info!("Callbacks initialized successfully (Rust Opus)");
    Ok(())
}

/// Get thread-local JNI environment
fn get_thread_env() -> Option<JNIEnv<'static>> {
    let jvm = JVM.get()?;

    // Try to get existing env
    if let Ok(env) = jvm.get_env() {
        return Some(unsafe { std::mem::transmute(env) });
    }

    // Attach current thread
    match jvm.attach_current_thread_permanently() {
        Ok(env) => Some(unsafe { std::mem::transmute(env) }),
        Err(e) => {
            error!("Failed to attach thread: {:?}", e);
            None
        }
    }
}

// ============================================================================
// Video Decoder Callbacks
// ============================================================================

/// Video decoder setup callback
pub unsafe extern "C" fn bridge_dr_setup(
    video_format: c_int,
    width: c_int,
    height: c_int,
    redraw_rate: c_int,
    _context: *mut c_void,
    _dr_flags: c_int,
) -> c_int {
    let Some(mut env) = get_thread_env() else {
        return -1;
    };

    let result = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeDrSetup",
        "(IIII)I",
        &[
            JValue::Int(video_format),
            JValue::Int(width),
            JValue::Int(height),
            JValue::Int(redraw_rate),
        ],
    );

    match result {
        Ok(val) => {
            let err = val.i().unwrap_or(-1);
            if err != 0 {
                return err;
            }

            // Allocate frame buffer (32K initial size)
            if let Ok(buffer) = env.new_byte_array(32768) {
                if let Ok(global_ref) = env.new_global_ref(&buffer) {
                    if let Some(buf_lock) = DECODED_FRAME_BUFFER.get() {
                        if let Ok(mut guard) = buf_lock.lock() {
                            *guard = Some(global_ref);
                        }
                    }
                }
            }
            0
        }
        Err(e) => {
            error!("bridgeDrSetup failed: {:?}", e);
            -1
        }
    }
}

/// Video decoder start callback
pub unsafe extern "C" fn bridge_dr_start() {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeDrStart", "()V", &[]);
}

/// Video decoder stop callback
pub unsafe extern "C" fn bridge_dr_stop() {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeDrStop", "()V", &[]);
}

/// Video decoder cleanup callback
pub unsafe extern "C" fn bridge_dr_cleanup() {
    if let Some(buf_lock) = DECODED_FRAME_BUFFER.get() {
        if let Ok(mut guard) = buf_lock.lock() {
            *guard = None;
        }
    }

    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeDrCleanup", "()V", &[]);
}

/// Video decoder submit decode unit callback
pub unsafe extern "C" fn bridge_dr_submit_decode_unit(decode_unit: *mut DECODE_UNIT) -> c_int {
    if decode_unit.is_null() {
        return DR_NEED_IDR;
    }

    let du = &*decode_unit;
    let Some(mut env) = get_thread_env() else {
        return DR_NEED_IDR;
    };
    let Some(buf_lock) = DECODED_FRAME_BUFFER.get() else {
        return DR_NEED_IDR;
    };

    let mut guard = match buf_lock.lock() {
        Ok(g) => g,
        Err(_) => return DR_NEED_IDR,
    };

    let buffer_ref = match guard.as_ref() {
        Some(r) => r,
        None => return DR_NEED_IDR,
    };

    let buffer: JByteArray = JByteArray::from(JObject::from_raw(buffer_ref.as_obj().as_raw()));
    let current_len = env.get_array_length(&buffer).unwrap_or(0);

    if current_len < du.fullLength {
        drop(buffer);
        if let Ok(new_buffer) = env.new_byte_array(du.fullLength) {
            if let Ok(new_ref) = env.new_global_ref(&new_buffer) {
                *guard = Some(new_ref);
            }
        }
    }

    let buffer_ref = match guard.as_ref() {
        Some(r) => r,
        None => return DR_NEED_IDR,
    };
    let buffer: JByteArray = JByteArray::from(JObject::from_raw(buffer_ref.as_obj().as_raw()));

    let mut current_entry = du.bufferList;
    let mut offset = 0i32;

    while !current_entry.is_null() {
        let entry = &*current_entry;

        if entry.bufferType != BUFFER_TYPE_PICDATA {
            let slice = std::slice::from_raw_parts(entry.data as *const i8, entry.length as usize);
            if env.set_byte_array_region(&buffer, 0, slice).is_err() {
                return DR_NEED_IDR;
            }

            let result = env.call_static_method(
                BRIDGE_CLASS_NAME,
                "bridgeDrSubmitDecodeUnit",
                "([BIIIICJJ)I",
                &[
                    JValue::Object(&buffer),
                    JValue::Int(entry.length),
                    JValue::Int(entry.bufferType),
                    JValue::Int(du.frameNumber),
                    JValue::Int(du.frameType),
                    JValue::Char(du.frameHostProcessingLatency as jchar),
                    JValue::Long(du.receiveTimeUs as jlong),
                    JValue::Long(du.enqueueTimeUs as jlong),
                ],
            );

            if env.exception_check().unwrap_or(true) {
                return DR_OK;
            }

            if let Ok(val) = result {
                let ret = val.i().unwrap_or(DR_NEED_IDR);
                if ret != DR_OK {
                    return ret;
                }
            }
        } else {
            let slice = std::slice::from_raw_parts(entry.data as *const i8, entry.length as usize);
            if env.set_byte_array_region(&buffer, offset, slice).is_err() {
                return DR_NEED_IDR;
            }
            offset += entry.length;
        }

        current_entry = entry.next;
    }

    let result = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeDrSubmitDecodeUnit",
        "([BIIIICJJ)I",
        &[
            JValue::Object(&buffer),
            JValue::Int(offset),
            JValue::Int(BUFFER_TYPE_PICDATA),
            JValue::Int(du.frameNumber),
            JValue::Int(du.frameType),
            JValue::Char(du.frameHostProcessingLatency as jchar),
            JValue::Long(du.receiveTimeUs as jlong),
            JValue::Long(du.enqueueTimeUs as jlong),
        ],
    );

    if env.exception_check().unwrap_or(true) {
        return DR_OK;
    }

    result.map(|v| v.i().unwrap_or(DR_OK)).unwrap_or(DR_OK)
}

// ============================================================================
// Audio Renderer Callbacks - Using Rust Opus Decoder
// ============================================================================

/// Audio renderer init callback
pub unsafe extern "C" fn bridge_ar_init(
    audio_configuration: c_int,
    opus_config: *const OPUS_MULTISTREAM_CONFIGURATION,
    _context: *mut c_void,
    _ar_flags: c_int,
) -> c_int {
    if opus_config.is_null() {
        return -1;
    }

    let config = &*opus_config;
    let Some(mut env) = get_thread_env() else {
        return -1;
    };

    let result = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeArInit",
        "(III)I",
        &[
            JValue::Int(audio_configuration),
            JValue::Int(config.sampleRate),
            JValue::Int(config.samplesPerFrame),
        ],
    );

    let err = match result {
        Ok(val) => val.i().unwrap_or(-1),
        Err(_) => -1,
    };

    if env.exception_check().unwrap_or(true) || err != 0 {
        return if err != 0 { err } else { -1 };
    }

    // Create Rust Opus decoder
    let Some(opus_decoder) = OPUS_DECODER.get() else {
        return -1;
    };

    let rust_config = OpusConfig {
        sample_rate: config.sampleRate,
        channel_count: config.channelCount,
        streams: config.streams,
        coupled_streams: config.coupledStreams,
        samples_per_frame: config.samplesPerFrame,
        mapping: config.mapping,
    };

    if let Err(e) = opus_decoder.initialize(&rust_config) {
        error!("Failed to create Opus decoder: {}", e);
        let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeArCleanup", "()V", &[]);
        return -1;
    }

    info!(
        "Opus decoder initialized: {}Hz, {} channels, {} samples/frame",
        config.sampleRate, config.channelCount, config.samplesPerFrame
    );

    // Allocate audio buffer
    let buffer_size = config.channelCount * config.samplesPerFrame;
    if let Ok(buffer) = env.new_short_array(buffer_size) {
        if let Ok(global_ref) = env.new_global_ref(&buffer) {
            if let Some(buf_lock) = DECODED_AUDIO_BUFFER.get() {
                if let Ok(mut guard) = buf_lock.lock() {
                    *guard = Some(global_ref);
                }
            }
        }
    }

    0
}

/// Audio renderer start callback
pub unsafe extern "C" fn bridge_ar_start() {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeArStart", "()V", &[]);
}

/// Audio renderer stop callback
pub unsafe extern "C" fn bridge_ar_stop() {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeArStop", "()V", &[]);
}

/// Audio renderer cleanup callback
pub unsafe extern "C" fn bridge_ar_cleanup() {
    // Destroy Rust Opus decoder
    if let Some(opus_decoder) = OPUS_DECODER.get() {
        opus_decoder.destroy();
    }

    // Release audio buffer
    if let Some(buf_lock) = DECODED_AUDIO_BUFFER.get() {
        if let Ok(mut guard) = buf_lock.lock() {
            *guard = None;
        }
    }

    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeArCleanup", "()V", &[]);
}

/// Audio renderer decode and play sample callback - Using Rust Opus
pub unsafe extern "C" fn bridge_ar_decode_and_play_sample(
    sample_data: *mut c_char,
    sample_length: c_int,
) {
    if sample_data.is_null() || sample_length <= 0 {
        return;
    }

    let Some(mut env) = get_thread_env() else {
        return;
    };
    let Some(opus_decoder) = OPUS_DECODER.get() else {
        return;
    };
    let Some(buf_lock) = DECODED_AUDIO_BUFFER.get() else {
        return;
    };

    let config = opus_decoder.get_config();
    let output_size = (config.channel_count * config.samples_per_frame) as usize;

    // Decode using Rust Opus decoder
    let input_data = std::slice::from_raw_parts(sample_data as *const u8, sample_length as usize);
    let mut decoded_samples = vec![0i16; output_size];

    let decode_result = opus_decoder.decode(input_data, &mut decoded_samples);

    let decode_len = match decode_result {
        Ok(samples) => samples,
        Err(e) => {
            error!("Opus decode error: {}", e);
            return;
        }
    };

    if decode_len <= 0 {
        return;
    }

    // Get the Java buffer and copy decoded data
    let guard = match buf_lock.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let buffer_ref = match guard.as_ref() {
        Some(r) => r,
        None => return,
    };

    let buffer: JShortArray = JShortArray::from(JObject::from_raw(buffer_ref.as_obj().as_raw()));

    // Copy decoded samples to Java array
    let samples_to_copy = (decode_len * config.channel_count as usize).min(output_size);
    if env
        .set_short_array_region(&buffer, 0, &decoded_samples[..samples_to_copy])
        .is_err()
    {
        error!("Failed to copy decoded audio to Java array");
        return;
    }

    drop(guard);

    // Call Java to play the sample
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeArPlaySample",
        "([S)V",
        &[JValue::Object(&buffer)],
    );

    if env.exception_check().unwrap_or(false) {
        error!("Exception in audio playback");
    }
}

// ============================================================================
// Connection Listener Callbacks
// ============================================================================

pub unsafe extern "C" fn bridge_cl_stage_starting(stage: c_int) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClStageStarting",
        "(I)V",
        &[JValue::Int(stage)],
    );
}

pub unsafe extern "C" fn bridge_cl_stage_complete(stage: c_int) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClStageComplete",
        "(I)V",
        &[JValue::Int(stage)],
    );
}

pub unsafe extern "C" fn bridge_cl_stage_failed(stage: c_int, error_code: c_int) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClStageFailed",
        "(II)V",
        &[JValue::Int(stage), JValue::Int(error_code)],
    );
}

pub unsafe extern "C" fn bridge_cl_connection_started() {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(BRIDGE_CLASS_NAME, "bridgeClConnectionStarted", "()V", &[]);
}

pub unsafe extern "C" fn bridge_cl_connection_terminated(error_code: c_int) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClConnectionTerminated",
        "(I)V",
        &[JValue::Int(error_code)],
    );
}

pub unsafe extern "C" fn bridge_cl_rumble(
    controller_number: u16,
    low_freq_motor: u16,
    high_freq_motor: u16,
) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClRumble",
        "(SSS)V",
        &[
            JValue::Short(controller_number as jshort),
            JValue::Short(low_freq_motor as jshort),
            JValue::Short(high_freq_motor as jshort),
        ],
    );
}

pub unsafe extern "C" fn bridge_cl_connection_status_update(connection_status: c_int) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClConnectionStatusUpdate",
        "(I)V",
        &[JValue::Int(connection_status)],
    );
}

pub unsafe extern "C" fn bridge_cl_set_hdr_mode(enabled: bool) {
    let Some(mut env) = get_thread_env() else {
        return;
    };

    let hdr_metadata_array: JObject = if enabled {
        let mut metadata = SS_HDR_METADATA {
            displayPrimaries: [[0; 2]; 3],
            whitePoint: [0; 2],
            maxDisplayMasteringLuminance: 0,
            minDisplayMasteringLuminance: 0,
            maxContentLightLevel: 0,
            maxFrameAverageLightLevel: 0,
        };

        if LiGetHdrMetadata(&mut metadata) {
            let size = std::mem::size_of::<SS_HDR_METADATA>();
            if let Ok(arr) = env.new_byte_array(size as i32) {
                let bytes =
                    std::slice::from_raw_parts(&metadata as *const _ as *const i8, size);
                let _ = env.set_byte_array_region(&arr, 0, bytes);
                arr.into()
            } else {
                JObject::null()
            }
        } else {
            JObject::null()
        }
    } else {
        JObject::null()
    };

    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClSetHdrMode",
        "(Z[B)V",
        &[
            JValue::Bool(enabled as jni::sys::jboolean),
            JValue::Object(&hdr_metadata_array),
        ],
    );
}

pub unsafe extern "C" fn bridge_cl_rumble_triggers(
    controller_number: u16,
    left_trigger: u16,
    right_trigger: u16,
) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClRumbleTriggers",
        "(SSS)V",
        &[
            JValue::Short(controller_number as jshort),
            JValue::Short(left_trigger as jshort),
            JValue::Short(right_trigger as jshort),
        ],
    );
}

pub unsafe extern "C" fn bridge_cl_set_motion_event_state(
    controller_number: u16,
    motion_type: u8,
    report_rate_hz: u16,
) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClSetMotionEventState",
        "(SBS)V",
        &[
            JValue::Short(controller_number as jshort),
            JValue::Byte(motion_type as jbyte),
            JValue::Short(report_rate_hz as jshort),
        ],
    );
}

pub unsafe extern "C" fn bridge_cl_set_controller_led(controller_number: u16, r: u8, g: u8, b: u8) {
    let Some(mut env) = get_thread_env() else {
        return;
    };
    let _ = env.call_static_method(
        BRIDGE_CLASS_NAME,
        "bridgeClSetControllerLED",
        "(SBBB)V",
        &[
            JValue::Short(controller_number as jshort),
            JValue::Byte(r as jbyte),
            JValue::Byte(g as jbyte),
            JValue::Byte(b as jbyte),
        ],
    );
}

// ============================================================================
// Callback Structures for moonlight-common-c
// ============================================================================

/// Create video renderer callbacks structure
pub fn create_video_callbacks(capabilities: c_int) -> DECODER_RENDERER_CALLBACKS {
    DECODER_RENDERER_CALLBACKS {
        setup: Some(bridge_dr_setup),
        start: Some(bridge_dr_start),
        stop: Some(bridge_dr_stop),
        cleanup: Some(bridge_dr_cleanup),
        submitDecodeUnit: Some(bridge_dr_submit_decode_unit),
        capabilities,
    }
}

/// Create audio renderer callbacks structure
pub fn create_audio_callbacks() -> AUDIO_RENDERER_CALLBACKS {
    AUDIO_RENDERER_CALLBACKS {
        init: Some(bridge_ar_init),
        start: Some(bridge_ar_start),
        stop: Some(bridge_ar_stop),
        cleanup: Some(bridge_ar_cleanup),
        decodeAndPlaySample: Some(bridge_ar_decode_and_play_sample),
        capabilities: CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION,
    }
}

/// Create connection listener callbacks structure
pub fn create_connection_callbacks() -> CONNECTION_LISTENER_CALLBACKS {
    CONNECTION_LISTENER_CALLBACKS {
        stageStarting: Some(bridge_cl_stage_starting),
        stageComplete: Some(bridge_cl_stage_complete),
        stageFailed: Some(bridge_cl_stage_failed),
        connectionStarted: Some(bridge_cl_connection_started),
        connectionTerminated: Some(bridge_cl_connection_terminated),
        logMessage: None, // Varargs not supported in stable Rust
        rumble: Some(bridge_cl_rumble),
        connectionStatusUpdate: Some(bridge_cl_connection_status_update),
        setHdrMode: Some(bridge_cl_set_hdr_mode),
        rumbleTriggers: Some(bridge_cl_rumble_triggers),
        setMotionEventState: Some(bridge_cl_set_motion_event_state),
        setControllerLED: Some(bridge_cl_set_controller_led),
    }
}

