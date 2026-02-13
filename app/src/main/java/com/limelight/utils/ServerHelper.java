package com.limelight.utils;

import android.app.Activity;
import android.content.Intent;
import android.util.Log;
import android.widget.Toast;

import com.limelight.Game;
import com.limelight.R;
import com.limelight.binding.PlatformBinding;
import com.limelight.computers.ComputerManagerService;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.HostHttpResponseException;
import com.limelight.nvstream.http.NvApp;
import com.limelight.nvstream.http.NvHTTP;

import org.xmlpull.v1.XmlPullParserException;

import java.io.FileNotFoundException;
import java.io.IOException;
import java.net.UnknownHostException;
import java.security.cert.CertificateEncodingException;

public class ServerHelper {
    private static final String TAG = "ServerHelper";


    public static ComputerDetails.AddressTuple getCurrentAddressFromComputer(ComputerDetails computer) throws IOException {
        if (computer.activeAddress == null) {
            throw new IOException("No active address for " + computer.name);
        }
        return computer.activeAddress;
    }

    public static Intent createStartIntent(Activity parent, NvApp app, ComputerDetails computer,
                                           ComputerManagerService.ComputerManagerBinder managerBinder) {
        Intent intent = new Intent(parent, Game.class);
        intent.putExtra(Game.EXTRA_HOST, computer.activeAddress.address);
        intent.putExtra(Game.EXTRA_PORT, computer.activeAddress.port);
        intent.putExtra(Game.EXTRA_HTTPS_PORT, computer.httpsPort);
        intent.putExtra(Game.EXTRA_APP_NAME, app.getAppName());
        intent.putExtra(Game.EXTRA_APP_ID, app.getAppId());
        intent.putExtra(Game.EXTRA_APP_HDR, app.isHdrSupported());
        intent.putExtra(Game.EXTRA_UNIQUEID, managerBinder.getUniqueId());
        intent.putExtra(Game.EXTRA_PC_UUID, computer.uuid);
        intent.putExtra(Game.EXTRA_PC_NAME, computer.name);
        try {
            if (computer.serverCert != null) {
                intent.putExtra(Game.EXTRA_SERVER_CERT, computer.serverCert.getEncoded());
            }
        } catch (CertificateEncodingException e) {
            Log.e(TAG, "createStartIntent: " + e.getMessage(), e);
        }
        return intent;
    }

    public static void doStart(Activity parent, NvApp app, ComputerDetails computer,
                               ComputerManagerService.ComputerManagerBinder managerBinder) {
        if (computer.state == ComputerDetails.State.OFFLINE || computer.activeAddress == null) {
            Dialog.displayDialog(parent,
                    parent.getResources().getString(R.string.conn_error_title),
                    parent.getResources().getString(R.string.pair_pc_offline),
                    false);
            return;
        }
        parent.startActivity(createStartIntent(parent, app, computer, managerBinder));
    }

