# F6 Vibration Tiny-Capsule Prototype

This prototype carries a useful authenticated request over the existing
synthetic vibration OOK carrier.

It uses the shared `tiny-capsule` core with the domain:

```text
SP3V
```

## Useful task

The court transports a real
`peer_egress::ResolveRequest(example.com)`.

Pipeline:

```text
ResolveRequest
 -> SP3V HMAC/replay capsule
 -> bits
 -> vibration OOK waveform
 -> shared-surface gain/noise model
 -> RMS OOK decoder
 -> SP3V authentication + replay guard
 -> ResolveRequest
```

The reconstructed request must exactly equal the original.

A second delivery of the same recovered SP3V wire must fail with
`ReplayDetected`.

## Rate honesty

The reference vibration carrier uses:

```text
bit duration = 400 ms
nominal raw rate = 2.5 bit/s
```

The court reports:

- logical useful payload bytes;
- full authenticated capsule bytes;
- transmitted bits;
- synthetic accelerometer samples;
- modeled serialization duration;
- useful-payload bit rate.

This makes the extremely low throughput visible instead of presenting a tiny
capsule as normal Internet transport.

The intended role is a last-resort control/request channel where a few dozen
bytes can still have value.

## Security

SP3V inherits the same domain-separated HMAC-SHA256/128 envelope and bounded
64-sequence replay window as SP3A/SP3O.

The shared core prevents implementation drift across these constrained carrier
prototypes.

## Evidence boundary

This closes a software/synthetic useful-task baseline only.

It does not close:

- physical phone haptic-motor to accelerometer coupling;
- resonance/impulse-response behavior;
- phone orientation/body/table profiles;
- physical range;
- setup time;
- measured useful bits/s;
- energy;
- recorded sensor-trace replay;
- two-device interoperability.

Those remain required before F7 product promotion.
