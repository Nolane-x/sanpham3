# F5 Dual-AVD Wi-Fi Direct Authenticated Pair Court

Android Emulator networking now exposes P2P / Wi-Fi Direct on Emulator 36.5+
images.

This court uses that capability as Android-framework evidence.

It is not physical-radio evidence.

## Useful path

The court exercises:

```text
AVD A
  -> WifiP2pManager.createGroup()
  -> group owner

AVD B
  -> WifiP2pManager.discoverPeers()
  -> discovered AVD A
  -> WifiP2pManager.connect(groupOwnerIntent=0)

Wi-Fi Direct group
  -> TCP socket
  -> shared Rust AndroidPeerSession
  -> authenticated peer node IDs
  -> encrypted G8 challenge
  -> encrypted G8 ACK
```

A framework connection callback without the authenticated G8 exchange is not a
PASS.

## Recovery Lab activity

`WifiDirectPairCourtActivity` supports two roles.

### Owner

The owner:

1. records the `android.hardware.wifi.direct` feature flag for diagnostics;
2. verifies the current peer-LAN runtime permissions;
3. relies on the orchestrator's live `wifip2p` service gate;
4. creates a Wi-Fi Direct group;
5. requires itself to be group owner;
6. accepts a TCP connection through `AndroidWifiDirectDataPath`;
7. upgrades it to `AndroidPeerSession.server`;
8. validates and echoes the encrypted G8 challenge.

### Client

The client:

1. starts Wi-Fi Direct peer discovery;
2. chooses the discovered peer hint;
3. connects with group-owner intent 0 because the court owner already created
   the group;
4. requires itself to remain the group client;
5. connects to the group-owner address;
6. upgrades the TCP socket to `AndroidPeerSession.client`;
7. sends the encrypted G8 challenge and requires the exact authenticated ACK.

Wi-Fi Direct MAC/device hints are never project identity.

The Rust peer-session node ID is the authenticated project identity.

## Orchestrator

Run against two already-booted AVDs:

```bash
bash scripts/virtual-phone-avd-wifi-direct.sh \
  emulator-5554 \
  emulator-5556 \
  dev.nolane.sanpham3.recoverylab
```

The script requires:

- Android Emulator 36.5+;
- both AVDs online;
- Recovery Lab installed;
- Wi-Fi enabled;
- Location Mode enabled because `discoverPeers()` / `requestPeers()`
  require it;
- a live `wifip2p` Android system service on both AVDs;
- required runtime permissions granted;
- owner PASS;
- client PASS;
- owner authenticates client node 100;
- client authenticates owner node 200;
- exact same G8 challenge on both sides.

## Real CI

`.github/workflows/f5-avd-wifi-direct-pair.yml`:

1. installs the current Android 17 / API 37.0 x86_64 system image;
2. installs the latest Emulator package;
3. rejects Emulator versions below 36.5;
4. builds Recovery Lab and Rust JNI;
5. boots two independent AVDs;
6. installs the APK on both;
7. runs the strict Wi-Fi Direct court;
8. uploads the AVD evidence bundle.

## Evidence

The court captures:

- emulator version;
- AVD API levels;
- feature lists, including whether the optional Wi-Fi Direct feature flag is
  advertised;
- live `wifip2p` service status;
- Location Mode state;
- IP addresses/routes;
- Wi-Fi dumps;
- Wi-Fi P2P dumps;
- connectivity dumps;
- tagged court logcat;
- authenticated local/peer node IDs;
- G8 challenge;
- git commit;
- SHA-256 manifest.

Evidence is labeled:

```text
ANDROID_AVD
```

## Promotion boundary

A green court closes the F5 **emulator Wi-Fi Direct pair court** only.

It does not close:

- physical Wi-Fi Direct interoperability;
- real radio range;
- real setup latency;
- useful physical throughput;
- coexistence with OEM Wi-Fi/mobile-data behavior;
- physical energy measurements.

Those remain F7/F6 physical evidence requirements.


## Framework permission requirements

Recovery Lab declares the normal Wi-Fi framework permissions used by the
official Wi-Fi Direct API flow:

- `ACCESS_WIFI_STATE`;
- `CHANGE_WIFI_STATE`;
- `CHANGE_NETWORK_STATE`;
- `INTERNET`;
- `ACCESS_NETWORK_STATE`.

For API 33+ it also uses `NEARBY_WIFI_DEVICES`; Android 17 local-network
access follows the separate Recovery Lab permission framework.

The AVD orchestrator explicitly enables Location Mode before discovery. This is
required by Android's Wi-Fi Direct peer-discovery APIs even when
`NEARBY_WIFI_DEVICES` is used.


## System join approval in the AVD court

When an already-created group owner receives a new peer join request, Android
enters a user-authorization state and shows a system Wi-Fi Direct invitation
dialog.

The application does not receive ordinary permission to silently accept that
request. The framework API for programmatic connection-request decisions
requires privileged Wi-Fi network-selection authority.

For deterministic CI only, the host orchestrator therefore:

1. waits for the owner system dialog;
2. dumps the active UI hierarchy with UiAutomator;
3. locates the positive/Accept control;
4. taps it through ADB input;
5. stores the UI XML and approval coordinates in the evidence bundle;
6. records `join_approval=AVD_UI_AUTOMATION`.

This models a user approving the connection. It is not a product bypass and
does not change the requirement for user/framework authorization on real
devices.
