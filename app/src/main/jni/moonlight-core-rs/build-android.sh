#!/bin/bash
# Build script for moonlight-core-rs
# This script builds the Rust library for Android arm64-v8a only

set -e

# Check if cargo-ndk is installed
if ! command -v cargo-ndk &> /dev/null; then
    echo "Installing cargo-ndk..."
    cargo install cargo-ndk
fi

# Check if ninja is installed
if ! command -v ninja &> /dev/null; then
    echo "Error: ninja is required but not installed."
    echo "Install it with:"
    echo "  macOS: brew install ninja"
    echo "  Linux: sudo apt install ninja-build"
    exit 1
fi

# Add Android target (arm64-v8a only)
rustup target add aarch64-linux-android

# Set NDK path (adjust as needed)
if [ -z "$ANDROID_NDK_HOME" ]; then
    echo "Warning: ANDROID_NDK_HOME not set, trying to find NDK..."
    if [ -d "$HOME/Android/Sdk/ndk" ]; then
        export ANDROID_NDK_HOME=$(ls -d $HOME/Android/Sdk/ndk/*/ | head -1 | sed 's:/*$::')
    fi
fi

echo "Using NDK: $ANDROID_NDK_HOME"

# Set up CMake environment for audiopus_sys cross-compilation
export CMAKE_GENERATOR="Ninja"
export CMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake"
export ANDROID_PLATFORM="android-34"
export CMAKE_POLICY_VERSION_MINIMUM="3.5"
export ANDROID_ABI="arm64-v8a"

# Build for arm64-v8a only
echo "Building for arm64-v8a..."
cargo ndk -t arm64-v8a -o ./jniLibs build --release


echo "Build complete!"
echo "Libraries are in ./jniLibs/"

