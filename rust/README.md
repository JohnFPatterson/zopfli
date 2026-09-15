# Rust libzopfli

Workspace crates:

- `zopfli-core`: Rust API wrapping the crates.io `zopfli` crate
- `zopfli-ffi`: C ABI static library (`libzopfli.a`) for `go/zopfli` CGO

## Build

```bash
cd rust
cargo build --release
```

The static library is written to `rust/target/release/libzopfli.a`.
The C header is `zopfli-ffi/include/zopfli.h` (unchanged copy of `src/zopfli/zopfli.h`).

## Link with Go CGO

```bash
REPO=$(git rev-parse --show-toplevel)
export CGO_CFLAGS="-I${REPO}/rust/zopfli-ffi/include"
export CGO_LDFLAGS="-L${REPO}/rust/target/release -lzopfli -lm"
cd ${REPO}/go/zopfli && go test -v
```

On macOS, if the linker drops archive members:

```bash
export CGO_LDFLAGS="-L${REPO}/rust/target/release -Wl,-force_load,${REPO}/rust/target/release/libzopfli.a -lm"
```
