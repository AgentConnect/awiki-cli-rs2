#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${ROOT_DIR}"
export COPYFILE_DISABLE=1

usage() {
  cat <<'USAGE'
Build an awiki-deamon release archive.

Usage:
  scripts/release/daemon/_build-artifact.sh [--version VERSION] [--os OS] [--arch ARCH] [--target TRIPLE] [--dist DIR] [--dry-run]

Options:
  --version VERSION   Package version. Defaults to crates/awiki-deamon/Cargo.toml package version.
  --os OS            Release OS name: linux or darwin. Defaults to current host.
  --arch ARCH        Release arch name: amd64 or arm64. Defaults to current host.
  --target TRIPLE    Rust target triple. Defaults from --os/--arch.
  --dist DIR         Output directory. Defaults to dist/daemon.
  --dry-run          Print the plan without building.
USAGE
}

die() {
  echo "Error: $*" >&2
  exit 1
}

read_crate_version() {
  awk '
    $1 == "version" && $2 == "=" {
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

target_for() {
  case "$1/$2" in
    linux/amd64) printf '%s\n' "x86_64-unknown-linux-musl" ;;
    linux/arm64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
    darwin/amd64) printf '%s\n' "x86_64-apple-darwin" ;;
    darwin/arm64) printf '%s\n' "aarch64-apple-darwin" ;;
    *) die "unsupported daemon release target $1/$2" ;;
  esac
}

verify_release_binary() {
  local binary="$1"
  local expected_version="$2"
  "${binary}" __self-check --expected-version "${expected_version}" >/dev/null
  if [[ "${OS_NAME}" == "linux" ]]; then
    if command -v strings >/dev/null 2>&1 && strings "${binary}" | grep -q 'GLIBC_[0-9]'; then
      die "Linux daemon release binary contains GLIBC symbol requirements; build a musl/static-compatible package"
    fi
  fi
}

VERSION=""
OS_NAME=""
ARCH_NAME=""
TARGET_TRIPLE=""
DIST_DIR="${ROOT_DIR}/dist/daemon"
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

VERSION="${VERSION:-$(read_crate_version)}"
VERSION="${VERSION#v}"
[[ -n "${VERSION}" ]] || die "version is required"
OS_NAME="${OS_NAME:-$(host_os)}"
ARCH_NAME="${ARCH_NAME:-$(host_arch)}"
TARGET_TRIPLE="${TARGET_TRIPLE:-$(target_for "${OS_NAME}" "${ARCH_NAME}")}"

case "${OS_NAME}" in
  linux|darwin) ;;
  *) die "unsupported daemon release OS ${OS_NAME}" ;;
esac
case "${ARCH_NAME}" in
  amd64|arm64) ;;
  *) die "unsupported daemon release arch ${ARCH_NAME}" ;;
esac

cargo_bin="${CARGO:-cargo}"
toolchain="${AWIKI_CLI_RUST_TOOLCHAIN:-1.88.0}"
if [[ "${cargo_bin}" == "cargo" && -n "${toolchain}" ]]; then
  cargo_cmd=(cargo "+${toolchain}")
else
  cargo_cmd=("${cargo_bin}")
fi

archive_path="${DIST_DIR}/awiki-deamon-${OS_NAME}-${ARCH_NAME}.tar.gz"
build_bin="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/awiki-deamon"

if [[ "${DRY_RUN}" == "1" ]]; then
  cat <<EOF
Would run: ${cargo_cmd[*]} build -p awiki-deamon --bin awiki-deamon --release --locked --target ${TARGET_TRIPLE}
Would archive: ${build_bin} -> ${archive_path}
Would include: awiki-deamon awiki-deamon-runtime README.txt LICENSE checksums.txt
EOF
  exit 0
fi

"${cargo_cmd[@]}" build -p awiki-deamon --bin awiki-deamon --release --locked --target "${TARGET_TRIPLE}"
[[ -f "${build_bin}" ]] || die "built daemon binary not found: ${build_bin}"
verify_release_binary "${build_bin}" "${VERSION}"

mkdir -p "${DIST_DIR}"
stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/awiki-daemon-release.XXXXXX")"
cleanup() {
  rm -rf "${stage_dir}"
}
trap cleanup EXIT

cp "${build_bin}" "${stage_dir}/awiki-deamon"
chmod 0755 "${stage_dir}/awiki-deamon"
if ln -s awiki-deamon "${stage_dir}/awiki-deamon-runtime" 2>/dev/null; then
  :
else
  cp "${stage_dir}/awiki-deamon" "${stage_dir}/awiki-deamon-runtime"
fi

cat >"${stage_dir}/README.txt" <<EOF
Awiki Daemon Agent Runtime Host ${VERSION}

Install through the official installer:
curl -fsSL https://<service-domain>/daemon/install.sh | sh -s -- --token <token>
EOF

if [[ -f LICENSE ]]; then
  cp LICENSE "${stage_dir}/LICENSE"
else
  printf '%s\n' "License: see repository metadata." >"${stage_dir}/LICENSE"
fi

(cd "${stage_dir}" && shasum -a 256 awiki-deamon awiki-deamon-runtime README.txt LICENSE > checksums.txt)
rm -f "${archive_path}"
tar -C "${stage_dir}" -czf "${archive_path}" awiki-deamon awiki-deamon-runtime README.txt LICENSE checksums.txt
echo "daemon release archive created: ${archive_path}"
