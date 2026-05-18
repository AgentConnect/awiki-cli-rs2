# 2026-05-19 Foundation Rust Selector Batch

Timestamp: 2026-05-19T01:18:18+0800.

Scope: expose deterministic foundation/local Rust Cargo contract targets to the
`awiki-system-test` acceptance surface through a new Rust-only selector. This
batch does not change production Rust behavior, does not add dependencies, and
does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for foundation/local contract
  visibility instead of one-off selector edits.
- Three read-only Native Agents mapped Go reference packages, current Rust
  contract targets, and `awiki-system-test` selector coverage in parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only
  `tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py` and the nearest
  `tests_v2/cli/CLAUDE.md` member-list entry.
- The leader reviewed the scoped diff, ran direct Rust and focused Go guards,
  ran the new focused system selector, and batched docs/evidence.
- Existing dirty helper files in `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/anpsdk/registry.go` | ANP SDK facade symbols, DID-WBA helpers, proof helpers, direct E2EE file store shapes | `crates/awiki-cli/src/anpsdk.rs` | implemented and focused-contract tested | `anpsdk_contract` passed 16 tests | `tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py::test_awiki_cli_foundation_rust_contracts` | low; SDK drift remains a dependency-version risk |
| `internal/authsdk/session.go` | token capture scope, signed JSON headers, challenge retry, JSON-RPC request/response and JWT fallback behavior | `crates/awiki-cli/src/authsdk/*`, `src/transportcfg/http.rs` | implemented and focused-contract tested | `authsdk_contract` passed 15 tests | same Rust-only selector | low; tests use loopback HTTP only |
| `internal/transportcfg/config.go` | timeout/profile defaults, env override parsing, HTTP client CA/proxy/timeout boundaries | `crates/awiki-cli/src/transportcfg.rs`, `src/transportcfg/http.rs` | implemented and focused-contract tested | `transportcfg_contract` passed 4 tests; `transportcfg_http_contract` passed 10 | same Rust-only selector | low; local timeout tests are timing-sensitive but non-service |
| `internal/cmdmeta/catalog.go`, `internal/cli/root.go`, `internal/output/output.go` | command metadata schema emission, flag field omission, command ordering, core CLI envelopes, status/docs/schema/version/init/config/debug contracts | `crates/awiki-cli/src/cmdmeta/*`, `src/app.rs`, `src/cli/*`, `src/output.rs` | implemented and focused-contract tested | `cmdmeta_schema_contract` passed 2 tests; `core_contract` passed 20 | same Rust-only selector | low; `core_contract.rs` is 1183 lines, near the 1200 visibility target but below it |
| `internal/cli/debug.go`, `internal/doctor/doctor.go`, `internal/traceutil/trace.go`, `internal/content/service.go` | debug handle-history/query/import, doctor local checks and fake `anp-mls` probe, trace formatting/call-site visibility, content/page wire builders and result shapes | `crates/awiki-cli/src/app/debug_handlers.rs`, `src/doctor/*`, `src/traceutil.rs`, `src/content/*` | implemented and focused-contract tested | `debug_contract` passed 5 tests; `doctor_contract` passed 4; `traceutil_contract` passed 9; `content_wire_contract` passed 3 | same Rust-only selector | low; all selector paths are deterministic local contracts |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --test authsdk_contract --test transportcfg_contract --test transportcfg_http_contract --test cmdmeta_schema_contract --test core_contract --test debug_contract --test doctor_contract --test traceutil_contract --test content_wire_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/authsdk ./internal/transportcfg ./internal/cmdmeta ./internal/traceutil ./internal/content ./internal/site ./internal/doctor -count=1
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli -run 'TestRootCommandSuccessContracts|TestRootDocsErrorsUseFrozenExitCodes|TestCommandFromSpecConfiguresFlagsAndAliases|TestNewRootCommandExposesConfigSet|TestNormalizeDebugHandleTrimsPrefixesAndDomains|TestBuildHandleHistoryOwnersAggregatesByOwner|TestPageDryRunPlansRenderStableContracts|TestSiteDryRunPlansRenderStableContracts' -count=1
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message -run 'TestBuildDirectSendRPCParamsUsesOriginProofScheme|TestBuildInboxRPCParamsAppliesDefaultLimitAndMetadata|TestBuildHistoryRPCParamsValidatesTargetAndCursor|TestBuildMarkReadRPCParamsValidatesMessageIDs|TestBuildGroup.*RPCParams|TestHTTPTransport.*' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py::test_awiki_cli_foundation_rust_contracts -ra -q
```

Observed results:

- Rust direct foundation/local contract targets passed 88 tests total:
  `anpsdk_contract` 16, `authsdk_contract` 15, `transportcfg_contract` 4,
  `transportcfg_http_contract` 10, `cmdmeta_schema_contract` 2,
  `core_contract` 20, `debug_contract` 5, `doctor_contract` 4,
  `traceutil_contract` 9, and `content_wire_contract` 3.
- Focused Go foundation guards passed for `internal/authsdk`,
  `internal/transportcfg`, `internal/cmdmeta`, `internal/traceutil`,
  `internal/content`, `internal/site`, `internal/doctor`, selected
  `internal/cli`, and selected `internal/message`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 12.09s. The selector checks that all ten Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the new system-test wrapper is 85 lines, and
  `tests_v2/cli/CLAUDE.md` is 38 lines. The largest scoped Rust target,
  `core_contract.rs`, is 1183 lines, below the default 1200-line visibility
  target. `xtask check-structure` reported no undocumented Rust files over
  1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_foundation_rust_contracts.py::test_awiki_cli_foundation_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. It runs deterministic local Rust Cargo contracts.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for existing
  foundation/local Rust contract targets. It does not claim full repository-wide
  acceptance, live user/message/content/site service behavior, live mail-service
  behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
