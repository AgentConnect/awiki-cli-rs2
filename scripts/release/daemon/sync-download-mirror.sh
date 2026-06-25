#!/usr/bin/env bash
set -euo pipefail

# Synchronize a daemon static download mirror by pulling from the authoritative
# release source over HTTP(S). This script accepts no command-line arguments.
# Copy sync-download-mirror.toml.template to sync-download-mirror.toml on each
# mirror host and run this script there.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_PATH="${SCRIPT_DIR}/sync-download-mirror.toml"
TMP_ROOT="/tmp/awiki-daemon-mirror-sync"

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

validate_source_base_url() {
  case "$1" in
    http://*|https://*) ;;
    *) die "source_base_url must start with http:// or https://" ;;
  esac
}

validate_keep_versions() {
  case "$1" in
    ""|*[!0123456789]*) die "keep_versions must be a non-negative integer" ;;
  esac
}

sudo_prefix_for_path() {
  local path="$1"
  local existing="$path"
  while [[ ! -e "${existing}" && "${existing}" != "/" ]]; do
    existing="$(dirname "${existing}")"
  done
  if [[ "$(id -u)" == "0" || -w "${existing}" ]]; then
    return 0
  fi
  command -v sudo >/dev/null 2>&1 || die "sudo is required to write ${path}"
  printf '%s\n' "sudo"
}

run_as_target_writer() {
  if [[ -n "${SUDO_WORD:-}" ]]; then
    "${SUDO_WORD}" "$@"
  else
    "$@"
  fi
}

read_config() {
  [[ -f "${CONFIG_PATH}" ]] || die "missing config: ${CONFIG_PATH}. Copy sync-download-mirror.toml.template and fill it in."
  python3 - "${CONFIG_PATH}" <<'PY'
import pathlib
import re
import shlex
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
    data[key] = bytes(raw_value, "utf-8").decode("unicode_escape").strip()

required = ["source_base_url", "target_dir"]
allowed = set(required + ["keep_versions"])
for key in required:
    if not data.get(key):
        raise SystemExit(f"config field {key!r} is required")
for key in data:
    if key not in allowed:
        raise SystemExit(f"unsupported config field {key!r}")
if not data.get("keep_versions"):
    data["keep_versions"] = "3"
for key in ["source_base_url", "target_dir", "keep_versions"]:
    print(f"{key}={shlex.quote(data[key])}")
PY
}

join_url() {
  local base
  local path
  base="$(trim_trailing_slash "$1")"
  path="$2"
  while [[ "${path#/}" != "${path}" ]]; do
    path="${path#/}"
  done
  printf '%s/%s\n' "$base" "$path"
}

