# 2026-05-19 Runtime Listener Secure Outbox Local Queue Selector Batch

Timestamp: 2026-05-19T00:00:00+0800.

Scope: accelerate the runtime listener foreground/session-loop parity lane by
adding a smaller non-mail `awiki-system-test` selector for existing Rust secure
outbox trigger and local notification queue/flush targets. This batch does not
change production Rust behavior, does not add dependencies, and does not run or
count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the runtime/listener
  foreground, session-loop, secure outbox trigger, and local notification queue
  cluster.
- Three read-only Native Agents mapped Go listener/server behavior, current
  Rust runtime/listener implementation/tests, and non-mail system-test selector
  coverage in parallel.
- The scans found no production Rust gap in the scoped deterministic helper and
  integration-adjacent cluster. The actionable gap was selector granularity:
  the existing 20-target session/secure-replay/host-notify wrapper covered the
  cluster, but it was too broad for a fast daily focused validation path.
- The implementation edit was limited to
  `/home/ecs-user/awiki-space/awiki-system-test/tests_v2/cli/test_awiki_cli_runtime_listener_local.py`
  and its nearest `tests_v2/cli/CLAUDE.md` entry. Existing dirty helper files
  in `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/server.go` | direct secure notification normalization keeps originals on non-secure/error paths, rewrites decrypted plaintext notifications, converts secure ACKs to `direct.secure.ack`, and schedules secure-init ACK/outbox side effects in Go order | `crates/awiki-cli/src/runtime/listener_secure_normalize.rs`, live wiring in `listener_supervisor_run.rs` | implemented; focused selector visibility added | `runtime_listener_secure_normalize_contract` | `test_awiki_cli_runtime_listener_secure_outbox_local_queue_contracts` | medium; deterministic planning is covered, full live WebSocket delivery remains broader listener acceptance |
| `internal/runtime/listener/server.go` | local secure ACK in-process path handles skip ladder, encrypted ACK shape, recipient fallback decrypt/save ladder, active-session queued outbox flush, managed-inactive local queue, and unmanaged network fallback | `crates/awiki-cli/src/runtime/listener_secure_ack_in_process.rs`, live wiring in `listener_supervisor_run.rs` | implemented; focused selector visibility added | `runtime_listener_secure_ack_in_process_contract` | same focused selector | medium; local planning is covered, real queue-then-later-connect remains live integration risk |
| `internal/runtime/listener/server.go` | peer queued secure outbox trigger scans session snapshot, matches owner DID exactly, stops on first owner without secure RPC, flushes the first owner with secure RPC, and logs warnings | `crates/awiki-cli/src/runtime/listener_secure_outbox_flush.rs`, live flush in `listener_supervisor_run.rs` | implemented; focused selector visibility added | `runtime_listener_secure_outbox_flush_contract` | same focused selector | low for trigger decision; actual SQLite/E2EE send behavior is covered by message secure outbox contracts |
| `internal/runtime/listener/server.go` | local notifications queue by exact recipient DID, skip blank/nil analogs, preserve append order, and delete on flush | `crates/awiki-cli/src/runtime/listener_local_notifications.rs` | implemented; focused selector visibility added | `runtime_listener_local_notifications_contract` | same focused selector | low for queue semantics |
| `internal/runtime/listener/server.go` | queued local notifications drain through the target session callback only when the target/current DID is present and exact-matching | `crates/awiki-cli/src/runtime/listener_local_notification_flush.rs`, live wiring in `listener_supervisor_run.rs` | implemented; focused selector visibility added | `runtime_listener_local_notification_flush_contract` | same focused selector | low for flush adapter semantics; live activation remains broader listener acceptance |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_session_secure_replay_host_notify_contracts tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_session_local_queue_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_outbox_local_queue_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && uv run python -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
```

Observed results:

- Existing runtime listener core non-mail selectors passed: 2 passed, 0 failed,
  0 skipped in 32.43s.
- New focused secure outbox/local queue selector passed: 1 passed, 0 failed,
  0 skipped in 0.50s.
- System-test wrapper syntax check passed through `uv run python -m
  py_compile`.
- `git diff --check` passed in `awiki-system-test`.
- Rust structure check passed: `structure ok: no undocumented Rust files over
  1200 lines`.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_secure_outbox_local_queue_contracts`.
- The selector runs deterministic Rust Cargo contracts only. It does not start
  live mail services and does not count mail selectors.

File-size and dependency notes:

- No Rust source or manifest changed in this batch.
- `listener_supervisor_run.rs` remains the documented runtime listener
  translation-time large integration hub and was not expanded.
- The modified `tests_v2/cli/test_awiki_cli_runtime_listener_local.py` is an
  existing system-test aggregation wrapper. It is above the older 1200-line
  review target but below the active 3000-line test-file limit.
- No dependency was added. The batch reuses existing std/Rustls listener code,
  existing local ANP/E2EE helper boundaries, and the approved `rusqlite +
  bundled` SQLite path. TLS remains Rustls-first; no OpenSSL, `native-tls`,
  async runtime, or SQLite backend change was introduced.

Coverage boundary:

- This batch improves acceptance granularity for already-translated
  deterministic runtime listener secure outbox trigger and local queue/flush
  behavior.
- It does not claim new production behavior, full end-to-end secure replay
  delivery through a real WebSocket listener, inactive-recipient secure ACK
  queue flush through live session activation, real service-manager parity,
  Windows named-pipe runtime I/O, group E2EE foreground listener handling, or
  mail selectors.
