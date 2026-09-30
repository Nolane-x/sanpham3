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

## Optical PGM frame replay

The optical bridge accepts one or more registered grayscale frames in binary
P5 PGM format:

```bash
cargo run -p signal-trace-replay-cli -- \
  optical-pgm <expected_hex> frame1.pgm [frame2.pgm ...]
```

Current constraints are deliberate:

- 8-bit binary P5 grayscale only;
- frame dimensions must already match the current optical court grid;
- frames must already be cropped/registered to the known geometry;
- the decoder does not estimate corners, homography or camera pose.

Each frame is decoded to repeated optical symbols. Multiple frames are merged by
per-symbol voting:

- strict majority 0 -> 0;
- strict majority 1 -> 1;
- tie/no observations -> erasure.

The normal repetition decoder then reconstructs logical bits.

The CLI reports SHA-256 for every input frame and always labels the replay
`UNCLASSIFIED_REPLAY`.

This is a useful bridge for exported/cropped camera frames, but it is not the
still-open camera-video acquisition gate.

## Automatic translation registration

The replay bridge also supports a same-scale frame embedded inside a larger
grayscale canvas:

```bash
cargo run -p signal-trace-replay-cli -- \
  optical-pgm-auto <expected_hex> frame1.pgm [frame2.pgm ...]
```

The current detector is deliberately narrow and evidence-honest.

It:

1. derives the expected optical-grid dimensions from the symbol count and
   `OpticalGridConfig`;
2. scans for pixels above a conservative activation threshold between the
   rendered quiet-zone background and logical-zero level;
3. finds the first active data-cell origin;
4. expands backward by the known quiet-zone width;
5. crops the exact expected registered frame;
6. runs the existing cell/repetition decoder.

This closes **translation registration inside a larger same-scale canvas**.

It does not estimate:

- scale;
- rotation;
- projective perspective/homography;
- lens distortion;
- corners/finders under arbitrary backgrounds;
- camera pose.

The CLI reports the discovered origin for each frame.

Synthetic CI fixtures intentionally place the same raster at different offsets
inside larger low-noise canvases and require exact payload recovery.

## Automatic translation + bounded axis-scale registration

A second acquisition bridge handles a bounded independent X/Y scale change in addition
to translation:

```bash
cargo run -p signal-trace-replay-cli -- \
  optical-pgm-auto-scale <expected_hex> frame1.pgm [frame2.pgm ...]
```

The detector:

1. finds the active optical-data bounding box;
2. compares observed active width/height with the known logical grid;
3. derives independent `scale_x` / `scale_y`;
4. rejects scales outside 0.50x..3.00x;
5. allows bounded X/Y anisotropy from camera resize/crop paths;
6. expands the quiet zone at the inferred scale;
7. crops the scaled raster;
8. nearest-resamples it back to the reference grid;
9. runs normal cell/repetition decoding.

The CLI reports, for every frame:

```text
origin_x:origin_y:scale_x:scale_y
```

Synthetic courts require exact payload recovery with different offsets and
both 2.0x and 1.5x input scale.

This baseline still does **not** estimate rotation, perspective/homography,
camera pose, lens distortion or arbitrary visual finders.

## Y4M camera-video replay

The replay harness also accepts a YUV4MPEG2 stream and decodes the luma plane
from a selected frame window:

```bash
cargo run -p signal-trace-replay-cli -- \
  optical-y4m-auto-scale \
  <expected_hex> \
  capture.y4m \
  [start_frame] \
  [frame_count]
```

The current parser reads:

- `W` / `H` dimensions;
- `F<num>:<den>` frame rate;
- `C` chroma mode;
- per-frame `FRAME` headers;
- the full luma plane for every selected frame.

Supported 8-bit chroma layouts are:

- 4:2:0 variants whose `C` token begins with `420`;
- 4:2:2 variants beginning with `422`;
- 4:4:4 variants beginning with `444`;
- `mono`.

Chroma planes are skipped with exact size accounting; only luma enters the
optical decoder.

Each selected video frame then runs through the same bounded
translation+uniform-scale registration used by PGM replay before symbol voting
and repetition decode.

The CLI reports:

- complete input SHA-256;
- video dimensions;
- frame-rate ratio;
- chroma mode;
- total frames;
- selected frame window;
- decoded bit count and BER.

The evidence label remains:

```text
UNCLASSIFIED_REPLAY
```

because the parser cannot know whether a Y4M file came from a real camera, a
transcoder or a synthetic generator.

The deterministic CI fixture uses two video frames containing the same optical
payload at different scales and offsets and requires exact payload recovery.

This closes the **software video-container/luma replay path**. It does not by
itself prove camera provenance, rotation/perspective acquisition, lens
distortion handling or physical screen-camera interoperability.

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
- P5 PGM parsing;
- optical multi-frame majority/erasure voting;
- optical repetition replay under known registered geometry;
- decoder wiring.

It does **not** prove that a real captured trace decodes.

## Gates intentionally still open

This harness does not close:

- near-ultrasonic real recorded impulse-response replay;
- vibration recorded sensor-trace replay;
- camera-video acquisition/corner detection/pose estimation;
- physical range/goodput;
- hardware AGC/OEM DSP behavior;
- surface/body/orientation measurements.

Those gates close only after checked-in or externally archived captures with
reproducible provenance pass the replay court.
