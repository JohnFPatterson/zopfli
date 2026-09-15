#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUST_DIR="${REPO}/rust"

if [[ ! -f "${RUST_DIR}/Cargo.toml" ]]; then
  echo "error: ${RUST_DIR}/Cargo.toml is missing." >&2
  exit 1
fi

cd "${RUST_DIR}"
# Workspace members: zopfli-core, zopfli-ffi, zopflipng, zopflipng-ffi.
# Emits rust/target/release/libzopfli.a and libzopflipng.a.
cargo build --release
