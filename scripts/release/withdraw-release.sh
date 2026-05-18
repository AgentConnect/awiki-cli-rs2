#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

VERSION="${1:-}"

if [ -z "${VERSION}" ]; then
  echo "Usage: scripts/release/withdraw-release.sh <version>" >&2
  echo "Example: scripts/release/withdraw-release.sh 0.1.0" >&2
  echo "         scripts/release/withdraw-release.sh 0.2.0-beta.1" >&2
  exit 1
fi

TAG="v${VERSION}"
EXECUTE="${AWIKI_CLI_WITHDRAW_EXECUTE:-0}"

echo "Preparing to withdraw awiki-cli version ${VERSION} (tag ${TAG})."
echo

if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "Warning: local tag ${TAG} does not exist." >&2
else
  echo "Local tag ${TAG} exists."
fi

if git ls-remote --tags origin "refs/tags/${TAG}" | grep -q .; then
  echo "Remote tag ${TAG} exists on origin."
else
  echo "Warning: remote tag ${TAG} does not exist on origin." >&2
fi

echo
echo "Recommended rollback actions:"
echo
echo "1) Delete Git tag (local + origin):"
echo "   git tag -d ${TAG} || true"
echo "   git push origin :refs/tags/${TAG} || true"
echo
echo "2) Adjust GitHub Release (requires GitHub CLI 'gh'):"
echo "   gh release delete ${TAG} --yes || true"
echo
echo "3) Adjust npm registry state for @awiki/cli@${VERSION}:"
echo "   # Option A: mark deprecated but keep the version:"
echo "   npm deprecate @awiki/cli@${VERSION} \"Deprecated due to bad release; please upgrade.\""
echo
echo "   # Option B: move dist-tags away from the bad version (if applicable):"
echo "   # npm dist-tag add @awiki/cli@<good-version> latest"
echo "   # npm dist-tag rm @awiki/cli@${VERSION} latest"
echo
echo "   # Option C: unpublish (only for very new versions; may be restricted by npm policy):"
echo "   # npm unpublish @awiki/cli@${VERSION}"
echo

if [ "${EXECUTE}" != "1" ]; then
  echo "AWIKI_CLI_WITHDRAW_EXECUTE is not set to 1; no destructive actions will be executed."
  echo "Review the above commands carefully, then re-run with:"
  echo
  echo "  AWIKI_CLI_WITHDRAW_EXECUTE=1 scripts/release/withdraw-release.sh ${VERSION}"
  echo
  exit 0
fi

echo "AWIKI_CLI_WITHDRAW_EXECUTE=1 set; executing rollback commands..."

set +e

git tag -d "${TAG}" 2>/dev/null || true
git push origin ":refs/tags/${TAG}" || true

if command -v gh >/dev/null 2>&1; then
  gh release delete "${TAG}" --yes || true
else
  echo "Warning: GitHub CLI 'gh' not found; skipping GitHub Release deletion." >&2
fi

echo "Rollback commands executed. You may still need to run npm deprecate/dist-tag commands manually as appropriate."
