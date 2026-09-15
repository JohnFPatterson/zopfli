#!/usr/bin/env bash
# Build the Rust staticlibs and run the Go CGO wrappers against them.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Build workspace libs (libzopfli.a / libzopflipng.a under rust/target/release).
"${SCRIPT_DIR}/build-rust.sh"

# Until the workspace is unified, Agent 2 may only emit libzopflipng.a from
# rust/zopflipng-ffi. Build that crate in-place if the workspace output is absent.
if [[ ! -f "${REPO}/rust/target/release/libzopflipng.a" ]] \
   && [[ -f "${REPO}/rust/zopflipng-ffi/Cargo.toml" ]]; then
  echo "libzopflipng.a not in rust/target/release; building rust/zopflipng-ffi" >&2
  cargo build --release --manifest-path "${REPO}/rust/zopflipng-ffi/Cargo.toml"
fi

find_staticlib() {
  local name="$1"
  # Prefer the unified workspace output, then the standalone zopflipng-ffi crate.
  local candidates=(
    "${REPO}/rust/target/release/${name}"
    "${REPO}/rust/zopflipng-ffi/target/release/${name}"
    "${REPO}/rust/zopfli-ffi/target/release/${name}"
  )
  local p
  for p in "${candidates[@]}"; do
    if [[ -f "${p}" ]]; then
      printf '%s\n' "${p}"
      return 0
    fi
  done
  return 1
}

ZOPFLI_LIB="$(find_staticlib libzopfli.a || true)"
ZOPFLIPNG_LIB="$(find_staticlib libzopflipng.a || true)"

if [[ -z "${ZOPFLI_LIB}" ]]; then
  echo "error: libzopfli.a not found." >&2
  echo "Looked in rust/target/release, rust/zopflipng-ffi/target/release, rust/zopfli-ffi/target/release." >&2
  exit 1
fi
if [[ -z "${ZOPFLIPNG_LIB}" ]]; then
  echo "error: libzopflipng.a not found." >&2
  echo "Looked in rust/target/release, then rust/zopflipng-ffi/target/release." >&2
  echo "If Agent 2 has not landed rust/zopflipng-ffi yet, make test-go cannot run." >&2
  exit 1
fi

ZOPFLI_INC="${REPO}/rust/zopfli-ffi/include"
ZOPFLIPNG_INC="${REPO}/rust/zopflipng-ffi/include"
if [[ ! -d "${ZOPFLI_INC}" ]]; then
  echo "error: missing FFI include dir ${ZOPFLI_INC}" >&2
  exit 1
fi
if [[ ! -d "${ZOPFLIPNG_INC}" ]]; then
  echo "error: missing FFI include dir ${ZOPFLIPNG_INC}" >&2
  exit 1
fi

ZOPFLI_LIBDIR="$(cd "$(dirname "${ZOPFLI_LIB}")" && pwd)"
ZOPFLIPNG_LIBDIR="$(cd "$(dirname "${ZOPFLIPNG_LIB}")" && pwd)"
ZOPFLI_LIB="${ZOPFLI_LIBDIR}/$(basename "${ZOPFLI_LIB}")"
ZOPFLIPNG_LIB="${ZOPFLIPNG_LIBDIR}/$(basename "${ZOPFLIPNG_LIB}")"

# Unique -L paths so #cgo LDFLAGS -lzopfli / -lzopflipng resolve.
LFLAGS=("-L${ZOPFLI_LIBDIR}")
if [[ "${ZOPFLIPNG_LIBDIR}" != "${ZOPFLI_LIBDIR}" ]]; then
  LFLAGS+=("-L${ZOPFLIPNG_LIBDIR}")
fi

export CGO_ENABLED=1
export CGO_CFLAGS="-I${ZOPFLI_INC} -I${ZOPFLIPNG_INC}"

OS="$(uname -s)"
case "${OS}" in
  Darwin)
    # force_load pulls the entire Rust staticlib; -L keeps the packages' -lzopfli
    # / -lzopflipng working. -lstdc++ is already in go/zopflipng and is harmless.
    export CGO_LDFLAGS_ALLOW='-Wl,-force_load,.*'
    export CGO_LDFLAGS="-Wl,-force_load,${ZOPFLI_LIB} -Wl,-force_load,${ZOPFLIPNG_LIB} ${LFLAGS[*]} -lm -lstdc++"
    ;;
  *)
    # Linux rustc staticlibs need pthread/dl; gcc_s covers unwind personality.
    export CGO_LDFLAGS="${LFLAGS[*]} -lzopfli -lzopflipng -lm -lpthread -ldl -lstdc++ -lgcc_s"
    ;;
esac

echo "CGO_CFLAGS=${CGO_CFLAGS}"
echo "CGO_LDFLAGS=${CGO_LDFLAGS}"
echo "libzopfli.a=${ZOPFLI_LIB}"
echo "libzopflipng.a=${ZOPFLIPNG_LIB}"

(
  cd "${REPO}/go/zopfli"
  go test -v
)
(
  cd "${REPO}/go/zopflipng"
  go test -v
)
