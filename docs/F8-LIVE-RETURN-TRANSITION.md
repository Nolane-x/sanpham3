# F8 Automatic Live-Return Transition Baseline

Zero-carrier continuity and live remote access must keep different truth labels.

The zero-carrier resolver still cannot produce `FreshRemote`.

This baseline adds a small runtime state machine above it so the product can
switch back to a live result when an actual carrier returns, refresh the local
cache, and then fall back to that newer cached result if connectivity is lost
again.

## Runtime states

```text
LocalOnly
LiveRemote
```

The runtime begins in `LocalOnly`.

## Live observation

A `LiveRemoteObservation` contains:

- logical cache key;
- exact remote bytes;
- source receipt;
- source validity horizon.

When a live observation is supplied:

1. its logical key must match the request;
2. its content hash must match the receipt;
3. if the receipt is signed, its Ed25519 signature must verify;
4. the observation is inserted/replaced in the continuity cache;
5. the runtime switches to `LiveRemote`;
6. the answer is returned as `FreshRemote`.

Freshness comes from the live-carrier pipeline, not from the cache.

## Carrier loss

When no live observation is available:

1. the runtime switches to `LocalOnly`;
2. it delegates to `resolve_zero_carrier()`;
3. the answer can only be `CachedRemote` or `LocallyGenerated`.

If a fresh observation had previously refreshed the cache, the newer bytes and
their provenance are what the local-only path serves.

## Court

The deterministic transition court runs:

```text
old cached "cloudy"
  -> no live carrier
  -> CachedRemote("cloudy")
  -> live signed observation "sunny"
  -> FreshRemote("sunny")
  -> carrier disappears again
  -> CachedRemote("sunny")
```

It also verifies:

- `require_current_remote_observation` fails with no live observation;
- the same contract succeeds when a valid live observation exists;
- wrong-key live observations are rejected;
- receipt/content mismatch is rejected before cache refresh.

## Evidence boundary

This is the continuity-state transition baseline.

It does not itself discover or establish a carrier. The recovery/connectivity
layers must supply a real live observation.

Therefore this closes the cache transition semantics, not physical G9 or any
carrier-specific recovery claim.
