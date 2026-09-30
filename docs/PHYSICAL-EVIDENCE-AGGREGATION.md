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
