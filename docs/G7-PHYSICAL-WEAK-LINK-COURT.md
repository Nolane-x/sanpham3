# G7 Physical Weak-Link Court

G7 measures whether a real pair of project endpoints can complete a useful
authenticated task when a controlled path is constrained to very low aggregate
bit rates.

This court uses a transparent byte-preserving TCP shaper. It does not claim to
be a new RF physical layer.

## Topology

Recommended three-machine topology:

```text
A — client
|
| TCP
v
B — g7-shaper-proxy
|
| rate-limited, byte-faithful TCP
v
C — peer-egress server
|
Internet
```

The project peer-session handshake remains end-to-end A <-> C.

B does not need the project PSK and does not decrypt project traffic.

## Standard ladder

Run separate evidence captures at:

| aggregate cap | recommended chunk |
| ---: | ---: |
| 1000 bit/s | 16 bytes |
| 100 bit/s | 4 bytes |
| 30 bit/s | 1 byte |
| 10 bit/s | 1 byte |

Use no periodic outage for the primary ladder:

```text
outage_period_ms=0
outage_down_ms=0
```

An additional outage experiment may use, for example:

```text
period=15000 ms
down=4000 ms
```

## C — egress

Set the same lab PSK on A and C:

```bash
export SP3_PEER_PSK_HEX=<64_hex_psk>
```

Then:

```bash
scripts/g7-physical-role.sh \
  egress \
  0.0.0.0:45123 \
  300 \
  100 \
  evidence/g7-c-100bps
```

## B — shaper

Example 100 bit/s:

```bash
scripts/g7-physical-role.sh \
  shaper \
  0.0.0.0:45200 \
  <C_IP>:45123 \
  100 \
  4 \
  0 \
  0 \
  evidence/g7-b-100bps
```

The shaper uses one shared wall-clock gate across both directions.

It prints:

- configured aggregate cap;
- total relayed bytes;
- actual wall-clock elapsed time;
- measured aggregate bit rate;
- serialization time;
- outage wait time.

The harness rejects a run if measured aggregate throughput exceeds the
configured cap.

## A — client

```bash
scripts/g7-physical-role.sh \
  client \
  <B_IP>:45200 \
  100 \
  300 \
  example.com \
  evidence/g7-a-100bps
```

A must authenticate C / node 300 through the transparent shaper and receive a
public result.

## Important timing expectation

At 10 bit/s this is intentionally slow.

The existing G7 virtual court predicts on the order of minutes for the current
handshake plus tiny request/response wire budget.

A multi-minute completion at 10 bit/s is not a failure. The target is useful
eventual information, not normal browsing latency.

## Required evidence per tier

Preserve all three role packages.

They must show:

```text
A authenticated C
C authenticated A
B target cap == requested tier
B measured aggregate rate <= requested tier
B total_bytes > 0
A received a fresh public result
```

Also record:

- physical machines/devices;
- OS versions;
- link used between A/B and B/C;
- tested commit;
- binary hashes;
- wall-clock start/end;
- whether other Internet paths on A were disabled or blocked.

## Physical closure

The primary G7 gate closes only after the 1000 / 100 / 30 / 10 bit/s ladder is
run on real machines with the controlled shaper and the useful task completes
at every tier.

This is controlled-path evidence, not proof that an arbitrary radio can
demodulate at those rates.
