#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
TARGET="${AWIKI_IM_CORE_LINUX_TARGET:-x86_64-unknown-linux-gnu}"

usage() {
  cat <<'USAGE'
Usage: scripts/flutter/build-linux.sh [--dry-run]

Build the awiki_im_core Linux native library and copy it into the Flutter
package's Linux plugin bundle directory.

Environment:
  AWIKI_IM_CORE_LINUX_TARGET  Rust target triple. Defaults to x86_64-unknown-linux-gnu.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

OUT_DIR="${ROOT_DIR}/packages/awiki_im_core/linux/lib"
SOURCE_LIB="${ROOT_DIR}/target/${TARGET}/release/libawiki_im_core.so"
DEST_LIB="${OUT_DIR}/libawiki_im_core.so"

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGET}"
  echo "Would cargo build im-core-dart for ${TARGET}"
  echo "Would copy ${SOURCE_LIB} to ${DEST_LIB}"
  exit 0
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux native SDK build must run on Linux." >&2
  exit 1
fi

rustup target add "${TARGET}"

cargo build \
  -p im-core-dart \
  --release \
  --target "${TARGET}" \
  --no-default-features \
  --features blocking,sqlite,http,linux,group-e2ee,identity-native-anp

mkdir -p "${OUT_DIR}"
cp "${SOURCE_LIB}" "${DEST_LIB}"
