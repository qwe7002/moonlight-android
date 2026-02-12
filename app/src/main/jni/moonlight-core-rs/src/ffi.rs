//! FFI bindings to moonlight-common-c library
//!
//! This module contains raw FFI declarations for the moonlight-common-c C library.

#![allow(dead_code)]

use libc::{c_char, c_int, c_short, c_uchar, c_uint, c_ushort, c_void, size_t};

// Buffer types for decode units
pub const BUFFER_TYPE_PICDATA: c_int = 0;
pub const BUFFER_TYPE_SPS: c_int = 1;
pub const BUFFER_TYPE_PPS: c_int = 2;
pub const BUFFER_TYPE_VPS: c_int = 3;

// Frame types
pub const FRAME_TYPE_PFRAME: c_int = 0;
pub const FRAME_TYPE_IDR: c_int = 1;

// Decoder return codes
pub const DR_OK: c_int = 0;

// Connection status codes
pub const CONN_STATUS_OKAY: c_int = 0;
pub const CONN_STATUS_POOR: c_int = 1;

// Encryption flags
pub const ENCFLG_NONE: c_int = 0x00;
pub const ENCFLG_AUDIO: c_int = 0x01;
pub const ENCFLG_VIDEO: c_int = 0x02;
pub const ENCFLG_RI: c_int = 0x04;
pub const ENCFLG_CONTROL: c_int = 0x08;
pub const ENCFLG_ALL: c_int = ENCFLG_AUDIO | ENCFLG_VIDEO | ENCFLG_RI | ENCFLG_CONTROL;

// Controller types
pub const LI_CTYPE_UNKNOWN: c_int = 0x00;
pub const LI_CTYPE_XBOX: c_int = 0x01;
pub const LI_CTYPE_PS: c_int = 0x02;
pub const LI_CTYPE_NINTENDO: c_int = 0x03;

/// Capability flags for audio renderer
pub const CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION: c_int = 0x10;

/// Linked list entry for decode unit buffer
#[repr(C)]
pub struct LENTRY {
    pub next: *mut LENTRY,
    pub data: *mut c_char,
    pub length: c_int,
    pub bufferType: c_int,
}

/// Decode unit structure
#[repr(C)]
pub struct DECODE_UNIT {
    pub frameNumber: c_int,
    pub frameType: c_int,
    pub frameHostProcessingLatency: c_ushort,
    pub receiveTimeUs: u64,
    pub enqueueTimeUs: u64,
    pub presentationTimeUs: u64,
    pub rtpTimestamp: c_uint,
    pub fullLength: c_int,
    pub bufferList: *mut LENTRY,
    pub hdrActive: bool,
    pub colorspace: c_uchar,
}

/// Opus multistream configuration
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OPUS_MULTISTREAM_CONFIGURATION {
    pub sampleRate: c_int,
    pub channelCount: c_int,
    pub streams: c_int,
    pub coupledStreams: c_int,
    pub samplesPerFrame: c_int,
    pub mapping: [c_uchar; 8],
}

/// Server information structure
#[repr(C)]
pub struct SERVER_INFORMATION {
    pub address: *const c_char,
    pub serverInfoAppVersion: *const c_char,
    pub serverInfoGfeVersion: *const c_char,
    pub rtspSessionUrl: *const c_char,
    pub serverCodecModeSupport: c_int,
}

/// Stream configuration structure
#[repr(C)]
pub struct STREAM_CONFIGURATION {
    pub width: c_int,
    pub height: c_int,
    pub fps: c_int,
    pub bitrate: c_int,
    pub packetSize: c_int,
    pub streamingRemotely: c_int,
    pub audioConfiguration: c_int,
    pub supportedVideoFormats: c_int,
    pub clientRefreshRateX100: c_int,
    pub colorSpace: c_int,
    pub colorRange: c_int,
    pub encryptionFlags: c_int,
    pub remoteInputAesKey: [c_uchar; 16],
    pub remoteInputAesIv: [c_uchar; 16],
}

/// HDR display primary coordinate
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SS_HDR_DISPLAY_PRIMARY {
    pub x: c_ushort,
    pub y: c_ushort,
}

