#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

cargo_bin="${CARGO:-cargo}"
toolchain="${AWIKI_CLI_RUST_TOOLCHAIN:-1.85.1}"

if [[ "${cargo_bin}" == "cargo" && -n "${toolchain}" ]]; then
  cargo_cmd=(cargo "+${toolchain}")
else
  cargo_cmd=("${cargo_bin}")
fi

tests=(
  legacy_path_cutover_contract
  cli_cutover_command_surface_contract
  command_catalog_schema_contract
  m_core_cli_adapter_policy_contract
  group_e2ee_cutover_policy_contract
)

for test_name in "${tests[@]}"; do
  "${cargo_cmd[@]}" test -p awiki-cli --test "${test_name}" --locked
done
