# Differential driver format (libzopfli)

Both `tools/zopfli-oracle.c` (C) and the `zopfli-driver` Rust binary print the same text for the same fixture path.

## Invocation

```
./build/oracle [--sections name[,name…]] <fixture-path>
./target/release/zopfli-driver [--sections name[,name…]] <fixture-path>
```

- Exit `0` on success (including empty input).
- Exit `2` if the fixture cannot be read.
- Exit `1` on internal compression failure (should not happen for valid options).
- Stderr is not compared (`compare_stderr: false`).

## Fixed options (both drivers)

Matches `ZopfliInitOptions` defaults except `numiterations`, which is set to `1` so every fixture finishes well under the gate timeout:

| Field | Value |
|-------|-------|
| verbose | 0 |
| verbose_more | 0 |
| numiterations | 1 |
| blocksplitting | 1 |
| blocksplittinglast | 0 |
| blocksplittingmax | 15 |

## Cost bound

If `insize > 4096`, both drivers print a single line and skip compression:

```
SKIP oversize <insize>
```

Exit status remains `0`. No section bodies are printed.

## Sections

Without `--sections`, print every section below in this order. With `--sections`, print only the named sections, still in the order listed here (not the CLI order).

### `gzip`

Compress with `ZOPFLI_FORMAT_GZIP` / `Format::Gzip`.

```
=== gzip ===
len <decimal byte length>
hex <lowercase hex of entire output, no spaces>
```

### `zlib`

Compress with `ZOPFLI_FORMAT_ZLIB` / `Format::Zlib`. Same `len` / `hex` lines under `=== zlib ===`.

### `deflate`

Compress with `ZOPFLI_FORMAT_DEFLATE` / `Format::Deflate`. Same `len` / `hex` lines under `=== deflate ===`.

## Oracle shape

Binary codec: input bytes → containerized DEFLATE bitstream (gzip / zlib / raw deflate). Observable surface for parity is the exact compressed byte sequence under the fixed options above.
