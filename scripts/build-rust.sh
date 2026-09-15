#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUST_DIR="${REPO}/rust"

if [[ ! -f "${RUST_DIR}/Cargo.toml" ]]; then
  echo "error: ${RUST_DIR}/Cargo.toml is missing." >&2
  echo "The Rust workspace must land first (Agent 1: rust/Cargo.toml, rust/zopfli-core, rust/zopfli-ffi)." >&2
  echo "Until then, cargo build --release cannot run." >&2
  exit 1
fi

cd "${RUST_DIR}"
cargo build --release
