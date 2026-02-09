package com.limelight.computers;

import android.content.Context;
import android.util.Base64;
import android.util.Log;

import com.limelight.nvstream.http.ComputerDetails;
import com.limelight.nvstream.http.NvHTTP;
import com.tencent.mmkv.MMKV;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayInputStream;
import java.security.cert.CertificateEncodingException;
import java.security.cert.CertificateException;
import java.security.cert.CertificateFactory;
import java.security.cert.X509Certificate;
import java.util.ArrayList;
import java.util.List;

public class ComputerDatabaseManager {
    private static final String MMKV_ID = "computers_mmkv";
    private static final String COMPUTERS_LIST_KEY = "computer_uuids";
    private static final String TAG = "ComputerDatabaseManager";

    private interface ComputerFields {
        String UUID = "uuid";
        String NAME = "name";
        String LOCAL_ADDRESS = "localAddress";
        String REMOTE_ADDRESS = "remoteAddress";
        String MANUAL_ADDRESS = "manualAddress";
        String IPV6_ADDRESS = "ipv6Address";
        String MAC_ADDRESS = "macAddress";
        String SERVER_CERT = "serverCert";
    }

    private interface AddressFields {
        String ADDRESS = "address";
        String PORT = "port";
    }

    private final MMKV mmkv;

    public ComputerDatabaseManager(Context c) {
        MMKV.initialize(c);
        mmkv = MMKV.mmkvWithID(MMKV_ID);
    }

    public void close() {
        // MMKV doesn't need explicit close
    }

    public static JSONObject tupleToJson(ComputerDetails.AddressTuple tuple) throws JSONException {
        if (tuple == null) {
            return null;
        }

        JSONObject json = new JSONObject();
        json.put(AddressFields.ADDRESS, tuple.address);
        json.put(AddressFields.PORT, tuple.port);

        return json;
    }

    public static ComputerDetails.AddressTuple tupleFromJson(JSONObject json) throws JSONException {
        if (json == null) {
            return null;
        }

        return new ComputerDetails.AddressTuple(
                json.getString(AddressFields.ADDRESS), json.getInt(AddressFields.PORT));
    }

    private String computerToJson(ComputerDetails details) {
        try {
            JSONObject json = new JSONObject();
            json.put(ComputerFields.UUID, details.uuid);
            json.put(ComputerFields.NAME, details.name);

            if (details.localAddress != null) {
                json.put(ComputerFields.LOCAL_ADDRESS, tupleToJson(details.localAddress));
            }
            if (details.remoteAddress != null) {
                json.put(ComputerFields.REMOTE_ADDRESS, tupleToJson(details.remoteAddress));
            }
            if (details.manualAddress != null) {
                json.put(ComputerFields.MANUAL_ADDRESS, tupleToJson(details.manualAddress));
            }
            if (details.ipv6Address != null) {
                json.put(ComputerFields.IPV6_ADDRESS, tupleToJson(details.ipv6Address));
            }

            json.put(ComputerFields.MAC_ADDRESS, details.macAddress);

            if (details.serverCert != null) {
                try {
                    String certBase64 = Base64.encodeToString(details.serverCert.getEncoded(), Base64.NO_WRAP);
                    json.put(ComputerFields.SERVER_CERT, certBase64);
                } catch (CertificateEncodingException e) {
                    Log.e(TAG, "computerToJson: " + e.getMessage(), e);
                }
            }

            return json.toString();
        } catch (JSONException e) {
            throw new RuntimeException(e);
        }
    }

