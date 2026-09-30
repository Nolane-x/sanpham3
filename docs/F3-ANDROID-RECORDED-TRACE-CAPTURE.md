# F3 Android Recorded Trace Capture Court

This tooling makes the remaining recorded acoustic/vibration gates reproducible
without pretending that every Android runtime is a physical phone.

The capture label is always:

```text
ANDROID_RUNTIME_CAPTURE
```

A separate court must prove whether the target was a physical device.

## Acoustic capture

`RecordedTraceCaptureActivity` mode `audio` uses Android `AudioRecord`:

- source: `MediaRecorder.AudioSource.MIC`;
- sample rate: 48,000 Hz;
- mono;
- signed PCM16;
- bounded capture duration: 250 ms .. 30 s;
- exact RIFF/WAVE header written after the final sample count is known;
- fsync before PASS.

Output:

```text
files/recorded-traces/latest-audio.wav
```

This format is directly accepted by:

```bash
signal-trace-replay-cli acoustic-wav
```

The activity requires the host app to have runtime `RECORD_AUDIO` permission.

## Accelerometer capture

Mode `accelerometer` uses `TYPE_ACCELEROMETER`.

Raw Android sensor events are requested at 5,000 microseconds and retained with
their monotonic event timestamps.

Because real sensor delivery is jittery, raw events are not falsely treated as
a perfect 200 Hz signal. Before writing the replay trace, the activity linearly
resamples x/y/z onto a fixed 200 Hz timestamp grid.

Output columns:

```text
sample_index,timestamp_ns,x,y,z,magnitude
```

The `magnitude` column is column 5 and is immediately consumable by:

```bash
signal-trace-replay-cli vibration-csv <file> <expected_hex> 5 <start_sample>
```

PASS metadata records both raw-event count and observed raw-event rate.

## ADB capture harness

Run:

```bash
bash scripts/android-recorded-trace-capture.sh \
  <serial> \
  dev.nolane.sanpham3.recoverylab \
  audio \
  4000
```

or:

```bash
bash scripts/android-recorded-trace-capture.sh \
  <serial> \
  dev.nolane.sanpham3.recoverylab \
  accelerometer \
  4000
```

The harness:

- validates the target is online and Recovery Lab is installed;
- grants `RECORD_AUDIO` for audio mode;
- triggers the capture activity;
- requires a typed capture PASS;
- extracts the app-private trace via `adb exec-out run-as`;
- captures Android properties plus sensor/audio diagnostics;
- writes metadata and SHA-256 manifest.

## Optional exact replay gate

If the experiment transmits a known project payload, set:

```bash
export SP3_TRACE_EXPECTED_HEX=<hex payload>
export SP3_TRACE_START_SAMPLE=<optional sample offset>
```

The harness then runs the current replay decoder automatically.

For audio it requires:

```text
F3_ACOUSTIC_REPLAY
bit_errors=0
```

For accelerometer it requires:

```text
F3_VIBRATION_REPLAY
bit_errors=0
```

If no expected payload is provided, the tool acts only as a capture/evidence
collector and does not invent a decoder PASS.

## Physical promotion

This tooling alone does **not** close:

- real recorded acoustic impulse-response replay;
- recorded physical vibration sensor replay.

To close either gate, the evidence bundle must additionally establish:

- physical device make/model;
- Android build/API;
- capture geometry/setup;
- transmitter identity/setup;
- distance/surface/orientation as applicable;
- project payload sent;
- capture SHA-256;
- replay parameters;
- exact BER result.

An emulator capture remains useful Android-framework evidence, but cannot be
promoted to physical evidence.
