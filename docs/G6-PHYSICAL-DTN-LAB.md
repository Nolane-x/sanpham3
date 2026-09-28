# G6 Physical DTN Lab

This lab validates the physical-device version of the delay-tolerant recovery path.

It is deliberately separate from CI evidence.

## Topology

Use three machines or three independently networked devices:

```text
A = origin
B = moving / intermediate custodian
C = Internet egress
```

Required experiment:

```text
time 0:
A has no route to C
A queues request locally

contact 1:
A <-> B
A transfers custody
A and B disconnect

partition:
B may stop the app / reboot
bundle remains in the persistent spool

contact 2:
B <-> C
B dispatches queued request
C reaches the Internet
B creates a return-result bundle
B and C disconnect

contact 3:
B <-> A
B transfers result custody
A reads result
```

A must never have a direct connection to C during the experiment.

## Build

From the repository root:

```bash
cargo build --release -p recovery-lab-cli -p peer-egress-cli
```

Use the same 32-byte laboratory PSK on A, B, and C. The command-line representation is 64 hexadecimal characters.

Example only:

```text
4242424242424242424242424242424242424242424242424242424242424242
```

Do not treat the lab PSK workflow as the final public pairing system.

## Phase 1 — A queues while offline

On A:

```bash
recovery-lab-cli enqueue \
  a-request.spool \
  1001 \
  501 \
  example.com \
  urgent \
  3600
```

No network contact is required for this step.

Then verify that ordinary/default Internet access from A to the target experiment path is unavailable.

Record:

- OS;
- interfaces;
- default route state;
- exact time;
- whether normal DNS/browser access succeeds;
- spool file size.

## Phase 2 — A gives custody to B

On B:

```bash
recovery-lab-cli custody-receive \
  0.0.0.0:45130 \
  200 \
  <PSK> \
  b-request.spool
```

On A:

```bash
recovery-lab-cli custody-send \
  <B_IP>:45130 \
  100 \
  <PSK> \
  a-request.spool
```

Success criteria:

- B reports `Accepted` or an idempotent `AlreadyHave`;
- A removes bundle 1001 only after the custody ACK;
- B's spool contains bundle 1001;
- A's spool no longer contains bundle 1001.

Disconnect A and B after this step.

## Phase 3 — restart B

Stop the B process.

Optionally reboot B.

Do not recreate the request.

The next command must load the existing `b-request.spool`.

This phase is required to distinguish durable DTN behavior from an in-memory retry queue.

## Phase 4 — B meets C

On C, which has real Internet egress:

```bash
peer-egress-cli server \
  0.0.0.0:45123 \
  300 \
  <PSK>
```

On B:

```bash
recovery-lab-cli dispatch \
  <C_IP>:45123 \
  200 \
  <PSK> \
  b-request.spool \
  b-return.spool \
  2001 \
  3600
```

Success criteria:

- request bundle 1001 is loaded from disk;
- B authenticates C;
- C resolves the public hostname;
- request bundle reaches a terminal result;
- B creates return bundle 2001;
- `b-return.spool` survives after the C contact ends.

Disconnect B and C.

## Phase 5 — B returns the result to A

On A:

```bash
recovery-lab-cli custody-receive \
  0.0.0.0:45131 \
  100 \
  <PSK> \
  a-result.spool
```

On B:

```bash
recovery-lab-cli custody-send \
  <A_IP>:45131 \
  200 \
  <PSK> \
  b-return.spool
```

On A:

```bash
recovery-lab-cli show-result a-result.spool
```

A successful run shows the original request ID and the remote result after a completely separate return contact.

## Evidence required for closing physical G6

Save:

- command output from A, B, C;
- timestamps;
- hashes of the binaries tested;
- commit SHA;
- OS versions;
- network topology;
- firewall state;
- whether nodes were on Ethernet, Wi-Fi, Wi-Fi Direct, hotspot, or another carrier;
- proof that A could not directly use C during the partition;
- spool files or sanitized spool metadata before/after each contact;
- failures and retries, not only the successful run.

Repeat the experiment multiple times.

## What this proves

A passing physical run demonstrates:

```text
no route now
!=
request lost
```

and:

```text
store -> contact -> custody -> carry -> later egress
      -> store result -> later contact -> return
```

## What this does not prove

It does not by itself prove:

- arbitrary Internet browsing over partitions;
- radio range beyond the underlying transport;
- Android Wi-Fi Direct/Aware support;
- resistance to malicious custodians;
- end-to-end payload secrecy from B;
- G7 weak-link performance.

Those require separate gates.
