# Android Recovery Lab

This is a deliberately small physical-court APK for the sanpham3 project.

It is not the end-user product UI.

Its purpose is to let two real Android devices exercise the project's Android
carrier stack without writing a separate test application.

## Current court

The app currently exposes the Android BLE L2CAP G8 court:

```text
BLE discovery
-> BLE L2CAP CoC
-> shared Rust peer-session JNI
-> authenticated project node ID
-> encrypted G8 challenge
-> encrypted byte-identical ACK
```

A successful run prints `G8_BLE_PASS` on both devices.

## Requirements

- Android 10 / API 29 or newer;
- Bluetooth LE hardware;
- Bluetooth enabled;
- required runtime Bluetooth permissions;
- two physical Android devices for a physical G8 result.

The app feature report exposes API/hardware capability hints, but only a
successful listen/connect/handshake is physical proof.

## Build

CI builds the Rust peer-session JNI libraries for:

- arm64-v8a;
- x86_64.

It then builds the debug APK and verifies both native libraries are packaged.

Workflow:

```text
.github/workflows/android-recovery-lab.yml
```

The workflow publishes an artifact named:

```text
sanpham3-android-recovery-lab-debug
```

## Two-device BLE G8 procedure

Use the same 64-hex laboratory PSK on both devices.

Recommended node IDs:

```text
server = 200
client = 100
```

### Device B — server

1. Open Recovery Lab.
2. Set node ID to `200`.
3. Paste the shared PSK.
4. Tap **Request BLE permissions**.
5. Tap **Show feature report**.
6. Tap **BLE G8 Server**.
7. Keep the app open.

Expected intermediate evidence includes:

```text
BLE_SERVER_LISTEN
BLE_SERVER_AUTH
```

### Device A — client

1. Open Recovery Lab.
2. Set node ID to `100`.
3. Paste the same PSK.
4. Tap **Request BLE permissions**.
5. Tap **Show feature report**.
6. Tap **BLE G8 Client**.

The client scans only; the server advertises only.

## PASS rule

Both devices must print:

```text
G8_BLE_PASS
```

The evidence must agree on:

```text
server local_node = 200
server peer_node  = 100

client local_node = 100
client peer_node  = 200

client challenge == server challenge
```

Record:

- UTC time;
- git commit / APK hash;
- Android version;
- device models;
- feature report;
- server PSM;
- client discovery RSSI;
- both `G8_BLE_PASS` lines;
- whether Wi-Fi and cellular were disabled.

## Stop behavior

**Stop current operation** closes the active peer session, BLE discovery and
L2CAP server.

Closing the Activity also releases those resources.

## Security boundary

The PSK field is for laboratory pairing only.

BLE address, RSSI and PSM are routing hints. The authenticated project node ID
comes from the shared Rust peer-session handshake.

This app intentionally does not introduce a Kotlin cryptographic protocol.

## Evidence boundary

A successful CI APK build is software readiness.

A physical G8 pair closes only after two real devices complete the court and
matching evidence is preserved.
