//! JNI Helper Functions
//!
//! This module provides safe wrappers around JNI operations for calling Java methods
//! from native callbacks.

#![allow(dead_code)]

use libc::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

// JNI types
pub type JNIEnv = *mut c_void;
pub type JClass = *mut c_void;
pub type JMethodID = *mut c_void;
pub type JObject = *mut c_void;
pub type JByteArray = *mut c_void;
pub type JShortArray = *mut c_void;
pub type JBoolean = u8;
pub type JByte = i8;
pub type JShort = i16;
pub type JInt = i32;
pub type JLong = i64;
pub type JChar = u16;
pub type JavaVM = *mut c_void;

pub const JNI_OK: JInt = 0;
pub const JNI_VERSION_1_4: JInt = 0x00010004;
pub const JNI_ABORT: JInt = 2;

// Global JavaVM and class references
static JAVA_VM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static GLOBAL_BRIDGE_CLASS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// Global method IDs
static DR_SETUP_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DR_START_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DR_STOP_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DR_CLEANUP_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DR_SUBMIT_DECODE_UNIT_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AR_INIT_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AR_START_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AR_STOP_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AR_CLEANUP_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AR_PLAY_SAMPLE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_STAGE_STARTING_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_STAGE_COMPLETE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_STAGE_FAILED_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_CONNECTION_STARTED_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_CONNECTION_TERMINATED_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_RUMBLE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_CONNECTION_STATUS_UPDATE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_SET_HDR_MODE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_RUMBLE_TRIGGERS_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_SET_MOTION_EVENT_STATE_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CL_SET_CONTROLLER_LED_METHOD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// Global buffer references
static DECODED_FRAME_BUFFER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DECODED_AUDIO_BUFFER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// JNI function indices (based on JNI specification)
const JNI_GET_VERSION: usize = 4;
const JNI_FIND_CLASS: usize = 6;
const JNI_EXCEPTION_OCCURRED: usize = 15;
const JNI_EXCEPTION_CLEAR: usize = 17;
const JNI_NEW_GLOBAL_REF: usize = 21;
const JNI_DELETE_GLOBAL_REF: usize = 22;
const JNI_DELETE_LOCAL_REF: usize = 23;
const JNI_GET_STATIC_METHOD_ID: usize = 113;
const JNI_CALL_STATIC_INT_METHOD_A: usize = 131;
const JNI_CALL_STATIC_VOID_METHOD_A: usize = 143;
const JNI_GET_ARRAY_LENGTH: usize = 171;
const JNI_NEW_BYTE_ARRAY: usize = 176;
const JNI_NEW_SHORT_ARRAY: usize = 178;
const JNI_GET_SHORT_ARRAY_ELEMENTS: usize = 186;
const JNI_RELEASE_SHORT_ARRAY_ELEMENTS: usize = 194;
const JNI_SET_BYTE_ARRAY_REGION: usize = 208;
const JNI_SET_SHORT_ARRAY_REGION: usize = 210;
const JNI_GET_JAVA_VM: usize = 219;
const JNI_GET_PRIMITIVE_ARRAY_CRITICAL: usize = 222;
const JNI_RELEASE_PRIMITIVE_ARRAY_CRITICAL: usize = 223;

// JNIInvokeInterface indices
const JVM_DESTROY_JAVA_VM: usize = 3;
const JVM_ATTACH_CURRENT_THREAD: usize = 4;
const JVM_DETACH_CURRENT_THREAD: usize = 5;
const JVM_GET_ENV: usize = 6;

/// JNI value union for method calls
#[repr(C)]
#[derive(Copy, Clone)]
pub union JValue {
    pub z: JBoolean,
    pub b: JByte,
    pub c: JChar,
    pub s: JShort,
    pub i: JInt,
    pub j: JLong,
    pub f: f32,
    pub d: f64,
    pub l: JObject,
}

