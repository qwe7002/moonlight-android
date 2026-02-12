# Moonlight Core RS - Android Native Library
#
# This makefile imports the pre-built Rust shared library.
# The Rust library (which includes moonlight-common-c) is built using cargo-ndk.
#
# Build instructions:
#   1. cd to this directory
#   2. Run: cargo ndk -t arm64-v8a -o ./jniLibs build --release

MY_LOCAL_PATH := $(call my-dir)
LOCAL_PATH := $(MY_LOCAL_PATH)

# Import pre-built Rust shared library
include $(CLEAR_VARS)
LOCAL_MODULE := moonlight-core
LOCAL_SRC_FILES := jniLibs/$(TARGET_ARCH_ABI)/libmoonlight_core.so
include $(PREBUILT_SHARED_LIBRARY)

