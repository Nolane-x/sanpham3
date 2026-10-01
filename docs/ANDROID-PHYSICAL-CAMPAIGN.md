# Android Two-Device Physical Campaign Harness

This harness standardizes real two-device Recovery Lab campaigns.

Both entry points implement the same campaign model:

```text
scripts/android-physical-campaign.sh
scripts/android-physical-campaign.ps1
```

Use the Bash entry point on Linux/macOS/WSL/Git Bash and the PowerShell entry
point on native Windows.

It does **not** automate the physical interaction itself and it does not promote
evidence to `PHYSICAL_DEVICE` on its own.

That boundary is intentional.

## Commands

### Prepare two devices

```bash
scripts/android-physical-campaign.sh \
  prepare \
  <serial_a> \
  <serial_b> \
  dev.nolane.sanpham3.recoverylab \
  [recovery-lab.apk] \
  [evidence_dir]
```

Preparation:

- requires two distinct online ADB serials;
- rejects emulator/qemu devices by default;
- optionally installs the same APK on both phones;
- verifies the package exists;
- optionally grants declared runtime permissions;
- optionally enables Wi-Fi/Bluetooth/location best-effort;
- records full device/build provenance;
- captures features, battery, Bluetooth, NFC, Wi-Fi, connectivity, routes and
  logcat;
- writes a campaign metadata file;
- hashes all preparation evidence.

Environment controls:

```text
SP3_ALLOW_NON_PHYSICAL=1
SP3_GRANT_RUNTIME_PERMISSIONS=1
SP3_ENABLE_RADIOS=1
SP3_CAMPAIGN_ID=<stable-id>
SP3_ADB_BIN=<adb>
```

`SP3_ALLOW_NON_PHYSICAL=1` exists for harness testing only. Evidence produced
from such a device is still not physical evidence.



### Native Windows equivalent

```powershell
pwsh scripts/android-physical-campaign.ps1 \
  prepare \
  <serial_a> \
  <serial_b> \
  dev.nolane.sanpham3.recoverylab \
  [recovery-lab.apk] \
  [evidence_dir]
```

`launch`, `collect` and `summarize` use the same arguments as the Bash
entry point.

## Open a court

```bash
scripts/android-physical-campaign.sh \
  launch <serial> dev.nolane.sanpham3.recoverylab <carrier> [evidence_dir]
```

Supported carrier names:

```text
gatt
rfcomm
nfc
hotspot
```

The script launches the matching Recovery Lab activity.

It deliberately does not:

- fake NFC tap/proximity;
- choose a BLE/RFCOMM peer on behalf of the operator;
- copy Local-Only Hotspot credentials through logs;
- bypass Android permission UI;
- synthesize range/energy observations.

The operator performs the real physical interaction in the app.

## Capture acoustic/vibration traces inside the campaign

Bash:

```bash
scripts/android-physical-campaign.sh \
  trace \
  <serial> \
  dev.nolane.sanpham3.recoverylab \
  <audio|accelerometer> \
  [duration_ms] \
  [evidence_dir]
```

Native Windows:

```powershell
pwsh scripts/android-physical-campaign.ps1 \
  trace \
  <serial> \
  dev.nolane.sanpham3.recoverylab \
  <audio|accelerometer> \
  [duration_ms] \
  [evidence_dir]
```

The campaign wrapper:

- rejects QEMU/emulator targets by default;
- invokes the existing recorded-trace capture court;
- stores WAV/CSV evidence under the same campaign root;
- preserves `ANDROID_RUNTIME_CAPTURE` on the original trace metadata;
- adds only a `CANDIDATE_PHYSICAL_TRACE` wrapper identifying the non-QEMU
  target;
- runs exact replay automatically when
  `SP3_TRACE_EXPECTED_HEX` is supplied;
- refreshes the readiness report and SHA-256 manifest.

Optional replay controls:

```text
SP3_TRACE_EXPECTED_HEX=<known project payload>
SP3_TRACE_START_SAMPLE=<sample offset, default 0>
SP3_TRACE_TIMEOUT=<poll timeout seconds>
```

The wrapper does not turn an arbitrary microphone/sensor recording into a
physical frontier PASS.

## Collect evidence

After completing one or more courts:

```bash
scripts/android-physical-campaign.sh \
  collect \
  <serial_a> \
  <serial_b> \
  dev.nolane.sanpham3.recoverylab \
  <same-evidence-dir>
```

Collection:

1. re-snapshots both devices;
2. pulls Recovery Lab evidence files from both devices;
3. falls back to `run-as` for the debug app when direct external-files pull
   is unavailable;
4. runs `scripts/summarize-physical-evidence.py`;
5. runs `scripts/physical-gate-readiness.py`;
6. writes text/JSON aggregate summaries and gate-readiness reports;
7. creates a recursive `SHA256SUMS` manifest.

## Repeated-run workflow

For a physical carrier campaign, use a stable campaign ID:

```bash
export SP3_CAMPAIGN_ID=gatt-phoneA-phoneB-20261001
```

Then:

1. `prepare`;
2. launch the court on both devices;
3. complete the physical interaction;
4. repeat the court enough times to characterize failure rate;
5. `collect`;
6. review the original evidence files and aggregate summary.

The existing physical evidence aggregator groups records by carrier/role and
reports PASS/FAIL count, success rate and timing/throughput statistics present
in valid PASS records.

## Gate-readiness report

Run independently:

```bash
scripts/android-physical-campaign.sh readiness <evidence_dir>
```

or:

```powershell
pwsh scripts/android-physical-campaign.ps1 readiness <evidence_dir>
```

The report checks candidate prerequisites for the ten remaining physical gates,
including:

- two distinct non-QEMU devices;
- required client/server/reader/HCE PASS roles;
- timing/throughput fields emitted by the carrier court;
- repeated outcome evidence where failure rate is required;
- zero-BER recorded acoustic/vibration replay;
- external range/energy/concurrent-Internet observations where the ledger
  requires them.

See `docs/PHYSICAL-GATE-READINESS.md`.

The report never edits or closes the frontier ledger.

## Provenance boundary

The harness writes:

```text
evidence_level=CANDIDATE_PHYSICAL_CAMPAIGN
```

at campaign preparation time.

That label is not a PASS.

A physical frontier gate closes only after the carrier-specific evidence proves
the required interoperability and measurements.

The harness never invents:

- physical range;
- energy;
- RSSI;
- setup latency not measured by the carrier court;
- concurrent-Internet behavior;
- physical failure rate from a single run.

## Emulator refusal

By default the harness checks `ro.kernel.qemu` and refuses any emulator.

This prevents an AVD run from accidentally entering the physical evidence
folder just because it was reachable through ADB.

## CI

```bash
bash -n scripts/android-physical-campaign.sh
scripts/android-physical-campaign.sh --self-test
```

```powershell
pwsh scripts/android-physical-campaign.ps1 --self-test
```

The self-test validates carrier/activity routing and filename sanitization only.

CI cannot prove a physical campaign.
