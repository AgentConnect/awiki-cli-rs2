#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${ROOT_DIR}"
export COPYFILE_DISABLE=1

usage() {
  cat <<'USAGE'
Build an awiki-deamon release archive.

Usage:
  scripts/release/daemon/_build-artifact.sh [--version VERSION] [--os OS] [--arch ARCH] [--target TRIPLE] [--dist DIR] [--local-core] [--dry-run]

Options:
  --version VERSION   Package version. Defaults to crates/awiki-deamon/Cargo.toml package version.
  --os OS            Release OS name: linux or darwin. Defaults to current host.
  --arch ARCH        Release arch name: amd64 or arm64. Defaults to current host.
  --target TRIPLE    Rust target triple. Defaults from --os/--arch.
  --dist DIR         Output directory. Defaults to dist/daemon.
  --test-sources FILE Explicit pinned-source Singapore test build; never a registry release.
  --local-core       Explicit temporary build with committed local Core; other SDKs stay registry.
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
LOCAL_CORE=0
TEST_SOURCES=""

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
    --test-sources)
      TEST_SOURCES="${2:-}"
      [[ -n "${TEST_SOURCES}" ]] || die "--test-sources requires a manifest"
      shift 2
      ;;
    --local-core)
      LOCAL_CORE=1
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
commit="${AWIKI_DAEMON_COMMIT:-$(git rev-parse HEAD 2>/dev/null || printf '%s' unknown)}"
[[ "${commit}" =~ ^[a-fA-F0-9]{40}$ ]] || die "source commit must be a full commit SHA"
anp_commit="${AWIKI_DAEMON_ANP_COMMIT:-$(git -C ../anp/anp rev-parse HEAD 2>/dev/null || printf '%s' unknown)}"
[[ "${anp_commit}" =~ ^[a-fA-F0-9]{40}$ ]] || die "ANP source commit must be a full commit SHA"
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
provenance_path="${archive_path}.source.json"
target_dir="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
case "${target_dir}" in /*) ;; *) target_dir="${ROOT_DIR}/${target_dir}" ;; esac
build_bin="${target_dir}/${TARGET_TRIPLE}/release/awiki-deamon"
if [[ -n "${TEST_SOURCES}" ]]; then
  [[ "${LOCAL_CORE}" == "0" ]] || die "source modes are mutually exclusive"
  cargo_cmd=(python3 "${ROOT_DIR}/scripts/release/test-source-build.py" --manifest "${TEST_SOURCES}" --provenance "${provenance_path}" -- "${cargo_cmd[@]}")
elif [[ "${LOCAL_CORE}" == "1" ]]; then
  [[ "${commit}" == "$(git rev-parse HEAD)" ]] || die "local Core source commit must match HEAD"
  cargo_cmd=(python3 "${ROOT_DIR}/scripts/release/daemon/local-core-build.py" --provenance "${provenance_path}" -- "${cargo_cmd[@]}")
else
  cargo_cmd=(python3 "${ROOT_DIR}/scripts/release/registry-build.py" -- "${cargo_cmd[@]}")
fi

if [[ "${DRY_RUN}" == "1" ]]; then
  cat <<EOF
Would run: ${cargo_cmd[*]} build -p awiki-deamon --bin awiki-deamon --release --locked --target ${TARGET_TRIPLE}
Would archive: ${build_bin} -> ${archive_path}
Would include: awiki-deamon awiki-deamon-runtime README.txt LICENSE LICENSE-APACHE COMMERCIAL-LICENSING.md SOURCE.md checksums.txt acp/
Would prepare: pinned ACP adapters for host Node for ${OS_NAME}/${ARCH_NAME}
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

python3 scripts/release/daemon/prepare-acp-components.py \
  --os "${OS_NAME}" --arch "${ARCH_NAME}" --output "${stage_dir}/acp"

cat >"${stage_dir}/README.txt" <<EOF
Awiki Daemon Agent Runtime Host ${VERSION}

Install through the official installer:
curl -fsSL https://<service-domain>/daemon/install.sh | sh -s -- --token <token>
EOF

cp LICENSE "${stage_dir}/LICENSE"
cp LICENSES/Apache-2.0.txt "${stage_dir}/LICENSE-APACHE"
cp COMMERCIAL-LICENSING.md "${stage_dir}/COMMERCIAL-LICENSING.md"
cat >"${stage_dir}/SOURCE.md" <<EOF
# AWiki Daemon Corresponding Source

Version: ${VERSION}
Commit: ${commit}
Source: https://github.com/AgentConnect/awiki-cli-rs2/tree/${commit}
Source archive: https://github.com/AgentConnect/awiki-cli-rs2/archive/${commit}.tar.gz
Build instructions: https://github.com/AgentConnect/awiki-cli-rs2/blob/${commit}/docs/development.md

ANP dependency commit: ${anp_commit}
ANP source: https://github.com/agent-network-protocol/anp/tree/${anp_commit}

The source location above identifies the exact revision used to build this
release. The Corresponding Source is provided under Apache License 2.0 as described in
the accompanying LICENSE file.
EOF

if [[ "${LOCAL_CORE}" == "1" || -n "${TEST_SOURCES}" ]]; then
  mode="local-core"
  [[ -z "${TEST_SOURCES}" ]] || mode="test-source (Singapore test build; SDKs unpublished)"
  printf '\nDependency mode: %s\n\n' "${mode}" >> "${stage_dir}/SOURCE.md"
  cat "${provenance_path}" >> "${stage_dir}/SOURCE.md"
fi

python3 - "${stage_dir}" <<'PY'
import hashlib
from pathlib import Path
import sys

root = Path(sys.argv[1])
with (root / "checksums.txt").open("w") as output:
    for path in sorted(root.rglob("*")):
        if path.name == "checksums.txt" and path.parent == root or not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if any(character in relative for character in "\r\n\\\0"):
            raise SystemExit("invalid package checksum path")
        digest = hashlib.sha256()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
        output.write(f"{digest.hexdigest()}  {relative}\n")
PY
rm -f "${archive_path}"
COPYFILE_DISABLE=1 tar -C "${stage_dir}" -czf "${archive_path}" \
  awiki-deamon awiki-deamon-runtime README.txt LICENSE LICENSE-APACHE \
  COMMERCIAL-LICENSING.md SOURCE.md checksums.txt acp
echo "daemon release archive created: ${archive_path}"
