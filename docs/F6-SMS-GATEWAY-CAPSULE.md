# F6 SMS / Data-SMS Gateway Capsule Prototype

This baseline turns the existing SMS capability model into a bounded software
transport for tiny gateway requests and responses.

It does not treat SMS as free Internet.

## Transport roles

The intended recovery path is:

```text
phone A with packet data unavailable
 -> cost-bearing SMS/data-SMS
 -> cooperating gateway/phone B
 -> remote lookup through B's egress
 -> SMS/data-SMS response
 -> phone A
```

Fresh remote truth still requires the gateway side to have real egress.

## SP3M authenticated capsule

SMS uses the shared `tiny-capsule` core with domain:

```text
SP3M
```

Therefore the final request/response capsule inherits:

- sender identity field;
- 64-bit monotonic sequence;
- HMAC-SHA256/128 authentication;
- bounded replay window;
- cross-domain separation from SP3A/SP3O/SP3V.

## S3MG segmentation layer

The project uses a conservative prototype data-message budget:

```text
120 bytes per modeled data-message payload
```

This is a project accounting bound, not a universal carrier/operator guarantee.

Each segment carries:

```text
magic        4 bytes  S3MG
version      1 byte
transfer_id  8 bytes
index        1 byte
count        1 byte
length       1 byte
payload     <=104 bytes
```

The transfer ID is the first 64 bits of SHA-256 over the complete authenticated
SP3M wire.

The bounded assembler supports:

- out-of-order segments;
- exact duplicate idempotence;
- conflicting-duplicate rejection;
- transfer/count mismatch rejection;
- maximum segment count;
- maximum reassembled bytes;
- whole-transfer fingerprint verification after assembly.

Segment integrity is not considered sufficient authentication. Acceptance still
requires the reassembled SP3M HMAC and replay checks.

## Send policy

Before modeled transmission, `SmsSendPolicy` requires:

- explicit user consent;
- an available subscription;
- roaming permission when currently roaming;
- a configured maximum segment count.

The policy court verifies that missing consent, missing subscription, blocked
roaming, or an excessive segment count fails closed.

No fixed monetary price is embedded because SMS billing is operator,
subscription and roaming dependent.

## Bidirectional useful-task court

The court creates a real `peer-egress::ResolveRequest` whose authenticated
SP3M wire spans multiple S3MG segments.

Delivery intentionally:

- reverses segment order;
- injects an exact duplicate;
- reassembles under bounded state;
- authenticates SP3M;
- decodes the exact ResolveRequest;
- rejects replay of the same complete request.

The gateway then creates a deterministic `ResolveResponse` with five IPv6
addresses, wraps it in SP3M, segments it, reorders/duplicates it, and the client
must reconstruct and decode the exact response.

PASS begins with:

```text
F6_SMS_GATEWAY_PASS
```

The court reports request/response logical bytes, authenticated capsule bytes,
segment counts and the explicit `cost_bearing=true user_consented=true`
boundary.

## Carrier model

`CarrierKind::CellularSms` now exposes `max_payload_bytes=Some(120)` to the
research scheduler.

That value means only the current conservative project transport budget. It
must not be cited as a guaranteed Android/carrier payload limit.

## Evidence boundary

This closes a cross-platform **software transport and gateway-protocol
baseline**.

It does not yet close:

- Android `SmsManager` send integration;
- receive/broadcast integration;
- subscription selection;
- operator delivery receipts;
- real SMS latency;
- real segment payload limits;
- real monetary cost;
- roaming/operator policy;
- Play-distribution permission policy;
- two-device/gateway physical interoperability.

Those remain required before F7 product promotion.
