# F6 Opportunistic Android USB Peer-Session Path

This baseline scavenges an already-connected USB bulk interface when Android
USB host APIs expose one.

It does not require sanpham3-specific hardware and does not make USB a mandatory
product dependency.

## Discovery

`AndroidUsbBulkDataPath.scan()` enumerates currently attached USB devices and
looks for one interface containing both:

- bulk IN endpoint;
- bulk OUT endpoint.

Each candidate records:

- device/vendor/product IDs;
- interface ID;
- endpoint addresses;
- endpoint max-packet sizes;
- whether Android has already granted USB permission.

Candidate metadata is transport discovery only. It is not project identity or
trust evidence.

## Permission boundary

`open()` fails closed unless:

- the device reports `FEATURE_USB_HOST`;
- the selected device is still present;
- Android already grants permission to the host app;
- the selected interface/endpoints still match;
- `UsbManager.openDevice()` succeeds;
- the interface can be claimed.

The adapter does not trigger a permission dialog itself. Product/UI code owns
the user-facing `UsbManager.requestPermission()` flow.

## Stream adapter

A claimed bulk IN/OUT pair is wrapped as bounded Java
`InputStream`/`OutputStream` objects over
`UsbDeviceConnection.bulkTransfer()`.

The implementation:

- limits each bulk call to 16 KiB;
- treats timeout/failure as an `IOException`;
- supports partial OUT transfers by looping until the requested bytes are sent;
- releases the interface and closes the connection exactly once.

## Shared peer-session security

The USB stream does not define its own trust protocol.

```kotlin
transport.clientPeerSession(nodeId, peerKey)
transport.serverPeerSession(nodeId, peerKey)
```

both reuse `AndroidPeerSession`, which performs the existing authenticated and
encrypted Rust peer-session handshake and framing.

Therefore USB VID/PID, device ID and endpoint descriptors are never sufficient
to trust a peer.

## Evidence boundary

This closes an Android **USB-host bulk stream + peer-session integration
baseline**.

It does not close:

- a corresponding USB gadget/accessory peer implementation;
- Android-to-Android role negotiation;
- USB permission UX;
- physical cable enumeration on target devices;
- measured setup latency/goodput/energy;
- detach/reconnect recovery;
- Windows/Linux USB peer implementations;
- physical G8 interoperability.

USB remains opportunistic: sanpham3 must still function when no USB device is
attached.