impl JValue {
    pub fn int(i: JInt) -> Self {
        JValue { i }
    }
    pub fn short(s: JShort) -> Self {
        JValue { s }
    }
    pub fn byte(b: JByte) -> Self {
        JValue { b }
    }
    pub fn boolean(z: bool) -> Self {
        JValue { z: if z { 1 } else { 0 } }
    }
    pub fn object(l: JObject) -> Self {
        JValue { l }
    }
    pub fn long(j: JLong) -> Self {
        JValue { j }
    }
    pub fn char(c: JChar) -> Self {
        JValue { c }
    }
}

// Helper to get function pointer from JNIEnv
#[inline]
unsafe fn get_jni_fn<T>(env: JNIEnv, index: usize) -> T {
    let env_ptr = *(env as *mut *mut *mut c_void);
    let fn_ptr = *env_ptr.add(index);
    std::mem::transmute_copy(&fn_ptr)
}

// Helper to get function pointer from JavaVM
#[inline]
unsafe fn get_jvm_fn<T>(vm: JavaVM, index: usize) -> T {
    let vm_ptr = *(vm as *mut *mut *mut c_void);
    let fn_ptr = *vm_ptr.add(index);
    std::mem::transmute_copy(&fn_ptr)
}

/// Store JavaVM reference
#[inline]
pub fn set_java_vm(vm: JavaVM) {
    JAVA_VM.store(vm, Ordering::Release);
}

/// Get JavaVM reference
#[inline]
pub fn get_java_vm() -> JavaVM {
    JAVA_VM.load(Ordering::Acquire)
}

/// Store global bridge class reference
#[inline]
pub fn set_bridge_class(class: JClass) {
    GLOBAL_BRIDGE_CLASS.store(class, Ordering::Release);
}

/// Get global bridge class reference
#[inline]
pub fn get_bridge_class() -> JClass {
    GLOBAL_BRIDGE_CLASS.load(Ordering::Acquire)
}

