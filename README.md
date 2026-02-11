# Deragabu

Deragabu is a modified version of [Moonlight Android](https://github.com/moonlight-stream/moonlight-android), an open source client for [Sunshine](https://github.com/LizardByte/Sunshine) and NVIDIA GameStream.

Deragabu will allow you to stream your full collection of games from your PC to your Android device,
whether in your own home or over the internet.

## Requirements

| Requirement | Specification |
|-------------|---------------|
| **Android Version** | Android 13+ (API 33) |
| **CPU Architecture** | ARM64 only |
| **Video Decoder** | MediaCodec C2 decoder required |
| **Video Codec** | HEVC (H.265) or AV1 only |
| **Minimum Resolution** | 1080P |

## Features
* Stream games from your PC to your Android device
* **Persistent USB controller permissions** - No need to re-authorize USB controllers every time they reconnect
* On-screen keyboard input with text entry bar
* Auto-copy PIN to clipboard and auto-open browser during pairing process
* Improved frame statistics tracking and performance monitoring
* 

## Recent Changes

### Video Format & Resolution Updates
- 🎥 **Removed H.264 (AVC) support** - Only HEVC (H.265) and AV1 codecs are supported for better quality
- 📺 **Removed 720P support** - Minimum streaming resolution is now 1080P
- ⬆️ **Default resolution upgraded** - New installations default to 1080P instead of 720P
- 🔄 **Legacy 720P settings auto-upgraded** - Existing 720P configurations automatically upgraded to 1080P

### USB Controller Improvements
- ✨ **Persistent USB permissions** - USB controllers now retain authorization after first connection
- 🎮 **Full Razer Kishi series support** - Added complete support for all Kishi models (original, V2, V2 Pro, Ultra)
- 📳 **Enhanced vibration simulation** - Better rumble support with proper vibration capability detection for all connected controllers
- 📱 Streamlined controller connection experience - Select "Use by default" once, never authorize again

### Keyboard Input Enhancement
- ⌨️ **Send + Enter button** - New button to send text and automatically press Enter key

### Pairing Process Improvements
- 📋 **Auto-copy PIN to clipboard** - PIN code is automatically copied when pairing starts
- 🌐 **Auto-open browser** - Browser automatically opens to server's web interface during pairing
- 📱 Added Android 13+ notification permission handling for foreground service

### Code Quality
- 🔧 **Major refactoring** of `submitDecodeUnit` function - Reduced from 345+ lines to modular functions
- 📊 Improved frame statistics tracking and performance monitoring
- 🎨 Better code organization with extracted methods for SPS/PPS/VPS handling

## Building
* Install Android Studio and the Android NDK
* Run ‘git submodule update --init --recursive’ from within moonlight-android/
* In moonlight-android/, create a file called ‘local.properties’. Add an ‘ndk.dir=’ property to the local.properties file and set it equal to your NDK directory.
* Build the APK using Android Studio or gradle

## Authors

* [Cameron Gutman](https://github.com/cgutman)  
* [Diego Waxemberg](https://github.com/dwaxemberg)  
* [Aaron Neyer](https://github.com/Aaronneyer)  
* [Andrew Hennessy](https://github.com/yetanothername)
* [qwe7002](https://github.com/qwe7002)