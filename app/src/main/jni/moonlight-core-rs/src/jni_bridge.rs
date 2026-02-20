//! JNI bridge implementations
//!
//! This module provides JNI function implementations that match the Java native method
//! declarations in MoonBridge.java, bridging between Java and moonlight-common-c.

use crate::callbacks::{
    has_fast_aes,
    bridge_dr_setup, bridge_dr_start, bridge_dr_stop, bridge_dr_cleanup, bridge_dr_submit_decode_unit,
    bridge_ar_init, bridge_ar_start, bridge_ar_stop, bridge_ar_cleanup, bridge_ar_decode_and_play_sample,
    bridge_cl_stage_starting, bridge_cl_stage_complete, bridge_cl_stage_failed,
    bridge_cl_connection_started, bridge_cl_connection_terminated, bridge_cl_rumble,
    bridge_cl_connection_status_update, bridge_cl_set_hdr_mode, bridge_cl_rumble_triggers,
    bridge_cl_set_motion_event_state, bridge_cl_set_controller_led,
    set_jni_callbacks,
};
use crate::ffi::*;
use crate::jni_helpers;
use libc::{c_char, c_void};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use log::{info, error, debug};

// JNI type definitions
pub type JNIEnv = *mut c_void;
pub type JClass = *mut c_void;
pub type JString = *mut c_void;
pub type JByteArray = *mut c_void;
pub type JShortArray = *mut c_void;
pub type JObject = *mut c_void;
pub type JMethodID = *mut c_void;
pub type JBoolean = u8;
pub type JByte = i8;
pub type JChar = u16;
pub type JShort = i16;
pub type JInt = i32;
pub type JLong = i64;
pub type JFloat = f32;
pub type JavaVM = *mut c_void;

// JNI constants
pub const JNI_FALSE: JBoolean = 0;
pub const JNI_TRUE: JBoolean = 1;
pub const JNI_VERSION_1_6: JInt = 0x00010006;

// Global JavaVM pointer
static JAVA_VM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// Global class and method IDs for callbacks
static MOON_BRIDGE_CLASS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Get the JavaVM pointer
pub fn get_java_vm() -> JavaVM {
    JAVA_VM.load(Ordering::SeqCst)
}

/// Get the MoonBridge class
pub fn get_moon_bridge_class() -> JClass {
    MOON_BRIDGE_CLASS.load(Ordering::SeqCst)
}


// ============================================================================
// JNI Native Method Implementations
// ============================================================================

/// Get JNIEnv for current thread (attaching if necessary)
pub fn get_jni_env() -> Option<JNIEnv> {
    jni_helpers::get_thread_env()
}

/// Initialize the MoonBridge native library
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_init(
    env: JNIEnv,
    clazz: JClass,
) {
    // Initialize Android logger
    #[cfg(target_os = "android")]
    {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Debug)
                .with_tag("moonlight-core-rs"),
        );
    }

    // Store JavaVM using jni_helpers
    if let Some(vm) = jni_helpers::get_java_vm_from_env(env) {
        JAVA_VM.store(vm, Ordering::Release);
        jni_helpers::set_java_vm(vm);
        debug!("JavaVM stored successfully");
    } else {
        error!("Failed to get JavaVM");
    }

    // Store MoonBridge class as global reference
    let global_class = jni_helpers::new_global_ref(env, clazz);
    if !global_class.is_null() {
        MOON_BRIDGE_CLASS.store(global_class, Ordering::Release);
        debug!("MoonBridge class stored successfully");
    } else {
        error!("Failed to create global reference for MoonBridge class");
    }

    // Initialize method IDs using jni_helpers
    jni_helpers::init_method_ids(env, clazz);


    // Set up callbacks to use JNI
    set_jni_callbacks(true);

    // Log initialization
    info!("Native library initialized (Rust)");
}


/// Send mouse move event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseMove(
    _env: JNIEnv,
    _clazz: JClass,
    delta_x: JShort,
    delta_y: JShort,
) {
    unsafe {
        LiSendMouseMoveEvent(delta_x, delta_y);
    }
}

/// Send mouse position event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMousePosition(
    _env: JNIEnv,
    _clazz: JClass,
    x: JShort,
    y: JShort,
    reference_width: JShort,
    reference_height: JShort,
) {
    unsafe {
        LiSendMousePositionEvent(x, y, reference_width, reference_height);
    }
}

/// Send mouse move as mouse position event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseMoveAsMousePosition(
    _env: JNIEnv,
    _clazz: JClass,
    delta_x: JShort,
    delta_y: JShort,
    reference_width: JShort,
    reference_height: JShort,
) {
    unsafe {
        LiSendMouseMoveAsMousePositionEvent(delta_x, delta_y, reference_width, reference_height);
    }
}

/// Send mouse button event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseButton(
    _env: JNIEnv,
    _clazz: JClass,
    button_event: JByte,
    mouse_button: JByte,
) {
    unsafe {
        LiSendMouseButtonEvent(button_event as u8, mouse_button as u8);
    }
}

/// Send multi-controller input event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMultiControllerInput(
    _env: JNIEnv,
    _clazz: JClass,
    controller_number: JShort,
    active_gamepad_mask: JShort,
    button_flags: JInt,
    left_trigger: JByte,
    right_trigger: JByte,
    left_stick_x: JShort,
    left_stick_y: JShort,
    right_stick_x: JShort,
    right_stick_y: JShort,
) {
    unsafe {
        LiSendMultiControllerEvent(
            controller_number,
            active_gamepad_mask,
            button_flags,
            left_trigger as u8,
            right_trigger as u8,
            left_stick_x,
            left_stick_y,
            right_stick_x,
            right_stick_y,
        );
    }
}

/// Send touch event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendTouchEvent(
    _env: JNIEnv,
    _clazz: JClass,
    event_type: JByte,
    pointer_id: JInt,
    x: JFloat,
    y: JFloat,
    pressure_or_distance: JFloat,
    contact_area_major: JFloat,
    contact_area_minor: JFloat,
    rotation: JShort,
) -> JInt {
    unsafe {
        LiSendTouchEvent(
            event_type as u8,
            pointer_id,
            x,
            y,
            pressure_or_distance,
            contact_area_major,
            contact_area_minor,
            rotation,
        )
    }
}

