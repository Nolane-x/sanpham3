# F8 Multi-Source Conflict / Reconciliation Baseline

Cached remote observations can disagree.

This baseline does not merge conflicting remote values and does not guess which
source is correct. It implements a conservative exact-byte quorum over
independent source identities.

## Admission before reconciliation

A candidate is eligible only if:

- its cached bytes still match the receipt SHA-256;
- the source-defined validity horizon has not expired;
- its age is within the reconciliation policy's max age;
- when the policy requires signatures, the source receipt has a valid Ed25519
  signature.

Candidates failing any of these checks do not vote.

## One source, one vote

A source ID contributes at most one vote.

If the same source ID provides multiple observations with the same digest, only
its newest admissible observation is retained.

If the same source ID provides different digests in the same reconciliation
set, that source is treated as internally conflicting and is excluded from the
quorum entirely.

This prevents repeated observations from one source from pretending to be
independent evidence.

## Exact grouping

The remaining candidates are grouped by exact content SHA-256.

A result is accepted only when:

1. the highest-support digest reaches `min_distinct_sources`; and
2. exactly one digest has that highest support.

If the best support is below quorum, the result is
`InsufficientConsensus`.

If multiple different digests tie for the highest support, the result is an
explicit `Conflict`.

## Output

A successful `ReconciledObservation` includes:

- exact selected bytes;
- selected SHA-256;
- sorted agreeing source IDs;
- oldest and newest observation timestamps among the agreeing sources;
- `CachedRemote` freshness.

Reconciliation never promotes cached data to `FreshRemote`.

## Courts

Tests require:

- two independent signed sources beat one dissenting source;
- 1-vs-1 exact disagreement returns `Conflict`;
- repeated observations from one source cannot satisfy a two-source quorum;
- a source that contradicts itself is excluded;
- unsigned candidates are excluded when signed provenance is required;
- candidates older than the reconciliation max age are excluded.

## Evidence boundary

This is an exact-byte consensus baseline.

It does not yet provide:

- semantic merging of partially overlapping facts;
- source reputation or authority scoring;
- Byzantine fault-tolerance guarantees;
- cross-source clock synchronization;
- proof that a majority is semantically correct.

Those need separate trust and semantic policies rather than being hidden inside
the cache layer.
