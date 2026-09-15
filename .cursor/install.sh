#!/usr/bin/env bash
# Idempotent Cloud Agent bootstrap for the Zopfli C/C++ library, the zopfli and
# zopflipng command-line binaries, and the Go cgo bindings under go/.
#
# Building with the provided Makefile produces the binaries plus the shared
# libraries. Those shared libraries and their public headers are then installed
# under /usr/local so the cgo bindings (which link with -lzopfli / -lzopflipng
# and include "zopfli.h" / "zopflipng_lib.h") build and test without extra flags.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The bundled CMakeLists.txt probes the default cc/c++ (Clang on this image),
# whose C++ driver cannot find libstdc++; the Makefile uses gcc/g++, which work.
export CC="${CC:-gcc}"
export CXX="${CXX:-g++}"

# Build binaries (zopfli, zopflipng), static libs, and shared libs.
make -j"$(nproc)"

# Install shared libraries + headers so the Go cgo bindings can link/include.
lib_ver="1.0.3"
lib_major="1"
sudo install -d /usr/local/lib /usr/local/include

install_shared_lib() {
  local name="$1" # e.g. libzopfli
  sudo install -m 0644 "${name}.so.${lib_ver}" /usr/local/lib/
  sudo ln -sf "${name}.so.${lib_ver}" "/usr/local/lib/${name}.so.${lib_major}"
  sudo ln -sf "${name}.so.${lib_major}" "/usr/local/lib/${name}.so"
}

install_shared_lib libzopfli
install_shared_lib libzopflipng

sudo install -m 0644 src/zopfli/zopfli.h /usr/local/include/
sudo install -m 0644 src/zopflipng/zopflipng_lib.h /usr/local/include/

# Refresh the dynamic linker cache so -lzopfli / -lzopflipng resolve.
sudo ldconfig

# Warm the Go build/module cache for the cgo bindings (best effort; the bindings
# have no external module dependencies, so this is fast and never fails setup).
if command -v go >/dev/null 2>&1; then
  GO111MODULE=off go build ./go/... >/dev/null 2>&1 || true
fi

echo "Zopfli environment ready: binaries built, shared libraries + headers installed."
