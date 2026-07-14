#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

usage() {
  cat <<'USAGE'
Build a Rust awiki-cli release archive with the Go release artifact name contract.

Usage:
  scripts/release/build-release-artifact.sh [--version VERSION] [--os OS] [--arch ARCH] [--target TRIPLE] [--dist DIR] [--dry-run]

Options:
  --version VERSION   Package version. Defaults to package.json.version.
  --os OS            Release OS name: linux, darwin, or windows. Defaults to the current host.
  --arch ARCH        Release arch name: amd64 or arm64. Defaults to the current host.
  --target TRIPLE    Rust target triple. Defaults from --os/--arch.
  --dist DIR         Output directory. Defaults to dist.
  --dry-run          Print the build and archive plan without building.
  -h, --help         Show this help.

Environment:
  CARGO                         Cargo binary (default: cargo)
  AWIKI_CLI_RUST_TOOLCHAIN      Cargo toolchain without leading + (default: 1.88.0)
  AWIKI_CLI_SKIP_E2EE_RELEASE_FEATURE_CHECK
                                Set to 1 to skip the local feature graph check.
  AWIKI_CLI_BUILD_DATE          Build date override for buildinfo.
  AWIKI_CLI_COMMIT              Commit override for buildinfo.
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
}

read_package_version() {
  node - package.json <<'NODE'
const fs = require('fs');
const pkg = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
process.stdout.write(typeof pkg.version === 'string' ? pkg.version.trim() : '');
NODE
}

host_os() {
  case "$(uname -s)" in
    Darwin) printf '%s\n' "darwin" ;;
    Linux) printf '%s\n' "linux" ;;
    MINGW*|MSYS*|CYGWIN*) printf '%s\n' "windows" ;;
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

target_for() {
  local os_name="$1"
  local arch_name="$2"

  case "${os_name}/${arch_name}" in
    linux/amd64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
    darwin/amd64) printf '%s\n' "x86_64-apple-darwin" ;;
    darwin/arm64) printf '%s\n' "aarch64-apple-darwin" ;;
    windows/amd64) printf '%s\n' "x86_64-pc-windows-msvc" ;;
    *) die "unsupported release target ${os_name}/${arch_name}" ;;
  esac
}

verify_e2ee_feature_graph() {
  if [[ "${AWIKI_CLI_SKIP_E2EE_RELEASE_FEATURE_CHECK:-0}" == "1" ]]; then
    echo "Skipping E2EE release feature graph check."
    return
  fi

  case "${OS_NAME}" in
    linux|darwin)
      ;;
    windows)
      echo "Windows E2EE package/release validation is deferred for this stage; artifact build continues without an E2EE release gate."
      return
      ;;
    *)
      return
      ;;
  esac

  local tree_output
  tree_output="$("${cargo_cmd[@]}" tree -p awiki-cli -e features --locked)"
  for required in \
    'im-core feature "group-e2ee"' \
    'anp feature "mls"'
  do
    if ! grep -Fq "${required}" <<<"${tree_output}"; then
      echo "${tree_output}" >&2
      die "release feature graph is missing ${required}"
    fi
  done
  echo "Verified Linux/macOS E2EE release feature graph: im-core/group-e2ee and anp/mls."
}

VERSION=""
OS_NAME=""
ARCH_NAME=""
TARGET_TRIPLE=""
DIST_DIR="${ROOT_DIR}/dist"
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      [[ -n "${VERSION}" ]] || die "--version requires a value"
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
    --dist)
      DIST_DIR="${2:-}"
      [[ -n "${DIST_DIR}" ]] || die "--dist requires a value"
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

VERSION="${VERSION:-$(read_package_version)}"
VERSION="${VERSION#v}"
[[ -n "${VERSION}" ]] || die "version is required and package.json.version is empty"

OS_NAME="${OS_NAME:-$(host_os)}"
ARCH_NAME="${ARCH_NAME:-$(host_arch)}"
TARGET_TRIPLE="${TARGET_TRIPLE:-$(target_for "${OS_NAME}" "${ARCH_NAME}")}"

case "${OS_NAME}" in
  linux|darwin|windows) ;;
  *) die "unsupported release OS ${OS_NAME}" ;;
esac
case "${ARCH_NAME}" in
  amd64|arm64) ;;
  *) die "unsupported release arch ${ARCH_NAME}" ;;
esac
expected_target="$(target_for "${OS_NAME}" "${ARCH_NAME}")"
[[ "${TARGET_TRIPLE}" == "${expected_target}" ]] || die \
  "target triple ${TARGET_TRIPLE} does not match ${OS_NAME}/${ARCH_NAME} (expected ${expected_target})"

