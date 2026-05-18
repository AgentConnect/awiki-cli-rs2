#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

SCRIPT_NAME="scripts/release/release-tag-stable.sh"
# shellcheck source=scripts/release/_release_tag_shared.sh
source "${ROOT_DIR}/scripts/release/_release_tag_shared.sh"

release_require_command node "${SCRIPT_NAME}"

VERSION="$(release_read_version "${ROOT_DIR}")"
if [[ "${VERSION}" == *-* ]]; then
  echo "Error: stable release version must not contain a pre-release suffix, got ${VERSION}" >&2
  exit 1
fi

TAG="v${VERSION}"

release_require_clean_worktree
BRANCH="$(release_require_branch_with_upstream)"

echo "Ensuring stable release tag ${TAG} on branch ${BRANCH}..."
release_ensure_tag_on_remote "${TAG}" "Release ${TAG}" origin

cat <<EOF

Done. Tag ${TAG} is ensured on origin; CI should pick it up and run or reuse the release workflow.

After the GitHub release finishes, mirror the release assets to Gitee from your local machine:

  GITEE_USERNAME=... GITEE_TOKEN=... scripts/release/publish-gitee-release.sh ${TAG}

EOF
