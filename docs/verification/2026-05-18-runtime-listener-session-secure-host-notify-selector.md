# 2026-05-18 Runtime Listener Session/Secure/Host-Notify Selector Batch

Timestamp: 2026-05-18T23:09:02+0800.

Scope: expose existing Rust runtime/listener session bootstrap, known-session,
identity-watch, secure replay/sync/ack/outbox, local notification queue, and
host-notify fake-service/local contracts to the `awiki-system-test` acceptance
surface through a non-mail Rust-only selector. This batch does not change
production Rust behavior, does not add dependencies, and does not run or count
mail selectors.

Pipeline:

- Followed the accelerated module-batch pipeline for the runtime/listener
  foreground/session/secure replay/host-notify cluster.
- Three read-only Native Agents mapped Go runtime/listener behavior, current
  Rust implementation/tests, and the non-mail system-test selector surface in
  parallel.
- The scans found no production Rust gap in the selected deterministic
  contract targets. The actionable gap was selector visibility for grouped
  existing Rust targets.
- The new system-test selector validates each expected Rust target file exists
  and then runs each target once. It deliberately excludes the process-signal
  foreground smoke target so signal handling can remain a separate live/smoke
  batch.
- Mail selectors remained deferred and were not run or counted.

Gap table:

| Go file | Go behavior | Rust file | Rust status | Rust test | System selector | Risk |
| --- | --- | --- | --- | --- | --- | --- |
| `internal/runtime/listener/server.go`, `service.go`, `manager.go` | session bootstrap, known-session startup, identity watch, connect-session planning, bridge-created session runtime, and supervisor shutdown | `crates/awiki-cli/src/runtime/listener_*` modules | implemented and already focused-contract tested | `host_runtime_listener_session_bootstrap_contract`, `host_runtime_listener_known_sessions_contract`, `host_runtime_listener_identity_watch_contract`, `host_runtime_listener_connect_session_contract`, `host_runtime_listener_session_methods_contract`, `runtime_listener_bridge_host_runtime_contract`, `host_runtime_listener_supervisor_shutdown_contract` | `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_session_secure_replay_host_notify_contracts` | low after selector exposure |
| `internal/runtime/listener/server.go` secure replay/ack paths | unread secure inbox replay, pending-confirmation history replay, secure notification reconstruction, secure normalization, local/network ack delivery, queued outbox flush, and delayed local notification queue | `crates/awiki-cli/src/runtime/listener_secure_*`, `listener_local_*` modules | implemented and already focused-contract tested | `runtime_listener_secure_replay_contract`, `runtime_listener_secure_sync_contract`, `runtime_listener_secure_notifications_contract`, `runtime_listener_secure_normalize_contract`, `runtime_listener_secure_ack_delivery_contract`, `runtime_listener_secure_ack_in_process_contract`, `runtime_listener_secure_outbox_flush_contract`, `host_runtime_listener_local_notification_flush_contract` | same Rust-only selector | low after selector exposure; live end-to-end replay delivery remains separate |
| `internal/runtime/listener/host_notify.go`, `hermes_host_notify.go`, `openclaw_host_notify.go`; `internal/runtime/openclawnotify/routes.go` | host event normalization, sink construction, file/Hermes/OpenClaw delivery contracts, route handling, and config-change listener refresh | `crates/awiki-cli/src/runtime/{host_notify,host_notify_sink,hermes_host_notify,openclaw_host_notify}.rs`, `crates/awiki-cli/src/app/runtime_host_notify_refresh.rs` | implemented and already focused-contract tested | `host_runtime_notify_contract`, `host_runtime_notify_sink_contract`, `host_runtime_hermes_host_notify_contract`, `host_runtime_openclaw_host_notify_contract`, `host_runtime_notify_enable_disable_contract` | same Rust-only selector | low after selector exposure; live foreground host-notify probes stay separate |

Commands run:

