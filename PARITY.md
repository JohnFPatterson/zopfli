# Parity report (libzopfli)

## Method

Drivers: `./build/oracle` vs `./target/release/zopfli-driver` over `tests/inputs/**/*`.
Reproduce: `make parity` (invokes [Parity Gate] `parity_gate.py --force`).

## Per-input results

Pending full algorithm port.

## Divergences

None recorded yet (stub Rust fails the gate until compress is implemented).

## Exceptions

None. See `PARITY_EXCEPTIONS.md`.

## Known gaps

- Driver uses `numiterations=1` (documented identical cost bound).
- Inputs larger than 4096 bytes are skipped identically (`SKIP oversize`).
- Hook-trace: N/A (no allocator hooks / callbacks on the public API).

## Oracle defect found

None.

## ASan

Pending (`make asan-oracle` over fixtures).