download_to() {
  local url="$1"
  local dest="$2"
  curl -fL --show-error --connect-timeout 15 --max-time 900 \
    --retry 3 --retry-delay 2 \
    "$url" -o "$dest"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

parse_manifest() {
  python3 - "$1" "$2" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
output_path = pathlib.Path(sys.argv[2])
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
latest = str(manifest.get("latest") or "").strip()
min_supported = str(manifest.get("min_supported") or "").strip()
if not latest:
    raise SystemExit("manifest latest is required")
rows = []
versions = {latest}
if min_supported:
    versions.add(min_supported)
for package in manifest.get("packages", []):
    version = str(package.get("version") or "").strip()
    path = str(package.get("path") or "").strip()
    sha256 = str(package.get("sha256") or "").strip().lower()
    if not version or not path or not sha256:
        raise SystemExit("manifest package must contain version, path, and sha256")
    if (
        path.startswith("/")
        or path.startswith("./")
        or path.startswith("../")
        or "/../" in path
        or path.endswith("/..")
        or "\\" in path
    ):
        raise SystemExit(f"unsafe package path: {path}")
    versions.add(version)
    rows.append((version, path, sha256))
with output_path.open("w", encoding="utf-8") as output:
    for version, path, sha256 in rows:
        output.write(f"package\t{version}\t{path}\t{sha256}\n")
    for version in sorted(versions):
        output.write(f"keep\t{version}\n")
PY
}

cleanup_old_versions() {
  local releases_dir="$1"
  local keep_versions="$2"
  local keep_file="$3"
  [[ -d "${releases_dir}" ]] || return 0
  python3 - "${releases_dir}" "${keep_versions}" "${keep_file}" <<'PY' | while IFS= read -r remove_path; do
import pathlib
import re
import sys

releases_dir = pathlib.Path(sys.argv[1])
keep_versions = int(sys.argv[2])
keep = set()
keep_file = pathlib.Path(sys.argv[3])
if keep_file.exists():
    keep = {line.strip() for line in keep_file.read_text(encoding="utf-8").splitlines() if line.strip()}

version_re = re.compile(r"^[0-9][0-9A-Za-z._-]*$")
versions = []
for child in releases_dir.iterdir():
    if not child.is_dir() or child.name in keep or not version_re.fullmatch(child.name):
        continue
    versions.append(child)

def key(path):
    parts = []
    for part in re.split(r"[._-]", path.name):
        if part.isdigit():
            parts.append((0, int(part)))
        else:
            parts.append((1, part))
    return parts

versions.sort(key=key, reverse=True)
for child in versions[keep_versions:]:
    print(child)
PY
    [[ -n "${remove_path}" ]] || continue
    run_as_target_writer rm -rf "${remove_path}"
  done
}

if [[ "$#" -ne 0 ]]; then
  die "sync-download-mirror.sh accepts no arguments; edit ${CONFIG_PATH}"
fi

require_command curl
require_command python3
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
  die "sha256sum or shasum is required"
fi

eval "$(read_config)"
SOURCE_BASE_URL="$(trim_trailing_slash "${source_base_url}")"
TARGET_DIR="${target_dir}"
KEEP_VERSIONS="${keep_versions}"
validate_source_base_url "${SOURCE_BASE_URL}"
validate_keep_versions "${KEEP_VERSIONS}"

SUDO_WORD="$(sudo_prefix_for_path "${TARGET_DIR}")"

SYNC_TMP_DIR="${TMP_ROOT}/sync.$$"
rm -rf "${SYNC_TMP_DIR}"
mkdir -p "${SYNC_TMP_DIR}/stage/releases"
cleanup() {
  rm -rf "${SYNC_TMP_DIR}"
}
trap cleanup EXIT INT TERM

manifest_stage="${SYNC_TMP_DIR}/stage/releases/manifest.json"
install_stage="${SYNC_TMP_DIR}/stage/install.sh"
download_to "$(join_url "${SOURCE_BASE_URL}" "releases/manifest.json")" "${manifest_stage}"
download_to "$(join_url "${SOURCE_BASE_URL}" "install.sh")" "${install_stage}"
chmod 0755 "${install_stage}"

manifest_rows="${SYNC_TMP_DIR}/manifest-rows.tsv"
parse_manifest "${manifest_stage}" "${manifest_rows}"
keep_file="${SYNC_TMP_DIR}/keep-versions.txt"
awk -F '\t' '$1 == "keep" { print $2 }' "${manifest_rows}" > "${keep_file}"

while IFS=$'\t' read -r kind version package_path expected_sha; do
  [[ "${kind}" == "package" ]] || continue
  package_dest="${SYNC_TMP_DIR}/stage/${package_path}"
  mkdir -p "$(dirname "${package_dest}")"
  download_to "$(join_url "${SOURCE_BASE_URL}" "${package_path}")" "${package_dest}"
  actual_sha="$(sha256_file "${package_dest}")"
  [[ "${actual_sha}" == "${expected_sha}" ]] || die "sha256 mismatch for ${package_path}"
  printf '%s  %s\n' "${expected_sha}" "$(basename "${package_path}")" >> "${SYNC_TMP_DIR}/stage/releases/${version}/checksums.txt"
done < "${manifest_rows}"

publish_tmp="${TARGET_DIR}.sync.$$"
run_as_target_writer rm -rf "${publish_tmp}"
run_as_target_writer mkdir -p "${publish_tmp}"
run_as_target_writer cp -R "${SYNC_TMP_DIR}/stage/." "${publish_tmp}/"
run_as_target_writer mkdir -p "${TARGET_DIR}/releases"

while IFS=$'\t' read -r kind version _package_path _expected_sha; do
  [[ "${kind}" == "package" ]] || continue
  [[ -d "${publish_tmp}/releases/${version}" ]] || continue
  run_as_target_writer rm -rf "${TARGET_DIR}/releases/${version}"
  run_as_target_writer cp -R "${publish_tmp}/releases/${version}" "${TARGET_DIR}/releases/${version}"
done < "${manifest_rows}"

run_as_target_writer cp "${publish_tmp}/install.sh" "${TARGET_DIR}/install.sh.tmp"
run_as_target_writer mv "${TARGET_DIR}/install.sh.tmp" "${TARGET_DIR}/install.sh"
run_as_target_writer cp "${publish_tmp}/releases/manifest.json" "${TARGET_DIR}/releases/manifest.json.tmp"
run_as_target_writer mv "${TARGET_DIR}/releases/manifest.json.tmp" "${TARGET_DIR}/releases/manifest.json"
run_as_target_writer rm -rf "${publish_tmp}"

cleanup_old_versions "${TARGET_DIR}/releases" "${KEEP_VERSIONS}" "${keep_file}"

cat <<EOF
daemon mirror synchronized
  source_base_url: ${SOURCE_BASE_URL}
  target_dir: ${TARGET_DIR}
  keep_versions: ${KEEP_VERSIONS}
EOF
