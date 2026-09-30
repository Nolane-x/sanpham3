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

1. verifies `android.hardware.wifi.direct`;
2. verifies the current peer-LAN runtime permissions;
3. creates a Wi-Fi Direct group;
4. requires itself to be group owner;
5. accepts a TCP connection through `AndroidWifiDirectDataPath`;
6. upgrades it to `AndroidPeerSession.server`;
7. validates and echoes the encrypted G8 challenge.

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
- `android.hardware.wifi.direct` on both;
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
- feature lists;
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
