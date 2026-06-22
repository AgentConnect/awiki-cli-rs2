#!/usr/bin/env bash
set -euo pipefail

# Build the local awiki-deamon workspace and publish the current host package
# into a local Nginx static directory. This is intended for integration testing
# local changes. The --official mode can expose the locally built host package
# from the official /daemon URL without replacing the official global latest.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT_DIR="${ROOT_DIR}/scripts/release/daemon"
DEFAULT_NGINX_ROOT="/var/www/awiki-web"
DEFAULT_DOWNLOAD_PATH="/daemon-local"
DEFAULT_BASE_URL="https://awiki.info"

cd "${ROOT_DIR}"
export COPYFILE_DISABLE=1

usage() {
  cat <<'USAGE'
Build the local awiki-deamon workspace and publish it through local Nginx.

Usage:
  scripts/release/daemon/publish-local-nginx.sh [options]

Options:
  --version VERSION       Package version. Defaults to crates/awiki-deamon/Cargo.toml.
                          The version must match the compiled crate version because
                          the release archive runs awiki-deamon __self-check.
  --base-url URL          Service base URL embedded in install.sh. Defaults to
                          https://awiki.info.
  --download-path PATH    Nginx URL path for local daemon downloads. Defaults to
                          /daemon-local.
  --official              Publish through the official /daemon URL. This merges
                          the current host package into the existing official
                          manifest and preserves the existing latest version by
                          default.
  --promote-latest        With --official, set manifest latest/min_supported to
                          this local version. Use only when all required platform
                          packages for this version are available.
  --nginx-root DIR        Local Nginx web root. Defaults to /var/www/awiki-web.
  --target-dir DIR        Publish directory override. Defaults to
                          NGINX_ROOT/DOWNLOAD_PATH.
  --os OS                 Release OS name: linux or darwin. Defaults to current host.
  --arch ARCH             Release arch name: amd64 or arm64. Defaults to current host.
  --target TRIPLE         Rust target triple. Defaults from --os/--arch in
                          _build-artifact.sh.
  --work-dir DIR          Build/stage workspace. Defaults to dist/daemon-local-nginx.
  --skip-http-verify      Skip HTTP verification after copying files.
  --merge-existing-manifest
                          Merge staged package metadata into the existing target
                          manifest instead of replacing it.
  --allow-official-path   Allow publishing to /daemon. By default this script
                          refuses to overwrite the official release channel.
  --dry-run               Print the plan and the underlying build dry-run.
  -h, --help              Show this help.

Examples:
  scripts/release/daemon/publish-local-nginx.sh

  scripts/release/daemon/publish-local-nginx.sh \
    --base-url https://awiki.info \
    --download-path /daemon-dev

  scripts/release/daemon/publish-local-nginx.sh --official

Install from the local Nginx channel:
  curl -fsSL https://awiki.info/daemon-local/install.sh | sh -s -- \
    --token <token> --state-root /tmp/awiki-daemon-local --foreground
USAGE
}

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

host_os() {
  case "$(uname -s)" in
    Darwin) printf '%s\n' "darwin" ;;
    Linux) printf '%s\n' "linux" ;;
    *) uname -s | tr '[:upper:]' '[:lower:]' ;;
  esac
}

host_arch() {
  case "$(uname -m)" in
    x86_64|amd64) printf '%s\n' "amd64" ;;
    arm64|aarch64) printf '%s\n' "arm64" ;;
    *) uname -m ;;
  esac
}

