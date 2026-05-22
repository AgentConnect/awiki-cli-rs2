# 2026-05-18 Message Group WebSocket/Cache Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the ordinary
  `message/group` WebSocket/local-cache selector visibility gap.
- Three read-only Native Agents mapped Go group behavior, current Rust
  implementation/tests/docs, and non-mail `awiki-system-test` selectors in
  parallel. The scans found no production Rust gap in ordinary group
  WebSocket/cache behavior.
- The actionable gap was public selector visibility: `msg_ws_group_live_contract`
  existed in the Rust repository but was not selected by the system-test Rust
  contract wrapper.
- The only system-test change was a focused Rust-only selector in
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py`. No production Rust
  code, Cargo dependency, ANP SDK, manifest, or mail selector changed.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/message/group_service.go` `sendGroup` | WebSocket runtime uses local bridge first for ordinary group sends, falls back to HTTP on bridge failure, has no cache-only send path, and preserves bridge error when HTTP prep fails | `src/message/group_ws.rs`, `tests/msg_ws_group_live_contract.rs` | implemented; selector visibility added | `msg_ws_group_live_contract` group-send tests | `test_awiki_cli_runtime_listener_message_group_ws_local_cache_contracts` | low |
| `internal/message/group_service.go` `GroupMessages` | WebSocket runtime uses local bridge first for group messages, then local group cache before HTTP, then HTTP fallback with warning, preserving bridge error when HTTP prep fails | `src/message/group_ws.rs`, `tests/msg_ws_group_live_contract.rs` | implemented; selector visibility added | `msg_ws_group_live_contract` group-message tests | same selector | low |
| `internal/message/group_service.go` group control/E2EE boundaries | group lifecycle/control remains HTTP-only in WebSocket mode, and group E2EE send remains HTTP-only rather than adding a `group.e2ee.send` local bridge method | `src/message/group_service.rs`, `src/message/group_e2ee_send.rs` | already covered by adjacent contracts; unchanged | `group_live_contract`, `group_e2ee_send_contract` | separate selectors/docs | low; outside this selector-only batch |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/message ./internal/runtime/listener -run 'TestHandleBridgeRequestPreservesSkipForHistoryAndGroupMessages|TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures|TestWebSocketFallbackWarnings|TestHTTPTransportGroupMethodsUseExpectedRPCMethods' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test msg_ws_group_live_contract --locked
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_message_group_ws_local_cache_contracts -ra -q
```

Observed results:

- Python compile and whitespace checks passed for the modified system-test
  wrapper.
- Focused Go guard passed for `internal/message` and
  `internal/runtime/listener`.
- Rust `msg_ws_group_live_contract` passed 7 tests.
- Focused non-mail system-test selector passed with 1 passed, 0 failed, and 0
  skipped in 35.33s.
- `tests_v2/cli/test_awiki_cli_runtime_listener_local.py` is 1199 lines after
  this selector update, still below the older 1200-line visibility target and
  well below the active 3000-line test-file limit.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
AWIKI_CLI_UPDATE_CACHE_ONLY=1
PYTHONDONTWRITEBYTECODE=1
```

Boundary note: this selector exposes existing ordinary group WebSocket/cache
Rust contracts through `awiki-system-test`. It does not claim new production
behavior, live group service acceptance, mail selectors, group E2EE local
bridge transport, foreground listener group E2EE handling, or broad
repository-wide acceptance.
