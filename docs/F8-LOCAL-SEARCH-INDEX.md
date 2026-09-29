# F8 Local Cache Search Index

Zero-carrier continuity can be useful only if locally stored information is
discoverable without pretending that local search is Internet access.

This baseline adds a deterministic in-memory inverted index over cached UTF-8
objects.

## Indexed material

The index tokenizes:

- cached UTF-8 content;
- logical cache key.

Binary/non-UTF-8 objects are skipped.

Tokenization uses Unicode alphanumeric runs and lowercasing.

## Provenance and freshness filtering

Index membership alone is never enough to return a hit.

At query time every candidate is checked against the current store:

- indexed digest must still match the current cached object's digest;
- exact bytes must still match the receipt SHA-256;
- source validity must still hold;
- object age must be within the search policy max age;
- when required, the source receipt signature must still verify.

This means an index built before a cache replacement cannot return a stale
posting for the replaced object. The old posting is ignored until the index is
rebuilt.

## Ranking

Hits are ordered deterministically by:

1. number of matched distinct query terms, descending;
2. newer observation timestamp;
3. lexical cache key.

The caller provides a max-result limit.

Every hit is labeled `CachedRemote`.

Local search never produces `FreshRemote`.

## Courts

The tests require:

- a two-term query finds the matching cached document;
- stale entries are filtered;
- unsigned entries are filtered when signed provenance is required;
- replacing an object's digest invalidates stale postings in an old index;
- rebuilding exposes the new content;
- binary/non-UTF-8 objects are skipped.

## Evidence boundary

This is a lightweight exact-token local search baseline.

It does not provide:

- semantic/vector retrieval;
- fuzzy spelling correction;
- ranking learned from user behavior;
- search over data that was never previously acquired;
- fresh Internet results.

Those are separate capabilities and must not be conflated with continuity
search.
