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

## In scope / out of scope

**In scope:** installed public header `src/zopfli/zopfli.h` (`ZopfliInitOptions`, `ZopfliCompress`) and the C sources that implement them under `src/zopfli/`.

**Out of scope:** `src/zopflipng/` (C++ PNG optimizer), `zopfli_bin` / `zopflipng_bin` CLI programs, Go wrappers under `go/`, internal headers as FFI exports (`deflate.h`, `lz77.h`, …).

## Behavior kept on purpose

- Match C compressed bytes including DEFLATE tree encoding quirks.
- Default `ZopfliOptions` values from `util.c` / `ZopfliInitOptions`.

## Behavior changed on purpose (approved)

None.

## Not covered

- ZopfliPNG
- Allocator hook-trace (library has no custom allocator hooks or callbacks)
- Full CLI flag surface of `zopfli_bin.c`

## SonarQube findings summary

[SonarQube MCP] `search_my_sonarqube_projects` was queried. No SonarQube project for this Zopfli repository is present in the organization (projects listed are unrelated: cJSON / heatshrink / INI). **No stored analysis** for this port. No findings to summarize. A CI quality gate must not fail on legacy C alone; none is configured for this repo yet.

## Export check pattern

Plain column-0 prototypes in `src/zopfli/zopfli.h` (no export macro). Expected FFI exports: `ZopfliInitOptions`, `ZopfliCompress`.

## Status

Phase 1 baseline scaffolding in progress; core compress is still a stub until the algorithm port lands.
