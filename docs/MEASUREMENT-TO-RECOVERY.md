# Measurement-to-Recovery Pipeline

This document records the first end-to-end desktop recovery decision path.

## Pipeline

```text
OS adapter
  -> exact per-path Tiny HTTPS probe
  -> repeated ProbeSeriesSummary
  -> MeasuredPathEvidence
  -> LinkObservation
  -> ConnectivityGraph
  -> adaptive RecoveryPlan
  -> live mode or DTN scheduling
```

## What is measurement

Measured values include:

- successful/failed app-layer attempts;
- loss ppm from repeated attempts;
- useful response bytes;
- observed useful bit rate;
- median/p95 completion time;
- intermittency.

## What is policy

`energy_cost` and unknown billing state are not inferred from packet timing.

When platform-specific billing/energy data is unavailable, the core uses explicit conservative scheduling defaults by transport. These are policy values, not measurements.

Current conservative defaults treat cellular and satellite as metered until better product/platform evidence overrides that choice.

## Link state conversion

A successful stable series becomes `Up`.

A successful series with transitions/high loss becomes `Intermittent`.

A failed series becomes `Down` with zero usable bitrate and 100% loss for routing purposes.

## RTT choice

The bridge prefers p95, then median, then minimum successful latency. This is deliberately conservative for recovery planning.

## Host probe CLI

With an exact HTTPS target configured, `host-probe-cli` now prints:

1. platform capabilities;
2. Recovery Ledger records;
3. measured graph links;
4. adaptive plan for the current compact 232-byte reference task.

This allows one executable to expose the entire software decision chain instead of requiring an operator to mentally combine unrelated logs.

## Evidence boundary

The graph link is only as trustworthy as the probe evidence that produced it.

Conservative scheduling defaults must not be reported as measured battery cost, billing state, RF strength, or physical throughput.

Android currently produces equivalent path evidence inside its native adapter. Cross-language integration with the shared Rust graph/runtime remains a separate G8/productization task.