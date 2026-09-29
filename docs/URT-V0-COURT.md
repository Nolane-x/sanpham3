# URT v0 — exact reconstruction court

URT is the payload-side companion to the sanpham3 connectivity engine.

sanpham3 asks: what legal path can still move information?

URT asks: what is the cheapest valid representation that fits through that path?

This first implementation intentionally covers only the exact-data foundation. It does not claim universal 1000x compression.

## Implemented exact strategies

The encoder compares:

1. raw bytes;
2. run-length encoding;
3. deterministic repeated-pattern programs;
4. exact cache references when the receiver already owns the object;
5. prefix/suffix base delta when the receiver owns a known base object.

The selected representation is always wrapped in a versioned URT packet carrying the original length, SHA-256 of the required exact output, strategy identifier, bounded payload length and strategy payload.

Decoding succeeds only when the reconstructed bytes hash to the original digest.

## Scientific accounting

A cache hit or base delta reports shared_state_bytes separately from network_bytes.

Therefore a 1 GiB logical object where the receiver already has almost all bytes is treated as shared-state-assisted transport, not as magical standalone compression.

Arbitrary high-entropy-like input falls back to raw when no implemented transform wins.

## Resource boundary

The primary decoder API can stream to an arbitrary writer. It enforces maximum output bytes, maximum decode-operation budget and maximum extra scratch memory.

The convenience materializing decoder refuses outputs larger than its working-memory budget.

## Deterministic court

urt-lab-cli proves four cases on Ubuntu and Windows:

- highly repetitive exact data;
- exact small edit against shared base;
- exact cache hit;
- high-entropy-like fallback.

Every case reconstructs byte-for-byte before the court passes.

The lab also projects serialization time at 10 bit/s so byte savings can be compared directly with the existing weak-link ladder.

## Next research layers

This v0 is a foundation, not the end state. The next useful strategies are content-defined chunking, cross-file deduplication, multi-base delta selection, strong conventional lossless baselines, a bounded reconstruction VM, transport-aware representation selection from live path budgets, and separately typed media Q4 to Q0 plus semantic-survival modes.

Exact and lossy or survival claims must remain distinct.


## V1 conventional-lossless baseline

URT now also evaluates a pure-Rust Zstandard frame as one exact candidate.

This is deliberately not presented as a new compression invention. Zstandard is the mature conventional lossless baseline that program, cache and delta representations must beat when they claim a network-byte win.

URT constrains frames it emits to a 1 MiB match window and rejects a received URT Zstandard representation whose declared decode window exceeds that limit. The Zstandard path remains inside the same outer URT SHA-256 exact-output contract.

Selection remains competitive: raw, RLE, repeat-program, Zstandard, cache-reference and base-delta candidates are compared, and the smallest implemented exact payload wins. Shared-state-assisted results remain separately accounted.


## V2 content-defined chunk reuse

URT now has an exact `ChunkManifest` strategy for receiver-side shared state
that is more general than one whole-file base delta.

The chunker uses a deterministic rolling 48-byte fingerprint and enforces:

```text
minimum chunk = 2 KiB
target average ≈ 8 KiB
maximum chunk = 32 KiB
```

Because the fingerprint is rolling across the byte stream rather than reset
from every previous boundary, a local edit perturbs boundaries only around the
edited region and can resynchronize after the rolling window. This is the
property needed for useful dedup across distributed edits.

Each manifest record carries:

```text
chunk length
embedded/cache flag
full SHA-256 chunk digest
embedded bytes only when the receiver lacks the chunk
```

Cached chunks are reconstructed only after their length and SHA-256 are
verified. The outer URT SHA-256 still verifies the complete reconstructed
object.

### Accounting

`shared_state_bytes` counts only logical bytes actually reused from the
receiver cache. Those bytes are never presented as standalone compression.

Manifest metadata and every missing chunk are included in
`network_bytes`.

### Cache indexing

`index_exact_object()` stores both the whole-object digest and deterministic
CDC chunks. That allows the same receiver cache to support:

- whole-object cache references;
- ordinary base delta;
- content-defined chunk reuse;
- later cross-file / cross-version reuse.

`encode_exact_with_cache()` can select ChunkManifest even when there is no
single designated base object, which is the foundation for cross-file dedup.

### Courts

V2 adds deterministic cases for:

- many small edits spread through a 1 MiB high-entropy-like base;
- reconstruction from a chunk cache without a whole-file base;
- carrying a CDC-assisted exact object through the existing 100 bit/s
  weak-link simulator.

The court must still reconstruct byte-for-byte before PASS.
