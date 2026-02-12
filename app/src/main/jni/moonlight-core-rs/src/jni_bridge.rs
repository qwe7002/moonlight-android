//! JNI bridge functions
//!
//! Ported from simplejni.c - provides JNI interface for Java/Android

use crate::callbacks::{
    create_audio_callbacks, create_connection_callbacks, create_video_callbacks, init_callbacks,
};
use crate::controller::{
    guess_controller_has_paddles, guess_controller_has_share_button, guess_controller_type,
};
use crate::ffi::*;
use crate::init_logging;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{
    jboolean, jbyte, jfloat, jint, jlong, jshort, JNI_FALSE, JNI_TRUE,
};
use jni::JNIEnv;
use libc::c_char;
use log::info;
use std::ffi::{CStr, CString};
use std::ptr;

/// Check if the CPU has fast AES support
fn has_fast_aes() -> bool {
    // On Android, we check for hardware AES support
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("aes")
    }
    #[cfg(target_arch = "arm")]
    {
        // For 32-bit ARM, we'd need to check NEON + AES
        false
    }
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("aes")
    }
    #[cfg(target_arch = "x86")]
    {
        std::arch::is_x86_feature_detected!("aes")
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "x86_64",
        target_arch = "x86"
    )))]
    {
        true // Assume new architectures have crypto acceleration
    }
}

// ============================================================================
// Initialization
// ============================================================================

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_init<'local>(
    mut env: JNIEnv<'local>,
    class: JClass<'local>,
) {
    init_logging();

    if let Err(e) = init_callbacks(&mut env, &class) {
        log::error!("Failed to initialize callbacks: {:?}", e);
    }

    info!("MoonBridge initialized (Rust)");
}

