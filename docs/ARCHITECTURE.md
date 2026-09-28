# Architecture v0.1

## Core invariant

The engine may report `LOCAL_ONLY` only after every connectivity capability that the current platform is permitted to use has either been tested, is unavailable, or has an explicit reason it cannot be attempted.

This is not a promise that ordinary apps can control every radio. OS, driver, permission, carrier and hardware restrictions are first-class facts.

## Recovery planes

### 1. Probe plane

Discovers local interfaces, radio/API capabilities, peer transports and path health. It produces observations, not routing decisions.

### 2. Capability graph

Represents devices and directed observed links. A peer may be a relay without being an Internet egress. An egress may be slow, lossy, intermittent or metered.

### 3. Policy plane

Selects paths by traffic class. Tiny semantic traffic may use links that are useless for interactive or bulk traffic.

### 4. Cooperative relay plane

Moves project capsules across consenting peers. The first design is application-level relay, not an unauthenticated open proxy.

### 5. DTN plane

Queues work when no end-to-end path currently exists. Delivery can resume when a peer, route or egress appears later.

### 6. Minimum-information plane

Carries compact structured requests and results when raw application traffic is not viable. Local rendering, caching and local-AI reconstruction belong above this layer.

## Dynamic node roles

A node can simultaneously be:

- scanner;
- peer;
- relay;
- Internet egress;
- DTN custodian;
- local-only device.

Roles are observations, not permanent identities.

## Platform strategy

### Android

Targets: per-network handles, Wi-Fi peer APIs where exposed, BLE, VPN/TUN integration, cellular/satellite visibility where exposed, and background-execution constraints.

### Windows

Targets: adapter enumeration, WLAN, Bluetooth, Wi-Fi Direct where available, Winsock/per-interface route binding, network-state events and a TUN path.

### Linux

Targets: netlink/nl80211, NetworkManager/wpa_supplicant capability discovery, BlueZ, TUN, routes/namespaces and driver-specific concurrent-interface capabilities.

## Capability-driven rule

The core never assumes that a platform exposes a feature. Adapters report evidence:

```text
Capability {
  available
  permission_state
  interface
  transport
  can_scan
  can_connect
  can_advertise
  can_relay
  can_bind_socket
  constraints
}
```

The routing/recovery core only acts on measured capabilities.
