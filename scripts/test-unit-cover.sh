#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

check_coverage=0
show_profile_path=0
passthrough_args=()

while (($# > 0)); do
  case "$1" in
    --check)
      check_coverage=1
      ;;
    --show-profile)
      show_profile_path=1
      ;;
    *)
      passthrough_args+=("$1")
      ;;
  esac
  shift
done

cleanup_dir=""
coverprofile="${AWIKI_CLI_COVERPROFILE:-}"
if [[ -z "${coverprofile}" ]]; then
  cleanup_dir="$(mktemp -d "${TMPDIR:-/tmp}/awiki-cli-cover.XXXXXX")"
  coverprofile="${cleanup_dir}/unit-cover.out"
fi
mkdir -p "$(dirname "${coverprofile}")"

cleanup() {
  if [[ -n "${cleanup_dir}" ]]; then
    rm -rf "${cleanup_dir}"
  fi
}
trap cleanup EXIT

cargo_bin="${CARGO:-cargo}"
toolchain="${AWIKI_CLI_RUST_TOOLCHAIN:-1.88.0}"
if [[ "${cargo_bin}" == "cargo" && -n "${toolchain}" ]]; then
  cargo_cmd=(cargo "+${toolchain}")
else
  cargo_cmd=("${cargo_bin}")
fi

if ! "${cargo_cmd[@]}" llvm-cov --version >/dev/null 2>&1; then
  if [[ "${check_coverage}" == "1" ]]; then
    echo "Error: cargo-llvm-cov is required for --check coverage validation." >&2
    echo "Install it with: cargo install cargo-llvm-cov" >&2
    exit 1
  fi

  echo "cargo-llvm-cov is not installed; running unit tests without coverage." >&2
  "${ROOT_DIR}/scripts/test-unit.sh" "${passthrough_args[@]}"
  exit 0
fi

coverage_cmd=("${cargo_cmd[@]}" llvm-cov --package awiki-cli --locked --lcov --output-path "${coverprofile}")
if ((${#passthrough_args[@]} > 0)); then
  coverage_cmd+=("${passthrough_args[@]}")
fi
"${coverage_cmd[@]}"

if [[ "${check_coverage}" == "1" ]]; then
  python3 scripts/check_rust_coverage.py "${coverprofile}"
fi

if [[ "${show_profile_path}" == "1" || -n "${AWIKI_CLI_COVERPROFILE:-}" ]]; then
  echo "Coverage profile: ${coverprofile}"
fi
