# Frontier Research Gates

These gates are separate from the product G0-G9 closure ledger.

A frontier carrier must not become a product claim merely because a paper,
simulator, or API exists.

## F0 — physics and provenance invariant

Required:

- [x] fresh remote information is impossible with no information channel;
- [x] cached/generated/local outputs are typed separately;
- [x] zero-carrier court fails fresh-remote requests;
- [x] simulator does not promote cache into fresh Internet.

## F1 — carrier surface census

Production/app-visible candidates:

- [x] ordinary per-path IP
- [x] Wi-Fi Direct
- [x] Wi-Fi Aware
- [x] Local-Only Hotspot
- [x] BLE advertising/control
- [x] BLE GATT
- [x] BLE L2CAP CoC
- [x] Bluetooth Classic/RFCOMM
- [x] NFC HCE/Reader
- [x] opportunistic USB
- [x] cellular SMS/data-SMS
- [x] DTN/data-mule
- [x] Linux MPTCP / multipath research path

Experimental app-only physical candidates:

- [x] near-ultrasonic audio
- [x] screen-camera optical
- [x] vibration-surface

Research references, not product carriers yet:

- [x] CPU-generated magnetic field -> phone magnetometer
- [x] fan acoustic emanation
- [x] modulated Johnson noise
- [x] custom RF/backscatter/SDR boundary recorded

## F2 — Virtual Android matrix

Required:

- [x] API-level model
- [x] hardware-feature model
- [x] permission-state model
- [x] Android 26/28/29/31/33/36/37 sweep
- [x] BLE CoC API gate
- [x] nearby-Wi-Fi permission gate
- [x] Bluetooth permission gate
- [x] SMS permission/telephony gate

Future:

- [x] OEM quirk profiles
- [x] background restriction profiles
- [x] battery/thermal model

Android F2 now applies execution state, conservative OEM quirk switches and a
battery/thermal resource policy only as capability reductions. The presets are
research policy models, not measured vendor-specific behavior.
- [x] Android 17 local-network-permission model

Android 17 F2 now distinguishes broad `ACCESS_LOCAL_NETWORK`, a
system-mediated selected-device path and denied LAN access. For target SDK 37+
broad LAN is disabled without permission; ordinary Internet remains separate.
Android 16 opt-in behavior remains a compatibility branch using
`NEARBY_WIFI_DEVICES`.
- [x] Windows capability/permission profile
- [x] Linux capability/privilege profile

Desktop F2 now uses a three-way intersection of physical hardware, modeled
OS/user access and project adapter implementation. Current Windows/Linux
profiles deliberately leave special carriers false until a matching sanpham3
adapter exists; ordinary/external OS network interfaces remain separately
scavengable.

## F3 — synthetic signal courts

Near-ultrasonic:

- [x] FSK waveform generation
- [x] deterministic room/noise impairment
- [x] correlation decoder
- [x] BER check
- [x] multipath impulse-response convolution baseline
- [x] clock drift/resampling baseline
- [x] deterministic AGC/nonlinear filtering baseline
- [ ] real recorded impulse-response replay

Current near-ultrasonic software court composes FSK, deterministic multipath,
sampling-clock drift, bounded RMS-window AGC, cubic nonlinear compression,
noise/gain/clipping and correlation decode. These remain synthetic impairments;
recorded handset traces are still open.

Optical:

- [x] repetition/erasure court
- [x] frame drop and sparse bit-flip impairment
- [x] actual pixel/cell raster renderer baseline
- [x] deterministic perspective/keystone transform baseline
- [x] blur/exposure/gamma photometric baseline
- [x] rolling-shutter/PWM banding baseline
- [x] camera-video replay decoder baseline

Current optical software court renders grayscale cells and applies known
perspective geometry, blur/exposure/gamma plus rolling-shutter/PWM banding before
sampling/repetition decode. The replay harness now also parses YUV4MPEG2 video,
extracts exact luma frame windows, performs bounded translation+uniform-scale
registration, votes symbols across frames and runs repetition decode.

This closes the software camera-video replay decoder baseline only. Y4M input
remains `UNCLASSIFIED_REPLAY`: Android Camera2/virtual-camera acquisition,
capture provenance, automatic rotation/projective pose recovery, lens distortion
and physical screen-camera interoperability remain open. The separate F5
camera-video source gate therefore stays open.

Vibration:

- [x] OOK accelerometer-trace generator
- [x] surface attenuation/noise model
- [x] RMS decoder
- [x] resonance/impulse-response baseline
- [x] deterministic mount/orientation profile baseline
- [ ] recorded sensor-trace replay

