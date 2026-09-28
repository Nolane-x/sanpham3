# sanpham3

> Temporary project name. Final branding is intentionally deferred until the recovery engine proves itself on real devices.

`sanpham3` is an experimental **app-only extreme connectivity recovery engine** for **Android, Windows, and Linux**.

The goal is stricter than building another mesh messenger or VPN: when the operating system says there is no Internet, the software should exhaust every connectivity path it is allowed to use before accepting that verdict.

## Hard product constraint

No external radio, dongle, custom antenna, or required hardware accessory.

The final product must use only capabilities already present in the user's phone or computer and exposed by the operating system/driver.

## Recovery model

```text
normal Internet fails
        |
        v
inventory interfaces + radios + peers
        |
        v
probe each usable path independently
        |
        +--> direct Internet egress
        +--> peer with egress
        +--> multi-hop peer route
        +--> intermittent path
        +--> store/carry/forward opportunity
        |
        v
choose the best route for the traffic class
        |
        +--> RAW mode      normal IP when viable
        +--> TINY mode     minimum-information capsules
        +--> DTN mode      queue until contact appears
        +--> LOCAL mode    cache/local compute only
```

## Bootstrap now in the repository

The first implementation is no longer only a specification. The branch contains:

- a portable Rust connectivity core;
- a capability graph for heterogeneous transports;
- traffic-aware scoring;
- multi-hop egress route selection;
- a DTN queue;
- a compact semantic capsule format with a 10-byte v0 header;
- a platform-scanner contract;
- a Linux host adapter that inventories interfaces from `/sys/class/net`;
- a real-host inventory CLI;
- a weak-link/multi-hop simulation CLI;
- Linux + Windows GitHub Actions tests;
- explicit research gates and a relay threat model.

The simulated 82 bit/s path is a software test scenario, **not** evidence that a real phone or PC currently exposes that exact path.

## Run the simulation

```bash
cargo run -p scavenger-cli
```

On Linux, inventory the real host:

```bash
cargo run -p host-probe-cli
```

Run tests:

```bash
cargo test --workspace
```

## Architecture direction

```text
                  connectivity-core (Rust)
                           |
          +----------------+----------------+
          |                |                |
       Android          Windows           Linux
          |                |                |
  connectivity APIs   Win32 / WinRT     netlink/nl80211
  Wi-Fi peer APIs     WLAN / BT         NetworkManager
  BLE                 Winsock           BlueZ
  VPN/TUN             TUN               TUN
```

Each adapter reports measured capabilities. The core must never assume that a radio/API exists merely because an operating system can support it.

## Cooperative mesh

Every consenting installation can eventually act as a scanner, peer, relay, Internet egress, or delay-tolerant custodian.

The design target is not an open proxy. Relay traffic will be bounded, authenticated, encrypted and policy-controlled.

## Non-negotiable physical boundary

The project does **not** claim that fresh remote information can arrive with literally zero physical information path.

If every usable carrier and every reachable peer is absent, the system can only use local cache/local compute until a physical path appears again.

## Research inheritance

The project reuses lessons from the earlier Nolane Causal World research program: carrier abstraction, minimum-information networking, delay tolerance, causal reconciliation, reliability/provenance accounting and extreme-bandwidth experiments.

This is a new product, not a continuation of the game.

## Rename gate

The placeholder `sanpham3` stays until real devices demonstrate peer egress, multi-hop recovery, weak-path utility and cross-platform operation. Branding comes after evidence.
