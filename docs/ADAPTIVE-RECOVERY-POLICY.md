# Adaptive Recovery Policy

The engine must not stop at discovering a path.

It must decide **how to use that path**.

The same request should behave very differently on:

- 50 Mbit/s Ethernet;
- 300 kbit/s cellular;
- a 5 kbit/s peer relay;
- a 30 bit/s intermittent path;
- a 1 bit/s experimental emergency path;
- no live path at all.

## Input evidence

The planner consumes the graph's measured route evidence:

- bottleneck bit rate;
- worst observed packet loss;
- total route RTT;
- number of intermittent hops;
- number of metered hops;
- number of peer-only hops;
- route cost from the traffic-class scorer.

The planner does not treat route presence as proof that ordinary Internet use is practical.

## Conservative effective bit rate

The current baseline estimate is:

```text
effective =
  bottleneck
  * (1 - worst_loss)
  * 0.5^(intermittent_hops)
```

This is intentionally conservative and inspectable.

It is a scheduling estimate, not a physical-layer throughput guarantee.

## Delivery degradation ladder

```text
>= 1,000,000 bit/s  Full
>=   100,000 bit/s  Compact
>=     1,000 bit/s  Semantic
>=        10 bit/s  TinySemantic
>=         1 bit/s  Emergency (experimental)
```

The sub-10-bit/s mode is explicitly experimental. It exists because G7 shows
that tiny structured tasks can eventually complete there, not because the
project promises normal web browsing at those rates.

## Traffic-class compatibility

### Critical

Can use every live mode, including Emergency.

Examples: SOS, acknowledgement, location, and tiny control operations.

### TinySemantic

Can use every live mode, including compact DNS/search lookup, weather state,
short structured messages, and minimal route queries.

### Interactive

Requires Full, Compact, or Semantic. The planner must not pretend that a
30 bit/s route is an interactive web path.

### Bulk

Requires Full or Compact. Large payloads on a tiny path should be deferred,
not forced through merely because a route technically exists.

## Path identity

The route records `peer_only_hops`.

```text
0 hops                    -> LocalEgress
route with no PeerOnly    -> DirectInternet
route containing PeerOnly -> PeerEgress
```

A one-hop peer relay is still a peer path. Hop count alone must never label it
as direct Internet.

## DTN fallback

A live route is not always the correct action.

The planner can choose DelayTolerant mode when:

- no live egress exists;
- a metered path is forbidden by policy;
- the traffic class is too heavy for the measured path;
- predicted live completion exceeds the app's configured live-wait budget.

If DTN is disabled, the same conditions produce a LocalOnly decision instead of
silently sending data over an unacceptable path.

## Completion-time estimate

The task provides estimated complete wire bytes and estimated protocol round trips.

```text
serialization_time = wire_bits / effective_bps
latency_time       = route_rtt * round_trips
completion_time    = serialization_time + latency_time
```

Callers should use measured wire budgets where possible.

For example, the current G7 encrypted resolve baseline costs 232 delivered wire
bytes before later protocol optimization.

## Why this layer matters

Without this policy, the project can discover an 82 bit/s path and then hand it
to software that behaves as though it were broadband.

With this policy, the engine instead does:

```text
path exists
-> estimate what it can actually carry
-> choose representation
-> choose direct / peer / DTN
-> expose expected completion time
```

That is the difference between network scanning and an actual connectivity
recovery engine.

## Evidence boundary

This policy is deterministic software logic.

Its thresholds must be refined using G2 measured path data, G7 weak-link court
results, physical weak-link trials, battery/energy measurements, and real
Android/Windows/Linux cross-platform experiments.

Do not turn the threshold table into a universal networking claim.