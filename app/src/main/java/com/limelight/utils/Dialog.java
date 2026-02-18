package com.limelight.utils;

import java.util.ArrayList;

import android.app.Activity;
import android.app.AlertDialog;
import android.text.InputType;
import android.text.method.PasswordTransformationMethod;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.Toast;

import androidx.annotation.NonNull;

import com.limelight.R;

public class Dialog implements Runnable {
    private final String title;
    private final String message;
    private final Activity activity;
    private final Runnable runOnDismiss;

    private AlertDialog alert;

    private static final ArrayList<Dialog> rundownDialogs = new ArrayList<>();

    private Dialog(Activity activity, String title, String message, Runnable runOnDismiss)
    {
        this.activity = activity;
        this.title = title;
        this.message = message;
        this.runOnDismiss = runOnDismiss;
    }

    public static void closeDialogs()
    {
        // Also dismiss progress dialog
        dismissProgressDialog();

        synchronized (rundownDialogs) {
            for (Dialog d : rundownDialogs) {
                try {
                    if (d.alert != null && d.alert.isShowing()) {
                        d.alert.dismiss();
                    }
                } catch (Exception ignored) {
                    // Dialog may have been destroyed with the activity
                }
            }

            rundownDialogs.clear();
        }
    }

    public static void displayDialog(final Activity activity, String title, String message, final boolean endAfterDismiss)
    {
        activity.runOnUiThread(new Dialog(activity, title, message, () -> {
            if (endAfterDismiss) {
                activity.finish();
            }
        }));
    }

    public static void displayDialog(Activity activity, String title, String message, Runnable runOnDismiss)
    {
        activity.runOnUiThread(new Dialog(activity, title, message, runOnDismiss));
    }


    public interface AddComputerCallback {
        void onAddComputer(String host);
    }

    public static void displayAddComputerDialog(final Activity activity, final AddComputerCallback callback) {
        activity.runOnUiThread(() -> {
            if (activity.isFinishing())
                return;

            final EditText input = new EditText(activity);
            input.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
            input.setHint(R.string.ip_hint);
            input.setSingleLine(true);
            input.setImeOptions(EditorInfo.IME_ACTION_DONE);

            // Add padding to the EditText
            int padding = (int) (16 * activity.getResources().getDisplayMetrics().density);
            input.setPadding(padding, padding, padding, padding);

            AlertDialog alert = new AlertDialog.Builder(activity)
                    .setTitle(R.string.title_add_pc)
                    .setView(input)
                    .setCancelable(true)
                    .setPositiveButton(android.R.string.ok, null) // Set to null, we'll override below
                    .setNegativeButton(android.R.string.cancel, (dialog, which) -> dialog.dismiss())
                    .create();

            alert.setOnShowListener(dialog -> {
                Button okButton = alert.getButton(AlertDialog.BUTTON_POSITIVE);
                okButton.setOnClickListener(v -> {
                    String hostAddress = input.getText().toString().trim();
                    if (hostAddress.isEmpty()) {
                        Toast.makeText(activity, R.string.addpc_enter_ip, Toast.LENGTH_SHORT).show();
                        return;
                    }
                    alert.dismiss();
                    if (callback != null) {
                        callback.onAddComputer(hostAddress);
                    }
                });

                // Handle IME action
                input.setOnEditorActionListener((v, actionId, event) -> {
                    if (actionId == EditorInfo.IME_ACTION_DONE) {
                        okButton.performClick();
                        return true;
                    }
                    return false;
                });

                // Focus on the input and show keyboard
                input.requestFocus();
            });

            synchronized (rundownDialogs) {
                Dialog wrapper = new Dialog(activity, activity.getString(R.string.title_add_pc), "", null);
                wrapper.alert = alert;
                rundownDialogs.add(wrapper);
                alert.show();
            }
        });
    }

    /**
     * Callback interface for Sunshine pairing credentials
     */
    public interface SunshinePairingCallback {
        void onCredentialsEntered(String username, String password);
        void onCancelled();
    }