// Thread-local cache for JNIEnv to avoid repeated JVM calls
// This is safe because JNIEnv is thread-specific and doesn't change
// within the same thread
thread_local! {
    static CACHED_ENV: std::cell::Cell<JNIEnv> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Get JNIEnv for current thread, using thread-local cache when possible
/// This is optimized for the hot path where the thread is already attached
#[inline]
pub fn get_thread_env() -> Option<JNIEnv> {
    // Fast path: check thread-local cache first
    let cached = CACHED_ENV.with(|c| c.get());
    if !cached.is_null() {
        // Verify the cached env is still valid by checking if we're still attached
        // This is a lightweight check
        return Some(cached);
    }

    get_thread_env_slow()
}

/// Slow path for get_thread_env - attaches thread if necessary
#[cold]
fn get_thread_env_slow() -> Option<JNIEnv> {
    let vm = get_java_vm();
    if vm.is_null() {
        return None;
    }

    unsafe {
        let mut env: JNIEnv = ptr::null_mut();

        // First try GetEnv
        type GetEnvFn = extern "C" fn(JavaVM, *mut JNIEnv, JInt) -> JInt;
        let get_env: GetEnvFn = get_jvm_fn(vm, JVM_GET_ENV);
        let result = get_env(vm, &mut env, JNI_VERSION_1_4);
        if result == JNI_OK {
            // Cache the env for this thread
            CACHED_ENV.with(|c| c.set(env));
            return Some(env);
        }

        // Attach current thread
        type AttachFn = extern "C" fn(JavaVM, *mut JNIEnv, *mut c_void) -> JInt;
        let attach: AttachFn = get_jvm_fn(vm, JVM_ATTACH_CURRENT_THREAD);
        let result = attach(vm, &mut env, ptr::null_mut());
        if result == JNI_OK {
            // Cache the env for this thread
            CACHED_ENV.with(|c| c.set(env));
            Some(env)
        } else {
            None
        }
    }
}

/// Detach current thread from JVM
pub fn detach_current_thread() {
    // Clear the thread-local cache
    CACHED_ENV.with(|c| c.set(ptr::null_mut()));

    let vm = get_java_vm();
    if vm.is_null() {
        return;
    }

    unsafe {
        type DetachFn = extern "C" fn(JavaVM) -> JInt;
        let detach: DetachFn = get_jvm_fn(vm, JVM_DETACH_CURRENT_THREAD);
        detach(vm);
    }
}

/// Check if exception occurred and handle it
pub fn check_exception(env: JNIEnv) -> bool {
    if env.is_null() {
        return false;
    }

    unsafe {
        type ExceptionOccurredFn = extern "C" fn(JNIEnv) -> JObject;
        type ExceptionClearFn = extern "C" fn(JNIEnv);

        let exception_occurred: ExceptionOccurredFn = get_jni_fn(env, JNI_EXCEPTION_OCCURRED);
        let exception = exception_occurred(env);

        if !exception.is_null() {
            let exception_clear: ExceptionClearFn = get_jni_fn(env, JNI_EXCEPTION_CLEAR);
            exception_clear(env);
            true
        } else {
            false
        }
    }
}

// Method ID setters and getters
macro_rules! define_method_id_accessors {
    ($set_name:ident, $get_name:ident, $static_var:ident) => {
        pub fn $set_name(method: JMethodID) {
            $static_var.store(method, Ordering::SeqCst);
        }

        pub fn $get_name() -> JMethodID {
            $static_var.load(Ordering::SeqCst)
        }
    };
}

define_method_id_accessors!(set_dr_setup_method, get_dr_setup_method, DR_SETUP_METHOD);
define_method_id_accessors!(set_dr_start_method, get_dr_start_method, DR_START_METHOD);
define_method_id_accessors!(set_dr_stop_method, get_dr_stop_method, DR_STOP_METHOD);
define_method_id_accessors!(set_dr_cleanup_method, get_dr_cleanup_method, DR_CLEANUP_METHOD);
define_method_id_accessors!(set_dr_submit_decode_unit_method, get_dr_submit_decode_unit_method, DR_SUBMIT_DECODE_UNIT_METHOD);
define_method_id_accessors!(set_ar_init_method, get_ar_init_method, AR_INIT_METHOD);
define_method_id_accessors!(set_ar_start_method, get_ar_start_method, AR_START_METHOD);
define_method_id_accessors!(set_ar_stop_method, get_ar_stop_method, AR_STOP_METHOD);
define_method_id_accessors!(set_ar_cleanup_method, get_ar_cleanup_method, AR_CLEANUP_METHOD);
define_method_id_accessors!(set_ar_play_sample_method, get_ar_play_sample_method, AR_PLAY_SAMPLE_METHOD);
define_method_id_accessors!(set_cl_stage_starting_method, get_cl_stage_starting_method, CL_STAGE_STARTING_METHOD);
define_method_id_accessors!(set_cl_stage_complete_method, get_cl_stage_complete_method, CL_STAGE_COMPLETE_METHOD);
define_method_id_accessors!(set_cl_stage_failed_method, get_cl_stage_failed_method, CL_STAGE_FAILED_METHOD);
define_method_id_accessors!(set_cl_connection_started_method, get_cl_connection_started_method, CL_CONNECTION_STARTED_METHOD);
define_method_id_accessors!(set_cl_connection_terminated_method, get_cl_connection_terminated_method, CL_CONNECTION_TERMINATED_METHOD);
define_method_id_accessors!(set_cl_rumble_method, get_cl_rumble_method, CL_RUMBLE_METHOD);
define_method_id_accessors!(set_cl_connection_status_update_method, get_cl_connection_status_update_method, CL_CONNECTION_STATUS_UPDATE_METHOD);
define_method_id_accessors!(set_cl_set_hdr_mode_method, get_cl_set_hdr_mode_method, CL_SET_HDR_MODE_METHOD);
define_method_id_accessors!(set_cl_rumble_triggers_method, get_cl_rumble_triggers_method, CL_RUMBLE_TRIGGERS_METHOD);
define_method_id_accessors!(set_cl_set_motion_event_state_method, get_cl_set_motion_event_state_method, CL_SET_MOTION_EVENT_STATE_METHOD);
define_method_id_accessors!(set_cl_set_controller_led_method, get_cl_set_controller_led_method, CL_SET_CONTROLLER_LED_METHOD);

// Buffer management
pub fn set_decoded_frame_buffer(buffer: JByteArray) {
    DECODED_FRAME_BUFFER.store(buffer, Ordering::SeqCst);
}

pub fn get_decoded_frame_buffer() -> JByteArray {
    DECODED_FRAME_BUFFER.load(Ordering::Acquire)
}

#[inline]
pub fn set_decoded_audio_buffer(buffer: JShortArray) {
    DECODED_AUDIO_BUFFER.store(buffer, Ordering::Release);
}

#[inline]
pub fn get_decoded_audio_buffer() -> JShortArray {
    DECODED_AUDIO_BUFFER.load(Ordering::Acquire)
}

/// Get static method ID
pub fn jni_get_static_method_id(env: JNIEnv, clazz: JClass, name: *const c_char, sig: *const c_char) -> JMethodID {
    if env.is_null() || clazz.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type GetStaticMethodIdFn = extern "C" fn(JNIEnv, JClass, *const c_char, *const c_char) -> JMethodID;
        let get_static_method_id: GetStaticMethodIdFn = get_jni_fn(env, JNI_GET_STATIC_METHOD_ID);
        get_static_method_id(env, clazz, name, sig)
    }
}

