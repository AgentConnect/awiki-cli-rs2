# 2026-05-19 Message Secure Rust Selector Batch

Timestamp: 2026-05-19T01:49:34+0800.

Scope: expose deterministic non-mail message secure Rust Cargo contract targets
to the `awiki-system-test` acceptance surface through a new Rust-only selector.
This batch does not change production Rust behavior, does not add dependencies,
and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for message secure selector
  visibility.
- Read-only Native Agents mapped Go direct secure/E2EE behavior, current Rust
  message secure contract targets, and the existing `awiki-system-test`
  selector pattern in parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only
  `tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py` and the
  nearest `tests_v2/cli/CLAUDE.md` member-list entry.
- The leader reviewed the scoped diff, ran direct Rust and focused Go guards,
  ran the new focused system selector, and batched docs/evidence.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/secure.go` | secure direct E2EE client setup, local DID document preference, Go P5 store roots, prekey publish shape, get-prekey retry without OPK, direct init/cipher processing, pending follow-up handling, and history decrypt ordering | `crates/awiki-cli/src/message/secure_client.rs`, `src/message/secure.rs`, ANP SDK adapter paths | implemented and focused-contract tested | `message_secure_client_contract` passed 14 tests | `tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py::test_awiki_cli_message_secure_rust_contracts` | low; pure local/injected RPC target; file is 1198 lines |
| `internal/message/secure_commands.go`, `internal/message/secure_control.go`, `internal/cli/msg.go` | `msg secure status/failed/drop/init/retry/repair` service behavior, active-identity filtering, redacted summaries, retry/drop validation, requeue/mark-sent/drop transitions, and command dry-run boundaries | `crates/awiki-cli/src/message/secure_commands.rs`, `src/app.rs`, `src/store/*` | implemented and focused-contract tested | `message_secure_commands_contract` passed 11 tests | same Rust-only selector | low; pure local target; file is 1150 lines |
| `internal/message/secure_incoming.go`, `internal/runtime/listener/server.go` | direct E2EE wire content types, incoming notification projection, decrypt ordering, hidden secure control/undecryptable rows, direct-init ACK creation, and queued outbox flush side effects | `crates/awiki-cli/src/message/secure_incoming.rs`, `src/runtime/listener/*` | implemented and focused-contract tested | `message_secure_incoming_contract` passed 12 tests | same Rust-only selector | low; pure in-process/injected side-effect target |
| `internal/message/secure_control.go`, `internal/runtime/listener/server.go` | queued secure outbox stable ordering, peer filtering, invalid/unsupported payload handling, send failure retry metadata, sent message storage, ACK/init payload helpers, and current session lookup | `crates/awiki-cli/src/message/secure_control.rs`, `src/store/e2ee_outbox.rs`, `src/runtime/listener/*` | implemented and focused-contract tested | `message_secure_outbox_flush_contract` passed 23 tests | same Rust-only selector | low; local temp workspace and SQLite only |
| `internal/message/secure.go`, `internal/message/secure_control.go` | secure send key-material gate, prekey publish-before-send, pending-confirmation queueing, and successful E2EE outbound persistence | `crates/awiki-cli/src/message/send.rs`, `src/message/secure_control.rs` | implemented and focused-contract tested | `message_secure_send_contract` passed 4 tests | same Rust-only selector | low; pure local/injected sender target |
| `internal/message/secure.go`, `internal/message/secure_incoming.go` | direct reads publish secure prekeys before inbox/history and preserve read success when prekey publish warns | `crates/awiki-cli/src/app.rs`, `src/message/*`, `src/transportcfg/http.rs` | implemented and loopback fake-service tested | `msg_secure_prekey_read_live_contract` passed 2 tests | same Rust-only selector | low; binds `127.0.0.1:0` fake HTTP server, not real external service |
| `internal/message/secure_commands.go`, `internal/message/secure_control.go` | `msg secure repair --with` resets peer state, requeues only matching failed outbox rows, and starts replacement direct init | `crates/awiki-cli/src/message/secure_commands.rs`, `src/app.rs`, `src/store/*` | implemented and loopback fake-service tested | `msg_secure_repair_live_contract` passed 1 test | same Rust-only selector | low; local CLI subprocess plus loopback fake HTTP only |
| `internal/message/secure_commands.go`, `internal/cli/msg.go` | live CLI-local `status`, `failed`, and `drop` routing, local outbox filtering, redaction, and legacy `config.json` migration before status reads | `crates/awiki-cli/src/app.rs`, `src/message/secure_commands.rs`, `src/upgrade/*` | implemented and focused-contract tested | `msg_secure_status_failed_live_contract` passed 4 tests | same Rust-only selector | low; local CLI subprocess, temp workspace, and SQLite only |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --test message_secure_commands_contract --test message_secure_incoming_contract --test message_secure_outbox_flush_contract --test message_secure_send_contract --test msg_secure_prekey_read_live_contract --test msg_secure_repair_live_contract --test msg_secure_status_failed_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/cli ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestPollingInboxDecryptsDirectInitAndSendsSecureAck|TestServiceSecureInitCreatesPendingSession|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation|TestServiceSecureStatusReturnsSessionAndOutboxSummary|TestServiceSecureFailedAndDropOperateOnOutbox|TestServiceSecureRetryMarksQueuedRecordSent|TestServiceSecureRepairResetsFailedOutboxAndStartsNewInit|TestRunMsgSecureRetryAndDropRequireOutboxID|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py::test_awiki_cli_message_secure_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust direct message secure contract targets passed 71 tests total:
  `message_secure_client_contract` 14, `message_secure_commands_contract` 11,
  `message_secure_incoming_contract` 12,
  `message_secure_outbox_flush_contract` 23, `message_secure_send_contract` 4,
  `msg_secure_prekey_read_live_contract` 2,
  `msg_secure_repair_live_contract` 1, and
  `msg_secure_status_failed_live_contract` 4.
- Focused Go message secure guards passed for `internal/message`,
  `internal/cli`, and `internal/runtime/listener`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 7.74s. The selector checks that the eight Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the new system-test wrapper is 86 lines, and
  `tests_v2/cli/CLAUDE.md` is 41 lines. The largest scoped Rust targets are
  `message_secure_client_contract.rs` at 1198 lines and
  `message_secure_commands_contract.rs` at 1150 lines. Both remain below the
  default 1200-line visibility target, but future additions should split rather
  than expand them. `xtask check-structure` reported no undocumented Rust files
  over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_message_secure_rust_contracts.py::test_awiki_cli_message_secure_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. `msg_secure_prekey_read_live_contract` and
  `msg_secure_repair_live_contract` use loopback fake HTTP servers, not
  external services.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for existing
  non-mail message secure Rust contract targets that are deterministic local,
  CLI-local, or loopback fake-service tests.
- It does not claim ordinary direct/group message selector coverage,
  message-service live broad coverage, runtime listener broad coverage, full
  repository-wide acceptance, live mail-service behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
