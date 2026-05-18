#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

SCRIPT_NAME="scripts/release/release-tag-prerelease.sh"
# shellcheck source=scripts/release/_release_tag_shared.sh
source "${ROOT_DIR}/scripts/release/_release_tag_shared.sh"

DIST_TAG="${1:-}"

if [[ -z "${DIST_TAG}" ]]; then
  echo "Usage: ${SCRIPT_NAME} <dist-tag>" >&2
  echo "Example: ${SCRIPT_NAME} beta" >&2
  exit 1
fi

release_require_command node "${SCRIPT_NAME}"

VERSION="$(release_read_version "${ROOT_DIR}")"
if [[ "${VERSION}" != *-* ]]; then
  echo "Error: pre-release version must contain a '-' suffix (e.g. 0.2.0-beta.1), got ${VERSION}" >&2
  exit 1
fi

TAG="v${VERSION}"

release_require_clean_worktree
BRANCH="$(release_require_branch_with_upstream)"

echo "Ensuring pre-release tag ${TAG} (dist-tag: ${DIST_TAG}) on branch ${BRANCH}..."
release_ensure_tag_on_remote "${TAG}" "Pre-release ${TAG} (dist-tag: ${DIST_TAG})" origin

cat <<EOF

Pre-release tag ${TAG} is ensured on origin.

Next steps:
- CI will build binaries and create a GitHub pre-release for ${TAG}.
- After the GitHub pre-release finishes, mirror the release assets to Gitee from your local machine:

    GITEE_USERNAME=... GITEE_TOKEN=... scripts/release/publish-gitee-release.sh ${TAG}

- To publish the npm pre-release package with dist-tag "${DIST_TAG}", run:

    NODE_AUTH_TOKEN=... npm publish --access public --tag ${DIST_TAG}

EOF