/// Initialize method IDs from JNI env
pub fn init_method_ids(env: JNIEnv, clazz: JClass) {
    if env.is_null() || clazz.is_null() {
        return;
    }

    // Video decoder callbacks
    set_dr_setup_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeDrSetup\0".as_ptr() as *const c_char,
        b"(IIII)I\0".as_ptr() as *const c_char
    ));
    set_dr_start_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeDrStart\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_dr_stop_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeDrStop\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_dr_cleanup_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeDrCleanup\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_dr_submit_decode_unit_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeDrSubmitDecodeUnit\0".as_ptr() as *const c_char,
        b"([BIIIICJJ)I\0".as_ptr() as *const c_char
    ));

    // Audio renderer callbacks
    set_ar_init_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeArInit\0".as_ptr() as *const c_char,
        b"(III)I\0".as_ptr() as *const c_char
    ));
    set_ar_start_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeArStart\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_ar_stop_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeArStop\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_ar_cleanup_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeArCleanup\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_ar_play_sample_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeArPlaySample\0".as_ptr() as *const c_char,
        b"([S)V\0".as_ptr() as *const c_char
    ));

    // Connection listener callbacks
    set_cl_stage_starting_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClStageStarting\0".as_ptr() as *const c_char,
        b"(I)V\0".as_ptr() as *const c_char
    ));
    set_cl_stage_complete_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClStageComplete\0".as_ptr() as *const c_char,
        b"(I)V\0".as_ptr() as *const c_char
    ));
    set_cl_stage_failed_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClStageFailed\0".as_ptr() as *const c_char,
        b"(II)V\0".as_ptr() as *const c_char
    ));
    set_cl_connection_started_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClConnectionStarted\0".as_ptr() as *const c_char,
        b"()V\0".as_ptr() as *const c_char
    ));
    set_cl_connection_terminated_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClConnectionTerminated\0".as_ptr() as *const c_char,
        b"(I)V\0".as_ptr() as *const c_char
    ));
    set_cl_rumble_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClRumble\0".as_ptr() as *const c_char,
        b"(SSS)V\0".as_ptr() as *const c_char
    ));
    set_cl_connection_status_update_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClConnectionStatusUpdate\0".as_ptr() as *const c_char,
        b"(I)V\0".as_ptr() as *const c_char
    ));
    set_cl_set_hdr_mode_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClSetHdrMode\0".as_ptr() as *const c_char,
        b"(Z[B)V\0".as_ptr() as *const c_char
    ));
    set_cl_rumble_triggers_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClRumbleTriggers\0".as_ptr() as *const c_char,
        b"(SSS)V\0".as_ptr() as *const c_char
    ));
    set_cl_set_motion_event_state_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClSetMotionEventState\0".as_ptr() as *const c_char,
        b"(SBS)V\0".as_ptr() as *const c_char
    ));
    set_cl_set_controller_led_method(jni_get_static_method_id(
        env, clazz,
        b"bridgeClSetControllerLED\0".as_ptr() as *const c_char,
        b"(SBBB)V\0".as_ptr() as *const c_char
    ));

    // Create global reference for bridge class
    let global_class = new_global_ref(env, clazz);
    set_bridge_class(global_class);
}

