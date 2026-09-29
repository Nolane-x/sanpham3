# F6 Near-Ultrasonic Request Capsule Prototype

This is a software/synthetic prototype for moving a tiny authenticated request
over the existing near-ultrasonic FSK research carrier.

It is not a physical-device carrier claim.

## SP3A capsule

The wire format is:

```text
magic      4 bytes   "SP3A"
version    1 byte
sender_id  8 bytes
sequence   8 bytes
length     2 bytes
payload    1..96 bytes
tag       16 bytes   HMAC-SHA256 truncated to 128 bits
```

The authentication tag covers every field before the tag, including sender ID,
sequence and payload.

The project key is external to the capsule and is never transmitted.

## Replay protection

The receiver keeps a bounded replay window per sender.

Each sender has:

- highest accepted 64-bit sequence;
- a 64-bit bitmap for the previous sequence window.

This allows small packet reordering while rejecting:

- exact replays;
- sequences older than the window;
- unbounded new sender state after configured capacity is exhausted.

## Useful task court

The court does not transmit a random fixture.

It creates a real constrained `peer-egress::ResolveRequest` for
`example.com`, serializes it, places it in SP3A and converts the complete
capsule to bits.

The bits travel through:

```text
50 bit/s FSK
 -> deterministic multipath
 -> +60 ppm clock drift
 -> gain/noise/clipping
 -> FSK decoder
 -> SP3A HMAC verification
 -> replay guard
 -> ResolveRequest decoder
```

The final request must be byte/field identical to the original.

The same recovered capsule is submitted a second time and must be rejected as
a replay.

PASS begins with:

```text
F6_ACOUSTIC_CAPSULE_PASS
```

The court reports:

- useful request bytes;
- full capsule bytes;
- transmitted bits;
- modeled serialization duration at the configured FSK bitrate;
- useful-payload bit rate;
- minimum decoder confidence.

## Security boundary

The shared/peer key authenticates the capsule fields under whatever key
provisioning policy the product uses.

This prototype does not define key exchange.

A per-peer key can authenticate a sender identity. A single project-wide shared
key only proves possession of that project key and would not prevent one holder
from impersonating another sender ID.

## Evidence boundary

This court proves a software pipeline and deterministic synthetic impairment
tolerance.

It does not prove:

- microphone/speaker synchronization acquisition;
- physical near-ultrasonic range;
- real useful bits/s;
- real setup latency;
- energy use;
- phone-specific filtering/AGC;
- human audibility;
- two-device interoperability.

Those remain physical measurement requirements before product promotion.
