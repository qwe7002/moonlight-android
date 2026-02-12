//! Callback implementations for moonlight-common-c
//!
//! This module provides callback functions for video decoding, audio rendering,
//! and connection events that bridge between moonlight-common-c and the JNI layer.

#![allow(dead_code)]

mod video;
mod audio;
mod connection;

use std::sync::atomic::{AtomicBool, Ordering};
use log::info;

// Re-export callback structures
pub use video::VIDEO_CALLBACKS;
pub use audio::AUDIO_CALLBACKS;
pub use connection::CONNECTION_CALLBACKS;

// Re-export video callbacks
pub use video::{
    bridge_dr_setup, bridge_dr_start, bridge_dr_stop, bridge_dr_cleanup, bridge_dr_submit_decode_unit,
};

// Re-export audio callbacks
pub use audio::{
    bridge_ar_init, bridge_ar_start, bridge_ar_stop, bridge_ar_cleanup, bridge_ar_decode_and_play_sample,
};

// Re-export connection callbacks
pub use connection::{
    bridge_cl_stage_starting, bridge_cl_stage_complete, bridge_cl_stage_failed,
    bridge_cl_connection_started, bridge_cl_connection_terminated, bridge_cl_rumble,
    bridge_cl_connection_status_update, bridge_cl_set_hdr_mode, bridge_cl_rumble_triggers,
    bridge_cl_set_motion_event_state, bridge_cl_set_controller_led,
};

// Flag to indicate if JNI callbacks are enabled
static JNI_CALLBACKS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable JNI callbacks
#[inline]
pub fn set_jni_callbacks(enabled: bool) {
    JNI_CALLBACKS_ENABLED.store(enabled, Ordering::Release);
    info!("JNI callbacks {}", if enabled { "enabled" } else { "disabled" });
}

/// Check if JNI callbacks are enabled
#[inline]
pub fn jni_callbacks_enabled() -> bool {
    JNI_CALLBACKS_ENABLED.load(Ordering::Acquire)
}

/// Check if platform has fast AES support
/// This is a simplified version - on aarch64 we assume hardware AES is available
pub fn has_fast_aes() -> bool {
    // On arm64-v8a (aarch64), most modern devices have hardware AES
    // This is a conservative implementation that returns true for arm64
    #[cfg(target_arch = "aarch64")]
    {
        true
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}

