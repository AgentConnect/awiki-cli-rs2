#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${ROOT_DIR}"
export COPYFILE_DISABLE=1
if [[ -n "${HOME:-}" && -d "${HOME}/.cargo/bin" ]]; then
  export PATH="${HOME}/.cargo/bin:${PATH}"
fi

NGINX_DAEMON_DIR="${AWIKI_DAEMON_NGINX_DIR:-/var/www/awiki-web/daemon}"
OS_NAME="linux"
ARCH_NAME="amd64"
TARGET_TRIPLE="x86_64-unknown-linux-gnu"

usage() {
  cat <<'USAGE'
Publish the Linux awiki-deamon package to the Nginx daemon download root.

Usage:
  scripts/release/daemon/publish-linux.sh --base-url URL [--dry-run]

Options:
  --base-url URL   Backend service base URL, e.g. https://example.com.
                   The daemon download root is derived as URL/daemon.
  --dry-run        Validate inputs and print the release plan without building
                   or writing the Nginx daemon directory. This mode may run on
                   non-Linux hosts.

Environment:
  AWIKI_DAEMON_NGINX_DIR
                   Nginx filesystem directory serving URL path /daemon.
                   Defaults to /var/www/awiki-web/daemon.
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
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
    *) die "--base-url must start with http:// or https://" ;;
  esac
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

ensure_required_commands() {
  require_command awk
  require_command curl
  require_command python3

  if [[ "${DRY_RUN}" != "1" ]]; then
    require_command cargo
    require_command node
    require_command tar
  fi
}

validate_numeric_version() {
  local value="$1"
  case "${value}" in
    ""|.*|*..*|*.|*[!0123456789.]*)
      die "$2 must be a numeric dotted version, got: ${value}"
      ;;
  esac
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

verify_manifest_package() {
  python3 - "$1" "$2" "$3" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
version = sys.argv[2]
download_base_url = sys.argv[3].rstrip("/")
expected_url = (
    f"{download_base_url}/releases/{version}/awiki-deamon-linux-amd64.tar.gz"
)

try:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
except Exception as exc:
    raise SystemExit(f"invalid daemon manifest {manifest_path}: {exc}") from exc

if manifest.get("latest") != version:
    raise SystemExit(
        f"manifest latest is {manifest.get('latest')!r}, expected {version!r}"
    )

packages = manifest.get("packages")
if not isinstance(packages, list):
    raise SystemExit("manifest packages must be a list")

for package in packages:
    if not isinstance(package, dict):
        continue
    if package.get("os") == "linux" and package.get("arch") == "amd64":
        if package.get("version") != version:
            raise SystemExit(
                f"linux/amd64 package version is {package.get('version')!r}, expected {version!r}"
            )
        if package.get("url") != expected_url:
            raise SystemExit(
                f"linux/amd64 package url is {package.get('url')!r}, expected {expected_url!r}"
            )
        if not isinstance(package.get("sha256"), str) or len(package["sha256"]) != 64:
            raise SystemExit("linux/amd64 package sha256 is missing or invalid")
        break
else:
    raise SystemExit("manifest is missing linux/amd64 package")
PY
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

ensure_linux_amd64_host() {
  case "$(uname -s)" in
    Linux) ;;
    *) die "daemon Linux release publishing must run on a Linux host" ;;
  esac

  case "$(uname -m)" in
    x86_64|amd64) ;;
    *) die "daemon Linux release publishing currently supports linux/amd64 only" ;;
  esac
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
  verify_manifest_package "${tmp_manifest}" "${VERSION}" "${DOWNLOAD_BASE_URL}"
  rm -f "${tmp_manifest}"

  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-linux-amd64.tar.gz" >/dev/null
  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/install.sh" >/dev/null
}

BASE_URL=""
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url)
      BASE_URL="${2:-}"
      [[ -n "${BASE_URL}" ]] || die "--base-url requires a value"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${BASE_URL}" ]] || die "--base-url is required"
BASE_URL="$(trim_trailing_slash "${BASE_URL}")"
validate_base_url "${BASE_URL}"
DOWNLOAD_BASE_URL="${BASE_URL}/daemon"
ensure_required_commands

VERSION="$(read_crate_version)"
VERSION="${VERSION#v}"
[[ -n "${VERSION}" ]] || die "failed to read crates/awiki-deamon/Cargo.toml version"
validate_numeric_version "${VERSION}" "crates/awiki-deamon/Cargo.toml version"

LOCK_VERSION="$(read_lock_version)"
LOCK_VERSION="${LOCK_VERSION#v}"
[[ -n "${LOCK_VERSION}" ]] || die "failed to read awiki-deamon version from Cargo.lock"
validate_numeric_version "${LOCK_VERSION}" "Cargo.lock awiki-deamon version"
if [[ "${LOCK_VERSION}" != "${VERSION}" ]]; then
  die "Cargo.lock awiki-deamon version ${LOCK_VERSION} does not match Cargo.toml version ${VERSION}"
fi

if [[ "${DRY_RUN}" != "1" ]]; then
  ensure_linux_amd64_host
fi

read_published_version
if [[ -n "${PUBLISHED_VERSION}" ]]; then
  validate_numeric_version "${PUBLISHED_VERSION}" "published daemon latest version"
  if ! version_gt "${VERSION}" "${PUBLISHED_VERSION}"; then
    die "crate version ${VERSION} must be greater than published latest ${PUBLISHED_VERSION} (${PUBLISHED_VERSION_SOURCE})"
  fi
fi

BUILD_DIR="${ROOT_DIR}/dist/daemon-build-${VERSION}"
STAGE_DIR="${ROOT_DIR}/dist/daemon-downloads-${VERSION}"

cat <<EOF
daemon release plan
  version: ${VERSION}
  base_url: ${BASE_URL}
  download_base_url: ${DOWNLOAD_BASE_URL}
  nginx_dir: ${NGINX_DAEMON_DIR}
  target: ${OS_NAME}/${ARCH_NAME} (${TARGET_TRIPLE})
  published_latest: ${PUBLISHED_VERSION:-none}
  published_latest_source: ${PUBLISHED_VERSION_SOURCE}
EOF

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "dry run: no build, staging, or nginx publish performed"
  exit 0
fi

rm -rf "${BUILD_DIR}" "${STAGE_DIR}"

scripts/release/daemon/_build-artifact.sh \
  --version "${VERSION}" \
  --os "${OS_NAME}" \
  --arch "${ARCH_NAME}" \
  --target "${TARGET_TRIPLE}" \
  --dist "${BUILD_DIR}"

scripts/release/daemon/_stage-downloads.sh \
  --version "${VERSION}" \
  --source-dir "${BUILD_DIR}" \
  --output-dir "${STAGE_DIR}" \
  --base-url "${BASE_URL}" \
  --allow-partial

[[ -f "${STAGE_DIR}/install.sh" ]] || die "staged install.sh is missing"
[[ -f "${STAGE_DIR}/releases/manifest.json" ]] || die "staged manifest.json is missing"
[[ -f "${STAGE_DIR}/releases/${VERSION}/awiki-deamon-linux-amd64.tar.gz" ]] || die "staged linux/amd64 package is missing"

publish_to_nginx
verify_published_http

cat <<EOF
daemon release published
  version: ${VERSION}
  install: ${DOWNLOAD_BASE_URL}/install.sh
  manifest: ${DOWNLOAD_BASE_URL}/releases/manifest.json
  package: ${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-linux-amd64.tar.gz
EOF
