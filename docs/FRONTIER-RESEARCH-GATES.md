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

- [ ] OEM quirk profiles
- [ ] background restriction profiles
- [ ] battery/thermal model
- [ ] Android 17 local-network-permission model
- [ ] Windows capability/permission profile
- [ ] Linux capability/privilege profile

## F3 — synthetic signal courts

Near-ultrasonic:

- [x] FSK waveform generation
- [x] deterministic room/noise impairment
- [x] correlation decoder
- [x] BER check
- [x] multipath impulse-response convolution
- [x] clock drift/resampling
- [ ] AGC/nonlinear filtering
- [ ] real recorded impulse-response replay

Optical:

- [x] repetition/erasure court
- [x] frame drop and sparse bit-flip impairment
- [ ] actual pixel/cell renderer
- [ ] perspective transform
- [ ] blur/exposure/gamma
- [ ] rolling-shutter model
- [ ] camera-video replay decoder

Vibration:

- [x] OOK accelerometer-trace generator
- [x] surface attenuation/noise model
- [x] RMS decoder
- [ ] resonance/impulse-response model
- [ ] orientation/body/table profiles
- [ ] recorded sensor-trace replay

## F4 — bit scavenging

Required:

- [x] task fragments can accumulate across different contact windows;
- [x] no egress means accumulated bits cannot become fresh Internet;
- [x] completion requires minimum task information product.

Future:

- [x] fragment IDs/ranges
- [x] authenticated fragment envelope
- [x] single-loss XOR erasure stripe baseline
- [ ] fountain/rateless coding
- [ ] source/provenance merge
- [x] deadline/freshness-aware fragment scheduler baseline
- [x] correlated-failure model baseline
- [ ] energy-per-useful-bit objective

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

- [ ] automated app scenario driver
- [ ] automated permission revoke/restore
- [ ] emulator Wi-Fi Direct pair court
- [ ] exact-Network probe court across two AVDs
- [ ] camera video-source optical replay
- [ ] failure/restart/Doze scenario matrix

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
5. [ ] near-ultrasonic request capsule prototype
6. [ ] screen-camera optical capsule prototype
7. [ ] SMS gateway capsule prototype
8. [ ] vibration tiny-capsule prototype
9. [ ] opportunistic USB peer-session path

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

- [x] local search index
- [x] multi-source conflict/reconciliation baseline
- [x] signed source receipts
- [x] persistent cache store and eviction policy baseline
- [x] source-specific validity rules
- [x] automatic transition from local-only to fresh remote when a carrier returns


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

This closes authenticated fragment identity/range mechanics only. It does not
close fountain/erasure coding, provenance merge, deadline scheduling, correlated
failure or energy optimization.


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

This closes only a systematic single-erasure baseline. It does **not** close
fountain/rateless coding, correlated-loss resilience, provenance merge,
deadline/freshness scheduling, or energy optimization.


### F3 acoustic impairment reconciliation

The near-ultrasonic synthetic court now includes deterministic causal
impulse-response convolution and sampling-clock drift/resampling before the
existing gain/noise/clipping stage.

The reference court still requires zero BER for its fixture after:

```text
FSK
-> multipath
-> clock drift
-> gain/noise/clipping
-> correlation decode
```

This closes only the synthetic multipath and clock-drift baselines. AGC,
device/OEM nonlinear filtering and recorded real-room impulse-response replay
remain open.

Evidence: `docs/F3-ACOUSTIC-MULTIPATH-CLOCK-DRIFT.md`.

### F4 scheduler and correlated-failure reconciliation

The authenticated fragment layer now has a deterministic
deadline/freshness-aware scheduler that advances time as wire bytes are
assigned, refuses to start a fragment that would finish after freshness
expiry, and preserves partial progress across later contacts.

Carrier redundancy planning also models primary physical failure domains, so
multiple APIs sharing one radio are not counted as independent paths. In
particular BLE GATT/L2CAP/RFCOMM share the Bluetooth domain, while Wi-Fi
Direct/Aware/Local-Only Hotspot share the Wi-Fi domain.

These are software baselines only. Fountain/rateless coding,
source/provenance merge and energy-per-useful-bit optimization remain open.

Evidence:

- `docs/F4-DEADLINE-SCHEDULER.md`
- `docs/F4-CORRELATED-FAILURE-MODEL.md`

### F8 continuity software closure

The F8 software future list now has implemented baselines for all recorded
items while preserving the invariant that zero-carrier operation is not
Internet access.

Implemented:

- bounded versioned persistent snapshots with deterministic eviction;
- source-defined validity horizons enforced in cache admission;
- Ed25519 signed source receipts with persisted verifying keys/signatures and
  backward-compatible unsigned snapshot loading;
- exact multi-source quorum/conflict reconciliation with one source ID equal
  to at most one vote;
- a live-return state machine that labels actual live observations
  `FreshRemote`, refreshes the cache, and labels later offline use
  `CachedRemote`;
- a local inverted search index whose query-time results are revalidated
  against current digest, age, source validity and optional signatures.

This does not change F0: cached, reconciled, searched or locally transformed
data never becomes fresh remote truth without an actual information carrier.

Evidence:

- `docs/F8-PERSISTENT-CONTINUITY-STORE.md`
- `docs/F8-SIGNED-SOURCE-RECEIPTS.md`
- `docs/F8-MULTISOURCE-RECONCILIATION.md`
- `docs/F8-LIVE-RETURN-TRANSITION.md`
- `docs/F8-LOCAL-SEARCH-INDEX.md`
