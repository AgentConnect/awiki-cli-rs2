# 2026-05-18 Runtime Local Bridge Deterministic Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the runtime local bridge,
  listener bridge connection/dispatch, listener service-DID, and message
  WebSocket proxy cluster.
- Three read-only Native Agents mapped Go reference behavior, current Rust
  implementation/tests, and non-mail `awiki-system-test` selector coverage in
  parallel.
- The scan found no production Rust gap for Unix/local deterministic bridge
  behavior. The remaining gap was selector visibility for existing Rust
  contracts.
- A bounded GPT-5.5 xhigh Native Agent modified only
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py`, adding a focused
  Rust-only selector. Pre-existing dirty helper files in `awiki-system-test`
  were not touched or staged.
- Mail selectors remained deferred and were not run or counted.
- No Cargo dependency, ANP SDK dependency, manifest, or Rust production file
  changed.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/bridge_unix.go`, `internal/runtime/bridge_windows.go`, `internal/runtime/config.go` | bridge endpoint defaulting, Unix socket normalization/listen/probe/call, Windows named-pipe helper validation, request/response/error JSON, health-probe-before-request flow | `src/runtime/bridge.rs` | implemented for Unix/local deterministic behavior; selector added | `runtime_bridge_contract` | `test_awiki_cli_runtime_listener_local_bridge_deterministic_contracts` | low for Unix/local deterministic behavior; Windows named-pipe runtime I/O remains deferred |
| `internal/runtime/listener/server.go` `handleConn` | one newline-framed local bridge request, Go-shaped bridge error response on read/decode/dispatch failures | `src/runtime/bridge.rs`, `src/runtime/listener_bridge_connection.rs` | implemented; selector added | `runtime_listener_bridge_connection_contract` | same selector | low; live foreground session ownership remains separate evidence |
| `internal/runtime/listener/server.go` `handleBridgeRequest` | direct/inbox/history/mark-read/group local bridge method mapping, Go-like weak JSON boundaries, `ensureSession` and current-session error order, mark-read side effect only after successful RPC | `src/runtime/listener_bridge_dispatch.rs`, `src/runtime/listener_bridge_connection.rs` | implemented; selector added | `runtime_listener_bridge_dispatch_contract` | same selector | low for deterministic dispatch; live group bridge acceptance remains separate |
| `internal/runtime/listener/server.go` service DID lookup | connected session sends `anp.get_capabilities`, decodes string `service_did`, preserves disconnected/missing/non-string error text | `src/runtime/listener_service_did.rs` | implemented; selector added | `runtime_listener_service_did_contract` | same selector | low |
| `internal/message/ws_proxy_client.go` | message WebSocket proxy maps ordinary direct/group bridge calls and wraps unavailable bridge as transport unavailable | `src/message/ws_proxy.rs` | implemented; selector added | `message_ws_proxy_contract` | same selector | low; secure direct has no Go secure-specific local bridge method |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --test runtime_listener_bridge_connection_contract --test runtime_listener_bridge_dispatch_contract --test message_ws_proxy_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --test runtime_listener_bridge_connection_contract --test runtime_listener_bridge_dispatch_contract --test runtime_listener_service_did_contract --test message_ws_proxy_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime ./internal/runtime/listener ./internal/message -run 'Test.*(Bridge|WSProxy|HandleBridge|ListenBridge|CallLocalBridge|ResolveShortens|Endpoint|Pipe|Socket|ServiceDID)' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_local_bridge_deterministic_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py
```

Observed results:

- Initial Rust bridge/message target validation passed 40 tests.
- Full Rust deterministic local bridge validation passed 49 tests:
  `runtime_bridge_contract` 17, `runtime_listener_bridge_connection_contract`
  10, `runtime_listener_bridge_dispatch_contract` 10,
  `runtime_listener_service_did_contract` 9, and
  `message_ws_proxy_contract` 3.
- Focused Go guard passed for `internal/runtime`,
  `internal/runtime/listener`, and `internal/message`.
- Python compile and whitespace checks for the system-test wrapper passed.
- The new focused `awiki-system-test` selector passed with 1 passed, 0 failed,
  and 0 skipped in 0.57s.
- The modified system-test wrapper is 1121 lines, below both the older
  1200-line visibility target and the current ordinary 3000-line limit.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
AWIKI_CLI_UPDATE_CACHE_ONLY=1
PYTHONDONTWRITEBYTECODE=1
```

Boundary note: this selector batch exposes already implemented deterministic
Rust contracts through the system-test harness. It does not claim live
message-service WebSocket acceptance, real foreground group bridge acceptance,
secure-direct local bridge behavior, Windows named-pipe runtime I/O, platform
service-manager behavior, mail selectors, or broad repository-wide acceptance.
