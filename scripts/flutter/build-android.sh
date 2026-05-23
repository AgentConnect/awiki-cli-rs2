#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

OUT_DIR="${ROOT_DIR}/packages/awiki_im_core/android/src/main/jniLibs"
TARGETS=(aarch64-linux-android x86_64-linux-android armv7-linux-androideabi)

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would cargo ndk build arm64-v8a x86_64 armeabi-v7a into ${OUT_DIR}"
  exit 0
fi

rustup target add "${TARGETS[@]}"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is required. Install a version compatible with the workspace toolchain." >&2
  exit 1
fi

cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  -t armeabi-v7a \
  -o "${OUT_DIR}" \
  build \
  -p im-core-dart \
  --release \
  --no-default-features \
  --features blocking,sqlite,http,android
