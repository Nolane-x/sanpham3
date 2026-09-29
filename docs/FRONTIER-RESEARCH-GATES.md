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
- [ ] multipath impulse-response convolution
- [ ] clock drift/resampling
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

- [ ] fragment IDs/ranges
- [ ] authenticated fragment envelope
- [ ] erasure/fountain coding
- [ ] source/provenance merge
- [ ] deadline/freshness-aware fragment scheduler
- [ ] correlated-failure model
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

1. [ ] NFC HCE <-> Reader peer-session bootstrap
2. [ ] BLE GATT fallback data path
3. [ ] Bluetooth RFCOMM peer-session path
4. [ ] Local-Only Hotspot peer-session path
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

- [ ] local search index
- [ ] multi-source conflict/reconciliation
- [ ] signed source receipts
- [ ] persistent cache store and eviction policy
- [ ] source-specific validity rules
- [ ] automatic transition from local-only to fresh remote when a carrier returns
