# F3 Vibration Resonance + Mount-Profile Court

The original vibration court modeled OOK plus one shared-surface gain/noise
channel.

This baseline separates two additional physical effects:

- mechanical resonance / delayed coupling;
- orientation/contact/body-dependent attenuation and noise.

## Mechanical impulse response

`apply_mechanical_impulse_response()` applies deterministic causal taps.

The reference court uses:

```text
delay 0   gain  0.90
delay 3   gain  0.10
delay 8   gain -0.03
```

These are synthetic fixtures, not measured table/phone coefficients.

The court verifies the convolution is causal and deterministic.

## Mount profiles

`VibrationMountProfile` provides bounded signal/noise scaling presets:

```text
flat_table
edge_contact
handheld
```

The baseline intentionally orders them from stronger/cleaner coupling toward
weaker/noisier coupling.

They model broad research cases only. They are not OEM/device-specific profiles.

## Combined court

The reference synthetic path now runs:

```text
OOK accelerometer waveform
 -> mechanical impulse response
 -> flat-table mount profile
 -> gain + deterministic white noise
 -> RMS decoder
```

The reference payload must still reconstruct at zero BER.

## Evidence boundary

This closes deterministic software baselines for:

- resonance/impulse-response effects;
- coarse orientation/contact/body/table profiles.

It does not close:

- measured impulse responses;
- real phone chassis/haptic transfer functions;
- actual orientation classification;
- human motion traces;
- sensor quantization/filtering;
- recorded accelerometer replay;
- physical useful bits/s, range or energy.

Those remain physical/replay requirements before F7 promotion.
