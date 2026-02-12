# Deragabu - Moonlight Android Modification Summary

Deragabu is a modified version of [Moonlight Android](https://github.com/moonlight-stream/moonlight-android), focused on providing enhanced high-quality streaming experiences and improved game controller support.[1]

## Hard Requirements

| Requirement | Specification |
|---------|------|
| **Android Version** | Android 13+ (API 33) |
| **CPU Architecture** | ARM64 only |
| **Video Decoder** | MediaCodec C2 decoder required |
| **Video Encoding** | HEVC (H.265) or AV1 only |
| **Minimum Resolution** | 1080P |

***

## 1. Removal of H.264 and 720P Support

### Purpose
Focus on high-quality streaming experiences by removing older encoding formats and low-resolution options.[1]

### Modification Details

#### Video Formats
- **Removed H.264 (AVC) support**:
    - Removed `FORCE_H264` option from `FormatOption` enumeration
    - Removed H.264 option from video format selection arrays
    - Updated supported format detection in `Game.java` to be HEVC-based
    - Removed H.264 forced check in `DecoderCapabilityChecker`
    - Changed default supported format from `VIDEO_FORMAT_H264` to `VIDEO_FORMAT_H265`

#### Resolution
- **Removed 720P (1280x720) support**:
    - Removed 720P option from `arrays.xml` resolution list
    - Removed `RES_720P` constant from `PreferenceConfiguration`
    - Updated `isNativeResolution()` method to remove 720P check
    - Upgraded default resolution from 720P to 1080P
    - Updated `StreamConfiguration` default width and height to 1920x1080

#### Backward Compatibility
- **Automatic upgrade of legacy settings**:
    - `convertFromLegacyResolutionString()` maps 720P to 1080P
    - Legacy 720P settings in `readPreferences()` automatically upgrade to 1080P
    - Removed 720P entry from bitrate calculation table and readjusted interpolation

### Affected Files
1. **arrays.xml**: Removed 720P and H.264 options
2. **PreferenceConfiguration.java**: Updated default values and enumerations
3. **StreamConfiguration.java**: Updated default resolution and video format
4. **Game.java**: Removed H.264 support detection
5. **DecoderCapabilityChecker.java**: Removed FORCE_H264 check

### Effects
- All streams now use at least 1080P resolution
- Only HEVC (H.265) and AV1 encoding supported[2][3]
- Legacy 720P configurations on older devices automatically upgrade to 1080P
- Improved overall streaming quality and performance
- Simplified settings options, reducing user confusion

## 2. Persistent USB Permissions, Razer Kishi Series Support, and Vibration Improvements

### Issues
1. Android requests USB permissions again each time a USB game controller is connected, interrupting the gaming experience.[4][5]
2. Vibration test functionality attempts to vibrate without checking if the controller supports vibration.

### Fix Details

#### USB Permission Persistence
- **usb_device_filter.xml** (new file):
    - Created USB device filter declaring all supported Xbox controllers (Xbox One, Xbox 360, Xbox 360 Wireless)
    - Includes all supported vendor IDs (Microsoft, Mad Catz, Razer, PowerA, Hori, etc.)
    - **Explicitly added Razer Kishi series support**: Kishi, Kishi V2, Kishi V2 Pro, Kishi Ultra

- **AndroidManifest.xml**:
    - Added `USB_DEVICE_ATTACHED` intent-filter to `UsbDriverService`
    - Added meta-data pointing to USB device filter resource
    - System automatically grants permissions when supported USB devices are connected

- **XboxOneController.java**:
    - Updated Razer Vendor ID comments to explicitly list supported Kishi series models

- **Xbox360Controller.java**:
    - Updated Razer Vendor ID comments to include Kishi series

#### Vibration Function Improvements
- **GamepadTestActivity.java**:
    - Added `hasVibrator` property to `VibratorInfo` class
    - Check `device.getVibrator().hasVibrator()` when scanning game controllers
    - **Only controllers supporting vibration are added to the vibration test list**
    - Avoids invalid vibration operations on non-vibration controllers
    - Provides more accurate vibration capability detection and better user experience

### Supported Razer Controllers
- Razer Wildcat (Xbox One)
- Razer Kishi (original, Android USB-C)
- Razer Kishi V2 (Android USB-C)
- Razer Kishi V2 Pro (Android USB-C)
- Razer Kishi Ultra (Android USB-C)
- Razer Sabertooth (Xbox 360)
- Razer Onza (Xbox 360, using different Vendor ID 0x1689)

### Effects
- First time connecting a USB controller, the system asks if the app should be allowed to use the device
- After checking "Use by default", reconnecting the same device no longer requires re-authorization
- Significantly improves user experience, especially for frequent connect/disconnect scenarios
- **Razer Kishi series users can now enjoy seamless connection experience**
- **Vibration testing now only tests controllers that support vibration**, avoiding invalid operations
- Provides more accurate controller vibration capability detection

## 3. Keyboard Input Field Enhancement

### Feature
Added "Send and Enter" button to keyboard input field for quick command sending in games.

### Implementation Details
- **activity_game.xml**:
    - Added `keyboardInputSendEnter` button

- **strings.xml**:
    - Added `keyboard_input_send_enter` string resource

- **Game.java**:
    - Added click handler for "Send and Enter" button
    - Added `sendEnterKey()` method to send Enter key event
    - Added `sendEnterKeyDelayed()` method with 100ms delay to ensure text is processed first

### Button Descriptions
- **Send** - Send text only
- **Send + Enter** - Send text and automatically press Enter key
- **Cancel** - Close input field

## 4. Pairing Implementation Bug Fix

### Issue
Created `AddressTuple` in `PairingService` with incorrect port. Should use HTTP port (typically 47989), not HTTPS port (typically 47984).

### Fix Details
- **PairingService.java**:
    - Added `EXTRA_COMPUTER_HTTP_PORT` constant
    - Updated `doPairing` method signature to add `httpPort` parameter
    - Fixed `AddressTuple` creation logic to use correct HTTP port

- **PcView.java**:
    - Pass `computer.activeAddress.port` (HTTP port) when starting PairingService

## 5. Correct Notification Permission Request (Android 13+)

### Issue
On Android 13 (API 33+), `POST_NOTIFICATIONS` permission needs runtime request. Pairing service starts foreground service without requesting this permission.

### Fix Details
- **PcView.java**:
    - Added `REQUEST_NOTIFICATION_PERMISSION` constant
    - Added `pendingPairComputer` field to save pending pairing computer
    - Check notification permission in `doPair` method
    - Added `onRequestPermissionsResult` to handle permission request results
    - Extracted pairing service startup logic to `startPairingService` method

## 6. Auto-Copy PIN and Open Browser During Pairing

### Feature
When pairing starts:
1. Automatically copy PIN code to clipboard
2. Automatically open browser to server's web interface (HTTPS port)

### Implementation Details
- **PairingService.java**:
    - Added logic to auto-copy PIN to clipboard in `onStartCommand`
    - Added `openPairingWebPage` method to open browser to server's HTTPS address
    - Supports IPv6 address formatting

- **strings.xml**:
    - Added `pair_browser_open_failed` string resource

## 7. Simplified Add Server Dialog

### Issue
Originally used separate Activity (`AddComputerManually`) to add server, user experience was not smooth.

### Fix Details
- **Dialog.java**:
    - Added `AddComputerCallback` interface
    - Added `displayAddComputerDialog` method to display input dialog
    - Dialog includes input field, confirm and cancel buttons
    - Supports IME actions (Enter key submission)

- **PcView.java**:
    - Modified add server button click event to call dialog instead of launching Activity
    - Added `showAddComputerDialog` method to handle dialog logic
    - Added `parseHostInput` method to parse user input (supports IPv4, IPv6, hostnames and ports)

## 8. Simplified Performance Stats Notification Content

### Feature
- Simplified performance stats notification display content
- Only show key metrics: FPS, Bitrate, RTT (latency)
- Format: `60.00 FPS | 15.20 Mbps | 2 ms`

### Implementation Details
- **StatsNotificationHelper.java**:
    - Added `simplifyStatsText` method to parse and simplify stats text
    - Use `lastIndexOf` to correctly find starting position of values
    - Extract FPS, Mbps and RTT from full stats text
    - Removed BigTextStyle, using more compact format

## 9. Xiaomi HyperOS 3 Capsule Support

### Feature
On Xiaomi HyperOS 3+ devices, stats notification will display using Capsule mode.

### Implementation Details
- **StatsNotificationHelper.java**:
    - Added `detectXiaomiHyperOS` method to detect Xiaomi devices
    - Added `getHyperOSVersion` method to get HyperOS version
    - Added Xiaomi Capsule-related extras to notification:
        - `miui.extra.notification.use_capsule`: true
        - `miui.extra.notification.capsule_style`: 1 (compact mode)

### Reference Documentation
- https://dev.mi.com/xiaomihyperos/documentation/detail?pId=2140

## 10. Added WakeLock to Prevent Device Sleep

### Feature
Keeps device screen on during streaming to prevent device from entering sleep state.

### Implementation Details
- **Game.java**:
    - Added `PowerManager.WakeLock` field
    - Acquire `SCREEN_BRIGHT_WAKE_LOCK | ACQUIRE_CAUSES_WAKEUP` type WakeLock in `onCreate`
    - Acquire WakeLock when streaming starts
    - Release WakeLock in `onDestroy`
    - Use `setReferenceCounted(false)` to avoid reference counting issues

### Notes
- Uses `SCREEN_BRIGHT_WAKE_LOCK` to keep screen at brightest state
- Uses `ACQUIRE_CAUSES_WAKEUP` to wake device when lock is acquired
- `WAKE_LOCK` permission already declared in AndroidManifest.xml

## 11. Display Video Encoder in Performance Stats Notification

### Feature
Display currently used video encoder (H.264, HEVC, AV1, etc.) in performance stats notification title.

### Implementation Details
- **Game.java**:
    - Added `activeVideoCodec` field to save current encoder name
    - Added `getVideoCodecName` method to convert video format code to encoder name
        - Supports H.264
        - Supports HEVC / HEVC Main10
        - Supports AV1 Main8 / AV1 Main10
    - Update `activeVideoCodec` in `onPerfUpdate` and pass to StatsNotificationHelper

- **StatsNotificationHelper.java**:
    - Updated `showNotification` method to receive encoder name parameter
    - Display encoder information in notification title
    - Format: `H.264 - Moonlight Streaming` or `HEVC Main10 - Moonlight Streaming`

### Encoder Identification
- **H.264**: Basic encoder, supported by all devices[1]
- **HEVC**: H.265 encoder[3][2]
- **HEVC Main10**: H.265 10-bit HDR encoder
- **AV1**: AV1 8-bit encoder[2][3]
- **AV1 Main10**: AV1 10-bit HDR encoder

## Modified File List

1. `app/src/main/java/com/limelight/computers/PairingService.java`
2. `app/src/main/java/com/limelight/PcView.java`
3. `app/src/main/java/com/limelight/utils/Dialog.java`
4. `app/src/main/java/com/limelight/utils/StatsNotificationHelper.java`
5. `app/src/main/java/com/limelight/Game.java`
6. `app/src/main/java/com/limelight/preferences/GamepadTestActivity.java` ⭐ Vibration improvements
7. `app/src/main/res/values/strings.xml`
8. `app/src/main/res/xml/usb_device_filter.xml` ⭐ New file
9. `app/src/main/AndroidManifest.xml`

## Testing Recommendations

### USB Controller and Vibration Testing
1. Connect various USB game controllers (Xbox, PlayStation, Razer Kishi, etc.)
2. Test first-time connection permission grant flow
3. After checking "Use by default", disconnect and reconnect controller to verify no permission prompt appears
4. **Enter game controller test interface to test vibration functionality**
5. **Verify only controllers supporting vibration display vibration test options**
6. **Test that controllers without vibration support are correctly excluded**
7. Test vibration effects on Razer Kishi series

### Pairing Functionality Testing
1. Test pairing functionality on Android 13+ devices
2. Verify notification permission request appears normally
3. Verify PIN code is automatically copied to clipboard
4. Verify browser automatically opens to server web interface
5. Test IPv4 and IPv6 server addresses

### Add Server Testing
1. Test dialog input with various address formats (IP, domain, with port)
2. Test error message for empty input
3. Test cancel operation

### Performance Stats Testing
1. Enable performance stats notification
2. Verify notification content is simplified (showing only FPS, Bitrate, RTT)
3. Verify encoder name displays correctly in notification title
4. Test different video format displays (H.264, HEVC, AV1)
5. Verify Capsule display effect on Xiaomi HyperOS 3+ devices
6. Verify normal notification display on non-Xiaomi devices

### WakeLock Testing
1. After starting streaming, lock device to verify screen stays on
2. Long-duration streaming (exceeding device's default sleep time) to verify device doesn't sleep
3. After exiting streaming, verify WakeLock is correctly released
4. Verify battery consumption is within reasonable range

## Known Limitations

1. **HyperOS Detection**: Uses reflection to access system properties, some devices may fail detection
2. **Capsule Support**: Depends on Xiaomi's private API, may change in future versions
3. **Stats Text Parsing**: Based on current format, needs update if format changes
4. **WakeLock Battery Consumption**: Keeping screen on increases battery consumption, but this is necessary functionality for streaming apps

## Performance Impact

- **WakeLock**: Minor increase in battery consumption (keeping screen on)
- **Encoder Detection**: Called once per performance update, negligible performance impact
- **Stats Text Parsing**: String operations, negligible performance impact
- **HyperOS Detection**: Executed only once during initialization, no runtime impact

Sources
[1] Moonlight Game Streaming - Apps on Google Play https://play.google.com/store/apps/details?id=com.limelight&hl=en_US
[2] AV1 vs HEVC/H.265 - Which is the Codec of the Future? - WinXDVD https://www.winxdvd.com/video-transcoder/av1-vs-hevc.htm
[3] AV1 vs HEVC: Know Exactly Which Codec to Choose https://www.videoproc.com/resource/av1-vs-hevc.htm
[4] usb - Android USB_DEVICE_ATTACHED persistent permission https://stackoverflow.com/questions/40075128/android-usb-device-attached-persistent-permission
[5] Android USB_DEVICE_ATTACHED persistent permission https://stackoverflow.com/questions/40075128/android-usb-device-attached-persistent-permission/57079079
[6] Moonlight Game Streaming: Play Your PC Games Remotely https://moonlight-stream.org
[7] Moonlight Game Streaming - Google Play のアプリ https://play.google.com/store/apps/details?id=com.limelight&hl=ja
[8] Moonlight Game Streaming - Apps on Google Play https://play.google.com/store/apps/details?id=com.limelight
[9] Beginner's Guide to Moonlight: Setting Up Your Game Streaming on Android https://www.youtube.com/watch?v=6Xie5l-26tE
[10] Setup Guide · moonlight-stream/moonlight-docs Wiki https://github.com/moonlight-stream/moonlight-docs/wiki/Setup-Guide
[11] AV1 vs. HEVC: Which One is... https://www.gumlet.com/learn/av1-vs-hevc/
[12] USB host overview | Connectivity - Android Developers https://developer.android.com/develop/connectivity/usb/host
[13] [Setup Guide] Setting up Moonlight Android for full-screen video https://www.reddit.com/r/MoonlightStreaming/comments/ycko5n/setup_guide_setting_up_moonlight_android_for/
[14] AV1 vs H.265: Codec Comparison Guide [2026 Updated] - Red5 Prowww.red5.net › blog › av1-vs-h265 https://www.red5.net/blog/av1-vs-h265/
