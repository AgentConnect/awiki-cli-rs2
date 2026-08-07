#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

usage() {
  echo "Usage: scripts/flutter/verify-native-artifact.sh --ios|--macos" >&2
}

case "${1:-}" in
  --ios) platform=ios ;;
  --macos) platform=macos ;;
  *)
    usage
    exit 2
    ;;
esac
[[ "$#" -eq 1 ]] || {
  usage
  exit 2
}

exec python3 "$ROOT_DIR/scripts/flutter/native-artifact-manifest.py" \
  verify --platform "$platform"