/// Call static void method with arguments
pub fn call_static_void_method(env: JNIEnv, method: JMethodID, args: &[JValue]) {
    if env.is_null() || method.is_null() {
        return;
    }

    let class = get_bridge_class();
    if class.is_null() {
        return;
    }

    unsafe {
        type CallStaticVoidMethodAFn = extern "C" fn(JNIEnv, JClass, JMethodID, *const JValue);
        let call_static_void_method_a: CallStaticVoidMethodAFn = get_jni_fn(env, JNI_CALL_STATIC_VOID_METHOD_A);
        call_static_void_method_a(env, class, method, args.as_ptr());
    }
}

/// Call static int method with arguments
pub fn call_static_int_method(env: JNIEnv, method: JMethodID, args: &[JValue]) -> JInt {
    if env.is_null() || method.is_null() {
        return -1;
    }

    let class = get_bridge_class();
    if class.is_null() {
        return -1;
    }

    unsafe {
        type CallStaticIntMethodAFn = extern "C" fn(JNIEnv, JClass, JMethodID, *const JValue) -> JInt;
        let call_static_int_method_a: CallStaticIntMethodAFn = get_jni_fn(env, JNI_CALL_STATIC_INT_METHOD_A);
        call_static_int_method_a(env, class, method, args.as_ptr())
    }
}

/// Create a new byte array
pub fn new_byte_array(env: JNIEnv, length: JInt) -> JByteArray {
    if env.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type NewByteArrayFn = extern "C" fn(JNIEnv, JInt) -> JByteArray;
        let new_byte_array: NewByteArrayFn = get_jni_fn(env, JNI_NEW_BYTE_ARRAY);
        new_byte_array(env, length)
    }
}

/// Create a new short array
pub fn new_short_array(env: JNIEnv, length: JInt) -> JShortArray {
    if env.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type NewShortArrayFn = extern "C" fn(JNIEnv, JInt) -> JShortArray;
        let new_short_array: NewShortArrayFn = get_jni_fn(env, JNI_NEW_SHORT_ARRAY);
        new_short_array(env, length)
    }
}

/// Set byte array region
#[inline]
pub fn set_byte_array_region(env: JNIEnv, array: JByteArray, start: JInt, len: JInt, buf: *const i8) {
    if env.is_null() || array.is_null() || buf.is_null() {
        return;
    }

    unsafe {
        type SetByteArrayRegionFn = extern "C" fn(JNIEnv, JByteArray, JInt, JInt, *const JByte);
        let set_byte_array_region: SetByteArrayRegionFn = get_jni_fn(env, JNI_SET_BYTE_ARRAY_REGION);
        set_byte_array_region(env, array, start, len, buf);
    }
}

/// Set short array region
pub fn set_short_array_region(env: JNIEnv, array: JShortArray, start: JInt, len: JInt, buf: *const i16) {
    if env.is_null() || array.is_null() || buf.is_null() {
        return;
    }

    unsafe {
        type SetShortArrayRegionFn = extern "C" fn(JNIEnv, JShortArray, JInt, JInt, *const JShort);
        let set_short_array_region: SetShortArrayRegionFn = get_jni_fn(env, JNI_SET_SHORT_ARRAY_REGION);
        set_short_array_region(env, array, start, len, buf);
    }
}

/// Get array length
pub fn get_array_length(env: JNIEnv, array: JObject) -> JInt {
    if env.is_null() || array.is_null() {
        return 0;
    }

    unsafe {
        type GetArrayLengthFn = extern "C" fn(JNIEnv, JObject) -> JInt;
        let get_array_length: GetArrayLengthFn = get_jni_fn(env, JNI_GET_ARRAY_LENGTH);
        get_array_length(env, array)
    }
}

/// Create new global reference
pub fn new_global_ref(env: JNIEnv, obj: JObject) -> JObject {
    if env.is_null() || obj.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type NewGlobalRefFn = extern "C" fn(JNIEnv, JObject) -> JObject;
        let new_global_ref: NewGlobalRefFn = get_jni_fn(env, JNI_NEW_GLOBAL_REF);
        new_global_ref(env, obj)
    }
}