/// Send pen event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendPenEvent(
    _env: JNIEnv,
    _clazz: JClass,
    event_type: JByte,
    tool_type: JByte,
    pen_buttons: JByte,
    x: JFloat,
    y: JFloat,
    pressure_or_distance: JFloat,
    contact_area_major: JFloat,
    contact_area_minor: JFloat,
    rotation: JShort,
    tilt: JByte,
) -> JInt {
    unsafe {
        LiSendPenEvent(
            event_type as u8,
            tool_type as u8,
            pen_buttons as u8,
            x,
            y,
            pressure_or_distance,
            contact_area_major,
            contact_area_minor,
            rotation,
            tilt as u8,
        )
    }
}

/// Send controller arrival event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerArrivalEvent(
    _env: JNIEnv,
    _clazz: JClass,
    controller_number: JByte,
    active_gamepad_mask: JShort,
    controller_type: JByte,
    supported_button_flags: JInt,
    capabilities: JShort,
) -> JInt {
    unsafe {
        LiSendControllerArrivalEvent(
            controller_number as u8,
            active_gamepad_mask,
            controller_type as u8,
            supported_button_flags,
            capabilities,
        )
    }
}

/// Send controller touch event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerTouchEvent(
    _env: JNIEnv,
    _clazz: JClass,
    controller_number: JByte,
    event_type: JByte,
    pointer_id: JInt,
    x: JFloat,
    y: JFloat,
    pressure: JFloat,
) -> JInt {
    unsafe {
        LiSendControllerTouchEvent(
            controller_number as u8,
            event_type as u8,
            pointer_id,
            x,
            y,
            pressure,
        )
    }
}

/// Send controller motion event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerMotionEvent(
    _env: JNIEnv,
    _clazz: JClass,
    controller_number: JByte,
    motion_type: JByte,
    x: JFloat,
    y: JFloat,
    z: JFloat,
) -> JInt {
    unsafe {
        LiSendControllerMotionEvent(controller_number as u8, motion_type as u8, x, y, z)
    }
}

/// Send controller battery event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerBatteryEvent(
    _env: JNIEnv,
    _clazz: JClass,
    controller_number: JByte,
    battery_state: JByte,
    battery_percentage: JByte,
) -> JInt {
    unsafe {
        LiSendControllerBatteryEvent(
            controller_number as u8,
            battery_state as u8,
            battery_percentage as u8,
        )
    }
}

/// Send keyboard input
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendKeyboardInput(
    _env: JNIEnv,
    _clazz: JClass,
    key_code: JShort,
    key_action: JByte,
    modifiers: JByte,
    flags: JByte,
) {
    unsafe {
        LiSendKeyboardEvent2(key_code, key_action as u8, modifiers as u8, flags as u8);
    }
}

/// Send high resolution scroll event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseHighResScroll(
    _env: JNIEnv,
    _clazz: JClass,
    scroll_amount: JShort,
) {
    unsafe {
        LiSendHighResScrollEvent(scroll_amount);
    }
}

/// Send high resolution horizontal scroll event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseHighResHScroll(
    _env: JNIEnv,
    _clazz: JClass,
    scroll_amount: JShort,
) {
    unsafe {
        LiSendHighResHScrollEvent(scroll_amount);
    }
}

/// Send UTF-8 text event
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_sendUtf8Text(
    env: JNIEnv,
    _clazz: JClass,
    text: JString,
) {
    if text.is_null() {
        return;
    }

    // Get the UTF-8 string from Java
    let utf8_text = unsafe { jni_get_string_utf_chars(env, text) };
    if utf8_text.is_null() {
        return;
    }

    let c_str = unsafe { CStr::from_ptr(utf8_text) };
    let len = c_str.to_bytes().len();

    unsafe {
        LiSendUtf8TextEvent(utf8_text, len);
        jni_release_string_utf_chars(env, text, utf8_text);
    }
}

/// Stop connection
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_stopConnection(
    _env: JNIEnv,
    _clazz: JClass,
) {
    unsafe {
        LiStopConnection();
    }
}

/// Interrupt connection
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_interruptConnection(
    _env: JNIEnv,
    _clazz: JClass,
) {
    unsafe {
        LiInterruptConnection();
    }
}

/// Get stage name
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getStageName(
    env: JNIEnv,
    _clazz: JClass,
    stage: JInt,
) -> JString {
    let stage_name = unsafe { LiGetStageName(stage) };
    if stage_name.is_null() {
        return ptr::null_mut();
    }

    unsafe { jni_new_string_utf(env, stage_name) }
}

/// Find external IPv4 address using STUN
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_findExternalAddressIP4(
    env: JNIEnv,
    _clazz: JClass,
    stun_host_name: JString,
    stun_port: JInt,
) -> JString {
    if stun_host_name.is_null() {
        return ptr::null_mut();
    }

    let host_name_str = unsafe { jni_get_string_utf_chars(env, stun_host_name) };
    if host_name_str.is_null() {
        return ptr::null_mut();
    }

    let mut wan_addr: u32 = 0;
    let err = unsafe { LiFindExternalAddressIP4(host_name_str, stun_port, &mut wan_addr) };

    unsafe {
        jni_release_string_utf_chars(env, stun_host_name, host_name_str);
    }

    if err == 0 {
        // Convert IP to string
        let ip_bytes = wan_addr.to_ne_bytes();
        let ip_str = format!("{}.{}.{}.{}", ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);

        info!("Resolved WAN address to {}", ip_str);

        let c_str = CString::new(ip_str).unwrap_or_default();
        unsafe { jni_new_string_utf(env, c_str.as_ptr()) }
    } else {
        error!("STUN failed to get WAN address: {}", err);
        ptr::null_mut()
    }
}

/// Get pending audio duration
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getPendingAudioDuration(
    _env: JNIEnv,
    _clazz: JClass,
) -> JInt {
    unsafe { LiGetPendingAudioDuration() }
}

/// Get pending video frames
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getPendingVideoFrames(
    _env: JNIEnv,
    _clazz: JClass,
) -> JInt {
    unsafe { LiGetPendingVideoFrames() }
}

/// Test client connectivity
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_testClientConnectivity(
    env: JNIEnv,
    _clazz: JClass,
    test_server_host_name: JString,
    reference_port: JInt,
    test_flags: JInt,
) -> JInt {
    if test_server_host_name.is_null() {
        return -1;
    }

    let host_name_str = unsafe { jni_get_string_utf_chars(env, test_server_host_name) };
    if host_name_str.is_null() {
        return -1;
    }

    let ret = unsafe {
        LiTestClientConnectivity(host_name_str, reference_port as u16, test_flags)
    };

    unsafe {
        jni_release_string_utf_chars(env, test_server_host_name, host_name_str);
    }

    ret
}

