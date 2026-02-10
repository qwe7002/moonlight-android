package com.limelight.utils;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.GameManager;
import android.app.GameState;
import android.app.LocaleManager;
import android.content.Context;
import android.content.DialogInterface;
import android.content.SharedPreferences;
import android.graphics.Insets;
import android.os.LocaleList;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

import com.limelight.R;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.preferences.PreferenceConfiguration;

public class UiHelper {

    private static void setGameModeStatus(Context context, boolean streaming) {
        GameManager gameManager = context.getSystemService(GameManager.class);
        if (gameManager == null) {
            return;
        }

        if (streaming) {
            gameManager.setGameState(new GameState(false, GameState.MODE_GAMEPLAY_UNINTERRUPTIBLE));
        } else {
            gameManager.setGameState(new GameState(false, GameState.MODE_NONE));
        }
    }

    public static void notifyStreamConnecting(Context context) {
        setGameModeStatus(context, true);
    }

    public static void notifyStreamConnected(Context context) {
        setGameModeStatus(context, true);
    }

    public static void notifyStreamEnteringPiP(Context context) {
        setGameModeStatus(context, true);
    }

    public static void notifyStreamExitingPiP(Context context) {
        setGameModeStatus(context, true);
    }

    public static void notifyStreamEnded(Context context) {
        setGameModeStatus(context, false);
    }

    public static void setLocale(Activity activity) {
        String locale = PreferenceConfiguration.readPreferences(activity).language;
        if (!locale.equals(PreferenceConfiguration.DEFAULT_LANGUAGE)) {
            // On Android 13, migrate this non-default language setting into the OS native API
            LocaleManager localeManager = activity.getSystemService(LocaleManager.class);
            localeManager.setApplicationLocales(LocaleList.forLanguageTags(locale));
            PreferenceConfiguration.completeLanguagePreferenceMigration(activity);
        }
    }

    public static void applyStatusBarPadding(View view) {
        // This applies the padding for system bars and display cutout insets to this specific view.
        // We handle both system bars and display cutout to properly support landscape mode with notches.
        // We consume the insets to prevent double-padding when used with notifyNewRootView().
        view.setOnApplyWindowInsetsListener((v, windowInsets) -> {
            Insets systemBarsInsets = windowInsets.getInsets(WindowInsets.Type.systemBars());
            Insets displayCutoutInsets = windowInsets.getInsets(WindowInsets.Type.displayCutout());

            // Use the maximum of system bars and display cutout insets for each edge
            // This ensures proper padding in both portrait and landscape modes with notches
            int left = Math.max(systemBarsInsets.left, displayCutoutInsets.left);
            int top = Math.max(systemBarsInsets.top, displayCutoutInsets.top);
            int right = Math.max(systemBarsInsets.right, displayCutoutInsets.right);
            int bottom = Math.max(systemBarsInsets.bottom, displayCutoutInsets.bottom);

            v.setPadding(left, top, right, bottom);
            // Consume the insets so they don't propagate further
            return WindowInsets.CONSUMED;
        });

        // Request insets immediately if the view is already attached,
        // otherwise wait for the view to be attached
        if (view.isAttachedToWindow()) {
            view.requestApplyInsets();
        } else {
            view.addOnAttachStateChangeListener(new View.OnAttachStateChangeListener() {
                @Override
                public void onViewAttachedToWindow(View v) {
                    v.requestApplyInsets();
                    v.removeOnAttachStateChangeListener(this);
                }

                @Override
                public void onViewDetachedFromWindow(View v) {
                }
            });
        }
    }

    public static void notifyNewRootView(final Activity activity) {
        // Set GameState.MODE_NONE initially for all activities
        setGameModeStatus(activity, false);

        // Allow this non-streaming activity to layout under notches.
        //
        // We should NOT do this for the Game activity unless
        // the user specifically opts in, because it can obscure
        // parts of the streaming surface.
        activity.getWindow().getAttributes().layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;


        // Use the WindowInsetsController API
        WindowInsetsController controller = activity.getWindow().getInsetsController();
        if (controller != null) {
            controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);

        }
    }

    public static void showDecoderCrashDialog(Activity activity) {
        final SharedPreferences prefs = activity.getSharedPreferences("DecoderTombstone", 0);
        final int crashCount = prefs.getInt("CrashCount", 0);
        int lastNotifiedCrashCount = prefs.getInt("LastNotifiedCrashCount", 0);

        // Remember the last crash count we notified at, so we don't
        // display the crash dialog every time the app is started until
        // they stream again
        if (crashCount != 0 && crashCount != lastNotifiedCrashCount) {
            if (crashCount % 3 == 0) {
                // At 3 consecutive crashes, we'll forcefully reset their settings
                PreferenceConfiguration.resetStreamingSettings(activity);
                Dialog.displayDialog(activity,
                        activity.getResources().getString(R.string.title_decoding_reset),
                        activity.getResources().getString(R.string.message_decoding_reset),
                        () -> {
                            // Mark notification as acknowledged on dismissal
                            prefs.edit().putInt("LastNotifiedCrashCount", crashCount).apply();
                        });
            } else {
                Dialog.displayDialog(activity,
                        activity.getResources().getString(R.string.title_decoding_error),
                        activity.getResources().getString(R.string.message_decoding_error),
                        () -> {
                            // Mark notification as acknowledged on dismissal
                            prefs.edit().putInt("LastNotifiedCrashCount", crashCount).apply();
                        });
            }
        }
    }

    public static void displayQuitConfirmationDialog(Activity parent, final Runnable onYes, final Runnable onNo) {
        DialogInterface.OnClickListener dialogClickListener = new DialogInterface.OnClickListener() {
            @Override
            public void onClick(DialogInterface dialog, int which) {
                switch (which) {
                    case DialogInterface.BUTTON_POSITIVE:
                        if (onYes != null) {
                            onYes.run();
                        }
                        break;

                    case DialogInterface.BUTTON_NEGATIVE:
                        if (onNo != null) {
                            onNo.run();
                        }
                        break;
                }
            }
        };

        AlertDialog.Builder builder = new AlertDialog.Builder(parent);
        builder.setMessage(parent.getResources().getString(R.string.applist_quit_confirmation))
                .setPositiveButton(parent.getResources().getString(R.string.yes), dialogClickListener)
                .setNegativeButton(parent.getResources().getString(R.string.no), dialogClickListener)
                .show();
    }

    public static void displayDeletePcConfirmationDialog(Activity parent, ComputerDetails computer, final Runnable onYes, final Runnable onNo) {
        DialogInterface.OnClickListener dialogClickListener = new DialogInterface.OnClickListener() {
            @Override
            public void onClick(DialogInterface dialog, int which) {
                switch (which) {
                    case DialogInterface.BUTTON_POSITIVE:
                        if (onYes != null) {
                            onYes.run();
                        }
                        break;

                    case DialogInterface.BUTTON_NEGATIVE:
                        if (onNo != null) {
                            onNo.run();
                        }
                        break;
                }
            }
        };

        AlertDialog.Builder builder = new AlertDialog.Builder(parent);
        builder.setMessage(parent.getResources().getString(R.string.delete_pc_msg))
                .setTitle(computer.name)
                .setPositiveButton(parent.getResources().getString(R.string.yes), dialogClickListener)
                .setNegativeButton(parent.getResources().getString(R.string.no), dialogClickListener)
                .show();
    }
}
