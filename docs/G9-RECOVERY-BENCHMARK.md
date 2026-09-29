# G9 Recovery Benchmark

G9 is the minimum serious recovery claim.

The benchmark asks:

> Can the ordinary/default application path be unusable while the engine still
> finds a permitted alternate or peer path and returns new remote information?

## Software court

The deterministic software court models:

```text
local node A
├─ default Internet edge -> DOWN
└─ peer-only edge -> B (Internet egress)
```

The court requires all of the following:

1. default tiny-HTTPS evidence is failed;
2. default TCP evidence is failed;
3. a local peer information path is still present;
4. the graph contains a high-capacity default edge in `Down` state;
5. the graph contains a weak `PeerOnly` rescue edge;
6. adaptive planning ignores the dead default edge;
7. the selected path is `PeerEgress`;
8. the peer-session handshake authenticates the egress peer node ID;
9. a constrained resolve request crosses the encrypted peer session;
10. a public result returns through the peer.

Run:

```bash
cargo run -p g9-recovery-court
```

A pass prints:

```text
G9_SOFTWARE_PASS ...
```

## What the software court proves

It proves that the current software layers can cooperate correctly:

```text
failure evidence
-> no LOCAL_ONLY false conclusion
-> route graph
-> adaptive recovery plan
-> authenticated peer path
-> constrained remote operation
-> returned information
```

It also proves that a nominally fast default route does not win simply because
it exists in configuration when its measured state is `Down`.

## What it does not prove

The software court does not prove that a real operating system is offline.

Its peer resolver is deterministic test infrastructure.

Therefore it does not close physical G9.

## Physical G9 topology

A valid physical benchmark needs at least:

```text
Device A:
  ordinary/default Internet path unusable

Device B:
  locally reachable from A
  permitted to relay
  has working Internet egress
```

A must obtain information through B that was not available from A's failed
default path.

Examples of suitable returned information:

- current DNS answer captured during the test;
- current tiny HTTPS status/hash from a configured public endpoint;
- a timestamped short response from a controlled Internet service.

Avoid relying on a static fixture for the physical claim.

## Prepare the consenting peer egress

On the peer device/machine that still has Internet, build and run the
constrained egress server.

Linux:

```bash
cargo build --release -p peer-egress-cli
export SP3_PEER_PSK_HEX=<64_hex_psk>
./target/release/peer-egress-cli server 0.0.0.0:45123 300 -
```

Windows PowerShell:

```powershell
cargo build --release -p peer-egress-cli
$env:SP3_PEER_PSK_HEX = "<64_hex_psk>"
.\target\release\peer-egress-cli.exe server 0.0.0.0:45123 300 -
```

The `-` PSK argument means "read `SP3_PEER_PSK_HEX`". This keeps key
material out of the process command line.

The peer server exposes only the constrained public-host resolve operation; it
is not an arbitrary TCP proxy.

## Capture harnesses

Linux:

```bash
scripts/g9-physical-capture.sh \
  <peer_ip:port> \
  <local_node_id> \
  <64_hex_psk> \
  <hostname> \
  <https_probe_ip:port> \
  <https_server_name> \
  [evidence_dir]
```

Windows PowerShell:

```powershell
.\scripts\g9-physical-capture.ps1 `
  -PeerAddr <peer_ip:port> `
  -NodeId <local_node_id> `
  -Psk <64_hex_psk> `
  -Hostname <hostname> `
  -HttpsAddr <https_probe_ip:port> `
  -HttpsServerName <https_server_name> `
  -EvidenceDir <folder>
```

The harness deliberately refuses to print a candidate PASS if the direct
tiny-HTTPS probe still produces a verified Internet path.

On success it saves:

- raw direct-path probe output;
- raw authenticated peer-rescue output;
- git commit;
- UTC timestamp;
- OS/machine metadata;
- SHA-256 hashes of both raw logs.

The generated `evidence.txt` is a **PASS candidate**, not automatic gate
closure. Topology and freshness still require human review.

## Required physical evidence

Record:

```text
timestamp_utc:
git_commit:
device_a_platform:
device_a_os_version:
device_b_platform:
device_b_os_version:
default_path_probe:
default_path_result:
peer_transport:
authenticated_peer_node:
selected_recovery_mode:
selected_path_kind:
hostname_or_remote_operation:
returned_result:
result_freshness_evidence:
client_result: PASS/FAIL
peer_result: PASS/FAIL
```

Preserve raw logs from:

- host/Android exact-path probing;
- peer discovery/data path establishment;
- authenticated peer-session;
- adaptive plan;
- constrained request/result.

## Physical pass rule

Physical G9 passes only if:

- the default path has concrete failure evidence;
- an alternate/peer path is independently established;
- the engine selects the recovery path rather than the failed default path;
- authenticated traffic crosses that path;
- new remote information returns;
- the tested commit and topology are recorded.

A simulator, loopback test, or manually injected result is not physical closure.


## Android physical G9 APK court

The Android Recovery Lab now includes a dedicated G9 screen:

```text
SP3 Recovery Lab
  -> Open G9 peer-egress recovery court
```

It uses Local-Only Hotspot as the rescue carrier and the same Rust-backed
authenticated peer-session already used by G8.

### Constrained operation

The Android peer-egress implementation mirrors the Rust `peer-egress` wire
contract:

```text
0x20 resolve request
0x21 resolve response
```

Only public-host DNS resolution is exposed. It is not an arbitrary TCP or HTTP
proxy. Hostnames that are local, literal IPs, single-label, malformed, or use
blocked local suffixes are rejected. Private, link-local, documentation,
benchmark, multicast and other non-public returned addresses are filtered.

### Device B — consenting egress peer

Device B should still have a working Internet path, typically cellular or
another permitted upstream.

1. Open the G9 court.
2. Grant nearby-Wi-Fi permission.
3. Set node ID, for example `300`.
4. Enter the shared laboratory PSK.
5. Keep the target hostname/HTTPS URL matched.
6. Tap **Start G9 egress server**.
7. Copy the generated hotspot bootstrap capsule to Device A.

After Device A authenticates, B performs a live system resolver call and
returns only filtered public addresses through the encrypted peer session.

### Device A — failed default path

Before the app joins the rescue hotspot, it performs a direct HTTPS probe using
the ordinary/default app path.

The client **refuses to print a G9 PASS candidate** if that HTTPS probe still
receives any HTTP response code. This prevents an ordinary working Internet
route from being mislabeled as recovery.

If the direct HTTPS probe fails:

1. the app requests the exact Local-Only Hotspot Network;
2. opens the rescue socket through that Network's `socketFactory`;
3. authenticates the peer node through Rust `AndroidPeerSession`;
4. sends the constrained resolve request;
5. receives a live public DNS result.

A successful run prints:

```text
G9_PHYSICAL_PASS default_failed=true carrier=local_only_hotspot ...
```

The saved client record is deliberately named a **PASS_CANDIDATE**. Final
physical gate closure still requires reviewing the matching server evidence,
topology, timestamp and freshness claim.

### Evidence

Client evidence includes:

- direct HTTPS probe URL;
- direct-probe start/end timestamp;
- concrete failure class;
- rescue carrier;
- hotspot bootstrap SHA-256, never credentials;
- authenticated peer node ID;
- network join latency;
- requested hostname and request ID;
- peer-result observation timestamp;
- returned public addresses.

Server evidence includes the authenticated client node, requested hostname,
resolver observation timestamp, filtered addresses and resolve status.

The raw hotspot SSID/passphrase is not written to evidence.
