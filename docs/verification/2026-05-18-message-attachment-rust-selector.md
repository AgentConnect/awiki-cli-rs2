# 2026-05-18 Message Attachment Rust Selector Batch

Timestamp: 2026-05-18T22:59:18+0800.

Scope: expose existing Rust direct/group attachment fake-service contracts to
the `awiki-system-test` acceptance surface through a non-mail Rust-only
selector. This batch does not change production Rust behavior, does not add
dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the `message` attachment
  HTTP-warning/backfill/wire cluster.
- Three read-only Native Agents mapped Go attachment behavior, current Rust
  implementation/tests, and the non-mail system-test selector surface in
  parallel.
- The scans found no production Rust gap. The actionable gap was selector
  visibility for existing Rust fake-service Cargo contracts.
- The new system-test wrapper verifies each expected Rust contract function is
  present, then runs the two Cargo targets once each to avoid repeated Cargo
  startup cost.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/attachment_service.go`, `internal/message/attachment.go` | direct/group attachment send forces HTTP transport, creates slot, uploads bytes with `PUT`, commits object, then sends the direct/group manifest | `crates/awiki-cli/src/message/attachment_service.rs` | implemented and already focused-contract tested | `attachment_live_contract` passed 4 tests | `tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts` | low after selector exposure |
| `internal/message/attachment_service.go` | group attachment send rejects secure mode, emits the websocket-mode HTTP warning, and backfills visible group message id from `group_did:group_event_seq` | `crates/awiki-cli/src/message/attachment_service.rs` | implemented and already focused-contract tested | `attachment_live_contract::msg_group_attachment_send_live_uploads_commits_and_group_sends_like_go` | same Rust-only selector | low after selector exposure; live forced websocket group-send warning remains local-contract evidence only |
| `internal/message/attachment_wire.go` | create-slot, commit-object, manifest, group `group.send`, target, service DID, and attachment selection/service-discovery wire contracts | `crates/awiki-cli/src/message/attachment.rs`, `crates/awiki-cli/src/message/attachment_service.rs` | implemented and already focused-contract tested | `message_contract` attachment functions | same Rust-only selector | low after selector exposure |
| `internal/cli/msg.go` | attachment dry-run and warning display keep attachment warnings visible rather than filtering them as quiet lifecycle warnings | `crates/awiki-cli/src/app/msg_handlers.rs`, `crates/awiki-cli/src/message/attachment_service.rs` | implemented and already covered by adjacent CLI/message contracts | `attachment_live_contract`, `message_contract` | same Rust-only selector for attachment fake-service coverage | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test attachment_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli -run 'Test.*(Attachment|GroupAttachment|MessageAttachment|Msg)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
```

Observed results:

- Rust `attachment_live_contract`: 4 passed, 0 failed.
- Focused Go `internal/message` and `internal/cli` attachment guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  46.19s. The selector exposes 8 Rust attachment fake-service contract
  functions across two Cargo targets and runs each target once.
- Rust formatting, package check, and structure checks passed.
- File-size check for the relevant Rust files:
  `attachment_service.rs` 884 lines, `attachment.rs` 529 lines,
  `attachment_live_contract.rs` 1074 lines, and `message_contract.rs` 523
  lines. No Rust source or test file grew in this batch, and no file-size
  exception is needed. Future attachment edge tests should avoid growing
  `attachment_live_contract.rs` past the default review target unless the file
  is split or an exception is documented.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_attachment_rust_contracts.py::test_awiki_cli_attachment_rust_contracts`.
- The selector runs deterministic Rust fake-service Cargo contracts only. It
  does not start live mail services and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic local contracts
  that already covered Go direct/group attachment live HTTP, group id backfill,
  attachment wire/manifest construction, attachment selection, service
  discovery, and attachment error mapping.
- It does not claim new production behavior, full repository-wide acceptance,
  live forced websocket group-send warning acceptance, WebSocket/local bridge
  attachment transport, secure direct E2EE, group E2EE/MLS, or mail selectors.
- The existing live selector
  `tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_send_and_download_group_attachments`
  remains the real group attachment send/download acceptance surface, but it
  was not rerun in this selector-visibility batch.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing Rustls/std HTTP transport, authsdk session
handling, attachment wire helpers, local SQLite storage on the approved
`rusqlite + bundled` path, and pure-Rust/cross-platform dependency policy.
TLS remains Rustls-first; no OpenSSL, `native-tls`, async runtime, or SQLite
backend change was introduced.
