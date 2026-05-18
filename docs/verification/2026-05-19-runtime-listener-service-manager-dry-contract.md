# 2026-05-19 Runtime Listener Service-Manager Dry-Contract Batch

## Scope

This accelerated runtime/listener batch covers the platform service-manager
contract boundary around Go `internal/runtime/listener/service.go`.

Implemented in this batch:

- A pure Rust macOS launchd dry-contract module for listener service label,
  LaunchAgent path, plist content, environment, logs, RunAtLoad/KeepAlive, and
  launchctl-style status parsing.
- A pure Rust Windows Service dry-contract module for listener service
  name/display/config intent, empty Windows working directory, automatic start,
  restart-on-failure intent, environment, logs/PID projection, and service-state
  parsing.
- Go-equivalent fallback `runtime listener restart` behavior: when the local
  fallback service is not installed, restart now fails with
  `listener service is not installed` instead of flowing through `start`.

Not implemented or claimed in this batch:

- Live macOS `launchctl` install/start/stop/restart/uninstall/status.
- Live Windows SCM install/start/stop/restart/uninstall/status.
- Any new platform service-manager dependency or production command routing.
- Mail selector acceptance.

## Pipeline

- The leader selected this as a module-batch lane after the previous runtime
  listener structural split.
- Two read-only Native Agents mapped Go runtime listener service-manager
  behavior, Rust status, system-test selectors, and line-count risk.
- Two bounded GPT-5.5 xhigh Native Agents implemented independent write scopes:
  `listener_launchd.rs` plus its test, and `listener_windows_service.rs` plus
  its test.
- The leader fixed the non-systemd fallback restart parity, integrated the
  parallel work, ran layered validation, and updated batch-level docs.

## Gap Table

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/service.go` | `RestartService` checks installed state and returns `listener service is not installed` when missing; it does not auto-install | `crates/awiki-cli/src/runtime/mod.rs` | implemented for non-systemd local fallback | `runtime_contract::listener_restart_requires_installed_service_like_go_contract` | `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_service_foreground_status_contracts` | low for fallback semantics; live service managers remain separate |
| `internal/runtime/listener/service.go` launchd backend via `kardianos/service` | per-workspace launch agent identity, `runtime listener service-run`, env vars, RunAtLoad, KeepAlive, working directory, logs, status parsing | `crates/awiki-cli/src/runtime/listener_launchd.rs` | dry-contract implemented | `runtime_listener_launchd_contract` | same non-mail listener selector for adjacent entrypoint health | medium until live macOS launchctl acceptance |
| `internal/runtime/listener/service.go` Windows backend via `kardianos/service` | per-workspace Windows service identity, empty working directory, service-run args, env vars, automatic start, restart-on-failure, logs/PID projection, state mapping | `crates/awiki-cli/src/runtime/listener_windows_service.rs` | dry-contract implemented | `runtime_listener_windows_service_contract` | same non-mail listener selector for adjacent entrypoint health | medium until live Windows SCM acceptance |

## Files

- `crates/awiki-cli/src/runtime/listener_launchd.rs`
- `crates/awiki-cli/src/runtime/listener_windows_service.rs`
- `crates/awiki-cli/src/runtime/mod.rs`
- `crates/awiki-cli/tests/runtime_listener_launchd_contract.rs`
- `crates/awiki-cli/tests/runtime_listener_windows_service_contract.rs`
- `crates/awiki-cli/tests/runtime_contract.rs`
- `docs/parity-matrix.md`
- `docs/verification/README.md`
- `docs/verification/2026-05-19-runtime-listener-service-manager-dry-contract.md`

## Commands

```bash
cargo +1.79.0 test -p awiki-cli --test runtime_contract listener_restart_requires_installed_service_like_go_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_launchd_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_windows_service_contract --locked
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_launchd_contract --test runtime_listener_windows_service_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_service_foreground_status_contracts \
  -ra -q
```

## Observed Results

- `runtime_contract::listener_restart_requires_installed_service_like_go_contract`
  passed.
- `runtime_listener_launchd_contract` passed 3 tests.
- `runtime_listener_windows_service_contract` passed 3 tests.
- `cargo +1.79.0 fmt --check` passed after formatting.
- `cargo +1.79.0 check -p awiki-cli --locked` passed.
- `cargo +1.79.0 run --bin xtask --locked -- check-structure` passed:
  no undocumented Rust files over 1200 lines.
- `git diff --check` passed.
- Focused non-mail `awiki-system-test` selector passed: 1 passed in 6.06s.

## File Size

- `listener_launchd.rs`: 281 lines.
- `listener_windows_service.rs`: 187 lines.
- `runtime_listener_launchd_contract.rs`: 225 lines.
- `runtime_listener_windows_service_contract.rs`: 253 lines.
- `runtime/mod.rs`: 604 lines.
- `runtime_contract.rs`: 1187 lines after formatting.

No new file-size exception is needed.

## Dependency Note

No Rust dependency was added. Cargo manifests and `Cargo.lock` remain unchanged.
The launchd and Windows service surfaces are pure dry-contract helpers and do
not introduce a service-manager crate, OpenSSL, `native-tls`, bundled OpenSSL,
alternate SQLite backend, WebSocket crate, YAML crate, systemd/dbus crate, or
platform library dependency.

SQLite remains on the approved `rusqlite + bundled` path. TLS remains
Rustls-first.

## Boundary

This batch reduces the service-manager parity gap by making the macOS and
Windows platform contracts explicit and tested, and by fixing a local fallback
restart semantic that differed from Go. It does not claim full platform service
acceptance. Future live batches still need actual macOS `launchctl` and Windows
SCM install/start/status/restart/uninstall verification, ideally using
platform-specific CI or host runners.