/// Get port flags from stage
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getPortFlagsFromStage(
    _env: JNIEnv,
    _clazz: JClass,
    stage: JInt,
) -> JInt {
    unsafe { LiGetPortFlagsFromStage(stage) }
}

/// Get port flags from termination error code
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getPortFlagsFromTerminationErrorCode(
    _env: JNIEnv,
    _clazz: JClass,
    error_code: JInt,
) -> JInt {
    unsafe { LiGetPortFlagsFromTerminationErrorCode(error_code) }
}

/// Stringify port flags
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_stringifyPortFlags(
    env: JNIEnv,
    _clazz: JClass,
    port_flags: JInt,
    separator: JString,
) -> JString {
    if separator.is_null() {
        return ptr::null_mut();
    }

    let separator_str = unsafe { jni_get_string_utf_chars(env, separator) };
    if separator_str.is_null() {
        return ptr::null_mut();
    }

    let mut output_buffer = [0u8; 512];

    unsafe {
        LiStringifyPortFlags(
            port_flags,
            separator_str,
            output_buffer.as_mut_ptr() as *mut c_char,
            output_buffer.len(),
        );
        jni_release_string_utf_chars(env, separator, separator_str);
    }

    // Find null terminator
    let len = output_buffer.iter().position(|&c| c == 0).unwrap_or(output_buffer.len());
    let result = std::str::from_utf8(&output_buffer[..len]).unwrap_or("");

    let c_str = CString::new(result).unwrap_or_default();
    unsafe { jni_new_string_utf(env, c_str.as_ptr()) }
}

/// Get estimated RTT info
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getEstimatedRttInfo(
    _env: JNIEnv,
    _clazz: JClass,
) -> JLong {
    let mut rtt: u32 = 0;
    let mut variance: u32 = 0;

    let success = unsafe { LiGetEstimatedRttInfo(&mut rtt, &mut variance) };

    if !success {
        return -1;
    }

    ((rtt as u64) << 32) as i64 | (variance as i64)
}

/// Get launch URL query parameters
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_getLaunchUrlQueryParameters(
    env: JNIEnv,
    _clazz: JClass,
) -> JString {
    let params = unsafe { LiGetLaunchUrlQueryParameters() };
    if params.is_null() {
        return ptr::null_mut();
    }

    unsafe { jni_new_string_utf(env, params) }
}


/// Start connection
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_startConnection(
    env: JNIEnv,
    _clazz: JClass,
    address: JString,
    app_version: JString,
    gfe_version: JString,
    rtsp_session_url: JString,
    server_codec_mode_support: JInt,
    width: JInt,
    height: JInt,
    fps: JInt,
    bitrate: JInt,
    packet_size: JInt,
    streaming_remotely: JInt,
    audio_configuration: JInt,
    supported_video_formats: JInt,
    client_refresh_rate_x100: JInt,
    ri_aes_key: JByteArray,
    ri_aes_iv: JByteArray,
    video_capabilities: JInt,
    color_space: JInt,
    color_range: JInt,
    disable_encryption: JBoolean,
) -> JInt {
    info!("startConnection called: {}x{} @ {}fps, bitrate={}, disable_encryption={}", width, height, fps, bitrate, disable_encryption != 0);

    // Get string parameters
    let address_str = unsafe { jni_get_string_utf_chars(env, address) };
    let app_version_str = unsafe { jni_get_string_utf_chars(env, app_version) };
    let gfe_version_str = if !gfe_version.is_null() {
        unsafe { jni_get_string_utf_chars(env, gfe_version) }
    } else {
        ptr::null()
    };
    let rtsp_session_url_str = if !rtsp_session_url.is_null() {
        unsafe { jni_get_string_utf_chars(env, rtsp_session_url) }
    } else {
        ptr::null()
    };

    if !address_str.is_null() {
        let addr = unsafe { CStr::from_ptr(address_str) };
        info!("Connecting to: {:?}", addr);
    }

    // Create server info
    let server_info = SERVER_INFORMATION {
        address: address_str,
        serverInfoAppVersion: app_version_str,
        serverInfoGfeVersion: gfe_version_str,
        rtspSessionUrl: rtsp_session_url_str,
        serverCodecModeSupport: server_codec_mode_support,
    };

    // Get AES key and IV
    let mut aes_key = [0u8; 16];
    let mut aes_iv = [0u8; 16];

    unsafe {
        jni_get_byte_array_region(env, ri_aes_key, 0, 16, aes_key.as_mut_ptr() as *mut i8);
        jni_get_byte_array_region(env, ri_aes_iv, 0, 16, aes_iv.as_mut_ptr() as *mut i8);
    }

    // Determine encryption flags based on hardware AES support and user preference
    let encryption_flags = if disable_encryption != 0 {
        info!("Encryption disabled by user preference");
        ENCFLG_NONE
    } else if has_fast_aes() {
        info!("Using hardware AES encryption");
        ENCFLG_ALL
    } else {
        info!("Using software AES encryption (audio only)");
        ENCFLG_AUDIO
    };

    // Create stream config
    let stream_config = STREAM_CONFIGURATION {
        width,
        height,
        fps,
        bitrate,
        packetSize: packet_size,
        streamingRemotely: streaming_remotely,
        audioConfiguration: audio_configuration,
        supportedVideoFormats: supported_video_formats,
        clientRefreshRateX100: client_refresh_rate_x100,
        colorSpace: color_space,
        colorRange: color_range,
        encryptionFlags: encryption_flags,
        remoteInputAesKey: aes_key,
        remoteInputAesIv: aes_iv,
    };

    info!("Creating callbacks...");

    // Create video callbacks on stack with capabilities
    let video_callbacks = DECODER_RENDERER_CALLBACKS {
        setup: Some(bridge_dr_setup),
        start: Some(bridge_dr_start),
        stop: Some(bridge_dr_stop),
        cleanup: Some(bridge_dr_cleanup),
        submitDecodeUnit: Some(bridge_dr_submit_decode_unit),
        capabilities: video_capabilities,
    };

    // Create audio callbacks on stack
    let audio_callbacks = AUDIO_RENDERER_CALLBACKS {
        init: Some(bridge_ar_init),
        start: Some(bridge_ar_start),
        stop: Some(bridge_ar_stop),
        cleanup: Some(bridge_ar_cleanup),
        decodeAndPlaySample: Some(bridge_ar_decode_and_play_sample),
        capabilities: CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION,
    };

    // Create connection callbacks on stack
    let connection_callbacks = CONNECTION_LISTENER_CALLBACKS {
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
        setAdaptiveTriggers: None,
    };

    info!("SERVER_INFORMATION size={}", std::mem::size_of::<SERVER_INFORMATION>());
    info!("STREAM_CONFIGURATION size={}", std::mem::size_of::<STREAM_CONFIGURATION>());
    info!("DECODER_RENDERER_CALLBACKS size={}", std::mem::size_of::<DECODER_RENDERER_CALLBACKS>());
    info!("AUDIO_RENDERER_CALLBACKS size={}", std::mem::size_of::<AUDIO_RENDERER_CALLBACKS>());
    info!("CONNECTION_LISTENER_CALLBACKS size={}", std::mem::size_of::<CONNECTION_LISTENER_CALLBACKS>());

    info!("Calling LiStartConnection...");

    // Start connection
    let ret = unsafe {
        LiStartConnection(
            &server_info,
            &stream_config,
            &connection_callbacks,
            &video_callbacks,
            &audio_callbacks,
            ptr::null(),
            0,
            ptr::null(),
            0,
        )
    };

    info!("LiStartConnection returned: {}", ret);

    // Release strings
    unsafe {
        jni_release_string_utf_chars(env, address, address_str);
        jni_release_string_utf_chars(env, app_version, app_version_str);
        if !gfe_version.is_null() && !gfe_version_str.is_null() {
            jni_release_string_utf_chars(env, gfe_version, gfe_version_str);
        }
        if !rtsp_session_url.is_null() && !rtsp_session_url_str.is_null() {
            jni_release_string_utf_chars(env, rtsp_session_url, rtsp_session_url_str);
        }
    }

    ret
}

