//! FFI bindings to moonlight-common-c (Limelight) and Opus
//!
//! This module contains auto-generated bindings from bindgen,
//! as well as manually defined types and functions.

// Include auto-generated bindings
// include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// For now, we'll define the bindings manually until bindgen is properly set up
use libc::{c_char, c_int, c_short, c_uchar, c_uint, c_ushort, c_void};

// Stream configuration constants
pub const STREAM_CFG_LOCAL: c_int = 0;
pub const STREAM_CFG_REMOTE: c_int = 1;
pub const STREAM_CFG_AUTO: c_int = 2;

// Colorspace constants
pub const COLORSPACE_REC_601: c_int = 0;
pub const COLORSPACE_REC_709: c_int = 1;
pub const COLORSPACE_REC_2020: c_int = 2;

// Color range constants
pub const COLOR_RANGE_LIMITED: c_int = 0;
pub const COLOR_RANGE_FULL: c_int = 1;

// Encryption flags
pub const ENCFLG_NONE: c_int = 0x00000000;
pub const ENCFLG_AUDIO: c_int = 0x00000001;
pub const ENCFLG_VIDEO: c_int = 0x00000002;
pub const ENCFLG_ALL: c_int = 0x7FFFFFFF;

// Buffer types
pub const BUFFER_TYPE_PICDATA: c_int = 0x00;
pub const BUFFER_TYPE_SPS: c_int = 0x01;
pub const BUFFER_TYPE_PPS: c_int = 0x02;
pub const BUFFER_TYPE_VPS: c_int = 0x03;

// Frame types
pub const FRAME_TYPE_PFRAME: c_int = 0x00;
pub const FRAME_TYPE_IDR: c_int = 0x01;

// Decoder renderer return codes
pub const DR_OK: c_int = 0;
pub const DR_NEED_IDR: c_int = -1;

// Controller types (from Limelight.h)
pub const LI_CTYPE_UNKNOWN: i8 = 0x00;
pub const LI_CTYPE_XBOX: i8 = 0x01;
pub const LI_CTYPE_PS: i8 = 0x02;
pub const LI_CTYPE_NINTENDO: i8 = 0x03;

