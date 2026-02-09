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
import com.limelight.LimeLog;
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

    private String simplifyStatsText(String statsText) {
        // Parse the stats text and extract key metrics only
        // Expected multi-line format:
        // Video stream: 1920x1080 60.00 FPS
        // Decoder: OMX.xxx.decoder
        // Incoming frame rate from network: 60.00 FPS
        // Rendering frame rate: 60.00 FPS
        // Frames dropped by your network connection: 0.00%
        // Average network latency: 2 ms (variance: 0 ms)
        // Average decoding time: 1.50 ms
        try {
            StringBuilder simplified = new StringBuilder();
            String[] lines = statsText.split("\n");

            for (String line : lines) {
                // Extract resolution and FPS from Video stream line
                if (line.contains("Video stream:") && line.contains("FPS")) {
                    // Extract resolution (e.g., 1920x1080)
                    int colonIndex = line.indexOf(":");
                    if (colonIndex >= 0) {
                        String afterColon = line.substring(colonIndex + 1).trim();
                        int spaceIndex = afterColon.indexOf(' ');
                        if (spaceIndex > 0) {
                            String resolution = afterColon.substring(0, spaceIndex).trim();
                            simplified.append(resolution);
                        }
                    }
                    // Extract FPS
                    int fpsEnd = line.lastIndexOf("FPS");
                    int fpsStart = line.lastIndexOf(' ', fpsEnd - 2);
                    if (fpsStart >= 0 && fpsEnd > fpsStart) {
                        String fps = line.substring(fpsStart + 1, fpsEnd).trim();
                        if (simplified.length() > 0) {
                            simplified.append(" ");
                        }
                        simplified.append(fps).append(" FPS");
                    }
                }
                // Extract network latency
                else if (line.contains("latency:")) {
                    int latencyStart = line.indexOf(":") + 1;
                    int latencyEnd = line.indexOf("ms");
                    if (latencyStart > 0 && latencyEnd > latencyStart) {
                        String latency = line.substring(latencyStart, latencyEnd).trim();
                        if (simplified.length() > 0) {
                            simplified.append(" | ");
                        }
                        simplified.append("RTT ").append(latency).append(" ms");
                    }
                }
                // Extract decoding time
                else if (line.contains("decoding time:")) {
                    int decStart = line.indexOf(":") + 1;
                    int decEnd = line.indexOf("ms");
                    if (decStart > 0 && decEnd > decStart) {
                        String decTime = line.substring(decStart, decEnd).trim();
                        if (simplified.length() > 0) {
                            simplified.append(" | ");
                        }
                        simplified.append("Dec ").append(decTime).append(" ms");
                    }
                }
            }

            String result = simplified.toString();
            return result.isEmpty() ? statsText : result;
        } catch (Exception e) {
            LimeLog.warning("Failed to simplify stats: " + e.getMessage());
            return statsText;
        }
    }

    public void showNotification(String statsText, String videoCodec) {
        // Check for notification permission before attempting to show
        if (!hasNotificationPermission()) {
            return;
        }

        // Simplify the stats text
        String simplifiedStats = simplifyStatsText(statsText);

        // Prepend decoder info to content if available
        if (videoCodec != null && !videoCodec.isEmpty() && !videoCodec.equals("Unknown")) {
            simplifiedStats = videoCodec + " | " + simplifiedStats;
        }

        String title = context.getString(R.string.stats_notification_title);

        Intent intent = new Intent(context, Game.class);
        intent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pendingIntent = PendingIntent.getActivity(
                context, 0, intent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        Notification.Builder builder = new Notification.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.app_icon)
                .setContentTitle(title)
                .setContentText(simplifiedStats)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(pendingIntent)
                .setCategory(Notification.CATEGORY_STATUS);

        notificationManager.notify(NOTIFICATION_ID, builder.build());
        isShowing = true;
    }

    // Overload for backward compatibility
    public void showNotification(String statsText) {
        showNotification(statsText, "");
    }

    public void updateNotification(String statsText, String videoCodec) {
        if (isShowing) {
            showNotification(statsText, videoCodec);
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

