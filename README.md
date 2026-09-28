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

## Non-negotiable physical boundary

The project does **not** claim that fresh remote information can arrive with literally zero physical information path.

If every usable carrier and every reachable peer is truly absent, the system can only use local cache/local compute until a physical path appears again.

## Status

Bootstrap in progress. The first milestone is a portable Rust core for connectivity graphing, link scoring, multi-hop egress selection, delay-tolerant queuing and compact semantic capsules, followed by real Android/Windows/Linux adapters and physical-device validation.

## Rename gate

The placeholder `sanpham3` stays until the project demonstrates real connectivity recovery on Android, Windows and Linux. Branding comes after evidence.
