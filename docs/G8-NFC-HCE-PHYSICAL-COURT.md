# G8 Android NFC HCE / Reader Physical Court

This court validates NFC as a radio-independent, extremely short-range
authenticated capsule/bootstrap carrier between two Android devices.

It is not an Internet carrier by itself.

## Requirements

Two physical Android devices.

HCE side:

- NFC hardware;
- `android.hardware.nfc.hce`;
- sanpham3 HCE service enabled by the platform.

Reader side:

- NFC hardware;
- reader mode support;
- foreground Activity while the court runs.

Both sides must use the same laboratory project peer key.

## Wire path

```text
Reader Android
  enableReaderMode
      |
      | ISO-DEP / APDU
      |
HCE Android
  HostApduService
```

The registered proprietary AID is:

```text
F053503346524E54
```

The APDU carrier does not define project identity.

## Authenticated sequence

The court performs:

```text
1. SELECT project AID
2. Reader creates Rust ClientHello
3. ClientHello crosses NFC APDU
4. HCE invokes Rust serverAccept(...)
5. HCE returns Rust ServerHello
6. Reader invokes Rust clientFinish(...)
7. both sides now have authenticated project peer IDs
8. Reader seals G8 challenge kind 0x50
9. encrypted frame crosses NFC APDU
10. HCE opens it with Rust peer-session
11. HCE seals identical challenge as kind 0x51
12. Reader opens and verifies byte-for-byte equality
```

The payload challenge remains:

```text
"G8P0" || 32 random bytes
```

This is the same G8 identity/challenge contract used by the other peer
transports.

## HCE setup

Before tapping devices, configure the HCE process:

```kotlin
AndroidNfcHceCourt.configure(
    nodeId = 200,
    peerKey = sharedPeerKey,
)
```

After a successful exchange:

```kotlin
val evidence = AndroidNfcHceCourt.evidence()
```

The evidence contains:

- authenticated peer project node ID;
- last verified G8 challenge.

## Reader setup

From a foreground Activity:

```kotlin
val reader = AndroidNfcG8Reader(
    activity = this,
    nodeId = 100,
    peerKey = sharedPeerKey,
)

reader.start { event ->
    when (event) {
        is AndroidNfcReaderEvent.Passed -> {
            // event.evidence.authenticatedPeerNodeId
            // event.evidence.challenge
            // event.evidence.maxTransceiveLength
        }
        else -> Unit
    }
}
```

Call `reader.destroy()` when the lab screen is finished so the key copy is
wiped and reader mode is disabled.

## Executable Recovery Lab court

The Recovery Lab exposes:

```text
Open NFC HCE / Reader physical G8 court
```

Run the same 32-byte laboratory peer key on two physical devices.

On the HCE device:

1. enter a project node ID;
2. enter the shared laboratory key;
3. tap `Configure HCE role`;
4. leave the court screen active.

On the Reader device:

1. enter a different project node ID;
2. enter the same key;
3. tap `Start Reader role`;
4. bring the devices into NFC range.

The HCE role does not report PASS immediately after authentication. It waits
until the reader has completed all authenticated benchmark probes.

### Authenticated APDU benchmark

After the normal encrypted G8 challenge/ACK, the reader performs:

```text
rounds        = 16
payload_bytes = 64
```

Each round:

1. builds a deterministic sequence-bound payload;
2. encrypts it as peer-session kind `0x60`;
3. sends it through the existing short-APDU frame command;
4. HCE decrypts and validates the sequence;
5. HCE returns the exact payload as encrypted kind `0x61`;
6. reader requires byte-for-byte equality.

The 64-byte useful payload remains below the 240-byte short-APDU project
ceiling after peer-session framing.

Reader evidence records:

- authenticated peer node ID;
- challenge;
- `IsoDep.maxTransceiveLength`;
- completed rounds;
- payload bytes;
- total benchmark elapsed time;
- min / median / p95 / max RTT;
- one-way useful bytes;
- round-trip useful bytes;
- one-way useful bits/s;
- round-trip useful bits/s.

HCE evidence records the authenticated peer/challenge and number of benchmark
frames completed.

These are application-level useful-throughput measurements. They are not raw
NFC controller bitrate or energy measurements.

## Security boundary

AID selection is routing only.

A malicious or unrelated NFC device can discover/attempt the AID, but a G8 pass
still requires successful Rust peer-session authentication using the project
key.

The carrier therefore does not trust:

- NFC tag identity;
- Android Tag object identity;
- AID alone;
- physical proximity alone.

## APDU budget

The prototype intentionally uses short APDUs with a project payload ceiling of
240 bytes.

That is enough for:

- the current peer-session handshake message;
- the encrypted G8 challenge/ACK;
- future tiny request/bootstrap capsules.

Large payloads should be fragmented or handed off to a higher-capacity carrier
after NFC bootstrap.

## Required evidence

Record on both devices:

- UTC timestamp;
- git commit;
- Android version;
- device model;
- local project node ID;
- authenticated peer project node ID;
- NFC/HCE feature report;
- challenge hex;
- reader max transceive length;
- result PASS/FAIL;
- benchmark rounds/payload size;
- RTT summary;
- useful-throughput summary;
- raw logcat around the tap.

The Reader and HCE challenge bytes must match.

## Closure boundary

CI can prove:

- APDU framing;
- AID registration/build;
- Android API compilation;
- shared JNI peer-session usage;
- G8 challenge wire format.

Only a two-device physical tap can prove:

- actual NFC controller compatibility;
- HCE routing on the OEM build;
- APDU timing;
- physical transceive limits;
- real Android-to-Android NFC interoperability.

Do not promote NFC into normal Recovery Mode until the physical court passes.