// ============================================================================
// JNI Helper Functions
// ============================================================================

// JNI Native Interface - proper structure definition
#[repr(C)]
struct JNINativeInterface {
    reserved0: *mut c_void,
    reserved1: *mut c_void,
    reserved2: *mut c_void,
    reserved3: *mut c_void,

    get_version: *mut c_void,
    define_class: *mut c_void,
    find_class: *mut c_void,
    from_reflected_method: *mut c_void,
    from_reflected_field: *mut c_void,
    to_reflected_method: *mut c_void,
    get_superclass: *mut c_void,
    is_assignable_from: *mut c_void,
    to_reflected_field: *mut c_void,
    throw: *mut c_void,
    throw_new: *mut c_void,
    exception_occurred: *mut c_void,
    exception_describe: *mut c_void,
    exception_clear: *mut c_void,
    fatal_error: *mut c_void,
    push_local_frame: *mut c_void,
    pop_local_frame: *mut c_void,
    new_global_ref: *mut c_void,
    delete_global_ref: *mut c_void,
    delete_local_ref: *mut c_void,
    is_same_object: *mut c_void,
    new_local_ref: *mut c_void,
    ensure_local_capacity: *mut c_void,
    alloc_object: *mut c_void,
    new_object: *mut c_void,
    new_object_v: *mut c_void,
    new_object_a: *mut c_void,
    get_object_class: *mut c_void,
    is_instance_of: *mut c_void,
    get_method_id: *mut c_void,

    // Call methods (34-63)
    call_object_method: *mut c_void,
    call_object_method_v: *mut c_void,
    call_object_method_a: *mut c_void,
    call_boolean_method: *mut c_void,
    call_boolean_method_v: *mut c_void,
    call_boolean_method_a: *mut c_void,
    call_byte_method: *mut c_void,
    call_byte_method_v: *mut c_void,
    call_byte_method_a: *mut c_void,
    call_char_method: *mut c_void,
    call_char_method_v: *mut c_void,
    call_char_method_a: *mut c_void,
    call_short_method: *mut c_void,
    call_short_method_v: *mut c_void,
    call_short_method_a: *mut c_void,
    call_int_method: *mut c_void,
    call_int_method_v: *mut c_void,
    call_int_method_a: *mut c_void,
    call_long_method: *mut c_void,
    call_long_method_v: *mut c_void,
    call_long_method_a: *mut c_void,
    call_float_method: *mut c_void,
    call_float_method_v: *mut c_void,
    call_float_method_a: *mut c_void,
    call_double_method: *mut c_void,
    call_double_method_v: *mut c_void,
    call_double_method_a: *mut c_void,
    call_void_method: *mut c_void,
    call_void_method_v: *mut c_void,
    call_void_method_a: *mut c_void,

    // Nonvirtual methods (64-96)
    call_nonvirtual_object_method: *mut c_void,
    call_nonvirtual_object_method_v: *mut c_void,
    call_nonvirtual_object_method_a: *mut c_void,
    call_nonvirtual_boolean_method: *mut c_void,
    call_nonvirtual_boolean_method_v: *mut c_void,
    call_nonvirtual_boolean_method_a: *mut c_void,
    call_nonvirtual_byte_method: *mut c_void,
    call_nonvirtual_byte_method_v: *mut c_void,
    call_nonvirtual_byte_method_a: *mut c_void,
    call_nonvirtual_char_method: *mut c_void,
    call_nonvirtual_char_method_v: *mut c_void,
    call_nonvirtual_char_method_a: *mut c_void,
    call_nonvirtual_short_method: *mut c_void,
    call_nonvirtual_short_method_v: *mut c_void,
    call_nonvirtual_short_method_a: *mut c_void,
    call_nonvirtual_int_method: *mut c_void,
    call_nonvirtual_int_method_v: *mut c_void,
    call_nonvirtual_int_method_a: *mut c_void,
    call_nonvirtual_long_method: *mut c_void,
    call_nonvirtual_long_method_v: *mut c_void,
    call_nonvirtual_long_method_a: *mut c_void,
    call_nonvirtual_float_method: *mut c_void,
    call_nonvirtual_float_method_v: *mut c_void,
    call_nonvirtual_float_method_a: *mut c_void,
    call_nonvirtual_double_method: *mut c_void,
    call_nonvirtual_double_method_v: *mut c_void,
    call_nonvirtual_double_method_a: *mut c_void,
    call_nonvirtual_void_method: *mut c_void,
    call_nonvirtual_void_method_v: *mut c_void,
    call_nonvirtual_void_method_a: *mut c_void,

