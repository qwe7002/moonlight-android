//! Moonlight Core Library for Android
//!
//! This is a Rust implementation of the moonlight-core native library,
//! providing JNI bindings for the Moonlight streaming client.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

mod ffi;
mod usb_ids;
mod controller;
mod callbacks;
mod jni_bridge;
mod opus;
mod crypto;

pub use ffi::*;
pub use controller::*;
pub use callbacks::*;
pub use jni_bridge::*;
pub use crypto::*;

use android_logger::Config;
use log::LevelFilter;
use once_cell::sync::OnceCell;

static INIT: OnceCell<()> = OnceCell::new();

/// Initialize the library (called from JNI_OnLoad or first JNI call)
pub fn init_logging() {
    INIT.get_or_init(|| {
        android_logger::init_once(
            Config::default()
                .with_max_level(LevelFilter::Debug)
                .with_tag("moonlight-core-rs"),
        );
        log::info!("Moonlight Core RS initialized");
    });
}

