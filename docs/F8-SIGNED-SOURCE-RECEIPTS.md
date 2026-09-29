# F8 Signed Source Receipts

The zero-carrier continuity layer already distinguishes cached remote data from
fresh remote truth. This baseline adds cryptographic source provenance so a
cached receipt can also prove that its metadata was signed by a specific
source key.

## Signature model

The implementation uses Ed25519.

A signed receipt binds all of the following fields:

- source ID;
- observation timestamp;
- SHA-256 of the exact cached bytes;
- provenance note.

The signed message is domain-separated with:

```text
SP3-SOURCE-RECEIPT-V1
```

and uses explicit lengths for variable text fields.

Changing the source ID, timestamp, content hash, or provenance note invalidates
the signature.

## Key boundary

Only the Ed25519 verifying key and signature are stored with a receipt.

The private signing key is never persisted by the continuity store.

A source adapter or trusted acquisition component is responsible for holding
the signing key and creating the receipt when it obtains the remote
observation.

## Cache admission

Unsigned receipts remain supported for sources that do not provide signatures.

If a receipt claims to be signed, both cache insertion and snapshot loading
require the signature to verify. A hash-valid object with tampered signed
metadata is rejected.

This prevents an invalid signature from being silently stored merely because
the cached bytes still match their SHA-256.

## Persistence

SP3S snapshots advance from version 1 to version 2.

Version 2 persists, per receipt:

- signature-present flag;
- Ed25519 verifying key;
- Ed25519 signature.

The loader accepts both:

- SP3S v1: unsigned historical snapshot;
- SP3S v2: optional signed receipts.

Signed receipts are re-verified after restart.

## Courts

The deterministic tests require:

- signed bytes + untouched metadata verify;
- changing source ID fails;
- changing observation timestamp fails;
- changing content fails before signature admission;
- changing provenance while retaining the same bytes is rejected on insert;
- a signed receipt survives snapshot/save/load and still verifies with the
  original public key.

## Evidence boundary

A valid signature proves that the holder of the corresponding private key
signed the receipt fields.

It does not prove that the signer itself is honest, that its clock is correct,
or that the remote observation was semantically true. Source-key trust and
multi-source reconciliation remain separate policy layers.
