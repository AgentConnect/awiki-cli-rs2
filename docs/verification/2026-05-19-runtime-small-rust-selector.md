# 2026-05-19 Runtime Small Rust Selector Batch

Timestamp: 2026-05-19T02:03:24+0800.

Scope: expose the remaining deterministic non-mail runtime helper Rust Cargo
contract targets to the `awiki-system-test` acceptance surface through the
existing runtime test module. This batch does not change production Rust
behavior, does not add dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for runtime small selector
  visibility.
- Pre-scanned remaining runtime targets, existing runtime Rust-only selector
  helpers, and Go reference tests before editing.
- Reused `tests_v2/runtime/test_runtime_cli.py` because it already owns runtime
  Rust-only selectors and shared Cargo helper functions.
- Added only a small runtime contract target list plus
  `test_runtime_small_rust_contracts`; updated the nearest runtime directory
  docs entry.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/hermesbridge/hermes_config.go`, `internal/cli/runtime.go`, Hermes bridge helpers | Hermes bridge defaults, local notify URL loopback validation, deliver target helpers, home-channel env/display names, `.env` line parsing, route status inspection, fixed target cleanup, legacy notify skill cleanup, and migration predicates | `crates/awiki-cli/src/runtime/hermes_bridge/*`, `src/app/runtime_handlers.rs` | implemented and focused-contract tested | `host_runtime_hermes_bridge_contract` passed 10 tests | `tests_v2/runtime/test_runtime_cli.py::test_runtime_small_rust_contracts` | low; pure local helper target |
| `internal/runtime/listener` JSON marshal/unmarshal helper behavior | `structToMap`-style JSON object projection, marshal failure fallback, non-object fallback, and JSON null preservation as nil-map equivalent | `crates/awiki-cli/src/runtime/listener/*` | implemented and focused-contract tested | `host_runtime_listener_json_helpers_contract` passed 4 tests | same Rust-only selector | low; pure local helper target |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test host_runtime_hermes_bridge_contract --test host_runtime_listener_json_helpers_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/hermesbridge ./internal/runtime/listener ./internal/cli -run 'TestEnsureRouteCreatesWebhookNotifyRouteAndUsesHomeChannel|TestValidateLocalNotifyURLRejectsRemoteHost|TestEnsureRouteKeepsCustomNonNotifySkills|TestHostNotifyConfigViewRedactsHermesSecretValue|TestResolveHermesNotifyURLFallsBackToConfigFileWhenSinkIsNotHermes|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHermesHostNotifySinkNotifySignsRequest|TestNewHermesHostNotifySinkRejectsInvalidNotifyURL|TestRuntimeValidationErrorsUseStableCodes' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/runtime/test_runtime_cli.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/runtime/test_runtime_cli.py tests_v2/runtime/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/runtime/test_runtime_cli.py::test_runtime_small_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust direct runtime small targets passed 14 tests total:
  `host_runtime_hermes_bridge_contract` 10 and
  `host_runtime_listener_json_helpers_contract` 4.
- Focused Go runtime guards passed for `internal/runtime/hermesbridge`,
  `internal/runtime/listener`, and `internal/cli`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 0.23s. The selector checks that the fourteen expected Rust test
  functions exist and runs the two Cargo targets once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the scoped Rust target files are
  `host_runtime_hermes_bridge_contract.rs` 370 lines and
  `host_runtime_listener_json_helpers_contract.rs` 70 lines. The system-test file
  `tests_v2/runtime/test_runtime_cli.py` is a pre-existing runtime aggregation
  file and is now 1630 lines; this batch added 70 lines there to reuse existing
  runtime selector helpers instead of creating another runtime wrapper.
  `xtask check-structure` reported no undocumented Rust files over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/runtime/test_runtime_cli.py::test_runtime_small_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. Both targets are pure local helper contracts.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for the remaining
  non-mail runtime helper Rust contract targets.
- It does not claim broad runtime listener WebSocket/local-cache behavior,
  Hermes service/platform targets already exposed by existing runtime selectors,
  full repository-wide acceptance, live mail-service behavior, or mail
  selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
