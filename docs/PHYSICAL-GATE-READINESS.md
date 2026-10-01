# Physical Gate Readiness Report

The remaining frontier gates require real-device evidence.

`scripts/physical-gate-readiness.py` checks whether an evidence bundle contains
the conservative prerequisites for those gates.

It never edits `FRONTIER-RESEARCH-GATES.md` and it never promotes evidence.

## Run

```bash
python scripts/physical-gate-readiness.py evidence/android-physical-campaign-...
```

JSON:

```bash
python scripts/physical-gate-readiness.py --json evidence/android-physical-campaign-...
```

The Android campaign wrappers expose the same report:

```bash
scripts/android-physical-campaign.sh readiness <evidence_dir>
```

```powershell
pwsh scripts/android-physical-campaign.ps1 readiness <evidence_dir>
```

`collect` also writes:

```text
gate-readiness.txt
gate-readiness.json
```

automatically.

## Physical-device proof

A campaign counts a target as a physical-device candidate only when evidence
contains:

```text
serial=<adb serial>
qemu=<value other than 1>
```

At least two distinct physical serials are required before any two-device
carrier is reported as interoperability-candidate-ready.

An emulator allowed through `SP3_ALLOW_NON_PHYSICAL=1` therefore remains
visible in the campaign but cannot satisfy the physical-device count.

## Recorded trace gates

The acoustic trace candidate requires all of:

- `mode=audio`;
- `evidence_level=ANDROID_RUNTIME_CAPTURE`;
- a physical serial;
- known `expected_hex`;
- `replay.txt` containing `F3_ACOUSTIC_REPLAY`;
- `bit_errors=0`.

The vibration trace candidate uses the same rules with:

```text
mode=accelerometer
F3_VIBRATION_REPLAY
bit_errors=0
```

This still does not prove the experiment geometry or transmitter setup.

The original capture, metadata, SHA-256 manifest and experiment notes remain
part of the evidence review.

## Carrier interoperability

The current role pairs are:

| Carrier | Required PASS roles |
| --- | --- |
| NFC HCE | `reader`, `hce` |
| BLE GATT | `client`, `server` |
| RFCOMM | `client`, `server` |
| Local-Only Hotspot | `client`, `server` |

The report requires at least one PASS on every required role plus two distinct
physical-device candidates before it marks interoperability as
candidate-ready.

## Core measurement fields

The report also checks app-measured fields.

### NFC HCE reader

Required:

```text
max_transceive_length
benchmark_rounds
benchmark_payload_bytes
benchmark_elapsed_ns
round_trip_useful_bps
```

### BLE GATT client

Required:

```text
g8_plus_benchmark_total_ms
benchmark_rounds
benchmark_payload_bytes
benchmark_elapsed_ns
benchmark_rtt_median_ns
benchmark_rtt_p95_ns
benchmark_one_way_useful_bps
```

### RFCOMM client

Required:

```text
benchmark_rounds
benchmark_payload_bytes
benchmark_elapsed_ns
benchmark_rtt_median_ns
benchmark_rtt_p95_ns
benchmark_one_way_useful_bps
```

### Local-Only Hotspot

Server:

```text
hotspot_startup_ms
benchmark_rounds
benchmark_payload_bytes
```

Client:

```text
network_join_ms
g8_ms
benchmark_rounds
benchmark_payload_bytes
benchmark_elapsed_ns
benchmark_rtt_median_ns
benchmark_rtt_p95_ns
benchmark_one_way_useful_bps
```

## Repeated failure-rate evidence

NFC, GATT and RFCOMM remaining gates ask for failure/reliability evidence.

A single PASS is not treated as a failure-rate measurement.

The readiness engine requires at least two recorded outcomes for every required
role before the `failure_rate` prerequisite is considered present.

Those outcomes may be PASS or FAIL.

The report does not choose how many repetitions are scientifically sufficient;
it only prevents a one-shot run from being mislabeled as a failure-rate study.

## External physical observations

Some required quantities cannot be inferred from an Android peer-session
benchmark.

They may be added as ordinary key-value evidence records.

Every such file should include:

```text
timestamp_utc=<ISO-8601>
carrier=<carrier key>
role=observation
result=PASS
method=<measurement method>
operator_note=<short description>
```

Use the same carrier keys as the courts:

```text
nfc_hce
ble_gatt
rfcomm
local_only_hotspot
```

Additional fields currently recognized by gate readiness are:

```text
setup_latency_ms=<measured value>
range_m=<measured value>
energy_joules=<measured value>
energy_method=<meter / battery-counter / external instrument method>
concurrent_internet=<true|false plus context in operator_note>
```

Requirements match the open ledger:

- NFC: repeated failure-rate evidence;
- GATT: setup latency, repeated failures and energy;
- RFCOMM: setup latency, repeated reliability, range and energy;
- Local-Only Hotspot: concurrent-Internet behavior and energy.

Do not invent values to make readiness green.

If a quantity was not measured, leave it missing.

## Meaning of the three readiness levels

### interoperability_candidate_ready

Two physical-device candidates exist and every required court role has a PASS.

### measurement_core_candidate_ready

Interoperability is candidate-ready and the transport court contains its
required app-measured timing/throughput fields.

### full_measurement_candidate_ready

Core measurement is candidate-ready and all remaining external physical
requirements visible to the readiness engine are also present.

Even `full_measurement_candidate_ready=true` is not an automatic frontier
closure.

A human still reviews:

- original court logs;
- device identity;
- topology/setup;
- experiment geometry;
- SHA-256 manifests;
- measurement method;
- repeated-run quality;
- whether the evidence actually matches the frontier claim.

## Self-test

```bash
python scripts/physical-gate-readiness.py --self-test
```

The deterministic self-test includes two non-QEMU device metadata records, an
RFComm two-role PASS pair and a zero-BER acoustic trace.

It also verifies that missing range/energy observations keep the full RFComm
measurement candidate false.
