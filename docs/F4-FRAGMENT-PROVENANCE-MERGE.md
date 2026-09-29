# F4 Fragment Source / Provenance Merge Baseline

Bit scavenging can receive different fragments of the same exact object from
different peers and carriers.

The fragment layer now preserves that origin metadata while keeping the exact
SP3F reconstruction contract unchanged.

## Per-fragment provenance

Each accepted fragment can be accompanied by:

- source ID;
- carrier label;
- observation timestamp.

Empty source/carrier labels are rejected.

The provenance wrapper first authenticates and validates the SP3F fragment,
then records provenance only if the fragment is accepted as part of the same
transfer.

## Merge rules

Fragments from different sources may contribute to one transfer only when the
existing fragment invariants agree:

- transfer ID;
- total length;
- whole-object SHA-256;
- non-overlapping byte ranges.

A fragment from another transfer is still rejected with
`TransferMismatch`.

An exact duplicate fragment from a second source is idempotent for data bytes,
but the additional source/carrier provenance is retained.

## Reconstruction summary

After exact reconstruction, the provenance summary reports:

- sorted unique source IDs;
- sorted unique carrier labels;
- first and last observation timestamp;
- number of fragment offsets tracked;
- number of offsets observed from more than one distinct source.

The reconstructed byte object must still pass the original whole-object
SHA-256.

## Court

The F4 court now runs an additional provenance case over a real URT wire
object.

Fragments are delivered in reverse order across:

- BLE GATT;
- Wi-Fi Direct;
- acoustic.

One exact fragment is then observed again through a fourth source over NFC.

The court requires:

- exact URT wire reconstruction;
- four unique sources;
- all carrier labels retained;
- one multi-source duplicate offset;
- no change to the reconstructed bytes.

PASS begins with:

```text
F4_PROVENANCE_PASS
```

## Trust boundary

Fragment bytes are authenticated by the existing SP3F HMAC.

The provenance metadata itself is supplied by the caller and is not
cryptographically signed by this baseline.

Therefore this closes source/carrier provenance *merge and preservation*, not
a claim that arbitrary provenance labels are independently trustworthy.
