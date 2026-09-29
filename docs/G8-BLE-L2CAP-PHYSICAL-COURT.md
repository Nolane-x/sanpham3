# G8 Android BLE L2CAP Physical Court

This court validates the lowest-bandwidth Android peer carrier currently in the
project without falling back to Wi-Fi or ordinary Internet.

## Required devices

Two physical Android devices:

- Android 10 / API 29 or newer;
- BLE hardware;
- Bluetooth enabled;
- app granted the Bluetooth permissions required by the OS version.

The feature scanner must report:

```text
bluetoothLeHardware = true
bluetoothLeL2capCocAvailable = true
bluetoothScanPermission = true
bluetoothAdvertisePermission = true
bluetoothConnectPermission = true
```

## Topology

```text
Android A
  no project cloud dependency
  BLE scan + L2CAP client
        |
        | BLE L2CAP CoC
        |
Android B
  BLE advertise + L2CAP server
```

Wi-Fi and cellular may be disabled for the pure BLE carrier test.

## Server sequence

1. Start `AndroidBleL2capServerDataPath`.
2. Obtain its 7-byte discovery payload:

```text
"SP3L" || version(0) || psm(u16)
```

3. Advertise that payload through `AndroidBleDiscovery`.
4. Accept the L2CAP connection.
5. Upgrade the `BluetoothSocket` with:

```kotlin
val session = serverDataPath.acceptPeerSession(
    timeoutMillis = 30_000,
    nodeId = serverNodeId,
    peerKey = sharedPeerKey,
)
```

6. Run:

```kotlin
val evidence = AndroidG8PairCourt.serveOnce(session)
```

## Client sequence

1. Scan with `AndroidBleDiscovery`.
2. Select the peer carrying a valid `SP3L` payload.
3. Connect:

```kotlin
val session = clientDataPath.connectPeerSession(
    peer = discoveredPeer,
    nodeId = clientNodeId,
    peerKey = sharedPeerKey,
)
```

4. Run:

```kotlin
val evidence = AndroidG8PairCourt.runClient(session)
```

## Trust boundary

BLE address, RSSI and the advertised PSM are routing hints only.

A PASS requires the Rust peer-session handshake to authenticate the remote
project node ID. The G8 encrypted challenge/ACK then runs inside that session.

## Required PASS evidence

Both devices must record:

- UTC timestamp;
- git commit;
- Android version;
- device model;
- local project node ID;
- authenticated peer project node ID;
- BLE RSSI at discovery;
- advertised PSM;
- G8 challenge hex;
- client/server result;
- whether Wi-Fi/cellular were disabled;
- logcat excerpts for discovery, connection and pair court.

The challenge hex must match on both devices.

## What this proves

A physical pass demonstrates:

```text
BLE discovery
-> BLE L2CAP byte stream
-> shared Rust authentication
-> encrypted bidirectional project traffic
```

without requiring Wi-Fi Direct/Aware.

## What it does not prove

It does not prove:

- long-range BLE operation;
- Internet access through BLE by itself;
- G9 recovery;
- G7 10 bit/s behavior;
- battery efficiency.

Those remain separate measured gates.
