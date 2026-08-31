#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
ABI=""

usage() {
  cat <<'USAGE'
Usage: scripts/flutter/build-android.sh [--dry-run] [--abi arm64-v8a|x86_64|armeabi-v7a]

Builds every supported Android ABI by default. Use --abi when the consuming
package intentionally targets one ABI.
USAGE
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      ;;
    --abi)
      if [[ "$#" -lt 2 || -z "${2:-}" ]]; then
        echo "--abi requires an Android ABI." >&2
        exit 2
      fi
      ABI="$2"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
  shift
done

OUT_DIR="${ROOT_DIR}/packages/awiki_im_core/android/src/main/jniLibs"
TARGETS=(aarch64-linux-android x86_64-linux-android armv7-linux-androideabi)
ABIS=(arm64-v8a x86_64 armeabi-v7a)
if [[ -n "${ABI}" ]]; then
  case "${ABI}" in
    arm64-v8a)
      TARGETS=(aarch64-linux-android)
      ;;
    x86_64)
      TARGETS=(x86_64-linux-android)
      ;;
    armeabi-v7a)
      TARGETS=(armv7-linux-androideabi)
      ;;
    *)
      echo "Unsupported Android ABI: ${ABI}" >&2
      exit 2
      ;;
  esac
  ABIS=("${ABI}")
fi

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would cargo ndk build ${ABIS[*]} into ${OUT_DIR}"
  exit 0
fi

rustup target add "${TARGETS[@]}"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is required. Install a version compatible with the workspace toolchain." >&2
  exit 1
fi

# Keep the generated native directory aligned with the selected ABI set while
# preserving its tracked directory placeholders.
mkdir -p "${OUT_DIR}"
find "${OUT_DIR}" -type f -name "*.so" -delete

NDK_TARGET_ARGS=()
for abi in "${ABIS[@]}"; do
  NDK_TARGET_ARGS+=(-t "${abi}")
done

cargo ndk \
  "${NDK_TARGET_ARGS[@]}" \
  -o "${OUT_DIR}" \
  build \
  -p im-core-dart \
  --release \
  --no-default-features \
  --features blocking,sqlite,http,android,group-e2ee,secure-direct,identity-native-anp
