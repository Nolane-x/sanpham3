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
