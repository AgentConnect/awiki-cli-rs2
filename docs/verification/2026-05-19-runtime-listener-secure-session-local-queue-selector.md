# 2026-05-19 Runtime Listener Secure Session Local Queue Selector Batch

Timestamp: 2026-05-19T00:33:46+0800.

Scope: expose existing non-mail Rust runtime/listener secure inbox polling,
secure session helper, and local notification queue contracts to the
`awiki-system-test` acceptance surface. This batch does not change production
Rust behavior, does not add dependencies, and does not run or count mail
selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the runtime/listener
  foreground/session/secure replay/local queue cluster.
- Three read-only Native Agents mapped Go listener/server behavior, current
  Rust runtime/listener implementation/tests, and non-mail system-test selector
  coverage in parallel.
- The scans found no production Rust gap in the selected deterministic
  contract targets. The actionable gap was selector visibility for three
  existing Rust targets that were not exposed by the runtime listener wrapper.
- A bounded GPT-5.5 xhigh Native Agent modified only the system-test runtime
  listener wrapper and nearest `tests_v2/cli/CLAUDE.md` entry. Existing dirty
  helper files in `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/server.go` | secure unread inbox polling runs both unread replay and pending-confirmation history sync before starting the 2s ticker, then repeats both syncs on each tick until context shutdown | `crates/awiki-cli/src/runtime/listener_secure_inbox_poll.rs` | implemented; selector visibility added | `runtime_listener_secure_inbox_poll_contract` passed 4 tests | `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_session_local_queue_contracts` | low for deterministic poll ordering; live reconnect/backlog replay remains separate |
| `internal/runtime/listener/server.go` | pending-confirmation peer discovery reads secure session files under the selected identity, ignores malformed/missing inputs, and returns peer DIDs for pending-confirmation sessions | `crates/awiki-cli/src/runtime/listener_secure_sessions.rs` | implemented; selector visibility added | `runtime_listener_secure_sessions_contract` passed 4 tests | same focused selector | low for local file-scan helper behavior; live secure session mutation remains broader listener work |
| `internal/runtime/listener/server.go` | local notifications for managed-but-not-yet-active recipients are queued by exact DID, skip blank/nil analogs, preserve order, and are deleted on flush | `crates/awiki-cli/src/runtime/listener_local_notifications.rs` | implemented; selector visibility added | `runtime_listener_local_notifications_contract` passed 5 tests | same focused selector | low for queue semantics; end-to-end inactive-recipient secure ACK delivery remains separate |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_inbox_poll_contract --test runtime_listener_secure_sessions_contract --test runtime_listener_local_notifications_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && wc -l crates/awiki-cli/src/runtime/listener_supervisor_run.rs crates/awiki-cli/src/runtime/listener_secure_inbox_poll.rs crates/awiki-cli/src/runtime/listener_secure_sessions.rs crates/awiki-cli/src/runtime/listener_local_notifications.rs crates/awiki-cli/tests/runtime_listener_secure_inbox_poll_contract.rs crates/awiki-cli/tests/runtime_listener_secure_sessions_contract.rs crates/awiki-cli/tests/runtime_listener_local_notifications_contract.rs
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_session_local_queue_contracts -ra -q
```

Observed results:

- Rust direct validation passed 13 tests total: 4 secure inbox poll tests,
  4 secure session helper tests, and 5 local notification queue tests.
- New focused non-mail `awiki-system-test` selector: 1 passed, 0 failed,
  0 skipped in 0.25s.
- System-test wrapper syntax check passed.
- File sizes: `listener_secure_inbox_poll.rs` 60 lines,
  `listener_secure_sessions.rs` 68 lines, `listener_local_notifications.rs`
  42 lines, `runtime_listener_secure_inbox_poll_contract.rs` 49 lines,
  `runtime_listener_secure_sessions_contract.rs` 184 lines, and
  `runtime_listener_local_notifications_contract.rs` 104 lines.
- `listener_supervisor_run.rs` remains 2334 lines. It is above the default
  1200-line review target but below the active 2500-line source
  limit. The file is the Rust integration hub for Go `server.go`, which is
  already 1802 lines; this batch did not grow it. Future runtime/listener work
  should prefer adding or extending small helper modules instead of increasing
  the supervisor file unless integration code is unavoidable.
- `tests_v2/cli/test_awiki_cli_runtime_listener_local.py` is 1322 lines after
  this selector update. It is above the default 1200-line review target but
  below the active 3000-line test-file limit; the exception is localized to an
  existing system-test wrapper that aggregates runtime listener probes and
  Rust-only selector entrypoints.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_session_local_queue_contracts`.
- The selector runs deterministic Rust Cargo contracts only. It does not start
  live mail services and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic secure/session
  and local queue helpers that already matched Go behavior.
- It does not claim new production behavior, full end-to-end secure replay
  delivery through a real WebSocket listener, inactive-recipient secure ACK
  queue flush through live session activation, real service-manager parity,
  Windows named-pipe runtime I/O, OpenClaw/Hermes config work, or mail
  selectors.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing std/Rustls listener code, existing local
ANP/E2EE helper boundaries, and the approved `rusqlite + bundled` SQLite path.
TLS remains Rustls-first; no OpenSSL, `native-tls`, async runtime, or SQLite
backend change was introduced.
