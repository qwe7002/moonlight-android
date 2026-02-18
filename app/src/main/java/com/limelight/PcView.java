package com.limelight;

import java.io.FileNotFoundException;
import java.io.IOException;
import java.net.UnknownHostException;
import java.util.Objects;

import com.limelight.binding.PlatformBinding;
import com.limelight.binding.crypto.AndroidCryptoProvider;
import com.limelight.computers.ComputerManagerService;
import com.limelight.computers.PairingService;
import com.limelight.grid.PcGridAdapter;
import com.limelight.grid.assets.DiskAssetLoader;
import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.NvApp;
import com.limelight.nvstream.http.NvHTTP;
import com.limelight.nvstream.http.PairingManager.PairState;
import com.limelight.preferences.VulkanPreferences;
import com.limelight.preferences.PreferenceConfiguration;
import com.limelight.preferences.StreamSettings;
import com.limelight.ui.AdapterFragment;
import com.limelight.ui.AdapterFragmentCallbacks;
import com.limelight.utils.Dialog;
import com.limelight.utils.HelpLauncher;
import com.limelight.utils.ServerHelper;
import com.limelight.utils.UiHelper;

import android.app.ActivityManager;
import android.app.Service;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.Manifest;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.util.Log;
import android.view.ContextMenu;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ContextMenu.ContextMenuInfo;
import android.widget.AbsListView;
import android.widget.ImageButton;
import android.widget.ProgressBar;
import android.widget.RelativeLayout;
import android.widget.TextView;
import android.widget.Toast;
import android.widget.AdapterView.AdapterContextMenuInfo;

import androidx.annotation.NonNull;
import androidx.appcompat.widget.AppCompatImageButton;
import androidx.fragment.app.FragmentActivity;


import org.xmlpull.v1.XmlPullParserException;

import com.limelight.utils.VulkanHelper;

public class PcView extends FragmentActivity implements AdapterFragmentCallbacks {
    private final static String TAG = "PcView";
    private static final int REQUEST_NOTIFICATION_PERMISSION = 1001;

    private RelativeLayout noPcFoundLayout;
    private PcGridAdapter pcGridAdapter;
    private ComputerManagerService.ComputerManagerBinder managerBinder;
    private boolean freezeUpdates, runningPolling, inForeground, completeOnCreateCalled;
    private ComputerDetails pendingPairComputer;

    private final ServiceConnection serviceConnection = new ServiceConnection() {
        public void onServiceConnected(ComponentName className, IBinder binder) {
            final ComputerManagerService.ComputerManagerBinder localBinder =
                    ((ComputerManagerService.ComputerManagerBinder) binder);

            // Wait in a separate thread to avoid stalling the UI
            new Thread(() -> {
                // Wait for the binder to be ready
                localBinder.waitForReady();

                // Now make the binder visible
                managerBinder = localBinder;

                // Start updates
                startComputerUpdates();

                // Force a keypair to be generated early to avoid discovery delays
                new AndroidCryptoProvider(PcView.this).getClientCertificate();
            }).start();
        }

        public void onServiceDisconnected(ComponentName className) {
            managerBinder = null;
        }
    };

    @Override
    public void onConfigurationChanged(@NonNull Configuration newConfig) {
        super.onConfigurationChanged(newConfig);

        // Only reinitialize views if completeOnCreate() was called
        // before this callback. If it was not, completeOnCreate() will
        // handle initializing views with the config change accounted for.
        // This is not prone to races because both callbacks are invoked
        // in the main thread.
        if (completeOnCreateCalled) {
            // Reinitialize views just in case orientation changed
            initializeViews();
        }
    }

    private final static int PAIR_ID = 2;
    private final static int UNPAIR_ID = 3;
    private final static int DELETE_ID = 5;
    private final static int RESUME_ID = 6;
    private final static int QUIT_ID = 7;
    private final static int VIEW_DETAILS_ID = 8;
    private final static int FULL_APP_LIST_ID = 9;
    private final static int TEST_NETWORK_ID = 10;

