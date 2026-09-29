# F4 Authenticated Rateless Random-Linear Coding Baseline

The existing SP3E parity stripe can recover one missing shard per stripe, but
its repair budget is fixed when the transfer is created.

SP3R adds a rateless software baseline: the sender can continue generating new
authenticated repair symbols until the receiver has enough independent
equations to reconstruct the object.

## Coding model

The object is split into fixed-size source shards determined by the wire
budget.

SP3R is systematic:

- `symbol_id < K` carries one source shard directly;
- `symbol_id >= K` carries a deterministic random-linear XOR equation over
  the K source shards.

Repair coefficients are regenerated from:

- a domain-separated coefficient seed;
- transfer ID;
- symbol ID.

The coefficient vector is therefore not transmitted, keeping the wire header
bounded.

The sender does not declare a fixed repair count. It can keep increasing
`symbol_id`.

## SP3R envelope

Each symbol carries:

- SP3R magic/version;
- transfer ID;
- total object length;
- whole-object SHA-256;
- source shard count;
- shard payload size;
- 64-bit symbol ID;
- encoded symbol payload;
- HMAC-SHA256 authentication truncated to 128 bits.

The payload size is selected only after accounting for the full SP3R envelope
overhead.

The current baseline bounds one transfer to at most 256 source shards.

## Decoder

The receiver:

1. authenticates each symbol;
2. rejects transfer-descriptor mismatch;
3. regenerates the coefficient vector;
4. incrementally reduces the equation over GF(2);
5. tracks innovative rank;
6. ignores exact duplicate symbols;
7. accepts linearly dependent but consistent symbols without increasing rank;
8. reconstructs source shards by back substitution once rank reaches K;
9. truncates source padding to the exact total length;
10. requires the original whole-object SHA-256.

A zero-coefficient equation with non-zero payload is rejected as inconsistent.

## Court

The F4 court uses the real URT exact packet.

It:

- sends the systematic symbol range but deliberately loses every fourth source
  symbol;
- confirms the receiver is still incomplete;
- begins generating repair symbols beyond K;
- deliberately loses additional repair symbols;
- keeps increasing symbol IDs until receiver rank reaches K;
- reconstructs the exact URT wire;
- decodes URT back to the original logical bytes.

PASS begins with:

```text
F4_RATELESS_PASS
```

The court records source shard count, generated symbols, delivered symbols,
lost symbols and final rank.

## Evidence boundary

This is an authenticated deterministic random-linear rateless software
baseline.

It is not claimed to be an optimized LT/Raptor implementation. It does not yet
optimize:

- coefficient degree distribution;
- sparse decoding complexity;
- feedback/ACK policy;
- CPU/energy cost at very large K;
- physical-loss-specific coding rate.

The important closed invariant is that repair capacity is no longer fixed:
additional independent symbols can continue to be generated until exact
reconstruction becomes possible.
