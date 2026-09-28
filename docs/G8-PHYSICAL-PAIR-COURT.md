# G8 Cross-Platform Pair Court

G8 is about proving that two real project instances on different supported
platforms can authenticate each other and exchange encrypted project traffic.

A socket connection alone is not a G8 pass.

## Shared wire court

All platforms use the same pair-court payload:

```text
challenge kind = 0x50
ack kind       = 0x51
payload        = "G8P0" || 32 random bytes
payload bytes  = 36
```

The session itself is the shared Rust `peer-session` protocol.

A PASS requires:

1. the peer-session handshake authenticates the remote project node ID;
2. the client sends an encrypted 0x50 challenge;
3. the server validates the G8P0 payload;
4. the server returns the identical payload as encrypted kind 0x51;
5. the client validates both kind and byte-for-byte challenge equality.

## Windows / Linux runner

Build:

```bash
cargo build -p g8-pair-cli --release
```

Example server:

```bash
g8-pair-cli server 0.0.0.0:39080 200 <64-hex-psk>
```

Example client:

```bash
g8-pair-cli client 192.168.1.10:39080 100 <same-64-hex-psk>
```

Both sides must print `G8_PAIR_PASS` with:

- their local authenticated project node ID;
- the authenticated peer node ID;
- the same challenge hex.

## Android runner

Android first establishes a socket-capable path using LAN, Wi-Fi Direct,
Wi-Fi Aware, or another permitted app-visible transport.

The socket is upgraded into `AndroidPeerSession`, which uses the shared Rust
peer-session through JNI.

Client side:

```kotlin
val evidence = AndroidG8PairCourt.runClient(session)
```

Server side:

```kotlin
val evidence = AndroidG8PairCourt.serveOnce(session)
```

`evidence.peerNodeId` must be the authenticated project node ID, not a MAC,
IP address, PeerHandle, BLE address, or OS discovery identifier.

## Required physical matrix

The project gate requires:

```text
Android <-> Android
Android <-> Windows
Android <-> Linux
Windows <-> Linux
```

The first physical pass may use a local LAN with Internet disconnected. Later
tests should also exercise Wi-Fi Direct/Aware where supported.

## Evidence record

For every physical pair run, save:

```text
timestamp_utc:
git_commit:
client_platform:
client_os_version:
client_device_or_machine:
client_node_id:
server_platform:
server_os_version:
server_device_or_machine:
server_node_id:
transport:
internet_available_on_local_link: yes/no
server_bind_or_endpoint:
client_peer_endpoint:
challenge_hex:
client_result: PASS/FAIL
server_result: PASS/FAIL
notes:
```

Also preserve:

- exact binaries/AAR/APK hashes where practical;
- terminal/logcat output from both sides;
- topology description;
- whether normal Internet was disabled;
- whether the transport was LAN, Wi-Fi Direct, Wi-Fi Aware, or another path.

## CI evidence

CI can prove:

- Rust pair protocol logic;
- Windows/Linux compilation;
- loopback authenticated challenge/ACK;
- Android wire-format compatibility;
- Android JNI/native packaging.

CI cannot prove radio interoperability between two physical devices.

## Closure rule

A G8 pair is only checked as physically complete after both endpoints are real
devices/machines and both sides produce matching authenticated challenge
evidence.

Do not mark a pair complete from emulator, loopback, or unit-test results.
