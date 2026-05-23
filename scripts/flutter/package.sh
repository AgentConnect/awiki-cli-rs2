#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

PACKAGE_DIR="${ROOT_DIR}/packages/awiki_im_core"
VERSION="$(python3 - <<'PY'
from pathlib import Path
for line in Path('packages/awiki_im_core/pubspec.yaml').read_text().splitlines():
    if line.startswith('version:'):
        print(line.split(':', 1)[1].strip())
        break
else:
    raise SystemExit('version not found')
PY
)"
OUT_DIR="${ROOT_DIR}/dist/flutter"
ARCHIVE="${OUT_DIR}/awiki_im_core-${VERSION}.tar.gz"

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Would verify generated bindings with scripts/flutter/codegen-check.sh"
  echo "Would run Flutter package checks in ${PACKAGE_DIR}"
  echo "Would create ${ARCHIVE} from packages/awiki_im_core"
  exit 0
fi

scripts/flutter/codegen-check.sh
(
  cd "${PACKAGE_DIR}"
  flutter pub get
  dart analyze
  flutter test
)

mkdir -p "${OUT_DIR}"
tar \
  --exclude='.dart_tool' \
  --exclude='build' \
  --exclude='pubspec.lock' \
  --exclude='.flutter-plugins' \
  --exclude='.flutter-plugins-dependencies' \
  -czf "${ARCHIVE}" \
  -C "${ROOT_DIR}/packages" \
  awiki_im_core

echo "Wrote ${ARCHIVE}"
