package com.limelight.utils;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;

import com.limelight.Game;
import com.limelight.R;

public class StatsNotificationHelper {
    private static final String CHANNEL_ID = "streaming_stats_channel";
    private static final int NOTIFICATION_ID = 1001;

    private final Context context;
    private final NotificationManager notificationManager;
    private boolean isShowing = false;

    public StatsNotificationHelper(Context context) {
        this.context = context;
        this.notificationManager = (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        createNotificationChannel();
    }

    private void createNotificationChannel() {
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.stats_notification_channel_name),
                NotificationManager.IMPORTANCE_LOW
        );
        channel.setDescription(context.getString(R.string.stats_notification_channel_description));
        channel.setShowBadge(false);
        channel.enableVibration(false);
        channel.setSound(null, null);
        notificationManager.createNotificationChannel(channel);
    }

    private boolean hasNotificationPermission() {
        return context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED;
    }

    public void showNotification(String statsText) {
        // Check for notification permission before attempting to show
        if (!hasNotificationPermission()) {
            return;
        }

        Intent intent = new Intent(context, Game.class);
        intent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pendingIntent = PendingIntent.getActivity(
                context, 0, intent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        Notification.Builder builder = new Notification.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.app_icon)
                .setContentTitle(context.getString(R.string.stats_notification_title))
                .setContentText(statsText)
                .setStyle(new Notification.BigTextStyle().bigText(statsText))
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(pendingIntent)
                .setCategory(Notification.CATEGORY_STATUS);

        notificationManager.notify(NOTIFICATION_ID, builder.build());
        isShowing = true;
    }

    public void updateNotification(String statsText) {
        if (isShowing) {
            showNotification(statsText);
        }
    }

    public void cancelNotification() {
        notificationManager.cancel(NOTIFICATION_ID);
        isShowing = false;
    }

    public boolean isShowing() {
        return isShowing;
    }
}
