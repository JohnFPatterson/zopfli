# Parity report (libzopfli)

7 fixtures, 7 identical.

## Method

Compared stdout bytes and exit status of `./build/oracle {input}` and `./target/release/zopfli-driver {input}` for every file under `tests/inputs/**/*`. Stderr is not compared. Reproduce:

```sh
printf '%s' '{"status": "completed", "loop_count": 0, "workspace_roots": ["/workspace"]}' | '/workspace/.cursor/hooks/c-rust-parity/parity_gate.py' --force
```

Gate output:

```
parity-gate: /workspace: PASS: 7 fixtures, 7 compared: 7 identical, 0 logged exceptions, 0 diverged; 0 gate problems.
```

## Per-input results

| Fixture | Result | C status | Rust status | stdout sha256 |
|---|---|---|---|---|
| `tests/inputs/bytes_256.bin` | identical | exit 0 | exit 0 | `4d9e1ab0233f` |
| `tests/inputs/empty.bin` | identical | exit 0 | exit 0 | `4055a0a9473a` |
| `tests/inputs/hello.txt` | identical | exit 0 | exit 0 | `900f93f3265a` |
| `tests/inputs/pattern_1k.bin` | identical | exit 0 | exit 0 | `847e754bf926` |
| `tests/inputs/repeat_a64.txt` | identical | exit 0 | exit 0 | `0c1cc35e57b3` |
| `tests/inputs/text_lines.txt` | identical | exit 0 | exit 0 | `501bb39a0915` |
| `tests/inputs/zeros_512.bin` | identical | exit 0 | exit 0 | `d163f198e36a` |

## Divergences

None.

## Exceptions

None.

## Known gaps

- Driver uses `numiterations=1` (documented identical cost bound).
- Inputs larger than 4096 bytes are skipped identically (`SKIP oversize`).
- Hook-trace: N/A (no allocator hooks / callbacks on the public API).
- stderr is not compared.
- Inputs outside `tests/inputs/**/*` are not covered.

## Oracle defect found

None.

## ASan

Pending (`make asan-oracle` over fixtures).