/// Delete global reference
pub fn delete_global_ref(env: JNIEnv, obj: JObject) {
    if env.is_null() || obj.is_null() {
        return;
    }

    unsafe {
        type DeleteGlobalRefFn = extern "C" fn(JNIEnv, JObject);
        let delete_global_ref: DeleteGlobalRefFn = get_jni_fn(env, JNI_DELETE_GLOBAL_REF);
        delete_global_ref(env, obj);
    }
}

/// Delete local reference
pub fn delete_local_ref(env: JNIEnv, obj: JObject) {
    if env.is_null() || obj.is_null() {
        return;
    }

    unsafe {
        type DeleteLocalRefFn = extern "C" fn(JNIEnv, JObject);
        let delete_local_ref: DeleteLocalRefFn = get_jni_fn(env, JNI_DELETE_LOCAL_REF);
        delete_local_ref(env, obj);
    }
}

/// Get JavaVM from JNIEnv
pub fn get_java_vm_from_env(env: JNIEnv) -> Option<JavaVM> {
    if env.is_null() {
        return None;
    }

    unsafe {
        let mut vm: JavaVM = ptr::null_mut();
        type GetJavaVMFn = extern "C" fn(JNIEnv, *mut JavaVM) -> JInt;
        let get_java_vm: GetJavaVMFn = get_jni_fn(env, JNI_GET_JAVA_VM);
        if get_java_vm(env, &mut vm) == JNI_OK {
            Some(vm)
        } else {
            None
        }
    }
}

/// Get short array elements
pub fn get_short_array_elements(env: JNIEnv, array: JShortArray) -> *mut JShort {
    if env.is_null() || array.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type GetShortArrayElementsFn = extern "C" fn(JNIEnv, JShortArray, *mut JBoolean) -> *mut JShort;
        let get_short_array_elements: GetShortArrayElementsFn = get_jni_fn(env, JNI_GET_SHORT_ARRAY_ELEMENTS);
        get_short_array_elements(env, array, ptr::null_mut())
    }
}

/// Release short array elements
pub fn release_short_array_elements(env: JNIEnv, array: JShortArray, elems: *mut JShort, mode: JInt) {
    if env.is_null() || array.is_null() || elems.is_null() {
        return;
    }

    unsafe {
        type ReleaseShortArrayElementsFn = extern "C" fn(JNIEnv, JShortArray, *mut JShort, JInt);
        let release_short_array_elements: ReleaseShortArrayElementsFn = get_jni_fn(env, JNI_RELEASE_SHORT_ARRAY_ELEMENTS);
        release_short_array_elements(env, array, elems, mode);
    }
}

/// Get primitive array critical - gives direct access to array data without copying
/// WARNING: Between Get and Release, you cannot call any other JNI functions or block!
pub fn get_primitive_array_critical(env: JNIEnv, array: JObject) -> *mut c_void {
    if env.is_null() || array.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        type GetPrimitiveArrayCriticalFn = extern "C" fn(JNIEnv, JObject, *mut JBoolean) -> *mut c_void;
        let get_primitive_array_critical: GetPrimitiveArrayCriticalFn = get_jni_fn(env, JNI_GET_PRIMITIVE_ARRAY_CRITICAL);
        get_primitive_array_critical(env, array, ptr::null_mut())
    }
}

/// Release primitive array critical
pub fn release_primitive_array_critical(env: JNIEnv, array: JObject, carray: *mut c_void, mode: JInt) {
    if env.is_null() || array.is_null() || carray.is_null() {
        return;
    }

    unsafe {
        type ReleasePrimitiveArrayCriticalFn = extern "C" fn(JNIEnv, JObject, *mut c_void, JInt);
        let release_primitive_array_critical: ReleasePrimitiveArrayCriticalFn = get_jni_fn(env, JNI_RELEASE_PRIMITIVE_ARRAY_CRITICAL);
        release_primitive_array_critical(env, array, carray, mode);
    }
}

