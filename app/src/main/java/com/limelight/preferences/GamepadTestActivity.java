package com.limelight.preferences;

import android.content.Context;
import android.hardware.input.InputManager;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.os.VibratorManager;
import android.view.InputDevice;
import android.view.LayoutInflater;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.SeekBar;
import android.widget.TextView;

import androidx.appcompat.app.AppCompatActivity;

import com.limelight.R;
import com.limelight.binding.input.ControllerDetection;
import com.limelight.binding.input.ControllerHandler;
import com.limelight.binding.input.driver.UsbDriverService;
import com.limelight.utils.UiHelper;

import java.util.ArrayList;
import java.util.List;

/**
 * Activity for testing connected gamepads.
 * Displays gamepad information including type (Xbox, PlayStation, Nintendo, etc.),
 * protocol (XInput/HID), and allows testing vibration functionality.
 *
 * XInput Motor Specifications:
 * - Left motor (wLeftMotorSpeed): 0-65535, heavy eccentric mass (~40g), ~20-30Hz
 * - Right motor (wRightMotorSpeed): 0-65535, light eccentric mass (~10g), ~100-150Hz
 * - Left/Right triggers (Xbox One+): 0-65535, linear resonant actuators
 */
public class GamepadTestActivity extends AppCompatActivity implements InputManager.InputDeviceListener {

    private LinearLayout gamepadListContainer;
    private TextView gamepadStatus;
    private InputManager inputManager;
    private final List<GamepadInfo> detectedGamepads = new ArrayList<>();

    // Store vibrators for each gamepad to use for rumble testing
    private final List<VibratorInfo> gamepadVibrators = new ArrayList<>();

    // XInput intensity sliders (0-65535 range, displayed as 0-100%)
    private SeekBar seekBarLeftMotor;
    private SeekBar seekBarRightMotor;
    private SeekBar seekBarLeftTrigger;
    private SeekBar seekBarRightTrigger;
    private TextView textLeftMotorValue;
    private TextView textRightMotorValue;
    private TextView textLeftTriggerValue;
    private TextView textRightTriggerValue;

    // Continuous vibration handler
    private Handler vibrationHandler;
    private Runnable vibrationRunnable;
    private boolean isVibrating = false;

    // XInput constants
    private static final int XINPUT_MAX_VALUE = 65535;
    private static final int SLIDER_MAX = 100;  // Slider shows percentage

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        UiHelper.setLocale(this);

        setContentView(R.layout.activity_gamepad_test);

        UiHelper.notifyNewRootView(this);

        gamepadListContainer = findViewById(R.id.gamepad_list_container);
        gamepadStatus = findViewById(R.id.gamepad_status);

        inputManager = (InputManager) getSystemService(Context.INPUT_SERVICE);

        // Initialize vibration handler for continuous updates
        vibrationHandler = new Handler(Looper.getMainLooper());

