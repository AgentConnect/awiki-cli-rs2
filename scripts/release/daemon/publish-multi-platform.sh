#!/usr/bin/env bash
set -euo pipefail

# Publish awiki-deamon packages for linux/amd64, darwin/arm64, and darwin/amd64.
#
# Usage:
#   scripts/release/daemon/publish-multi-platform.sh
#
# This script accepts no arguments and does not require external environment
# variables. Copy the sibling publish-multi-platform.toml.template to
# publish-multi-platform.toml, fill in the GitHub token and release settings,
# then run this script on the server that owns /var/www/awiki-web/daemon.
#
# GitHub Actions builds the packages. This server downloads the workflow
# artifacts, stages install.sh plus manifest.json, and publishes them to the
# local Nginx daemon static directory.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT_DIR="${ROOT_DIR}/scripts/release/daemon"
CONFIG_PATH="${SCRIPT_DIR}/publish-multi-platform.toml"
NGINX_DAEMON_DIR="/var/www/awiki-web/daemon"
TMP_ROOT="/tmp/awiki-daemon-release"
GITHUB_REPO="AgentConnect/awiki-cli-rs2"
WORKFLOW_FILE="build-daemon-release.yml"
# workflow_dispatch must be available from the repository default branch. The
# actual daemon source ref remains configurable through source_ref.
WORKFLOW_REF="main"

cd "${ROOT_DIR}"
export COPYFILE_DISABLE=1

die() {
  echo "Error: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

trim_trailing_slash() {
  local value="$1"
  while [[ "${value%/}" != "${value}" ]]; do
    value="${value%/}"
  done
  printf '%s\n' "${value}"
}

validate_base_url() {
  case "$1" in
    http://*|https://*) ;;
    *) die "base_url must start with http:// or https://" ;;
  esac
}

validate_numeric_version() {
  local value="$1"
  case "${value}" in
    ""|.*|*..*|*.|*[!0123456789.]*)
      die "$2 must be a numeric dotted version, got: ${value}"
      ;;
  esac
}

version_gt() {
  python3 - "$1" "$2" <<'PY'
import sys

left = [int(part) for part in sys.argv[1].split(".")]
right = [int(part) for part in sys.argv[2].split(".")]
width = max(len(left), len(right))
left.extend([0] * (width - len(left)))
right.extend([0] * (width - len(right)))
raise SystemExit(0 if left > right else 1)
PY
}

