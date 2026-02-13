package com.limelight.utils;

import android.os.Build;
import android.util.Log;


import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Helper class for detecting Vulkan GPU information.
 * This replaces the OpenGL ES based GPU detection.
 */
public class VulkanHelper {

    private static final String TAG = "VulkanHelper";

    /**
     * Get the GPU renderer name using Vulkan.
     * This reads from system properties as a fallback and uses dumpsys if available.
     *
     * @return GPU renderer name string, or empty string if not available
     */
    public static String getGpuRenderer() {
        // First, try to get GPU info from system properties
        String gpuRenderer = getGpuFromSystemProperties();
        if (gpuRenderer != null && !gpuRenderer.isEmpty()) {
            Log.i(TAG, "Got GPU from system properties: " + gpuRenderer);
            return gpuRenderer;
        }

        // Fallback: Try to get from Build properties
        gpuRenderer = getGpuFromBuildProperties();
        if (gpuRenderer != null && !gpuRenderer.isEmpty()) {
            Log.i(TAG, "Got GPU from Build properties: " + gpuRenderer);
            return gpuRenderer;
        }

        // Try to get Vulkan device info via dumpsys (requires shell access on some devices)
        gpuRenderer = getGpuFromDumpsys();
        if (gpuRenderer != null && !gpuRenderer.isEmpty()) {
            Log.i(TAG, "Got GPU from dumpsys: " + gpuRenderer);
            return gpuRenderer;
        }

        Log.e(TAG, "Could not detect GPU renderer via Vulkan methods");
        return "";
    }