    private void initializeViews() {
        setContentView(R.layout.activity_pc_view);

        UiHelper.notifyNewRootView(this);
        UiHelper.applyStatusBarPadding(findViewById(android.R.id.content));

        // Allow floating expanded PiP overlays while browsing PCs
        setShouldDockBigOverlays(false);


        // Set the correct layout for the PC grid
        PreferenceConfiguration prefConfig = PreferenceConfiguration.readPreferences(this);
        pcGridAdapter.updateLayoutWithPreferences(this, prefConfig);

        // Setup the list view
        ImageButton settingsButton = findViewById(R.id.settingsButton);
        AppCompatImageButton addComputerButton = findViewById(R.id.manuallyAddPc);
        ImageButton helpButton = findViewById(R.id.helpButton);

        settingsButton.setOnClickListener(v -> startActivity(new Intent(PcView.this, StreamSettings.class)));
        addComputerButton.setOnClickListener(v -> showAddComputerDialog());
        helpButton.setOnClickListener(v -> HelpLauncher.launchSetupGuide(PcView.this));

        getSupportFragmentManager().beginTransaction()
                .replace(R.id.pcFragmentContainer, new AdapterFragment())
                .commitAllowingStateLoss();

        noPcFoundLayout = findViewById(R.id.no_pc_found_layout);

        // Update the hint text and ProgressBar visibility based on mDNS setting
        TextView searchingText = findViewById(R.id.searching_text);
        ProgressBar pcsLoading = findViewById(R.id.pcs_loading);
        if (!prefConfig.enableMdns) {
            // mDNS is disabled, show manual add hint and hide ProgressBar
            searchingText.setText(R.string.searching_pc_mdns_disabled);
            pcsLoading.setVisibility(View.GONE);
        } else {
            // mDNS is enabled, show searching hint and ProgressBar
            searchingText.setText(R.string.searching_pc);
            pcsLoading.setVisibility(View.VISIBLE);
        }

        if (pcGridAdapter.getCount() == 0) {
            noPcFoundLayout.setVisibility(View.VISIBLE);
        } else {
            noPcFoundLayout.setVisibility(View.INVISIBLE);
        }
        pcGridAdapter.notifyDataSetChanged();
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        // Assume we're in the foreground when created to avoid a race
        // between binding to CMS and onResume()
        inForeground = true;

        // Get GPU renderer info using Vulkan instead of OpenGL ES
        final VulkanPreferences vulkanPreferences = VulkanPreferences.readPreferences(this);
        if (!vulkanPreferences.savedFingerprint.equals(Build.FINGERPRINT) || vulkanPreferences.VulkanRenderer.isEmpty()) {
            // Use Vulkan to detect GPU renderer
            String gpuRenderer = VulkanHelper.getGpuRenderer();
            vulkanPreferences.VulkanRenderer = gpuRenderer;
            vulkanPreferences.savedFingerprint = Build.FINGERPRINT;
            vulkanPreferences.writePreferences();
            Log.i(TAG, "Fetched GPU Renderer via Vulkan: " + gpuRenderer);
        } else {
            Log.i(TAG, "Cached GPU Renderer: " + vulkanPreferences.VulkanRenderer);
        }

        completeOnCreate();
    }

    private void completeOnCreate() {
        completeOnCreateCalled = true;


        UiHelper.setLocale(this);

        // Bind to the computer manager service
        bindService(new Intent(PcView.this, ComputerManagerService.class), serviceConnection,
                Service.BIND_AUTO_CREATE);

        // Bind to USB driver service early to request gamepad permissions
        PreferenceConfiguration prefConfig = PreferenceConfiguration.readPreferences(this);

        pcGridAdapter = new PcGridAdapter(this, prefConfig);

        initializeViews();
    }

    private void startComputerUpdates() {
        // Only allow polling to start if we're bound to CMS, polling is not already running,
        // and our activity is in the foreground.
        if (managerBinder != null && !runningPolling && inForeground) {
            freezeUpdates = false;
            managerBinder.startPolling(details -> {
                if (!freezeUpdates && !isFinishing() && !isDestroyed()) {
                    PcView.this.runOnUiThread(() -> updateComputer(details));
                }
            });
            runningPolling = true;
        }
    }

    private void stopComputerUpdates() {
        if (managerBinder != null) {
            if (!runningPolling) {
                return;
            }

            freezeUpdates = true;

            managerBinder.stopPolling();

            runningPolling = false;
        }
    }

