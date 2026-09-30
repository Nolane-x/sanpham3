# F5 Two-AVD Exact-Network Probe Court

This court validates that the Android recovery probe uses the exact selected
`android.net.Network` object instead of silently falling back to the process
default network.

## Recovery Lab entrypoint

The lab APK now exposes a dedicated:

```text
.ExactNetworkProbeActivity
```

for ADB-driven testing.

The activity:

1. selects the requested network handle when supplied, otherwise the active
   `Network`;
2. obtains a DNS server from that network's `LinkProperties`;
3. calls `AndroidBoundDnsProbe.probe(network, resolver)`;
4. the probe creates a datagram socket and calls
   `network.bindSocket(socket)` before sending DNS;
5. requires the returned `networkHandle` to equal the selected
   `Network.networkHandle`;
6. emits one typed log/evidence line.

A successful line begins with:

```text
EXACT_NETWORK_PASS
```

and includes both:

```text
network_handle=...
result_handle=...
```

These values must be identical.

## Two-AVD runner

Against already-running twins:

```bash
bash scripts/virtual-phone-avd-exact-network.sh \
  emulator-5554 emulator-5556 \
  dev.nolane.sanpham3.recoverylab
```

For each AVD the runner:

- verifies the device is online and the APK is installed;
- clears logcat;
- explicitly launches `ExactNetworkProbeActivity`;
- waits for PASS/FAIL;
- requires selected/result network handles to match;
- captures ConnectivityService state;
- captures the route table;
- preserves the exact-network log;
- hashes the evidence set.

The final marker is:

```text
F5_EXACT_NETWORK_AVD_PASS ... devices=2
```

## Evidence level

A real run of this script against Android Emulator twins is:

```text
evidence_level=ANDROID_AVD
```

Repository CI currently:

- compiles/packages the real Activity and Android-bound probe code;
- syntax-checks the ADB runner.

CI does **not** currently boot two emulators for this court, so CI success alone
does not close the two-AVD runtime evidence requirement.

## Why this is stronger than route inspection

A route table or default-network snapshot cannot prove that application
traffic used a particular `Network`.

The underlying DNS probe explicitly binds its socket with:

```kotlin
network.bindSocket(socket)
```

and carries the same handle into its result.

The HTTPS probe elsewhere in android-host similarly uses
`network.socketFactory`, but this first court uses DNS because emulator
network-provided DNS is self-discovering and does not require a hard-coded
external HTTPS endpoint.

## Security / product boundary

This is a lab-only exported Activity in the Recovery Lab APK.

It does not exist as a production background control surface.

The Activity accepts only an optional numeric network handle and triggers a
small DNS liveness probe; it does not accept arbitrary hostnames, URLs or
payloads.

## PASS boundary

This software contribution closes:

- a real Android app entrypoint for exact-`Network` probing;
- a deterministic two-AVD evidence collector;
- compile/package coverage for the bound socket path.

The F5 ledger item **exact-Network probe court across two AVDs** should remain
open until the runner is executed against two real AVD instances and the
resulting evidence is retained.
