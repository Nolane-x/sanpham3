# F3 Acoustic AGC + Nonlinear Processing Court

The near-ultrasonic court already modeled gain/noise, multipath and sampling
clock drift.

This baseline adds two deterministic device-processing impairments commonly
encountered before/inside a phone audio pipeline:

- automatic gain control (AGC);
- bounded nonlinear amplitude compression.

## AGC model

`apply_acoustic_agc()` operates on fixed sample windows.

For each window it:

1. measures RMS;
2. computes the gain required to move toward a target RMS;
3. clamps the desired gain to configured min/max bounds;
4. smooths gain changes across windows;
5. clamps output to normalized audio bounds.

Reference configuration:

```text
window_samples = 240
target_rms     = 0.30
min_gain       = 0.40
max_gain       = 2.50
smoothing      = 0.25
```

This is a deterministic research AGC, not a model of any specific OEM DSP.

## Nonlinear model

`apply_acoustic_nonlinearity()` applies cubic compression:

```text
y = x - k*x^3
```

followed by an explicit clip bound.

Reference configuration:

```text
cubic_compression = 0.18
clip_level        = 0.92
```

The parameter range is bounded so invalid/extreme configurations fail rather
than silently creating meaningless waveforms.

## Combined court

The synthetic acoustic path now executes:

```text
FSK
 -> multipath impulse response
 -> sampling-clock drift
 -> RMS-window AGC
 -> cubic nonlinear compression
 -> gain + white noise + clipping
 -> correlation decoder
```

The reference payload must still finish with zero bit errors.

The dedicated tests also verify:

- AGC moves a constant-amplitude signal toward the configured target RMS;
- nonlinear processing is deterministic;
- nonlinear output never exceeds its configured clip level.

## Evidence boundary

This closes deterministic software baselines for:

- AGC-like dynamic gain;
- bounded nonlinear amplitude distortion.

It does not close:

- measured OEM AGC behavior;
- frequency-selective microphone/speaker response;
- echo cancellation;
- noise suppression;
- codec/resampler pipelines;
- recorded handset audio replay;
- physical useful bits/s or range.

Recorded device traces and physical measurements remain required before F7
promotion.
