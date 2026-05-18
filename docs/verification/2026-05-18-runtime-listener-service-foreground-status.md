# 2026-05-18 Runtime Listener Service/Foreground/Status-Init Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the runtime/listener
  service, foreground, supervisor-init, and saved-status cluster.
- Three read-only Native Agents mapped Go reference files, existing Rust
  implementation/tests, and non-mail `awiki-system-test` selectors in parallel.
  The leader integrated the selector evidence and documentation.
- The scan found no production Rust gap for this cluster. The remaining gap was
  system-test visibility: `awiki-system-test` did not have a focused selector
  that runs the existing Rust service/foreground/supervisor-init/status
  contracts.
- The only `awiki-system-test` file intentionally changed by this batch was
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py`. Pre-existing dirty
  helper files in that repository were not touched or staged.
- Mail selectors remained deferred and were not run or counted.
- No Cargo dependency, ANP SDK dependency, or Rust production file changed.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/service.go` | service install/start/stop/restart/uninstall plans, wait-for-status, boot-id handoff, artifact cleanup, service-mode detection, service-program start/stop ordering | `src/runtime/listener_service.rs`, `src/runtime/listener_systemd.rs` | implemented; selector added | `runtime_listener_service_contract` | `test_awiki_cli_runtime_listener_service_foreground_status_contracts` | low for deterministic service planner behavior; real service-manager execution remains broader runtime work |
| `internal/runtime/listener/run_foreground_unix.go`, `run_foreground_windows.go`, `sysproc_unix.go`, `sysproc_windows.go` | platform foreground signal and child-process setup intent | `src/runtime/listener_service.rs`, `src/runtime/listener_foreground.rs` | implemented; selector added | `runtime_listener_service_contract`, `runtime_listener_foreground_contract` | same selector | low for helper parity; non-Linux process-control acceptance remains separate |
| `internal/runtime/listener/server.go` | foreground startup ordering, non-websocket rejection before side effects, PID/status write order, socket startup, bridge availability, accept-loop behavior | `src/runtime/listener_foreground.rs` | implemented; selector added | `runtime_listener_foreground_contract` | same selector | low for helper behavior; live WebSocket service health is external |
| `internal/runtime/listener/manager.go`, `files.go`, supervisor initialization paths | store open/schema/boot-id/path/host-notify initialization order, cleanup order, supplied host-notify status preservation, saved listener status merge | `src/runtime/listener_supervisor_init.rs`, `src/runtime/listener.rs` | implemented; selector added | `runtime_listener_supervisor_init_contract`, focused `runtime_contract` saved-status merge test | same selector | low for local deterministic behavior; full live listener acceptance remains separate |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_listener_service_contract --test runtime_listener_foreground_contract --test runtime_listener_supervisor_init_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_contract listener_status_merges_saved_sessions_and_host_notify_state --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener ./internal/runtime -run 'Test.*(Service|Foreground|Status|Supervisor|Boot|Runtime|Listener|Systemd|Signal|Sysproc|Install|Policy|PID|Pid|Bridge|HostNotify)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 python3 -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_service_foreground_status_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && wc -l tests_v2/cli/test_awiki_cli_runtime_listener_local.py && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py
```

Observed results:

- `runtime_listener_service_contract`, `runtime_listener_foreground_contract`,
  and `runtime_listener_supervisor_init_contract` passed 36 tests total.
- The focused saved-status merge contract passed.
- The focused Go runtime/listener guard passed.
- Python compile and whitespace checks for the system-test wrapper passed. The
  wrapper is 962 lines, below the current ordinary file-size target.
- The new focused `awiki-system-test` selector passed with 1 passed in 0.40s.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
PYTHONDONTWRITEBYTECODE=1
```

Boundary note: this selector batch exposes existing deterministic Rust
contracts through the system-test harness. It does not claim live platform
service-manager acceptance, Windows named-pipe I/O, mail selectors, or broad
repository-wide runtime listener acceptance.
