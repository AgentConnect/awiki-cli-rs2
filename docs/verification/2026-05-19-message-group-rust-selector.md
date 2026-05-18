# 2026-05-19 Message Group Rust Selector Batch

Timestamp: 2026-05-19T01:58:23+0800.

Scope: expose deterministic non-mail ordinary message/group Rust Cargo contract
targets to the `awiki-system-test` acceptance surface through a new Rust-only
selector. This batch does not change production Rust behavior, does not add
dependencies, and does not run or count mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for ordinary message/group
  selector visibility.
- Read-only Native Agents mapped Go ordinary `msg`/`group` behavior, current
  Rust target coverage, and the existing `awiki-system-test` selector pattern
  in parallel.
- A bounded GPT-5.5 xhigh Native Agent wrote only
  `tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py` and the nearest
  `tests_v2/cli/CLAUDE.md` member-list entry.
- The leader reviewed the scoped diff, ran direct Rust and focused Go guards,
  ran the new focused system selector, and batched docs/evidence.
- Existing dirty helper files in `awiki-system-test` were not touched or
  staged.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/cli/msg.go`, `internal/message/wire.go` | `msg` schema, dry-run plans, direct/group target validation, text/text-file and attachment argument boundaries, required-flag errors, inbox/history/mark-read local profile defaults, limits, cursor, and message ID validation | `crates/awiki-cli/src/app.rs`, `src/message/wire.rs` | implemented and focused-contract tested | `msg_contract` passed 6 tests | `tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py::test_awiki_cli_message_group_rust_contracts` | low; CLI-local only |
| `internal/message/service.go`, `internal/message/wire.go`, `internal/message/secure.go` | direct send fake-service RPC, outbound row persistence, secure-on direct send, secure init/retry live command paths, inbox/history/mark-read output shape, handle history merge, and secure wire-row filtering from handle history | `crates/awiki-cli/src/app.rs`, `src/message/*`, `src/store/*`, `src/transportcfg/http.rs` | implemented and loopback fake-service tested | `msg_live_contract` passed 7 tests | same Rust-only selector | moderate; local CLI subprocess plus `127.0.0.1:0` fake HTTP only; file is exactly 1200 lines |
| `internal/message/service.go`, `internal/message/warnings.go`, `internal/message/ws_proxy_client.go`, `internal/traceutil/trace.go` | HTTP 401 JWT refresh and fallback trace behavior, persisted refreshed bearer, websocket-to-HTTP fallback warning readability, and trace timing visibility | `crates/awiki-cli/src/authsdk/*`, `src/message/*`, `src/traceutil.rs`, `src/transportcfg/http.rs` | implemented and loopback fake-service tested | `msg_jwt_fallback_trace_contract` passed 2 tests | same Rust-only selector | moderate; local fake HTTP plus trace stderr assertions, no external service |
| `internal/message/group_wire.go`, `internal/message/proof.go`, `internal/cli/group.go` | group create/join/add/remove/leave/update/send/get/list/members/messages JSON-RPC params, origin proof, base vs local profiles, default limits, group policy, and validation errors | `crates/awiki-cli/src/message/group_wire.rs`, `src/app/group_handlers.rs` | implemented and focused-contract tested | `message_group_wire_contract` passed 6 tests | same Rust-only selector | low; pure local wire/proof/validation target |
| `internal/cli/group.go`, `internal/cli/msg.go`, `internal/message/group_service.go`, `internal/message/service.go` | loopback group get/members/group-send flows, group control remaining HTTP in websocket mode, owner hint preservation on group add error, accepted delivery mapping, operation IDs, message ID suffix, and group event seq | `crates/awiki-cli/src/app/group_handlers.rs`, `src/message/group_service.rs`, `src/message/group_wire.rs`, `src/transportcfg/http.rs` | implemented and loopback fake-service tested | `group_live_contract` passed 5 tests | same Rust-only selector | moderate; local CLI subprocess plus `127.0.0.1:0` fake HTTP only |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test msg_contract --test msg_live_contract --test msg_jwt_fallback_trace_contract --test message_group_wire_contract --test group_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/cli ./internal/message -run 'TestMsgDryRunPlansRenderStableContracts|TestRunMsgSendRejectsInvalidFlagCombinationsBeforeService|TestGroupDryRunPlansRenderStableContracts|TestBuildDirectSendRPCParamsUsesOriginProofScheme|TestBuildInboxRPCParamsAppliesDefaultLimitAndMetadata|TestBuildHistoryRPCParamsValidatesTargetAndCursor|TestBuildMarkReadRPCParamsValidatesMessageIDs|TestBuildInboxRPCParamsUsesDefaultLimit|TestBuildHistoryRPCParamsValidatesTargetAndOptionalCursor|TestBuildMarkReadRPCParamsRequiresMessageIDs|TestBuildGroupCreateRPCParamsUsesOriginProofAndServiceTarget|TestBuildGroupCreateRPCParamsAppliesDefaultPolicyContract|TestBuildGroupMessagesRPCParamsUsesLocalProfile|TestBuildGroupMembersRPCParamsDefaultsLimitToHundred|TestBuildGroupListRPCParamsDefaultsLimitToFifty|TestBuildGroupMessagesRPCParamsDefaultsLimitToFifty|TestTransportSourceMatchesActualMode|TestHTTPTransportPersistsAuthenticationInfoTokenFromFirstSignedRequest|TestHTTPTransportRefreshesExpiredBearerAfterHTTP401|TestSyncPeerHandleRebindsCurrentContactAndPreservesHistory|TestReadHistoryFromCacheByPeerDIDsAggregatesHistoricalBindings|TestWebsocketFallbackWarningsUseReadableTransportDetails|TestWSProxyTransportWrapsBridgeFailures|TestHTTPTransportGroupMethodsUseExpectedRPCMethods' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py::test_awiki_cli_message_group_rust_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- Rust direct ordinary message/group contract targets passed 26 tests total:
  `msg_contract` 6, `msg_live_contract` 7,
  `msg_jwt_fallback_trace_contract` 2, `message_group_wire_contract` 6, and
  `group_live_contract` 5.
- Focused Go ordinary message/group guards passed for `internal/cli` and
  `internal/message`.
- System-test wrapper syntax and whitespace checks passed.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 42.74s. The selector checks that the five Rust Cargo targets exist
  and runs each target once.
- Rust formatting, package check, structure check, and whitespace check passed.
- File-size evidence: the new system-test wrapper is 82 lines, and
  `tests_v2/cli/CLAUDE.md` is 42 lines. Scoped Rust targets are
  `msg_contract.rs` 679 lines, `message_group_wire_contract.rs` 410,
  `msg_jwt_fallback_trace_contract.rs` 546, `group_live_contract.rs` 707, and
  `msg_live_contract.rs` exactly 1200. Future additions to
  `msg_live_contract.rs` should split the target or document an exception first.
  `xtask check-structure` reported no undocumented Rust files over 1200 lines.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_message_group_rust_contracts.py::test_awiki_cli_message_group_rust_contracts`.
- Service URLs, WebSocket URL, DID domain, and mail service endpoints were not
  used by this selector. `msg_live_contract`,
  `msg_jwt_fallback_trace_contract`, and `group_live_contract` use loopback
  fake HTTP servers, not external services.
- Failures: 0. Skips: 0.

Coverage boundary:

- This batch fills `awiki-system-test` selector visibility for existing
  non-mail ordinary message/group Rust contract targets that are CLI-local,
  pure-local wire, or loopback fake-service tests.
- It does not claim runtime listener WebSocket/local-cache selector coverage,
  group E2EE selector coverage, all-inbox mail-like cache behavior, full
  repository-wide acceptance, live mail-service behavior, or mail selectors.
- It does not add new production parity behavior; the scoped Rust contracts were
  already implemented and passed direct Cargo verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. SQLite remains on the approved `rusqlite + bundled` path for runtime
portability. TLS remains Rustls-first; no OpenSSL, `native-tls`, bundled
OpenSSL, ANP SDK, platform service, or SQLite backend change was introduced.
