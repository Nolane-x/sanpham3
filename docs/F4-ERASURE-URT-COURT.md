# F4 Authenticated Erasure + URT Court

This court connects three layers that previously had independent software
evidence:

```text
logical exact payload
  -> URT representation selection
  -> authenticated SP3F data fragments
  -> authenticated SP3E XOR parity
  -> intermittent contact windows
  -> fragment reconstruction
  -> URT exact decode
  -> original byte-identical payload
```

## SP3E parity boundary

SP3E is a systematic one-parity-per-stripe erasure baseline.

Every parity envelope carries:

- transfer ID;
- whole-object SHA-256;
- total object length;
- stripe index and start offset;
- fixed shard payload size;
- number of data shards in the stripe;
- XOR parity bytes;
- truncated 128-bit HMAC-SHA256 authentication tag.

The sender chooses the shard payload size only after accounting for the larger
of SP3F and SP3E envelope overheads, so both data and parity packets fit the
declared wire budget.

## Recovery rule

For a stripe:

- zero missing data shards -> parity is not needed;
- exactly one missing data shard -> reconstruct by XOR;
- two or more missing data shards -> return `Insufficient`.

Recovered bytes are inserted back into the ordinary fragment assembler.
Completion still requires the original whole-object SHA-256.

This is deliberately narrower than Reed-Solomon or fountain coding.

## Court

`fragment-court-cli` creates a structured logical payload, lets URT choose
its exact representation, and then fragments the resulting URT wire.

The court:

1. separates data shards into multiple synthetic contact windows;
2. reverses order;
3. injects an exact duplicate;
4. drops the second data shard from every parity stripe;
5. delivers parity only after the data contacts;
6. reconstructs the exact URT packet;
7. decodes URT;
8. requires byte-for-byte equality with the original logical payload.

PASS output begins with:

```text
F4_ERASURE_URT_PASS
```

## Evidence boundary

This court proves deterministic software behavior under a modeled loss pattern.

It does not prove a physical radio loss distribution, correlated failures,
energy cost, fountain/rateless recovery, or real contact-window scheduling.
