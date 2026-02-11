package com.limelight.preferences;

import android.content.Context;
import android.hardware.input.InputManager;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.os.Bundle;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.os.VibratorManager;
import android.view.InputDevice;
import android.view.LayoutInflater;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import androidx.appcompat.app.AppCompatActivity;

import com.limelight.R;
import com.limelight.binding.input.driver.UsbDriverService;
import com.limelight.nvstream.jni.MoonBridge;
import com.limelight.utils.UiHelper;

import java.util.ArrayList;
import java.util.List;

/**
 * Activity for testing connected gamepads.
 * Displays gamepad information including type (Xbox, PlayStation, Nintendo, etc.),
 * protocol (XInput/HID), and allows testing vibration functionality.
 */
public class GamepadTestActivity extends AppCompatActivity implements InputManager.InputDeviceListener {

    private LinearLayout gamepadListContainer;
    private TextView gamepadStatus;
    private InputManager inputManager;
    private final List<GamepadInfo> detectedGamepads = new ArrayList<>();

    // Store vibrators for each gamepad to use for rumble testing
    private final List<VibratorInfo> gamepadVibrators = new ArrayList<>();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        UiHelper.setLocale(this);

        setContentView(R.layout.activity_gamepad_test);

        UiHelper.notifyNewRootView(this);

        gamepadListContainer = findViewById(R.id.gamepad_list_container);
        gamepadStatus = findViewById(R.id.gamepad_status);

        inputManager = (InputManager) getSystemService(Context.INPUT_SERVICE);

        // Setup button listeners
        setupVibrationButtons();

        // Setup refresh button
        Button refreshButton = findViewById(R.id.btn_refresh);
        refreshButton.setOnClickListener(v -> refreshGamepadList());