    // Field access (94-113)
    get_field_id: *mut c_void,
    get_object_field: *mut c_void,
    get_boolean_field: *mut c_void,
    get_byte_field: *mut c_void,
    get_char_field: *mut c_void,
    get_short_field: *mut c_void,
    get_int_field: *mut c_void,
    get_long_field: *mut c_void,
    get_float_field: *mut c_void,
    get_double_field: *mut c_void,
    set_object_field: *mut c_void,
    set_boolean_field: *mut c_void,
    set_byte_field: *mut c_void,
    set_char_field: *mut c_void,
    set_short_field: *mut c_void,
    set_int_field: *mut c_void,
    set_long_field: *mut c_void,
    set_float_field: *mut c_void,
    set_double_field: *mut c_void,

    // Static methods (113-145)
    get_static_method_id: *mut c_void,
    call_static_object_method: *mut c_void,
    call_static_object_method_v: *mut c_void,
    call_static_object_method_a: *mut c_void,
    call_static_boolean_method: *mut c_void,
    call_static_boolean_method_v: *mut c_void,
    call_static_boolean_method_a: *mut c_void,
    call_static_byte_method: *mut c_void,
    call_static_byte_method_v: *mut c_void,
    call_static_byte_method_a: *mut c_void,
    call_static_char_method: *mut c_void,
    call_static_char_method_v: *mut c_void,
    call_static_char_method_a: *mut c_void,
    call_static_short_method: *mut c_void,
    call_static_short_method_v: *mut c_void,
    call_static_short_method_a: *mut c_void,
    call_static_int_method: *mut c_void,
    call_static_int_method_v: *mut c_void,
    call_static_int_method_a: *mut c_void,
    call_static_long_method: *mut c_void,
    call_static_long_method_v: *mut c_void,
    call_static_long_method_a: *mut c_void,
    call_static_float_method: *mut c_void,
    call_static_float_method_v: *mut c_void,
    call_static_float_method_a: *mut c_void,
    call_static_double_method: *mut c_void,
    call_static_double_method_v: *mut c_void,
    call_static_double_method_a: *mut c_void,
    call_static_void_method: *mut c_void,
    call_static_void_method_v: *mut c_void,
    call_static_void_method_a: *mut c_void,

    // Static field access (145-165)
    get_static_field_id: *mut c_void,
    get_static_object_field: *mut c_void,
    get_static_boolean_field: *mut c_void,
    get_static_byte_field: *mut c_void,
    get_static_char_field: *mut c_void,
    get_static_short_field: *mut c_void,
    get_static_int_field: *mut c_void,
    get_static_long_field: *mut c_void,
    get_static_float_field: *mut c_void,
    get_static_double_field: *mut c_void,
    set_static_object_field: *mut c_void,
    set_static_boolean_field: *mut c_void,
    set_static_byte_field: *mut c_void,
    set_static_char_field: *mut c_void,
    set_static_short_field: *mut c_void,
    set_static_int_field: *mut c_void,
    set_static_long_field: *mut c_void,
    set_static_float_field: *mut c_void,
    set_static_double_field: *mut c_void,

    // String operations (163-172)
    new_string: *mut c_void,
    get_string_length: *mut c_void,
    get_string_chars: *mut c_void,
    release_string_chars: *mut c_void,
    new_string_utf: extern "C" fn(env: JNIEnv, chars: *const c_char) -> JString,
    get_string_utf_length: *mut c_void,
    get_string_utf_chars: extern "C" fn(env: JNIEnv, string: JString, is_copy: *mut JBoolean) -> *const c_char,
    release_string_utf_chars: extern "C" fn(env: JNIEnv, string: JString, chars: *const c_char),

    // Array operations (171-...)
    get_array_length: *mut c_void,
    new_object_array: *mut c_void,
    get_object_array_element: *mut c_void,
    set_object_array_element: *mut c_void,
    new_boolean_array: *mut c_void,
    new_byte_array: *mut c_void,
    new_char_array: *mut c_void,
    new_short_array: *mut c_void,
    new_int_array: *mut c_void,
    new_long_array: *mut c_void,
    new_float_array: *mut c_void,
    new_double_array: *mut c_void,
    get_boolean_array_elements: *mut c_void,
    get_byte_array_elements: *mut c_void,
    get_char_array_elements: *mut c_void,
    get_short_array_elements: *mut c_void,
    get_int_array_elements: *mut c_void,
    get_long_array_elements: *mut c_void,
    get_float_array_elements: *mut c_void,
    get_double_array_elements: *mut c_void,
    release_boolean_array_elements: *mut c_void,
    release_byte_array_elements: *mut c_void,
    release_char_array_elements: *mut c_void,
    release_short_array_elements: *mut c_void,
    release_int_array_elements: *mut c_void,
    release_long_array_elements: *mut c_void,
    release_float_array_elements: *mut c_void,
    release_double_array_elements: *mut c_void,
    get_boolean_array_region: *mut c_void,
    get_byte_array_region: extern "C" fn(env: JNIEnv, array: JByteArray, start: JInt, len: JInt, buf: *mut i8),
    // ... more functions follow
}

unsafe fn jni_get_string_utf_chars(env: JNIEnv, string: JString) -> *const c_char {
    if env.is_null() || string.is_null() {
        return ptr::null();
    }

    let jni_env = *(env as *mut *const JNINativeInterface);
    ((*jni_env).get_string_utf_chars)(env, string, ptr::null_mut())
}

unsafe fn jni_release_string_utf_chars(env: JNIEnv, string: JString, chars: *const c_char) {
    if env.is_null() || string.is_null() || chars.is_null() {
        return;
    }

    let jni_env = *(env as *mut *const JNINativeInterface);
    ((*jni_env).release_string_utf_chars)(env, string, chars);
}

unsafe fn jni_new_string_utf(env: JNIEnv, chars: *const c_char) -> JString {
    if env.is_null() || chars.is_null() {
        return ptr::null_mut();
    }

    let jni_env = *(env as *mut *const JNINativeInterface);
    ((*jni_env).new_string_utf)(env, chars)
}

unsafe fn jni_get_byte_array_region(
    env: JNIEnv,
    array: JByteArray,
    start: JInt,
    len: JInt,
    buf: *mut i8,
) {
    if env.is_null() || array.is_null() || buf.is_null() {
        return;
    }

    let jni_env = *(env as *mut *const JNINativeInterface);
    ((*jni_env).get_byte_array_region)(env, array, start, len, buf);
}

// ============================================================================
// WireGuard JNI Bridge Functions
// ============================================================================