read_crate_version() {
  awk '
    /^\[package\]$/ {
      in_package = 1;
      next;
    }
    /^\[/ {
      if (in_package) {
        exit;
      }
    }
    in_package && $1 == "version" && $2 == "=" {
      gsub(/"/, "", $3);
      print $3;
      exit;
    }
  ' crates/awiki-deamon/Cargo.toml
}

read_lock_version() {
  awk '
    $0 == "[[package]]" {
      name = "";
      version = "";
      in_package = 1;
      next;
    }
    in_package && $1 == "name" && $2 == "=" {
      gsub(/"/, "", $3);
      name = $3;
    }
    in_package && $1 == "version" && $2 == "=" {
      gsub(/"/, "", $3);
      version = $3;
    }
    in_package && name == "awiki-deamon" && version != "" {
      print version;
      exit;
    }
  ' Cargo.lock
}

read_config() {
  [[ -f "${CONFIG_PATH}" ]] || die "missing config: ${CONFIG_PATH}. Copy publish-multi-platform.toml.template and fill it in."
  python3 - "${CONFIG_PATH}" <<'PY'
import re
import pathlib
import sys

config_path = pathlib.Path(sys.argv[1])
data = {}
for line_no, raw_line in enumerate(config_path.read_text(encoding="utf-8").splitlines(), start=1):
    line = raw_line.strip()
    if not line or line.startswith("#"):
        continue
    match = re.fullmatch(r'([A-Za-z_][A-Za-z0-9_]*)\s*=\s*"((?:[^"\\]|\\.)*)"\s*(?:#.*)?', line)
    if not match:
        raise SystemExit(f"invalid config line {line_no}: only simple quoted string values are supported")
    key, raw_value = match.groups()
    if key in data:
        raise SystemExit(f"duplicate config field {key!r}")
    data[key] = bytes(raw_value, "utf-8").decode("unicode_escape")

required = ["base_url", "source_ref", "github_token"]
for key in required:
    value = data.get(key)
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f"config field {key!r} is required")

allowed = set(required)
for key in data:
    if key not in allowed:
        raise SystemExit(f"unsupported config field {key!r}")

for key in required:
    print(f"{key}={data[key].strip()}")
PY
}

parse_manifest_latest() {
  python3 - "$1" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except Exception as exc:
    raise SystemExit(f"invalid daemon manifest {path}: {exc}") from exc

latest = data.get("latest")
if not isinstance(latest, str) or not latest.strip():
    raise SystemExit(f"daemon manifest {path} is missing latest")

latest = latest.strip()
if latest.startswith("v"):
    latest = latest[1:]
print(latest)
PY
}

find_writable_parent() {
  local path="$1"
  while [[ ! -e "${path}" && "${path}" != "/" ]]; do
    path="$(dirname "${path}")"
  done
  printf '%s\n' "${path}"
}

sudo_prefix_for_path() {
  local path="$1"
  local writable_parent
  if [[ "$(id -u)" == "0" ]]; then
    return 0
  fi
  writable_parent="$(find_writable_parent "${path}")"
  if [[ -w "${writable_parent}" ]]; then
    return 0
  fi
  command -v sudo >/dev/null 2>&1 || die "sudo is required to write ${NGINX_DAEMON_DIR}"
  printf '%s\n' "sudo"
}

ensure_required_commands() {
  require_command awk
  require_command curl
  require_command gh
  require_command git
  require_command node
  require_command python3
  require_command tar
}

ensure_no_arguments() {
  if [[ "$#" -ne 0 ]]; then
    die "publish-multi-platform.sh accepts no arguments; edit ${CONFIG_PATH}"
  fi
}

read_published_version() {
  PUBLISHED_VERSION=""
  PUBLISHED_VERSION_SOURCE="none"

  local local_manifest="${NGINX_DAEMON_DIR}/releases/manifest.json"
  if [[ -f "${local_manifest}" ]]; then
    PUBLISHED_VERSION="$(parse_manifest_latest "${local_manifest}")"
    PUBLISHED_VERSION_SOURCE="${local_manifest}"
    return 0
  fi

  local tmp_manifest
  tmp_manifest="$(mktemp "${TMPDIR:-/tmp}/awiki-daemon-published-manifest.XXXXXX")"
  if curl -fsSL --max-time 8 "${DOWNLOAD_BASE_URL}/releases/manifest.json" -o "${tmp_manifest}" 2>/dev/null; then
    PUBLISHED_VERSION="$(parse_manifest_latest "${tmp_manifest}")"
    PUBLISHED_VERSION_SOURCE="${DOWNLOAD_BASE_URL}/releases/manifest.json"
  fi
  rm -f "${tmp_manifest}"
}

trigger_workflow() {
  local created_after
  created_after="$(python3 - <<'PY'
from datetime import datetime, timezone, timedelta

print((datetime.now(timezone.utc) - timedelta(seconds=10)).strftime("%Y-%m-%dT%H:%M:%SZ"))
PY
)"
  GH_TOKEN="${GITHUB_TOKEN_VALUE}" gh workflow run "${WORKFLOW_FILE}" \
    --repo "${GITHUB_REPO}" \
    --ref "${WORKFLOW_REF}" \
    --field "source_ref=${SOURCE_REF}" \
    --field "expected_version=${VERSION}"
  RUN_ID="$(find_triggered_run "${created_after}")"
  [[ -n "${RUN_ID}" ]] || die "failed to determine GitHub Actions run id"
}

find_triggered_run() {
  local created_after="$1"
  local attempt runs run_id
  for attempt in {1..30}; do
    runs="$(
      GH_TOKEN="${GITHUB_TOKEN_VALUE}" gh run list \
        --repo "${GITHUB_REPO}" \
        --workflow "${WORKFLOW_FILE}" \
        --event workflow_dispatch \
        --created ">=${created_after}" \
        --limit 20 \
        --json databaseId,createdAt,event,workflowName,status,displayTitle \
        2>/dev/null || true
    )"
    run_id="$(
      RUNS_JSON="${runs}" python3 - "${SOURCE_REF}" "${VERSION}" <<'PY'
import json
import os
import sys

source_ref = sys.argv[1]
version = sys.argv[2]
try:
    runs = json.loads(os.environ.get("RUNS_JSON", "[]"))
except Exception:
    runs = []

for run in runs:
    title = str(run.get("displayTitle") or "")
    # The workflow title includes the source ref and version so simultaneous
    # releases from different branches do not accidentally select each other.
    if source_ref in title and version in title:
        run_id = run.get("databaseId")
        if isinstance(run_id, int):
            print(run_id)
            break
PY
    )"
    if [[ -n "${run_id}" ]]; then
      printf '%s\n' "${run_id}"
      return 0
    fi
    sleep 2
  done
  return 1
}

