# Performance Optimizations for moonlight-core-rs

This document summarizes the performance optimizations applied to the Rust native library.

## 1. Memory Ordering Optimizations

Changed from `Ordering::SeqCst` (sequentially consistent) to more appropriate memory orderings:

### Files Modified:
- `callbacks.rs`
- `jni_helpers.rs`
- `jni_bridge.rs`

### Changes:
- **JNI_CALLBACKS_ENABLED**: `Acquire/Release` semantics - these are simple flag loads/stores
- **OPUS_DECODER**: `Acquire/Release` for load/store, `AcqRel` for swap operations
- **JAVA_VM, GLOBAL_BRIDGE_CLASS**: `Acquire/Release` - set once during init, read many times
- **Method ID accessors**: `Acquire/Release` - set once during JNI_OnLoad, read frequently
- **Buffer accessors**: `Acquire/Release` - modified infrequently, read frequently

**Impact**: Reduced CPU memory barrier overhead on ARM processors, especially in hot paths like audio/video callbacks.

## 2. Thread-Local JNIEnv Caching

Added thread-local caching for JNIEnv pointers to avoid repeated JVM calls.

### File: `jni_helpers.rs`

### Changes:
```rust
thread_local! {
    static CACHED_ENV: std::cell::Cell<JNIEnv> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}
```

- Fast path: Check thread-local cache first (single memory access)
- Slow path: Falls through to JVM GetEnv/AttachCurrentThread when cache misses
- Cache is cleared on `detach_current_thread()`

**Impact**: Eliminates JVM calls for JNIEnv lookup in the common case where the thread is already attached.

## 3. Inline Function Hints

Added `#[inline]` and `#[inline(always)]` hints to critical path functions:

### Files Modified:
- `jni_helpers.rs`
- `callbacks.rs`
- `crypto.rs`
- `jni_bridge.rs`

### Functions Inlined:
- `get_jni_fn`, `get_jvm_fn` - JNI vtable lookups (`#[inline(always)]`)
- `get_thread_env`, `get_java_vm`, `get_bridge_class` - Accessor functions
- `call_static_void_method`, `call_static_int_method` - JNI call wrappers
- `check_exception` - Exception checking
- `get_primitive_array_critical`, `release_primitive_array_critical` - Array access
- `set_byte_array_region` - Data copying
- All method ID getter/setter macros
- Buffer accessor functions

**Impact**: Reduces function call overhead in hot paths, allows compiler to optimize across function boundaries.

## 4. Dead Code Removal

Removed unused debug instrumentation from hot paths:

### File: `callbacks.rs`

### Changes:
- Removed `AUDIO_SAMPLE_COUNT` atomic counter
- Removed debug sample logging in `bridge_ar_decode_and_play_sample`
- Removed periodic audio sample value inspection

**Impact**: Eliminated atomic operations and dead code from the audio decoding hot path.

## 5. Cargo.toml Optimization

Enhanced release profile for better runtime performance:

### Changes:
```toml
[profile.release]
opt-level = 3
lto = "fat"           # Changed from "thin" for better cross-crate optimization
codegen-units = 1
panic = "abort"
strip = false
overflow-checks = false  # Added: disable runtime overflow checks
debug = false           # Added: disable debug info in release
```

**Impact**:
- `lto = "fat"`: Better whole-program optimization at the cost of longer compile times
- `overflow-checks = false`: Eliminates runtime arithmetic overflow checks
- `debug = false`: Smaller binary size

## 6. Cold Path Annotation

Added `#[cold]` attribute to infrequently executed paths:

### File: `jni_helpers.rs`

### Changes:
- `get_thread_env_slow()` marked as `#[cold]` to hint the compiler that this path is rarely taken

**Impact**: Compiler can better optimize the hot path by knowing the slow path is unlikely.

## Summary of Performance Benefits

1. **Reduced CPU cycles** in hot paths (audio/video callbacks) through better memory ordering
2. **Eliminated JVM overhead** for JNIEnv lookups via thread-local caching
3. **Better compiler optimization** through inline hints and cold path annotations
4. **Smaller code size** through dead code removal
5. **Better whole-program optimization** through fat LTO

## 7. Controller Detection Moved to Java

Moved USB VID/PID based controller detection logic from Rust to Java.

### Files Removed (Rust):
- `usb_ids.rs` - USB VID/PID constants (deleted)
- `controller.rs` - Controller type detection logic (deleted)
- Related JNI functions in `jni_bridge.rs` (removed)

### Files Added (Java):
- `ControllerDetection.java` - New Java class with all controller detection logic

### Files Modified:
- `lib.rs` - Removed `usb_ids` and `controller` module declarations
- `jni_bridge.rs` - Removed controller-related JNI functions
- `ControllerHandler.java` - Uses `ControllerDetection` instead of native calls
- `GamepadTestActivity.java` - Uses `ControllerDetection` instead of native calls
- `MoonBridge.java` - Removed native controller detection methods

### Benefits:
- **Eliminates JNI overhead** for controller detection calls
- **Easier maintenance** - Java code is simpler to update
- **Reduced binary size** - ~200+ lines of Rust code removed
- **Faster compilation** - Less Rust code to compile
- **Faster startup** - No need to initialize Rust controller lookup tables


## Testing Recommendations

After applying these optimizations, test:
1. Audio playback latency and smoothness
2. Video decoding frame times
3. Controller input latency
4. Memory usage (should be similar or slightly lower)
5. Binary size comparison

