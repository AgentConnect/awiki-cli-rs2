# 2026-05-18 Runtime Host-Notify/OpenClaw/Hermes Local Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the runtime host-notify,
  OpenClaw, Hermes, config, enable/disable, route, and webhook cluster.
- Three read-only Native Agents mapped Go reference behavior, current Rust
  implementation/tests, and non-mail `awiki-system-test` selector coverage in
  parallel.
- The scan found no production Rust gap for this deterministic cluster. The
  remaining gap was local Rust-only selector visibility: live host-notify probe
  wrappers depend on the configured message-service/WebSocket stack and were
  not appropriate for this batch.
- A bounded GPT-5.5 xhigh Native Agent modified only
  `tests_v2/runtime/test_runtime_cli.py`, adding a focused local Rust-contract
  selector. Pre-existing dirty helper files in `awiki-system-test` were not
  touched or staged.
- Mail selectors remained deferred and were not run or counted.
- No Cargo dependency, ANP SDK dependency, manifest, or Rust production file
  changed.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/host_notify.go` | host event normalization, ID fallbacks, handle merge, file/noop/log/OpenClaw/Hermes sink construction and status | `src/runtime/host_notify.rs`, `host_notify_sink.rs` | implemented; selector added | `host_runtime_notify_sink_contract`; selected non-mail `host_runtime_notify_contract` tests | `test_runtime_host_notify_local_rust_contracts` | low; live mail selectors deferred |
| `internal/runtime/listener/hermes_host_notify.go`, `internal/cli/runtime.go` Hermes paths | Hermes URL validation, secret precedence, HMAC signing, POST error mapping, config writes, route-file setup helpers | `src/runtime/hermes_host_notify.rs`, `src/app/runtime_hermes_handlers.rs`, Hermes route/config helpers | implemented; selector added | `host_runtime_hermes_host_notify_contract`, `host_runtime_hermes_config_write_contract`, `host_runtime_hermes_ensure_route_contract` | same selector | low; real Hermes bridge/service-manager/live delivery remains separate |
| `internal/runtime/listener/openclaw_host_notify.go`, `internal/runtime/openclawnotify/{config,routes,webhook}.go`, `internal/cli/runtime_host_notify_routes.go` | OpenClaw config probe, hook URL validation, route fan-out, failure aggregation, route CLI surface, dry-run validation order | `src/runtime/openclaw_host_notify.rs`, `openclaw_routes.rs`, `openclaw_webhook.rs`, OpenClaw config helpers, `src/app/runtime_handlers.rs` | implemented; selector added | `host_runtime_openclaw_config_contract`, `host_runtime_openclaw_cli_contract`, selected non-mail `host_runtime_openclaw_host_notify_contract` tests, `host_runtime_contract host_notify` | same selector | low; live WebSocket probes remain environment-risky |
| `internal/cli/runtime.go` host-notify enable/disable/config set | enable/disable persistence, sink preservation, dry-run, listener refresh/restart warnings, offline status view | `src/app/runtime_handlers.rs`, `runtime_host_notify_refresh.rs`, `runtime/mod.rs` | implemented; selector added | `host_runtime_notify_enable_disable_contract`, `host_runtime_contract host_notify` | same selector | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_notify_contract --test host_runtime_notify_sink_contract --test host_runtime_hermes_host_notify_contract --test host_runtime_openclaw_host_notify_contract --test host_runtime_openclaw_config_contract --test host_runtime_openclaw_cli_contract --test host_runtime_notify_enable_disable_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_contract host_notify --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_hermes_config_write_contract --test host_runtime_hermes_ensure_route_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener ./internal/runtime/openclawnotify ./internal/cli ./internal/config -run 'Test.*(HostNotify|OpenClaw|Hermes|Webhook)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/runtime/test_runtime_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/runtime/test_runtime_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_local_rust_contracts -ra -q
```

Observed results:

- Host-notify/OpenClaw/Hermes local Rust contract targets passed 50 tests in
  the initial direct validation batch.
- `host_runtime_contract host_notify` passed 10 filtered tests.
- Additional local Hermes config-write and route-file contracts passed 19
  tests before selector integration.
- The focused Go guard passed for listener host-notify, OpenClaw runtime,
  runtime CLI, and config behavior.
- Python compile and whitespace checks for the system-test wrapper passed.
- The new focused `awiki-system-test` selector passed with 1 passed in 1.85s.
- The modified system-test wrapper is 1560 lines. This exceeds the older
  1200-line visibility threshold but remains below the active 3000-line
  test-file limit; it is an existing runtime command wrapper that now
  aggregates local Rust contract selectors as well.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
PYTHONDONTWRITEBYTECODE=1
```

Boundary note: this selector batch exposes already implemented deterministic
Rust contracts through the system-test harness. It does not claim live
message-service WebSocket acceptance, real OpenClaw/Hermes group delivery, real
Hermes bridge service-manager execution, mail selectors, or broad
repository-wide acceptance.
