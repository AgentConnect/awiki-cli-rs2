#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

RUN_CODEGEN_CHECK=1
BUILD_APPLE=1
BUILD_ANDROID=1
BUILD_LINUX=0
APPLE_ARGS=()
DRY_RUN=0

usage() {
  cat <<'USAGE'
Usage: scripts/flutter/build-sdk-native.sh [--dry-run] [--apple-only|--android-only|--linux-only|--ios-only|--macos-only] [--skip-codegen-check]

Build the Flutter SDK native artifacts used by awiki_im_core.

Default behavior:
  1. scripts/flutter/codegen-check.sh
  2. scripts/flutter/build-apple.sh
  3. scripts/flutter/build-android.sh

Options:
  --dry-run             Print the build plan without compiling native artifacts.
  --apple-only          Run codegen check and build both iOS and macOS artifacts.
  --android-only        Run codegen check and build Android artifacts only.
  --linux-only          Run codegen check and build Linux artifacts only.
  --ios-only            Run codegen check and build iOS artifacts only.
  --macos-only          Run codegen check and build macOS artifacts only.
  --skip-codegen-check  Skip generated Rust/Dart bridge consistency check.
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --apple-only)
      BUILD_APPLE=1
      BUILD_ANDROID=0
      BUILD_LINUX=0
      APPLE_ARGS=()
      shift
      ;;
    --android-only)
      BUILD_APPLE=0
      BUILD_ANDROID=1
      BUILD_LINUX=0
      APPLE_ARGS=()
      shift
      ;;
    --linux-only)
      BUILD_APPLE=0
      BUILD_ANDROID=0
      BUILD_LINUX=1
      APPLE_ARGS=()
      shift
      ;;
    --ios-only)
      BUILD_APPLE=1
      BUILD_ANDROID=0
      BUILD_LINUX=0
      APPLE_ARGS=(--ios)
      shift
      ;;
    --macos-only)
      BUILD_APPLE=1
      BUILD_ANDROID=0
      BUILD_LINUX=0
      APPLE_ARGS=(--macos)
      shift
      ;;
    --skip-codegen-check)
      RUN_CODEGEN_CHECK=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

if [[ "${BUILD_APPLE}" != "1" && "${BUILD_ANDROID}" != "1" && "${BUILD_LINUX}" != "1" ]]; then
  die "at least one platform must be selected"
fi

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "sdk native build plan"
  if [[ "${RUN_CODEGEN_CHECK}" == "1" ]]; then
    echo "  would run: scripts/flutter/codegen-check.sh"
  else
    echo "  would skip: scripts/flutter/codegen-check.sh"
  fi
  if [[ "${BUILD_APPLE}" == "1" ]]; then
    if [[ "${#APPLE_ARGS[@]}" -gt 0 ]]; then
      echo "  would run: scripts/flutter/build-apple.sh ${APPLE_ARGS[*]}"
    else
      echo "  would run: scripts/flutter/build-apple.sh"
    fi
  fi
  if [[ "${BUILD_ANDROID}" == "1" ]]; then
    echo "  would run: scripts/flutter/build-android.sh"
  fi
  if [[ "${BUILD_LINUX}" == "1" ]]; then
    echo "  would run: scripts/flutter/build-linux.sh"
  fi
  exit 0
fi

if [[ "${RUN_CODEGEN_CHECK}" == "1" ]]; then
  scripts/flutter/codegen-check.sh
fi

if [[ "${BUILD_APPLE}" == "1" ]]; then
  if [[ "${#APPLE_ARGS[@]}" -gt 0 ]]; then
    scripts/flutter/build-apple.sh "${APPLE_ARGS[@]}"
  else
    scripts/flutter/build-apple.sh
  fi
fi

if [[ "${BUILD_ANDROID}" == "1" ]]; then
  scripts/flutter/build-android.sh
fi

if [[ "${BUILD_LINUX}" == "1" ]]; then
  scripts/flutter/build-linux.sh
fi
