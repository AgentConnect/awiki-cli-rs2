# 2026-05-18 Message Group E2EE Stale/Negative Contract Batch

Timestamp: 2026-05-18T22:40:00+0800.

Scope: close focused contract and selector gaps for Go `group_e2ee_service.go`
stale-epoch outbound send repair/retry and remove/leave pending-commit
submit/finalize/abort negative paths. This batch does not change production
Rust behavior, does not add a dependency, and does not run or count mail
selectors.

Pipeline:

- Read-only Native Agents mapped Go group E2EE stale retry, Rust group E2EE
  implementation/tests, and system-test selector coverage in parallel before
  implementation.
- The gap matrix showed existing Rust production surfaces in
  `group_e2ee_send.rs`, `group_e2ee_repair.rs`, and
  `group_e2ee_remove.rs`; the gap was focused fake-service contract coverage
  and system-test selector visibility.
- Code-writing Native Agents were started with bounded write scopes but were
  shut down after they failed to produce timely final results. The leader
  integrated the final bounded patches and validation. The stale-retry test
  landed in the assigned send contract file before shutdown and was reviewed by
  the leader; remove/leave negative tests were completed by the leader.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/group_e2ee_service.go` `sendGroupE2EE` | first hidden `group.e2ee.send` can fail with epoch mismatch; Go repairs stale notices, re-encrypts with retry IDs, and retries send | `crates/awiki-cli/src/message/group_e2ee_send.rs`, `crates/awiki-cli/src/message/group_e2ee_repair.rs` | implemented and now focused-contract tested | `group_e2ee_send_contract::msg_send_group_e2ee_retries_after_stale_epoch_mismatch_like_go` | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts` | medium reduced to contract-only residual; live real-service forced epoch mismatch remains outside deterministic system tests |
| `internal/message/group_e2ee_service.go` `submitPreparedGroupE2EECommit` | deterministic service rejection aborts local pending commit | `crates/awiki-cli/src/message/group_e2ee_remove.rs` | implemented and now focused-contract tested | `group_e2ee_remove_leave_contract::group_remove_e2ee_deterministic_submit_failure_aborts_pending_commit_like_go` | same Rust-only selector | low for remove path; recover/update duplicate policy remain separate focused rows |
| `internal/message/group_e2ee_service.go` `submitPreparedGroupE2EECommit` | retryable/unknown service failure retains local pending commit for retry | `crates/awiki-cli/src/message/group_e2ee_remove.rs` | implemented and now focused-contract tested | `group_e2ee_remove_leave_contract::group_remove_e2ee_retryable_submit_failure_retains_pending_commit_like_go` | same Rust-only selector | low for HTTP 5xx remove path; broader transport/internal variants remain contract candidates |
| `internal/message/group_e2ee_service.go` `finalizePreparedGroupE2EECommit` after accepted submit | service delivery is preserved when local finalize fails, with warning and null finalize data | `crates/awiki-cli/src/message/group_e2ee_remove.rs` | implemented and now focused-contract tested | `group_e2ee_remove_leave_contract::group_remove_e2ee_finalize_failure_keeps_service_delivery_with_warning_like_go` | same Rust-only selector | low for remove path; recover/update finalize warnings remain separate candidates |
| `internal/runtime/listener/server.go` | foreground listener has no group-E2EE notice auto-repair path in Go | no Rust listener group-E2EE path added | intentionally unchanged for 1:1 parity | not applicable | not applicable | low; adding listener group-E2EE auto-repair would be product expansion, not translation |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_send_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli -run 'Test.*(GroupE2EE|E2EE|RecoverMember|Update|Repair|Leave|Remove|Submit|Abort|Finalize|Epoch)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts -ra -q
```

Observed results:

- Rust `group_e2ee_send_contract`: 3 passed, 0 failed.
- Rust `group_e2ee_remove_leave_contract`: 6 passed, 0 failed.
- Go focused `internal/message` and `internal/cli` guard: passed.
- `cargo fmt --check`, `cargo check`, `xtask check-structure`, and
  `git diff --check`: passed.
- New system-test wrapper syntax and whitespace checks: passed.
- Focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  62.85s.
- File-size check after edits:
  `group_e2ee_send_contract.rs` 1112 lines and
  `group_e2ee_remove_leave_contract.rs` 1112 lines. Both remain below the
  active 3000-line test limit; no file-size exception is needed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_stale_retry_and_negative_rust_contracts`.
- This selector runs Rust fake-service Cargo contracts only. It does not start
  live user/message services, does not require mail services, and does not
  count mail selectors.

Coverage boundary:

- This batch promotes deterministic contract evidence for the exact stale send
  retry and remove/leave negative paths listed above.
- The live real-MLS group E2EE selectors remain the separate system evidence
  for success-path send/decrypt, repair, lifecycle, recovery, update, and
  negative service boundaries.
- Recover-member and update-key share related abort/finalize policies but are
  not newly tested for deterministic submit failure/finalize failure in this
  batch.
- Foreground listener group E2EE auto-repair is not added because the Go
  listener has no such path.
- Mail selectors remain deferred.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing `std::process::Command`, `serde_json`,
existing Rustls/std message HTTP transport, existing group E2EE wire builders,
the external local ANP Rust SDK `anp-mls` binary boundary, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, MLS provider crates, or a new SQLite
backend.
