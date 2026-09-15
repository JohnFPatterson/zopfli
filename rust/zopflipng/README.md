# zopflipng (Rust)

PNG optimizer ported from Google ZopfliPNG (`src/zopflipng/zopflipng_lib.cc`).

- Decode/encode with the [`png`](https://crates.io/crates/png) crate (LodePNG is not ported).
- Final IDAT zlib payload is produced with the crates.io [`zopfli`](https://crates.io/crates/zopfli) `0.8` crate (`Format::Zlib`), not miniz/flate2.
- C ABI is in the sibling `zopflipng-ffi` crate (`libzopflipng.a`).

## Standalone build

This crate is **not** listed in a workspace `Cargo.toml` on this branch. Agent 1 owns `rust/Cargo.toml`. Build and test with `--manifest-path`:

```bash
# Library tests (includes go_compat: shrink go/zopflipng/testdata/zoidberg.png)
cargo test --manifest-path rust/zopflipng/Cargo.toml --release

# C ABI staticlib
cargo build --release --manifest-path rust/zopflipng-ffi/Cargo.toml
```

`libzopflipng.a` is emitted at:

```
rust/zopflipng-ffi/target/release/libzopflipng.a
```

(If these crates are later added to `rust/Cargo.toml` as workspace members, Cargo will instead write `rust/target/release/libzopflipng.a`.)

The FFI crate depends on this library via `path = "../zopflipng"`. Do **not** depend on a path crate named `zopfli-core` from this branch (Agent 1 creates that in parallel).

## Merge: workspace members

When merging with Agent 1’s `rust/Cargo.toml`, add:

```toml
[workspace]
members = [
    # Agent 1 crates, e.g. "zopfli-core", "zopfli-ffi", ...
    "zopflipng",
    "zopflipng-ffi",
]
```

Paths are relative to `rust/`. After that, `cargo test --release -p zopflipng` and `cargo build --release -p zopflipng-ffi` work from `rust/`.

### Switch to in-tree `zopfli-core`

This crate currently depends on crates.io `zopfli = "0.8"` so it builds without Agent 1’s path crate. After merge, consider replacing that dependency with:

```toml
zopfli-core = { path = "../zopfli-core" }
```

(or whatever package name Agent 1 chose) so there is a single Zopfli implementation. The algorithm is the same; the path crate avoids two copies of Zopfli in the workspace.

## C ABI (for unchanged Go CGO)

Header (copy of `src/zopflipng/zopflipng_lib.h`):

```
rust/zopflipng-ffi/include/zopflipng_lib.h
```

Exports:

- `void CZopfliPNGSetDefaults(CZopfliPNGOptions* png_options);`
- `int CZopfliPNGOptimize(...)` — `0` on success; nonzero on error (`ENOMEM` if `malloc` fails). Output is `malloc`’d; Go frees with `C.free`.

`go/zopflipng` still has `#cgo LDFLAGS: -lzopflipng -lzopfli -lstdc++ -lm`. After merge it also needs Agent 1’s `libzopfli.a` (`ZopfliInitOptions` / `ZopfliCompress`). This crate does **not** stub those symbols.

### Linux CGO smoke (optional)

Build a throwaway C `libzopfli.a` from this repo (do not commit `obj/` or C-built `.a`):

```bash
make libzopfli.a
cargo build --release --manifest-path rust/zopflipng-ffi/Cargo.toml

# go/ has no go.mod; module mode needs a throwaway module file, or GOPATH.
cd go/zopflipng
printf 'module github.com/google/zopfli/go/zopflipng\n\ngo 1.22\n' > go.mod   # do not commit
CGO_ENABLED=1 \
CGO_CFLAGS="-I$(pwd)/../../src/zopflipng" \
CGO_LDFLAGS="-L$(pwd)/../../rust/zopflipng-ffi/target/release -L$(pwd)/../.. -ldl -lpthread" \
go test -count=1 -timeout 180s
rm go.mod
```

`#cgo LDFLAGS` in `zopflipng.go` already has `-lzopflipng -lzopfli -lstdc++ -lm`. The extra `-L` paths locate the Rust `libzopflipng.a` and the C `libzopfli.a` linker stand-in. `-lstdc++` is unused for the Rust staticlib but is harmless.

If the workspace layout is used, point `-L` at `rust/target/release` instead.

### Darwin linker flags

A Rust `staticlib` on macOS is often dropped by the linker unless the archive is force-loaded, because CGO may not see the object files that define `CZopfliPNG*`:

```text
-Wl,-force_load,/absolute/path/to/libzopflipng.a
```

Example:

```bash
CGO_CFLAGS="-I/path/to/zopflipng-ffi/include" \
CGO_LDFLAGS="-Wl,-force_load,/path/to/libzopflipng.a -L/path/to/libzopfli -lzopfli -lm" \
go test ./go/zopflipng
```

On Linux, `--whole-archive` is the analogue if `--as-needed` drops the archive:

```text
-Wl,--whole-archive -lzopflipng -Wl,--no-whole-archive
```

## C smoke test (no libzopfli)

```bash
cargo build --release --manifest-path rust/zopflipng-ffi/Cargo.toml
cc -O2 -I rust/zopflipng-ffi/include \
  rust/zopflipng-ffi/c_smoke.c \
  rust/zopflipng-ffi/target/release/libzopflipng.a \
  -lpthread -ldl -lm \
  -o /tmp/zopflipng_smoke
/tmp/zopflipng_smoke go/zopflipng/testdata/zoidberg.png
```

## Incomplete vs C++ ZopfliPNG

Implemented enough for the Go `TestCompress` (defaults, shrink `zoidberg.png`):

- Decode to RGBA8, strip ancillary chunks, auto color type (gray / palette / RGB / RGBA / tRNS key).
- Auto filter search: None / Sub / Up / Average / Paeth / Adaptive (MinSum, Entropy, Predefined, BruteForce all use Adaptive via the `png` crate).
- Final IDAT: Zopfli zlib, `num_iterations` if uncompressed size &lt; 200000 else `num_iterations_large`. Window size is Zopfli’s 32768.
- `lossy_transparent` is implemented; `keepchunks` copies named ancillary chunks.

Not a full LodePNG `auto_convert` clone:

- 16-bit output is not preserved (`lossy_8bit` is accepted; input is always decoded to 8-bit).
- Sub-8-bit packing (1/2/4) is not implemented.
- Original palette order / `kStrategyPredefined` original filter bytes are not applied.
- Cheap filter pass uses `png::Compression::Fast` rather than LodePNG window size 8192.
- `block_split_strategy` is unused (same as C++).
