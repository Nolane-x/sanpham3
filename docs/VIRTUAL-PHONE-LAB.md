# Virtual Phone Lab

The goal of this lab is to reduce dependence on physical handsets during early
research without pretending that simulation is physical proof.

## Three simulation layers

### Layer 1 — deterministic Rust virtual phone

Implemented by:

```text
crates/carrier-frontier
apps/virtual-phone-lab
```

This layer models:

- Android API level;
- hardware feature presence;
- runtime permissions;
- carrier kind;
- setup latency;
- modeled bit rate;
- duplex mode;
- line-of-sight/contact requirements;
- whether peer Internet egress exists;
- cache availability;
- freshness contract;
- app-layer fragment accumulation across contacts.

It can execute thousands of scenarios quickly in CI.

Commands:

```bash
cargo run -p virtual-phone-lab -- frontier
cargo run -p virtual-phone-lab -- android-minimal
cargo run -p virtual-phone-lab -- android-matrix
cargo run -p virtual-phone-lab -- scavenge
cargo run -p virtual-phone-lab -- zero-carrier
cargo run -p virtual-phone-lab -- continuity
cargo run -p virtual-phone-lab -- acoustic-synthetic
cargo run -p virtual-phone-lab -- optical-synthetic
cargo run -p virtual-phone-lab -- vibration-synthetic
```

### Layer 2 — Android Virtual Device twins

Use Android Emulator to validate Android framework behavior before using real
phones.

Current Android Emulator tooling supports:

- configurable Android API/system images;
- emulated cameras or host webcams/video/image sources;
- network speed throttling;
- network latency;
- packet capture;
- multiple concurrent AVDs;
- newer emulator networking intended for multi-device peer testing.

This layer should test:

- permission denial/revocation;
- foreground/background lifecycle;
- local sockets;
- peer-session JNI loading;
- AAR/APK packaging;
- exact-Network probing;
- Wi-Fi Direct/NSD behavior where emulator support permits;
- failure/recovery state transitions.

Useful emulator options include:

```text
-netspeed
-netdelay
-tcpdump
-camera-back
-camera-front
-port
```

References:

https://developer.android.com/studio/run/emulator-commandline
https://developer.android.com/studio/releases/emulator
https://developer.android.com/studio/run/managing-avds

### Layer 3 — synthetic physical-signal replay

Nontraditional carriers need more than API simulation.

Before physical devices, create deterministic signal datasets.

Current implementation already includes deterministic courts for near-ultrasonic
FSK, optical repetition/erasure, and vibration OOK. These are MODEL_ONLY
evidence and deliberately do not count as physical carrier proof.

#### Acoustic

Generate PCM waveforms with:

- FSK/PSK/DSSS;
- 18–20 kHz near-ultrasonic profiles;
- frequency-response filters;
- multipath impulse responses;
- sample-rate error;
- Doppler-like clock mismatch;
- ambient noise;
- clipping;
- automatic-gain-control distortion.

Decoder success must be measured by:

- BER;
- packet success;
- acquisition time;
- useful bits/s;
- CPU cost.

#### Optical

Generate video/frame sequences with:

- QR/fountain cells;
- perspective warp;
- blur;
- rolling shutter;
- brightness changes;
- autofocus failure;
- dropped frames;
- camera noise;
- display gamma;
- partial occlusion.

Measure:

- frame detection rate;
- payload BER;
- useful bits/s;
- minimum readable cell size;
- latency to first verified capsule.

#### Vibration

Generate accelerometer traces with:

- on/off vibration symbols;
- surface attenuation;
- resonances;
- orientation changes;
- human movement;
- sensor-rate limits;
- background vibration.

Measure task completion at extremely small rates rather than raw throughput.

## Virtual Android API matrix

The Rust model distinguishes hardware presence from OS/API permission.

Examples encoded in the model:

- Wi-Fi Aware requires API 26+ plus hardware/API permission;
- BLE L2CAP CoC requires API 29+ plus Bluetooth capability/permission;
- Android 12+ Bluetooth permissions differ from older location-gated scanning;
- Android 13+ Wi-Fi-nearby operations depend on NEARBY_WIFI_DEVICES;
- SMS fallback depends on telephony messaging and SEND_SMS;
- NFC HCE/reader remains a separate hardware feature.

These rules are a research model and should track Android documentation as the
platform evolves.

## Physics invariant

The simulator has a non-negotiable test:

```text
NO PHYSICAL/INFORMATION CHANNEL
+ fresh remote fact required
= infeasible
```

This prevents a simulation bug from turning cached/generated data into a fake
Internet claim.

## Bit scavenging model

The simulator can accumulate authenticated task fragments across different
contacts.

Example:

```text
surface vibration contact     -> 80 bits
near-ultrasound contact       -> 120 bits
screen-camera contact         -> 120 bits
----------------------------------------
minimum task product          -> complete
```

This is not TCP bonding.

It represents application-level fragments protected by the project protocol.

Physical implementation still needs:

- task ID;
- fragment ID/range;
- integrity/authentication;
- duplicate suppression;
- expiry/freshness;
- custody;
- source/provenance.

## Simulation evidence levels

Every result should carry one of these evidence levels:

```text
MODEL_ONLY
ANDROID_AVD
HOST_SIGNAL_REPLAY
CONTROLLED_TRAFFIC_SHAPER
PHYSICAL_DEVICE
FIELD
```

Never automatically promote one level into another.

## What simulation can replace

Simulation is excellent for:

- architecture bugs;
- permission matrices;
- scheduler decisions;
- coding/FEC research;
- protocol overhead;
- outage/loss ladders;
- contact schedules;
- stale/fresh provenance logic;
- large scenario sweeps.

## What simulation cannot replace

Eventually real hardware is still required for:

- RF sensitivity;
- antenna orientation;
- OEM Bluetooth/Wi-Fi quirks;
- microphone/speaker frequency response;
- camera exposure/rolling shutter;
- vibrator/surface propagation;
- sensor noise;
- battery/thermal behavior;
- real background throttling differences;
- regulatory/carrier behavior.

The purpose of the Virtual Phone Lab is to make those physical tests the final
validation step, not the place where basic design bugs are first discovered.
