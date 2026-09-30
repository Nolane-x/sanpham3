# Android 17 Local-Network Framework Integration

This work moves the Android 17 local-network requirement from the deterministic
F2 model into the Android Recovery Lab and android-host build.

It is still not physical carrier proof.

## Build/target boundary

The Android host adapter and Recovery Lab now compile against API 37.

The Recovery Lab itself targets API 37 so Android 17 local-network enforcement
is exercised under the target-SDK boundary documented by Android.

CI installs:

```text
platforms;android-37
build-tools;36.0.0
```

The build-tools version is independent from the compile SDK platform package.

## Manifest boundary

Only the Recovery Lab declares:

```xml
<uses-permission
    android:name="android.permission.ACCESS_LOCAL_NETWORK" />
```

The reusable `android-host` library does not silently add this dangerous
runtime permission to every embedding application.

Production apps must make their own explicit manifest/runtime UX decision.

## Feature report

`AndroidFeatureReport` now exposes:

```text
accessLocalNetworkPermission: Boolean?
```

Meaning:

- `null`: OS API < 37, permission not applicable;
- `true`: API 37+ and granted;
- `false`: API 37+ and denied/not granted.

This is separate from `nearbyWifiPermission`.

## Recovery Lab permission policy

`RecoveryLabPermissions` centralizes API-level requirements.

BLE:

- API <31: location compatibility permission;
- API 31+: Bluetooth scan/advertise/connect.

Peer LAN:

- API <33: location compatibility permission;
- API 33-36: `NEARBY_WIFI_DEVICES`;
- API 37+: `NEARBY_WIFI_DEVICES` plus
  `ACCESS_LOCAL_NETWORK`.

The main lab permission button requests the union without duplicates.

Unit tests lock this matrix.

## Hotspot and G9 courts

Local-Only Hotspot G8 and peer-egress G9 now fail before opening their peer-LAN
socket path unless every required peer-LAN permission is granted.

On API 37 this includes `ACCESS_LOCAL_NETWORK`.

Evidence/transcript output includes the initial local-network permission state.

This avoids misclassifying an Android 17 permission denial as an unexplained TCP
timeout or carrier failure.

## Exact-Network AVD court

The exact-Network DNS court records:

```text
local_network_permission=<granted|denied|not_applicable>
dns_port53_exception=true
```

The DNS probe itself must not be used as proof that broad LAN permission is
working, because Android's Local Network Protection documentation exempts
traffic to a configured local DNS server on port 53.

For API 37+ AVD runs, the shell court explicitly grants
`android.permission.ACCESS_LOCAL_NETWORK` before launching the activity and
requires the activity to report `granted`.

It also captures `dumpsys package` in the evidence set.

## Official references

- https://developer.android.com/about/versions/17/behavior-changes-17
- https://developer.android.com/privacy-and-security/local-network-permission
- https://developer.android.com/reference/android/Manifest.permission#ACCESS_LOCAL_NETWORK
- https://developer.android.com/about/versions/16/behavior-changes-16

## Evidence boundary

This closes a framework-integration baseline for:

- compiling API-37 permission constants;
- targeting API 37 in Recovery Lab;
- explicit Recovery Lab manifest declaration;
- runtime permission request matrix;
- feature-report visibility;
- peer-LAN preflight;
- AVD permission grant/evidence plumbing.

It does not yet close:

- user denial/revocation court on a real Android 17 device;
- Local-Only Hotspot physical interoperability under API 37;
- Wi-Fi Direct/Aware physical behavior under API 37;
- system-mediated picker integration;
- production permission UX.

Those remain higher-level Android/physical evidence.
