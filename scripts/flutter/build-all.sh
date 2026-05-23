#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

scripts/flutter/codegen.sh
scripts/flutter/build-host.sh
scripts/flutter/build-android.sh --dry-run

if [[ "$(uname -s)" == "Darwin" ]]; then
  scripts/flutter/build-apple.sh --dry-run
fi

(
  cd packages/awiki_im_core
  flutter pub get
  dart analyze
  flutter test
)
