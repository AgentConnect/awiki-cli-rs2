# 2026-05-18 Workspace Config Update Selector Batch

Timestamp: 2026-05-18T23:42:00+0800.

Scope: expose existing Rust workspace/config/update/local-upgrade contracts to
the `awiki-system-test` acceptance surface through a non-mail Rust-only
selector. This batch does not change production Rust behavior, does not add
dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the
  workspace/config/update/local-upgrade cluster.
- Three read-only Native Agents mapped Go config/update/upgrade behavior,
  current Rust implementation/tests, and non-mail system-test selector
  coverage in parallel.
- The scans found no production Rust gap in the scoped cluster. The actionable
  gap was selector visibility for existing Rust Cargo contract targets.
- The new system-test wrapper validates that each expected target exists and
  runs each Cargo target once.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/config/config.go`, `internal/config/write.go`, `internal/cli/config.go` | config resolution, deprecated service URL guards, `config show` snapshot, `config set --did-domain`, atomic config write and mutators | `crates/awiki-cli/src/config/mod.rs`, `src/config/write.rs`, `src/app.rs` | implemented and focused-contract tested | `config_policy_contract` passed 4 tests; `config_writer_contract` passed 7 tests | `tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py::test_awiki_cli_workspace_config_update_rust_contracts` | low after selector exposure |
| `internal/update/update.go`, `internal/cli/root.go`, `internal/cli/upgrade.go` | update cache-only policy, strict disable/minimum-version handling, root preflight exemptions, `upgrade` status/install decision path | `crates/awiki-cli/src/update/*`, `src/app/update_preflight.rs`, `src/app/update_handlers.rs` | implemented and focused-contract tested | `update_contract` passed 6 tests | same Rust-only selector | low for deterministic update/preflight behavior; live npm install/registry path remains out of scope |
| `internal/upgrade/*`, `internal/cli/app.go`, `internal/cli/init.go` | workspace schema detection, automatic `upgrade_if_needed`, backup, lock, journal, meta, migration chain 0->1->2->3, config-show/doctor status surface | `crates/awiki-cli/src/upgrade/*`, `src/app.rs`, `src/doctor/mod.rs` | implemented and focused-contract tested | `workspace_upgrade_contract` passed 20 tests; `workspace_upgrade_if_needed_contract` passed 13 tests; `workspace_migration_v0_to_v1_contract` passed 25 tests | same Rust-only selector | low after selector exposure; no standalone workspace-upgrade CLI command is claimed |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test update_contract --test config_policy_contract --test config_writer_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --test workspace_upgrade_if_needed_contract --test workspace_migration_v0_to_v1_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/config ./internal/upgrade ./internal/cli -run 'Test.*(Config|Upgrade|Update|Preflight|Workspace|Migration|Strict|Deprecated|Durable|Write)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/update/test_update_policy.py -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py::test_awiki_cli_workspace_config_update_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust direct workspace/config/update targets passed 75 tests total:
  `update_contract` 6, `config_policy_contract` 4,
  `config_writer_contract` 7, `workspace_upgrade_contract` 20,
  `workspace_upgrade_if_needed_contract` 13, and
  `workspace_migration_v0_to_v1_contract` 25.
- Focused Go `internal/config`, `internal/upgrade`, and `internal/cli` guard:
  passed.
- Existing non-mail `tests_v2/update/test_update_policy.py`: 4 passed, 0
  failed, 0 skipped.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  1.14s.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- The new system-test wrapper is below the default review-size limit. Rust
  files did not grow in this batch. The largest related Rust contract targets
  remain under the current ordinary 3000-line cap and near the older 1200-line
  visibility threshold, so future local-upgrade additions should prefer a split
  or a documented exception before growing them further.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_workspace_config_update_rust_contracts.py::test_awiki_cli_workspace_config_update_rust_contracts`.
- The selector runs deterministic Rust Cargo contracts only. It does not start
  live mail services and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic local contracts
  that already covered config policy/writer behavior, update/root-preflight
  policy, workspace inspection, locking, backup, journaling, meta handling, and
  migrations.
- It does not claim new production behavior, full repository-wide acceptance,
  mail selectors, a live npm install or external registry upgrade path, or a
  standalone workspace-upgrade CLI command. Go exposes workspace upgrade through
  automatic workspace preflight and status surfaces.
- Identity-specific upgrade command-boundary selectors are left for the
  identity lane and are not part of this workspace/config/update batch.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses the approved `rusqlite + bundled` SQLite path,
existing config/update/upgrade code, and the pure-Rust/cross-platform
dependency policy. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, or SQLite backend change was introduced.