        // Setup XInput intensity sliders
        setupXInputSliders();

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
        stopContinuousVibration();
        stopAllVibration();
    }

    /**
     * Setup XInput-compatible intensity sliders for precise motor control.
     * Each slider represents 0-100% of the XInput 0-65535 range.
     */
    private void setupXInputSliders() {
        // Left motor (low frequency) slider
        seekBarLeftMotor = findViewById(R.id.seekbar_left_motor);
        textLeftMotorValue = findViewById(R.id.text_left_motor_value);

        // Right motor (high frequency) slider
        seekBarRightMotor = findViewById(R.id.seekbar_right_motor);
        textRightMotorValue = findViewById(R.id.text_right_motor_value);

        // Left trigger slider
        seekBarLeftTrigger = findViewById(R.id.seekbar_left_trigger);
        textLeftTriggerValue = findViewById(R.id.text_left_trigger_value);

        // Right trigger slider
        seekBarRightTrigger = findViewById(R.id.seekbar_right_trigger);
        textRightTriggerValue = findViewById(R.id.text_right_trigger_value);

        // Setup listeners for real-time value display
        SeekBar.OnSeekBarChangeListener sliderListener = new SeekBar.OnSeekBarChangeListener() {
            @Override
            public void onProgressChanged(SeekBar seekBar, int progress, boolean fromUser) {
                updateSliderValueDisplay();
                if (fromUser && isVibrating) {
                    // Update vibration in real-time when sliders change
                    applyCurrentSliderVibration();
                }
            }

            @Override
            public void onStartTrackingTouch(SeekBar seekBar) {}

            @Override
            public void onStopTrackingTouch(SeekBar seekBar) {}
        };

        if (seekBarLeftMotor != null) {
            seekBarLeftMotor.setMax(SLIDER_MAX);
            seekBarLeftMotor.setOnSeekBarChangeListener(sliderListener);
        }
        if (seekBarRightMotor != null) {
            seekBarRightMotor.setMax(SLIDER_MAX);
            seekBarRightMotor.setOnSeekBarChangeListener(sliderListener);
        }
        if (seekBarLeftTrigger != null) {
            seekBarLeftTrigger.setMax(SLIDER_MAX);
            seekBarLeftTrigger.setOnSeekBarChangeListener(sliderListener);
        }
        if (seekBarRightTrigger != null) {
            seekBarRightTrigger.setMax(SLIDER_MAX);
            seekBarRightTrigger.setOnSeekBarChangeListener(sliderListener);
        }

        updateSliderValueDisplay();
    }

    /**
     * Updates the text displays showing current XInput values.
     */
    private void updateSliderValueDisplay() {
        if (textLeftMotorValue != null && seekBarLeftMotor != null) {
            int xinputValue = percentToXInput(seekBarLeftMotor.getProgress());
            textLeftMotorValue.setText(String.format("%d%% (%d)", seekBarLeftMotor.getProgress(), xinputValue));
        }
        if (textRightMotorValue != null && seekBarRightMotor != null) {
            int xinputValue = percentToXInput(seekBarRightMotor.getProgress());
            textRightMotorValue.setText(String.format("%d%% (%d)", seekBarRightMotor.getProgress(), xinputValue));
        }
        if (textLeftTriggerValue != null && seekBarLeftTrigger != null) {
            int xinputValue = percentToXInput(seekBarLeftTrigger.getProgress());
            textLeftTriggerValue.setText(String.format("%d%% (%d)", seekBarLeftTrigger.getProgress(), xinputValue));
        }
        if (textRightTriggerValue != null && seekBarRightTrigger != null) {
            int xinputValue = percentToXInput(seekBarRightTrigger.getProgress());
            textRightTriggerValue.setText(String.format("%d%% (%d)", seekBarRightTrigger.getProgress(), xinputValue));
        }
    }

    /**
     * Converts percentage (0-100) to XInput value (0-65535).
     */
    private int percentToXInput(int percent) {
        return (int) ((percent / 100.0) * XINPUT_MAX_VALUE);
    }

    /**
     * Converts XInput value (0-65535) to short for rumble APIs.
     */
    private short xinputToShort(int xinputValue) {
        // XInput uses 0-65535, our APIs use signed short
        return (short) Math.min(xinputValue, 0x7FFF * 2);
    }

    private void setupVibrationButtons() {
        Button btnVibrateLow = findViewById(R.id.btn_vibrate_low);
        Button btnVibrateHigh = findViewById(R.id.btn_vibrate_high);
        Button btnVibrateBoth = findViewById(R.id.btn_vibrate_both);
        Button btnVibrateStop = findViewById(R.id.btn_vibrate_stop);
        Button btnVibrateTriggerLeft = findViewById(R.id.btn_vibrate_trigger_left);
        Button btnVibrateTriggerRight = findViewById(R.id.btn_vibrate_trigger_right);

        // Quick test buttons use 50% intensity
        btnVibrateLow.setOnClickListener(v -> testVibration(true, false, false, false));
        btnVibrateHigh.setOnClickListener(v -> testVibration(false, true, false, false));
        btnVibrateBoth.setOnClickListener(v -> testVibration(true, true, false, false));
        btnVibrateStop.setOnClickListener(v -> {
            stopContinuousVibration();
            stopAllVibration();
        });
        btnVibrateTriggerLeft.setOnClickListener(v -> testVibration(false, false, true, false));
        btnVibrateTriggerRight.setOnClickListener(v -> testVibration(false, false, false, true));

        // Custom intensity test button (uses slider values)
        Button btnVibrateCustom = findViewById(R.id.btn_vibrate_custom);
        if (btnVibrateCustom != null) {
            btnVibrateCustom.setOnClickListener(v -> startContinuousVibration());
        }
    }

    /**
     * Starts continuous vibration using the current slider values.
     * The vibration will update every 100ms to maintain the effect.
     */
    private void startContinuousVibration() {
        isVibrating = true;

        // Create runnable for continuous vibration
        vibrationRunnable = new Runnable() {
            @Override
            public void run() {
                if (isVibrating) {
                    applyCurrentSliderVibration();
                    // Re-apply vibration every 100ms to maintain continuous effect
                    vibrationHandler.postDelayed(this, 100);
                }
            }
        };

        // Start immediately
        vibrationHandler.post(vibrationRunnable);
    }

    /**
     * Stops continuous vibration.
     */
    private void stopContinuousVibration() {
        isVibrating = false;
        if (vibrationRunnable != null) {
            vibrationHandler.removeCallbacks(vibrationRunnable);
            vibrationRunnable = null;
        }
    }

    /**
     * Applies vibration based on current slider values.
     * This method directly uses XInput-compatible values.
     */
    private void applyCurrentSliderVibration() {
        int leftMotorXInput = seekBarLeftMotor != null ? percentToXInput(seekBarLeftMotor.getProgress()) : 0;
        int rightMotorXInput = seekBarRightMotor != null ? percentToXInput(seekBarRightMotor.getProgress()) : 0;
        int leftTriggerXInput = seekBarLeftTrigger != null ? percentToXInput(seekBarLeftTrigger.getProgress()) : 0;
        int rightTriggerXInput = seekBarRightTrigger != null ? percentToXInput(seekBarRightTrigger.getProgress()) : 0;

        // Convert to shorts for API calls
        short leftMotor = xinputToShort(leftMotorXInput);
        short rightMotor = xinputToShort(rightMotorXInput);
        short leftTrigger = xinputToShort(leftTriggerXInput);
        short rightTrigger = xinputToShort(rightTriggerXInput);

        boolean gamepadVibrated = false;

        for (VibratorInfo vibratorInfo : gamepadVibrators) {
            if (vibratorInfo.hasQuadVibrators && vibratorInfo.vibratorManager != null) {
                ControllerHandler.rumbleQuadVibrators(vibratorInfo.vibratorManager,
                        leftMotor, rightMotor, leftTrigger, rightTrigger);
                gamepadVibrated = true;
            } else if (vibratorInfo.hasDualVibrators && vibratorInfo.vibratorManager != null) {
                ControllerHandler.rumbleDualVibrators(vibratorInfo.vibratorManager,
                        leftMotor, rightMotor);
                gamepadVibrated = true;
            }
        }

        // Fallback to device vibrator for motor simulation
        if (!gamepadVibrated && (leftMotorXInput > 0 || rightMotorXInput > 0)) {
            Vibrator deviceVibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
            if (deviceVibrator != null && deviceVibrator.hasVibrator()) {
                // Use XInput values converted to 0-255 range for waveform
                int lowFreqAmplitude = (leftMotorXInput * 255) / XINPUT_MAX_VALUE;
                int highFreqAmplitude = (rightMotorXInput * 255) / XINPUT_MAX_VALUE;
                vibrateSingleMotorSimulation(deviceVibrator, lowFreqAmplitude, highFreqAmplitude);
            }
        }
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

                // Store vibrator info only if the device has real gamepad vibration support
                // (dual or quad vibrators through VibratorManager)
                // We don't use single vibrator fallback because it may reference the device vibrator
                VibratorInfo vibratorInfo = new VibratorInfo();
                vibratorInfo.deviceId = deviceId;
                vibratorInfo.vibratorManager = device.getVibratorManager();
                vibratorInfo.hasQuadVibrators = ControllerHandler.hasQuadAmplitudeControlledRumbleVibrators(device.getVibratorManager());
                vibratorInfo.hasDualVibrators = ControllerHandler.hasDualAmplitudeControlledRumbleVibrators(device.getVibratorManager());
                // Only add gamepads with actual gamepad vibration (dual or quad motors)
                if (vibratorInfo.hasQuadVibrators || vibratorInfo.hasDualVibrators) {
                    gamepadVibrators.add(vibratorInfo);
                }
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


        // Determine protocol (XInput vs HID)
        info.protocol = determineProtocol(device, info.vendorId, info.productId);

        // Check vibration support
        info.hasVibration = device.getVibrator().hasVibrator();
        info.hasAmplitudeControl = device.getVibrator().hasAmplitudeControl();
        info.hasDualMotorVibration = ControllerHandler.hasDualAmplitudeControlledRumbleVibrators(device.getVibratorManager());
        info.hasTriggerVibration = ControllerHandler.hasQuadAmplitudeControlledRumbleVibrators(device.getVibratorManager());

        // Check capabilities
        info.hasMotionSensors = device.getSensorManager().getDefaultSensor(android.hardware.Sensor.TYPE_ACCELEROMETER) != null ||
                device.getSensorManager().getDefaultSensor(android.hardware.Sensor.TYPE_GYROSCOPE) != null;

        info.hasTouchpad = device.getMotionRange(android.view.MotionEvent.AXIS_X, InputDevice.SOURCE_TOUCHPAD) != null;

        // Check for paddles and share button
        // Using Java implementation instead of native calls
        info.hasPaddles = ControllerDetection.guessControllerHasPaddles(info.vendorId, info.productId);
        info.hasShareButton = ControllerDetection.guessControllerHasShareButton(info.vendorId, info.productId);

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


        // USB devices using our driver are typically XInput-compatible
        info.protocol = getString(R.string.gamepad_test_protocol_usb_driver);

        // USB devices through our driver typically support vibration
        info.hasVibration = true;
        info.hasDualMotorVibration = true;

        return info;
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
        TextView protocolView = view.findViewById(R.id.gamepad_protocol);
        TextView vendorProductView = view.findViewById(R.id.gamepad_vendor_product);
        TextView capabilitiesView = view.findViewById(R.id.gamepad_capabilities);
        TextView vibrationView = view.findViewById(R.id.gamepad_vibration_support);

        nameView.setText(info.name);
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

    /**
     * Quick test vibration for button presses (uses 50% intensity).
     */
    private void testVibration(boolean lowFreq, boolean highFreq, boolean leftTrigger, boolean rightTrigger) {
        boolean gamepadVibrated = false;

        // 50% intensity = 32767 (half of 65535)
        short halfIntensity = (short) 32767;

        for (VibratorInfo vibratorInfo : gamepadVibrators) {
            if (vibratorInfo.hasQuadVibrators && vibratorInfo.vibratorManager != null) {
                ControllerHandler.rumbleQuadVibrators(vibratorInfo.vibratorManager,
                        lowFreq ? halfIntensity : 0,
                        highFreq ? halfIntensity : 0,
                        leftTrigger ? halfIntensity : 0,
                        rightTrigger ? halfIntensity : 0);
                gamepadVibrated = true;
            } else if (vibratorInfo.hasDualVibrators && vibratorInfo.vibratorManager != null) {
                ControllerHandler.rumbleDualVibrators(vibratorInfo.vibratorManager,
                        lowFreq ? halfIntensity : 0,
                        highFreq ? halfIntensity : 0);
                gamepadVibrated = true;
            }
        }

        // If no gamepad vibrated, try device vibrator as fallback
        // Don't simulate trigger vibration on single-motor devices
        if (!gamepadVibrated && !leftTrigger && !rightTrigger) {
            Vibrator deviceVibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
            if (deviceVibrator != null && deviceVibrator.hasVibrator() && (lowFreq || highFreq)) {
                // Use 128 (50% of 255) for half intensity
                int lowFreqAmplitude = lowFreq ? 128 : 0;
                int highFreqAmplitude = highFreq ? 128 : 0;
                vibrateSingleMotorSimulation(deviceVibrator, lowFreqAmplitude, highFreqAmplitude);
            }
        }
    }

    /**
     * Simulates dual-motor vibration on a single-motor device using waveforms.
     * Uses XInput-accurate frequencies for precise motor simulation.
     *
     * XInput specifications:
     * - Left motor (lowFreq): ~20-30Hz, heavy eccentric mass
     * - Right motor (highFreq): ~100-150Hz, light eccentric mass
     *
     * @param vibrator The device vibrator to use
     * @param lowFreqAmplitude Low frequency motor amplitude (0-255)
     * @param highFreqAmplitude High frequency motor amplitude (0-255)
     */
    private void vibrateSingleMotorSimulation(Vibrator vibrator, int lowFreqAmplitude, int highFreqAmplitude) {
        if (lowFreqAmplitude == 0 && highFreqAmplitude == 0) {
            vibrator.cancel();
            return;
        }

        // Require amplitude control for proper simulation
        if (!vibrator.hasAmplitudeControl()) {
            // Fallback for devices without amplitude control
            if (lowFreqAmplitude > 30 || highFreqAmplitude > 30) {
                VibrationEffect effect = VibrationEffect.createOneShot(200, VibrationEffect.DEFAULT_AMPLITUDE);
                vibrator.vibrate(effect);
            }
            return;
        }

        VibrationEffect effect = ControllerHandler.createDualMotorWaveformEffect(lowFreqAmplitude, highFreqAmplitude);
        if (effect != null) {
            vibrator.vibrate(effect);
        }
    }

    /**
     * Quick test vibration for button presses (uses 50% intensity).
     */

    private void stopAllVibration() {
        for (VibratorInfo vibratorInfo : gamepadVibrators) {
            if (vibratorInfo.vibratorManager != null) {
                vibratorInfo.vibratorManager.cancel();
            }
        }

        // Also cancel device vibrator
        Vibrator deviceVibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
        if (deviceVibrator != null) {
            deviceVibrator.cancel();
        }
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
        boolean hasQuadVibrators;
        boolean hasDualVibrators;
    }
}



