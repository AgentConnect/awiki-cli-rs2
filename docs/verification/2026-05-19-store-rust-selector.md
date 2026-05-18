# 2026-05-19 Store Rust Selector Batch

Timestamp: 2026-05-19T01:03:04+0800.

Scope: close the `store/*` batch for local SQLite cache parity by adding
focused Rust message-store contract coverage and exposing all non-mail store
Cargo contract targets to the `awiki-system-test` acceptance surface through a
Rust-only selector. This batch does not change production Rust behavior, does
not add dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the Go `internal/store`
  package.
- Three read-only Native Agents mapped Go store behavior, current Rust
  implementation/test coverage, and `awiki-system-test` selector coverage in
  parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only the system-test selector
  wrapper and nearest CLI docs entry while the leader added the Rust
  `store_messages_contract` target and owned integration, validation, docs, and
  commits.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.
- Mail selectors remained deferred and were not run or counted. Mail-like
  notification rows are covered here only as local SQLite cache rows because
  Go store query predicates include them.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/store/dao.go`, `dao_test.go` | store messages, update thread view, replace raw secure wire with decrypted content, preserve decrypted content when raw wire arrives later, preserve read/E2EE flags and latest server sequence | `crates/awiki-cli/src/store/messages.rs` | implemented; direct store contract coverage added | `store_messages_contract` passed 3 tests | `tests_v2/cli/test_awiki_cli_store_rust_contracts.py::test_awiki_cli_store_rust_contracts` | low after focused coverage |
| `internal/store/query.go`, `query_test.go` | owner-scoped message lookup, mark-read row count, thread lookup blank validation, local inbox direct/group/mail-like filters, notification inbox predicates, and notification ordering | `crates/awiki-cli/src/store/messages.rs`, `src/store/query.rs` | implemented; direct store contract coverage added | `store_messages_contract` passed 3 tests | same Rust-only selector | low; mail service acceptance remains deferred |
| `internal/store/dao.go`, `query.go`, contact tests | contacts, current handle rebind, handle-history fallback, direct-message peer lookup dedupe/filtering | `crates/awiki-cli/src/store/contacts.rs`, `src/store/messages.rs` | implemented and already focused-contract tested | `store_contact_contract` passed 3 tests | same Rust-only selector | low |
| `internal/store/dao.go` | E2EE outbox queue/list/get/sent/failed/status/failure fallback paths | `crates/awiki-cli/src/store/e2ee_outbox.rs` | implemented and already focused-contract tested | `store_e2ee_outbox_contract` passed 6 tests | same Rust-only selector | low |
| `internal/store/query.go`, group cache helpers | group snapshot/member/message cache query, touch, leave projection, and required-key validation | `crates/awiki-cli/src/store/groups.rs` | implemented and already focused-contract tested | `store_groups_contract` passed 2 tests | same Rust-only selector | low |
| `internal/store/helpers.go` | thread-id construction and RFC3339 UTC timestamp shape | `crates/awiki-cli/src/store/helpers.rs` | implemented and already focused-contract tested | `store_helpers_contract` passed 2 tests | same Rust-only selector | low |
| `internal/store/import.go` | legacy SQLite v11 import, missing-table tolerance, owner inference, and pre-v6 rejection | `crates/awiki-cli/src/store/import.rs` | implemented and already focused-contract tested | `store_import_contract` passed 3 tests | same Rust-only selector | low |
| `internal/store/rebind.go`, `recover_merge.go` | local identity state rebind, owner-DID rebind counts, E2EE cleanup, recovered-handle merge algebra, current handle selection, and relationship-event merge identity | `crates/awiki-cli/src/store/rebind.rs`, `src/store/recover_merge.rs`, `src/store/recover_merge/*` | implemented and already focused-contract tested | `store_rebind_contract` passed 5 tests; `store_recover_merge_contract` passed 6 tests | same Rust-only selector | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test store_contact_contract --test store_e2ee_outbox_contract --test store_groups_contract --test store_helpers_contract --test store_import_contract --test store_messages_contract --test store_rebind_contract --test store_recover_merge_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/store -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_store_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_store_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_store_rust_contracts.py::test_awiki_cli_store_rust_contracts -ra -q
cd /home/ecs-user/awiki-space && df -h /home/ecs-user/awiki-space
```

Observed results:

- Rust direct store contract targets passed 30 tests total:
  `store_contact_contract` 3, `store_e2ee_outbox_contract` 6,
  `store_groups_contract` 2, `store_helpers_contract` 2,
  `store_import_contract` 3, `store_messages_contract` 3,
  `store_rebind_contract` 5, and `store_recover_merge_contract` 6.
- Focused Go `internal/store` guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  1.23s. The selector verifies expected Rust contract function names and runs
  all eight store Cargo targets once each.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- File-size evidence: the new `store_messages_contract.rs` is 385 lines.
  Existing near-threshold store files remain under the default review target:
  `src/store/import.rs` is 1117 lines and `store_recover_merge_contract.rs` is
  1009 lines. `xtask check-structure` reported no undocumented Rust files over
  1200 lines.
- Disk evidence: `/home/ecs-user/awiki-space` had 26G available, 73% used.
  No cleanup was required during this batch.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_store_rust_contracts.py::test_awiki_cli_store_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. It runs deterministic local Rust Cargo contracts.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch adds direct store-level evidence for message/cache query parity
  that was previously covered mostly through higher-level message/runtime
  contracts.
- It does not claim full repository-wide acceptance, live message-service
  behavior, live mail-service behavior, mail selectors, or a new SQLite backend.
- Mail-like notification rows are included only to preserve Go store predicate
  behavior for local cached rows; mail-focused selectors remain deferred.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for
runtime portability. TLS remains Rustls-first; no OpenSSL, `native-tls`,
bundled OpenSSL, ANP SDK, platform service, or SQLite backend change was
introduced.
