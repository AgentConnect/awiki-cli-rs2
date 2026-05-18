# 2026-05-18 Message Group E2EE Create/Add/Repair/Decrypt Selector Batch

Timestamp: 2026-05-18T22:44:02+0800.

Scope: expose existing Rust fake-service contracts for Go group E2EE create,
add, repair, rejoin, and decrypt/cache-projection behavior to the
`awiki-system-test` acceptance surface. This batch does not change production
Rust behavior, does not add dependencies, and does not run or count mail
selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the
  `message/group` E2EE create/add/repair/decrypt cluster.
- Three read-only Native Agents mapped Go candidate behavior, Rust contract
  targets and line counts, and the current system-test selector surface in
  parallel. The Go mapper also identified group attachment as a good next
  non-mail batch candidate; it is recorded for later and not mixed into this
  batch.
- The Rust/system-test gap matrix showed existing Rust production and contract
  coverage. The remaining gap was system-test selector visibility for the
  existing focused Cargo contract targets.
- The system-test wrapper was extended with a separate selector that verifies
  expected Rust test functions exist, then runs each target once to reduce
  repeated Cargo startup cost.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/group.go`, `internal/message/group_e2ee_service.go` | `group create --e2ee` creates the P4 group, bootstraps MLS, publishes hidden group E2EE head, and downgrades provider failure to warnings without E2EE data | `crates/awiki-cli/src/message/group_e2ee_create.rs`, `crates/awiki-cli/src/message/group_e2ee_provider.rs` | implemented and already focused-contract tested | `group_e2ee_create_contract` passed 2 tests | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_create_add_repair_decrypt_rust_contracts` | low after selector exposure |
| `internal/cli/group.go`, `internal/message/group_e2ee_service.go` | `group add --e2ee` leases KeyPackage, prepares MLS add, publishes hidden add, preserves P4 add on warning paths, and exposes rejoin as canonical group-add plan | `crates/awiki-cli/src/message/group_e2ee_add.rs`, `crates/awiki-cli/src/app/group_e2ee_handlers.rs` | implemented and already focused-contract tested | `group_e2ee_add_contract` passed 6 tests | same Rust-only selector | low for selector exposure; file is near the 1200-line review threshold so future test additions should split/extract helpers |
| `internal/message/group_e2ee_service.go` | `group e2ee repair` pulls hidden notices, replays commit delivery through the provider, marks processed notices delivered, and reports diagnosis | `crates/awiki-cli/src/message/group_e2ee_repair.rs` | implemented and already focused-contract tested | `group_e2ee_repair_contract` passed 1 test | same Rust-only selector | low after selector exposure |
| `internal/message/group_e2ee_service.go`, `internal/message/group_wire.go` | `group messages` decrypts group E2EE cipher messages before cache projection, accepts Go cipher object locations, and preserves cipher projection with warning on decrypt failure | `crates/awiki-cli/src/message/group_e2ee_decrypt.rs`, `crates/awiki-cli/src/message/group_e2ee_wire.rs` | implemented and already focused-contract tested | `group_e2ee_decrypt_contract` passed 3 tests | same Rust-only selector | low after selector exposure |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_repair_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_decrypt_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli -run 'Test.*(GroupE2EE|E2EE|Create|Add|Repair|Decrypt|Messages|Cipher|Welcome|Notice|KeyPackage|DryRun)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_create_add_repair_decrypt_rust_contracts -ra -q
```

Observed results:

- Rust `group_e2ee_create_contract`: 2 passed, 0 failed.
- Rust `group_e2ee_add_contract`: 6 passed, 0 failed.
- Rust `group_e2ee_repair_contract`: 1 passed, 0 failed.
- Rust `group_e2ee_decrypt_contract`: 3 passed, 0 failed.
- Focused Go `internal/message` and `internal/cli` guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  82.24s. The selector exposes 12 Rust fake-service group E2EE contracts
  across four Cargo targets.
- File-size check after the batch:
  `group_e2ee_create_contract.rs` 674 lines,
  `group_e2ee_add_contract.rs` 1072 lines,
  `group_e2ee_repair_contract.rs` 578 lines, and
  `group_e2ee_decrypt_contract.rs` 737 lines. No Rust source or test file
  grew in this batch, and no file-size exception is needed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_create_add_repair_decrypt_rust_contracts`.
- The selector runs deterministic Rust fake-service Cargo contracts only. It
  does not start live user/message services, does not require mail services,
  and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic local contracts
  that already covered Go create, add, repair, rejoin, and decrypt projection
  surfaces.
- It does not claim new production behavior, live real-service forced failure
  coverage, WebSocket/local bridge group E2EE transport, foreground listener
  group E2EE handling, group attachment behavior, or mail selectors.
- The Go mapper identified group attachment send HTTP-warning/backfill parity
  as a good next non-mail message/group batch. That is recorded as a future
  module-batch candidate, not part of this selector batch.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing `std::process::Command`, `serde_json`,
existing Rustls/std message HTTP transport, existing group E2EE wire builders,
the external local ANP Rust SDK `anp-mls` binary boundary, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, WebSocket crates, async runtimes, MLS provider crates, or a
new SQLite backend. TLS remains Rustls-first.
