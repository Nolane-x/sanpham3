# G8 Android BLE GATT Fallback Physical Court

This court validates GATT as a fallback when BLE discovery works but L2CAP CoC
is unavailable, unreliable, or rejected by the device/ROM.

GATT is not treated as equivalent to L2CAP. It has its own evidence.

## Layering

```text
AndroidBleDiscovery
    |
    | SP3G marker in BLE service data
    v
BluetoothDevice
    |
    | connectGatt
    v
sanpham3 GATT service
    |
    | request/response envelopes
    v
shared Rust peer-session
    |
    v
authenticated G8 challenge / ACK
```

BLE address and the discovery marker remain routing hints only.

## Prototype protocol

Service UUID:

```text
7d7938f8-4e12-4f1d-a12b-940c54fd2601
```

Characteristics:

```text
COMMAND  7d7938f8-4e12-4f1d-a12b-940c54fd2602  WRITE
RESPONSE 7d7938f8-4e12-4f1d-a12b-940c54fd2603  READ
```

Envelope:

```text
opcode:u8
payload_len:u16_be
payload
```

Opcodes:

```text
0x01 peer-session handshake
0x02 encrypted project frame
```

## MTU boundary

The prototype requests:

```text
MTU = 128
```

and currently requires:

```text
negotiated MTU >= 96
```

This is deliberate.

The current handshake and G8 frame fit in that budget.

If the device negotiates less than the minimum, the court must fail rather than
pretend fragmentation has been implemented.

A future fragmentation layer is a separate research task.

## Server

Start the GATT server:

```kotlin
val server = AndroidBleGattServer(context)

server.start(
    nodeId = 200,
    peerKey = sharedPeerKey,
) { event ->
    // record Started / PeerAuthenticated / PairPassed / Failed
}
```

Advertise through the existing BLE discovery layer:

```kotlin
bleDiscovery.start(
    discoveryInfo = server.discoveryInfo(),
) { event ->
    // ...
}
```

The GATT server handles:

- Rust ClientHello;
- Rust ServerHello;
- authenticated peer node ID;
- encrypted G8 challenge;
- encrypted G8 ACK.

## Client

Use the exact `AndroidBlePeer` returned by discovery:

```kotlin
val evidence = AndroidBleGattG8Client(context).run(
    peer = discoveredPeer,
    nodeId = 100,
    peerKey = sharedPeerKey,
)
```

Run this function on a worker thread.

The evidence includes:

- authenticated peer project node ID;
- challenge bytes;
- negotiated MTU.

## Required physical evidence

Capture on both devices:

- UTC timestamp;
- git commit;
- Android version;
- device model;
- local node ID;
- authenticated peer node ID;
- discovery RSSI;
- negotiated GATT MTU;
- challenge hex;
- PASS/FAIL;
- failure reason if MTU/service/write/read fails;
- whether L2CAP CoC was also available on the same pair.

## Comparison requirement

When possible, run GATT and L2CAP on the same two devices.

Record:

- setup latency;
- negotiated MTU;
- useful payload bytes;
- G8 completion latency;
- disconnect/failure rate;
- RSSI;
- battery/energy observations if available.

This lets Recovery Mode choose from measured evidence rather than assuming one
BLE data plane is universally better.

## Closure boundary

CI can prove:

- envelope format;
- Android API compilation;
- protocol budget rules;
- shared Rust peer-session integration;
- G8 wire compatibility.

CI cannot prove:

- OEM GATT server interoperability;
- physical MTU negotiation;
- radio range;
- connection stability;
- actual useful throughput.

Those require physical Android devices.