/// HDR metadata structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SS_HDR_METADATA {
    pub displayPrimaries: [SS_HDR_DISPLAY_PRIMARY; 3],
    pub whitePoint: SS_HDR_DISPLAY_PRIMARY,
    pub maxDisplayLuminance: c_ushort,
    pub minDisplayLuminance: c_ushort,
    pub maxContentLightLevel: c_ushort,
    pub maxFrameAverageLightLevel: c_ushort,
    pub maxFullFrameLuminance: c_ushort,
}

impl Default for SS_HDR_METADATA {
    fn default() -> Self {
        Self {
            displayPrimaries: [SS_HDR_DISPLAY_PRIMARY::default(); 3],
            whitePoint: SS_HDR_DISPLAY_PRIMARY::default(),
            maxDisplayLuminance: 0,
            minDisplayLuminance: 0,
            maxContentLightLevel: 0,
            maxFrameAverageLightLevel: 0,
            maxFullFrameLuminance: 0,
        }
    }
}

/// Video decoder callbacks
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DECODER_RENDERER_CALLBACKS {
    pub setup: Option<
        extern "C" fn(
            videoFormat: c_int,
            width: c_int,
            height: c_int,
            redrawRate: c_int,
            context: *mut c_void,
            drFlags: c_int,
        ) -> c_int,
    >,
    pub start: Option<extern "C" fn()>,
    pub stop: Option<extern "C" fn()>,
    pub cleanup: Option<extern "C" fn()>,
    pub submitDecodeUnit: Option<extern "C" fn(decodeUnit: *mut DECODE_UNIT) -> c_int>,
    pub capabilities: c_int,
}

/// Audio renderer callbacks
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AUDIO_RENDERER_CALLBACKS {
    pub init: Option<
        extern "C" fn(
            audioConfiguration: c_int,
            opusConfig: *const OPUS_MULTISTREAM_CONFIGURATION,
            context: *mut c_void,
            flags: c_int,
        ) -> c_int,
    >,
    pub start: Option<extern "C" fn()>,
    pub stop: Option<extern "C" fn()>,
    pub cleanup: Option<extern "C" fn()>,
    pub decodeAndPlaySample:
        Option<extern "C" fn(sampleData: *mut c_char, sampleLength: c_int)>,
    pub capabilities: c_int,
}

/// Connection listener callbacks
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CONNECTION_LISTENER_CALLBACKS {
    pub stageStarting: Option<extern "C" fn(stage: c_int)>,
    pub stageComplete: Option<extern "C" fn(stage: c_int)>,
    pub stageFailed: Option<extern "C" fn(stage: c_int, errorCode: c_int)>,
    pub connectionStarted: Option<extern "C" fn()>,
    pub connectionTerminated: Option<extern "C" fn(errorCode: c_int)>,
    pub logMessage: Option<unsafe extern "C" fn(format: *const c_char, ...)>,
    pub rumble: Option<extern "C" fn(controllerNumber: c_ushort, lowFreqMotor: c_ushort, highFreqMotor: c_ushort)>,
    pub connectionStatusUpdate: Option<extern "C" fn(connectionStatus: c_int)>,
    pub setHdrMode: Option<extern "C" fn(enabled: bool)>,
    pub rumbleTriggers: Option<extern "C" fn(controllerNumber: c_ushort, leftTrigger: c_ushort, rightTrigger: c_ushort)>,
    pub setMotionEventState: Option<extern "C" fn(controllerNumber: c_ushort, motionType: c_uchar, reportRateHz: c_ushort)>,
    pub setControllerLED: Option<extern "C" fn(controllerNumber: c_ushort, r: c_uchar, g: c_uchar, b: c_uchar)>,
    pub setAdaptiveTriggers: Option<extern "C" fn(controllerNumber: c_ushort, eventFlags: c_uchar, typeLeft: c_uchar, typeRight: c_uchar, left: *mut c_uchar, right: *mut c_uchar)>,
}

