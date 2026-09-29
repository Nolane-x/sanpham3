# Optional external assistance policy

sanpham3 remains installable and useful without mandatory companion hardware.

That constraint does not mean Recovery Mode should ignore hardware the user already owns.

If an attached device is already exposed through a permitted operating-system driver or API, sanpham3 may treat it as another candidate information path. Examples include an already-present tether/network interface, USB network adapter, modem-like interface, or another user-owned device that the OS exposes as a legal transport.

## Hard boundary

Optional external assistance:

- must never become required for the base product;
- must never be counted as proof of the app-only claim;
- must be labeled separately in evidence and benchmarks;
- must still pass normal path measurement, trust, privacy, quota and user-consent policy;
- must not rely on bypassing platform security controls or unsupported radio access.

A detected interface is not automatically useful Internet. It remains a candidate until exact probing demonstrates what it can actually carry.

## Frontier model

carrier-frontier now has an ExternalOsInterface carrier class.

The simulator may use it when the device capability report says such an interface is actually present. The carrier is marked requires_extra_hardware and app_only_candidate=false, so it can improve recovery while remaining ineligible for the core app-only product proof.

This creates two honest claims:

1. Core app-only recovery: works without external hardware.
2. External-assisted recovery: opportunistically becomes stronger when extra OS-exposed hardware is already available.

The second claim may expand capability, but it can never silently replace the first.
