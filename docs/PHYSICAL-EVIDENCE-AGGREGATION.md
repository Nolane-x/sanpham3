# Physical Evidence Aggregation

The Android Recovery Lab physical courts save deterministic `key=value`
evidence files before the transcript section.

`scripts/summarize-physical-evidence.py` aggregates repeated runs without
changing their provenance.

## Usage

```bash
python scripts/summarize-physical-evidence.py evidence/
```

JSON output:

```bash
python scripts/summarize-physical-evidence.py --json evidence/ > summary.json
```

The script recursively reads `.txt` evidence and groups records by:

```text
carrier + role
```

For each group it reports:

- record count;
- PASS count;
- FAIL count;
- success rate;
- min / median / nearest-rank p95 / max for numeric timing, throughput and
  payload metrics present in PASS records.

The parser stops at `--- transcript ---`, so arbitrary log text cannot become
metrics accidentally.

## Evidence boundary

This tool **does not** decide whether a file came from a physical phone,
emulator, replay fixture or edited text.

It never upgrades evidence to `PHYSICAL_DEVICE`.

Physical closure still requires the original court bundle and its provenance.

The aggregator is only for repeated-run statistics such as failure rate,
latency and useful throughput after valid physical evidence has been captured.

## Gate-readiness is a separate layer

`scripts/summarize-physical-evidence.py` answers:

- how many PASS/FAIL records exist;
- success rate;
- distribution of numeric timing/throughput metrics.

It intentionally does not decide whether a frontier gate is evidence-complete.

Use:

```bash
python scripts/physical-gate-readiness.py <campaign-dir>
```

for that second, conservative check.

The readiness layer additionally verifies:

- physical-candidate device metadata;
- required role pairs;
- transport-specific measurement fields;
- repeated outcomes for failure-rate claims;
- zero-BER recorded trace replay;
- external physical observations such as range, energy or concurrent Internet.

Neither layer edits the frontier ledger.

## Energy and range

Energy, distance/range and concurrent-Internet observations are not synthesized.

If a physical court does not record one of those measurements, this tool leaves
it absent rather than inferring a value.

## CI

```bash
python scripts/summarize-physical-evidence.py --self-test
```

The deterministic self-test checks:

- parsing before the transcript boundary;
- PASS/FAIL accounting;
- grouping by carrier/role;
- success-rate calculation;
- median and nearest-rank p95;
- invalid-record skipping.


## Two-device Android campaign

`scripts/android-physical-campaign.sh` prepares two real Android devices,
launches the Recovery Lab carrier courts and collects their evidence into one
campaign directory before this aggregator runs.

The campaign harness rejects qemu/emulator devices by default and records both
device fingerprints/builds.

It still does not promote evidence automatically. Physical interaction and the
carrier-specific PASS conditions remain authoritative.
