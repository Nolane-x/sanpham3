# Frontier Carrier Research

This document defines the next research layer for sanpham3: exhaust every
lawful, app-visible information path before making stronger connectivity claims.

The goal is not to pretend physics can be bypassed.

The goal is to stop treating "Internet connectivity" as synonymous with one
working Wi-Fi/cellular IP route.

## Fundamental invariant

Fresh remote information requires information to cross some physical or logical
channel at some point.

If there is literally:

- no RF path;
- no optical path;
- no acoustic path;
- no mechanical/vibration path;
- no wired path;
- no peer contact;
- no carried storage;
- no delayed future contact;

then no app can obtain a fresh fact from a remote source.

In that state the product may still provide:

- cached remote facts;
- local service twins;
- deterministic local reconstruction;
- locally generated predictions;

but those outputs must be labeled as cached/stale/generated rather than fresh
Internet truth.

The research target is therefore:

```text
exhaust every lawful information carrier
+ minimize bits required per useful task
+ accumulate fragments across intermittent contacts
+ preserve provenance/freshness
+ never confuse local reconstruction with new remote information
```

## Tier A — production/app-visible carriers

These are the highest-priority product candidates because stock operating
systems expose APIs for them.

### Existing IP paths

Probe every app-visible Network/interface independently.

Do not trust only the OS default route.

Current project work already includes exact-path DNS/TCP/tiny-HTTPS evidence.

### Wi-Fi Direct

Android exposes peer discovery and direct socket networking without a normal
access point.

Project status: implemented.

Reference:

https://developer.android.com/develop/connectivity/wifi/wifi-direct

### Wi-Fi Aware

Android devices that support Wi-Fi Aware can discover peers and establish a
bidirectional network without an access point.

Project status: implemented.

Reference:

https://developer.android.com/develop/connectivity/wifi/wifi-aware

### Local-Only Hotspot

Android can create a Wi-Fi LAN explicitly documented as having no Internet
access.

This is useful as a local carrier from disconnected devices to another app
instance that may later provide relay/DTN service.

Research action:

- add capability detection;
- create/join lab path;
- upgrade sockets into peer-session;
- measure background restrictions.

Reference:

https://developer.android.com/develop/connectivity/wifi/localonlyhotspot

### Bluetooth LE L2CAP CoC

Current Android tiny-stream carrier.

Project status:

- BLE discovery;
- L2CAP CoC;
- shared Rust peer-session;
- physical court authored.

### Bluetooth Classic RFCOMM

Android exposes stream-oriented BluetoothSocket/RFCOMM.

This gives another independent Bluetooth path that may exist on devices where
LE behavior is poor or L2CAP CoC compatibility is limited.

Research action:

- capability/discovery adapter;
- RFCOMM secure-session transport;
- compare setup latency/range/energy against BLE CoC.

Reference:

https://developer.android.com/develop/connectivity/bluetooth

### NFC HCE <-> Reader mode

Android HCE can emulate an ISO-DEP card while another Android device operates as
an NFC reader.

Android's own HCE documentation explicitly describes Android reader devices
talking to HCE applications and recommends keeping exchanges small.

This is an excellent "almost touching" fallback for:

- key/bootstrap exchange;
- request capsules;
- custody transfer;
- pairing;
- emergency tiny messages.

It is not a long-range carrier.

Research action:

- proprietary AID for sanpham3;
- HCE APDU framing;
- reader-mode counterpart;
- peer-session bootstrap over APDUs;
- G8 NFC physical court.

References:

https://developer.android.com/develop/connectivity/nfc/hce
https://developer.android.com/reference/android/nfc/NfcAdapter

### USB when already connected

The product must not require an external accessory, but an already-present USB
connection is still an app-visible path worth scavenging.

Android exposes host/accessory modes.

Research action:

- enumerate existing USB interfaces;
- distinguish user-connected cable from required hardware;
- peer-session over USB stream where OS APIs permit.

Reference:

https://developer.android.com/develop/connectivity/usb

