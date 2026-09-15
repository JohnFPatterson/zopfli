# Rust libzopfli / libzopflipng

Workspace crates:

- `zopfli-core`: Rust API wrapping the crates.io `zopfli` crate
- `zopfli-ffi`: C ABI static library (`libzopfli.a`) for `go/zopfli` CGO
- `zopflipng`: PNG optimizer (decode/filter/color; IDAT via `zopfli-core`)
- `zopflipng-ffi`: C ABI static library (`libzopflipng.a`) for `go/zopflipng` CGO

## Build

```bash
cd rust
cargo build --release
```

Both static libraries are written to `rust/target/release/`:

- `libzopfli.a`
- `libzopflipng.a`

Headers:

- `zopfli-ffi/include/zopfli.h` (copy of `src/zopfli/zopfli.h`)
- `zopflipng-ffi/include/zopflipng_lib.h` (copy of `src/zopflipng/zopflipng_lib.h`)

The workspace uses a single `rust/Cargo.lock`. `zopflipng` depends on path
`zopfli-core` rather than a second crates.io `zopfli` copy.

## Tests

From the repo root:

```bash
make rust      # cargo build --release in rust/
make test-go   # Go CGO tests against the Rust staticlibs
```

Rust unit/integration tests (includes `zopflipng` shrinking `go/zopflipng/testdata/zoidberg.png`):

```bash
cd rust
cargo test --release
```

`go/zopfli` and `go/zopflipng` have `go.mod` files, so module mode is the default
(`GO111MODULE=off` is not required). `scripts/test-go.sh` sets `CGO_CFLAGS` /
`CGO_LDFLAGS` for the unified `rust/target/release` output.

On macOS, if the linker drops archive members, `scripts/test-go.sh` uses
`-Wl,-force_load` for both staticlibs.
