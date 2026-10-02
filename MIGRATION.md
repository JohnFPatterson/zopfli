# Migration: libzopfli C → Rust

## Oracle shape

**Binary codec.** Public entry points `ZopfliInitOptions` / `ZopfliCompress` in `src/zopfli/zopfli.h` turn input bytes into gzip, zlib, or raw deflate bitstreams. Differential drivers compare the exact compressed bytes under fixed options (`numiterations=1`; see `tools/DRIVER_FORMAT.md`).

## Layout

| Path | Role |
|------|------|
| `src/zopfli/*.c`, `*.h` | Unmodified C oracle |
| `zopfli-core/` | All compression logic; `#![forbid(unsafe_code)]` |
| `zopfli-ffi/` | Thin C ABI (`staticlib` + `cdylib`) |
| `zopfli-driver/` | Rust differential driver |
| `tools/zopfli-oracle.c` | C differential driver (public header only) |
| `tests/inputs/` | Parity fixtures |

Algorithm modules in `zopfli-core` are adapted from the Apache-2.0 [zopfli-rs](https://github.com/zopfli-rs/zopfli) v0.8.3 reimplementation of Google's C Zopfli (same license as this tree). Hash tables use heap `Vec`s so the crate stays `forbid(unsafe_code)`. Byte identity vs the in-tree C oracle is enforced by [Parity Gate].

## In scope / out of scope

**In scope:** installed public header `src/zopfli/zopfli.h` (`ZopfliInitOptions`, `ZopfliCompress`) and the C sources that implement them under `src/zopfli/`.

**Out of scope:** `src/zopflipng/` (C++ PNG optimizer), `zopfli_bin` / `zopflipng_bin` CLI programs, Go wrappers under `go/`, internal headers as FFI exports (`deflate.h`, `lz77.h`, …).

## Behavior kept on purpose

- Match C compressed bytes including DEFLATE tree encoding quirks.
- Default `ZopfliOptions` values from `util.c` / `ZopfliInitOptions`.
- Gzip/zlib container framing identical to C.

## Behavior changed on purpose (approved)

None.

## Not covered

- ZopfliPNG
- Allocator hook-trace (library has no custom allocator hooks or callbacks)
- Full CLI flag surface of `zopfli_bin.c`
- White-box C `static` internals (no C unit suite in-tree to port)

## Unsafe audit

```sh
rg -n 'unsafe' --glob '*.rs' --glob '!target/**'
```

Executable `unsafe` appears only under `zopfli-ffi/` (and its ABI tests). `zopfli-core` has `#![forbid(unsafe_code)]`.

## Export check

Pattern: plain column-0 prototypes in `src/zopfli/zopfli.h` (no export macro).

```sh
printf 'ZopfliInitOptions\nZopfliCompress\n' | sort > build/header-fns.txt
nm -gD target/release/libzopfli_ffi.so | awk '$2=="T"{print $3}' | sed 's/^_//' | sort -u > build/ffi-exports.txt
comm -23 build/header-fns.txt build/ffi-exports.txt
```

Result: empty (both symbols exported). `wc -l build/header-fns.txt` → 2.

## Tests

- No in-tree C unit suite; public API covered by `zopfli-core/tests/api.rs` and `zopfli-ffi/tests/abi.rs`.
- Go CGO tests (`go/zopfli`) remain linked against system/C `libzopfli` when built separately; not required for the parity gate.
- Removed FFI cases: none (no white-box C suite).

## SonarQube findings summary

[SonarQube MCP] `search_my_sonarqube_projects` was queried. **No stored analysis** for this Zopfli repository exists in the organization (listed projects are unrelated: cJSON / heatshrink / INI). Zero findings to summarize. A CI quality gate must not fail on legacy C alone; none is configured for this repo yet.

## CI / Sonar policy

If Sonar is wired later, gate on new code only so legacy C issues alone cannot fail the port.
