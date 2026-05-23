#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

LIB_NAME="awiki_im_core"
IOS_FRAMEWORK_DIR="${ROOT_DIR}/packages/awiki_im_core/ios/Frameworks"
MACOS_FRAMEWORK_DIR="${ROOT_DIR}/packages/awiki_im_core/macos/Frameworks"
IOS_INCLUDE_DIR="${ROOT_DIR}/packages/awiki_im_core/ios/include"
MACOS_INCLUDE_DIR="${ROOT_DIR}/packages/awiki_im_core/macos/include"
IOS_XCFRAMEWORK="${IOS_FRAMEWORK_DIR}/AwikiImCore.xcframework"
MACOS_XCFRAMEWORK="${MACOS_FRAMEWORK_DIR}/AwikiImCore.xcframework"
TARGETS=(
  aarch64-apple-ios
  aarch64-apple-ios-sim
  x86_64-apple-ios
  aarch64-apple-darwin
  x86_64-apple-darwin
)

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would rustup target add: ${TARGETS[*]}"
  echo "Would build staticlibs and create iOS/macOS XCFrameworks"
  exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Apple build must run on macOS." >&2
  exit 1
fi

rustup target add "${TARGETS[@]}"

for target in "${TARGETS[@]}"; do
  cargo build \
    -p im-core-dart \
    --release \
    --target "${target}" \
    --no-default-features \
    --features blocking,sqlite,http,ios,macos
done

mkdir -p "${IOS_FRAMEWORK_DIR}" "${MACOS_FRAMEWORK_DIR}" "${IOS_INCLUDE_DIR}" "${MACOS_INCLUDE_DIR}"
cat > "${IOS_INCLUDE_DIR}/awiki_im_core.h" <<'HEADER'
#pragma once
HEADER
cat > "${MACOS_INCLUDE_DIR}/awiki_im_core.h" <<'HEADER'
#pragma once
HEADER

SIM_DIR="$(mktemp -d "${TMPDIR:-/tmp}/awiki-ios-sim.XXXXXX")"
MACOS_DIR="$(mktemp -d "${TMPDIR:-/tmp}/awiki-macos.XXXXXX")"

lipo -create \
  "target/aarch64-apple-ios-sim/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-ios/release/lib${LIB_NAME}.a" \
  -output "${SIM_DIR}/lib${LIB_NAME}.a"

lipo -create \
  "target/aarch64-apple-darwin/release/lib${LIB_NAME}.a" \
  "target/x86_64-apple-darwin/release/lib${LIB_NAME}.a" \
  -output "${MACOS_DIR}/lib${LIB_NAME}.a"

rm -rf "${IOS_XCFRAMEWORK}" "${MACOS_XCFRAMEWORK}"

xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/release/lib${LIB_NAME}.a" \
  -headers "${IOS_INCLUDE_DIR}" \
  -library "${SIM_DIR}/lib${LIB_NAME}.a" \
  -headers "${IOS_INCLUDE_DIR}" \
  -output "${IOS_XCFRAMEWORK}"

xcodebuild -create-xcframework \
  -library "${MACOS_DIR}/lib${LIB_NAME}.a" \
  -headers "${MACOS_INCLUDE_DIR}" \
  -output "${MACOS_XCFRAMEWORK}"
