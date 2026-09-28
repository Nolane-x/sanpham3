# Threat model v0.1

A cooperative connectivity network can easily become unsafe if relay behavior is treated as ordinary tethering. The initial protocol therefore assumes project-specific capsules and explicit consent.

## Assets

Protect:

- user traffic;
- device identity;
- local network information;
- battery and bandwidth;
- metered data;
- peer topology;
- queued DTN content.

## Primary threats

### Open-proxy abuse

A peer must not gain unrestricted Internet proxy access merely by discovering another node.

### Resource exhaustion

Attackers may flood discovery, routing, relay, queue or verification work.

### Replay

Old capsules may be replayed to waste bandwidth or duplicate actions.

### Route lies

A node may advertise false egress quality or nonexistent peers.

### DTN custody abuse

A custodian can drop, inspect, indefinitely retain or selectively forward queued data.

### Topology privacy

Peer discovery can reveal nearby-device relationships and movement patterns.

## Required protocol properties before public mesh deployment

- end-to-end authenticated encryption;
- ephemeral peer identifiers where practical;
- explicit relay consent;
- relay quotas;
- hop limits;
- replay protection;
- bounded queues;
- metered-network policy;
- egress allow-list / capsule allow-list;
- route advertisement expiry;
- abuse telemetry stored locally by default;
- no silent background open proxy.

Security gates must be measured alongside connectivity gains.
