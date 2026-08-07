#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
RELEASE_CONFIG="${SCRIPT_DIR}/release-config.json"
SERVER_CONFIG="${SCRIPT_DIR}/publish-server.toml"
CHANNEL=""

usage() { echo "Usage: $0 [--config FILE] beta|stable"; }
while [[ $# -gt 0 ]]; do
  case "$1" in
    --config) SERVER_CONFIG="${2:-}"; shift 2 ;;
    beta|stable) CHANNEL="$1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Error: unknown argument $1" >&2; usage >&2; exit 2 ;;
  esac
done
[[ -n "${CHANNEL}" ]] || { usage >&2; exit 2; }
[[ -f "${SERVER_CONFIG}" ]] || { echo "Error: missing server config ${SERVER_CONFIG}" >&2; exit 1; }
mode="$(stat -c '%a' "${SERVER_CONFIG}" 2>/dev/null || stat -f '%Lp' "${SERVER_CONFIG}")"
[[ "${mode}" == "600" || "${mode}" == "400" ]] || { echo "Error: ${SERVER_CONFIG} must have mode 0600 or 0400 (got ${mode})" >&2; exit 1; }

cfg() { node "${SCRIPT_DIR}/config.js" server "${SERVER_CONFIG}" "$1"; }
rel() { node "${SCRIPT_DIR}/config.js" release "${RELEASE_CONFIG}" "$1"; }
PUBLIC_ORIGIN="$(cfg public_origin)"
PUBLIC_BASE_PATH="$(cfg public_base_path)"
WEB_ROOT="$(cfg web_root)"
ARCHIVE_ROOT="$(cfg archive_root)"
GITHUB_REPO="$(cfg github_repo)"
WORKFLOW="$(cfg github_workflow)"
GH_TOKEN_VALUE="$(cfg github_token)"
VERSION="$(rel "channels.${CHANNEL}.version")"
KEEP_VERSIONS="$(rel archive_keep_versions)"
TAG="cli-v${VERSION}"

command -v gh >/dev/null && command -v node >/dev/null && command -v npm >/dev/null && command -v curl >/dev/null || { echo "Error: gh, node, npm, and curl are required" >&2; exit 1; }
exec 9>"/tmp/awiki-cli-release.lock"
flock -n 9 || { echo "Error: another awiki-cli release is running" >&2; exit 1; }

tag_commit="$(GH_TOKEN="${GH_TOKEN_VALUE}" gh api "repos/${GITHUB_REPO}/git/ref/tags/${TAG}" --jq '.object.sha')"
if [[ "$(GH_TOKEN="${GH_TOKEN_VALUE}" gh api "repos/${GITHUB_REPO}/git/tags/${tag_commit}" --jq '.object.type' 2>/dev/null || true)" == "commit" ]]; then
  tag_commit="$(GH_TOKEN="${GH_TOKEN_VALUE}" gh api "repos/${GITHUB_REPO}/git/tags/${tag_commit}" --jq '.object.sha')"
fi

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# Keep the workflow definition and the checked-out source on the same immutable release tag.
GH_TOKEN="${GH_TOKEN_VALUE}" gh workflow run "${WORKFLOW}" --repo "${GITHUB_REPO}" --ref "${TAG}" \
  -f source_ref="${TAG}" -f channel="${CHANNEL}" -f expected_version="${VERSION}"
run_id=""
for _ in $(seq 1 60); do
  run_id="$(GH_TOKEN="${GH_TOKEN_VALUE}" gh run list --repo "${GITHUB_REPO}" --workflow "${WORKFLOW}" --limit 30 \
    --json databaseId,name,createdAt,headSha --jq ".[] | select(.name == \"CLI ${CHANNEL} ${TAG}\" and .headSha == \"${tag_commit}\" and .createdAt >= \"${started_at}\") | .databaseId" | head -n1)"
  [[ -n "${run_id}" ]] && break
  sleep 2
done
[[ -n "${run_id}" ]] || { echo "Error: could not locate workflow run for ${TAG}" >&2; exit 1; }
GH_TOKEN="${GH_TOKEN_VALUE}" gh run watch "${run_id}" --repo "${GITHUB_REPO}" --exit-status

