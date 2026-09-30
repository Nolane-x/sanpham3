# F3 Optical Raster / Camera-Like Court

The original optical court operated only on repeated logical symbols with
synthetic drops and sparse bit flips.

This baseline moves the signal court one layer closer to an actual
screen-camera path by introducing a real grayscale raster.

## Raster renderer

`render_optical_cells()` maps repeated optical symbols into a row-major cell
grid.

The reference configuration uses:

- 16 columns;
- 10x10 pixels per symbol cell;
- a two-cell quiet zone;
- separate grayscale levels for logical 0 and 1;
- a decoder threshold plus erasure margin.

The renderer produces an explicit `OpticalGrayFrame { width, height, pixels }`
rather than a logical bit vector.

## Perspective baseline

`warp_optical_perspective()` applies a deterministic trapezoid/keystone
projection.

The scale changes from the top row to the bottom row and can include horizontal
shift.

The decoder court is given the same synthetic geometry and projects canonical
cell centers into the distorted raster before sampling.

Therefore this closes a perspective-transform impairment baseline, not
automatic corner detection or camera pose estimation.

## Blur and photometric model

The raster then passes through:

1. bounded box blur;
2. exposure scaling;
3. gamma transfer.

All pixels remain normalized grayscale values.

The reference court uses:

```text
blur radius = 1 pixel
exposure    = 0.88
gamma       = 1.15
```

## Decoder

`decode_optical_cells()` samples a small neighborhood around each projected
cell center.

If the mean value falls inside the configured decision margin around the
threshold, the symbol is emitted as an erasure rather than guessed.

The existing repetition decoder then reconstructs the original logical bits.

## Court pipeline

```text
logical bits
 -> repetition symbols
 -> grayscale cell raster
 -> perspective warp
 -> box blur
 -> exposure/gamma
 -> projected cell sampling
 -> repetition decode
 -> exact logical bits
```

The deterministic reference payload must finish with zero bit errors.

The court runs in `carrier-frontier-lab` on Linux and Windows through:

```text
cargo run -p virtual-phone-lab -- optical-synthetic
```

## Evidence boundary

This baseline closes software/synthetic evidence for:

- actual pixel/cell raster generation;
- deterministic perspective/keystone transformation;
- blur;
- exposure/gamma;
- raster cell sampling under known synthetic geometry.

It does not close:

- automatic finder/corner detection;
- unknown projective geometry estimation;
- rolling-shutter distortion;
- motion blur;
- camera autofocus;
- screen PWM/flicker;
- real camera-video replay;
- physical phone-to-phone range, setup latency, useful bit rate or energy.

Those remain separate research and physical-measurement requirements.