// External C functions from moonlight-common-c
#[link(name = "moonlight-common-c")]
extern "C" {
    // Input functions
    pub fn LiSendMouseMoveEvent(deltaX: c_short, deltaY: c_short);
    pub fn LiSendMousePositionEvent(x: c_short, y: c_short, referenceWidth: c_short, referenceHeight: c_short);
    pub fn LiSendMouseMoveAsMousePositionEvent(deltaX: c_short, deltaY: c_short, referenceWidth: c_short, referenceHeight: c_short);
    pub fn LiSendMouseButtonEvent(buttonEvent: c_uchar, mouseButton: c_uchar);
    pub fn LiSendMultiControllerEvent(
        controllerNumber: c_short,
        activeGamepadMask: c_short,
        buttonFlags: c_int,
        leftTrigger: c_uchar,
        rightTrigger: c_uchar,
        leftStickX: c_short,
        leftStickY: c_short,
        rightStickX: c_short,
        rightStickY: c_short,
    );
    pub fn LiSendTouchEvent(
        eventType: c_uchar,
        pointerId: c_int,
        x: f32,
        y: f32,
        pressureOrDistance: f32,
        contactAreaMajor: f32,
        contactAreaMinor: f32,
        rotation: c_short,
    ) -> c_int;
    pub fn LiSendPenEvent(
        eventType: c_uchar,
        toolType: c_uchar,
        penButtons: c_uchar,
        x: f32,
        y: f32,
        pressureOrDistance: f32,
        contactAreaMajor: f32,
        contactAreaMinor: f32,
        rotation: c_short,
        tilt: c_uchar,
    ) -> c_int;
    pub fn LiSendControllerArrivalEvent(
        controllerNumber: c_uchar,
        activeGamepadMask: c_short,
        controllerType: c_uchar,
        supportedButtonFlags: c_int,
        capabilities: c_short,
    ) -> c_int;
    pub fn LiSendControllerTouchEvent(
        controllerNumber: c_uchar,
        eventType: c_uchar,
        pointerId: c_int,
        x: f32,
        y: f32,
        pressure: f32,
    ) -> c_int;
    pub fn LiSendControllerMotionEvent(
        controllerNumber: c_uchar,
        motionType: c_uchar,
        x: f32,
        y: f32,
        z: f32,
    ) -> c_int;
    pub fn LiSendControllerBatteryEvent(
        controllerNumber: c_uchar,
        batteryState: c_uchar,
        batteryPercentage: c_uchar,
    ) -> c_int;
    pub fn LiSendKeyboardEvent2(
        keyCode: c_short,
        keyAction: c_uchar,
        modifiers: c_uchar,
        flags: c_uchar,
    );
    pub fn LiSendHighResScrollEvent(scrollAmount: c_short);
    pub fn LiSendHighResHScrollEvent(scrollAmount: c_short);
    pub fn LiSendUtf8TextEvent(text: *const c_char, length: size_t);

    // Connection functions
    pub fn LiStopConnection();
    pub fn LiInterruptConnection();
    pub fn LiStartConnection(
        serverInfo: *const SERVER_INFORMATION,
        streamConfig: *const STREAM_CONFIGURATION,
        connCallbacks: *const CONNECTION_LISTENER_CALLBACKS,
        videoCallbacks: *const DECODER_RENDERER_CALLBACKS,
        audioCallbacks: *const AUDIO_RENDERER_CALLBACKS,
        platformInfo: *const c_void,
        platformInfoSize: c_int,
        videoContext: *const c_void,
        videoContextSize: c_int,
    ) -> c_int;

    // Info functions
    pub fn LiGetStageName(stage: c_int) -> *const c_char;
    pub fn LiFindExternalAddressIP4(
        stunHostName: *const c_char,
        stunPort: c_int,
        wanAddr: *mut c_uint,
    ) -> c_int;
    pub fn LiGetPendingAudioDuration() -> c_int;
    pub fn LiGetPendingVideoFrames() -> c_int;
    pub fn LiTestClientConnectivity(
        testServerHostName: *const c_char,
        referencePort: c_ushort,
        testFlags: c_int,
    ) -> c_int;
    pub fn LiGetPortFlagsFromStage(stage: c_int) -> c_int;
    pub fn LiGetPortFlagsFromTerminationErrorCode(errorCode: c_int) -> c_int;
    pub fn LiStringifyPortFlags(
        portFlags: c_int,
        separator: *const c_char,
        outputBuffer: *mut c_char,
        outputBufferSize: size_t,
    );
    pub fn LiGetEstimatedRttInfo(rtt: *mut c_uint, variance: *mut c_uint) -> bool;
    pub fn LiGetLaunchUrlQueryParameters() -> *const c_char;
    pub fn LiGetHdrMetadata(hdrMetadata: *mut SS_HDR_METADATA) -> bool;
}

