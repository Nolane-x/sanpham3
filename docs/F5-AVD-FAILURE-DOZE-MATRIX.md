# F5 Dual-AVD Restart / Doze Failure Matrix

This driver extends the Android AVD lab beyond permission transitions.

It exercises two state classes that matter during recovery:

- application process restart;
- forced deep Doze followed by restoration to the original device-idle state.

## Driver

Run:

```bash
bash scripts/virtual-phone-avd-failure-matrix.sh \
  emulator-5554 emulator-5556 dev.nolane.sanpham3.recoverylab
```

Each target must be an online adb device with the package installed.

## Restart scenario

For each AVD the driver:

1. launches the app;
2. captures baseline process/connectivity/device-idle evidence;
3. force-stops the package;
4. captures stopped-state evidence;
5. relaunches the app;
6. captures post-restart evidence.

A successful command/state-machine transition is recorded as:

```text
PASS ... scenario=app_restart
```

The current baseline proves orchestration and evidence capture. It does not yet
assert application-specific recovery invariants after restart.

## Deep Doze scenario

The driver reads:

```text
adb shell dumpsys deviceidle get deep
```

If the device reports `ACTIVE` or `IDLE`, it:

1. records the original state;
2. forces deep idle;
3. requires the reported state to become `IDLE`;
4. captures evidence;
5. restores the original state;
6. verifies the final state equals the original state;
7. captures post-restore evidence.

If device-idle control is unavailable, the scenario is explicitly `SKIP`
unless `SP3_REQUIRE_DOZE=1`.

## Evidence

The driver captures per device and phase:

- package state;
- ActivityManager process snapshot;
- connectivity snapshot;
- device-idle snapshot;
- recent logcat.

It also writes:

- git commit;
- AVD serials;
- package;
- restart/doze pass/skip/fail counts;
- evidence level;
- SHA-256 manifest over the evidence set.

## Evidence typing

Real AVD runs default to:

```text
evidence_level=ANDROID_AVD
```

The CI state-machine court uses a fake adb implementation and is required to
override this as:

```text
evidence_level=MODEL_ONLY_FAKE_ADB
```

Therefore CI cannot be cited as Android framework evidence.

## CI court

The fake-adb court verifies:

- both virtual device identities are exercised;
- app restart completes on both;
- deep Doze is forced on both;
- both devices are restored to their original `ACTIVE` state;
- modeled evidence is typed correctly;
- evidence hashes are emitted.

PASS begins with:

```text
F5_AVD_FAILURE_MATRIX_PASS
```

## Boundary

This closes a restart/Doze **driver/state-machine baseline**.

It does not yet close:

- real-emulator app semantic assertions after restart;
- socket/session survival expectations;
- background execution behavior under OEM power policy;
- physical-device Doze behavior;
- radio recovery during Doze;
- battery/energy measurements.
