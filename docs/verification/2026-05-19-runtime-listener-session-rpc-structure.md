# 2026-05-19 Runtime Listener Session-RPC Structure Batch

## Scope

This accelerated runtime/listener structure batch split the foreground
session-RPC helper lifecycle out of
`crates/awiki-cli/src/runtime/listener_supervisor_run.rs` into
`crates/awiki-cli/src/runtime/listener_session_rpc.rs`.

This is a behavior-preserving structural split. It does not add a new Go
feature claim, dependency, platform service-manager path, Windows live
acceptance, or mail selector acceptance.

## Pipeline

- Two earlier read-only Native Agents mapped Go runtime listener residuals and
  Rust runtime listener file sizes/coverage.
- A bounded code-writing Native Agent with a fixed write scope extracted the
  helper module and moved the focused unit tests.
- A read-only verification Native Agent mapped the smallest regression set for
  the extraction.
- The leader integrated the result, updated batch-level docs, and ran final
  validation.

## Files

- `crates/awiki-cli/src/runtime/listener_session_rpc.rs`: new 318-line module
  containing `SessionRpcSender`, `SessionRpcRegistry`, `SessionRpcRequest`,
  `PendingSessionRpc`, `SessionSharedRpc`, pending/queued failure helpers,
  pending expiration, active-gate closure, and moved focused tests.
- `crates/awiki-cli/src/runtime/listener_supervisor_run.rs`: removed the
  embedded session-RPC helper block and imports the focused module.
- `crates/awiki-cli/src/runtime/mod.rs`: registers `listener_session_rpc`.
- `docs/file-size-exceptions.md`: updates `listener_supervisor_run.rs` from
  2334 to 2030 lines and records the new helper split.
- `docs/parity-matrix.md` and `docs/verification/README.md`: record this batch
  as structural evidence only.

## Commands

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli listener_session_rpc --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_bridge_dispatch_contract --test runtime_listener_wsclient_contract --test runtime_listener_secure_replay_contract --test runtime_listener_notification_consume_contract --test runtime_listener_secure_inbox_poll_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_local_bridge_deterministic_contracts \
  -ra -q
```

## Observed Results

- `cargo +1.79.0 fmt --check` passed.
- `cargo +1.79.0 check -p awiki-cli --locked` passed.
- `cargo +1.79.0 test -p awiki-cli listener_session_rpc --locked` passed 3
  moved session-RPC unit tests.
- Focused Rust regression targets passed 57 tests:
  - `runtime_listener_bridge_dispatch_contract`: 10 passed.
  - `runtime_listener_wsclient_contract`: 31 passed.
  - `runtime_listener_secure_replay_contract`: 5 passed.
  - `runtime_listener_notification_consume_contract`: 7 passed.
  - `runtime_listener_secure_inbox_poll_contract`: 4 passed.
- `cargo +1.79.0 run --bin xtask --locked -- check-structure` passed.
- `git diff --check` passed.
- Focused non-mail `awiki-system-test` selector passed: 1 passed, 0 failed,
  0 skipped.

## File Size

- `listener_session_rpc.rs`: 318 lines, below the default 1200-line target.
- `listener_supervisor_run.rs`: 2030 lines after extraction, still above the
  default 1200-line target but below the ordinary 3000-line relaxed limit.
  The exception remains documented because Go `internal/runtime/listener/server.go`
  is an oversized integrated runtime owner and Rust still keeps the foreground
  execution owner in one traceable file.

## Not Tested

- Full `cargo test -p awiki-cli --locked` was not run; this batch used the
  focused extraction regression set.
- Full `awiki-system-test` was not run; only the non-mail deterministic local
  bridge selector was used as a smoke test.
- Mail selectors remain deferred and are not counted as passed.
- Live Windows named-pipe acceptance and non-Linux service-manager execution
  remain separate batches.

## Dependency Boundary

No dependencies changed. The split keeps the existing std/Rustls listener and
approved `rusqlite + bundled` SQLite path unchanged, and does not introduce
OpenSSL, `native-tls`, a WebSocket crate, an async runtime, a platform
service-manager crate, or a YAML parser.
