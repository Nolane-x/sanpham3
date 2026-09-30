# G8 Android Bluetooth Classic RFCOMM Physical Court

This court validates Bluetooth Classic RFCOMM as another app-only peer carrier
when BLE L2CAP/GATT are unavailable, unstable, or poorly supported by a device.

RFCOMM is not trusted for identity.

The Bluetooth device/address and SDP service UUID are routing only. The accepted
BluetoothSocket is upgraded into the same Rust-backed AndroidPeerSession used by
Wi-Fi and BLE L2CAP.

## Requirements

Two Android devices with Bluetooth Classic hardware.

Android 12+ requires runtime permission for:

- BLUETOOTH_SCAN during discovery;
- BLUETOOTH_CONNECT for socket operations.

Older Android versions use the legacy Bluetooth/location permission model.

## Service

The prototype registers:

```text
name = sanpham3-recovery-rfcomm
uuid = bdbd3af8-7b52-4e1c-96fb-0fd6b4ba1a72
```

The data plane is a duplex BluetoothSocket byte stream.

## Server

```kotlin
val server = AndroidBluetoothRfcommServerDataPath(context)
server.start()

val session = server.acceptPeerSession(
    timeoutMillis = 30_000,
    nodeId = 200,
    peerKey = sharedPeerKey,
)

val evidence = AndroidG8PairCourt.serveOnce(session)
```

## Client discovery

Try already bonded/known peers first:

```kotlin
val discovery = AndroidBluetoothClassicDiscovery(context)
val knownPeers = discovery.bondedPeers()
```

For unknown nearby peers, start Classic discovery:

```kotlin
discovery.start { event ->
    if (event is AndroidBluetoothClassicEvent.PeerDiscovered) {
        // event.peer is only a route candidate.
    }
}
```

A server that must be discoverable to unknown peers requires normal Android
user approval. The app can launch:

```kotlin
startActivity(
    AndroidBluetoothClassicDiscovery.requestDiscoverableIntent(
        durationSeconds = 120,
    ),
)
```

Do not silently claim that an arbitrary hidden Classic device is discoverable.

The discovery layer intentionally does not claim that every discovered Classic
Bluetooth device runs sanpham3. Service discovery/connect decides whether the
project RFCOMM service exists.

## Client

After choosing a discovered candidate:

```kotlin
val client = AndroidBluetoothRfcommClientDataPath(context)

val session = client.connectPeerSession(
    peer = discoveredPeer,
    nodeId = 100,
    peerKey = sharedPeerKey,
)

val evidence = AndroidG8PairCourt.runClient(session)
```

The blocking RFCOMM connect and G8 court must run off the Android main thread.

## Recovery Lab executable court

The Android Recovery Lab now exposes:

```text
Open RFCOMM physical G8 court
```

The screen supports:

- explicit project node ID and laboratory peer PSK;
- runtime Bluetooth permission request;
- Android discoverability request;
- bonded-peer inventory plus Classic discovery;
- explicit target Bluetooth address selection;
- RFCOMM server mode;
- RFCOMM client mode;
- shared Rust `AndroidPeerSession` authentication;
- encrypted G8 challenge/ACK;
- shared stream benchmark after G8.

The court does not automatically trust or connect to the first nearby Classic
device. The user selects a target address from the discovered/bonded candidates.

### Benchmark evidence

After G8 succeeds, the client runs the shared reference workload:

```text
rounds        = 32
payload_bytes = 1024
```

Evidence records:

- min / median / p95 / max RTT;
- benchmark elapsed time;
- one-way useful bits/s;
- round-trip useful bits/s;
- authenticated peer project node ID;
- route address/name/RSSI only as non-identity hints.

The server records the configured workload and total benchmark serve duration.

A benchmark mismatch or transport failure prevents the run from recording PASS.

## Security boundary

The server currently uses the insecure RFCOMM API so OS-level bonding is not the
project trust root.

A PASS still requires:

```text
BluetoothSocket
-> Rust ClientHello / ServerHello
-> authenticated project node ID
-> encrypted G8 challenge
-> encrypted G8 ACK
```

Wrong peer keys, replayed handshakes or tampered frames must fail in the shared
peer-session layer.

## Required evidence

Record:

- UTC timestamp;
- git commit;
- Android version;
- device model;
- Bluetooth Classic feature report;
- local project node ID;
- authenticated peer project node ID;
- route candidate address/name only as non-identity hints;
- RSSI if discovery supplied it;
- G8 challenge hex;
- connect latency;
- G8 completion latency;
- PASS/FAIL;
- failure reason;
- comparison with BLE L2CAP/GATT on the same pair when possible.

## Comparison

The same device pair should ideally run:

```text
BLE L2CAP
BLE GATT
Classic RFCOMM
```

Compare:

- discovery time;
- connect/setup time;
- G8 completion latency;
- reconnect reliability;
- range;
- energy/battery observations;
- background restrictions.

Recovery Mode should eventually choose from measured carrier evidence rather
than assume one Bluetooth data plane always wins.

## Closure boundary

CI can prove:

- Android API compilation;
- Classic discovery code;
- RFCOMM server/client construction;
- reuse of AndroidPeerSession;
- shared G8 protocol compatibility.

Only physical Android devices can prove:

- actual SDP discovery/interoperability;
- controller/OEM behavior;
- radio range;
- connection stability;
- useful throughput and energy.

Do not promote RFCOMM into normal Recovery Mode until physical evidence exists.
