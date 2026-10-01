# F3 Android Physical Signal Emit + Recorded Replay Court

This court completes the software path required to gather the two remaining
recorded-signal physical evidence classes:

- near-ultrasonic acoustic replay;
- vibration / accelerometer replay.

It does **not** close those gates by itself.

## Reference acoustic emitter

Recovery Lab now exposes `PhysicalSignalEmitterActivity`.

The `audio` mode matches the Rust `AcousticFskConfig::near_ultrasonic_50bps()`
reference:

```text
sample_rate_hz = 48000
bit_rate_bps   = 50
zero_hz        = 18500
one_hz         = 19500
amplitude      = 0.65
PCM            = signed 16-bit mono
```

Every symbol resets phase exactly as the Rust reference encoder does.

Android `AudioTrack` is only the playback primitive. A PASS from the emitter
means the requested PCM was submitted/played without a synchronous platform
failure. It is not proof that a handset speaker reproduced 18.5/19.5 kHz with
usable amplitude.

That is exactly what the recorded microphone trace must determine.

## Reference vibration emitter

The Rust research model is:

```text
bit_duration_ms = 400
carrier_hz      = 35
amplitude       = 0.75
```

Android vibrator APIs do not expose a signed mechanical sinusoid.

The Recovery Lab therefore uses an explicit **35 Hz envelope approximation**:

- 14 ms high;
- 14 ms low;
- repeated for a logical 1;
- fully off for a logical 0;
- exact 400 ms total per bit;
- amplitude ~= 0.75 * 255.

The emitter evidence includes:

```text
android_haptic_envelope_approximation=true
```

A physical accelerometer recording is the authority on whether this produces a
decodable shared-surface signal on the tested devices/surface.

## Automatic trace alignment

Real capture and playback are started by two independent Android devices, so
the signal cannot be expected to begin at sample zero.

The trace replay crate now exposes:

- `search_acoustic_wav_alignment()`;
- `search_vibration_csv_alignment()`.

CLI:

```bash
cargo run -p signal-trace-replay-cli -- \
  acoustic-wav-search capture.wav <expected_hex> [channel]

cargo run -p signal-trace-replay-cli -- \
  vibration-csv-search capture.csv <expected_hex> [value_column]
```

### Acoustic search

The acoustic search:

1. validates the exact WAV sample rate;
2. performs a coarse scan at 1/16 symbol increments;
3. scores the first up-to-16 expected bits by BER and FSK confidence;
4. refines sample-by-sample around the best coarse point;
5. decodes the complete expected payload;
6. reports discovered `start_sample`, BER and minimum confidence.

It does not suppress a mismatch: non-zero final BER fails the CLI command.

### Vibration search

The 200 Hz vibration trace is small enough to scan every possible start sample.

Each candidate window is decoded with the normal RMS OOK decoder and scored by
BER. The earliest minimum-BER window is selected and reported.

A physical court should use a transition-rich payload rather than an all-zero
or highly repetitive payload to avoid ambiguous alignment.

## Two-device court

Run:

```bash
scripts/android-recorded-signal-court.sh \
  <emitter_serial> \
  <capture_serial> \
  dev.nolane.sanpham3.recoverylab \
  <audio|vibration> \
  <payload_hex> \
  [evidence_dir]
```

Recommended initial payload:

```text
b2d4
```

The script:

1. requires distinct ADB devices;
2. rejects qemu/emulator by default;
3. verifies Recovery Lab on both;
4. derives signal and capture duration from the payload;
5. starts `RecordedTraceCaptureActivity` on the receiver;
6. starts `PhysicalSignalEmitterActivity` on the source with a default
   1000 ms pre-delay;
7. waits for both runtime PASS markers;
8. pulls the WAV or accelerometer CSV;
9. runs automatic alignment/replay;
10. requires `bit_errors=0`;
11. records both device fingerprints/builds and relevant audio/sensor state;
12. hashes the complete evidence directory.

## Payload bounds

To remain within the current 30-second capture bound:

```text
audio     <= 64 bytes
vibration <= 8 bytes
```

The vibration limit is the practical one because each bit occupies 400 ms.

## Evidence levels

Emitter runtime:

```text
ANDROID_RUNTIME_EMIT
```

Capture runtime:

```text
ANDROID_RUNTIME_CAPTURE
```

Combined host-side court:

```text
CANDIDATE_PHYSICAL_RECORDED_SIGNAL
```

None of these labels automatically means `PHYSICAL_DEVICE`.

The operator must still verify and retain:

- real source/capture handset models;
- real topology;
- acoustic distance/orientation or mechanical surface/contact;
- ambient conditions;
- volume/haptic settings where relevant;
- raw trace;
- replay output;
- SHA-256 manifest.

## Gate closure

The existing frontier gates remain open until a real-device campaign produces
valid evidence:

- `real recorded impulse-response replay`;
- `recorded sensor-trace replay`.

The new emitter + capture + alignment tooling removes the remaining software
dependency for those experiments. It does not replace the experiment.