    @Override
    public void onDestroy() {
        super.onDestroy();

        if (managerBinder != null) {
            unbindService(serviceConnection);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();

        // Display a decoder crash notification if we've returned after a crash
        UiHelper.showDecoderCrashDialog(this);

        inForeground = true;
        startComputerUpdates();
    }

    @Override
    protected void onPause() {
        super.onPause();

        inForeground = false;
        stopComputerUpdates();
    }

    @Override
    protected void onStop() {
        super.onStop();

        Dialog.closeDialogs();
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, @NonNull String[] permissions, @NonNull int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQUEST_NOTIFICATION_PERMISSION) {
            // Continue with pairing regardless of whether permission was granted
            // The foreground service will still work, just without notification on Android 13+
            if (pendingPairComputer != null) {
                final ComputerDetails computer = pendingPairComputer;
                pendingPairComputer = null;
                doPair(computer);
            }
        }
    }

    @Override
    public void onCreateContextMenu(ContextMenu menu, View v, ContextMenuInfo menuInfo) {
        stopComputerUpdates();

        // Call superclass
        super.onCreateContextMenu(menu, v, menuInfo);

        AdapterContextMenuInfo info = (AdapterContextMenuInfo) menuInfo;
        ComputerObject computer = (ComputerObject) pcGridAdapter.getItem(info.position);

        // Add a header with PC status details
        menu.clearHeader();
        String headerTitle = computer.details.name + " - ";
        switch (computer.details.state) {
            case ONLINE:
                headerTitle += getResources().getString(R.string.pcview_menu_header_online);
                break;
            case OFFLINE:
                menu.setHeaderIcon(R.drawable.ic_pc_offline);
                headerTitle += getResources().getString(R.string.pcview_menu_header_offline);
                break;
            case UNKNOWN:
                headerTitle += getResources().getString(R.string.pcview_menu_header_unknown);
                break;
        }

        menu.setHeaderTitle(headerTitle);

        // Inflate the context menu
        if (computer.details.pairState != PairState.PAIRED) {
            menu.add(Menu.NONE, PAIR_ID, 1, getResources().getString(R.string.pcview_menu_pair_pc));
        } else {
            if (computer.details.runningGameId != 0) {
                menu.add(Menu.NONE, RESUME_ID, 1, getResources().getString(R.string.applist_menu_resume));
                menu.add(Menu.NONE, QUIT_ID, 2, getResources().getString(R.string.applist_menu_quit));
            }


            menu.add(Menu.NONE, FULL_APP_LIST_ID, 4, getResources().getString(R.string.pcview_menu_app_list));
        }

        menu.add(Menu.NONE, TEST_NETWORK_ID, 5, getResources().getString(R.string.pcview_menu_test_network));
        menu.add(Menu.NONE, DELETE_ID, 6, getResources().getString(R.string.pcview_menu_delete_pc));
        menu.add(Menu.NONE, VIEW_DETAILS_ID, 7, getResources().getString(R.string.pcview_menu_details));
    }

    @Override
    public void onContextMenuClosed(@NonNull Menu menu) {
        // For some reason, this gets called again _after_ onPause() is called on this activity.
        // startComputerUpdates() manages this and won't actual start polling until the activity
        // returns to the foreground.
        startComputerUpdates();
    }