normalize_download_path() {
  local value="$1"
  [[ -n "${value}" ]] || die "--download-path is required"
  [[ "${value}" == /* ]] || die "--download-path must start with /"
  while [[ "${value%/}" != "${value}" && "${value}" != "/" ]]; do
    value="${value%/}"
  done
  case "${value}" in
    "/"|*"/../"*|*"/.."|*"//"*|*"/./"*|*"/.")
      die "--download-path is unsafe: ${value}"
      ;;
  esac
  printf '%s\n' "${value}"
}

target_dir_from_root_and_path() {
  local root="$1"
  local path="$2"
  local relative="${path#/}"
  printf '%s/%s\n' "$(trim_trailing_slash "${root}")" "${relative}"
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
  command -v sudo >/dev/null 2>&1 || die "sudo is required to write ${path}"
  printf '%s\n' "sudo"
}

copy_to_nginx() {
  local sudo_cmd=()
  local sudo_word
  sudo_word="$(sudo_prefix_for_path "${TARGET_DIR}")"
  if [[ -n "${sudo_word}" ]]; then
    sudo_cmd=("${sudo_word}")
  fi

  "${sudo_cmd[@]}" mkdir -p "${TARGET_DIR}/releases"
  "${sudo_cmd[@]}" rm -rf "${TARGET_DIR}/releases/${VERSION}"
  "${sudo_cmd[@]}" cp -R "${STAGE_DIR}/releases/${VERSION}" "${TARGET_DIR}/releases/${VERSION}"

  "${sudo_cmd[@]}" cp "${STAGE_DIR}/install.sh" "${TARGET_DIR}/install.sh.tmp"
  "${sudo_cmd[@]}" mv "${TARGET_DIR}/install.sh.tmp" "${TARGET_DIR}/install.sh"

  "${sudo_cmd[@]}" cp "${STAGE_DIR}/releases/manifest.json" "${TARGET_DIR}/releases/manifest.json.tmp"
  "${sudo_cmd[@]}" mv "${TARGET_DIR}/releases/manifest.json.tmp" "${TARGET_DIR}/releases/manifest.json"
}

merge_existing_manifest() {
  local target_manifest="${TARGET_DIR}/releases/manifest.json"
  [[ -f "${target_manifest}" ]] || return 0

  python3 - "${target_manifest}" "${STAGE_DIR}/releases/manifest.json" "${VERSION}" "${PROMOTE_LATEST}" <<'PY'
import json
import pathlib
import sys

target_path = pathlib.Path(sys.argv[1])
staged_path = pathlib.Path(sys.argv[2])
version = sys.argv[3]
promote_latest = sys.argv[4] == "1"

existing = json.loads(target_path.read_text(encoding="utf-8"))
staged = json.loads(staged_path.read_text(encoding="utf-8"))

packages_by_key = {}
for package in existing.get("packages", []):
    key = (package.get("version"), package.get("os"), package.get("arch"))
    packages_by_key[key] = package
for package in staged.get("packages", []):
    key = (package.get("version"), package.get("os"), package.get("arch"))
    packages_by_key[key] = package

packages = sorted(
    packages_by_key.values(),
    key=lambda package: (
        str(package.get("version", "")),
        str(package.get("os", "")),
        str(package.get("arch", "")),
    ),
)

if promote_latest:
    latest = version
    min_supported = version
else:
    latest = existing.get("latest") or staged.get("latest") or version
    min_supported = existing.get("min_supported") or staged.get("min_supported") or latest

merged = {
    "latest": latest,
    "min_supported": min_supported,
    "packages": packages,
}
staged_path.write_text(json.dumps(merged, indent=2) + "\n", encoding="utf-8")
PY
}

verify_http() {
  local manifest_tmp
  manifest_tmp="$(mktemp "${TMPDIR:-/tmp}/awiki-daemon-local-manifest.XXXXXX")"
  curl -fsSL --max-time 10 "${DOWNLOAD_BASE_URL}/releases/manifest.json" -o "${manifest_tmp}"
  python3 - "${manifest_tmp}" "${VERSION}" "${OS_NAME}" "${ARCH_NAME}" "${EXPECT_LATEST_VERSION}" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
expected = sys.argv[2]
os_name = sys.argv[3]
arch_name = sys.argv[4]
expect_latest = sys.argv[5] == "1"
data = json.loads(path.read_text(encoding="utf-8"))
latest = data.get("latest")
if expect_latest and latest != expected:
    raise SystemExit(f"manifest latest {latest!r} != expected {expected!r}")
for package in data.get("packages", []):
    if (
        package.get("version") == expected
        and package.get("os") == os_name
        and package.get("arch") == arch_name
    ):
        break
else:
    raise SystemExit(f"manifest missing package {expected} {os_name}/{arch_name}")
PY
  rm -f "${manifest_tmp}"

  curl -fsSIL --max-time 10 "${DOWNLOAD_BASE_URL}/install.sh" >/dev/null
  curl -fsSIL --max-time 10 \
    "${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-${OS_NAME}-${ARCH_NAME}.tar.gz" \
    >/dev/null
}

VERSION=""
BASE_URL="${DEFAULT_BASE_URL}"
DOWNLOAD_PATH="${DEFAULT_DOWNLOAD_PATH}"
NGINX_ROOT="${DEFAULT_NGINX_ROOT}"
TARGET_DIR=""
OS_NAME=""
ARCH_NAME=""
TARGET_TRIPLE=""
WORK_DIR="${ROOT_DIR}/dist/daemon-local-nginx"
SKIP_HTTP_VERIFY=0
ALLOW_OFFICIAL_PATH=0
MERGE_EXISTING_MANIFEST=0
PROMOTE_LATEST=0
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      [[ -n "${VERSION}" ]] || die "--version requires a value"
      shift 2
      ;;
    --base-url)
      BASE_URL="${2:-}"
      [[ -n "${BASE_URL}" ]] || die "--base-url requires a value"
      shift 2
      ;;
    --download-path)
      DOWNLOAD_PATH="${2:-}"
      [[ -n "${DOWNLOAD_PATH}" ]] || die "--download-path requires a value"
      shift 2
      ;;
    --official)
      DOWNLOAD_PATH="/daemon"
      ALLOW_OFFICIAL_PATH=1
      MERGE_EXISTING_MANIFEST=1
      shift
      ;;
    --promote-latest)
      PROMOTE_LATEST=1
      shift
      ;;
    --nginx-root)
      NGINX_ROOT="${2:-}"
      [[ -n "${NGINX_ROOT}" ]] || die "--nginx-root requires a value"
      shift 2
      ;;
    --target-dir)
      TARGET_DIR="${2:-}"
      [[ -n "${TARGET_DIR}" ]] || die "--target-dir requires a value"
      shift 2
      ;;
    --os)
      OS_NAME="${2:-}"
      [[ -n "${OS_NAME}" ]] || die "--os requires a value"
      shift 2
      ;;
    --arch)
      ARCH_NAME="${2:-}"
      [[ -n "${ARCH_NAME}" ]] || die "--arch requires a value"
      shift 2
      ;;
    --target)
      TARGET_TRIPLE="${2:-}"
      [[ -n "${TARGET_TRIPLE}" ]] || die "--target requires a value"
      shift 2
      ;;
    --work-dir)
      WORK_DIR="${2:-}"
      [[ -n "${WORK_DIR}" ]] || die "--work-dir requires a value"
      shift 2
      ;;
    --skip-http-verify)
      SKIP_HTTP_VERIFY=1
      shift
      ;;
    --merge-existing-manifest)
      MERGE_EXISTING_MANIFEST=1
      shift
      ;;
    --allow-official-path)
      ALLOW_OFFICIAL_PATH=1
      shift
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

require_command awk
require_command curl
require_command node
require_command python3
require_command tar

VERSION="${VERSION:-$(read_crate_version)}"
VERSION="${VERSION#v}"
[[ -n "${VERSION}" ]] || die "version is required"
BASE_URL="$(trim_trailing_slash "${BASE_URL}")"
case "${BASE_URL}" in
  http://*|https://*) ;;
  *) die "--base-url must start with http:// or https://" ;;
esac

DOWNLOAD_PATH="$(normalize_download_path "${DOWNLOAD_PATH}")"
if [[ "${DOWNLOAD_PATH}" == "/daemon" && "${ALLOW_OFFICIAL_PATH}" != "1" ]]; then
  die "refusing to publish local build to /daemon; use --official or pass --allow-official-path"
fi
if [[ "${PROMOTE_LATEST}" == "1" && "${MERGE_EXISTING_MANIFEST}" != "1" ]]; then
  die "--promote-latest requires --official or --merge-existing-manifest"
fi

OS_NAME="${OS_NAME:-$(host_os)}"
ARCH_NAME="${ARCH_NAME:-$(host_arch)}"
case "${OS_NAME}" in
  linux|darwin) ;;
  *) die "unsupported daemon local release OS ${OS_NAME}" ;;
esac
case "${ARCH_NAME}" in
  amd64|arm64) ;;
  *) die "unsupported daemon local release arch ${ARCH_NAME}" ;;
esac

if [[ -z "${TARGET_DIR}" ]]; then
  TARGET_DIR="$(target_dir_from_root_and_path "${NGINX_ROOT}" "${DOWNLOAD_PATH}")"
fi
DOWNLOAD_BASE_URL="${BASE_URL}${DOWNLOAD_PATH}"
PACKAGE_DIR="${WORK_DIR}/packages"
STAGE_DIR="${WORK_DIR}/staged"
EXPECT_LATEST_VERSION=1
if [[ "${MERGE_EXISTING_MANIFEST}" == "1" && "${PROMOTE_LATEST}" != "1" ]]; then
  EXPECT_LATEST_VERSION=0
fi

build_args=(
  --version "${VERSION}"
  --os "${OS_NAME}"
  --arch "${ARCH_NAME}"
  --dist "${PACKAGE_DIR}"
)
if [[ -n "${TARGET_TRIPLE}" ]]; then
  build_args+=(--target "${TARGET_TRIPLE}")
fi

cat <<EOF
daemon local nginx publish plan
  version: ${VERSION}
  os_arch: ${OS_NAME}/${ARCH_NAME}
  base_url: ${BASE_URL}
  download_path: ${DOWNLOAD_PATH}
  download_base_url: ${DOWNLOAD_BASE_URL}
  target_dir: ${TARGET_DIR}
  work_dir: ${WORK_DIR}
  merge_existing_manifest: $([[ "${MERGE_EXISTING_MANIFEST}" == "1" ]] && printf 'yes' || printf 'no')
  promote_latest: $([[ "${PROMOTE_LATEST}" == "1" ]] && printf 'yes' || printf 'no')
  http_verify: $([[ "${SKIP_HTTP_VERIFY}" == "1" ]] && printf 'skip' || printf 'enabled')
EOF

if [[ "${DRY_RUN}" == "1" ]]; then
  "${SCRIPT_DIR}/_build-artifact.sh" "${build_args[@]}" --dry-run
  cat <<EOF
Would stage downloads with --allow-partial into ${STAGE_DIR}
Would merge existing manifest: $([[ "${MERGE_EXISTING_MANIFEST}" == "1" ]] && printf 'yes' || printf 'no')
Would publish staged files into ${TARGET_DIR}
EOF
  exit 0
fi

rm -rf "${PACKAGE_DIR}" "${STAGE_DIR}"
mkdir -p "${PACKAGE_DIR}" "${STAGE_DIR}"

"${SCRIPT_DIR}/_build-artifact.sh" "${build_args[@]}"

"${SCRIPT_DIR}/_stage-downloads.sh" \
  --version "${VERSION}" \
  --min-supported "${VERSION}" \
  --source-dir "${PACKAGE_DIR}" \
  --output-dir "${STAGE_DIR}" \
  --base-url "${BASE_URL}" \
  --download-base-url "${DOWNLOAD_BASE_URL}" \
  --allow-partial

if [[ "${MERGE_EXISTING_MANIFEST}" == "1" ]]; then
  merge_existing_manifest
fi

copy_to_nginx

if [[ "${SKIP_HTTP_VERIFY}" != "1" ]]; then
  verify_http
fi

cat <<EOF
daemon local nginx publish complete
  install: ${DOWNLOAD_BASE_URL}/install.sh
  manifest: ${DOWNLOAD_BASE_URL}/releases/manifest.json
  package: ${DOWNLOAD_BASE_URL}/releases/${VERSION}/awiki-deamon-${OS_NAME}-${ARCH_NAME}.tar.gz

install test command:
  curl -fsSL ${DOWNLOAD_BASE_URL}/install.sh | sh -s -- --token <token>$([[ "${EXPECT_LATEST_VERSION}" == "1" ]] || printf ' --version %s' "${VERSION}") --state-root /tmp/awiki-daemon-local --foreground
EOF
