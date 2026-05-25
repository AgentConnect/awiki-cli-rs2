# 2026-05-19 Runtime Listener Service-Managed Restart

Timestamp: 2026-05-19T00:00:00+0800.

Scope: close the Linux portion of the remaining runtime listener
service-managed host-notify restart evidence. This batch proves that a running
real Linux user-systemd listener service is restarted after
`runtime host-notify config set --sink file`, and that status remains running
with `service_platform=linux-systemd`.

This is a Linux user-systemd system-test evidence batch. It does not claim
macOS launchd or Windows Service Manager parity, and it does not run or count
mail selectors.

## Gap Table

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `awiki-cli/internal/cli/runtime.go`, `awiki-cli/internal/runtime/listener/service.go` | `refreshListenerForHostNotifyChange` restarts a running service-managed listener after host-notify config changes; Linux service commands install/start/restart/stop/uninstall via user service manager. | `crates/awiki-cli/src/app/runtime_host_notify_refresh.rs`, `crates/awiki-cli/src/runtime/{listener_systemd,listener_service,mod}.rs` | `system_verified` on Linux user-systemd | `host_runtime_notify_enable_disable_contract`, `host_runtime_listener_service_contract`, `host_runtime_contract` | `tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_change_restarts_service_managed_listener` plus lifecycle/apply selectors | macOS launchd and Windows Service Manager remain deferred platform lanes. |

## Commands

Deterministic pre-edit selectors:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_service_foreground_status_contracts \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_local_rust_contracts \
  -ra -q
```

Observed result: 2 passed in 20.86s, 0 failed, 0 skipped.

Existing gated Linux user-systemd selectors before adding the focused restart
selector:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_listener_lifecycle_commands_work \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_apply_and_listener_config_commands_work \
  -ra -q
```

Observed result: 2 passed in 19.61s, 0 failed, 0 skipped.

New focused selector:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_change_restarts_service_managed_listener \
  -ra -q
```

Observed result: 1 passed in 2.63s, 0 failed, 0 skipped.

Combined gated service-manager batch:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_listener_lifecycle_commands_work \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_apply_and_listener_config_commands_work \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_change_restarts_service_managed_listener \
  -ra -q
```

Observed result: 3 passed in 21.49s, 0 failed, 0 skipped.

## Configuration Context

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1` enabled real listener service-manager
  selectors.
- The system-test helper sets
  `AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE=1` for service-manager
  subprocesses.
- `PYTHONDONTWRITEBYTECODE=1`.
- The real service-manager path reports `service_platform=linux-systemd`.
- Mail selectors were not run and remain deferred.

## Coverage

- Starts from `runtime mode set websocket`, which uses the real listener
  service-manager path through `_run_service`.
- Applies `runtime host-notify config set --sink file` while the service-managed
  listener is running.
- Verifies the Go-shaped warning
  `Listener restarted to apply host notify configuration.`.
- Verifies the listener remains running after restart.
- Verifies listener status reports host-notify sink `file`.
- Verifies listener status reports `service_platform=linux-systemd`.
- Uses `cleanup_listener_service(runtime)` in `finally` to remove the helper
  service state.

## Boundaries

- This proves the Linux user-systemd restart path only. macOS launchd and
  Windows Service Manager remain deferred platform lanes.
- This does not expand mail acceptance. Mail-related system-test selectors
  remain deferred/gated and must not be counted as passed.
- No Rust dependency was added. The implementation continues to use std
  process/filesystem calls around `systemctl --user`; no OpenSSL,
  `native-tls`, platform service crate, dbus crate, alternate SQLite backend,
  or bundled non-SQLite native dependency was introduced.
