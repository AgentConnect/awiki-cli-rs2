# 2026-05-18 Message Group E2EE Status/Pending/Publish/Wire Selector Batch

Timestamp: 2026-05-18T22:32:28+0800.

Scope: expose existing Rust fake-service contracts for Go group E2EE status,
pending notice pull, KeyPackage publish, and hidden wire selector behavior to
the `awiki-system-test` acceptance surface. This batch does not change
production Rust behavior, does not add dependencies, and does not run or count
mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the
  `message/group` E2EE status/pending/publish/wire cluster.
- A read-only Native Agent mapped Go `group_e2ee.go`,
  `group_e2ee_service.go`, `group_wire.go`, and adjacent Go tests into a
  behavior matrix. Additional Native Agents could not be started because the
  session had already reached its agent limit, so the leader performed the
  Rust and system-test mapping locally.
- The gap matrix showed existing Rust production and contract coverage. The
  remaining gap was system-test selector visibility for the existing focused
  Cargo contract targets.
- The system-test wrapper was extended with a separate selector that verifies
  expected Rust test functions exist, then runs each target once to reduce
  repeated Cargo startup cost.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/group_e2ee.go`, `internal/message/group_e2ee_service.go` | `group e2ee status` inspects local MLS status, service head, pending notices, and recovery diagnosis while keeping discovery hidden | `crates/awiki-cli/src/message/group_e2ee_status.rs`, `crates/awiki-cli/src/app/group_e2ee_handlers.rs` | implemented and already focused-contract tested | `group_e2ee_status_contract` passed 2 tests | `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_status_pending_publish_rust_contracts` | low after selector exposure; live real-service status remains covered by separate group E2EE selectors |
| `internal/cli/group_e2ee.go`, `internal/message/group_e2ee_service.go` | `group e2ee pending` pulls hidden `group.e2ee.notice` data with `mark_delivered=false`, optional trimmed group filter, and Go-shaped summary/data fields | `crates/awiki-cli/src/message/group_e2ee_pending.rs`, `crates/awiki-cli/src/app/group_e2ee_handlers.rs` | implemented and already focused-contract tested | `group_e2ee_pending_contract` passed 2 tests | same Rust-only selector | low after selector exposure |
| `internal/cli/group_e2ee.go`, `internal/message/group_e2ee_service.go`, `internal/message/group_wire.go` | `publish-key-package` normalizes purpose/device, requires group for recovery/update packages, signs DID-WBA binding, strips private provider fields, and publishes through service target | `crates/awiki-cli/src/message/group_e2ee_publish.rs`, `group_e2ee_provider.rs`, `group_e2ee_wire.rs` | implemented and already focused-contract tested | `group_e2ee_publish_contract` passed 4 tests | same Rust-only selector | low after selector exposure; real service publish success remains separate live evidence |
| `internal/message/group_wire.go`, `internal/message/http_client.go` | hidden wire selectors preserve Go targets and security profiles: send to group with `group-e2ee`, notice to active agent with transport protection, head to group, and key-package publish/get to service | `crates/awiki-cli/src/message/group_e2ee_wire.rs`, `group_e2ee_transport.rs` | implemented and already focused-contract tested | `message_group_e2ee_wire_contract` passed 7 tests | same Rust-only selector | low after selector exposure |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli -run 'Test.*(GroupE2EE|Status|Pending|Publish|KeyPackage|Notice|Head|DryRun|RPC)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_status_pending_publish_rust_contracts -ra -q
```

Observed results:

- Rust `group_e2ee_status_contract`: 2 passed, 0 failed.
- Rust `group_e2ee_pending_contract`: 2 passed, 0 failed.
- Rust `group_e2ee_publish_contract`: 4 passed, 0 failed.
- Rust `message_group_e2ee_wire_contract`: 7 passed, 0 failed.
- Focused Go `internal/message` and `internal/cli` guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  47.27s. The selector exposes 15 Rust fake-service group E2EE contracts
  across four Cargo targets.
- File-size check after the batch:
  `group_e2ee_status_contract.rs` 507 lines,
  `group_e2ee_pending_contract.rs` 404 lines,
  `group_e2ee_publish_contract.rs` 676 lines, and
  `message_group_e2ee_wire_contract.rs` 556 lines. No Rust source or test file
  grew in this batch, and no file-size exception is needed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_group_e2ee_rust_contracts.py::test_awiki_cli_group_e2ee_status_pending_publish_rust_contracts`.
- The selector runs deterministic Rust fake-service Cargo contracts only. It
  does not start live user/message services, does not require mail services,
  and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic local contracts
  that already covered the Go status, pending, publish, and wire selector
  surfaces.
- It does not claim new production behavior, live real-service forced failure
  coverage, WebSocket/local bridge group E2EE transport, foreground listener
  group E2EE handling, or mail selectors.
- Status/pending/publish live success paths remain covered by the separate
  focused group E2EE local selectors and earlier local Rust contract slices.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing `std::process::Command`, `serde_json`,
existing Rustls/std message HTTP transport, existing group E2EE wire builders,
the external local ANP Rust SDK `anp-mls` binary boundary, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, WebSocket crates, async runtimes, MLS provider crates, or a
new SQLite backend. TLS remains Rustls-first.
