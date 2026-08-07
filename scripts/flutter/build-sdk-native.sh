#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

RUN_CODEGEN_CHECK=1
BUILD_APPLE=1
BUILD_ANDROID=1
BUILD_LINUX=0
BUILD_WINDOWS=0
APPLE_ARGS=()
MACOS_ARCH=""
ANDROID_ABI=""
DRY_RUN=0

usage() {
  cat <<'USAGE'
Usage: scripts/flutter/build-sdk-native.sh [--dry-run] [--apple-only|--android-only|--linux-only|--windows-only|--ios-only|--macos-only] [--macos-arch arm64|x86_64] [--android-abi ABI] [--skip-codegen-check]

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
  --windows-only        Run codegen check and build the Windows x64 artifact only.
  --ios-only            Run codegen check and build iOS artifacts only.
  --macos-only          Run codegen check and build macOS artifacts only.
  --macos-arch ARCH     Limit --macos-only to arm64 or x86_64.
  --android-abi ABI     Limit --android-only to one supported Android ABI.
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
      BUILD_WINDOWS=0
      APPLE_ARGS=()
      shift
      ;;
    --android-only)
      BUILD_APPLE=0
      BUILD_ANDROID=1
      BUILD_LINUX=0
      BUILD_WINDOWS=0
      APPLE_ARGS=()
      shift
      ;;
    --linux-only)
      BUILD_APPLE=0
      BUILD_ANDROID=0
      BUILD_LINUX=1
      BUILD_WINDOWS=0
      APPLE_ARGS=()
      shift
      ;;
    --windows-only)
      BUILD_APPLE=0
      BUILD_ANDROID=0
      BUILD_LINUX=0
      BUILD_WINDOWS=1
      APPLE_ARGS=()
      shift
      ;;
    --ios-only)
      BUILD_APPLE=1
      BUILD_ANDROID=0
      BUILD_LINUX=0
      BUILD_WINDOWS=0
      APPLE_ARGS=(--ios)
      shift
      ;;
    --macos-only)
      BUILD_APPLE=1
      BUILD_ANDROID=0
      BUILD_LINUX=0
      BUILD_WINDOWS=0
      APPLE_ARGS=(--macos)
      shift
      ;;
    --macos-arch)
      [[ "$#" -ge 2 && -n "${2:-}" ]] || die "--macos-arch requires a value"
      MACOS_ARCH="$2"
      shift 2
      ;;
    --android-abi)
      [[ "$#" -ge 2 && -n "${2:-}" ]] || die "--android-abi requires a value"
      ANDROID_ABI="$2"
      shift 2
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

if [[ "${BUILD_APPLE}" != "1" && "${BUILD_ANDROID}" != "1" && "${BUILD_LINUX}" != "1" && "${BUILD_WINDOWS}" != "1" ]]; then
  die "at least one platform must be selected"
fi
if [[ -n "${MACOS_ARCH}" ]]; then
  if [[ "${BUILD_APPLE}" != "1" || "${APPLE_ARGS[0]:-}" != "--macos" ]]; then
    die "--macos-arch requires --macos-only"
  fi
  case "${MACOS_ARCH}" in
    arm64|x86_64) ;;
    *) die "unsupported macOS architecture: ${MACOS_ARCH}" ;;
  esac
  APPLE_ARGS+=(--macos-arch "${MACOS_ARCH}")
fi
if [[ -n "${ANDROID_ABI}" ]]; then
  if [[ "${BUILD_ANDROID}" != "1" || "${BUILD_APPLE}" == "1" || "${BUILD_LINUX}" == "1" || "${BUILD_WINDOWS}" == "1" ]]; then
    die "--android-abi requires --android-only"
  fi
  case "${ANDROID_ABI}" in
    arm64-v8a|x86_64|armeabi-v7a) ;;
    *) die "unsupported Android ABI: ${ANDROID_ABI}" ;;
  esac
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
    if [[ -n "${ANDROID_ABI}" ]]; then
      echo "  would run: scripts/flutter/build-android.sh --abi ${ANDROID_ABI}"
    else
      echo "  would run: scripts/flutter/build-android.sh"
    fi
  fi
  if [[ "${BUILD_LINUX}" == "1" ]]; then
    echo "  would run: scripts/flutter/build-linux.sh"
  fi
  if [[ "${BUILD_WINDOWS}" == "1" ]]; then
    echo "  would run: scripts/flutter/build-windows.ps1"
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
  if [[ -n "${ANDROID_ABI}" ]]; then
    scripts/flutter/build-android.sh --abi "${ANDROID_ABI}"
  else
    scripts/flutter/build-android.sh
  fi
fi

if [[ "${BUILD_LINUX}" == "1" ]]; then
  scripts/flutter/build-linux.sh
fi

if [[ "${BUILD_WINDOWS}" == "1" ]]; then
  if command -v pwsh >/dev/null 2>&1; then
    pwsh -NoProfile -File scripts/flutter/build-windows.ps1
  elif command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/flutter/build-windows.ps1
  else
    die "PowerShell is required for the Windows native SDK build"
  fi
fi
