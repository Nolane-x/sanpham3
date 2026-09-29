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

Software evidence:

- [x] encrypted three-node relay path is covered by peer-egress tests;
- [x] application relay budget prevents unbounded forwarding;
- [x] relay authenticates downstream and upstream peer-session identities;
- [x] constrained public-host result can return C -> B -> A;
- [x] Linux and Windows physical role harnesses authored;
- [x] harness syntax is checked on both operating systems.

Physical closure:

- [ ] run A, B and C on three real machines/devices;
- [ ] preserve proof that A cannot directly use C;
- [ ] preserve evidence from all three authenticated roles;
- [ ] return new remote information to A through B and C;
- [ ] preserve tested commit and binary hashes.

Physical evidence must follow `docs/G5-PHYSICAL-MULTIHOP-COURT.md`.

## G6 — partition / DTN

A queues a request while no route exists. A later contact or moving peer carries it to an egress and the result survives the return path.

Software evidence:

- [x] durable DTN spool persists queued bundles on disk;
- [x] custody transfer removes the sender copy only after an authenticated ACK;
- [x] duplicate bundle IDs are idempotently rejected;
- [x] queued request can survive a simulated process restart before egress;
- [x] later egress creates a separate return-result bundle;
- [x] result can return during a later independent contact;
- [x] `inspect-spool` exposes bundle metadata without requiring payload decoding;
- [x] laboratory PSK can be supplied through `SP3_PEER_PSK_HEX` instead of argv;
- [x] Linux and Windows phase evidence harnesses authored;
- [x] dedicated harness CI validates Rust lab tests and script syntax.

Physical closure:

- [ ] A queues while it has no route to C;
- [ ] A transfers custody to B and removes its local request only after ACK;
- [ ] B restarts/reboots and bundle survives in the same spool;
- [ ] B later meets C and obtains new remote information;
- [ ] B stores a return bundle after C contact ends;
- [ ] B later meets A and transfers the result back;
- [ ] preserve per-phase spool hashes, commit, binary hashes and endpoint logs.

Physical evidence must follow `docs/G6-PHYSICAL-DTN-LAB.md`.

## G7 — weak-path ladder

Demonstrate useful behavior under measured caps:

```text
1000 bit/s
100 bit/s
30 bit/s
10 bit/s
```

Software evidence:

- [x] deterministic virtual-time weak-link court;
- [x] real authenticated handshake bytes cross the virtual carrier;
- [x] real encrypted constrained request/response frames cross the carrier;
- [x] 1000 / 100 / 30 / 10 bit/s ladder exercised in CI;
- [x] deterministic loss, retransmission cost and periodic outage support;
- [x] wire overhead and virtual task-completion time reported.

Physical closure:

- [ ] reproduce the ladder with a measured real link or controlled traffic shaper;
- [ ] record actual loss, outage periods and transmitted bytes;
- [ ] demonstrate useful remote task completion on physical devices;
- [ ] preserve tested commit, binary hashes and topology evidence.

Simulator success is explicitly not physical G7 closure.

Below 10 bit/s is an experimental extension, not a promised product requirement.

## G8 — cross-platform mesh

Software evidence:

- [x] Windows/Linux use the shared Rust peer-session implementation;
- [x] Android uses the same Rust peer-session through JNI;
- [x] Android Wi-Fi Direct/Aware sockets can be upgraded directly into authenticated peer sessions;
- [x] shared G8 challenge/ACK wire court defined across Android/Windows/Linux;
- [x] Windows/Linux loopback court verifies authenticated peer IDs and encrypted bidirectional challenge/ACK;
- [x] Android wire-format tests lock the same challenge kind, ACK kind, magic and payload length;
- [x] Android AAR packages arm64-v8a and x86_64 Rust peer-session native libraries.

Required physical pairs:

- [ ] Android <-> Android
- [ ] Android <-> Windows
- [ ] Android <-> Linux
- [ ] Windows <-> Linux

Physical evidence must follow `docs/G8-PHYSICAL-PAIR-COURT.md`.
Loopback, emulator and unit-test success do not close a physical pair.

## G9 — recovery benchmark

Create a reproducible case where the OS/default application path reports no useful Internet while the engine discovers a permitted alternate/peer path and returns new remote information.

Software evidence:

- [x] deterministic court records failed default HTTPS/TCP evidence;
- [x] peer contact prevents a false LOCAL_ONLY conclusion;
- [x] a configured high-capacity default edge in Down state is ignored;
- [x] adaptive planning selects PeerEgress over the dead default edge;
- [x] secure peer-session authenticates the rescue egress node;
- [x] constrained remote information returns through the authenticated peer;
- [x] Windows/Linux G9 court workflow authored.

Physical closure:

- [ ] reproduce default-path failure on a real supported OS/device;
- [ ] discover/establish a permitted alternate or peer path independently;
- [ ] show the engine selecting that recovery path;
- [ ] return fresh remote information unavailable through the failed default path;
- [ ] preserve commit, topology, probe logs and both endpoint logs.

Physical evidence must follow `docs/G9-RECOVERY-BENCHMARK.md`.
Software/loopback success is explicitly not physical G9 closure.

This is the minimum evidence for a serious public connectivity-recovery claim.

## Rename gate

Do not replace the temporary `sanpham3` name until G4, G5, G7 and at least two G8 pairs have passed on real devices.
