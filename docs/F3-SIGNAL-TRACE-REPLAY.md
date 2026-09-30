# F3 Recorded Signal Trace Replay Harness

This harness reduces the gap between deterministic synthetic courts and actual
captured device signals.

It deliberately does **not** mark recorded-trace gates closed by itself.

## Acoustic WAV replay

Supported input baseline:

- RIFF/WAVE;
- PCM format 1;
- signed 16-bit little-endian samples;
- one or more channels;
- explicit channel selection;
- sample rate must exactly match the selected F3 decoder profile.

The current CLI command uses the near-ultrasonic 48 kHz / 50 bit/s reference
profile:

```bash
cargo run -p signal-trace-replay-cli -- \
  acoustic-wav capture.wav <expected_hex> [channel] [start_sample]
```

The expected payload length determines the number of decoded bits.

`start_sample` allows a real recording to retain leading/trailing silence or
noise instead of requiring a manually cropped symbol-perfect WAV.

The replay output reports:

- input path;
- SHA-256 of the complete input file;
- sample rate/channels;
- selected channel;
- start sample;
- expected/decoded bit count;
- bit errors;
- minimum FSK confidence.

## Vibration CSV replay

The vibration harness accepts a numeric CSV column.

Comments beginning with `#`, blank lines and one header row are supported.

Example:

```text
timestamp_ns,z_accel
0,0.01
5000000,0.04
...
```

Replay:

```bash
cargo run -p signal-trace-replay-cli -- \
  vibration-csv capture.csv <expected_hex> [value_column] [start_sample]
```

The current command uses the F3 200 Hz / 400 ms-per-bit vibration reference
decoder.

The harness intentionally accepts a preselected scalar channel rather than
claiming that arbitrary 3-axis raw accelerometer files are automatically
orientation-corrected.

## Windowed replay

Both transports support:

```text
start_sample + expected bit count
```

The decoder computes the exact sample window from the selected signal profile.

Out-of-range windows fail closed.

This makes captured files practical while keeping synchronization explicit.
Automatic packet acquisition/synchronization remains a separate research gate.

## Evidence typing

The CLI always prints:

```text
evidence_level=UNCLASSIFIED_REPLAY
```

It cannot know whether a file came from:

- a real phone;
- a DAW/export tool;
- a generated fixture;
- a transformed synthetic dataset.

A physical or host-signal court must separately record capture provenance before
promoting the evidence level to `HOST_SIGNAL_REPLAY` or
`PHYSICAL_DEVICE`.

## CI boundary

CI generates/uses deterministic synthetic samples only through unit tests.

CI proves:

- WAV chunk parsing;
- PCM16 conversion;
- channel selection;
- sample-rate validation;
- leading/trailing window support;
- CSV column parsing;
- vibration window replay;
- decoder wiring.

It does **not** prove that a real captured trace decodes.

## Gates intentionally still open

This harness does not close:

- near-ultrasonic real recorded impulse-response replay;
- vibration recorded sensor-trace replay;
- physical range/goodput;
- hardware AGC/OEM DSP behavior;
- surface/body/orientation measurements.

Those gates close only after checked-in or externally archived captures with
reproducible provenance pass the replay court.