Current vibration software court composes OOK, a causal mechanical impulse
response and deterministic flat-table/edge-contact/handheld mount profiles
before the shared-surface noise model and RMS decode. Recorded physical sensor
traces remain open.

## F4 — bit scavenging

Required:

- [x] task fragments can accumulate across different contact windows;
- [x] no egress means accumulated bits cannot become fresh Internet;
- [x] completion requires minimum task information product.

Future:

- [x] fragment IDs/ranges
- [x] authenticated fragment envelope
- [x] single-loss XOR erasure stripe baseline
- [x] authenticated rateless random-linear coding baseline
- [x] source/provenance merge across carriers
- [x] deadline/freshness-aware fragment scheduler baseline
- [x] correlated-failure-domain model
- [x] measured energy-per-useful-bit objective

Current F4 software stack now composes authenticated exact fragments,
single-erasure XOR parity, authenticated rateless random-linear coding,
provenance retention/merge, deadline/freshness scheduling, primary
failure-domain diversification, measured setup+transfer energy per useful bit
and URT exact reconstruction beneath weak/intermittent contacts.

## F5 — Android AVD twin lab

Required:

- [x] dual-AVD launch harness
- [x] configurable network speed
- [x] configurable network latency
- [x] pcap capture
- [x] feature/connectivity snapshots
- [x] optional APK install
- [x] evidence hashing

Future:

- [x] automated app scenario driver baseline
- [x] automated permission revoke/restore baseline
- [x] emulator Wi-Fi Direct pair court
- [x] exact-Network probe court across two AVDs
- [x] camera video-source optical replay
- [x] failure/restart/Doze scenario driver/state-machine baseline

The current F5 baseline now includes dual-AVD lifecycle/permission drivers,
a real two-AVD exact-Network probe court, an authenticated dual-AVD Wi-Fi
Direct group/pair court and a Camera2 video-source optical replay court.

The Wi-Fi Direct court requires real framework group formation, client
discovery/join, TCP over the group-owner address, authenticated Rust
`AndroidPeerSession` node IDs and the encrypted G8 challenge/ACK exchange.

The Camera2 court injects a deterministic optical video source into the Android
Emulator camera, captures through Camera2 `YUV_420_888`, persists the captured
Y4M and replays that captured evidence through the Rust optical decoder with
zero BER.

These close Android AVD/framework evidence only. They do not promote physical
radio/camera interoperability, range, goodput, failure-rate or energy claims.

## F6 — new app-only prototype carriers

Priority order:

1. NFC HCE <-> Reader peer-session bootstrap
   - [x] proprietary HCE AID and short-APDU framing
   - [x] shared Rust peer-session authentication
   - [x] encrypted G8 challenge/ACK
   - [x] NFC/HCE capability reporting and manifest registration
   - [x] APDU protocol unit tests and compile-time payload budget guard
   - [x] physical court authored
   - [ ] two-device physical interoperability
   - [ ] measured APDU timing, transceive limits, useful bits/s and failure rate
2. BLE GATT fallback data path
   - [x] request/response envelope
   - [x] shared Rust peer-session authentication
   - [x] encrypted G8 challenge/ACK
   - [x] conservative MTU boundary; fails instead of pretending fragmentation
   - [x] Android server/client compile and unit tests
   - [x] physical court authored
   - [ ] two-device physical interoperability
   - [ ] measured useful bits/s, setup latency, failures and energy
3. Bluetooth RFCOMM peer-session path
   - [x] Classic discovery and bonded-peer inventory
   - [x] user-approved discoverability helper
   - [x] insecure RFCOMM server/client data path
   - [x] shared Rust peer-session authentication
   - [x] G8 compatibility through AndroidPeerSession
   - [x] Bluetooth Classic feature reporting
   - [x] physical court authored
   - [ ] two-device physical interoperability
   - [ ] measured setup latency, reliability, range, useful bits/s and energy
4. Local-Only Hotspot peer-session path
   - [x] Android 8+ hotspot server and Android 10+ programmatic client
   - [x] exact WifiNetworkSpecifier / Network socket path
   - [x] compact SP3H bootstrap capsule <= 110 bytes
   - [x] DHCP server / IPv4 gateway derivation without hard-coded LAN address
   - [x] shared Rust peer-session authentication
   - [x] typed local-only connection failure evidence on supported Android versions
   - [x] Wi-Fi and local-only STA concurrency capability reporting
   - [x] physical court authored
   - [ ] two-device physical interoperability
   - [ ] measured startup/join/G8 latency, useful bits/s, concurrent-Internet behavior and energy
