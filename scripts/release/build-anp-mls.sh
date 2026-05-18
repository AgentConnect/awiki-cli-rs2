#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Build and stage the Rust anp-mls binary for awiki-cli release artifacts.

Usage:
  scripts/release/build-anp-mls.sh [--dry-run] [--output DIR]

Environment:
  ANP_MLS_SOURCE_DIR           Path to the anp/rust crate (default: ../anp/rust)
  AWIKI_ANP_MLS_RELEASE_DIR    Output directory (default: dist/anp-mls)
  CARGO                        Cargo binary (default: cargo)

The staged binary can be bundled next to awiki-cli artifacts or installed on PATH.
Users can always override discovery with AWIKI_ANP_MLS_BINARY=/absolute/path/to/anp-mls.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_dir="${ANP_MLS_SOURCE_DIR:-$(cd "${repo_root}/.." && pwd)/anp/rust}"
output_dir="${AWIKI_ANP_MLS_RELEASE_DIR:-${repo_root}/dist/anp-mls}"
dry_run=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=1
      shift
      ;;
    --output)
      if [[ $# -lt 2 ]]; then
        echo "--output requires a directory" >&2
        exit 2
      fi
      output_dir="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

cargo_bin="${CARGO:-cargo}"
manifest_path="${source_dir}/Cargo.toml"
if [[ ! -f "${manifest_path}" ]]; then
  echo "anp-mls Cargo.toml not found: ${manifest_path}" >&2
  echo "Set ANP_MLS_SOURCE_DIR to the sibling anp/rust crate." >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin) os_name="darwin" ;;
  Linux) os_name="linux" ;;
  *) os_name="$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch_name="amd64" ;;
  arm64|aarch64) arch_name="arm64" ;;
  *) arch_name="$(uname -m)" ;;
esac

bin_name="anp-mls"
if [[ "${os_name}" == "windows" ]]; then
  bin_name="anp-mls.exe"
fi
src_bin="${source_dir}/target/release/${bin_name}"
stage_dir="${output_dir}/${os_name}-${arch_name}"
staged_bin="${stage_dir}/${bin_name}"

if [[ ${dry_run} -eq 1 ]]; then
  cat <<EOF_DRY
Would run: ${cargo_bin} build --manifest-path ${manifest_path} --bin anp-mls --release
Would stage: ${src_bin} -> ${staged_bin}
EOF_DRY
  exit 0
fi

"${cargo_bin}" build --manifest-path "${manifest_path}" --bin anp-mls --release
mkdir -p "${stage_dir}"
cp "${src_bin}" "${staged_bin}"
chmod 0755 "${staged_bin}"

echo "anp-mls staged at ${staged_bin}"
echo "Set AWIKI_ANP_MLS_BINARY=${staged_bin} or place it on PATH for awiki-cli group E2EE."
