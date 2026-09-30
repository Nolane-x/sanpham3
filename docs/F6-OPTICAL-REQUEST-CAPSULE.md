# F6 Screen-Camera Optical Request Capsule Prototype

This software/synthetic prototype carries a useful authenticated request through
the optical raster court.

It is intentionally stacked on the F3 raster/perspective baseline and is not a
physical phone-camera claim.

## Shared tiny-capsule core

The constrained acoustic and optical prototypes now share one protocol core:
`tiny-capsule`.

The wire format keeps a four-byte transport domain at the front:

```text
domain     4 bytes
version    1 byte
sender_id  8 bytes
sequence   8 bytes
length     2 bytes
payload    1..96 bytes
tag       16 bytes HMAC-SHA256/128
```

Current domains:

```text
SP3A  near-ultrasonic research capsule
SP3O  screen-camera optical research capsule
```

The domain bytes are authenticated. A valid SP3A wire cannot be opened as
SP3O, even under the same key.

## Replay protection

Both transports use the same bounded per-sender 64-sequence replay window.

The receiver accepts small out-of-order arrival exactly once and rejects:

- an exact replay;
- a sequence older than the retained window;
- new sender state after the configured sender-capacity limit.

## Useful task court

The optical court creates a real
`peer_egress::ResolveRequest(example.com)`.

Pipeline:

```text
ResolveRequest
 -> SP3O HMAC/replay capsule
 -> bits
 -> optical repetition symbols
 -> grayscale cell raster
 -> perspective/keystone warp
 -> blur
 -> exposure/gamma
 -> cell sampling
 -> repetition decode
 -> SP3O authentication + replay guard
 -> ResolveRequest
```

The reconstructed request must exactly equal the original.

The same recovered SP3O wire is then submitted again and must fail with
`ReplayDetected`.

PASS begins with:

```text
F6_OPTICAL_CAPSULE_PASS
```

The court reports payload bytes, full capsule bytes, logical bits, rendered
symbols, raster dimensions and decoder erasures.

## Acoustic compatibility

`acoustic-capsule` remains as the SP3A compatibility wrapper over the shared
core, so the existing near-ultrasonic court keeps its public API and wire
domain while removing duplicated authentication/replay logic.

## Evidence boundary

This closes a software/synthetic useful-task prototype only.

It does not close:

- visual finder/corner acquisition;
- unknown camera pose estimation;
- rolling shutter;
- autofocus/exposure adaptation;
- screen refresh/PWM effects;
- camera-video replay;
- two-device screen-camera interoperability;
- physical setup latency, useful bits/s, range or energy.

Those remain required before product promotion under F7.
