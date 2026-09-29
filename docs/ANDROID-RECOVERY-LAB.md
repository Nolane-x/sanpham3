# Android Recovery Lab

Android Recovery Lab is a deliberately small installable APK for physical G8
validation.

It is not the final product UI.

Its job is to make real-device evidence easy to reproduce.

## Current court

The first lab court is:

```text
Android A
  BLE discovery
  -> BLE L2CAP CoC
  -> shared Rust peer-session
  -> encrypted G8 challenge
          |
          v
Android B
  encrypted G8 ACK
```

The court does not depend on a project cloud service.

## Build in CI

Workflow:

```text
android-recovery-lab
```

The workflow:

1. builds the shared Rust peer-session JNI library;
2. cross-compiles arm64-v8a and x86_64;
3. runs lab unit tests;
4. assembles the debug APK;
5. verifies both JNI libraries are inside the APK;
6. writes the APK SHA-256;
7. uploads artifact `sp3-android-recovery-lab`.

## Local build

Requirements:

- Android SDK platform 36;
- Android NDK;
- Rust;
- cargo-ndk;
- Gradle 9.6 compatible environment.

Build the JNI library first:

```bash
export ANDROID_NDK_HOME=<ndk_path>
bash adapters/android-host/build-peer-session-native.sh
```

Then:

```bash
gradle -p apps/android-recovery-lab :app:assembleDebug
```

## Install

Example:

```bash
adb install -r apps/android-recovery-lab/app/build/outputs/apk/debug/app-debug.apk
```

Use two physical Android devices.

Recommended IDs:

```text
client A = 100
server B = 200
```

Do not use the same project node ID on both devices. The lab rejects that case.

## Prepare both devices

1. Open **SP3 Recovery Lab**.
2. Tap **Grant BLE permissions**.
3. Tap **Scan capabilities**.
4. Confirm:
   - BLE hardware is present;
   - BLE L2CAP CoC API support is reported;
   - scan/advertise/connect permissions are granted.
5. Generate one laboratory PSK on one device.
6. Temporarily show it and enter the same 64-hex PSK on the second device.

The lab never writes the PSK into evidence files.

## Server B

Set node ID:

```text
200
```

Tap:

```text
Start BLE G8 server
```

Expected log progression:

```text
BLE_L2CAP listening ...
BLE_DISCOVERY advertising=true scanning=true
G8_SERVER_WAIT ...
...
G8_PASS role=server ...
```

The app advertises the 7-byte L2CAP endpoint:

```text
SP3L || version(0) || psm(u16)
```

## Client A

Set node ID:

```text
100
```

Tap:

```text
Start BLE G8 client
```

Expected progression:

```text
BLE_DISCOVERY client_scan_started=true
BLE_L2CAP_CANDIDATE ...
...
G8_PASS role=client ...
```

## PASS rule

Both devices must report:

- authenticated peer project node ID;
- carrier = `ble_l2cap`;
- matching challenge hex;
- result = PASS.

Expected identity relation:

```text
client local=100 peer=200
server local=200 peer=100
```

BLE addresses are not trusted identities.

## Evidence files

The app automatically saves each PASS/FAIL record twice:

1. in its app-specific external files directory:

```text
.../files/evidence/
```

2. in public Downloads using MediaStore:

```text
Downloads/SP3-Recovery-Lab/
```

The Downloads copy requires no broad storage permission and can be retrieved
directly through the device's Files app.

Each evidence file includes:

- UTC timestamp;
- git commit embedded at APK build time;
- Android manufacturer/model;
- Android SDK;
- local node ID;
- role;
- carrier;
- PSM;
- RSSI where available;
- authenticated peer node ID;
- G8 challenge;
- PASS/FAIL;
- complete lab transcript.

It never includes the PSK.

## Pure BLE experiment

For the strongest BLE-specific evidence:

- disable Wi-Fi;
- disable cellular/mobile data if practical;
- leave Bluetooth enabled;
- run the same G8 court.

This demonstrates the project byte stream and authenticated session are carried
over BLE L2CAP rather than an ordinary Internet path.

It still does not prove Internet recovery. That belongs to G9.

## Next lab expansion

After BLE G8 is physically proven, this APK should grow in this order:

1. Wi-Fi Direct G8 pair court;
2. Wi-Fi Aware G8 pair court;
3. exact-Network path probe view;
4. peer egress G4;
5. G9 failed-default -> peer-rescue court;
6. exportable evidence bundle.
