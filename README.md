# Deragabu

Deragabu is an open source client for [Sunshine](https://github.com/LizardByte/Sunshine).

Deragabu will allow you to stream your full collection of games from your PC to your Android device,
whether in your own home or over the internet.

## Features
* Stream games from your PC to your Android device
* Support Sunshine
* **Persistent USB controller permissions** - No need to re-authorize USB controllers every time they reconnect
  * Supports Xbox One, Xbox 360, and Xbox 360 Wireless controllers
  * Supports multiple manufacturers: Microsoft, Mad Catz, Razer, PowerA, Hori, and more
  * **Full Razer Kishi series support**: Kishi, Kishi V2, Kishi V2 Pro, Kishi Ultra
  * Select "Use by default for this USB device" on first connection for seamless experience
* On-screen keyboard input with text entry bar
  * **Send** - Send text without Enter key
  * **Send + Enter** - Send text and automatically press Enter
  * **Cancel** - Close the input bar

## Recent Changes

### USB Controller Improvements
- ✨ **Persistent USB permissions** - USB controllers now retain authorization after first connection
- 🎮 **Full Razer Kishi series support** - Added complete support for all Kishi models (original, V2, V2 Pro, Ultra)
- 📱 Streamlined controller connection experience - Select "Use by default" once, never authorize again

### Keyboard Input Enhancement
- ⌨️ **Send + Enter button** - New button to send text and automatically press Enter key

### Pairing Process Improvements
- 🔒 Fixed pairing port selection - Now correctly uses HTTP port (47989) instead of HTTPS port
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