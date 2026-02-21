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

### Minimum SoC Requirements

Deragabu requires hardware with MediaCodec C2 low-latency decoder support. The minimum supported SoCs are:
•	Qualcomm Snapdragon: Snapdragon 845 (Adreno 630) or newer
•	MediaTek Dimensity: Dimensity 7300 or newer (with `c2.mtk.hevc.decoder.lowlatency` support)
•	Samsung Exynos: Not recommended due to inconsistent low-latency implementation
•	Google Tensor: Tensor G3 or newer (Pixel 8 series and later)

**Note**: Devices must support `FEATURE_LowLatency` or vendor-specific low latency options for optimal streaming performance.

## Features
* Stream games from your PC to your Android device
* **WireGuard VPN integration** - Built-in WireGuard tunnel support for secure remote streaming without a separate VPN app; configure your WireGuard profile directly within the app to establish an encrypted tunnel before connecting to your host
* **Persistent USB controller permissions** - No need to re-authorize USB controllers every time they reconnect (Includes full Razer Kishi series support)
* On-screen keyboard input with text entry bar and "Send + Enter" shortcut
* Auto-copy PIN to clipboard and auto-open browser during pairing process
* Improved frame statistics tracking and performance monitoring
* Support for HEVC (H.265) and AV1 video codecs for better quality streaming
* Minimum streaming resolution of 1080P for enhanced visual experience
* Streamlined controller connection experience with "Use by default" option for persistent authorization
* Better code organization with extracted methods for handling SPS/PPS/VPS and frame statistics
* Major refactoring of the `submitDecodeUnit` function for improved readability and maintainability
* Enhanced vibration simulation with proper detection of vibration capabilities for all connected controllers

For a detailed summary of changes, see [CHANGES_SUMMARY.md](docs/CHANGES_SUMMARY.md).

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