wait_for_workflow() {
  GH_TOKEN="${GITHUB_TOKEN_VALUE}" gh run watch "${RUN_ID}" --repo "${GITHUB_REPO}" --exit-status
}

download_artifacts() {
  local output_dir="$1"
  mkdir -p "${output_dir}"
  GH_TOKEN="${GITHUB_TOKEN_VALUE}" gh run download "${RUN_ID}" --repo "${GITHUB_REPO}" --dir "${output_dir}"
}

collect_packages() {
  local artifact_dir="$1"
  local package_dir="$2"
  mkdir -p "${package_dir}"
  find "${artifact_dir}" -type f -name 'awiki-deamon-*.tar.gz' -exec cp {} "${package_dir}/" \;

  local required=(
    "awiki-deamon-linux-amd64.tar.gz"
    "awiki-deamon-darwin-arm64.tar.gz"
    "awiki-deamon-darwin-amd64.tar.gz"
  )
  local package
  for package in "${required[@]}"; do
    [[ -f "${package_dir}/${package}" ]] || die "GitHub artifacts missing ${package}"
  done

  local count
  count="$(find "${package_dir}" -maxdepth 1 -type f -name 'awiki-deamon-*.tar.gz' | wc -l | tr -d ' ')"
  [[ "${count}" == "3" ]] || die "expected exactly 3 daemon packages, found ${count}"
}

publish_to_nginx() {
  local sudo_cmd=()
  local sudo_word
  sudo_word="$(sudo_prefix_for_path "${NGINX_DAEMON_DIR}")"
  if [[ -n "${sudo_word}" ]]; then
    sudo_cmd=("${sudo_word}")
  fi

  local tmp_publish_dir="${NGINX_DAEMON_DIR}.tmp.${VERSION}.$$"
  "${sudo_cmd[@]}" rm -rf "${tmp_publish_dir}"
  "${sudo_cmd[@]}" mkdir -p "${tmp_publish_dir}/releases"
  "${sudo_cmd[@]}" cp "${STAGE_DIR}/install.sh" "${tmp_publish_dir}/install.sh"
  "${sudo_cmd[@]}" cp "${STAGE_DIR}/releases/manifest.json" "${tmp_publish_dir}/releases/manifest.json"
  "${sudo_cmd[@]}" cp -R "${STAGE_DIR}/releases/${VERSION}" "${tmp_publish_dir}/releases/${VERSION}"

  "${sudo_cmd[@]}" mkdir -p "${NGINX_DAEMON_DIR}/releases"
  "${sudo_cmd[@]}" rm -rf "${NGINX_DAEMON_DIR}/releases/${VERSION}"
  "${sudo_cmd[@]}" cp -R "${tmp_publish_dir}/releases/${VERSION}" "${NGINX_DAEMON_DIR}/releases/${VERSION}"

  "${sudo_cmd[@]}" cp "${tmp_publish_dir}/install.sh" "${NGINX_DAEMON_DIR}/install.sh.tmp"
  "${sudo_cmd[@]}" mv "${NGINX_DAEMON_DIR}/install.sh.tmp" "${NGINX_DAEMON_DIR}/install.sh"

  "${sudo_cmd[@]}" cp "${tmp_publish_dir}/releases/manifest.json" "${NGINX_DAEMON_DIR}/releases/manifest.json.tmp"
  "${sudo_cmd[@]}" mv "${NGINX_DAEMON_DIR}/releases/manifest.json.tmp" "${NGINX_DAEMON_DIR}/releases/manifest.json"

  "${sudo_cmd[@]}" rm -rf "${tmp_publish_dir}"
}

verify_published_http() {
  local tmp_manifest
  tmp_manifest="$(mktemp "${TMPDIR:-/tmp}/awiki-daemon-verify-manifest.XXXXXX")"
  curl -fsSL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/manifest.json" -o "${tmp_manifest}"
  local latest
  latest="$(parse_manifest_latest "${tmp_manifest}")"
  [[ "${latest}" == "${VERSION}" ]] || die "published manifest latest is ${latest}, expected ${VERSION}"
  rm -f "${tmp_manifest}"

  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/install.sh" >/dev/null
  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-linux-amd64.tar.gz" >/dev/null
  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-darwin-arm64.tar.gz" >/dev/null
  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-darwin-amd64.tar.gz" >/dev/null
}

