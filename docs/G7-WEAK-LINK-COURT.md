# G7 Weak-Link Court

This court is the deterministic software gate for near-zero-connectivity behavior.

It does **not** close the physical G7 gate by itself.

## What the court actually exercises

The task is not a byte-count spreadsheet.

It uses the project's real protocol pieces:

```text
authenticated client hello
-> authenticated server hello
-> encrypted ResolveRequest frame
-> constrained egress operation
-> encrypted ResolveResponse frame
```

The virtual carrier then applies:

- exact serialization time from the configured bit rate;
- per-chunk propagation latency;
- deterministic lower-layer loss;
- retransmission cost;
- periodic outages;
- encrypted framing overhead;
- handshake overhead.

No real-time sleeping is required. A transfer that would need several minutes at
10 bit/s completes quickly in CI while preserving the virtual completion time.

## Standard ladder

```text
1000 bit/s
100 bit/s
30 bit/s
10 bit/s
```

Run:

```bash
cargo run -p weak-link-court-cli -- ladder
```

Every tier must return a fresh structured result successfully.

The 10 bit/s case is expected to be slow in virtual time. The important claim is
task completion, not normal web-page throughput.

## Baseline v0 wire budget

The first encrypted resolve court establishes this pre-optimization baseline:

| Link rate | Virtual completion time |
| ---: | ---: |
| 1000 bit/s | 6.106 s |
| 100 bit/s | 22.810 s |
| 30 bit/s | 66.116 s |
| 10 bit/s | 189.850 s |
| 1 bit/s *(experimental)* | 1,860.250 s (~31 min) |

The logical task carries 28 bytes of compact request/response payload, while
the complete exchange currently costs 232 delivered wire bytes:

```text
140 bytes  authenticated two-message handshake
 49 bytes  encrypted request frame
 43 bytes  encrypted response frame
----------------------------------------
232 bytes  total delivered wire bytes
```

This is intentionally recorded as a regression/optimization baseline. A later
compact or resumed session must show its savings against the same task rather
than changing the benchmark.

## Adversarial profile

Example:

```bash
cargo run -p weak-link-court-cli -- \
  profile 100 200000 300 8 15000 4000
```

Meaning:

- 100 bit/s;
- 20% deterministic lower-layer attempt loss;
- 300 ms one-way latency per chunk attempt;
- 8-byte chunks;
- a 15 s outage cycle;
- 4 s down time in every cycle.

Lost chunks are retransmitted by the virtual reliable carrier. The application
sees an intact byte stream, while the court records retransmission bytes and the
extra virtual time.

## Metrics

The court reports:

- successful task completion;
- total virtual elapsed time;
- logical protocol messages;
- delivered chunks;
- lost chunk attempts;
- delivered wire bytes;
- attempted wire bytes;
- retransmitted bytes;
- serialization time;
- propagation time;
- outage wait;
- compact request/response payload sizes;
- encrypted request/response frame sizes;
- useful-payload efficiency.

## Why this matters

At very low rates, handshake and framing overhead can dominate the actual useful
payload. This court makes that cost visible.

It also gives us a repeatable baseline before experimenting with:

- smaller handshake bootstrap;
- resumed sessions;
- negotiated dictionaries;
- batch acknowledgements;
- rateless/erasure coding;
- progressive responses;
- state-delta transport.

## Physical closure still required

A physical G7 run must use real measured links or a controlled traffic shaper
and capture:

- actual bit rate;
- actual packet loss;
- outage periods;
- task completion latency;
- transmitted bytes;
- energy where measurable;
- exact tested commit and binaries.

Simulator success must never be reported as physical RF/network success.