```text
cd /home/ecs-user/awiki-space/awiki-system-test && PYTHONDONTWRITEBYTECODE=1 uv run python -m py_compile tests_v2/cli/test_awiki_cli_runtime_listener_local.py
cd /home/ecs-user/awiki-space/awiki-system-test && git diff --check -- tests_v2/cli/test_awiki_cli_runtime_listener_local.py tests_v2/cli/CLAUDE.md
cd /home/ecs-user/awiki-space/awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 AWIKI_CLI_UPDATE_CACHE_ONLY=1 PYTHONDONTWRITEBYTECODE=1 uv run pytest -p no:cacheprovider tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_session_secure_replay_host_notify_contracts -ra -q
cd /home/ecs-user/awiki-space/awiki-cli && go test ./internal/runtime/listener ./internal/runtime/openclawnotify -run 'Test(StartServiceAutoInstallsWhenMissing|WaitForServiceStatusWithWaitsForBridgeAvailability|WaitForServiceStatusWithWaitsForExpectedBootID|MergeSavedRuntimeStatus|SessionWarnings|HasDisconnectedSessions|NewSupervisorMarksInstalledWhenRunningAsService|StartSocketPersistsBridgeAvailability|SessionLoopReconnectsAndStoresNotifications|MessageRecordFrom|RecordsFromGroupStateChanged|HandleNotification|HandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|DeliverLocalSecureAckInProcessPromotesPendingInitiatorSession|NormalizeHostNotification|HandleNotificationDispatchesHostNotificationToSink|NewOpenClawHostNotifySink|OpenClawHostNotifySinkNotifyUsesRouteRegistry|BuildOpenClawHookRequestIncludesChannelDelivery|BuildOpenClawEventTextUsesMainAgentSessionFormat|NewHermesHostNotifySinkRejectsInvalidNotifyURL|HermesHostNotifySinkNotifySignsRequest|ResolveRouteInputSupportsChannelAndTo|ResolveRouteInputSupportsSessionKey|LoadRoutesMissingFileReturnsEmpty|AddAndRemoveRoutePersistRegistry)' -count=1
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 fmt --check
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 check -p awiki-cli --locked
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && cargo +1.79.0 run --bin xtask --locked -- check-structure
cd /home/ecs-user/awiki-space/awiki-cli-rs2 && git diff --check
```

Observed results:

- System-test wrapper syntax and whitespace checks: passed.
- New focused non-mail system-test selector: 1 passed, 0 failed, 0 skipped in
  1.34s. The selector exposes 20 Rust runtime/listener and host-notify Cargo
  contract targets.
- Focused Go `internal/runtime/listener` and `internal/runtime/openclawnotify`
  guards: passed.
- Rust formatting, package check, structure check, and whitespace check:
  passed.
- `tests_v2/cli/test_awiki_cli_runtime_listener_local.py` is 1256 lines after
  this selector update. This exceeds the older 1200-line review target but
  stays well below the active 3000-line test-file limit; it is the existing
  runtime/listener selector hub and no Rust source/test file grew in this
  batch. No Rust file-size exception is needed.

System-test configuration context:

- `AWIKI_CLI_UNDER_TEST=rust`.
- `AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`.
- `AWIKI_CLI_UPDATE_CACHE_ONLY=1`.
- `PYTHONDONTWRITEBYTECODE=1`.
- Selector path:
  `tests_v2/cli/test_awiki_cli_runtime_listener_local.py::test_awiki_cli_runtime_listener_session_secure_replay_host_notify_contracts`.
- The selector runs deterministic Rust Cargo contracts only. It does not start
  live mail services and does not count mail selectors.

Coverage boundary:

- This batch promotes system-test visibility for deterministic local contracts
  that already covered Go runtime/listener session bootstrap, identity watch,
  secure replay/sync, secure ack/outbox flush, local notification queue, and
  host-notify payload/sink/config behavior.
- It does not claim new production behavior, live foreground signal handling,
  full end-to-end secure replay delivery through a real WebSocket listener,
  broad repository-wide acceptance, Windows Service Manager parity, macOS
  launchd parity, real Hermes service/process integration, or mail selectors.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The batch reuses existing std/Rustls listener and host-notify code,
existing local ANP/E2EE helper boundaries, and the approved `rusqlite +
bundled` SQLite path. TLS remains Rustls-first; no OpenSSL, `native-tls`,
async runtime, or SQLite backend change was introduced.