// JNI function indices for string and byte array operations
const JNI_GET_STRING_UTF_CHARS: usize = 169;
const JNI_RELEASE_STRING_UTF_CHARS: usize = 170;
const JNI_GET_BYTE_ARRAY_ELEMENTS: usize = 184;
const JNI_RELEASE_BYTE_ARRAY_ELEMENTS: usize = 192;
const JNI_GET_BYTE_ARRAY_REGION: usize = 200;

/// Get a byte array from JNI as a Vec<u8>
pub fn get_byte_array(env: JNIEnv, array: JByteArray) -> Option<Vec<u8>> {
    if env.is_null() || array.is_null() {
        return None;
    }

    unsafe {
        let len = get_array_length(env, array);
        if len <= 0 {
            return Some(Vec::new());
        }

        // Get byte array elements
        type GetByteArrayElementsFn = extern "C" fn(JNIEnv, JByteArray, *mut JBoolean) -> *mut JByte;
        let get_byte_array_elements: GetByteArrayElementsFn = get_jni_fn(env, JNI_GET_BYTE_ARRAY_ELEMENTS);
        let elements = get_byte_array_elements(env, array, ptr::null_mut());

        if elements.is_null() {
            return None;
        }

        // Copy to Vec
        let mut result = vec![0u8; len as usize];
        ptr::copy_nonoverlapping(elements as *const u8, result.as_mut_ptr(), len as usize);

        // Release elements
        type ReleaseByteArrayElementsFn = extern "C" fn(JNIEnv, JByteArray, *mut JByte, JInt);
        let release_byte_array_elements: ReleaseByteArrayElementsFn = get_jni_fn(env, JNI_RELEASE_BYTE_ARRAY_ELEMENTS);
        release_byte_array_elements(env, array, elements, JNI_ABORT);

        Some(result)
    }
}

/// Get a region of a byte array from JNI as a Vec<u8>
pub fn get_byte_array_region(env: JNIEnv, array: JByteArray, start: JInt, len: JInt) -> Option<Vec<u8>> {
    if env.is_null() || array.is_null() || len <= 0 {
        return None;
    }

    unsafe {
        let array_len = get_array_length(env, array);
        if start < 0 || start + len > array_len {
            return None;
        }

        let mut result = vec![0i8; len as usize];
        
        type GetByteArrayRegionFn = extern "C" fn(JNIEnv, JByteArray, JInt, JInt, *mut JByte);
        let get_byte_array_region: GetByteArrayRegionFn = get_jni_fn(env, JNI_GET_BYTE_ARRAY_REGION);
        get_byte_array_region(env, array, start, len, result.as_mut_ptr());

        // Convert i8 to u8
        Some(result.into_iter().map(|b| b as u8).collect())
    }
}

/// Create a new byte array from a slice
pub fn create_byte_array(env: JNIEnv, data: &[u8]) -> JByteArray {
    if env.is_null() {
        return ptr::null_mut();
    }

    let array = new_byte_array(env, data.len() as JInt);
    if array.is_null() {
        return ptr::null_mut();
    }

    set_byte_array_region(env, array, 0, data.len() as JInt, data.as_ptr() as *const i8);
    array
}

/// Get a String from JNI JString
pub fn get_string(env: JNIEnv, jstring: *mut c_void) -> Option<String> {
    if env.is_null() || jstring.is_null() {
        return None;
    }

    unsafe {
        type GetStringUtfCharsFn = extern "C" fn(JNIEnv, *mut c_void, *mut JBoolean) -> *const c_char;
        type ReleaseStringUtfCharsFn = extern "C" fn(JNIEnv, *mut c_void, *const c_char);

        let get_string_utf_chars: GetStringUtfCharsFn = get_jni_fn(env, JNI_GET_STRING_UTF_CHARS);
        let release_string_utf_chars: ReleaseStringUtfCharsFn = get_jni_fn(env, JNI_RELEASE_STRING_UTF_CHARS);

        let chars = get_string_utf_chars(env, jstring, ptr::null_mut());
        if chars.is_null() {
            return None;
        }

        let result = std::ffi::CStr::from_ptr(chars).to_string_lossy().into_owned();
        release_string_utf_chars(env, jstring, chars);

        Some(result)
    }
}

