# zopflipng (Rust)

PNG optimizer ported from Google ZopfliPNG (`src/zopflipng/zopflipng_lib.cc`).

- Decode/encode with the [`png`](https://crates.io/crates/png) crate (LodePNG is not ported).
- Scanlines are filtered in this crate; the final IDAT zlib payload is produced
  with in-tree [`zopfli-core`](../zopfli-core) (`Format::Zlib`).
- C ABI is in the sibling `zopflipng-ffi` crate (`libzopflipng.a`).

## Workspace build

These crates are members of `rust/Cargo.toml`. From `rust/`:

```bash
cargo test --release -p zopflipng
cargo build --release
```

`libzopflipng.a` and `libzopfli.a` are both emitted at `rust/target/release/`.

The FFI crate depends on this library via `path = "../zopflipng"`. Compression
goes through `zopfli-core` (`path = "../zopfli-core"`), not a second crates.io
`zopfli` copy.

## C ABI (for unchanged Go CGO)

Header (copy of `src/zopflipng/zopflipng_lib.h`):

```
rust/zopflipng-ffi/include/zopflipng_lib.h
```

Exports:

- `void CZopfliPNGSetDefaults(CZopfliPNGOptions* png_options);`
- `int CZopfliPNGOptimize(...)` — `0` on success; nonzero on error (`ENOMEM` if `malloc` fails). Output is `malloc`’d; Go frees with `C.free`.

`go/zopflipng` still has `#cgo LDFLAGS: -lzopflipng -lzopfli -lstdc++ -lm`.
`make test-go` / `scripts/test-go.sh` supply `-L rust/target/release` plus the
Linux extra libs (`-lpthread -ldl -lgcc_s`).

### Darwin linker flags

A Rust `staticlib` on macOS is often dropped by the linker unless the archive
is force-loaded. `scripts/test-go.sh` already does this:

```text
-Wl,-force_load,/absolute/path/to/libzopflipng.a
```

## C smoke test

```bash
cd rust && cargo build --release
cc -O2 -I rust/zopflipng-ffi/include \
  rust/zopflipng-ffi/c_smoke.c \
  rust/target/release/libzopflipng.a \
  -lpthread -ldl -lm \
  -o /tmp/zopflipng_smoke
/tmp/zopflipng_smoke go/zopflipng/testdata/zoidberg.png
```

## Incomplete vs C++ ZopfliPNG

Implemented enough for the Go `TestCompress` (defaults, shrink `zoidberg.png`):

- Decode to RGBA8, strip ancillary chunks, auto color type (gray / palette / RGB / RGBA / tRNS key).
- Auto filter search: None / Sub / Up / Average / Paeth / MinSum / Entropy / Predefined.
- Final IDAT: Zopfli zlib via `zopfli-core`, `num_iterations` if uncompressed size &lt; 200000 else `num_iterations_large`. Window size is Zopfli’s 32768.
- `lossy_transparent` is implemented; `keepchunks` copies named ancillary chunks.

Not a full LodePNG `auto_convert` clone:

- 16-bit output is not preserved (`lossy_8bit` is accepted; input is always decoded to 8-bit).
- Sub-8-bit packing (1/2/4) is not implemented.
- Cheap filter pass uses flate2 zlib level 1 rather than LodePNG window size 8192.
- `block_split_strategy` is unused (same as C++).
