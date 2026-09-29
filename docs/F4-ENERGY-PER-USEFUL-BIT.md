# F4 Energy per Useful Bit Objective

This baseline adds an energy-aware path objective for weak/intermittent
fragment transport.

It deliberately does **not** invent physical power values for a carrier.

## Measurement inputs

Each candidate path supplies externally measured or evidence-backed values:

- setup/wake time;
- setup energy in microjoules;
- active transmit power in microwatts;
- usable wire bitrate;
- expected wire bytes for the task.

The task supplies the logical useful bits that must be delivered.

Expected wire bytes are allowed to include:

- fragment envelope overhead;
- parity;
- estimated retransmissions;
- other transport overhead known by the caller.

## Objective

For one candidate:

```text
serialization_time = expected_wire_bits / bitrate
completion_time = setup_time + serialization_time

active_energy_uj =
    active_power_uw * serialization_time_seconds

estimated_energy_uj =
    setup_energy_uj + active_energy_uj

score =
    estimated_energy_uj / useful_bits
```

The implementation keeps the score as an exact integer ratio rather than a
floating-point approximation.

Candidates in one selection must target the same number of useful bits.

## Deadline interaction

A low-energy path is not eligible if it cannot finish before the supplied
completion deadline.

Among feasible candidates the selector minimizes energy per useful bit. Ties
prefer:

1. lower absolute estimated energy;
2. shorter completion time;
3. lexical path ID for deterministic output.

## Court

The fragment scheduler court uses the real URT recovery object and generates
real SP3F fragment wires to measure expected wire bytes including envelope
overhead.

Two evidence fixtures are supplied:

- a fast/high-power path;
- a slower/low-power path with a longer setup.

With a relaxed deadline the low-energy path must win.

With a tight deadline the slow path becomes infeasible and the fast path must
win.

PASS output begins with:

```text
F4_ENERGY_OBJECTIVE_PASS
```

## Evidence boundary

The optimizer is only as physically meaningful as its measurement inputs.

This baseline proves:

- deterministic energy accounting;
- wire-overhead sensitivity;
- deadline-aware path selection;
- no use of arbitrary relative `energy_cost` values as if they were joules.

It does not prove the energy consumption of any real Bluetooth, Wi-Fi,
acoustic, NFC, cellular or other carrier until those values are measured on
actual devices.
