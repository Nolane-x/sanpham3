# F5 Automated AVD Scenario Driver

The existing twin-AVD harness launches two Android emulators, applies network
speed/latency constraints, captures packet traces and framework snapshots.

This baseline adds an automated application scenario driver for already
running AVDs.

## Command

```bash
bash scripts/virtual-phone-avd-scenarios.sh \
  emulator-5554 \
  emulator-5556 \
  <android.package.name> \
  [evidence_dir]
```

The package must already be installed on both devices.

## Permission state machine

The driver does not assume that a permission begins granted.

For each configured runtime permission and each AVD it:

1. reads the current package permission state from `dumpsys package`;
2. skips permissions that are not requested/observable;
3. selects the opposite state;
4. performs `pm grant` or `pm revoke`;
5. verifies the state actually changed;
6. force-stops and launches the application;
7. captures package/connectivity/route/logcat evidence;
8. restores the exact original granted/denied state;
9. verifies restoration;
10. captures the restored-state evidence.

This means a test beginning denied is restored denied, while one beginning
granted is restored granted.

## Launch behavior

By default the app is launched through:

```text
monkey -p <package> -c android.intent.category.LAUNCHER 1
```

A precise activity can be supplied with:

```bash
SP3_LAUNCH_COMPONENT=com.example/.MainActivity
```

## Permission set

The default research set is:

- `ACCESS_FINE_LOCATION`;
- `BLUETOOTH_SCAN`;
- `BLUETOOTH_CONNECT`;
- `NEARBY_WIFI_DEVICES`.

Override it with the space-separated
`SP3_SCENARIO_PERMISSIONS` environment variable.

Older API levels or apps that do not request a listed permission are recorded
as typed skips rather than false failures.

By default the driver fails when zero permissions can be exercised. This can
be disabled with `SP3_REQUIRE_PERMISSION_EXERCISE=0` for snapshot-only runs.

## Evidence

The driver writes:

- scenario result ledger;
- package snapshots;
- connectivity snapshots;
- route snapshots;
- recent logcat;
- timestamp;
- tested git commit;
- AVD serials;
- package;
- permission set;
- exercised/skipped/failure counts;
- SHA-256 manifest.

A successful execution prints:

```text
F5_AVD_SCENARIO_PASS ... restored=true
```

Evidence generated against real Android emulators is typed `ANDROID_AVD`.

## CI court

GitHub CI cannot substitute for Android Emulator framework behavior.

The dedicated workflow therefore uses a fake `adb` implementation only to
test the driver's own state machine.

The mock court deliberately starts:

- emulator-5554 with the permission granted;
- emulator-5556 with the permission denied.

It requires both devices to transition to the opposite state and return to
their original states, while producing the expected evidence ledger and hash
manifest.

That CI result proves driver logic only. It is not Android-framework evidence.

## Gate boundary

This closes software baselines for:

- automated app scenario driving;
- automated permission revoke/restore.

It does not close:

- emulator Wi-Fi Direct interoperability;
- exact-Network probing across two AVDs;
- camera video-source optical replay;
- Doze/background/restart behavior;
- physical-device behavior.

Those remain separate F5 courts.