// ============================================================================
// Input Functions
// ============================================================================

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseMove(
    _env: JNIEnv,
    _class: JClass,
    delta_x: jshort,
    delta_y: jshort,
) {
    unsafe {
        LiSendMouseMoveEvent(delta_x, delta_y);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMousePosition(
    _env: JNIEnv,
    _class: JClass,
    x: jshort,
    y: jshort,
    reference_width: jshort,
    reference_height: jshort,
) {
    unsafe {
        LiSendMousePositionEvent(x, y, reference_width, reference_height);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseMoveAsMousePosition(
    _env: JNIEnv,
    _class: JClass,
    delta_x: jshort,
    delta_y: jshort,
    reference_width: jshort,
    reference_height: jshort,
) {
    unsafe {
        LiSendMouseMoveAsMousePositionEvent(delta_x, delta_y, reference_width, reference_height);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseButton(
    _env: JNIEnv,
    _class: JClass,
    button_event: jbyte,
    mouse_button: jbyte,
) {
    unsafe {
        LiSendMouseButtonEvent(button_event as c_char, mouse_button as c_char);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMultiControllerInput(
    _env: JNIEnv,
    _class: JClass,
    controller_number: jshort,
    active_gamepad_mask: jshort,
    button_flags: jint,
    left_trigger: jbyte,
    right_trigger: jbyte,
    left_stick_x: jshort,
    left_stick_y: jshort,
    right_stick_x: jshort,
    right_stick_y: jshort,
) {
    unsafe {
        LiSendMultiControllerEvent(
            controller_number,
            active_gamepad_mask,
            button_flags,
            left_trigger as c_char,
            right_trigger as c_char,
            left_stick_x,
            left_stick_y,
            right_stick_x,
            right_stick_y,
        );
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendTouchEvent(
    _env: JNIEnv,
    _class: JClass,
    event_type: jbyte,
    pointer_id: jint,
    x: jfloat,
    y: jfloat,
    pressure_or_distance: jfloat,
    contact_area_major: jfloat,
    contact_area_minor: jfloat,
    rotation: jshort,
) -> jint {
    unsafe {
        LiSendTouchEvent(
            event_type as c_char,
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

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendPenEvent(
    _env: JNIEnv,
    _class: JClass,
    event_type: jbyte,
    tool_type: jbyte,
    pen_buttons: jbyte,
    x: jfloat,
    y: jfloat,
    pressure_or_distance: jfloat,
    contact_area_major: jfloat,
    contact_area_minor: jfloat,
    rotation: jshort,
    tilt: jbyte,
) -> jint {
    unsafe {
        LiSendPenEvent(
            event_type as c_char,
            tool_type as c_char,
            pen_buttons as c_char,
            x,
            y,
            pressure_or_distance,
            contact_area_major,
            contact_area_minor,
            rotation,
            tilt as c_char,
        )
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerArrivalEvent(
    _env: JNIEnv,
    _class: JClass,
    controller_number: jbyte,
    active_gamepad_mask: jshort,
    controller_type: jbyte,
    supported_button_flags: jint,
    capabilities: jshort,
) -> jint {
    unsafe {
        LiSendControllerArrivalEvent(
            controller_number as c_char,
            active_gamepad_mask,
            controller_type as c_char,
            supported_button_flags,
            capabilities,
        )
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerTouchEvent(
    _env: JNIEnv,
    _class: JClass,
    controller_number: jbyte,
    event_type: jbyte,
    pointer_id: jint,
    x: jfloat,
    y: jfloat,
    pressure: jfloat,
) -> jint {
    unsafe { LiSendControllerTouchEvent(controller_number as c_char, event_type as c_char, pointer_id, x, y, pressure) }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerMotionEvent(
    _env: JNIEnv,
    _class: JClass,
    controller_number: jbyte,
    motion_type: jbyte,
    x: jfloat,
    y: jfloat,
    z: jfloat,
) -> jint {
    unsafe { LiSendControllerMotionEvent(controller_number as c_char, motion_type as c_char, x, y, z) }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendControllerBatteryEvent(
    _env: JNIEnv,
    _class: JClass,
    controller_number: jbyte,
    battery_state: jbyte,
    battery_percentage: jbyte,
) -> jint {
    unsafe {
        LiSendControllerBatteryEvent(controller_number as c_char, battery_state as c_char, battery_percentage as c_char)
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendKeyboardInput(
    _env: JNIEnv,
    _class: JClass,
    key_code: jshort,
    key_action: jbyte,
    modifiers: jbyte,
    flags: jbyte,
) {
    unsafe {
        LiSendKeyboardEvent2(key_code, key_action as c_char, modifiers as c_char, flags as c_char);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseHighResScroll(
    _env: JNIEnv,
    _class: JClass,
    scroll_amount: jshort,
) {
    unsafe {
        LiSendHighResScrollEvent(scroll_amount);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendMouseHighResHScroll(
    _env: JNIEnv,
    _class: JClass,
    scroll_amount: jshort,
) {
    unsafe {
        LiSendHighResHScrollEvent(scroll_amount);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_sendUtf8Text<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    text: JString<'local>,
) {
    let Ok(text_str) = env.get_string(&text) else {
        return;
    };

    let c_str: &CStr = unsafe { CStr::from_ptr(text_str.as_ptr()) };
    let bytes = c_str.to_bytes();

    unsafe {
        LiSendUtf8TextEvent(c_str.as_ptr(), bytes.len() as u32);
    }
}

// ============================================================================
// Connection Management
// ============================================================================

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_stopConnection(
    _env: JNIEnv,
    _class: JClass,
) {
    unsafe {
        LiStopConnection();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_interruptConnection(
    _env: JNIEnv,
    _class: JClass,
) {
    unsafe {
        LiInterruptConnection();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_startConnection<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    address: JString<'local>,
    app_version: JString<'local>,
    gfe_version: JString<'local>,
    rtsp_session_url: JString<'local>,
    server_codec_mode_support: jint,
    width: jint,
    height: jint,
    fps: jint,
    bitrate: jint,
    packet_size: jint,
    streaming_remotely: jint,
    audio_configuration: jint,
    supported_video_formats: jint,
    client_refresh_rate_x100: jint,
    ri_aes_key: JByteArray<'local>,
    ri_aes_iv: JByteArray<'local>,
    video_capabilities: jint,
    color_space: jint,
    color_range: jint,
) -> jint {
    // Convert Java strings to C strings
    let address_str = match env.get_string(&address) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let address_c = CString::new(unsafe { CStr::from_ptr(address_str.as_ptr()) }.to_bytes())
        .unwrap_or_default();

    let app_version_str = match env.get_string(&app_version) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let app_version_c =
        CString::new(unsafe { CStr::from_ptr(app_version_str.as_ptr()) }.to_bytes())
            .unwrap_or_default();

    let gfe_version_c = if gfe_version.is_null() {
        None
    } else {
        env.get_string(&gfe_version).ok().map(|s| {
            CString::new(unsafe { CStr::from_ptr(s.as_ptr()) }.to_bytes()).unwrap_or_default()
        })
    };

    let rtsp_session_url_c = if rtsp_session_url.is_null() {
        None
    } else {
        env.get_string(&rtsp_session_url).ok().map(|s| {
            CString::new(unsafe { CStr::from_ptr(s.as_ptr()) }.to_bytes()).unwrap_or_default()
        })
    };

    // Get AES key and IV
    let mut aes_key = [0u8; 16];
    let mut aes_iv = [0u8; 16];

    if let Ok(key_elements) = unsafe { env.get_array_elements(&ri_aes_key, jni::objects::ReleaseMode::NoCopyBack) } {
        let key_slice = unsafe { std::slice::from_raw_parts(key_elements.as_ptr() as *const u8, 16) };
        aes_key.copy_from_slice(key_slice);
    }

    if let Ok(iv_elements) = unsafe { env.get_array_elements(&ri_aes_iv, jni::objects::ReleaseMode::NoCopyBack) } {
        let iv_slice = unsafe { std::slice::from_raw_parts(iv_elements.as_ptr() as *const u8, 16) };
        aes_iv.copy_from_slice(iv_slice);
    }

    // Build server info
    let server_info = SERVER_INFORMATION {
        address: address_c.as_ptr(),
        serverInfoAppVersion: app_version_c.as_ptr(),
        serverInfoGfeVersion: gfe_version_c
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null()),
        rtspSessionUrl: rtsp_session_url_c
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null()),
        serverCodecModeSupport: server_codec_mode_support,
    };

    // Determine encryption flags based on CPU capabilities
    let encryption_flags = if has_fast_aes() {
        ENCFLG_ALL
    } else {
        ENCFLG_AUDIO
    };

    // Build stream config
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

    // Create callbacks
    let video_callbacks = create_video_callbacks(video_capabilities);
    let audio_callbacks = create_audio_callbacks();
    let connection_callbacks = create_connection_callbacks();

    // Start connection
    unsafe {
        LiStartConnection(
            &server_info,
            &stream_config,
            &connection_callbacks,
            &video_callbacks,
            &audio_callbacks,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
        )
    }
}

// ============================================================================
// Status and Utility Functions
// ============================================================================

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getStageName<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    stage: jint,
) -> jni::sys::jstring {
    let c_str = unsafe { CStr::from_ptr(LiGetStageName(stage)) };
    match env.new_string(c_str.to_string_lossy()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_findExternalAddressIP4<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    stun_host_name: JString<'local>,
    stun_port: jint,
) -> jni::sys::jstring {
    let Ok(stun_str) = env.get_string(&stun_host_name) else {
        return ptr::null_mut();
    };

    let stun_c = CString::new(unsafe { CStr::from_ptr(stun_str.as_ptr()) }.to_bytes())
        .unwrap_or_default();

    let mut wan_addr: u32 = 0;
    let err = unsafe { LiFindExternalAddressIP4(stun_c.as_ptr(), stun_port, &mut wan_addr) };

    if err == 0 {
        // Convert IP address to string
        let ip = std::net::Ipv4Addr::from(wan_addr.to_be());
        info!("Resolved WAN address to {}", ip);
        match env.new_string(ip.to_string()) {
            Ok(s) => s.into_raw(),
            Err(_) => ptr::null_mut(),
        }
    } else {
        log::error!("STUN failed to get WAN address: {}", err);
        ptr::null_mut()
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getPendingAudioDuration(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    unsafe { LiGetPendingAudioDuration() }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getPendingVideoFrames(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    unsafe { LiGetPendingVideoFrames() }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_testClientConnectivity<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    test_server_host_name: JString<'local>,
    reference_port: jint,
    test_flags: jint,
) -> jint {
    let Ok(host_str) = env.get_string(&test_server_host_name) else {
        return -1;
    };

    let host_c = CString::new(unsafe { CStr::from_ptr(host_str.as_ptr()) }.to_bytes())
        .unwrap_or_default();

    unsafe { LiTestClientConnectivity(host_c.as_ptr(), reference_port as u16, test_flags) }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getPortFlagsFromStage(
    _env: JNIEnv,
    _class: JClass,
    stage: jint,
) -> jint {
    unsafe { LiGetPortFlagsFromStage(stage) }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getPortFlagsFromTerminationErrorCode(
    _env: JNIEnv,
    _class: JClass,
    error_code: jint,
) -> jint {
    unsafe { LiGetPortFlagsFromTerminationErrorCode(error_code) }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_stringifyPortFlags<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    port_flags: jint,
    separator: JString<'local>,
) -> jni::sys::jstring {
    let Ok(sep_str) = env.get_string(&separator) else {
        return ptr::null_mut();
    };

    let sep_c = CString::new(unsafe { CStr::from_ptr(sep_str.as_ptr()) }.to_bytes())
        .unwrap_or_default();

    let mut output_buffer = [0u8; 512];
    unsafe {
        LiStringifyPortFlags(
            port_flags,
            sep_c.as_ptr(),
            output_buffer.as_mut_ptr() as *mut c_char,
            512,
        );
    }

    let result = unsafe { CStr::from_ptr(output_buffer.as_ptr() as *const c_char) };
    match env.new_string(result.to_string_lossy()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getEstimatedRttInfo(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let mut rtt: u32 = 0;
    let mut variance: u32 = 0;

    let success = unsafe { LiGetEstimatedRttInfo(&mut rtt, &mut variance) };

    if success {
        ((rtt as u64) << 32) as i64 | variance as i64
    } else {
        -1
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_getLaunchUrlQueryParameters<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jni::sys::jstring {
    let c_str = unsafe { CStr::from_ptr(LiGetLaunchUrlQueryParameters()) };
    match env.new_string(c_str.to_string_lossy()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

// ============================================================================
// Controller Detection Functions
// ============================================================================

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_guessControllerType(
    _env: JNIEnv,
    _class: JClass,
    vendor_id: jint,
    product_id: jint,
) -> jbyte {
    guess_controller_type(vendor_id, product_id)
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_guessControllerHasPaddles(
    _env: JNIEnv,
    _class: JClass,
    vendor_id: jint,
    product_id: jint,
) -> jboolean {
    if guess_controller_has_paddles(vendor_id, product_id) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_limelight_nvstream_jni_MoonBridge_guessControllerHasShareButton(
    _env: JNIEnv,
    _class: JClass,
    vendor_id: jint,
    product_id: jint,
) -> jboolean {
    if guess_controller_has_share_button(vendor_id, product_id) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