    public static void doNetworkTest(final Activity parent, final ComputerDetails computer) {
        new Thread(() -> {
            SpinnerDialog spinnerDialog = SpinnerDialog.displayDialog(parent,
                    parent.getResources().getString(R.string.nettest_title_waiting),
                    parent.getResources().getString(R.string.nettest_text_waiting),
                    false);

            StringBuilder dialogSummary = new StringBuilder();

            // First, test TCP connectivity to the host if computer details are provided
            if (computer != null) {
                boolean hasAnyAddress = false;

                // Test all available addresses
                if (computer.activeAddress != null) {
                    hasAnyAddress = true;
                    dialogSummary.append("Active Address (").append(computer.activeAddress).append("):\n");
                    TcpReachability.TcpPingResult result = TcpReachability.tcpPingAddress(computer.activeAddress);
                    appendPingResult(parent, dialogSummary, result);
                }

                if (computer.localAddress != null && !computer.localAddress.equals(computer.activeAddress)) {
                    hasAnyAddress = true;
                    dialogSummary.append("Local Address (").append(computer.localAddress).append("):\n");
                    TcpReachability.TcpPingResult result = TcpReachability.tcpPingAddress(computer.localAddress);
                    appendPingResult(parent, dialogSummary, result);
                }

                if (computer.remoteAddress != null && !computer.remoteAddress.equals(computer.activeAddress)) {
                    hasAnyAddress = true;
                    dialogSummary.append("Remote Address (").append(computer.remoteAddress).append("):\n");
                    TcpReachability.TcpPingResult result = TcpReachability.tcpPingAddress(computer.remoteAddress);
                    appendPingResult(parent, dialogSummary, result);
                }

                if (computer.manualAddress != null && !computer.manualAddress.equals(computer.activeAddress)) {
                    hasAnyAddress = true;
                    dialogSummary.append("Manual Address (").append(computer.manualAddress).append("):\n");
                    TcpReachability.TcpPingResult result = TcpReachability.tcpPingAddress(computer.manualAddress);
                    appendPingResult(parent, dialogSummary, result);
                }

                if (computer.ipv6Address != null && !computer.ipv6Address.equals(computer.activeAddress)) {
                    hasAnyAddress = true;
                    dialogSummary.append("IPv6 Address (").append(computer.ipv6Address).append("):\n");
                    TcpReachability.TcpPingResult result = TcpReachability.tcpPingAddress(computer.ipv6Address);
                    appendPingResult(parent, dialogSummary, result);
                }

                if (!hasAnyAddress) {
                    dialogSummary.append(parent.getResources().getString(R.string.nettest_pc_no_address)).append("\n");
                }
            }
            spinnerDialog.dismiss();
            Dialog.displayDialog(parent,
                    parent.getResources().getString(R.string.nettest_title_done),
                    dialogSummary.toString(),
                    false);
        }).start();
    }

    private static void appendPingResult(Activity parent, StringBuilder sb, TcpReachability.TcpPingResult result) {
        if (result.success) {
            sb.append("  ").append(parent.getResources().getString(R.string.nettest_pc_tcp_success));
            sb.append(" (").append(String.format(parent.getResources().getString(R.string.nettest_pc_tcp_latency), result.latencyMs)).append(")\n\n");
        } else {
            sb.append("  ").append(parent.getResources().getString(R.string.nettest_pc_tcp_failed));
            if (result.errorMessage != null) {
                sb.append("\n  ").append(result.errorMessage);
            }
            sb.append("\n\n");
        }
    }

    /**
     * @deprecated Use {@link #doNetworkTest(Activity, ComputerDetails)} instead
     */
    @Deprecated
    public static void doNetworkTest(final Activity parent) {
        doNetworkTest(parent, null);
    }

    public static void doQuit(final Activity parent,
                              final ComputerDetails computer,
                              final NvApp app,
                              final ComputerManagerService.ComputerManagerBinder managerBinder,
                              final Runnable onComplete) {
        Toast.makeText(parent, parent.getResources().getString(R.string.applist_quit_app) + " " + app.getAppName() + "...", Toast.LENGTH_SHORT).show();
        new Thread(() -> {
            NvHTTP httpConn;
            String message;
            try {
                httpConn = new NvHTTP(ServerHelper.getCurrentAddressFromComputer(computer), computer.httpsPort,
                        managerBinder.getUniqueId(), computer.serverCert, PlatformBinding.getCryptoProvider(parent));
                if (httpConn.quitApp()) {
                    message = parent.getResources().getString(R.string.applist_quit_success) + " " + app.getAppName();
                } else {
                    message = parent.getResources().getString(R.string.applist_quit_fail) + " " + app.getAppName();
                }
            } catch (HostHttpResponseException e) {
                if (e.getErrorCode() == 599) {
                    message = "This session wasn't started by this device," +
                            " so it cannot be quit. End streaming on the original " +
                            "device or the PC itself. (Error code: " + e.getErrorCode() + ")";
                } else {
                    message = e.getMessage();
                }
            } catch (UnknownHostException e) {
                message = parent.getResources().getString(R.string.error_unknown_host);
            } catch (FileNotFoundException e) {
                message = parent.getResources().getString(R.string.error_404);
            } catch (IOException | XmlPullParserException e) {
                message = parent.getResources().getString(R.string.applist_quit_fail) + " " + app.getAppName() + ": " + e.getMessage();
                Log.e(TAG, "doQuit: " + e.getMessage(), e);
            } finally {
                if (onComplete != null) {
                    onComplete.run();
                }
            }

            final String toastMessage = message;
            parent.runOnUiThread(() -> Toast.makeText(parent, toastMessage, Toast.LENGTH_LONG).show());
        }).start();
    }
}