cleanup_release_tmp() {
  [[ -n "${RELEASE_TMP_DIR:-}" ]] || return 0
  rm -rf "${RELEASE_TMP_DIR}"
}

ensure_no_arguments "$@"
ensure_required_commands

eval "$(read_config)"
VERSION="$(read_crate_version)"
VERSION="${VERSION#v}"
MIN_SUPPORTED_VERSION="${VERSION}"
BASE_URL="$(trim_trailing_slash "${base_url}")"
SOURCE_REF="${source_ref}"
GITHUB_TOKEN_VALUE="${github_token}"
DOWNLOAD_BASE_URL="${BASE_URL}/daemon"

validate_numeric_version "${VERSION}" "version"
validate_base_url "${BASE_URL}"
case "${GITHUB_TOKEN_VALUE}" in
  ghp_replace_with_token|github_token|changeme|CHANGE_ME)
    die "github_token still contains a template placeholder"
    ;;
esac

CRATE_VERSION="$(read_crate_version)"
CRATE_VERSION="${CRATE_VERSION#v}"
[[ "${CRATE_VERSION}" == "${VERSION}" ]] || die "Cargo.toml awiki-deamon version ${CRATE_VERSION} does not match daemon version ${VERSION}"

LOCK_VERSION="$(read_lock_version)"
LOCK_VERSION="${LOCK_VERSION#v}"
[[ "${LOCK_VERSION}" == "${VERSION}" ]] || die "Cargo.lock awiki-deamon version ${LOCK_VERSION} does not match daemon version ${VERSION}"

read_published_version
if [[ -n "${PUBLISHED_VERSION}" ]]; then
  validate_numeric_version "${PUBLISHED_VERSION}" "published daemon latest version"
  if ! version_gt "${VERSION}" "${PUBLISHED_VERSION}"; then
    die "daemon version ${VERSION} must be greater than published latest ${PUBLISHED_VERSION} (${PUBLISHED_VERSION_SOURCE})"
  fi
fi

RELEASE_TMP_DIR="${TMP_ROOT}/${VERSION}"
ARTIFACT_DIR="${RELEASE_TMP_DIR}/github-artifacts"
PACKAGE_DIR="${RELEASE_TMP_DIR}/packages"
STAGE_DIR="${RELEASE_TMP_DIR}/staged"
trap cleanup_release_tmp EXIT INT TERM
rm -rf "${RELEASE_TMP_DIR}"
mkdir -p "${RELEASE_TMP_DIR}"

cat <<EOF
daemon release plan
  version: ${VERSION}
  min_supported_version: ${MIN_SUPPORTED_VERSION}
  base_url: ${BASE_URL}
  download_base_url: ${DOWNLOAD_BASE_URL}
  source_ref: ${SOURCE_REF}
  github_repo: ${GITHUB_REPO}
  workflow: ${WORKFLOW_FILE}@${WORKFLOW_REF}
  nginx_dir: ${NGINX_DAEMON_DIR}
  published_latest: ${PUBLISHED_VERSION:-none}
  published_latest_source: ${PUBLISHED_VERSION_SOURCE}
EOF

trigger_workflow
echo "github_actions_run: ${RUN_ID}"
wait_for_workflow
download_artifacts "${ARTIFACT_DIR}"
collect_packages "${ARTIFACT_DIR}" "${PACKAGE_DIR}"

scripts/release/daemon/_stage-downloads.sh \
  --version "${VERSION}" \
  --min-supported "${MIN_SUPPORTED_VERSION}" \
  --source-dir "${PACKAGE_DIR}" \
  --output-dir "${STAGE_DIR}" \
  --base-url "${BASE_URL}"

publish_to_nginx
verify_published_http

cat <<EOF
daemon release published
  version: ${VERSION}
  install: ${DOWNLOAD_BASE_URL}/install.sh
  manifest: ${DOWNLOAD_BASE_URL}/releases/manifest.json
  linux_amd64: ${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-linux-amd64.tar.gz
  darwin_arm64: ${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-darwin-arm64.tar.gz
  darwin_amd64: ${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-darwin-amd64.tar.gz
EOF
