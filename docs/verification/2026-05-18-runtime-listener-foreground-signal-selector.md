# 2026-05-18 Runtime Listener Foreground Signal Selector Batch

Timestamp: 2026-05-18T23:13:39+0800.

Scope: expose the existing Rust Unix foreground listener SIGINT/SIGTERM cleanup
contract to the `awiki-system-test` acceptance surface through a non-mail
Rust-only selector. This batch does not change production Rust behavior, does
not add dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the runtime/listener
  foreground signal cleanup cluster.
- The pre-scan compared Go foreground signal handling with the existing Rust
  `runtime_listener_signal_cli_contract` target and the current system-test
  selector surface.
- The scans found no production Rust gap. The actionable gap was selector
  visibility for the existing Rust foreground signal smoke target.
- The new system-test selector validates the target exists and runs it once.
  The broader deterministic foreground/shutdown plan contracts remain covered
  by existing runtime/listener selectors.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/run_foreground_unix.go`, `manager.go` | foreground listener exits cleanly on `os.Interrupt` or `SIGTERM`, closes supervisor, and removes runtime artifacts | `crates/awiki-cli/src/runtime/listener_supervisor_run.rs`, `crates/awiki-cli/tests/runtime_listener_signal_cli_contract.rs` | implemented and already focused-contract tested on Unix | `runtime_listener_signal_cli_contract` passed 2 tests | `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_foreground_signal_cli_contracts` | low after selector exposure |
| `internal/runtime/listener/run_foreground_windows.go` | Windows foreground listener listens for interrupt only | Rust cfg-specific production code | unchanged; outside current Unix host smoke | not run on this host | none in this batch | residual platform acceptance deferred |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_listener_signal_cli_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener -run 'Test.*Foreground|Test.*Signal|TestStartServiceAutoInstallsWhenMissing' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_foreground_signal_cli_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust `runtime_listener_signal_cli_contract`: 2 passed, 0 failed.
- Focused Go `internal/runtime/listener` foreground/signal guard: passed.
- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  0.45s.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- `tests_v2/cli/test_awiki_cli_runtime_listener_local.py` is 1282 lines after
  this selector update. This stays below the active 3000-line test-file limit;
  it is the existing runtime/listener selector hub and no Rust source/test file
  grew in this batch. No Rust file-size exception is needed.

Coverage boundary:

- This batch promotes system-test visibility for the existing Unix foreground
  signal cleanup smoke target.
- It does not claim Windows signal handling, platform service-manager
  lifecycle, live remote WebSocket session behavior, broad repository-wide
  acceptance, or mail selectors.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing std/Rustls runtime code and does not
change SQLite, TLS, ANP SDK, or platform library choices.