        // Initial scan
        refreshGamepadList();
    }

    @Override
    protected void onResume() {
        super.onResume();
        inputManager.registerInputDeviceListener(this, null);
        refreshGamepadList();
    }

    @Override
    protected void onPause() {
        super.onPause();
        inputManager.unregisterInputDeviceListener(this);
        stopAllVibration();
    }

    private void setupVibrationButtons() {
        Button btnVibrateLow = findViewById(R.id.btn_vibrate_low);
        Button btnVibrateHigh = findViewById(R.id.btn_vibrate_high);
        Button btnVibrateBoth = findViewById(R.id.btn_vibrate_both);
        Button btnVibrateStop = findViewById(R.id.btn_vibrate_stop);
        Button btnVibrateTriggerLeft = findViewById(R.id.btn_vibrate_trigger_left);
        Button btnVibrateTriggerRight = findViewById(R.id.btn_vibrate_trigger_right);

        btnVibrateLow.setOnClickListener(v -> testVibration(true, false, false, false));
        btnVibrateHigh.setOnClickListener(v -> testVibration(false, true, false, false));
        btnVibrateBoth.setOnClickListener(v -> testVibration(true, true, false, false));
        btnVibrateStop.setOnClickListener(v -> stopAllVibration());
        btnVibrateTriggerLeft.setOnClickListener(v -> testVibration(false, false, true, false));
        btnVibrateTriggerRight.setOnClickListener(v -> testVibration(false, false, false, true));
    }

    private void refreshGamepadList() {
        gamepadListContainer.removeAllViews();
        detectedGamepads.clear();
        gamepadVibrators.clear();

        // Scan for Android InputDevices
        int[] deviceIds = InputDevice.getDeviceIds();
        for (int deviceId : deviceIds) {
            InputDevice device = InputDevice.getDevice(deviceId);
            if (device != null && isGamepad(device)) {
                GamepadInfo info = createGamepadInfo(device);
                detectedGamepads.add(info);

                // Store vibrator info
                VibratorInfo vibratorInfo = new VibratorInfo();
                vibratorInfo.deviceId = deviceId;
                vibratorInfo.vibratorManager = device.getVibratorManager();
                vibratorInfo.vibrator = device.getVibrator();
                vibratorInfo.hasQuadVibrators = hasQuadAmplitudeControlledRumbleVibrators(device.getVibratorManager());
                vibratorInfo.hasDualVibrators = hasDualAmplitudeControlledRumbleVibrators(device.getVibratorManager());
                gamepadVibrators.add(vibratorInfo);
            }
        }

        // Scan for USB devices that might be gamepads
        PreferenceConfiguration prefConfig = PreferenceConfiguration.readPreferences(this);
        if (prefConfig.usbDriver) {
            UsbManager usbManager = (UsbManager) getSystemService(Context.USB_SERVICE);
            if (usbManager != null) {
                for (UsbDevice dev : usbManager.getDeviceList().values()) {
                    if (UsbDriverService.shouldClaimDevice(dev, false) &&
                            !UsbDriverService.isRecognizedInputDevice(dev)) {
                        GamepadInfo info = createUsbGamepadInfo(dev);
                        detectedGamepads.add(info);
                    }
                }
            }
        }

        // Update UI
        if (detectedGamepads.isEmpty()) {
            gamepadStatus.setText(R.string.gamepad_test_no_gamepad);
        } else {
            gamepadStatus.setText(getString(R.string.gamepad_test_found, detectedGamepads.size()));
        }

        // Add views for each gamepad
        LayoutInflater inflater = LayoutInflater.from(this);
        for (GamepadInfo info : detectedGamepads) {
            View itemView = inflater.inflate(R.layout.gamepad_info_item, gamepadListContainer, false);
            populateGamepadView(itemView, info);
            gamepadListContainer.addView(itemView);
        }
    }

    private boolean isGamepad(InputDevice device) {
        int sources = device.getSources();
        return ((sources & InputDevice.SOURCE_JOYSTICK) == InputDevice.SOURCE_JOYSTICK ||
                (sources & InputDevice.SOURCE_GAMEPAD) == InputDevice.SOURCE_GAMEPAD) &&
                hasJoystickAxes(device);
    }

    private boolean hasJoystickAxes(InputDevice device) {
        return device.getMotionRange(android.view.MotionEvent.AXIS_X, InputDevice.SOURCE_JOYSTICK) != null &&
                device.getMotionRange(android.view.MotionEvent.AXIS_Y, InputDevice.SOURCE_JOYSTICK) != null;
    }

    private GamepadInfo createGamepadInfo(InputDevice device) {
        GamepadInfo info = new GamepadInfo();
        info.name = device.getName();
        info.deviceId = device.getId();
        info.vendorId = device.getVendorId();
        info.productId = device.getProductId();
        info.isExternal = device.isExternal();

        // Determine controller type
        info.controllerType = getControllerTypeName(
                MoonBridge.guessControllerType(info.vendorId, info.productId));

        // Determine protocol (XInput vs HID)
        info.protocol = determineProtocol(device, info.vendorId, info.productId);

        // Check vibration support
        info.hasVibration = device.getVibrator().hasVibrator();
        info.hasAmplitudeControl = device.getVibrator().hasAmplitudeControl();
        info.hasDualMotorVibration = hasDualAmplitudeControlledRumbleVibrators(device.getVibratorManager());
        info.hasTriggerVibration = hasQuadAmplitudeControlledRumbleVibrators(device.getVibratorManager());

        // Check capabilities
        info.hasMotionSensors = device.getSensorManager().getDefaultSensor(android.hardware.Sensor.TYPE_ACCELEROMETER) != null ||
                device.getSensorManager().getDefaultSensor(android.hardware.Sensor.TYPE_GYROSCOPE) != null;

        info.hasTouchpad = device.getMotionRange(android.view.MotionEvent.AXIS_X, InputDevice.SOURCE_TOUCHPAD) != null;

        // Check for paddles and share button
        info.hasPaddles = MoonBridge.guessControllerHasPaddles(info.vendorId, info.productId);
        info.hasShareButton = MoonBridge.guessControllerHasShareButton(info.vendorId, info.productId);

        return info;
    }

    private GamepadInfo createUsbGamepadInfo(UsbDevice device) {
        GamepadInfo info = new GamepadInfo();
        info.name = device.getProductName() != null ? device.getProductName() : device.getDeviceName();
        info.deviceId = device.getDeviceId();
        info.vendorId = device.getVendorId();
        info.productId = device.getProductId();
        info.isExternal = true;
        info.isUsbDevice = true;

        // Determine controller type
        info.controllerType = getControllerTypeName(
                MoonBridge.guessControllerType(info.vendorId, info.productId));

        // USB devices using our driver are typically XInput-compatible
        info.protocol = getString(R.string.gamepad_test_protocol_usb_driver);

        // USB devices through our driver typically support vibration
        info.hasVibration = true;
        info.hasDualMotorVibration = true;

        return info;
    }

    private String getControllerTypeName(byte type) {
        switch (type) {
            case MoonBridge.LI_CTYPE_XBOX:
                return getString(R.string.gamepad_test_type_xbox);
            case MoonBridge.LI_CTYPE_PS:
                return getString(R.string.gamepad_test_type_playstation);
            case MoonBridge.LI_CTYPE_NINTENDO:
                return getString(R.string.gamepad_test_type_nintendo);
            default:
                return getString(R.string.gamepad_test_type_unknown);
        }
    }

    private String determineProtocol(InputDevice device, int vendorId, int productId) {
        String deviceName = device.getName().toLowerCase();

        // Check for known XInput device patterns
        if (deviceName.contains("xbox") || deviceName.contains("x-box")) {
            // Xbox controllers typically use XInput on Android via the xpad driver
            return getString(R.string.gamepad_test_protocol_xinput);
        }

        // Microsoft vendor ID with known Xbox product IDs
        if (vendorId == 0x045e) {
            // Microsoft Xbox controllers
            if (productId == 0x028e || // Xbox 360
                    productId == 0x02d1 || // Xbox One
                    productId == 0x02dd || // Xbox One S
                    productId == 0x02e0 || // Xbox One Elite
                    productId == 0x0b00 || // Xbox Elite Series 2
                    productId == 0x0b05 || // Xbox Elite Series 2 (new)
                    productId == 0x0b12 || // Xbox Series X
                    productId == 0x0b13) { // Xbox Series X (BT)
                return getString(R.string.gamepad_test_protocol_xinput);
            }
        }

        // Sony controllers use HID
        if (vendorId == 0x054c) {
            return getString(R.string.gamepad_test_protocol_hid);
        }

        // Nintendo controllers use HID
        if (vendorId == 0x057e) {
            return getString(R.string.gamepad_test_protocol_hid);
        }

        // Check if it's being handled as a standard HID device
        if ((device.getSources() & InputDevice.SOURCE_GAMEPAD) != 0) {
            return getString(R.string.gamepad_test_protocol_hid);
        }

        return getString(R.string.gamepad_test_protocol_generic);
    }

    private void populateGamepadView(View view, GamepadInfo info) {
        TextView nameView = view.findViewById(R.id.gamepad_name);
        TextView typeView = view.findViewById(R.id.gamepad_type);
        TextView protocolView = view.findViewById(R.id.gamepad_protocol);
        TextView vendorProductView = view.findViewById(R.id.gamepad_vendor_product);
        TextView capabilitiesView = view.findViewById(R.id.gamepad_capabilities);
        TextView vibrationView = view.findViewById(R.id.gamepad_vibration_support);

        nameView.setText(info.name);
        typeView.setText(getString(R.string.gamepad_test_controller_type, info.controllerType));
        protocolView.setText(getString(R.string.gamepad_test_protocol_label, info.protocol));
        vendorProductView.setText(getString(R.string.gamepad_test_vendor_product,
                String.format("0x%04X", info.vendorId),
                String.format("0x%04X", info.productId)));

        // Build capabilities string
        StringBuilder caps = new StringBuilder();
        if (info.hasMotionSensors) caps.append(getString(R.string.gamepad_test_cap_motion)).append(", ");
        if (info.hasTouchpad) caps.append(getString(R.string.gamepad_test_cap_touchpad)).append(", ");
        if (info.hasPaddles) caps.append(getString(R.string.gamepad_test_cap_paddles)).append(", ");
        if (info.hasShareButton) caps.append(getString(R.string.gamepad_test_cap_share)).append(", ");
        if (caps.length() > 0) {
            caps.setLength(caps.length() - 2); // Remove trailing ", "
        } else {
            caps.append(getString(R.string.gamepad_test_cap_none));
        }
        capabilitiesView.setText(getString(R.string.gamepad_test_capabilities, caps.toString()));

        // Build vibration support string
        StringBuilder vibration = new StringBuilder();
        if (info.hasVibration) {
            vibration.append(getString(R.string.gamepad_test_vibration_yes));
            if (info.hasTriggerVibration) {
                vibration.append(" (").append(getString(R.string.gamepad_test_vibration_quad)).append(")");
            } else if (info.hasDualMotorVibration) {
                vibration.append(" (").append(getString(R.string.gamepad_test_vibration_dual)).append(")");
            } else if (info.hasAmplitudeControl) {
                vibration.append(" (").append(getString(R.string.gamepad_test_vibration_amplitude)).append(")");
            }
        } else {
            vibration.append(getString(R.string.gamepad_test_vibration_no));
        }
        vibrationView.setText(getString(R.string.gamepad_test_vibration_label, vibration.toString()));
    }

    private void testVibration(boolean lowFreq, boolean highFreq, boolean leftTrigger, boolean rightTrigger) {
        for (VibratorInfo vibratorInfo : gamepadVibrators) {
            if (vibratorInfo.hasQuadVibrators && vibratorInfo.vibratorManager != null) {
                rumbleQuadVibrators(vibratorInfo.vibratorManager,
                        lowFreq ? (short)32767 : 0,
                        highFreq ? (short)32767 : 0,
                        leftTrigger ? (short)32767 : 0,
                        rightTrigger ? (short)32767 : 0);
            } else if (vibratorInfo.hasDualVibrators && vibratorInfo.vibratorManager != null) {
                rumbleDualVibrators(vibratorInfo.vibratorManager,
                        lowFreq ? (short)32767 : 0,
                        highFreq ? (short)32767 : 0);
            } else if (vibratorInfo.vibrator != null && vibratorInfo.vibrator.hasVibrator()) {
                // Use waveform to simulate different motor types on single-motor devices
                // Don't simulate trigger vibration on single-motor devices
                if (!leftTrigger && !rightTrigger) {
                    vibrateSingleMotorSimulation(vibratorInfo.vibrator, lowFreq, highFreq);
                }
            }
        }

        // If no gamepad vibrators found, try device vibrator
        // Don't simulate trigger vibration on single-motor devices
        if (gamepadVibrators.isEmpty() && !leftTrigger && !rightTrigger) {
            Vibrator deviceVibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
            if (deviceVibrator != null && deviceVibrator.hasVibrator() && (lowFreq || highFreq)) {
                vibrateSingleMotorSimulation(deviceVibrator, lowFreq, highFreq);
            }
        }
    }

    /**
     * Simulates dual-motor vibration on a single-motor device using waveforms.
     * Requires amplitude control - devices without it cannot properly simulate dual-motor vibration.
     * Low frequency: Long, slow pulses - deep rumble
     * High frequency: Short, rapid pulses - sharp buzz
     * Both: Combined pattern that alternates between both feels
     */
    private void vibrateSingleMotorSimulation(Vibrator vibrator, boolean lowFreq, boolean highFreq) {
        if (!lowFreq && !highFreq) {
            vibrator.cancel();
            return;
        }

        // Require amplitude control for proper simulation
        if (!vibrator.hasAmplitudeControl()) {
            return;
        }

        VibrationEffect effect;

        if (lowFreq && highFreq) {
            // Both motors: create a complex waveform that combines both characteristics
            // Alternates between low-freq (strong, slow) and high-freq (lighter, fast) patterns
            long[] timings = {
                0,   // delay
                80,  // low freq pulse (strong)
                20,  // pause
                30,  // high freq pulse
                10,  // pause
                30,  // high freq pulse
                10,  // pause
                80,  // low freq pulse (strong)
                20,  // pause
                30,  // high freq pulse
                10,  // pause
                30,  // high freq pulse
                10,  // pause
                80,  // low freq pulse
                20,  // pause
            };
            int[] amplitudes = {
                0,    // delay
                255,  // low freq - strong
                0,    // pause
                180,  // high freq
                0,    // pause
                180,  // high freq
                0,    // pause
                255,  // low freq - strong
                0,    // pause
                180,  // high freq
                0,    // pause
                180,  // high freq
                0,    // pause
                255,  // low freq
                0,    // pause
            };
            effect = VibrationEffect.createWaveform(timings, amplitudes, 0);
        } else if (lowFreq) {
            // Low frequency motor simulation: slow, heavy pulses
            // Creates a deep, throbbing rumble sensation
            long[] timings = {
                0,    // delay
                100,  // on - long pulse
                50,   // off - short pause
                100,  // on
                50,   // off
                100,  // on
                50,   // off
            };
            int[] amplitudes = {
                0,    // delay
                255,  // strong
                0,    // off
                230,  // slightly less
                0,    // off
                255,  // strong
                0,    // off
            };
            effect = VibrationEffect.createWaveform(timings, amplitudes, 0);
        } else {
            // High frequency motor simulation: rapid, short pulses
            // Creates a sharp, buzzing sensation
            long[] timings = {
                0,   // delay
                25,  // on - short pulse
                15,  // off
                25,  // on
                15,  // off
                25,  // on
                15,  // off
                25,  // on
                15,  // off
                25,  // on
                15,  // off
                25,  // on
                15,  // off
            };
            int[] amplitudes = {
                0,    // delay
                200,  // medium-high
                0,    // off
                180,  // slightly less
                0,    // off
                200,  // medium-high
                0,    // off
                180,  // slightly less
                0,    // off
                200,  // medium-high
                0,    // off
                180,  // slightly less
                0,    // off
            };
            effect = VibrationEffect.createWaveform(timings, amplitudes, 0);
        }

        vibrator.vibrate(effect);
    }

    private void stopAllVibration() {
        for (VibratorInfo vibratorInfo : gamepadVibrators) {
            if (vibratorInfo.vibratorManager != null) {
                vibratorInfo.vibratorManager.cancel();
            }
            if (vibratorInfo.vibrator != null) {
                vibratorInfo.vibrator.cancel();
            }
        }

        // Also cancel device vibrator
        Vibrator deviceVibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
        if (deviceVibrator != null) {
            deviceVibrator.cancel();
        }
    }

    private static boolean hasDualAmplitudeControlledRumbleVibrators(VibratorManager vm) {
        int[] vibratorIds = vm.getVibratorIds();
        if (vibratorIds.length != 2) {
            return false;
        }
        for (int vid : vibratorIds) {
            if (!vm.getVibrator(vid).hasAmplitudeControl()) {
                return false;
            }
        }
        return true;
    }

    private static boolean hasQuadAmplitudeControlledRumbleVibrators(VibratorManager vm) {
        int[] vibratorIds = vm.getVibratorIds();
        if (vibratorIds.length != 4) {
            return false;
        }
        for (int vid : vibratorIds) {
            if (!vm.getVibrator(vid).hasAmplitudeControl()) {
                return false;
            }
        }
        return true;
    }

    private void rumbleDualVibrators(VibratorManager vm, short lowFreqMotor, short highFreqMotor) {
        highFreqMotor = (short)((highFreqMotor >> 8) & 0xFF);
        lowFreqMotor = (short)((lowFreqMotor >> 8) & 0xFF);

        if (lowFreqMotor == 0 && highFreqMotor == 0) {
            vm.cancel();
            return;
        }

        int[] vibratorIds = vm.getVibratorIds();
        int[] vibratorAmplitudes = new int[] { highFreqMotor, lowFreqMotor };

        android.os.CombinedVibration.ParallelCombination combo = android.os.CombinedVibration.startParallel();

        for (int i = 0; i < vibratorIds.length; i++) {
            if (vibratorAmplitudes[i] != 0) {
                combo.addVibrator(vibratorIds[i], VibrationEffect.createOneShot(500, vibratorAmplitudes[i]));
            }
        }

        android.os.VibrationAttributes.Builder vibrationAttributes = new android.os.VibrationAttributes.Builder();
        vibrationAttributes.setUsage(android.os.VibrationAttributes.USAGE_MEDIA);

        vm.vibrate(combo.combine(), vibrationAttributes.build());
    }

    private void rumbleQuadVibrators(VibratorManager vm, short lowFreqMotor, short highFreqMotor, short leftTrigger, short rightTrigger) {
        highFreqMotor = (short)((highFreqMotor >> 8) & 0xFF);
        lowFreqMotor = (short)((lowFreqMotor >> 8) & 0xFF);
        leftTrigger = (short)((leftTrigger >> 8) & 0xFF);
        rightTrigger = (short)((rightTrigger >> 8) & 0xFF);

        if (lowFreqMotor == 0 && highFreqMotor == 0 && leftTrigger == 0 && rightTrigger == 0) {
            vm.cancel();
            return;
        }

        int[] vibratorIds = vm.getVibratorIds();
        int[] vibratorAmplitudes = new int[] { highFreqMotor, lowFreqMotor, leftTrigger, rightTrigger };

        android.os.CombinedVibration.ParallelCombination combo = android.os.CombinedVibration.startParallel();

        for (int i = 0; i < vibratorIds.length; i++) {
            if (vibratorAmplitudes[i] != 0) {
                combo.addVibrator(vibratorIds[i], VibrationEffect.createOneShot(500, vibratorAmplitudes[i]));
            }
        }

        android.os.VibrationAttributes.Builder vibrationAttributes = new android.os.VibrationAttributes.Builder();
        vibrationAttributes.setUsage(android.os.VibrationAttributes.USAGE_MEDIA);

        vm.vibrate(combo.combine(), vibrationAttributes.build());
    }

    // InputDeviceListener callbacks
    @Override
    public void onInputDeviceAdded(int deviceId) {
        refreshGamepadList();
    }

    @Override
    public void onInputDeviceRemoved(int deviceId) {
        refreshGamepadList();
    }

    @Override
    public void onInputDeviceChanged(int deviceId) {
        refreshGamepadList();
    }

    // Helper classes
    private static class GamepadInfo {
        String name;
        int deviceId;
        int vendorId;
        int productId;
        boolean isExternal;
        boolean isUsbDevice;
        String controllerType;
        String protocol;
        boolean hasVibration;
        boolean hasAmplitudeControl;
        boolean hasDualMotorVibration;
        boolean hasTriggerVibration;
        boolean hasMotionSensors;
        boolean hasTouchpad;
        boolean hasPaddles;
        boolean hasShareButton;
    }

    private static class VibratorInfo {
        int deviceId;
        VibratorManager vibratorManager;
        Vibrator vibrator;
        boolean hasQuadVibrators;
        boolean hasDualVibrators;
    }
}