// Capability flags
pub const CAPABILITY_DIRECT_SUBMIT: c_int = 0x1;
pub const CAPABILITY_REFERENCE_FRAME_INVALIDATION_AVC: c_int = 0x2;
pub const CAPABILITY_REFERENCE_FRAME_INVALIDATION_HEVC: c_int = 0x4;
pub const CAPABILITY_SLOW_OPUS_DECODER: c_int = 0x8;
pub const CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION: c_int = 0x10;
pub const CAPABILITY_PULL_RENDERER: c_int = 0x20;
pub const CAPABILITY_REFERENCE_FRAME_INVALIDATION_AV1: c_int = 0x40;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
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
    pub remoteInputAesKey: [u8; 16],
    pub remoteInputAesIv: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SERVER_INFORMATION {
    pub address: *const c_char,
    pub serverInfoAppVersion: *const c_char,
    pub serverInfoGfeVersion: *const c_char,
    pub rtspSessionUrl: *const c_char,
    pub serverCodecModeSupport: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct LENTRY {
    pub next: *mut LENTRY,
    pub data: *mut c_char,
    pub length: c_int,
    pub bufferType: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DECODE_UNIT {
    pub frameNumber: c_int,
    pub frameType: c_int,
    pub frameHostProcessingLatency: u16,
    pub receiveTimeUs: u64,
    pub enqueueTimeUs: u64,
    pub presentationTimeUs: u64,
    pub rtpTimestamp: u32,
    pub fullLength: c_int,
    pub bufferList: *mut LENTRY,
    pub hdrActive: bool,
    pub colorspace: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct OPUS_MULTISTREAM_CONFIGURATION {
    pub sampleRate: c_int,
    pub channelCount: c_int,
    pub streams: c_int,
    pub coupledStreams: c_int,
    pub samplesPerFrame: c_int,
    pub mapping: [c_uchar; 8],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SS_HDR_METADATA {
    pub displayPrimaries: [[u16; 2]; 3],
    pub whitePoint: [u16; 2],
    pub maxDisplayMasteringLuminance: u16,
    pub minDisplayMasteringLuminance: u16,
    pub maxContentLightLevel: u16,
    pub maxFrameAverageLightLevel: u16,
}

// Callback function types
pub type DecoderRendererSetup = Option<
    unsafe extern "C" fn(
        videoFormat: c_int,
        width: c_int,
        height: c_int,
        redrawRate: c_int,
        context: *mut c_void,
        drFlags: c_int,
    ) -> c_int,
>;

pub type DecoderRendererStart = Option<unsafe extern "C" fn()>;
pub type DecoderRendererStop = Option<unsafe extern "C" fn()>;
pub type DecoderRendererCleanup = Option<unsafe extern "C" fn()>;
pub type DecoderRendererSubmitDecodeUnit =
    Option<unsafe extern "C" fn(decodeUnit: *mut DECODE_UNIT) -> c_int>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DECODER_RENDERER_CALLBACKS {
    pub setup: DecoderRendererSetup,
    pub start: DecoderRendererStart,
    pub stop: DecoderRendererStop,
    pub cleanup: DecoderRendererCleanup,
    pub submitDecodeUnit: DecoderRendererSubmitDecodeUnit,
    pub capabilities: c_int,
}

pub type AudioRendererInit = Option<
    unsafe extern "C" fn(
        audioConfiguration: c_int,
        opusConfig: *const OPUS_MULTISTREAM_CONFIGURATION,
        context: *mut c_void,
        arFlags: c_int,
    ) -> c_int,
>;

pub type AudioRendererStart = Option<unsafe extern "C" fn()>;
pub type AudioRendererStop = Option<unsafe extern "C" fn()>;
pub type AudioRendererCleanup = Option<unsafe extern "C" fn()>;
pub type AudioRendererDecodeAndPlaySample =
    Option<unsafe extern "C" fn(sampleData: *mut c_char, sampleLength: c_int)>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AUDIO_RENDERER_CALLBACKS {
    pub init: AudioRendererInit,
    pub start: AudioRendererStart,
    pub stop: AudioRendererStop,
    pub cleanup: AudioRendererCleanup,
    pub decodeAndPlaySample: AudioRendererDecodeAndPlaySample,
    pub capabilities: c_int,
}

pub type ConnListenerStageStarting = Option<unsafe extern "C" fn(stage: c_int)>;
pub type ConnListenerStageComplete = Option<unsafe extern "C" fn(stage: c_int)>;
pub type ConnListenerStageFailed = Option<unsafe extern "C" fn(stage: c_int, errorCode: c_int)>;
pub type ConnListenerConnectionStarted = Option<unsafe extern "C" fn()>;
pub type ConnListenerConnectionTerminated = Option<unsafe extern "C" fn(errorCode: c_int)>;
pub type ConnListenerLogMessage = Option<unsafe extern "C" fn(format: *const c_char, ...)>;
pub type ConnListenerRumble = Option<
    unsafe extern "C" fn(controllerNumber: c_ushort, lowFreqMotor: c_ushort, highFreqMotor: c_ushort),
>;
pub type ConnListenerConnectionStatusUpdate = Option<unsafe extern "C" fn(connectionStatus: c_int)>;
pub type ConnListenerSetHdrMode = Option<unsafe extern "C" fn(enabled: bool)>;
pub type ConnListenerRumbleTriggers = Option<
    unsafe extern "C" fn(controllerNumber: c_ushort, leftTrigger: c_ushort, rightTrigger: c_ushort),
>;
pub type ConnListenerSetMotionEventState = Option<
    unsafe extern "C" fn(controllerNumber: u16, motionType: u8, reportRateHz: u16),
>;
pub type ConnListenerSetControllerLED = Option<
    unsafe extern "C" fn(controllerNumber: u16, r: u8, g: u8, b: u8),
>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CONNECTION_LISTENER_CALLBACKS {
    pub stageStarting: ConnListenerStageStarting,
    pub stageComplete: ConnListenerStageComplete,
    pub stageFailed: ConnListenerStageFailed,
    pub connectionStarted: ConnListenerConnectionStarted,
    pub connectionTerminated: ConnListenerConnectionTerminated,
    pub logMessage: ConnListenerLogMessage,
    pub rumble: ConnListenerRumble,
    pub connectionStatusUpdate: ConnListenerConnectionStatusUpdate,
    pub setHdrMode: ConnListenerSetHdrMode,
    pub rumbleTriggers: ConnListenerRumbleTriggers,
    pub setMotionEventState: ConnListenerSetMotionEventState,
    pub setControllerLED: ConnListenerSetControllerLED,
}

// Opus decoder - now using Rust implementation via audiopus crate
// See src/opus.rs for the Rust Opus wrapper

// External functions from moonlight-common-c
extern "C" {
    // Input functions
    pub fn LiSendMouseMoveEvent(deltaX: c_short, deltaY: c_short);
    pub fn LiSendMousePositionEvent(
        x: c_short,
        y: c_short,
        referenceWidth: c_short,
        referenceHeight: c_short,
    );
    pub fn LiSendMouseMoveAsMousePositionEvent(
        deltaX: c_short,
        deltaY: c_short,
        referenceWidth: c_short,
        referenceHeight: c_short,
    );
    pub fn LiSendMouseButtonEvent(buttonEvent: c_char, mouseButton: c_char);
    pub fn LiSendMultiControllerEvent(
        controllerNumber: c_short,
        activeGamepadMask: c_short,
        buttonFlags: c_int,
        leftTrigger: c_char,
        rightTrigger: c_char,
        leftStickX: c_short,
        leftStickY: c_short,
        rightStickX: c_short,
        rightStickY: c_short,
    );
    pub fn LiSendTouchEvent(
        eventType: c_char,
        pointerId: c_int,
        x: f32,
        y: f32,
        pressureOrDistance: f32,
        contactAreaMajor: f32,
        contactAreaMinor: f32,
        rotation: c_short,
    ) -> c_int;
    pub fn LiSendPenEvent(
        eventType: c_char,
        toolType: c_char,
        penButtons: c_char,
        x: f32,
        y: f32,
        pressureOrDistance: f32,
        contactAreaMajor: f32,
        contactAreaMinor: f32,
        rotation: c_short,
        tilt: c_char,
    ) -> c_int;
    pub fn LiSendControllerArrivalEvent(
        controllerNumber: c_char,
        activeGamepadMask: c_short,
        controllerType: c_char,
        supportedButtonFlags: c_int,
        capabilities: c_short,
    ) -> c_int;
    pub fn LiSendControllerTouchEvent(
        controllerNumber: c_char,
        eventType: c_char,
        pointerId: c_int,
        x: f32,
        y: f32,
        pressure: f32,
    ) -> c_int;
    pub fn LiSendControllerMotionEvent(
        controllerNumber: c_char,
        motionType: c_char,
        x: f32,
        y: f32,
        z: f32,
    ) -> c_int;
    pub fn LiSendControllerBatteryEvent(
        controllerNumber: c_char,
        batteryState: c_char,
        batteryPercentage: c_char,
    ) -> c_int;
    pub fn LiSendKeyboardEvent2(
        keyCode: c_short,
        keyAction: c_char,
        modifiers: c_char,
        flags: c_char,
    );
    pub fn LiSendHighResScrollEvent(scrollAmount: c_short);
    pub fn LiSendHighResHScrollEvent(scrollAmount: c_short);
    pub fn LiSendUtf8TextEvent(text: *const c_char, length: c_uint);

    // Connection functions
    pub fn LiStartConnection(
        serverInfo: *const SERVER_INFORMATION,
        streamConfig: *const STREAM_CONFIGURATION,
        clCallbacks: *const CONNECTION_LISTENER_CALLBACKS,
        drCallbacks: *const DECODER_RENDERER_CALLBACKS,
        arCallbacks: *const AUDIO_RENDERER_CALLBACKS,
        platformInfo: *mut c_void,
        platformInfoSize: c_int,
        videoInfo: *mut c_void,
        videoInfoSize: c_int,
    ) -> c_int;
    pub fn LiStopConnection();
    pub fn LiInterruptConnection();
    pub fn LiGetStageName(stage: c_int) -> *const c_char;

    // Network utilities
    pub fn LiFindExternalAddressIP4(
        stunHostName: *const c_char,
        stunPort: c_int,
        wanAddr: *mut c_uint,
    ) -> c_int;
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
        outputBufferSize: c_int,
    );

    // Status functions
    pub fn LiGetPendingAudioDuration() -> c_int;
    pub fn LiGetPendingVideoFrames() -> c_int;
    pub fn LiGetEstimatedRttInfo(rtt: *mut u32, variance: *mut u32) -> bool;
    pub fn LiGetLaunchUrlQueryParameters() -> *const c_char;
    pub fn LiGetHdrMetadata(hdrMetadata: *mut SS_HDR_METADATA) -> bool;
}