/// Start a WireGuard tunnel
/// Parameters:
///   privateKey: 32-byte private key
///   peerPublicKey: 32-byte peer public key
///   presharedKey: 32-byte preshared key (nullable)
///   endpointAddr: endpoint address string (e.g. "1.2.3.4")
///   endpointPort: endpoint port
///   tunnelAddr: tunnel IP address string (e.g. "10.0.0.2")
///   mtu: tunnel MTU
/// Returns: 0 on success, non-zero on failure
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgStartTunnel(
    env: JNIEnv,
    _clazz: JClass,
    private_key: JByteArray,
    peer_public_key: JByteArray,
    preshared_key: JByteArray,
    endpoint_addr: JString,
    endpoint_port: JInt,
    tunnel_addr: JString,
    mtu: JInt,
) -> JInt {
    info!("wgStartTunnel called, endpoint port: {}", endpoint_port);

    // Read private key
    let mut priv_key = [0u8; 32];
    unsafe { jni_get_byte_array_region(env, private_key, 0, 32, priv_key.as_mut_ptr() as *mut i8) };

    // Read peer public key
    let mut pub_key = [0u8; 32];
    unsafe { jni_get_byte_array_region(env, peer_public_key, 0, 32, pub_key.as_mut_ptr() as *mut i8) };

    // Read preshared key (optional)
    let psk = if !preshared_key.is_null() {
        let mut psk_bytes = [0u8; 32];
        unsafe { jni_get_byte_array_region(env, preshared_key, 0, 32, psk_bytes.as_mut_ptr() as *mut i8) };
        Some(psk_bytes)
    } else {
        None
    };

    // Get endpoint address string
    let endpoint_str = unsafe { jni_get_string_utf_chars(env, endpoint_addr) };
    if endpoint_str.is_null() {
        error!("wgStartTunnel: endpoint address is null");
        return -1;
    }
    let endpoint_addr_str = unsafe { CStr::from_ptr(endpoint_str) }.to_string_lossy().to_string();
    unsafe { jni_release_string_utf_chars(env, endpoint_addr, endpoint_str) };

    // Get tunnel address string
    let tunnel_str = unsafe { jni_get_string_utf_chars(env, tunnel_addr) };
    if tunnel_str.is_null() {
        error!("wgStartTunnel: tunnel address is null");
        return -2;
    }
    let tunnel_addr_str = unsafe { CStr::from_ptr(tunnel_str) }.to_string_lossy().to_string();
    unsafe { jni_release_string_utf_chars(env, tunnel_addr, tunnel_str) };

    // Parse addresses
    let tunnel_ip: std::net::IpAddr = match tunnel_addr_str.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("wgStartTunnel: invalid tunnel address '{}': {}", tunnel_addr_str, e);
            return -4;
        }
    };

    // Construct endpoint string (host:port) for dynamic DNS resolution
    let endpoint_str = format!("{}:{}", endpoint_addr_str, endpoint_port);
    info!("wgStartTunnel: endpoint '{}' will be resolved dynamically", endpoint_str);

    let config = crate::wireguard::WireGuardConfig {
        private_key: priv_key,
        peer_public_key: pub_key,
        preshared_key: psk,
        endpoint: endpoint_str,
        tunnel_address: tunnel_ip,
        mtu: mtu as u16,
    };

    match crate::wireguard::wg_start_tunnel(config) {
        Ok(()) => {
            info!("WireGuard tunnel started successfully");
            0
        }
        Err(e) => {
            error!("Failed to start WireGuard tunnel: {}", e);
            -5
        }
    }
}

/// Stop the WireGuard tunnel
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgStopTunnel(
    _env: JNIEnv,
    _clazz: JClass,
) {
    info!("wgStopTunnel called");
    crate::wireguard::wg_stop_tunnel();
}

/// Check if the WireGuard tunnel is active
/// Returns: 1 if active, 0 if not
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgIsTunnelActive(
    _env: JNIEnv,
    _clazz: JClass,
) -> JBoolean {
    if crate::wireguard::wg_is_tunnel_active() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Enable direct WireGuard routing for UDP traffic.
/// JNI interface: MoonBridge.wgEnableDirectRouting(String serverAddr)
///
/// This enables zero-copy routing: sendto calls targeting the WG server IP
/// are intercepted and encapsulated directly through the WG tunnel.
/// No local proxy is created - use the actual WG server IP as the host.
///
/// Arguments:
///   serverAddr: WireGuard server IP address string (e.g., "10.0.0.1")
/// Returns: true on success, false on failure
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgEnableDirectRouting(
    env: JNIEnv,
    _clazz: JClass,
    server_addr: JString,
) -> JBoolean {
    let addr_str = unsafe { jni_get_string_utf_chars(env, server_addr) };
    if addr_str.is_null() {
        return JNI_FALSE;
    }
    let addr = unsafe { CStr::from_ptr(addr_str) }.to_string_lossy().to_string();
    unsafe { jni_release_string_utf_chars(env, server_addr, addr_str) };

    let server_ip: std::net::Ipv4Addr = match addr.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("wgEnableDirectRouting: invalid address '{}': {}", addr, e);
            return JNI_FALSE;
        }
    };

    match crate::wireguard::wg_enable_direct_routing(server_ip) {
        Ok(()) => {
            info!("Direct WireGuard routing enabled for server {}", server_ip);
            JNI_TRUE
        }
        Err(e) => {
            error!("Failed to enable direct WireGuard routing: {}", e);
            JNI_FALSE
        }
    }
}

/// Rebind the WireGuard endpoint socket after a network change (WiFi ↔ mobile).
/// Creates a new UDP socket on the current default network and re-initiates handshake.
/// JNI interface: MoonBridge.wgRebindEndpoint()
/// Returns: true on success, false on failure
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgRebindEndpoint(
    _env: JNIEnv,
    _clazz: JClass,
) -> JBoolean {
    info!("wgRebindEndpoint called (network change detected)");
    match crate::wireguard::wg_rebind_endpoint() {
        Ok(()) => {
            info!("WireGuard endpoint rebound successfully");
            JNI_TRUE
        }
        Err(e) => {
            error!("Failed to rebind WireGuard endpoint: {}", e);
            JNI_FALSE
        }
    }
}

/// Notify that the device is going to sleep (screen off).
/// DDNS re-resolution will be paused to avoid futile DNS lookups during doze.
/// JNI interface: MoonBridge.wgNotifyDeviceSleep()
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgNotifyDeviceSleep(
    _env: JNIEnv,
    _clazz: JClass,
) {
    crate::wireguard::wg_notify_device_sleep();
}

