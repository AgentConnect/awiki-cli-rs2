#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

VERSION_OR_TAG="${1:-}"

if [[ -z "${VERSION_OR_TAG}" ]]; then
  echo "Usage: scripts/release/delete-tag.sh <version-or-tag>" >&2
  echo "Example: scripts/release/delete-tag.sh 1.0.0" >&2
  echo "         scripts/release/delete-tag.sh v1.0.0" >&2
  exit 1
fi

if [[ "${VERSION_OR_TAG}" == v* ]]; then
  TAG="${VERSION_OR_TAG}"
else
  TAG="v${VERSION_OR_TAG}"
fi

echo "Deleting tag ${TAG} from local repository and origin..."

if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  git tag -d "${TAG}"
else
  echo "Local tag ${TAG} does not exist; skipping local deletion."
fi

if git remote get-url origin >/dev/null 2>&1; then
  if git ls-remote --tags origin "refs/tags/${TAG}" | grep -q .; then
    git push origin ":refs/tags/${TAG}"
  else
    echo "Remote tag ${TAG} does not exist on origin; skipping remote deletion."
  fi
else
  echo "Remote origin is not configured; skipping remote deletion."
fi

echo "Done."
