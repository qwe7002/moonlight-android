package com.limelight.utils;

import java.util.ArrayList;
import java.util.Iterator;

import android.app.Activity;
import android.content.DialogInterface;
import android.content.DialogInterface.OnCancelListener;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.Space;
import android.widget.TextView;

import androidx.appcompat.app.AlertDialog;

public class SpinnerDialog implements Runnable,OnCancelListener {
    private final String title;
    private final String message;
    private final Activity activity;
    private AlertDialog progress;
    private final boolean finish;

    private static final ArrayList<SpinnerDialog> rundownDialogs = new ArrayList<>();

    private SpinnerDialog(Activity activity, String title, String message, boolean finish)
    {
        this.activity = activity;
        this.title = title;
        this.message = message;
        this.progress = null;
        this.finish = finish;
    }

    public static SpinnerDialog displayDialog(Activity activity, String title, String message, boolean finish)
    {
        SpinnerDialog spinner = new SpinnerDialog(activity, title, message, finish);
        activity.runOnUiThread(spinner);
        return spinner;
    }

    public static void closeDialogs(Activity activity)
    {
        synchronized (rundownDialogs) {
            Iterator<SpinnerDialog> i = rundownDialogs.iterator();
            while (i.hasNext()) {
                SpinnerDialog dialog = i.next();
                if (dialog.activity == activity) {
                    i.remove();
                    try {
                        if (dialog.progress != null && dialog.progress.isShowing()) {
                            dialog.progress.dismiss();
                        }
                    } catch (Exception ignored) {
                        // Dialog may have been destroyed with the activity
                    }
                }
            }
        }
    }

    public void dismiss()
    {
        // Running again with progress != null will destroy it
        activity.runOnUiThread(this);
    }

    private TextView messageView;

    public void setMessage(final String message)
    {
        activity.runOnUiThread(() -> {
            if (messageView != null) {
                messageView.setText(message);
            }
        });
    }

    @Override
    public void run() {

        // If we're dying, don't bother doing anything
        if (activity.isFinishing()) {
            return;
        }

        if (progress == null)
        {
            // Create layout with progress bar
            LinearLayout layout = new LinearLayout(activity);
            layout.setOrientation(LinearLayout.HORIZONTAL);
            layout.setGravity(android.view.Gravity.CENTER_VERTICAL);
            int padding = (int) (24 * activity.getResources().getDisplayMetrics().density);
            layout.setPadding(padding, padding, padding, padding);

            // Add progress bar
            ProgressBar progressBar = new ProgressBar(activity);
            // Set the progress bar color to red (colorAccent) to match app theme
            progressBar.setIndeterminateTintList(android.content.res.ColorStateList.valueOf(0xFFFF5252));
            layout.addView(progressBar);

            // Add spacing
            Space space = new Space(activity);
            space.setMinimumWidth((int) (16 * activity.getResources().getDisplayMetrics().density));
            layout.addView(space);

            // Add message text
            messageView = new TextView(activity);
            messageView.setText(message);
            messageView.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 16);
            layout.addView(messageView);

            // Create the dialog
            AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                    .setTitle(title)
                    .setView(layout)
                    .setOnCancelListener(this);

            // If we want to finish the activity when this is killed, make it cancellable
            if (finish)
            {
                builder.setCancelable(true);
                progress = builder.create();
                progress.setCanceledOnTouchOutside(false);
            }
            else
            {
                builder.setCancelable(false);
                progress = builder.create();
            }

            synchronized (rundownDialogs) {
                rundownDialogs.add(this);
                progress.show();
            }
        }
        else
        {
            synchronized (rundownDialogs) {
                rundownDialogs.remove(this);
                try {
                    if (progress != null && progress.isShowing()) {
                        progress.dismiss();
                    }
                } catch (Exception ignored) {
                    // Dialog may have been destroyed with the activity
                }
            }
        }
    }

    @Override
    public void onCancel(DialogInterface dialog) {
        synchronized (rundownDialogs) {
            rundownDialogs.remove(this);
        }

        // This will only be called if finish was true, so we don't need to check again
        activity.finish();
    }
}