    /**
     * Display a dialog to enter Sunshine server credentials for automatic pairing
     */
    public static void displaySunshinePairingDialog(final Activity activity, String computerName, final SunshinePairingCallback callback) {
        activity.runOnUiThread(() -> {
            if (activity.isFinishing())
                return;

            // Create a custom layout with username and password fields
            android.widget.LinearLayout layout = new android.widget.LinearLayout(activity);
            layout.setOrientation(android.widget.LinearLayout.VERTICAL);
            int padding = (int) (16 * activity.getResources().getDisplayMetrics().density);
            layout.setPadding(padding, padding, padding, padding);

            // Username field
            final EditText usernameInput = new EditText(activity);
            usernameInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
            usernameInput.setHint(R.string.sunshine_username_hint);
            usernameInput.setSingleLine(true);
            layout.addView(usernameInput);

            // Add spacing
            android.widget.Space space = new android.widget.Space(activity);
            space.setMinimumHeight((int) (8 * activity.getResources().getDisplayMetrics().density));
            layout.addView(space);

            // Password field
            final EditText passwordInput = new EditText(activity);
            passwordInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
            passwordInput.setTransformationMethod(PasswordTransformationMethod.getInstance());
            passwordInput.setHint(R.string.sunshine_password_hint);
            passwordInput.setMaxLines(1);
            passwordInput.setImeOptions(EditorInfo.IME_ACTION_DONE);
            layout.addView(passwordInput);

            AlertDialog alert = new AlertDialog.Builder(activity)
                    .setTitle(activity.getString(R.string.sunshine_pairing_title, computerName))
                    .setMessage(R.string.sunshine_pairing_message)
                    .setView(layout)
                    .setCancelable(true)
                    .setPositiveButton(android.R.string.ok, null)
                    .setNegativeButton(android.R.string.cancel, (dialog, which) -> {
                        dialog.dismiss();
                        if (callback != null) {
                            callback.onCancelled();
                        }
                    })
                    .create();

            alert.setOnShowListener(dialog -> {
                Button okButton = alert.getButton(AlertDialog.BUTTON_POSITIVE);
                okButton.setOnClickListener(v -> {
                    String username = usernameInput.getText().toString().trim();
                    String password = passwordInput.getText().toString();

                    if (username.isEmpty()) {
                        Toast.makeText(activity, R.string.sunshine_enter_username, Toast.LENGTH_SHORT).show();
                        return;
                    }
                    if (password.isEmpty()) {
                        Toast.makeText(activity, R.string.sunshine_enter_password, Toast.LENGTH_SHORT).show();
                        return;
                    }

                    alert.dismiss();
                    if (callback != null) {
                        callback.onCredentialsEntered(username, password);
                    }
                });

                // Handle IME action on password field
                passwordInput.setOnEditorActionListener((v, actionId, event) -> {
                    if (actionId == EditorInfo.IME_ACTION_DONE) {
                        okButton.performClick();
                        return true;
                    }
                    return false;
                });

                // Focus on username input
                usernameInput.requestFocus();
            });

            synchronized (rundownDialogs) {
                Dialog wrapper = new Dialog(activity, "", "", null);
                wrapper.alert = alert;
                rundownDialogs.add(wrapper);
                alert.show();
            }
        });
    }

    @Override
    public void run() {
        // If we're dying, don't bother creating a dialog
        if (activity.isFinishing())
            return;

        alert = new AlertDialog.Builder(activity).create();

        alert.setTitle(title);
        alert.setMessage(message);
        alert.setCancelable(false);
        alert.setCanceledOnTouchOutside(false);
 
        alert.setButton(AlertDialog.BUTTON_POSITIVE, activity.getResources().getText(android.R.string.ok), (dialog, which) -> {
            synchronized (rundownDialogs) {
                rundownDialogs.remove(Dialog.this);
                alert.dismiss();
            }

            runOnDismiss.run();
        });
        alert.setOnShowListener(dialog -> {
            // Set focus to the OK button by default
            Button button = alert.getButton(AlertDialog.BUTTON_POSITIVE);
            button.setFocusable(true);
            button.setFocusableInTouchMode(true);
            button.requestFocus();
        });

        synchronized (rundownDialogs) {
            rundownDialogs.add(this);
            alert.show();
        }
    }

    // Progress dialog for pairing
    private static AlertDialog progressDialog;

    /**
     * Display a progress dialog with a spinner
     */
    public static void displayProgressDialog(final Activity activity, String title, String message, final Runnable onCancel) {
        activity.runOnUiThread(() -> {
            if (activity.isFinishing())
                return;

            // Dismiss any existing progress dialog
            dismissProgressDialog();

            // Create layout with progress bar
            LinearLayout layout = getLinearLayout(activity, message);

            AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                    .setTitle(title)
                    .setView(layout)
                    .setCancelable(false);

            if (onCancel != null) {
                builder.setNegativeButton(android.R.string.cancel, (dialog, which) -> {
                    dialog.dismiss();
                    onCancel.run();
                });
            }

            progressDialog = builder.create();
            progressDialog.show();
        });
    }

    @NonNull
    private static LinearLayout getLinearLayout(Activity activity, String message) {
        LinearLayout layout = new LinearLayout(activity);
        layout.setOrientation(LinearLayout.HORIZONTAL);
        layout.setGravity(android.view.Gravity.CENTER_VERTICAL);
        int padding = (int) (24 * activity.getResources().getDisplayMetrics().density);
        layout.setPadding(padding, padding, padding, padding);

        // Add progress bar
        android.widget.ProgressBar progressBar = new android.widget.ProgressBar(activity);
        // Set the progress bar color to red (colorAccent)
        progressBar.setIndeterminateTintList(android.content.res.ColorStateList.valueOf(0xFFFF5252));
        layout.addView(progressBar);

        // Add spacing
        android.widget.Space space = new android.widget.Space(activity);
        space.setMinimumWidth((int) (16 * activity.getResources().getDisplayMetrics().density));
        layout.addView(space);

        // Add message text
        android.widget.TextView messageView = new android.widget.TextView(activity);
        messageView.setText(message);
        messageView.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 16);
        layout.addView(messageView);
        return layout;
    }

    /**
     * Dismiss the progress dialog
     */
    public static void dismissProgressDialog() {
        AlertDialog dialog = progressDialog;
        progressDialog = null;

        if (dialog != null) {
            try {
                if (dialog.isShowing()) {
                    dialog.dismiss();
                }
            } catch (Exception ignored) {
                // Dialog may have been destroyed with the activity
            }
        }
    }

}
