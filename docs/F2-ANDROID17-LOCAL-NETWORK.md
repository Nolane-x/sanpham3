# F2 Android 17 Local-Network Permission Model

Android 17 changes a key assumption for sanpham3 peer-LAN recovery paths:
`INTERNET` permission alone is no longer sufficient for broad local-network
access when the application targets Android 17 / API 37 or higher.

Official references:

- https://developer.android.com/about/versions/17/behavior-changes-17
- https://developer.android.com/privacy-and-security/local-network-permission
- https://developer.android.com/reference/android/Manifest.permission#ACCESS_LOCAL_NETWORK
- https://developer.android.com/about/versions/16/behavior-changes-16

## Android 17 enforcement

For:

```text
OS API >= 37
AND targetSdk >= 37
```

the model requires one of two distinct paths:

1. broad runtime `ACCESS_LOCAL_NETWORK` permission;
2. a privacy-preserving system-mediated selected-device path.

These are intentionally not equivalent.

### Broad permission

When `ACCESS_LOCAL_NETWORK` is granted:

- arbitrary app-owned LAN sockets are allowed by this F2 model;
- current sanpham3 peer-LAN paths can remain eligible, subject to all their
  other hardware/API/permission/runtime gates.

### System-mediated picker

A system-mediated picker grants a path to a user-selected device/use case
without granting broad arbitrary LAN access.

The model therefore exposes:

```text
allows_selected_device_lan = true
allows_arbitrary_lan       = false
```

Current sanpham3 Wi-Fi Direct / Wi-Fi Aware / Local-Only Hotspot data paths use
app-owned peer LAN sockets and do not yet integrate such a picker.

Therefore picker-only state does **not** keep those current arbitrary peer
socket paths enabled.

A future picker-backed adapter can use the selected-device capability
separately rather than turning broad LAN access on.

## Denied/revoked permission

For API/target 37+ with no broad permission and no selected-device picker path:

```text
arbitrary LAN       = blocked
selected-device LAN = blocked
ordinary Internet   = unaffected by this permission model
```

The project capability reduction disables the current LAN-socket-based:

- Wi-Fi Direct peer data path;
- Wi-Fi Aware peer data path;
- Local-Only Hotspot peer data path.

It does not disable unrelated Bluetooth, NFC, USB, SMS or Internet paths.

## Target SDK compatibility

On Android 17 with:

```text
targetSdk <= 36
```

the model keeps the legacy implicit LAN behavior.

This follows Android's published enforcement boundary for apps targeting API 37
or higher.

## Android 16 opt-in phase

Android 16 / API 36 provided an opt-in compatibility phase.

The model represents this separately with:

```text
android16_restrict_local_network_opt_in
nearby_wifi_devices_granted
```

When opt-in restriction is active:

- denied `NEARBY_WIFI_DEVICES` -> arbitrary LAN denied;
- granted `NEARBY_WIFI_DEVICES` -> arbitrary LAN restored.

This compatibility branch must not be confused with Android 17's final
`ACCESS_LOCAL_NETWORK` permission.

## Model API

`AndroidLocalNetworkPolicy` reports one of:

- `LegacyUnrestricted`;
- `AccessLocalNetworkGranted`;
- `Android16OptInGranted`;
- `SystemMediatedSelectedDevice`;
- `Denied`.

It also reports independently:

- `allows_arbitrary_lan()`;
- `allows_selected_device_lan()`;
- `allows_internet()`.

## Court

Run:

```bash
cargo run -p virtual-phone-lab -- android-local-network-matrix
```

The deterministic court requires:

- Android 17 + target 37 + denied permission blocks broad LAN;
- ordinary Internet remains allowed by this permission model;
- explicit permission restores broad LAN;
- system-mediated selected-device state does not become broad LAN;
- target 36 on Android 17 keeps legacy behavior;
- Android 16 opt-in denied/granted behavior remains a separate compatibility
  branch.

The court runs on Linux and Windows CI because it is a platform policy model,
not Android framework evidence.

## Evidence boundary

This closes the F2 **Android 17 local-network-permission model** gate.

It does not by itself prove:

- the host application's API-37 manifest declaration;
- runtime permission prompt/revocation UX;
- real Android 17 socket failures;
- picker integration;
- physical Local-Only Hotspot/Wi-Fi Direct/Wi-Fi Aware behavior.

Those require Android app/framework and physical courts.

