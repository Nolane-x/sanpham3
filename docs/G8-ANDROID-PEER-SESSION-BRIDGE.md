# G8 Android Peer-Session Bridge

Android discovery/data-path APIs and project identity are separate layers.

Wi-Fi Direct, Wi-Fi Aware, BLE, IP addresses, and MAC-like identifiers are
route hints only. They must never become trusted peer identity.

## Trust pipeline

```text
Android discovery
-> Android socket data path
-> Rust peer-session JNI
-> authenticated node ID
-> encrypted project frames
-> constrained peer operations
```

The Android side does not reimplement HMAC, XChaCha20-Poly1305, replay
protection, frame counters, or wire parsing in Kotlin.

Those remain inside the same `peer-session` Rust crate used on Windows/Linux.

## JNI boundary

The JNI bridge exposes:

- client handshake begin;
- server handshake accept;
- client handshake finish;
- authenticated peer node ID;
- encrypted frame seal;
- encrypted frame open;
- validated frame ciphertext length;
- explicit native-session close.

Kotlin owns only socket I/O and lifecycle.

## Security properties preserved

- fresh handshake nonces;
- HMAC-SHA256 transcript authentication;
- server-side replay cache;
- XChaCha20-Poly1305 frames;
- directional nonce domains;
- strict receive counters;
- authenticated project node IDs.

## Android transports

The wrapper accepts any established `java.net.Socket`, including sockets from:

- Wi-Fi Direct;
- Wi-Fi Aware;
- LAN;
- later Android transports.

Transport identity is not project identity.

## Native build

The Android artifact must package:

```text
jni/arm64-v8a/libsp3_android_peer_session.so
jni/x86_64/libsp3_android_peer_session.so
```

CI cross-compiles both ABIs with cargo-ndk and verifies both libraries are
actually present inside the generated AAR.

## G8 evidence boundary

This bridge is required for Android <-> Windows/Linux protocol compatibility,
but CI success is still software evidence.

Physical G8 closure still requires real devices exchanging authenticated
encrypted frames over the intended transport.
