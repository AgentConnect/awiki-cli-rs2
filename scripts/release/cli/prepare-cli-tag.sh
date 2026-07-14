#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
CONFIG="${SCRIPT_DIR}/release-config.json"
CHANNEL="${1:-}"
[[ "${CHANNEL}" == "beta" || "${CHANNEL}" == "stable" ]] || { echo "Usage: $0 beta|stable" >&2; exit 2; }
cd "${ROOT_DIR}"
version="$(node "${SCRIPT_DIR}/config.js" release "${CONFIG}" "channels.${CHANNEL}.version")"
tag="cli-v${version}"
[[ -z "$(git status --porcelain)" ]] || { echo "Error: worktree must be clean" >&2; exit 1; }
branch="$(git branch --show-current)"
[[ -n "${branch}" ]] || { echo "Error: detached HEAD is not allowed" >&2; exit 1; }
git fetch origin "${branch}" --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse "origin/${branch}")" ]] || { echo "Error: ${branch} is not synchronized with origin" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/${tag}" >/dev/null && { echo "Error: tag ${tag} already exists" >&2; exit 1; }
git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1 && { echo "Error: remote tag ${tag} already exists" >&2; exit 1; }
git tag -a "${tag}" -m "Release awiki-cli ${version} (${CHANNEL})"
git push origin "refs/tags/${tag}"
echo "created_tag=${tag}"
echo "commit=$(git rev-parse HEAD)"
