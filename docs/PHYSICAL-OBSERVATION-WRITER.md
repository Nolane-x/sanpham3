# Validated Physical Observation Writer

Some remaining physical gates require measurements that the Android court cannot
reliably infer by itself, such as external energy, range or concurrent-Internet
behavior.

Use:

```bash
python scripts/write-physical-observation.py \
  <evidence_dir> \
  --carrier rfcomm \
  --method external-meter \
  --setup-latency-ms 87.5 \
  --range-m 4.2 \
  --energy-joules 1.25 \
  --energy-method usb-power-meter \
  --note "table-top run"
```

The writer validates values and emits one key-value evidence record with:

```text
role=observation
result=PASS
evidence_level=OPERATOR_RECORDED_OBSERVATION
```

It does not infer measurements and it does not close gates.

Supported carriers:

```text
nfc_hce
ble_gatt
rfcomm
local_only_hotspot
```

Supported observation fields:

```text
setup_latency_ms   finite and >= 0
range_m            finite and > 0
energy_joules      finite and > 0
energy_method      required whenever energy_joules is present
concurrent_internet true|false
```

At least one observation metric must be provided.

## Campaign wrappers

Bash:

```bash
scripts/android-physical-campaign.sh \
  observe \
  <evidence_dir> \
  rfcomm \
  external-meter \
  --range-m 4.2 \
  --energy-joules 1.25 \
  --energy-method usb-power-meter
```

PowerShell:

```powershell
pwsh scripts/android-physical-campaign.ps1 \
  observe \
  <evidence_dir> \
  local_only_hotspot \
  phone-battery-counter \
  --energy-joules 2.6 \
  --energy-method Android-BatteryStats \
  --concurrent-internet true
```

After writing the observation, both wrappers automatically regenerate:

```text
gate-readiness.txt
gate-readiness.json
SHA256SUMS
```

## Measurement boundary

The writer validates syntax and basic numeric ranges only.

It cannot prove that:

- the instrument was calibrated;
- the stated distance was measured correctly;
- an energy reading isolates only the tested carrier;
- concurrent Internet actually worked end-to-end;
- the observation corresponds to the same physical run as another evidence
  file.

That remains part of human evidence review.

Do not enter a value merely to make readiness green.

## Self-test

```bash
python scripts/write-physical-observation.py --self-test
```

The self-test verifies a valid RFCOMM observation and rejects an energy value
without an energy method.
