# 2026-05-18 Message Group E2EE Recover/Update RPC Deterministic Batch

Timestamp: 2026-05-18T24:15:00+0800.

Scope: close focused contract and selector gaps for deterministic JSON-RPC
submit failures in Go `RecoverGroupE2EEMember` and `UpdateGroupE2EEKey`. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the
  `message/group` E2EE recover/update JSON-RPC deterministic submit cluster.
- The Go/Rust scan confirmed the shared deterministic classifier: HTTP 4xx
  aborts, HTTP 5xx does not abort, and JSON-RPC `rpc_code >= 2000` aborts.
- Rust production code already matched Go. The gap was focused fake-service
  coverage proving recover/update abort for JSON-RPC deterministic service
  rejections, plus system-test selector visibility.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/group_e2ee_service.go` `RecoverGroupE2EEMember` plus `shouldAbortGroupE2EEPendingCommit` | JSON-RPC code `>= 2000` from hidden `group.e2ee.recover_member` is deterministic and aborts the local pending commit through generic `CommitAbort` | `crates/awiki-cli/src/message/group_e2ee_recover.rs`, `group_e2ee_remove.rs` | implemented and now focused-contract tested | `group_e2ee_recover_member_contract::group_e2ee_recover_member_rpc_deterministic_submit_failure_aborts_pending_commit_like_go` | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts` | low after fake-service RPC 2001 contract |
| `internal/message/group_e2ee_service.go` `UpdateGroupE2EEKey` plus `shouldAbortGroupE2EEPendingCommit` | JSON-RPC code `>= 2000` from hidden `group.e2ee.update` is deterministic and aborts the local pending update through `UpdateMemberAbort` | `crates/awiki-cli/src/message/group_e2ee_update.rs`, `group_e2ee_remove.rs` | implemented and now focused-contract tested | `group_e2ee_update_key_contract::group_e2ee_update_key_rpc_deterministic_submit_failure_aborts_pending_update_like_go` | same Rust-only selector | low after fake-service RPC 2001 contract |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_update_key_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli -run 'Test.*(GroupE2EE|E2EE|RecoverMember|Update|Repair|Leave|Remove|Submit|Abort|Finalize|Epoch|RPC)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts -ra -q
```

Observed results:

- Rust `group_e2ee_recover_member_contract`: 5 passed, 0 failed.
- Rust `group_e2ee_update_key_contract`: 5 passed, 0 failed.
- Focused Go `internal/message` and `internal/cli` guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- `cargo fmt --check`, `cargo check`, `xtask check-structure`, and
  `git diff --check`: passed.
- Extended focused non-mail system-test selector: 1 passed, 0 failed, 0
  skipped in 179.52s. The wrapper now exposes 12 Rust fake-service group E2EE
  contracts across stale send retry, remove/leave negative paths, recover
  negative paths, and update negative paths.
- File-size check after edits:
  `group_e2ee_recover_member_contract.rs` 1165 lines and
  `group_e2ee_update_key_contract.rs` 1141 lines. Both remain below the
  1200-line structure visibility threshold and below the ordinary 3000-line
  cap; no file-size exception is needed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts`.
- The selector runs deterministic Rust fake-service Cargo contracts only. It
  does not start live user/message services, does not require mail services,
  and does not count mail selectors.

Coverage boundary:

- This batch promotes deterministic contract evidence for recover/update
  JSON-RPC code `>= 2000` submit failures.
- It does not claim live real-service forced recover/update RPC failure
  selectors, WebSocket/local bridge group E2EE transport, foreground listener
  group E2EE handling, or mail selectors.
- It does not modify the shared deterministic classifier; the batch only proves
  the existing Rust implementation matches Go behavior for these paths.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing `std::process::Command`, `serde_json`,
existing Rustls/std message HTTP transport, existing group E2EE wire builders,
the external local ANP Rust SDK `anp-mls` binary boundary, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, WebSocket crates, async runtimes, MLS provider crates, or a
new SQLite backend.
