# F5 Android Emulator Camera Video-Source Optical Court

This court closes the gap between file-only optical replay and an Android
Camera2 source inside an AVD.

It uses the Android Emulator camera's `videofile:` input mode.

It is Android-emulator camera evidence, not physical camera evidence.

## End-to-end path

The court executes:

```text
signal-frontier encoder
  -> deterministic optical raster
  -> 640x480 mono Y4M fixture
  -> ffmpeg H.264 MP4
  -> Android Emulator -camera-back videofile:<source.mp4>
  -> Camera2 CameraDevice
  -> ImageReader YUV_420_888
  -> exact Y plane extraction with row/pixel stride handling
  -> app-owned mono Y4M capture
  -> adb run-as extraction
  -> signal-trace-replay Y4M parser
  -> translation + uniform-scale registration
  -> multi-frame voting
  -> repetition decode
  -> BER=0
```

The decoder never reads the original fixture file during the replay check. It
only reads the Y4M captured back through Camera2.

## Deterministic fixture

`signal-trace-replay-cli optical-y4m-fixture` uses the project renderer:

- `encode_optical_repetition()`;
- `render_optical_cells()`;
- 1x project raster scale so the emulator camera's orientation/crop path does not discard symbols;
- portrait 480x640 source canvas, aligned with the emulator back camera's 90-degree sensor orientation;
- centered low-intensity canvas;
- 30 fps mono Y4M.

Example:

```bash
cargo run -p signal-trace-replay-cli -- \
  optical-y4m-fixture \
  a53cc35a \
  source.y4m \
  480 640 120
```

The CI workflow converts this deterministic Y4M into a lossless H.264/yuv420p
MP4 for the emulator camera backend.

## Recovery Lab Camera2 capture

`CameraOpticalCaptureActivity`:

1. requires runtime `CAMERA` permission;
2. chooses a back-facing camera when available;
3. requests YUV_420_888;
4. prefers 640x480 and otherwise selects the closest 4:3 output size;
5. discards configurable warmup frames;
6. reads the Y plane using the actual row and pixel strides;
7. reads `SENSOR_ORIENTATION` and rotates the tight luma into logical upright
   orientation before serialization;
8. swaps Y4M output dimensions for 90/270 degree sensors;
9. writes a tight `Cmono` Y4M stream;
10. fsyncs the capture before declaring PASS.

The first real AVD evidence showed a 90-degree back-camera sensor orientation.
Keeping the original 2x fixture caused the emulator camera's center-crop path to
truncate the active raster. The court therefore uses the 1x raster and preserves
camera orientation handling as an explicit part of the capture path rather than
teaching the decoder a one-off artifact crop.

The capture defaults are:

```text
warmup frames = 15
recorded frames = 12
preferred output = 640x480
```

## Strict host orchestrator

`scripts/virtual-phone-avd-camera-optical.sh`:

- verifies an online AVD and installed Recovery Lab;
- grants `android.permission.CAMERA`;
- requires Android camera hardware feature exposure;
- starts the Camera2 court;
- requires `CAMERA_VIDEO_SOURCE_PASS`;
- extracts `files/camera-optical/latest.y4m` with `adb exec-out run-as`;
- feeds only that capture to `optical-y4m-auto-scale`;
- requires `bit_errors=0`;
- records camera dumps, package state, logs and SHA-256 hashes.

## Real CI

`.github/workflows/f5-avd-camera-video-source.yml`:

1. installs Android 17 / API 37.0 and the current emulator;
2. requires Emulator 36.6.4+ as the tested camera-video baseline;
3. builds the deterministic project optical fixture;
4. converts it to MP4 with ffmpeg;
5. builds Recovery Lab and Rust JNI;
6. boots one headless Android AVD with:

```text
-camera-back videofile:<camera-source.mp4>
```

7. installs Recovery Lab;
8. executes the Camera2 capture court;
9. runs the Rust replay decoder over the captured Y4M;
10. uploads source/capture/log evidence.

## PASS conditions

A PASS requires all of:

- the AVD boots with the injected video camera source;
- Camera2 exposes a usable YUV_420_888 stream;
- the app captures the requested number of post-warmup frames;
- capture bytes are extracted from app storage;
- Y4M parsing succeeds;
- optical translation+scale registration succeeds;
- logical payload reconstruction has BER=0;
- evidence is labeled `ANDROID_AVD_CAMERA`.

## Evidence boundary

A green court closes the F5 **camera video-source optical replay** gate.

It does not prove:

- a physical phone camera;
- screen-to-camera optical range;
- physical rolling shutter;
- autofocus/exposure behavior on OEM hardware;
- camera lens distortion;
- physical useful bits/s;
- physical energy.

Those remain physical F6/F7 evidence requirements.


The CI source is intentionally portrait 480x640 while Camera2 requests a
640x480 sensor stream. The emulator back camera reports a 90-degree sensor
orientation. After the activity rotates sensor luma into logical orientation,
the serialized capture returns to 480x640, matching the source geometry and
avoiding the center-crop observed with a landscape source.
