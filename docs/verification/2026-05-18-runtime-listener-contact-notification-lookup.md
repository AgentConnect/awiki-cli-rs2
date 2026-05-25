# 2026-05-18 Runtime Listener Contact/Notification/Lookup Selector Batch

Pipeline note:

- Followed the accelerated module-batch pipeline for the runtime/listener
  contact-sync, notification plan/execute/handler, service-DID, and
  session-lookup cluster.
- Three read-only Native Agents mapped Go reference behavior, current Rust
  implementation/tests, and non-mail `awiki-system-test` selector coverage in
  parallel.
- The scan found no production Rust gap for this cluster. The remaining gap was
  selector visibility for existing Rust contracts.
- A bounded GPT-5.5 xhigh Native Agent modified only
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py`, adding a focused
  Rust-contract selector. Pre-existing dirty helper files in
  `awiki-system-test` were not touched or staged.
- Mail selectors remained deferred and were not run or counted.
- No Cargo dependency, ANP SDK dependency, manifest, or Rust production file
  changed.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/contact_sync.go` | incoming direct/group contact sync: trim DIDs, skip blank/self, prefer local handle, optional remote DID-to-handle fallback, Go handle normalization, upsert contact metadata | `src/runtime/listener_contact_sync.rs` | implemented; selector added | 9 inline `runtime::listener_contact_sync` lib tests | `test_awiki_cli_runtime_listener_contact_notification_lookup_contracts` | low |
| `internal/runtime/listener/server.go` notification handling | direct/mail/group/group-state route planning, contact sync requests, handle enrichment, store-before-dispatch ordering, host-notify status update semantics | `src/runtime/listener_notification_plan.rs`, `listener_notification_execute.rs`, `listener_notification_handler.rs` | implemented; selector added | `runtime_listener_notification_plan_contract`, `runtime_listener_notification_execute_contract`, `runtime_listener_notification_handler_contract` | same selector | low; live mail selectors remain deferred |
| `internal/runtime/listener/server.go` service DID lookup | connected session sends Go-shaped `anp.get_capabilities`, decodes string `service_did`, and returns Go-compatible disconnected/missing/non-string errors | `src/runtime/listener_service_did.rs` | implemented; selector added | `host_runtime_listener_service_did_contract` | same selector | low |
| `internal/runtime/listener/server.go` session lookup | active session lookup, record-by-DID lookup, and runtime-session existence fallback preserve blank gates, ordered scan, and first matching load behavior | `src/runtime/listener_session_lookup.rs` | implemented; selector added | `host_runtime_listener_session_lookup_contract` | same selector | low |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli listener_contact_sync --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 test -p awiki-cli --test runtime_listener_notification_plan_contract --test runtime_listener_notification_execute_contract --test runtime_listener_notification_handler_contract --test host_runtime_listener_service_did_contract --test host_runtime_listener_session_lookup_contract --locked
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener ./internal/store -run 'Test(NormalizeHostNotification|HandleNotification|MessageRecord|SessionLoop|UpsertContact|ListDIDsByHandle|EnsureSchema).*|TestMessageQueryHelpersLookupAndMarkReadRespectOwner' -count=1
cd /home/ecs-user/awiki-space/awiki-system-test && python3 -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_contact_notification_lookup_contracts -ra -q
```

Observed results:

- Listener contact-sync lib tests passed 9 tests.
- Notification plan/execute/handler, service-DID, and session-lookup contract
  targets passed 38 tests total.
- Focused Go guard passed for listener notification/contact and store contact
  behavior.
- Python compile and whitespace checks for the system-test wrapper passed.
- The new focused `awiki-system-test` selector passed with 1 passed in 0.95s.
- The modified system-test wrapper is 1079 lines, below the ordinary 1200-line
  target.

System-test configuration context:

```text
AWIKI_CLI_UNDER_TEST=rust
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2
PYTHONDONTWRITEBYTECODE=1
```

Boundary note: this selector batch exposes already implemented deterministic
Rust contracts through the system-test harness. It does not claim new live
message-service WebSocket acceptance, real platform service-manager behavior,
Windows named-pipe I/O, OpenClaw/Hermes group live delivery, mail selectors, or
broad repository-wide acceptance.