/// Notify that the device has woken up (screen on).
/// Triggers immediate DDNS re-resolution to restore connectivity ASAP.
/// JNI interface: MoonBridge.wgNotifyDeviceWake()
#[no_mangle]
pub extern "C" fn Java_com_limelight_nvstream_jni_MoonBridge_wgNotifyDeviceWake(
    _env: JNIEnv,
    _clazz: JClass,
) {
    crate::wireguard::wg_notify_device_wake();
}

// ============================================================================
// WireGuardManager JNI Functions
// ============================================================================

/// Start WireGuard tunnel (WireGuardManager.nativeStartTunnel)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeStartTunnel(
    env: JNIEnv,
    _clazz: JClass,
    private_key: JByteArray,
    peer_public_key: JByteArray,
    preshared_key: JByteArray,
    endpoint: JString,
    tunnel_address: JString,
    mtu: JInt,
) -> JBoolean {
    // Get private key bytes
    let private_key_bytes = match jni_helpers::get_byte_array(env, private_key) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            error!("nativeStartTunnel: invalid private key");
            return JNI_FALSE;
        }
    };

    // Get peer public key bytes
    let peer_public_key_bytes = match jni_helpers::get_byte_array(env, peer_public_key) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            error!("nativeStartTunnel: invalid peer public key");
            return JNI_FALSE;
        }
    };

    // Get optional preshared key
    let psk_bytes = if !preshared_key.is_null() {
        match jni_helpers::get_byte_array(env, preshared_key) {
            Some(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(arr)
            }
            _ => None,
        }
    } else {
        None
    };

    // Get endpoint string
    let endpoint_str = match jni_helpers::get_string(env, endpoint) {
        Some(s) => s,
        None => {
            error!("nativeStartTunnel: invalid endpoint");
            return JNI_FALSE;
        }
    };

    // Get tunnel address string
    let tunnel_addr_str = match jni_helpers::get_string(env, tunnel_address) {
        Some(s) => s,
        None => {
            error!("nativeStartTunnel: invalid tunnel address");
            return JNI_FALSE;
        }
    };

    // Validate endpoint format (host:port)
    if !endpoint_str.contains(':') {
        error!("nativeStartTunnel: invalid endpoint format '{}' (expected host:port)", endpoint_str);
        return JNI_FALSE;
    }
    info!("nativeStartTunnel: endpoint '{}' will be resolved dynamically on each connection", endpoint_str);

    // Parse tunnel address
    let tunnel_ip: std::net::IpAddr = match tunnel_addr_str.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("nativeStartTunnel: invalid tunnel address '{}': {}", tunnel_addr_str, e);
            return JNI_FALSE;
        }
    };

    // Build config - endpoint stored as string for DDNS support
    let config = crate::wireguard_config::WireGuardConfig {
        private_key: private_key_bytes,
        peer_public_key: peer_public_key_bytes,
        preshared_key: psk_bytes,
        endpoint: endpoint_str,
        tunnel_address: tunnel_ip,
        mtu: mtu as u16,
    };

    // Start tunnel
    match crate::wireguard::wg_start_tunnel(config) {
        Ok(()) => {
            info!("WireGuard tunnel started successfully via JNI");
            JNI_TRUE
        }
        Err(e) => {
            error!("Failed to start WireGuard tunnel: {}", e);
            JNI_FALSE
        }
    }
}

/// Stop WireGuard tunnel (WireGuardManager.nativeStopTunnel)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeStopTunnel(
    _env: JNIEnv,
    _clazz: JClass,
) {
    crate::wireguard::wg_stop_tunnel();
    info!("WireGuard tunnel stopped via JNI");
}

/// Check if tunnel is active (WireGuardManager.nativeIsTunnelActive)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeIsTunnelActive(
    _env: JNIEnv,
    _clazz: JClass,
) -> JBoolean {
    if crate::wireguard::wg_is_tunnel_active() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Generate a new WireGuard private key (WireGuardManager.nativeGeneratePrivateKey)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeGeneratePrivateKey(
    env: JNIEnv,
    _clazz: JClass,
) -> JByteArray {
    match crate::wireguard_config::generate_private_key() {
        Ok(key) => jni_helpers::create_byte_array(env, &key),
        Err(e) => {
            error!("Failed to generate private key: {}", e);
            ptr::null_mut()
        }
    }
}

/// Derive public key from private key (WireGuardManager.nativeDerivePublicKey)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeDerivePublicKey(
    env: JNIEnv,
    _clazz: JClass,
    private_key: JByteArray,
) -> JByteArray {
    let private_key_bytes = match jni_helpers::get_byte_array(env, private_key) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            error!("nativeDerivePublicKey: invalid private key");
            return ptr::null_mut();
        }
    };

    let public_key = crate::wireguard_config::derive_public_key(&private_key_bytes);
    jni_helpers::create_byte_array(env, &public_key)
}

// ============================================================================
// WireGuard Direct HTTP JNI Functions
// ============================================================================

