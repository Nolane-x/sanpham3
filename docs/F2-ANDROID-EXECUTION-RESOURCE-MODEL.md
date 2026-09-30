# F2 Android Execution, OEM Quirk and Resource Model

This model makes Recovery Mode sensitive to execution state instead of assuming
that a permission-granted phone can always use every carrier.

It is deliberately conservative and only removes capabilities.

## Three independent inputs

Effective Android carrier availability starts from the normal API/hardware/
permission model, then applies:

1. execution state;
2. OEM quirk profile;
3. battery/thermal resource policy.

No execution/OEM/resource rule can enable a capability that the base Android
model did not already expose.

## Execution profiles

`AndroidExecutionProfile` models:

- app foreground/background state;
- background restriction;
- deep Doze;
- battery saver.

Reference presets:

- `foreground()`;
- `restricted_background()`;
- `deep_doze()`.

In the conservative background model, Wi-Fi peer discovery/hotspot-style
capabilities are removed when the app is background-restricted.

## OEM quirk profiles

`AndroidOemQuirkProfile` intentionally avoids vendor names.

The model contains independent switches for:

- unreliable background Wi-Fi peer operations;
- unreliable background Bluetooth;
- unreliable Local-Only Hotspot;
- peer radios becoming unreliable in deep Doze.

Two presets exist:

- `reference()` — no extra quirks;
- `aggressive_background()` — all conservative quirk switches enabled.

These are research profiles, not claims about a specific manufacturer.

## Battery and thermal model

`AndroidResourceState` records:

- battery percentage;
- charging state;
- thermal level:
  - Nominal;
  - Warm;
  - Hot;
  - Critical.

The deterministic policy maps state to:

- `Normal`;
- `Conserve`;
- `Critical`.

Reference policy:

```text
Critical:
  thermal == Critical
  OR battery <= 3% while not charging

Conserve:
  thermal >= Hot
  OR battery saver enabled
  OR battery <= 10% while not charging
```

Charging prevents low battery percentage alone from forcing Conserve/Critical,
but thermal pressure still applies.

Battery values above 100 are rejected.

## Carrier reductions

Conserve mode suppresses the active nontraditional transducer candidates first:

- acoustic input;
- camera optical input;
- vibration output.

Critical mode additionally suppresses:

- Wi-Fi Direct;
- Wi-Fi Aware;
- Local-Only Hotspot;
- BLE L2CAP CoC;
- Bluetooth Classic/RFCOMM.

USB is intentionally not disabled solely by the reference low-battery policy.

These choices are conservative **project policy defaults**. They are not
physical power measurements and should later be tuned from F4/F7 evidence.

## Court

Run:

```bash
cargo run -p virtual-phone-lab -- android-execution-matrix
```

The court requires:

- clean foreground/reference/nominal state exactly equals base capabilities;
- restricted background + aggressive quirks + hot/low-battery state removes
  Wi-Fi/Bluetooth and active transducer carriers;
- critical resource mode removes the configured high-cost carriers;
- USB remains available when it was otherwise valid;
- constraints never promote a base-false capability to true.

The court runs on Linux and Windows CI because the model itself is deterministic
Rust logic.

## Evidence boundary

This closes the F2 **OEM quirk profile**, **background restriction profile** and
**battery/thermal model** gates.

It does not claim:

- exact OEM behavior;
- exact Android scheduler timing;
- measured power cost;
- real Doze radio behavior on a handset;
- Android 17 local-network permission semantics.

Physical/OEM validation and Android 17 permission modeling remain separate.