    /**
     * Get GPU info from system properties.
     */
    private static String getGpuFromSystemProperties() {
        try {
            // Try common GPU-related system properties
            String[] props = {
                    "ro.hardware.vulkan",
                    "ro.hardware.egl",
                    "ro.board.platform",
                    "ro.hardware"
            };

            for (String prop : props) {
                String value = getSystemProperty(prop);
                if (value != null && !value.isEmpty()) {
                    // Map known platform names to GPU names
                    String gpuName = mapPlatformToGpu(value);
                    if (gpuName != null) {
                        return gpuName;
                    }
                }
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to get GPU from system properties: " + e.getMessage(), e);
        }
        return null;
    }

    /**
     * Get system property value using reflection.
     */
    private static String getSystemProperty(String key) {
        try {
            Class<?> systemPropertiesClass = Class.forName("android.os.SystemProperties");
            java.lang.reflect.Method getMethod = systemPropertiesClass.getMethod("get", String.class);
            return (String) getMethod.invoke(null, key);
        } catch (Exception e) {
            return null;
        }
    }

    /**
     * Map platform/hardware names to GPU names.
     */
    private static String mapPlatformToGpu(String platform) {
        if (platform == null) return null;

        platform = platform.toLowerCase();

        // Qualcomm Snapdragon (Adreno GPU)
        if (platform.contains("qcom") || platform.contains("msm") || platform.contains("sdm") ||
                platform.contains("sm") || platform.contains("trinket") || platform.contains("bengal") ||
                platform.contains("holi") || platform.contains("taro") || platform.contains("kalama") ||
                platform.contains("pineapple") || platform.contains("crow") || platform.contains("sun")) {
            return getAdrenoGpuName(platform);
        }

        // Samsung Exynos (Mali or Xclipse GPU)
        if (platform.contains("exynos") || platform.contains("universal") || platform.contains("s5e")) {
            return getExynosGpuName(platform);
        }

        // MediaTek (Mali or PowerVR GPU)
        if (platform.contains("mt") && platform.matches(".*mt\\d{4}.*")) {
            return getMediatekGpuName(platform);
        }

        // HiSilicon Kirin (Mali GPU)
        if (platform.contains("kirin") || platform.contains("hi")) {
            return getKirinGpuName(platform);
        }

        // Google Tensor (Mali GPU)
        if (platform.contains("tensor") || platform.contains("gs")) {
            return getTensorGpuName(platform);
        }

        // Intel (for x86 devices)
        if (platform.contains("intel") || platform.contains("atom")) {
            return "Intel HD Graphics";
        }

        // NVIDIA
        if (platform.contains("tegra")) {
            return "NVIDIA Tegra";
        }

        return null;
    }

    /**
     * Get Adreno GPU name based on Qualcomm platform.
     */
    private static String getAdrenoGpuName(String platform) {
        // Map newer platforms to Adreno GPU generations
        if (platform.contains("sun") || platform.contains("pineapple")) {
            return "Adreno 830"; // Snapdragon 8 Elite
        } else if (platform.contains("crow") || platform.contains("kalama")) {
            return "Adreno 750"; // Snapdragon 8 Gen 3
        } else if (platform.contains("taro")) {
            return "Adreno 740"; // Snapdragon 8 Gen 2
        } else if (platform.contains("waipio") || platform.contains("8450")) {
            return "Adreno 730"; // Snapdragon 8 Gen 1
        } else if (platform.contains("lahaina") || platform.contains("888")) {
            return "Adreno 660";
        } else if (platform.contains("kona") || platform.contains("865")) {
            return "Adreno 650";
        } else if (platform.contains("holi") || platform.contains("778")) {
            return "Adreno 642";
        } else if (platform.contains("bengal") || platform.contains("4")) {
            return "Adreno 610";
        }
        // Generic fallback
        return "Adreno GPU";
    }

    /**
     * Get GPU name for Samsung Exynos platforms.
     */
    private static String getExynosGpuName(String platform) {
        if (platform.contains("2400") || platform.contains("s5e9945")) {
            return "Xclipse 940"; // Exynos 2400
        } else if (platform.contains("2200") || platform.contains("s5e9925")) {
            return "Xclipse 920"; // Exynos 2200
        } else if (platform.contains("2100") || platform.contains("s5e9815")) {
            return "Mali-G78 MP14";
        } else if (platform.contains("990")) {
            return "Mali-G77 MP11";
        }
        return "Mali GPU";
    }

    /**
     * Get GPU name for MediaTek platforms.
     */
    private static String getMediatekGpuName(String platform) {
        if (platform.contains("9400") || platform.contains("9300")) {
            return "Immortalis-G925";
        } else if (platform.contains("9200")) {
            return "Immortalis-G715";
        } else if (platform.contains("8300") || platform.contains("8200")) {
            return "Mali-G610 MC6";
        } else if (platform.contains("1200") || platform.contains("1100")) {
            return "Mali-G77 MC9";
        }
        return "Mali GPU";
    }

    /**
     * Get GPU name for HiSilicon Kirin platforms.
     */
    private static String getKirinGpuName(String platform) {
        if (platform.contains("9000") || platform.contains("9010")) {
            return "Mali-G710 MC10";
        } else if (platform.contains("990")) {
            return "Mali-G76 MP16";
        }
        return "Mali GPU";
    }

    /**
     * Get GPU name for Google Tensor platforms.
     */
    private static String getTensorGpuName(String platform) {
        if (platform.contains("gs401") || platform.contains("tensor g4")) {
            return "Mali-G715";
        } else if (platform.contains("gs301") || platform.contains("tensor g3")) {
            return "Mali-G715";
        } else if (platform.contains("gs201") || platform.contains("tensor g2")) {
            return "Mali-G710";
        } else if (platform.contains("gs101") || platform.contains("tensor")) {
            return "Mali-G78";
        }
        return "Mali GPU";
    }

    /**
     * Get GPU info from Build class.
     */
    private static String getGpuFromBuildProperties() {
        try {
            String hardware = Build.HARDWARE;
            String board = Build.BOARD;

            // Try to map hardware/board to GPU
            String gpuName = mapPlatformToGpu(hardware);
            if (gpuName != null) {
                return gpuName;
            }

            gpuName = mapPlatformToGpu(board);
            if (gpuName != null) {
                return gpuName;
            }

            // Fallback: return hardware name as GPU identifier
            if (hardware != null && !hardware.isEmpty()) {
                return hardware;
            }
        } catch (Exception e) {
           Log.e(TAG,"Failed to get GPU from Build properties: " + e.getMessage(),e);
        }
        return null;
    }

    /**
     * Try to get GPU info from dumpsys (may not work on all devices).
     */
    private static String getGpuFromDumpsys() {
        try {
            Process process = Runtime.getRuntime().exec("dumpsys gpu");
            BufferedReader reader = new BufferedReader(new InputStreamReader(process.getInputStream()));
            StringBuilder output = new StringBuilder();
            String line;
            while ((line = reader.readLine()) != null) {
                output.append(line).append("\n");
                // Look for device name in Vulkan info
                if (line.contains("deviceName") || line.contains("Device Name")) {
                    Pattern pattern = Pattern.compile("(?:deviceName|Device Name)[:\\s]+(.+)");
                    Matcher matcher = pattern.matcher(line);
                    if (matcher.find()) {
                        String group = matcher.group(1);
                        if (group != null) {
                            return group.trim();
                        }
                    }
                }
            }
            reader.close();
            process.waitFor();
        } catch (IOException | InterruptedException e) {
            // This is expected to fail on most devices without root
        }
        return null;
    }
}





