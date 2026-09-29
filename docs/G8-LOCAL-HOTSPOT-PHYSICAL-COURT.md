# G8 Android Local-Only Hotspot Physical Court

This court validates an Android local Wi-Fi LAN that explicitly does not provide
Internet access, then upgrades the TCP socket on that LAN into the shared Rust
peer-session.

The carrier is useful when ordinary Internet is unavailable because nearby
sanpham3 nodes can still communicate, relay DTN bundles, or reach a different
peer that has egress.

## Architecture

```text
small bootstrap carrier
  NFC / BLE GATT / QR / manual lab entry
        |
        | SP3H endpoint capsule
        v
Local-Only Hotspot credentials + project TCP port
        |
        v
Android client requests exact local Wi-Fi Network
        |
        v
Network.socketFactory -> hotspot host:port
        |
        v
shared Rust peer-session
        |
        v
authenticated G8 challenge / ACK
```

The hotspot SSID/passphrase is transport access, not project identity.

## Platform boundary

Server:

- Local-Only Hotspot API: Android 8.0 / API 26+
- Android 13+ requires NEARBY_WIFI_DEVICES
- older versions use the legacy location permission path

Programmatic client join:

- WifiNetworkSpecifier path: Android 10 / API 29+

A system network-selection/approval UI may be shown. Do not claim that the app
can silently bypass Android's user/network policy.

## Server

```kotlin
val server = AndroidLocalHotspotServerDataPath(context)

server.start(
    port = 45125,
) { event ->
    if (event is AndroidLocalHotspotServerEvent.Ready) {
        val endpoint = event.endpoint
        val bootstrap = AndroidLocalHotspotBootstrap.encode(endpoint)

        // Transfer bootstrap through NFC/GATT/QR/lab channel.
    }
}
```

After the client joins:

```kotlin
val session = server.acceptPeerSession(
    timeoutMillis = 30_000,
    nodeId = 200,
    peerKey = sharedPeerKey,
)

val evidence = AndroidG8PairCourt.serveOnce(session)
```

## Bootstrap capsule

Binary format:

```text
"SP3H"          4 bytes
version         1 byte
security        1 byte
ssid_len        1 byte
pass_len        1 byte
port            2 bytes, u16 big-endian
ssid            1..32 UTF-8 bytes
passphrase      0..63 ASCII bytes
```

The implementation enforces:

```text
encoded length <= 110 bytes
```

This intentionally fits inside the project's NFC short-APDU budget and the
target BLE GATT MTU envelope.

The capsule is bootstrap information only. Anyone learning the hotspot
credentials still cannot pass the Rust peer-session handshake without the
project peer key.

## Client

Decode the bootstrap:

```kotlin
val endpoint = AndroidLocalHotspotBootstrap.decode(capsule)
```

Request the local network:

```kotlin
val client = AndroidLocalHotspotClientDataPath(context)

client.start(endpoint) { event ->
    // Wait for NetworkAvailable.
}
```

The client derives the hotspot host without hard-coding an address:

- API 30+: LinkProperties DHCP server address
- API 29 fallback: IPv4 default-route gateway

Then:

```kotlin
val session = client.connectPeerSession(
    timeoutMillis = 30_000,
    nodeId = 100,
    peerKey = sharedPeerKey,
)

val evidence = AndroidG8PairCourt.runClient(session)
```

The socket is created from the exact Android Network's socketFactory. The
process default route is not changed.

## Security types

Prototype support:

- OPEN
- WPA2-PSK
- WPA3-SAE

OWE / OWE transition are currently rejected instead of being silently
misclassified as open networks.

## Required evidence

Capture on both devices:

- UTC timestamp
- git commit
- Android version
- device model
- Wi-Fi and Local-Only Hotspot feature report
- multi-STA local-only concurrency capability where available
- bootstrap capsule SHA-256
- SSID hash, not plaintext SSID if publishing logs
- security mode
- local project node ID
- authenticated peer project node ID
- derived DHCP/gateway server address
- G8 challenge hex
- hotspot startup latency
- network join latency
- G8 completion latency
- whether another Internet Wi-Fi/cellular path stayed alive concurrently
- PASS/FAIL and raw failure reason

Do not publish hotspot passphrases in evidence bundles.

## Comparison requirement

Compare on the same pair where possible:

```text
Wi-Fi Direct
Wi-Fi Aware
Local-Only Hotspot
BLE L2CAP
BLE GATT
RFCOMM
NFC bootstrap
```

Measure task success rather than only raw carrier existence.

## Closure boundary

CI can prove:

- API compilation
- endpoint normalization
- bootstrap encode/decode and size budget
- exact-Network socket construction
- Rust peer-session integration
- G8 protocol compatibility

CI cannot prove:

- OEM hotspot startup behavior
- system-user approval behavior
- real DHCP/gateway topology
- concurrent STA behavior
- real radio throughput/range/energy

Those require AVD evidence where supported and ultimately physical Android
devices.
