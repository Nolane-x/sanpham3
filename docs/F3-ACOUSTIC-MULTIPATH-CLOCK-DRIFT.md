# F3 Acoustic Multipath + Clock-Drift Court

The near-ultrasonic synthetic carrier previously modeled only gain, white
noise and clipping. That was useful as a clean signal baseline but too simple
to represent two common real-room impairments:

- delayed reflected copies of the waveform;
- transmitter/receiver sampling-clock mismatch.

## Causal impulse response

`apply_acoustic_impulse_response()` convolves the generated FSK waveform with
a deterministic set of delayed taps.

Each tap has:

- integer sample delay;
- signed gain.

The implementation is causal and preserves the original nominal sample count.
It clamps the resulting synthetic signal to the normalized [-1, 1] range.

The reference court uses:

```text
direct path:   delay 0   gain  0.72
reflection 1:  delay 7   gain  0.16
reflection 2:  delay 19  gain -0.07
```

These values are research fixtures, not measured room claims.

## Clock drift

`apply_clock_drift_resampling()` models receiver sampling-rate mismatch using
deterministic linear interpolation.

Positive ppm advances through the source waveform faster. Negative ppm advances
more slowly.

The research API currently bounds drift to ±20,000 ppm to avoid accepting
obviously nonsensical configurations.

The reference court uses +80 ppm.

## Combined court

The acoustic synthetic court now applies:

```text
FSK encode
 -> multipath impulse response
 -> +80 ppm resampling drift
 -> gain + white noise + clipping
 -> correlation decode
```

The reference payload must still decode with zero bit errors.

This court runs in the existing `carrier-frontier-lab` matrix on Linux and
Windows.

## Evidence boundary

This closes deterministic synthetic baselines for:

- multipath impulse-response convolution;
- clock drift / resampling.

It does not close:

- measured real-room impulse response replay;
- microphone/speaker AGC;
- OEM nonlinear filters;
- device frequency response;
- physical near-ultrasonic range or useful bits/s.

Those still require recordings and physical-device measurements.
