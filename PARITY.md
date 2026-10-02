# Parity report (libzopfli)

## Result

**7 fixtures, 7 identical** (gzip / zlib / deflate sections).

## Method

Compared `./build/oracle` (C, public `zopfli.h` only) vs `./target/release/zopfli-driver` (Rust `zopfli-core`) over `tests/inputs/**/*`. Stdout bytes and exit status; stderr not compared.

Reproduce:

```sh
make parity
# or
printf '%s' '{"status":"completed","loop_count":0,"workspace_roots":["'"$PWD"'"]}' \
  | ./.cursor/hooks/c-rust-parity/parity_gate.py --force
```

## Per-input results

| Fixture | Result |
|---------|--------|
| `tests/inputs/bytes_256.bin` | identical |
| `tests/inputs/empty.bin` | identical |
| `tests/inputs/hello.txt` | identical |
| `tests/inputs/pattern_1k.bin` | identical |
| `tests/inputs/repeat_a64.txt` | identical |
| `tests/inputs/text_lines.txt` | identical |
| `tests/inputs/zeros_512.bin` | identical |

## Gate report (paste)

```
parity-gate: /workspace: PASS: 7 fixtures, 7 compared: 7 identical, 0 logged exceptions, 0 diverged; 0 gate problems.
```

Full report: `build/parity-gate/parity-report.md` (generated, not committed).

## Divergences

None.

## Exceptions

None. See `PARITY_EXCEPTIONS.md`.

## Quirks matched (C citations)

- Gzip header XFL=2 / OS=3 (`src/zopfli/gzip_container.c:100-101`).
- Zlib CMF/FLG packing with fcheck (`src/zopfli/zlib_container.c:55-63`).
- Default options from `ZopfliInitOptions` (`src/zopfli/util.c:28-35`); drivers force `numiterations=1` identically.
- Empty input still emits a valid container (gzip 20 bytes) via the same DEFLATE empty-block path as C.

## Known gaps

- Driver uses `numiterations=1` (documented identical cost bound in `tools/DRIVER_FORMAT.md`).
- Inputs larger than 4096 bytes are skipped identically (`SKIP oversize`).
- Hook-trace: N/A (no allocator hooks / callbacks on the public API).
- ZopfliPNG and CLI bins are out of scope.

## Oracle defect found

None.

## ASan

Built with `make asan-oracle` (gcc + AddressSanitizer/UBSan). Command:

```sh
make asan-oracle
for f in tests/inputs/*; do
  ASAN_OPTIONS=detect_leaks=0 ./build/asan/oracle "$f" > /dev/null || echo "ASAN: $f"
done
```

Output: (empty — all fixtures clean).
