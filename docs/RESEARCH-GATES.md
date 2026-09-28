# Research gates

Simulator success is not physical-device evidence.

## G0 — deterministic core

- [x] heterogeneous link model
- [x] platform capability contract
- [x] traffic-aware link scoring
- [x] multi-hop egress route selection
- [x] delay-tolerant bundle queue
- [x] compact semantic capsule roundtrip
- [x] unit tests authored
- [x] Linux interface inventory adapter authored

## G1 — real host interface inventory

On a real Linux machine:

- enumerate every network interface;
- preserve interface state even when the default route is dead;
- distinguish local reachability from Internet reachability;
- emit a machine-readable capability snapshot.

Then reproduce equivalent inventory on Windows and Android.

## G2 — independent per-path probing

For every usable interface/network handle, measure independently:

- IPv4;
- IPv6;
- DNS;
- UDP;
- TCP;
- tiny HTTPS;
- RTT;
- packet loss;
- useful throughput;
- intermittency.

A system-wide "offline" flag is never sufficient evidence.

## G3 — local peer discovery

Two instances discover one another without a project cloud service and exchange authenticated capability summaries.

## G4 — peer as egress

A device with no direct Internet submits a tiny request through a consenting peer with egress and receives a result.

## G5 — multi-hop egress

Demonstrate:

```text
A -> B -> C -> Internet
```

where A cannot directly reach C.

## G6 — partition / DTN

A queues a request while no route exists. A later contact or moving peer carries it to an egress and the result survives the return path.

## G7 — weak-path ladder

Demonstrate useful behavior under measured caps:

```text
1000 bit/s
100 bit/s
30 bit/s
10 bit/s
```

Below 10 bit/s is an experimental extension, not a promised product requirement.

## G8 — cross-platform mesh

Required pairs:

- Android <-> Android
- Android <-> Windows
- Android <-> Linux
- Windows <-> Linux

## G9 — recovery benchmark

Create a reproducible case where the OS/default application path reports no useful Internet while the engine discovers a permitted alternate/peer path and returns new remote information.

This is the minimum evidence for a serious public connectivity-recovery claim.

## Rename gate

Do not replace the temporary `sanpham3` name until G4, G5, G7 and at least two G8 pairs have passed on real devices.
