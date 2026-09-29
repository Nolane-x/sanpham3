# F8 Persistent Zero-Carrier Continuity Store

Zero-carrier continuity must survive an application restart if it is going to
be useful during a real outage.

The in-memory `ContinuityStore` now has a bounded, versioned snapshot
baseline.

## Snapshot format

A snapshot begins with:

```text
SP3S
version
entry_count
```

Each entry stores:

- cache key;
- source ID;
- provenance note;
- object byte length;
- source observation timestamp;
- source validity horizon;
- original SHA-256 receipt;
- exact cached bytes.

The format is deterministic: entries are serialized in lexical key order.

## Load boundary

`PersistenceLimits` bounds:

- total snapshot file bytes;
- entry count;
- individual object bytes.

The loader checks these limits before accepting the store and verifies every
object against its stored source receipt SHA-256.

A modified cached payload therefore fails load with `ReceiptMismatch`.

## Source validity

The existing `CachedObject.valid_for` horizon is now enforced by
`resolve_zero_carrier()`.

A caller cannot extend a source-defined five-second validity horizon merely by
asking for a sixty-second cache age.

Both conditions must hold:

```text
object age <= caller max_cache_age
AND
object age <= source valid_for
```

## Eviction

`evict_to_budget()` applies deterministic policy until both entry-count and
payload-byte budgets are satisfied:

1. source-invalid entries first;
2. older remote observations next;
3. lexical key order as a deterministic tie-break.

This is not LRU. It deliberately prefers provenance/freshness semantics over
pretending access recency is available when the store does not yet record it.

## Persistence behavior

`save_snapshot()` writes and `sync_all()`s a sibling temporary file before
replacement.

On systems where rename cannot replace an existing destination, the
implementation falls back to remove+rename.

Therefore the current claim is:

```text
bounded durable snapshot baseline
```

not:

```text
universally crash-atomic transactional database
```

A later production store may use platform-specific atomic replace or an
append/journal scheme.

## Courts

Cross-platform zero-carrier tests verify:

- snapshot -> restart/load -> exact receipt-preserving roundtrip;
- tampered cached bytes fail receipt verification;
- source-invalid entries are evicted before still-valid data;
- oldest valid observation is evicted next;
- source validity is stricter than a caller-provided cache-age allowance.

## Evidence boundary

Persistent cached data is still `CachedRemote`.

Loading it after restart does not make it current remote truth, and a
`require_current_remote_observation` contract must still fail when no carrier
exists.
