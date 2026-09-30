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
MTU = 160
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


## Recovery Lab executable court

The Android Recovery Lab now exposes:

```text
Open BLE GATT physical G8 court
```

The screen supports:

- explicit project node ID and laboratory peer PSK;
- runtime BLE scan/connect/advertise permission request;
- GATT server mode with the canonical SP3G discovery marker;
- BLE scan with explicit target address selection;
- negotiated MTU reporting;
- shared Rust peer-session authentication;
- encrypted G8 challenge/ACK;
- encrypted benchmark probes after G8.

### Benchmark

The client runs:

```text
rounds = 32
payload_bytes = min(64, negotiated encrypted plaintext budget)
```

At the target MTU 160, the encrypted plaintext budget is 122 bytes, so the
reference benchmark uses 64-byte application payloads.

If the device only negotiates the prototype minimum MTU 96, the budget is
58 bytes and the benchmark automatically reduces its application payload
instead of pretending fragmentation exists.

The server validates deterministic sequence-bound benchmark probes before
returning encrypted ACKs.

Client evidence records:

- negotiated MTU;
- min / median / p95 / max RTT;
- benchmark elapsed time;
- one-way useful bits/s;
- round-trip useful bits/s;
- authenticated peer project node ID;
- route address and RSSI only as non-identity hints.

The server records the final authenticated benchmark sequence and payload size.

A benchmark frame mismatch, MTU overflow or transport failure prevents PASS.