cargo_bin="${CARGO:-cargo}"
toolchain="${AWIKI_CLI_RUST_TOOLCHAIN:-1.88.0}"
if [[ "${cargo_bin}" == "cargo" && -n "${toolchain}" ]]; then
  cargo_cmd=(cargo "+${toolchain}")
else
  cargo_cmd=("${cargo_bin}")
fi

node - "${ROOT_DIR}/scripts/release/cli/release-config.json" "${VERSION}" <<'NODE'
const fs = require('fs');
const [configPath, version] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
const versions = Object.values(config.channels || {}).map(entry => entry.version);
if (!versions.includes(version)) {
  throw new Error(`version ${version} is not declared in ${configPath}`);
}
NODE

commit="${AWIKI_CLI_COMMIT:-$(git rev-parse --short HEAD 2>/dev/null || printf '%s' unknown)}"
build_date="${AWIKI_CLI_BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"
bin_name="awiki-cli"
if [[ "${OS_NAME}" == "windows" ]]; then
  bin_name="awiki-cli.exe"
fi

build_bin="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/${bin_name}"
archive_base="awiki-cli-${VERSION}-${OS_NAME}-${ARCH_NAME}"
if [[ "${OS_NAME}" == "windows" ]]; then
  archive_path="${DIST_DIR}/${archive_base}.zip"
else
  archive_path="${DIST_DIR}/${archive_base}.tar.gz"
fi

if [[ "${DRY_RUN}" == "1" ]]; then
  cat <<EOF
Would run: AWIKI_CLI_VERSION=${VERSION} AWIKI_CLI_COMMIT=${commit} AWIKI_CLI_BUILD_DATE=${build_date} AWIKI_CLI_CGO_ENABLED=0 ${cargo_cmd[*]} build -p awiki-cli --bin awiki-cli --release --locked --target ${TARGET_TRIPLE}
Would verify: ${cargo_cmd[*]} tree -p awiki-cli -e features --locked includes im-core/group-e2ee and anp/mls on Linux/macOS
Would archive: ${build_bin} -> ${archive_path}
EOF
  exit 0
fi

verify_e2ee_feature_graph

AWIKI_CLI_VERSION="${VERSION}" \
AWIKI_CLI_COMMIT="${commit}" \
AWIKI_CLI_BUILD_DATE="${build_date}" \
AWIKI_CLI_CGO_ENABLED=0 \
  "${cargo_cmd[@]}" build -p awiki-cli --bin awiki-cli --release --locked --target "${TARGET_TRIPLE}"

[[ -f "${build_bin}" ]] || die "built binary not found: ${build_bin}"

mkdir -p "${DIST_DIR}"
stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/awiki-release.XXXXXX")"
cleanup() {
  rm -rf "${stage_dir}"
}
trap cleanup EXIT

cp "${build_bin}" "${stage_dir}/${bin_name}"
if [[ "${OS_NAME}" != "windows" ]]; then
  chmod 0755 "${stage_dir}/${bin_name}"
fi

rm -f "${archive_path}"
if [[ "${OS_NAME}" == "windows" ]]; then
  archive_dir="$(cd "$(dirname "${archive_path}")" && pwd)"
  archive_abs="${archive_dir}/$(basename "${archive_path}")"
  if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
    python_cmd="python3"
    if ! command -v python3 >/dev/null 2>&1; then
      python_cmd="python"
    fi
    "${python_cmd}" - "${stage_dir}" "${bin_name}" "${archive_abs}" <<'PY'
import pathlib
import sys
import zipfile

stage_dir = pathlib.Path(sys.argv[1])
bin_name = sys.argv[2]
archive_path = pathlib.Path(sys.argv[3])
with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
    archive.write(stage_dir / bin_name, arcname=bin_name)
PY
  elif command -v pwsh >/dev/null 2>&1 || command -v powershell.exe >/dev/null 2>&1; then
    powershell_cmd="pwsh"
    if ! command -v pwsh >/dev/null 2>&1; then
      powershell_cmd="powershell.exe"
    fi
    "${powershell_cmd}" -NoProfile -Command \
      "Compress-Archive -LiteralPath '${stage_dir}/${bin_name}' -DestinationPath '${archive_abs}' -Force"
  elif command -v zip >/dev/null 2>&1; then
    (cd "${stage_dir}" && zip -q -9 "${archive_abs}" "${bin_name}")
  else
    die "zip archive creation requires python, PowerShell, or zip"
  fi
else
  COPYFILE_DISABLE=1 tar -C "${stage_dir}" -czf "${archive_path}" "${bin_name}"
fi

echo "release archive created: ${archive_path}"