    private ComputerDetails computerFromJson(String jsonStr) {
        if (jsonStr == null || jsonStr.isEmpty()) {
            return null;
        }

        try {
            JSONObject json = new JSONObject(jsonStr);
            ComputerDetails details = new ComputerDetails();

            details.uuid = json.optString(ComputerFields.UUID, null);
            details.name = json.optString(ComputerFields.NAME, null);

            if (json.has(ComputerFields.LOCAL_ADDRESS)) {
                details.localAddress = tupleFromJson(json.getJSONObject(ComputerFields.LOCAL_ADDRESS));
            }
            if (json.has(ComputerFields.REMOTE_ADDRESS)) {
                details.remoteAddress = tupleFromJson(json.getJSONObject(ComputerFields.REMOTE_ADDRESS));
            }
            if (json.has(ComputerFields.MANUAL_ADDRESS)) {
                details.manualAddress = tupleFromJson(json.getJSONObject(ComputerFields.MANUAL_ADDRESS));
            }
            if (json.has(ComputerFields.IPV6_ADDRESS)) {
                details.ipv6Address = tupleFromJson(json.getJSONObject(ComputerFields.IPV6_ADDRESS));
            }

            // External port is persisted in the remote address field
            if (details.remoteAddress != null) {
                details.externalPort = details.remoteAddress.port;
            } else {
                details.externalPort = NvHTTP.DEFAULT_HTTP_PORT;
            }

            details.macAddress = json.optString(ComputerFields.MAC_ADDRESS, null);

            String certBase64 = json.optString(ComputerFields.SERVER_CERT, null);
            if (!certBase64.isEmpty()) {
                try {
                    byte[] derCertData = Base64.decode(certBase64, Base64.NO_WRAP);
                    details.serverCert = (X509Certificate) CertificateFactory.getInstance("X.509")
                            .generateCertificate(new ByteArrayInputStream(derCertData));
                } catch (CertificateException e) {
                    Log.e(TAG, "computerFromJson: Failed to decode server certificate - " + e.getMessage(), e);
                }
            }

            // This signifies we don't have dynamic state (like pair state)
            details.state = ComputerDetails.State.UNKNOWN;

            return details;
        } catch (JSONException e) {
            Log.e(TAG, "computerFromJson: "+e.getMessage(),e );
            return null;
        }
    }

    private List<String> getComputerUuids() {
        String uuidsJson = mmkv.decodeString(COMPUTERS_LIST_KEY, "[]");
        List<String> uuids = new ArrayList<>();
        try {
            JSONArray jsonArray = new JSONArray(uuidsJson);
            for (int i = 0; i < jsonArray.length(); i++) {
                uuids.add(jsonArray.getString(i));
            }
        } catch (JSONException e) {
            Log.e(TAG, "getComputerUuids: " + e.getMessage(), e);
        }
        return uuids;
    }

    private void saveComputerUuids(List<String> uuids) {
        JSONArray jsonArray = new JSONArray(uuids);
        mmkv.encode(COMPUTERS_LIST_KEY, jsonArray.toString());
    }

    public void updateComputer(ComputerDetails details) {
        if (details.uuid == null) {
            return;
        }

        // Save computer data
        String jsonStr = computerToJson(details);
        mmkv.encode(details.uuid, jsonStr);

        // Update UUID list if not already present
        List<String> uuids = getComputerUuids();
        if (!uuids.contains(details.uuid)) {
            uuids.add(details.uuid);
            saveComputerUuids(uuids);
        }

    }

    public void deleteComputer(ComputerDetails details) {
        if (details.uuid == null) {
            return;
        }

        // Remove computer data
        mmkv.removeValueForKey(details.uuid);

        // Remove from UUID list
        List<String> uuids = getComputerUuids();
        uuids.remove(details.uuid);
        saveComputerUuids(uuids);
    }

    public List<ComputerDetails> getAllComputers() {
        List<ComputerDetails> computers = new ArrayList<>();
        List<String> uuids = getComputerUuids();

        for (String uuid : uuids) {
            String jsonStr = mmkv.decodeString(uuid, null);
            ComputerDetails details = computerFromJson(jsonStr);
            if (details != null && details.uuid != null) {
                computers.add(details);
            }
        }

        return computers;
    }

    public ComputerDetails getComputerByName(String name) {
        List<ComputerDetails> computers = getAllComputers();
        for (ComputerDetails computer : computers) {
            if (name.equals(computer.name)) {
                return computer;
            }
        }
        return null;
    }

    public ComputerDetails getComputerByUUID(String uuid) {
        String jsonStr = mmkv.decodeString(uuid, null);
        return computerFromJson(jsonStr);
    }
}
