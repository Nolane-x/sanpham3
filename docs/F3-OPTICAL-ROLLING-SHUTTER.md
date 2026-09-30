# F3 Optical Rolling-Shutter / PWM Banding Court

The optical raster court already models cell rendering, perspective,
blur and exposure/gamma.

This baseline adds a first-order rolling-shutter/display-PWM interaction model
as row-dependent exposure banding.

## Model

`OpticalRollingShutterBanding` defines:

- row period;
- minimum row exposure;
- phase offset.

For each raster row the model computes a deterministic periodic exposure factor
between `minimum_exposure` and 1.0 and applies it to the complete row.

The reference profile uses:

```text
period_rows      = 24
minimum_exposure = 0.72
phase_rows       = 5
```

This approximates camera row scanning interacting with display brightness/PWM.
It is not a full temporal camera sensor simulation.

## Combined court

The optical synthetic path now executes:

```text
logical bits
 -> repetition symbols
 -> grayscale cell raster
 -> perspective/keystone warp
 -> rolling-shutter/PWM row banding
 -> box blur
 -> exposure/gamma
 -> projected cell sampling
 -> repetition decode
```

The reference payload must still decode with zero BER.

Additional tests verify:

- deterministic output;
- frame dimensions are preserved;
- different rows receive different exposure;
- exposure remains inside configured bounds.

## Evidence boundary

This closes a deterministic **rolling-shutter banding baseline**.

It does not close:

- measured camera scan timing;
- real display PWM frequency/phase;
- motion-induced rolling-shutter geometry;
- autofocus;
- automatic exposure adaptation;
- camera-video ingestion/replay;
- physical screen-to-camera range/goodput/energy.

Those remain separate replay/physical requirements before F7 promotion.
