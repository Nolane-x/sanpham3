# Measurement -> Graph -> Recovery Decision

This document defines the evidence boundary between host probing and routing.

## Rule

A configured interface is not an Internet path.

A default route is not an Internet path.

A DNS server is not an Internet path.

A successful local TCP connect is not enough to declare Internet egress.

The current direct-Internet graph bridge requires a successful tiny HTTPS probe
that exchanges validated application-layer bytes on the exact measured path.

## Pipeline

```text
host inventory
-> exact-path probe
-> verified tiny HTTPS series
-> MeasuredInternetPath
-> LinkObservation(reachability=Internet)
-> ConnectivityGraph
-> adaptive recovery planner
-> Full / Compact / Semantic / TinySemantic / Emergency / DTN
```

## Evidence fusion

For each interface/adapter, multiple HTTPS targets may be measured.

The bridge selects one graph observation per path using:

1. higher observed useful bitrate;
2. lower measured loss;
3. lower median probe latency.

This prevents multiple probe targets for one adapter from creating duplicate or
order-dependent Internet edges.

## Conservative behavior

If no tiny HTTPS target succeeds, no direct Internet edge is emitted.

DNS/TCP results remain useful diagnostic evidence in the RecoveryLedger, but do
not silently become Internet reachability.

If a verified HTTPS series has no useful-throughput estimate, the core clamps
the graph bitrate to 1 bit/s instead of substituting NIC link speed. This is
deliberately conservative.

## State mapping

```text
successful series, zero failures -> Up
successful series with loss      -> Intermittent
no successful HTTPS series       -> no Internet graph edge
```

The measured series supplies:

- useful bitrate;
- loss in ppm;
- median latency surrogate;
- intermittency.

## Metering / energy baseline

Until every platform exposes richer native cost metadata, the core applies
transport-class defaults. Cellular and satellite default to metered. These are
policy priors, not measurements, and should be replaced when native evidence is
available.

## Host probe end-to-end behavior

On Linux and Windows, `host-probe-cli` now performs:

```text
scan interfaces
-> run exact-path probes
-> print RecoveryLedger
-> extract verified Internet measurements
-> create graph egress nodes
-> plan a 232-byte TinySemantic recovery task
-> print the chosen live/DTN/local decision
```

This is the first host-side path from real measurement evidence to an adaptive
recovery decision without constructing a `LinkObservation` by hand.

## Android

Android already has exact-`Network` tiny HTTPS probing. Its equivalent graph
bridge must preserve the same rule: only the exact `Network` that completed
the validated HTTPS probe may become the corresponding Internet edge.

Do not infer Android Internet reachability from a global default-network flag.

## Evidence boundary

This bridge improves software correctness. It is not a substitute for physical
G8/G9 evidence.

A public recovery claim still requires real devices and a topology where the
ordinary/default path is unusable while the engine returns new remote
information over a permitted alternate or peer path.
