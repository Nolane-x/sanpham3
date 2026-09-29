# F4 Correlated Failure-Domain Model

Multiple carrier APIs do not automatically mean multiple independent physical
paths.

Examples:

```text
BLE advertisement
BLE GATT
BLE L2CAP CoC
Bluetooth RFCOMM
```

all depend on the Bluetooth radio/controller. Treating four copies sent across
those APIs as four independent redundancy paths would overstate resilience.

The same applies to:

```text
Wi-Fi Direct
Wi-Fi Aware
Local-Only Hotspot
```

which share the Wi-Fi radio domain.

## Primary failure domains

The baseline maps current carrier kinds to a primary physical or operational
failure domain:

- Internet IP -> InternetPath
- Wi-Fi Direct / Aware / Local-Only Hotspot -> WifiRadio
- BLE advertisement / GATT / L2CAP / RFCOMM -> BluetoothRadio
- SMS -> CellularModem
- NFC HCE/Reader -> NfcController
- near-ultrasonic -> AudioTransducers
- screen-camera -> DisplayCamera
- vibration -> HapticsSensors
- magnetic -> MagnetometerPath
- USB -> UsbPath
- optional OS-exposed hardware -> ExternalInterface
- data mule -> HumanMobility
- local cache -> LocalState

This is a deterministic correlation class, not a probability estimate.

## Failure scenario

A `FailureScenario` marks one or more domains failed.

If `BluetoothRadio` fails, the model must reject all Bluetooth carrier
variants together while leaving unrelated Wi-Fi, optical, NFC, or other
domains unaffected.

## Redundancy selection

`choose_failure_diverse_carriers()` is a conservative greedy baseline.

The first copy uses the fastest supported candidate. Additional copies prefer
a previously unused primary failure domain before raw nominal bitrate.

Therefore, if the candidate set contains:

```text
BLE L2CAP   50 kbit/s
RFCOMM      40 kbit/s
BLE GATT    30 kbit/s
Wi-Fi       20 kbit/s
NFC          5 kbit/s
```

a three-copy redundancy plan should use one Bluetooth carrier, Wi-Fi and NFC
rather than three Bluetooth APIs.

## Court

Run:

```bash
cargo run -p virtual-phone-lab -- failure-domains
```

The court prints selected carriers, their failure domains and a simulated
Bluetooth-domain failure.

It runs on Linux and Windows in `carrier-frontier-lab`.

## Evidence boundary

This baseline models known shared physical primitives only.

It does not yet estimate:

- probability of each failure;
- partial degradation inside one radio;
- correlated OS/driver failures spanning multiple domains;
- energy cost;
- spatial common-mode failures;
- measured OEM-specific coupling.

Those require measurement or richer evidence rather than invented numeric
probabilities.
