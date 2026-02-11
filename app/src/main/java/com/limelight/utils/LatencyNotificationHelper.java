package com.limelight.utils;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.Context;
import android.content.pm.PackageManager;

import com.limelight.R;
import com.limelight.nvstream.jni.MoonBridge;

/**
 * Helper class for displaying latency statistics notification after stream ends.
 */
public class LatencyNotificationHelper {
    private static final String CHANNEL_ID = "latency_stats_channel";
    private static final int NOTIFICATION_ID = 1002;

    private final Context context;
    private final NotificationManager notificationManager;

    public LatencyNotificationHelper(Context context) {
        this.context = context;
        this.notificationManager = (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        createNotificationChannel();
    }

    private void createNotificationChannel() {
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.latency_notification_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT
        );
        channel.setDescription(context.getString(R.string.latency_notification_channel_description));
        channel.setShowBadge(true);
        channel.enableVibration(false);
        channel.setSound(null, null);
        notificationManager.createNotificationChannel(channel);
    }

    private boolean hasNotificationPermission() {
        return context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED;
    }

    /**
     * Show latency statistics notification after stream ends.
     *
     * @param averageEndToEndLat Average end-to-end latency in milliseconds
     * @param averageDecoderLat Average decoder latency in milliseconds
     * @param videoFormat Video format from MoonBridge
     */
    public void showLatencyNotification(int averageEndToEndLat, int averageDecoderLat, int videoFormat) {
        // Check for notification permission before attempting to show
        if (!hasNotificationPermission()) {
            return;
        }

        String message = buildLatencyMessage(averageEndToEndLat, averageDecoderLat, videoFormat);

        if (message == null) {
            return; // No data to show
        }

        String title = context.getString(R.string.latency_notification_title);

        Notification.Builder builder = new Notification.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle(title)
                .setContentText(message)
                .setStyle(new Notification.BigTextStyle().bigText(message))
                .setAutoCancel(true)
                .setOnlyAlertOnce(true)
                .setCategory(Notification.CATEGORY_STATUS);

        notificationManager.notify(NOTIFICATION_ID, builder.build());
    }

    /**
     * Build the latency message string.
     */
    private String buildLatencyMessage(int averageEndToEndLat, int averageDecoderLat, int videoFormat) {
        String message = null;

        if (averageEndToEndLat > 0) {
            message = context.getString(R.string.conn_client_latency) + " " + averageEndToEndLat + " ms";
            if (averageDecoderLat > 0) {
                message += " (" + context.getString(R.string.conn_client_latency_hw) + " " + averageDecoderLat + " ms)";
            }
        } else if (averageDecoderLat > 0) {
            message = context.getString(R.string.conn_hardware_latency) + " " + averageDecoderLat + " ms";
        }

        // Add the video codec to the message
        if (message != null) {
            message += " [";

            if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_H264) != 0) {
                message += "H.264";
            } else if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_H265) != 0) {
                message += "HEVC";
            } else if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_AV1) != 0) {
                message += "AV1";
            } else {
                message += "UNKNOWN";
            }

            if ((videoFormat & MoonBridge.VIDEO_FORMAT_MASK_10BIT) != 0) {
                message += " HDR";
            }

            message += "]";
        }

        return message;
    }

    /**
     * Cancel the latency notification if it's showing.
     */
    public void cancelNotification() {
        notificationManager.cancel(NOTIFICATION_ID);
    }
}