tmp="$(mktemp -d /tmp/awiki-cli-publish.XXXXXX)"
trap 'rm -rf "${tmp}"' EXIT
mkdir -p "${tmp}/downloads" "${tmp}/artifacts" "${tmp}/stage"
GH_TOKEN="${GH_TOKEN_VALUE}" gh run download "${run_id}" --repo "${GITHUB_REPO}" --dir "${tmp}/downloads"
while IFS= read -r -d '' artifact; do
  artifact_name="$(basename "${artifact}")"
  [[ ! -e "${tmp}/artifacts/${artifact_name}" ]] || {
    echo "Error: duplicate workflow artifact ${artifact_name}" >&2
    exit 1
  }
  cp "${artifact}" "${tmp}/artifacts/${artifact_name}"
done < <(find "${tmp}/downloads" -type f -name 'awiki-cli-*' -print0)
node "${SCRIPT_DIR}/stage-release.js" --channel "${CHANNEL}" --release-config "${RELEASE_CONFIG}" \
  --server-config "${SERVER_CONFIG}" --artifacts "${tmp}/artifacts" --output "${tmp}/stage" \
  --source-tag "${TAG}" --source-commit "${tag_commit}"

release_dir="${ARCHIVE_ROOT}/releases/${VERSION}/${CHANNEL}"
sudo mkdir -p "$(dirname "${release_dir}")" "${ARCHIVE_ROOT}/channels" "${WEB_ROOT}"
[[ ! -e "${release_dir}" ]] || { echo "Error: release archive already exists at ${release_dir}" >&2; exit 1; }
sudo rm -rf "${release_dir}.staging"
sudo cp -R "${tmp}/stage" "${release_dir}.staging"
sudo chmod -R u=rwX,go=rX "${release_dir}.staging"
sudo mv "${release_dir}.staging" "${release_dir}"
link_tmp="${WEB_ROOT}/.${CHANNEL}.${VERSION}.$$"
sudo ln -s "${release_dir}" "${link_tmp}"
sudo mv -Tf "${link_tmp}" "${WEB_ROOT}/${CHANNEL}"

if [[ "${CHANNEL}" == "stable" ]]; then
  onboarding_tmp="${ARCHIVE_ROOT}/channels/.stable-onboarding.$$"
  sudo ln -s "${release_dir}/onboarding.md" "${onboarding_tmp}"
  sudo mv -Tf "${onboarding_tmp}" "${ARCHIVE_ROOT}/channels/stable-onboarding.md"
  public_onboarding_tmp="${WEB_ROOT}/.onboarding.${VERSION}.$$"
  sudo ln -s "${release_dir}/onboarding.md" "${public_onboarding_tmp}"
  sudo mv -Tf "${public_onboarding_tmp}" "${WEB_ROOT}/onboarding.md"
fi

mapfile -t versions < <(sudo find "${ARCHIVE_ROOT}/releases" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' | sort -rn | awk '{print $2}')
if (( ${#versions[@]} > KEEP_VERSIONS )); then
  for old in "${versions[@]:KEEP_VERSIONS}"; do
    [[ "${old}" == "${ARCHIVE_ROOT}/releases/${VERSION}" ]] || sudo rm -rf "${old}"
  done
fi

curl -fsS "${PUBLIC_ORIGIN}${PUBLIC_BASE_PATH}/${CHANNEL}/manifest.json" | node -e \
  'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const m=JSON.parse(s);if(m.latest!==process.argv[1])process.exit(1)})' "${VERSION}"
curl -fsSI "${PUBLIC_ORIGIN}${PUBLIC_BASE_PATH}/${CHANNEL}/awiki-cli.tgz" >/dev/null
echo "channel=${CHANNEL}"
echo "version=${VERSION}"
echo "tag=${TAG}"
echo "commit=${tag_commit}"
echo "github_actions_run=${run_id}"
echo "manifest=${PUBLIC_ORIGIN}${PUBLIC_BASE_PATH}/${CHANNEL}/manifest.json"