### Cellular SMS / data SMS fallback

"Mobile data unavailable" is not always the same as "all cellular signaling is
unavailable".

Android SmsManager exposes text and data-message transmission when the device,
subscription, permission and carrier permit it.

This must be treated as:

- user-visible/cost-bearing;
- permission-gated;
- carrier-dependent;
- low-rate/asynchronous;

not as free Internet.

Possible architecture:

```text
phone A (packet data dead)
 -> tiny signed request capsule over SMS
 -> phone/gateway B
 -> Internet
 -> tiny signed answer capsule over SMS
 -> A
```

Reference:

https://developer.android.com/reference/android/telephony/SmsManager

### Multipath IP

Linux MPTCP can aggregate/fail over among multiple IP interfaces for one
connection.

Multipath QUIC is also progressing through the IETF.

These do not discover new physics, but they can exploit several weak IP paths
at once after path discovery.

References:

https://docs.kernel.org/networking/mptcp.html
https://datatracker.ietf.org/doc/draft-ietf-quic-multipath/

## Tier B — app-only nontraditional physical carriers

These are the most interesting frontier candidates because they use hardware
already inside ordinary phones/computers without relying on conventional
packet radio.

They are experimental until reproduced inside this project.

### Near-ultrasonic acoustic carrier

Built-in speakers and microphones can exchange information in roughly the
near-ultrasonic audio band.

Published work has demonstrated consumer-device near-ultrasonic communication,
including systems in the kilobit/s range under favorable conditions.

Product value:

- radios disabled but audio hardware available;
- localized peer discovery/bootstrap;
- short request capsules;
- emergency relay to a nearby device.

Research issues:

- device frequency response varies strongly;
- microphones/speakers may filter high frequencies;
- audible artifacts;
- OS audio processing;
- background execution;
- environmental noise;
- safety and user consent.

References:

https://arxiv.org/abs/2103.11261
https://ieeexplore.ieee.org/document/9208265/

### Screen -> camera optical carrier

A screen can transmit encoded visual symbols that another device camera
captures.

This is an unusually good app-only path because it needs no radio stack at all.

Candidate modes:

- large QR/fountain frames;
- rolling sequence codes;
- brightness/color modulation;
- adaptive cell size based on blur/distance;
- camera feedback channel by reversing roles.

Product value:

- radio blackout;
- air-gapped peer bootstrap;
- custody transfer;
- high-integrity request/answer capsule;
- human-visible emergency transfer.

Research issues:

- line of sight;
- camera permissions;
- screen/camera orientation;
- ambient light;
- autofocus/exposure;
- privacy UX.

References:

https://arxiv.org/abs/2506.23005
https://opg.optica.org/jlt/abstract.cfm?uri=jlt-34-17-4121

### Vibration -> accelerometer carrier

Research has demonstrated low-rate data exchange using a phone vibrator and
accelerometer, including communication through a shared surface.

This is genuinely outside conventional radio.

Potential product scope:

- two phones placed on the same table/object;
- tiny emergency/custody capsules;
- bootstrap when speakers/cameras/radios are unavailable.

Treat as experimental.

References:

https://pmc.ncbi.nlm.nih.gov/articles/PMC4279528/
https://pmc.ncbi.nlm.nih.gov/articles/PMC4177823/

### Computer-generated magnetic field -> phone magnetometer

Air-gap research has demonstrated receiving CPU-workload-modulated magnetic
signals with a nearby smartphone magnetometer.

Important boundary:

- this is primarily one-way computer -> phone security research;
- it is not established as a reliable stock-phone bidirectional product link;
- do not call it a production carrier yet.

Nevertheless it belongs in the simulator as a frontier reference because it
shows that "all radios off" does not imply "no physical information path".

Reference:

https://arxiv.org/abs/1802.02317

### Fan/acoustic emanation -> microphone

Research has shown data encoded through controllable fan noise from an
air-gapped computer and received by a phone microphone.

This is another useful prior-art reminder that ordinary device components can
become information carriers.

