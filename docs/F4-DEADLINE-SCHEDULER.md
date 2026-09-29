# F4 Deadline / Freshness Fragment Scheduler

This is the first scheduling baseline above the authenticated fragment layer.

It answers a practical weak-contact question:

> If a contact can carry only a small number of already-authenticated fragment
> wires, which transfer should spend those bytes?

## Inputs

Every pending transfer carries:

- deterministic fragment transfer ID;
- already-sealed SP3F wires;
- freshness deadline (`fresh_until`);
- application priority;
- current next-fragment cursor.

Every contact carries:

- start time;
- duration;
- measured/model bitrate;
- optional application-level wire-byte cap.

## Baseline policy

The scheduler:

1. sends nothing for transfers already stale at contact start;
2. prefers a transfer that can finish inside the current contact and before
   its freshness deadline;
3. among comparable transfers, prefers the earlier freshness deadline;
4. then higher application priority;
5. then fewer remaining wire bytes;
6. allows partial progress when no transfer can finish in the current contact.

The scheduler never rewrites or bypasses fragment authentication. It only
selects which existing SP3F wire is sent next.

## Court

The deterministic court creates two URT exact objects:

- a fresh remote recovery result with an 8-second freshness deadline;
- a much larger background object with a later deadline and higher numeric
  priority.

The background transfer is deliberately inserted first. The early constrained
contact must still complete the freshness-limited recovery result first.
A later contact resumes and completes the background object.

The scheduled SP3F wires are then assembled and the urgent URT object is
decoded byte-for-byte.

PASS begins with:

```text
F4_DEADLINE_SCHEDULER_PASS
```

## Evidence boundary

This is a deterministic policy baseline, not an optimal network scheduler.

It does not yet model:

- correlated carrier failures;
- probabilistic future contacts;
- parity/fountain marginal utility;
- energy per useful bit;
- multi-source provenance;
- user/application fairness across long-lived queues.
