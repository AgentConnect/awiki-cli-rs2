# 2026-05-19 Runtime Hermes Bridge Linux Service

Timestamp: 2026-05-19T03:08:35+0800.

Scope: close the live Linux user-systemd acceptance gap for the Hermes bridge
setup path. This batch proves that `runtime host-notify hermes setup`, when
explicitly gated for Rust Linux user-systemd service management, installs,
starts, and health-probes a real service-managed Hermes bridge and that
follow-up status reports the bridge as installed, running, and available.

This is a Linux user-systemd system-test evidence batch. It does not claim
macOS launchd, Windows Service Manager, real Hermes final delivery, or mail
acceptance.

## Pipeline

- Used the accelerated module-batch workflow rather than one-off diff chasing.
- Reused read-only Native Agent mapping for the Go Hermes bridge service file,
  the existing Rust implementation/tests, and the system-test selector surface.
- Delegated the system-test selector implementation to one GPT-5.5 xhigh
  Native Agent with a fixed write scope:
  `tests_v2/runtime/test_runtime_cli.py` and `tests_v2/runtime/CLAUDE.md`.
- The leader reviewed the diff, fixed the installation-layout precondition,
  ran the gated live selector, checked cleanup, and batched Rust docs.

## Gap Table

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `awiki-cli/internal/runtime/hermesbridge/service.go` | `Apply` installs and starts the bridge service for setup; `StatusFor` reports service install/running state and health only when running. | `crates/awiki-cli/src/runtime/hermes_bridge.rs`, `crates/awiki-cli/src/runtime/hermes_bridge/service.rs` | implemented and health-ready on gated Linux user-systemd; passive `rust-local` boundary when unsupported | `runtime_hermes_bridge_service_contract` | `tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_hermes_setup_starts_service_managed_bridge` | medium; non-Linux service managers remain separate |
| `awiki-cli/internal/cli/runtime.go` | `runtime host-notify hermes setup` writes awiki config, ensures the local Hermes route, refreshes listener config, then applies the bridge service. | `crates/awiki-cli/src/app/runtime_hermes_handlers.rs` | implemented; live Linux setup service apply now system-verified | `runtime_hermes_setup_dry_run_contract hermes_setup_non_dry_run` | same selector | low for Linux setup/install/start; macOS/Windows deferred |
| installed CLI layout | Go/Rust service-run expects the Hermes adapter script next to the installed `awiki-cli` binary. | `scripts/hermes_notify_adapter.py`; `runtime/hermes_bridge.rs` adapter resolver | implemented; system-test now prepares the pytest-managed install-like layout | adapter script copy/script tests and service-run contract | same selector | low; test-only install-layout preparation |

## Commands

Syntax and whitespace checks:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/runtime/test_runtime_cli.py
git diff --check -- tests_v2/runtime/test_runtime_cli.py tests_v2/runtime/CLAUDE.md
```

Observed result: both passed.

Focused gated live selector:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_hermes_setup_starts_service_managed_bridge \
  -ra -q
```

Observed result before health-readiness strengthening: 1 passed in 15.67s, 0
failed, 0 skipped.

Health-ready refresh:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
AWIKI_CLI_UPDATE_CACHE_ONLY=1 \
AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1 \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider \
  tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_hermes_setup_starts_service_managed_bridge \
  -ra -q
```

Observed result after switching the selector to an isolated adapter ingress
port and asserting bridge health: 1 passed in 0.93s, 0 failed, 0 skipped.

Post-run cleanup audit:

```text
cd /home/ecs-user/awiki-space/awiki-system-test
systemctl --user list-units --all 'awiki-cli-hermes-bridge-*.service' --no-legend --no-pager || true
systemctl --user list-unit-files 'awiki-cli-hermes-bridge-*.service' --no-legend --no-pager || true
find "$HOME/.config/systemd/user" -maxdepth 1 -name 'awiki-cli-hermes-bridge-*.service' -print 2>/dev/null || true
```

Observed result: no matching Hermes bridge units or unit files remained.

Rust-side docs/checks after doc updates:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
```

Observed result: see final batch report for pass/fail status.

## Configuration Context

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1` enabled the real user-systemd gate.
- The selector sets `AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE=1` only for
  Hermes bridge service-manager subprocesses.
- The selector uses an isolated `HERMES_HOME` under the pytest runtime root.
- The system-test helper keeps `AWIKI_CLI_WORKSPACE_HOME_DIR` isolated while
  using the real user's `HOME`, `XDG_RUNTIME_DIR`, and
  `DBUS_SESSION_BUS_ADDRESS` for `systemctl --user`.
- The pytest-managed Rust binary is copied to a temporary output directory, so
  the selector prepares an installation-like `scripts/hermes_notify_adapter.py`
  next to that binary before starting the real service.
- The selector uses an isolated loopback adapter ingress URL of the form
  `http://127.0.0.1:<free-port>/notify/host-event`, and asserts the derived
  health URL `http://127.0.0.1:<free-port>/healthz`. This preserves Go's
  separation between awiki adapter ingress and Hermes webhook egress.
- `AWIKI_SYSTEM_TEST_MODE`, user-service URL, message-service URL, WebSocket
  URL, DID domain, and mail-service URL were not used by this selector.

## Coverage

- Runs `runtime host-notify hermes setup` through the real Rust subprocess.
- Uses local notify URL `http://127.0.0.1:<free-port>/notify/host-event`,
  deliver target `telegram`, and an explicit secret.
- Verifies the setup envelope command and summary.
- Verifies host notify is enabled with `sink=hermes`, the configured notify
  URL, deliver target, and redacted secret metadata.
- Verifies the isolated local Hermes route is configured and has a route
  secret.
- Verifies the bridge status in setup reports `installed=true`,
  `running=true`, `bridge_available=true`, the derived `/healthz` URL,
  `service_platform=linux-systemd`, and a deterministic
  `awiki-cli-hermes-bridge-<12 hex>` service name.
- Runs `runtime host-notify hermes status` through the same gated service
  environment and verifies installed/running/bridge-available/Linux-systemd
  status again, including `data.readiness.bridge_available=true`.
- Uses best-effort cleanup in `finally` to stop, disable, reset-failed, remove
  the generated unit, and reload the user systemd daemon.

## Boundaries

- The selector now asserts `bridge_available=true`. The earlier false probe was
  traced to using the Hermes webhook port `8644` as the awiki adapter ingress
  port. Go's default adapter ingress is `8765/notify/host-event` and the
  adapter health endpoint is the same authority plus `/healthz`; `8644` is the
  Hermes webhook egress port (`/webhooks/notify`).
- This does not claim macOS launchd, Windows Service Manager, or full
  `kardianos/service` cross-platform parity.
- This does not claim final Hermes delivery to Telegram/Feishu/DingTalk.
- This does not expand mail acceptance. Mail-related system-test selectors
  remain deferred/gated and must not be counted as passed.

## Dependencies And File Size

- No Rust dependency was added. The implementation continues to use std
  process/filesystem/TCP helpers around `systemctl --user`.
- No OpenSSL, `native-tls`, platform service crate, dbus/systemd binding,
  alternate SQLite backend, or bundled non-SQLite native dependency was
  introduced.
- SQLite remains on the approved `rusqlite + bundled` path.
- TLS remains Rustls-first where TLS is involved; this selector uses only local
  HTTP/systemd paths.
- `tests_v2/runtime/test_runtime_cli.py` is a pre-existing system-test
  aggregation file and remains below the active 3000-line test-file policy. No Rust
  source file changed in this batch.