    private void doPair(final ComputerDetails computer) {
        if (computer.state == ComputerDetails.State.OFFLINE || computer.activeAddress == null) {
            Dialog.displayDialog(PcView.this,
                    getResources().getString(R.string.pairing_notification_failed_title),
                    getResources().getString(R.string.pair_pc_offline),
                    false);
            return;
        }
        if (managerBinder == null) {
            Dialog.displayDialog(PcView.this,
                    getResources().getString(R.string.conn_error_title),
                    getResources().getString(R.string.error_manager_not_running),
                    false);
            return;
        }

        // Check notification permission on Android 13+ before starting foreground service
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            // Save the computer for pairing after permission is granted
            pendingPairComputer = computer;
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, REQUEST_NOTIFICATION_PERMISSION);
            return;
        }

        // Show Sunshine pairing dialog to get credentials
        Dialog.displaySunshinePairingDialog(this, computer.name, new Dialog.SunshinePairingCallback() {
            @Override
            public void onCredentialsEntered(String username, String password) {
                startPairingService(computer, username, password);
            }

            @Override
            public void onCancelled() {
                // User cancelled pairing
            }
        });
    }

    private void showAddComputerDialog() {
        if (managerBinder == null) {
            Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
            return;
        }

        Dialog.displayAddComputerDialog(this, hostAddress -> {
            Toast.makeText(PcView.this, getResources().getString(R.string.msg_add_pc), Toast.LENGTH_SHORT).show();

            new Thread(() -> {
                boolean success = false;
                try {
                    // Parse the host address
                    java.net.URI uri = parseHostInput(hostAddress);
                    if (uri != null && uri.getHost() != null && !uri.getHost().isEmpty()) {
                        String host = uri.getHost();
                        int port = uri.getPort();
                        if (port == -1) {
                            port = NvHTTP.DEFAULT_HTTP_PORT;
                        }

                        ComputerDetails details = new ComputerDetails();
                        details.manualAddress = new ComputerDetails.AddressTuple(host, port);
                        success = managerBinder.addComputerBlocking(details);
                    }
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                } catch (Exception e) {
                    Log.e(TAG, "Failed to add computer: " + e.getMessage());
                }

                final boolean finalSuccess = success;
                runOnUiThread(() -> {
                    if (finalSuccess) {
                        Toast.makeText(PcView.this, getResources().getString(R.string.addpc_success), Toast.LENGTH_SHORT).show();
                    } else {
                        Toast.makeText(PcView.this, getResources().getString(R.string.addpc_fail), Toast.LENGTH_LONG).show();
                    }
                });
            }).start();
        });
    }

    private java.net.URI parseHostInput(String rawUserInput) {
        try {
            // Try adding a scheme and parsing the remaining input
            java.net.URI uri = new java.net.URI("moonlight://" + rawUserInput);
            if (uri.getHost() != null && !uri.getHost().isEmpty()) {
                return uri;
            }
        } catch (java.net.URISyntaxException ignored) {
        }

        try {
            // Attempt to escape the input as an IPv6 literal
            java.net.URI uri = new java.net.URI("moonlight://[" + rawUserInput + "]");
            if (uri.getHost() != null && !uri.getHost().isEmpty()) {
                return uri;
            }
        } catch (java.net.URISyntaxException ignored) {
        }

        return null;
    }

    private void startPairingService(final ComputerDetails computer, String username, String password) {
        // Show progress dialog
        Dialog.displayProgressDialog(this,
                getString(R.string.pair_pairing_title),
                getString(R.string.pairing),
                null);

        // Start PairingService with Sunshine credentials for automatic pairing
        Intent pairingIntent = new Intent(this, PairingService.class);
        pairingIntent.putExtra(PairingService.EXTRA_COMPUTER_UUID, computer.uuid);
        pairingIntent.putExtra(PairingService.EXTRA_COMPUTER_NAME, computer.name);
        pairingIntent.putExtra(PairingService.EXTRA_COMPUTER_ADDRESS, computer.activeAddress.address);
        pairingIntent.putExtra(PairingService.EXTRA_COMPUTER_HTTP_PORT, computer.activeAddress.port);
        pairingIntent.putExtra(PairingService.EXTRA_COMPUTER_HTTPS_PORT, computer.httpsPort);
        pairingIntent.putExtra(PairingService.EXTRA_UNIQUE_ID, managerBinder.getUniqueId());

        // Add Sunshine credentials
        pairingIntent.putExtra(PairingService.EXTRA_SUNSHINE_USERNAME, username);
        pairingIntent.putExtra(PairingService.EXTRA_SUNSHINE_PASSWORD, password);

        try {
            if (computer.serverCert != null) {
                pairingIntent.putExtra(PairingService.EXTRA_SERVER_CERT, computer.serverCert.getEncoded());
            }
        } catch (java.security.cert.CertificateEncodingException e) {
            Log.e(TAG, "Failed to encode server certificate for pairing service: " + e.getMessage());
            Log.e(TAG, "startPairingService: " + e.getMessage(), e);
        }

        // Bind to the service to receive pairing result callbacks
        ServiceConnection pairingServiceConnection = new ServiceConnection() {
            @Override
            public void onServiceConnected(ComponentName name, IBinder service) {
                PairingService.PairingBinder binder = (PairingService.PairingBinder) service;
                final ServiceConnection conn = this;
                binder.setListener(new PairingService.PairingListener() {
                    @Override
                    public void onPairingSuccess(String computerUuid, java.security.cert.X509Certificate serverCert) {
                        runOnUiThread(() -> {
                            Dialog.dismissProgressDialog();
                            if (isFinishing() || isDestroyed()) {
                                safeUnbind(conn);
                                return;
                            }
                            Toast.makeText(PcView.this, R.string.sunshine_pairing_success, Toast.LENGTH_SHORT).show();
                            // Pin this certificate for later HTTPS use
                            if (managerBinder != null) {
                                ComputerDetails comp = managerBinder.getComputer(computerUuid);
                                if (comp != null) {
                                    comp.serverCert = serverCert;
                                    // Invalidate reachability information after pairing
                                    managerBinder.invalidateStateForComputer(computerUuid);
                                }
                            }
                            // Open the app list after a successful pairing attempt
                            doAppList(computer, true, false);
                        });
                        safeUnbind(conn);
                    }

                    @Override
                    public void onPairingFailed(String computerUuid, String message) {
                        runOnUiThread(() -> {
                            Dialog.dismissProgressDialog();
                            if (isFinishing() || isDestroyed()) {
                                safeUnbind(conn);
                                return;
                            }
                            String errorMsg = message != null ? message : getString(R.string.pair_fail);
                            Dialog.displayDialog(PcView.this,
                                    getString(R.string.pairing_notification_failed_title),
                                    getString(R.string.sunshine_pairing_failed, errorMsg),
                                    false);
                            // Start polling again if we're still in the foreground
                            startComputerUpdates();
                        });
                        safeUnbind(conn);
                    }

                    private void safeUnbind(ServiceConnection connection) {
                        try {
                            PcView.this.unbindService(connection);
                        } catch (Exception e) {
                            // Ignore if already unbound
                        }
                    }
                });
            }

            @Override
            public void onServiceDisconnected(ComponentName name) {
                // Service crashed or was killed
                runOnUiThread(() -> startComputerUpdates());
            }
        };
        bindService(pairingIntent, pairingServiceConnection, Service.BIND_AUTO_CREATE);

        startForegroundService(pairingIntent);
    }

    private void doUnpair(final ComputerDetails computer) {
        if (computer.state == ComputerDetails.State.OFFLINE || computer.activeAddress == null) {
            Toast.makeText(PcView.this, getResources().getString(R.string.error_pc_offline), Toast.LENGTH_SHORT).show();
            return;
        }
        if (managerBinder == null) {
            Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
            return;
        }

        Toast.makeText(PcView.this, getResources().getString(R.string.unpairing), Toast.LENGTH_SHORT).show();
        new Thread(() -> {
            NvHTTP httpConn;
            String message;
            try {
                httpConn = new NvHTTP(ServerHelper.getCurrentAddressFromComputer(computer),
                        computer.httpsPort, managerBinder.getUniqueId(), computer.serverCert,
                        PlatformBinding.getCryptoProvider(PcView.this));
                if (httpConn.getPairState() == PairState.PAIRED) {
                    httpConn.unpair();
                    if (httpConn.getPairState() == PairState.NOT_PAIRED) {
                        message = getResources().getString(R.string.unpair_success);
                    } else {
                        message = getResources().getString(R.string.unpair_fail);
                    }
                } else {
                    message = getResources().getString(R.string.unpair_error);
                }
            } catch (UnknownHostException e) {
                message = getResources().getString(R.string.error_unknown_host);
            } catch (FileNotFoundException e) {
                message = getResources().getString(R.string.error_404);
            } catch (XmlPullParserException | IOException e) {
                message = e.getMessage();
                Log.e(TAG, "run: " + e.getMessage(), e);

            }

            final String toastMessage = message;
            runOnUiThread(() -> Toast.makeText(PcView.this, toastMessage, Toast.LENGTH_LONG).show());
        }).start();
    }

    private void doAppList(ComputerDetails computer, boolean newlyPaired, boolean showHiddenGames) {
        if (computer.state == ComputerDetails.State.OFFLINE) {
            Toast.makeText(PcView.this, getResources().getString(R.string.error_pc_offline), Toast.LENGTH_SHORT).show();
            return;
        }
        if (managerBinder == null) {
            Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
            return;
        }

        Intent i = new Intent(this, AppView.class);
        i.putExtra(AppView.NAME_EXTRA, computer.name);
        i.putExtra(AppView.UUID_EXTRA, computer.uuid);
        i.putExtra(AppView.NEW_PAIR_EXTRA, newlyPaired);
        i.putExtra(AppView.SHOW_HIDDEN_APPS_EXTRA, showHiddenGames);
        startActivity(i);
    }

    @Override
    public boolean onContextItemSelected(MenuItem item) {
        AdapterContextMenuInfo info = (AdapterContextMenuInfo) item.getMenuInfo();
        final ComputerObject computer = (ComputerObject) pcGridAdapter.getItem(Objects.requireNonNull(info).position);
        switch (item.getItemId()) {
            case PAIR_ID:
                doPair(computer.details);
                return true;

            case UNPAIR_ID:
                doUnpair(computer.details);
                return true;

            case DELETE_ID:
                if (ActivityManager.isUserAMonkey()) {
                    Log.i(TAG, "Ignoring delete PC request from monkey");
                    return true;
                }
                UiHelper.displayDeletePcConfirmationDialog(this, computer.details, () -> {
                    if (managerBinder == null) {
                        Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
                        return;
                    }
                    removeComputer(computer.details);
                }, null);
                return true;

            case FULL_APP_LIST_ID:
                doAppList(computer.details, false, true);
                return true;

            case RESUME_ID:
                if (managerBinder == null) {
                    Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
                    return true;
                }

                ServerHelper.doStart(this, new NvApp("app", computer.details.runningGameId, false), computer.details, managerBinder);
                return true;

            case QUIT_ID:
                if (managerBinder == null) {
                    Toast.makeText(PcView.this, getResources().getString(R.string.error_manager_not_running), Toast.LENGTH_LONG).show();
                    return true;
                }

                // Display a confirmation dialog first
                UiHelper.displayQuitConfirmationDialog(this, () -> ServerHelper.doQuit(PcView.this, computer.details,
                        new NvApp("app", 0, false), managerBinder, null), null);
                return true;

            case VIEW_DETAILS_ID:
                Dialog.displayDialog(PcView.this, getResources().getString(R.string.title_details), computer.details.toString(), false);
                return true;

            case TEST_NETWORK_ID:
                ServerHelper.doNetworkTest(PcView.this, computer.details);
                return true;

            default:
                return super.onContextItemSelected(item);
        }
    }

    private void removeComputer(ComputerDetails details) {
        managerBinder.removeComputer(details);

        new DiskAssetLoader(this).deleteAssetsForComputer(details.uuid);

        // Delete hidden games preference value
        getSharedPreferences(AppView.HIDDEN_APPS_PREF_FILENAME, MODE_PRIVATE)
                .edit()
                .remove(details.uuid)
                .apply();

        for (int i = 0; i < pcGridAdapter.getCount(); i++) {
            ComputerObject computer = (ComputerObject) pcGridAdapter.getItem(i);

            if (details.equals(computer.details)) {

                pcGridAdapter.removeComputer(computer);
                pcGridAdapter.notifyDataSetChanged();

                if (pcGridAdapter.getCount() == 0) {
                    // Show the "Discovery in progress" view
                    noPcFoundLayout.setVisibility(View.VISIBLE);
                }

                break;
            }
        }
    }

    private void updateComputer(ComputerDetails details) {
        ComputerObject existingEntry = null;

        for (int i = 0; i < pcGridAdapter.getCount(); i++) {
            ComputerObject computer = (ComputerObject) pcGridAdapter.getItem(i);

            // Check if this is the same computer
            if (details.uuid.equals(computer.details.uuid)) {
                existingEntry = computer;
                break;
            }
        }

        if (existingEntry != null) {
            // Replace the information in the existing entry
            existingEntry.details = details;
        } else {
            // Add a new entry
            pcGridAdapter.addComputer(new ComputerObject(details));

            // Remove the "Discovery in progress" view
            noPcFoundLayout.setVisibility(View.INVISIBLE);
        }

        // Notify the view that the data has changed
        pcGridAdapter.notifyDataSetChanged();
    }

    @Override
    public int getAdapterFragmentLayoutId() {
        return R.layout.pc_grid_view;
    }

    @Override
    public void receiveAbsListView(AbsListView listView) {
        listView.setAdapter(pcGridAdapter);
        listView.setOnItemClickListener((arg0, arg1, pos, id) -> {
            ComputerObject computer = (ComputerObject) pcGridAdapter.getItem(pos);
            if (computer.details.state == ComputerDetails.State.UNKNOWN ||
                    computer.details.state == ComputerDetails.State.OFFLINE) {
                // Open the context menu if a PC is offline or refreshing
                openContextMenu(arg1);
            } else if (computer.details.pairState != PairState.PAIRED) {
                // Pair an unpaired machine by default
                doPair(computer.details);
            } else {
                doAppList(computer.details, false, false);
            }
        });
        registerForContextMenu(listView);
    }

    public static class ComputerObject {
        public ComputerDetails details;

        public ComputerObject(ComputerDetails details) {
            if (details == null) {
                throw new IllegalArgumentException("details must not be null");
            }
            this.details = details;
        }

        @NonNull
        @Override
        public String toString() {
            return details.name;
        }
    }
}
