# G8 Stream Peer-Session Benchmark

This helper standardizes latency and useful-throughput measurement after a
stream carrier has already established the shared authenticated
`AndroidPeerSession`.

It is intended for stream transports such as:

- BLE L2CAP CoC;
- Bluetooth RFCOMM;
- Local-Only Hotspot TCP;
- Wi-Fi Direct TCP;
- opportunistic USB bulk streams.

It is not used for NFC APDU or BLE GATT because those paths have different
message-size and exchange mechanics.

## Protocol

The benchmark reserves two authenticated peer-session message kinds:

```text
0x60  benchmark probe
0x61  benchmark ACK
```

Every probe contains:

- 8-byte big-endian sequence number;
- deterministic payload bytes filling the configured application payload size.

The server validates sequence and payload length and echoes the complete
authenticated payload as an ACK.

The client requires an exact byte-for-byte echo.

## Default workload

```text
rounds        = 32
payload_bytes = 1024
```

Both values are configurable and bounded:

- rounds: 1..10000;
- payload: 16..1048576 bytes.

## Evidence

`AndroidPeerBenchmarkEvidence` reports:

- authenticated peer node ID;
- completed rounds;
- payload bytes per one-way probe;
- one-way useful byte total;
- round-trip useful byte total;
- total benchmark elapsed nanoseconds;
- minimum RTT;
- median RTT;
- p95 RTT;
- maximum RTT;
- one-way useful bits/s;
- round-trip useful bits/s.

RTT percentiles use nearest-rank ordering.

Useful throughput is application payload accounting. It is **not** raw radio,
link-layer, IP or encrypted-wire throughput.

## Failure semantics

The benchmark fails closed on:

- unexpected message kind;
- unexpected payload length;
- sequence mismatch;
- ACK payload mismatch;
- peer-session transport/decryption failure.

A thrown benchmark error means the run must not be recorded as a successful
measurement.

Repeated physical runs are still required to calculate empirical failure rate.

## Evidence boundary

This helper standardizes software-side measurement only.

It does not provide:

- physical range;
- radio RSSI;
- power/energy measurement;
- battery drain;
- setup/join latency before the peer session exists;
- operator/OEM behavior.

Those must be captured by the individual physical carrier courts.

In particular, the open F6 physical gates remain open until real devices are
used. This helper makes their latency/goodput evidence comparable; it does not
replace physical evidence.