/// Configure WireGuard HTTP client (WireGuardManager.nativeHttpSetConfig)
/// This configures the WireGuard tunnel for direct HTTP requests.
/// Parameters:
///   privateKey: 32-byte private key
///   peerPublicKey: 32-byte peer public key
///   presharedKey: 32-byte preshared key (nullable)
///   endpoint: WireGuard endpoint as "host:port"
///   tunnelAddress: Local tunnel IP (e.g., "10.0.0.2")
///   serverAddress: Server IP in the tunnel (e.g., "10.0.0.1")
///   mtu: MTU size
/// Returns: true on success, false on failure
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeHttpSetConfig(
    env: JNIEnv,
    _clazz: JClass,
    private_key: JByteArray,
    peer_public_key: JByteArray,
    preshared_key: JByteArray,
    endpoint: JString,
    tunnel_address: JString,
    server_address: JString,
    mtu: JInt,
) -> JBoolean {
    // Get private key bytes
    let private_key_bytes = match jni_helpers::get_byte_array(env, private_key) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            error!("nativeHttpSetConfig: invalid private key");
            return JNI_FALSE;
        }
    };

    // Get peer public key bytes
    let peer_public_key_bytes = match jni_helpers::get_byte_array(env, peer_public_key) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            error!("nativeHttpSetConfig: invalid peer public key");
            return JNI_FALSE;
        }
    };

    // Get optional preshared key
    let psk_bytes = if !preshared_key.is_null() {
        match jni_helpers::get_byte_array(env, preshared_key) {
            Some(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(arr)
            }
            _ => None,
        }
    } else {
        None
    };

    // Get endpoint string
    let endpoint_str = match jni_helpers::get_string(env, endpoint) {
        Some(s) => s,
        None => {
            error!("nativeHttpSetConfig: invalid endpoint");
            return JNI_FALSE;
        }
    };

    // Get tunnel address string
    let tunnel_addr_str = match jni_helpers::get_string(env, tunnel_address) {
        Some(s) => s,
        None => {
            error!("nativeHttpSetConfig: invalid tunnel address");
            return JNI_FALSE;
        }
    };

    // Get server address string
    let server_addr_str = match jni_helpers::get_string(env, server_address) {
        Some(s) => s,
        None => {
            error!("nativeHttpSetConfig: invalid server address");
            return JNI_FALSE;
        }
    };

    // Store endpoint as string for dynamic DNS resolution on each connection
    // Validation: check format is valid (host:port)
    if !endpoint_str.contains(':') {
        error!("nativeHttpSetConfig: invalid endpoint format '{}' (expected host:port)", endpoint_str);
        return JNI_FALSE;
    }
    info!("nativeHttpSetConfig: endpoint '{}' will be resolved dynamically on each connection", endpoint_str);

    // Parse tunnel address (supports IPv4 and IPv6)
    let tunnel_ip: std::net::IpAddr = match tunnel_addr_str.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("nativeHttpSetConfig: invalid tunnel address '{}': {}", tunnel_addr_str, e);
            return JNI_FALSE;
        }
    };

    // Parse server address (supports IPv4 and IPv6)
    let server_ip: std::net::IpAddr = match server_addr_str.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("nativeHttpSetConfig: invalid server address '{}': {}", server_addr_str, e);
            return JNI_FALSE;
        }
    };

    // Build HTTP config - endpoint stored as string for DDNS support
    let config = crate::wg_http::WgHttpConfig {
        private_key: private_key_bytes,
        peer_public_key: peer_public_key_bytes,
        preshared_key: psk_bytes,
        endpoint: endpoint_str,
        tunnel_ip,
        server_ip,
        mtu: mtu as u16,
    };

    crate::wg_http::wg_http_set_config(config);
    info!("WireGuard HTTP client configured");
    JNI_TRUE
}

/// Clear WireGuard HTTP client configuration (WireGuardManager.nativeHttpClearConfig)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeHttpClearConfig(
    _env: JNIEnv,
    _clazz: JClass,
) {
    crate::wg_http::wg_http_clear_config();
    info!("WireGuard HTTP client configuration cleared");
}

/// Check if WireGuard HTTP client is configured (WireGuardManager.nativeHttpIsConfigured)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WireGuardManager_nativeHttpIsConfigured(
    _env: JNIEnv,
    _clazz: JClass,
) -> JBoolean {
    if crate::wg_http::wg_http_is_configured() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ============================================================================
// WgSocket JNI Functions (for direct TCP socket access through WireGuard)
// ============================================================================

/// Create a TCP connection through WireGuard VirtualStack (WgSocket.nativeConnect)
/// Parameters:
///   host: Target host IP in the tunnel (e.g., "10.0.0.1")
///   port: Target port
///   timeoutMs: Connection timeout in milliseconds
/// Returns: Native handle (>0) on success, 0 on failure
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WgSocket_nativeConnect(
    env: JNIEnv,
    _clazz: JClass,
    host: JString,
    port: JInt,
    timeout_ms: JInt,
) -> JLong {
    let host_str = match jni_helpers::get_string(env, host) {
        Some(s) => s,
        None => {
            error!("WgSocket.nativeConnect: invalid host string");
            return 0;
        }
    };

    crate::wg_socket::wg_socket_connect(&host_str, port as u16, timeout_ms as u32) as JLong
}

/// Get the local port allocated for this connection (WgSocket.nativeGetLocalPort)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WgSocket_nativeGetLocalPort(
    _env: JNIEnv,
    _clazz: JClass,
    handle: JLong,
) -> JInt {
    crate::wg_socket::wg_socket_get_local_port(handle as u64) as JInt
}

/// Receive data from the connection (WgSocket.nativeRecv)
/// Parameters:
///   handle: Native connection handle
///   buffer: Buffer to receive into
///   offset: Offset in buffer
///   length: Maximum bytes to receive
///   timeoutMs: Read timeout in milliseconds (0 = default timeout)
/// Returns: Bytes received (>0), 0 on EOF, -1 on error, -2 on timeout
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WgSocket_nativeRecv(
    env: JNIEnv,
    _clazz: JClass,
    handle: JLong,
    buffer: JByteArray,
    offset: JInt,
    length: JInt,
    timeout_ms: JInt,
) -> JInt {
    if buffer.is_null() || length <= 0 {
        error!("WgSocket.nativeRecv: invalid buffer");
        return -1;
    }

    // Allocate temporary buffer for receive
    let mut recv_buf = vec![0u8; length as usize];
    
    let result = crate::wg_socket::wg_socket_recv(handle as u64, &mut recv_buf, timeout_ms as u32);
    
    if result > 0 {
        // Copy received data to Java buffer
        let bytes_to_copy = result as usize;
        jni_helpers::set_byte_array_region(env, buffer, offset, bytes_to_copy as i32, recv_buf.as_ptr() as *const i8);
    }
    
    result
}

/// Send data through the connection (WgSocket.nativeSend)
/// Parameters:
///   handle: Native connection handle
///   buffer: Data to send
///   offset: Offset in buffer
///   length: Number of bytes to send
/// Returns: Bytes sent (>0) on success, negative on error
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WgSocket_nativeSend(
    env: JNIEnv,
    _clazz: JClass,
    handle: JLong,
    buffer: JByteArray,
    offset: JInt,
    length: JInt,
) -> JInt {
    if buffer.is_null() || length <= 0 {
        error!("WgSocket.nativeSend: invalid buffer");
        return -1;
    }

    // Get data from Java buffer
    let data = match jni_helpers::get_byte_array_region(env, buffer, offset, length) {
        Some(d) => d,
        None => {
            error!("WgSocket.nativeSend: failed to get buffer data");
            return -1;
        }
    };
    
    crate::wg_socket::wg_socket_send(handle as u64, &data)
}

/// Close the connection (WgSocket.nativeClose)
#[no_mangle]
pub extern "C" fn Java_com_limelight_binding_wireguard_WgSocket_nativeClose(
    _env: JNIEnv,
    _clazz: JClass,
    handle: JLong,
) {
    crate::wg_socket::wg_socket_close(handle as u64);
}


