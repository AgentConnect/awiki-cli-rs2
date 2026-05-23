#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

scripts/flutter/codegen.sh

git diff --exit-code -- \
  crates/im-core-dart/src/frb_generated.rs \
  packages/awiki_im_core/lib/src/generated
