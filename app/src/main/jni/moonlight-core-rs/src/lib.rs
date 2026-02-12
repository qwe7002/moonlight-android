//! Moonlight Core RS - Rust implementation of moonlight-core native library for Android
//!
//! This library provides JNI bindings for the Moonlight streaming client,
//! wrapping the moonlight-common-c library with Rust for better safety and maintainability.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]

#[cfg(target_os = "android")]
mod ffi;
#[cfg(target_os = "android")]
mod opus;
#[cfg(target_os = "android")]
pub mod crypto;
#[cfg(target_os = "android")]
mod jni_helpers;
#[cfg(target_os = "android")]
mod callbacks;
#[cfg(target_os = "android")]
mod jni_bridge;

#[cfg(target_os = "android")]
pub use jni_bridge::*;
#[cfg(target_os = "android")]
pub use crypto::*;

