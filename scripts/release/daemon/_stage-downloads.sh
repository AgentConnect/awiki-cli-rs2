#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
CALLER_DIR="$(pwd)"
cd "${ROOT_DIR}"

usage() {
  cat <<'USAGE'
Stage daemon installer files into the official download-service layout.

Usage:
  scripts/release/daemon/_stage-downloads.sh --version VERSION (--base-url URL | --download-base-url URL) [--download-mirror-url URL ...] [--min-supported VERSION] [--source-dir DIR] [--output-dir DIR] [--allow-partial]

Options:
  --version VERSION   Daemon release version, with or without a leading v.
  --base-url URL      Awiki backend service base URL embedded in install.sh. When
                      --download-base-url is omitted, it defaults to URL/daemon.
  --download-base-url URL
                      Daemon static download root embedded in install.sh, e.g.
                      https://example.com/daemon. When --base-url is omitted, URL
                      must end with /daemon so the backend service base can be
                      inferred safely.
  --download-mirror-url URL
                      Additional daemon static download root embedded in
                      install.sh. May be repeated. Mirrors must expose the same
                      /install.sh, /releases/manifest.json, and release package
                      layout as --download-base-url.
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

trim_trailing_slash() {
  local value="$1"
  while [[ "${value%/}" != "${value}" ]]; do
    value="${value%/}"
  done
  printf '%s\n' "${value}"
}

infer_base_url_from_download_base_url() {
  local download_base
  download_base="$(trim_trailing_slash "$1")"
  case "${download_base}" in
    http://*/daemon|https://*/daemon)
      printf '%s\n' "${download_base%/daemon}"
      ;;
    *)
      return 1
      ;;
  esac
}

infer_download_base_url_from_base_url() {
  local base
  base="$(trim_trailing_slash "$1")"
  case "${base}" in
    http://*|https://*)
      printf '%s\n' "${base}/daemon"
      ;;
    *)
      return 1
      ;;
  esac
}

validate_base_url() {
  case "$1" in
    http://*|https://*) ;;
    *) die "--base-url must start with http:// or https://" ;;
  esac
}

validate_download_base_url() {
  case "$1" in
    http://*|https://*|file://*|/*|./*|../*) ;;
    *) die "--download-base-url must be a URL or local path" ;;
  esac
}

VERSION=""
MIN_SUPPORTED=""
SOURCE_DIR="${ROOT_DIR}/dist"
OUTPUT_DIR="${ROOT_DIR}/dist/daemon"
BASE_URL=""
DOWNLOAD_BASE_URL=""
DOWNLOAD_MIRROR_URLS=()
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
    --download-mirror-url)
      mirror_url="${2:-}"
      [[ -n "${mirror_url}" ]] || die "--download-mirror-url requires a value"
      DOWNLOAD_MIRROR_URLS+=("${mirror_url}")
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
[[ -n "${BASE_URL}" || -n "${DOWNLOAD_BASE_URL}" ]] || die "--base-url or --download-base-url is required"
if [[ -n "${BASE_URL}" ]]; then
  BASE_URL="$(trim_trailing_slash "${BASE_URL}")"
fi
if [[ -n "${DOWNLOAD_BASE_URL}" ]]; then
  DOWNLOAD_BASE_URL="$(trim_trailing_slash "${DOWNLOAD_BASE_URL}")"
fi
if [[ -z "${BASE_URL}" ]]; then
  BASE_URL="$(infer_base_url_from_download_base_url "${DOWNLOAD_BASE_URL}")" || die "--base-url is required unless --download-base-url ends with /daemon"
fi
if [[ -z "${DOWNLOAD_BASE_URL}" ]]; then
  DOWNLOAD_BASE_URL="$(infer_download_base_url_from_base_url "${BASE_URL}")" || die "--download-base-url is required unless --base-url is http:// or https://"
fi
validate_base_url "${BASE_URL}"
validate_download_base_url "${DOWNLOAD_BASE_URL}"
normalized_mirror_urls=()
seen_download_urls=$'\n'
seen_download_urls+="${DOWNLOAD_BASE_URL}"$'\n'
if ((${#DOWNLOAD_MIRROR_URLS[@]})); then
  for mirror_url in "${DOWNLOAD_MIRROR_URLS[@]}"; do
    mirror_url="$(trim_trailing_slash "${mirror_url}")"
    validate_download_base_url "${mirror_url}"
    case "${seen_download_urls}" in
      *$'\n'"${mirror_url}"$'\n'*) ;;
      *)
        normalized_mirror_urls+=("${mirror_url}")
        seen_download_urls+="${mirror_url}"$'\n'
        ;;
    esac
  done
fi
DOWNLOAD_MIRROR_URLS=()
if ((${#normalized_mirror_urls[@]})); then
  for mirror_url in "${normalized_mirror_urls[@]}"; do
    DOWNLOAD_MIRROR_URLS+=("${mirror_url}")
  done
fi
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
download_base_urls_file="${OUTPUT_DIR}/.download-base-urls.tmp"
{
  printf '%s\n' "${DOWNLOAD_BASE_URL}"
  if ((${#DOWNLOAD_MIRROR_URLS[@]})); then
    for mirror_url in "${DOWNLOAD_MIRROR_URLS[@]}"; do
      printf '%s\n' "${mirror_url}"
    done
  fi
} > "${download_base_urls_file}"

python3 - "${ROOT_DIR}/scripts/release/daemon/_install.sh.template" "${OUTPUT_DIR}/install.sh" "${BASE_URL}" "${download_base_urls_file}" <<'PY'
import pathlib
import sys

template_path, output_path, base_url, download_base_urls_path = sys.argv[1:5]
text = pathlib.Path(template_path).read_text(encoding="utf-8")
download_base_urls = pathlib.Path(download_base_urls_path).read_text(encoding="utf-8").strip()
text = text.replace("__AWIKI_DAEMON_BASE_URL__", base_url)
text = text.replace("__AWIKI_DAEMON_DOWNLOAD_BASE_URLS__", download_base_urls)
pathlib.Path(output_path).write_text(text, encoding="utf-8")
PY
chmod 0755 "${OUTPUT_DIR}/install.sh"

manifest_args=(
  node "${ROOT_DIR}/scripts/release/daemon/_generate-manifest.js"
  --version "${VERSION}"
  --dist "${version_dir}"
  --output "${release_dir}/manifest.json"
  --download-base-urls "${download_base_urls_file}"
)
if [[ -n "${MIN_SUPPORTED}" ]]; then
  manifest_args+=(--min-supported "${MIN_SUPPORTED}")
fi
if [[ "${ALLOW_PARTIAL}" == "1" ]]; then
  manifest_args+=(--allow-partial)
fi
"${manifest_args[@]}"
rm -f "${download_base_urls_file}"

echo "daemon download layout staged: ${OUTPUT_DIR}"