5. [x] near-ultrasonic request capsule software/synthetic prototype
6. [x] screen-camera optical capsule software/synthetic prototype
7. [x] SMS gateway capsule software transport prototype
8. [x] vibration tiny-capsule software/synthetic prototype
9. [x] opportunistic Android USB bulk peer-session baseline

Current F6 software baselines now include:

- `SP3A` near-ultrasonic authenticated ResolveRequest capsule;
- `SP3O` optical authenticated ResolveRequest capsule;
- `SP3V` vibration authenticated ResolveRequest capsule;
- `SP3M` segmented SMS request/response gateway transport with explicit
  consent/subscription/roaming policy and Android `SmsManager` send adapter;
- Android USB-host bulk IN/OUT stream discovery/opening reused beneath the
  existing authenticated Rust peer-session.

These are not physical promotion claims. NFC/GATT/RFCOMM/Hotspot/USB and the
nontraditional carriers still require real interoperability/latency/goodput/
failure/energy evidence; SMS still requires real receive/delivery/operator
evidence.

Each prototype needs:

- authenticated project identity;
- replay protection;
- payload integrity;
- measured setup time;
- measured useful bits/s;
- energy observations when possible;
- failure reasons;
- simulator-vs-physical reconciliation.

## F7 — product promotion rule

A frontier carrier is promoted into normal Recovery Mode only after:

- stock target OS exposes the required primitive;
- no root/vendor firmware is required;
- no mandatory new external hardware is required;
- the project reproduces the carrier itself;
- a useful task succeeds, not only raw bits;
- security metadata is included in the measurement;
- failure behavior is safe;
- evidence is clearly typed.

Paper-only or simulator-only carriers remain research candidates.

## F8 — zero-carrier continuity

This is not Internet access.

Implemented baseline:

- [x] content-addressed cache integrity through SHA-256 source receipts
- [x] source receipt / observed-at metadata
- [x] deterministic service twins over cached remote inputs
- [x] cache-age admission policy
- [x] locally generated answer labeling
- [x] current-remote contract is rejected with no carrier
- [x] service-twin output inherits cached-source provenance

Future work:

- [x] provenance-aware local search index
- [x] conservative multi-source conflict/reconciliation
- [x] Ed25519 signed source receipts
- [x] bounded persistent cache store and deterministic eviction policy
- [x] source-specific validity rules enforced during cache use
- [x] automatic transition from local-only to fresh remote when a carrier returns

Current F8 software closure remains truth-preserving: persisted/searchable
remote bytes are labeled `CachedRemote`; signed receipts bind source/time/hash
metadata; exact-digest multi-source reconciliation exposes conflicts instead of
inventing consensus; and only a live carrier observation can produce
`FreshRemote`.


### F4 authenticated fragment baseline

The closed baseline uses `fragment-transport`:

- deterministic transfer ID derived from the whole-object SHA-256;
- explicit byte offset and total length on every fragment;
- full whole-object SHA-256 carried in every envelope;
- HMAC-SHA256 authentication truncated to a 128-bit wire tag;
- bounded wire-budget fragmentation;
- out-of-order assembly;
- idempotent exact duplicates;
- conflicting duplicates and overlapping ranges rejected;
- missing byte ranges reported explicitly;
- final reconstruction accepted only if the whole-object SHA-256 matches.

The software court deliberately reverses fragment order and injects a duplicate
before requiring exact reconstruction.

This section records the authenticated fragment identity/range baseline only.
Later F4 courts separately close XOR erasure, authenticated rateless RLNC,
provenance merge, deadline/freshness scheduling, correlated failure domains and
measured energy-per-useful-bit selection.


### F4 authenticated erasure + URT baseline

The next F4 software baseline adds an authenticated `SP3E` parity envelope
without changing the existing `SP3F` data-fragment wire format.

Properties:

- data shards remain ordinary HMAC-authenticated SP3F fragments;
- each stripe has one HMAC-authenticated XOR parity shard;
- the wire budget includes parity-envelope overhead before payload sizing;
- one missing data shard per stripe is recoverable exactly;
- two or more missing shards in the same stripe remain explicitly
  `Insufficient`;
- tampered parity is rejected before recovery;
- final reconstructed object must still pass the whole-object SHA-256.

The dedicated court uses a real URT exact packet as the fragmented object,
splits delivery across multiple synthetic contact windows, drops one data shard
from every stripe, injects duplicate/out-of-order fragments, then recovers the
URT wire and decodes it back to the original logical payload.

This section closes only the systematic single-erasure baseline. Later F4
courts add authenticated rateless RLNC, provenance merge,
deadline/freshness scheduling, correlated-failure-domain awareness and a
measured energy-per-useful-bit objective.
