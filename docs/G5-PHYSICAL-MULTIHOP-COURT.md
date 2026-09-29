# G5 Physical Multi-Hop Court

G5 proves a real three-node path:

```text
A -> B -> C -> Internet
```

A must not directly use C as its egress during the tested run.

## Roles

Recommended project node IDs:

```text
A client = 100
B relay  = 200
C egress = 300
```

All three nodes use the same laboratory peer PSK through the environment:

Linux:

```bash
export SP3_PEER_PSK_HEX=<64_hex_psk>
```

Windows PowerShell:

```powershell
$env:SP3_PEER_PSK_HEX = "<64_hex_psk>"
```

The role harnesses never place the PSK in the peer-egress process command line.

## Required topology proof

Before the successful multi-hop request, document why A cannot directly use C.

Good evidence includes:

- separate L2 networks with routing/firewall policy that only permits A<->B and B<->C;
- Wi-Fi Direct/local transport A<->B plus a different B<->C path;
- a controlled firewall rule blocking A->C while allowing A->B and B->C.

Do not count a test where A could simply connect directly to C.

Record the topology rule separately in the evidence notes.

## Linux roles

### C — egress

```bash
scripts/g5-physical-role.sh \
  egress \
  0.0.0.0:45123 \
  300 \
  200 \
  evidence/g5-c
```

Expected authenticated peer: B / node 200.

### B — relay

After C is listening:

```bash
scripts/g5-physical-role.sh \
  relay \
  0.0.0.0:45124 \
  200 \
  <C_IP>:45123 \
  300 \
  100 \
  evidence/g5-b
```

B must authenticate:

- upstream C = node 300;
- downstream A = node 100.

### A — client

After B is listening:

```bash
scripts/g5-physical-role.sh \
  client \
  <B_IP>:45124 \
  100 \
  200 \
  example.com \
  evidence/g5-a
```

A must authenticate B / node 200 and receive the public result.

## Windows roles

The PowerShell harness has equivalent roles.

### C — egress

```powershell
.\scripts\g5-physical-role.ps1 \
  egress \
  "0.0.0.0:45123" \
  300 \
  200 \
  -EvidenceDir "evidence\g5-c"
```

### B — relay

```powershell
.\scripts\g5-physical-role.ps1 \
  relay \
  "0.0.0.0:45124" \
  200 \
  300 \
  "<C_IP>:45123" \
  100 \
  "evidence\g5-b"
```

### A — client

```powershell
.\scripts\g5-physical-role.ps1 \
  client \
  "<B_IP>:45124" \
  100 \
  200 \
  "example.com" \
  0 \
  "evidence\g5-a"
```

## Evidence generated per node

Each role directory contains:

- role log;
- metadata;
- binary SHA-256;
- git commit;
- UTC timestamp;
- OS/machine metadata;
- SHA256SUMS for log and metadata.

A role only prints `G5_ROLE_PASS` after its expected authenticated node IDs and
operation result are present in the raw log.

## Cross-check before closing G5

The three evidence packages must agree on:

```text
A sees authenticated relay       = 200
B sees authenticated downstream  = 100
B sees authenticated upstream    = 300
C sees authenticated peer        = 200
```

A must contain a successful returned result.

B must contain `relayed one request`.

C must contain `request served`.

The tested commit should be identical across all three machines.

## Physical closure

G5 closes only after:

- three real machines/devices participate;
- A cannot directly use C;
- two authenticated peer-session hops are established;
- the relay budget permits the single application relay;
- new remote information reaches A through B and C;
- raw evidence from all three nodes is preserved.

Loopback/unit tests remain software evidence only.
