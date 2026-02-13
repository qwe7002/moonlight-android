package com.limelight.binding.input.capture;

import android.app.Activity;

import com.limelight.R;

public class InputCaptureManager {
    public static InputCaptureProvider getInputCaptureProvider(Activity activity) {
        return new AndroidNativePointerCaptureProvider(activity, activity.findViewById(R.id.surfaceView));
    }
}
