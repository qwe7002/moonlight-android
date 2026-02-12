# Moonlight Core RS

Rust implementation of the moonlight-core native library for Android.

## Overview

This is a Rust port of the C code in `moonlight-core`, providing JNI bindings for the Moonlight streaming client. The external library `moonlight-common-c` is included and compiled via the cc crate.

## Structure

```
moonlight-core-rs/
├── Cargo.toml           # Rust package configuration
├── build.rs             # Build script (compiles moonlight-common-c and opus)
├── wrapper.h            # C header wrapper for bindgen
├── build-android.sh     # Build script (Linux/macOS)
├── build-android.ps1    # Build script (Windows)
├── moonlight-common-c/  # moonlight-common-c library source
└── src/
    ├── lib.rs           # Main library entry point
    ├── ffi.rs           # FFI bindings to moonlight-common-c and Opus
    ├── opus.rs          # Opus decoder FFI bindings
    ├── crypto.rs        # Crypto implementation using ring
    ├── usb_ids.rs       # USB vendor/product ID constants
    ├── controller.rs    # Controller type detection
    ├── callbacks.rs     # Video/audio/connection callbacks
    └── jni_bridge.rs    # JNI function implementations
```

## Building

### Prerequisites

1. Install Rust: https://rustup.rs/
2. Install Android NDK
3. Install cargo-ndk: `cargo install cargo-ndk`
4. Install Ninja build system (required for building libopus):
   - Windows: `scoop install ninja` or `choco install ninja`
   - macOS: `brew install ninja`
   - Linux: `sudo apt install ninja-build` or `sudo pacman -S ninja`
5. Add Android target (arm64-v8a only):
   ```bash
   rustup target add aarch64-linux-android
   ```

### Build Commands

**Windows (PowerShell):**
```powershell
.\build-android.ps1
```

**Linux/macOS:**
```bash
./build-android.sh
```

### Manual Build

```bash
# Set NDK path
export ANDROID_NDK_HOME=/path/to/ndk

# Build for specific architecture
cargo ndk -t arm64-v8a build --release
```

## Integration with moonlight-android

The built libraries will be placed in `jniLibs/<arch>/libmoonlight_core.so`.

These need to be linked with:
- moonlight-common-c (compiled separately)
- libopus
- libssl / libcrypto

## API Compatibility

All JNI functions maintain the same signatures as the original C implementation, ensuring drop-in compatibility with the Java code in `com.limelight.nvstream.jni.MoonBridge`.

## Dependencies

- `jni` - JNI bindings for Rust
- `log` + `android_logger` - Logging support
- `once_cell` - Lazy static initialization
- `libc` - C type definitions

## License

GPL-3.0 (same as moonlight-android)