It is not yet a sanpham3 product candidate because fan control and reproducible
modulation vary heavily by machine/OS.

Reference:

https://www.sciencedirect.com/science/article/pii/S0167404820300080

## Tier C — important science, outside final app-only boundary

These directions are worth understanding but must not quietly become required
hardware.

### Modulated Johnson noise

Published work demonstrated communication without an ambient/generated RF
carrier by modulating thermal Johnson noise.

Reported prototypes reached up to roughly 26 bit/s and several meters.

However the demonstrated system uses a resistor, RF switching/antenna and
receiver hardware.

Conclusion:

- scientifically important;
- useful for setting future lower-bound intuition;
- NOT a stock-phone app carrier.

Reference:

https://pmc.ncbi.nlm.nih.gov/articles/PMC9894108/

### Backscatter / custom SDR / LoRa / custom antennas

These can be powerful research directions, but they violate the current
"final product is only an app" requirement when they require extra hardware,
firmware access or raw radio capabilities unavailable to ordinary apps.

Keep them in references, not in product dependencies.

## Side-signals that are not payload carriers

Some APIs can improve routing/discovery without carrying arbitrary project
payload.

### Wi-Fi RTT

Useful for proximity/ranging to supported APs/Aware peers.

Do not count ranging measurements as an Internet data carrier.

Reference:

https://developer.android.com/develop/connectivity/wifi/wifi-rtt

### UWB

Current Android Jetpack UWB APIs center on ranging/position sessions.

Use as proximity/topology evidence if available, not as a generic byte stream
unless Android exposes such a data-plane API in the future.

Reference:

https://developer.android.com/reference/androidx/core/uwb/package-summary

### GNSS

Useful local source for:

- time;
- location;
- coarse state.

It is not an arbitrary fresh Internet downlink.

## Bit scavenging

The project should not require one carrier to finish an entire task.

Instead:

```text
contact 1: vibration      -> 40 bits
contact 2: acoustic       -> 80 bits
contact 3: optical        -> 200 bits
------------------------------------
authenticated fragments   -> complete task
```

This is application-layer fragment accumulation, not transparent TCP striping.

Requirements:

- same task/request identity;
- authenticated fragments;
- deduplication;
- integrity hash/MAC;
- freshness/deadline;
- provenance;
- replay protection;
- custody state.

The new `carrier-frontier` crate models this explicitly.

## Zero-carrier mode

When absolutely no channel exists, the product should still remain useful
without lying.

Candidate local-only features:

- content-addressed cache;
- service twins;
- offline maps;
- previously synchronized search index;
- local documentation/packages;
- deterministic UI/application shells;
- local AI explanation/synthesis;
- predicted values clearly labeled generated;
- source receipts and last-seen timestamps.

Recommended labels:

```text
FRESH_REMOTE
DELAYED_REMOTE
CACHED_REMOTE
LOCALLY_GENERATED
```

A user must be able to tell these apart.

## Frontier priorities

Highest product-research priority:

1. NFC HCE/Reader
2. near-ultrasonic acoustic
3. screen-camera optical
4. Bluetooth RFCOMM
5. Local-Only Hotspot
6. SMS/data-SMS gateway
7. USB opportunistic path
8. vibration-surface carrier
9. multipath aggregation
10. simulator-driven bit scavenging

Research reference only for now:

- magnetic covert channels;
- fan acoustic channels;
- Johnson-noise communication;
- hardware backscatter/custom RF.

## Falsification criteria

A candidate carrier is rejected or downgraded when:

- stock OS APIs cannot expose the needed primitive;
- it requires root/vendor firmware;
- it requires new external hardware;
- repeatable task-goodput is effectively zero;
- security metadata costs more than useful information;
- background restrictions make the intended UX impossible;
- energy cost is unacceptable;
- it cannot be authenticated safely;
- evidence only exists in a different hardware/population and cannot be
  reproduced for our target platform.

Novelty must be measured against strong prior art, not asserted from combining
many components.
