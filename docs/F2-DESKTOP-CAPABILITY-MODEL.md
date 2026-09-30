# F2 Windows / Linux Capability and Access Model

This model prevents the frontier scheduler from treating theoretical OS or
hardware capability as an implemented sanpham3 carrier.

For desktop platforms, effective carrier availability is the intersection of:

```text
physical hardware
AND
OS/user permission or privilege
AND
a carrier adapter actually implemented by sanpham3
```

A carrier is unavailable when any of the three layers is false.

## Shared hardware profile

`DesktopHardwareProfile` records device-side primitives such as:

- Wi-Fi Direct-capable radio;
- BLE / Bluetooth Classic;
- microphone / speaker;
- camera / screen;
- sensors;
- USB;
- an already-present OS network interface.

The reference `broad_laptop()` is intentionally generous about common laptop
hardware. Hardware presence alone never enables a carrier.

## Project adapter profile

`DesktopProjectAdapters` records which special carrier paths sanpham3 has
actually implemented.

Current Windows and Linux profiles are conservative.

The merged desktop host adapters already inventory, bind and probe ordinary OS
network interfaces, including an optional user-owned network interface exposed
normally by the OS. Therefore:

```text
external_os_interface = true
```

Current desktop profiles keep these special paths disabled until dedicated
project adapters exist:

- Wi-Fi Direct;
- Wi-Fi Aware;
- BLE/GATT/L2CAP;
- RFCOMM;
- local-only hotspot;
- SMS;
- NFC;
- acoustic;
- optical;
- vibration;
- magnetometer;
- USB peer-session.

This is deliberate even when the machine hardware could theoretically support
one of those primitives.

## Windows permission profile

`WindowsPermissionProfile` separately models access to:

- Wi-Fi peer operations;
- Bluetooth;
- audio capture/playback;
- camera/display;
- sensors;
- USB devices;
- external OS interfaces.

`VirtualWindowsDevice::capabilities()` intersects hardware, permission and
project adapter state.

## Linux privilege profile

`LinuxPrivilegeProfile` models user/session access to:

- Wi-Fi peer control;
- Bluetooth;
- audio capture/playback;
- video capture/display;
- sensors;
- USB devices;
- external OS interfaces.

`VirtualLinuxDevice::capabilities()` applies the same three-way intersection.

The word "privilege" here is a project model. It does not claim every Linux
distribution uses the same daemon, group or authorization mechanism.

## Current-project truth boundary

A broad laptop with every modeled permission granted still does **not** expose
unimplemented sanpham3 special carriers.

The deterministic court requires:

```text
Windows current-project:
  wifi_direct=false
  bluetooth=false
  acoustic=false
  optical=false
  usb_peer=false
  external_interface=true

Linux current-project:
  wifi_direct=false
  bluetooth=false
  acoustic=false
  optical=false
  usb_peer=false
  external_interface=true
```

A second court enables research adapter flags but denies Windows audio-input and
USB access. Acoustic and USB must remain unavailable while optical stays
available when its modeled camera/display access is still granted.

## CI

Run:

```bash
cargo run -p virtual-phone-lab -- desktop-matrix
```

The court runs on both Linux and Windows GitHub runners as part of
`carrier-frontier-lab`.

## Evidence boundary

This closes the F2 **Windows capability/permission profile** and **Linux
capability/privilege profile** modeling gates.

It does not close implementation gates for desktop BLE, Wi-Fi Direct, USB peer
transport, audio/optical carriers or OS-specific permission UX.

Those remain separate adapter/product work and cannot be promoted merely by
turning a model flag on.
