#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CALLER_DIR="$(pwd)"
cd "${ROOT_DIR}"

usage() {
  cat <<'USAGE'
Stage daemon installer files into the official download-service layout.

Usage:
  scripts/release/stage-daemon-downloads.sh --version VERSION --base-url URL --download-base-url URL [--min-supported VERSION] [--source-dir DIR] [--output-dir DIR] [--allow-partial]

Options:
  --version VERSION   Daemon release version, with or without a leading v.
  --base-url URL      Awiki backend service base URL embedded in install.sh.
  --download-base-url URL
                      Daemon static download root embedded in install.sh, e.g. https://awiki.ai/daemon.
  --min-supported VERSION
                      Minimum supported daemon version written to manifest. Defaults to --version.
  --source-dir DIR    Directory containing awiki-deamon-*.tar.gz packages. Defaults to dist.
  --output-dir DIR    Download-service root for /daemon. Defaults to dist/daemon.
  --allow-partial     Generate manifest from packages that exist in --source-dir instead of requiring all supported OS/arch packages.
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
}

validate_version_segment() {
  [[ -n "$1" ]] || die "version is required"
  case "$1" in
    .*|*..*|*[!0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz._-]*)
      die "version contains unsupported characters"
      ;;
  esac
}

checksum_packages() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum awiki-deamon-*.tar.gz > checksums.txt
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 awiki-deamon-*.tar.gz > checksums.txt
  else
    die "sha256sum or shasum is required"
  fi
}

resolve_path_arg() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s\n' "${CALLER_DIR}/$1" ;;
  esac
}

VERSION=""
MIN_SUPPORTED=""
SOURCE_DIR="${ROOT_DIR}/dist"
OUTPUT_DIR="${ROOT_DIR}/dist/daemon"
BASE_URL=""
DOWNLOAD_BASE_URL=""
ALLOW_PARTIAL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      [[ -n "${VERSION}" ]] || die "--version requires a value"
      shift 2
      ;;
    --source-dir)
      SOURCE_DIR="${2:-}"
      [[ -n "${SOURCE_DIR}" ]] || die "--source-dir requires a value"
      shift 2
      ;;
    --output-dir)
      OUTPUT_DIR="${2:-}"
      [[ -n "${OUTPUT_DIR}" ]] || die "--output-dir requires a value"
      shift 2
      ;;
    --base-url)
      BASE_URL="${2:-}"
      [[ -n "${BASE_URL}" ]] || die "--base-url requires a value"
      shift 2
      ;;
    --download-base-url)
      DOWNLOAD_BASE_URL="${2:-}"
      [[ -n "${DOWNLOAD_BASE_URL}" ]] || die "--download-base-url requires a value"
      shift 2
      ;;
    --min-supported)
      MIN_SUPPORTED="${2:-}"
      [[ -n "${MIN_SUPPORTED}" ]] || die "--min-supported requires a value"
      shift 2
      ;;
    --allow-partial)
      ALLOW_PARTIAL=1
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

VERSION="${VERSION#v}"
[[ -n "${VERSION}" ]] || die "--version is required"
validate_version_segment "${VERSION}"
MIN_SUPPORTED="${MIN_SUPPORTED#v}"
if [[ -n "${MIN_SUPPORTED}" ]]; then
  validate_version_segment "${MIN_SUPPORTED}"
fi
[[ -n "${BASE_URL}" ]] || die "--base-url is required"
[[ -n "${DOWNLOAD_BASE_URL}" ]] || die "--download-base-url is required"
BASE_URL="${BASE_URL%/}"
DOWNLOAD_BASE_URL="${DOWNLOAD_BASE_URL%/}"
SOURCE_DIR="$(resolve_path_arg "${SOURCE_DIR}")"
OUTPUT_DIR="$(resolve_path_arg "${OUTPUT_DIR}")"
[[ -d "${SOURCE_DIR}" ]] || die "source directory does not exist: ${SOURCE_DIR}"

SOURCE_DIR="$(cd "${SOURCE_DIR}" && pwd)"
mkdir -p "${OUTPUT_DIR}"
OUTPUT_DIR="$(cd "${OUTPUT_DIR}" && pwd)"

packages=("${SOURCE_DIR}"/awiki-deamon-*.tar.gz)
[[ -e "${packages[0]}" ]] || die "no daemon packages found in ${SOURCE_DIR}"

release_dir="${OUTPUT_DIR}/releases"
version_dir="${release_dir}/${VERSION}"
rm -rf "${version_dir}"
mkdir -p "${version_dir}"

for package in "${packages[@]}"; do
  cp "${package}" "${version_dir}/"
done

(cd "${version_dir}" && checksum_packages)
python3 - "${ROOT_DIR}/scripts/daemon/install.sh" "${OUTPUT_DIR}/install.sh" "${BASE_URL}" "${DOWNLOAD_BASE_URL}" <<'PY'
import pathlib
import sys

template_path, output_path, base_url, download_base_url = sys.argv[1:5]
text = pathlib.Path(template_path).read_text(encoding="utf-8")
text = text.replace("__AWIKI_DAEMON_BASE_URL__", base_url)
text = text.replace("__AWIKI_DAEMON_DOWNLOAD_BASE_URL__", download_base_url)
pathlib.Path(output_path).write_text(text, encoding="utf-8")
PY
chmod 0755 "${OUTPUT_DIR}/install.sh"

manifest_args=(
  node "${ROOT_DIR}/scripts/release/generate-daemon-manifest.js"
  --version "${VERSION}"
  --dist "${version_dir}"
  --output "${release_dir}/manifest.json"
  --base-url "${DOWNLOAD_BASE_URL}/releases"
)
if [[ -n "${MIN_SUPPORTED}" ]]; then
  manifest_args+=(--min-supported "${MIN_SUPPORTED}")
fi
if [[ "${ALLOW_PARTIAL}" == "1" ]]; then
  manifest_args+=(--allow-partial)
fi
"${manifest_args[@]}"

echo "daemon download layout staged: ${OUTPUT_DIR}"
