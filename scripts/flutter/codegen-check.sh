#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

BEFORE="$(mktemp "${TMPDIR:-/tmp}/awiki-codegen-before.XXXXXX")"
AFTER="$(mktemp "${TMPDIR:-/tmp}/awiki-codegen-after.XXXXXX")"
DIFF_OUT="$(mktemp "${TMPDIR:-/tmp}/awiki-codegen-diff.XXXXXX")"
trap 'rm -f "${BEFORE}" "${AFTER}" "${DIFF_OUT}"' EXIT

git diff -- \
  crates/im-core-dart/src/frb_generated.rs \
  packages/awiki_im_core/lib/src/generated > "${BEFORE}"

scripts/flutter/codegen.sh

git diff -- \
  crates/im-core-dart/src/frb_generated.rs \
  packages/awiki_im_core/lib/src/generated > "${AFTER}"

if ! diff -u "${BEFORE}" "${AFTER}" > "${DIFF_OUT}"; then
  echo "Generated Flutter/Rust bridge files changed after running codegen." >&2
  cat "${DIFF_OUT}" >&2
  exit 1
fi
