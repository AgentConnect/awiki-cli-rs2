# Verification Reports

Store command transcripts and summary reports for parity, structure, Rust unit tests, ANP SDK tests, and `awiki-system-test` runs here.

## 2026-05-16 Ordinary Group WebSocket Send/Messages Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_decrypt_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures|TestHTTPTransportGroupMethodsUseExpectedRPCMethods|TestBuildGroupMessagesRPCParams' -count=1
cd ../awiki-cli && go test ./internal/message -run 'TestTransportSource|TestSourceWithDefault|TestWebSocketFallbackWarnings' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_ws.rs crates/awiki-cli/tests/msg_ws_group_live_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `msg_ws_group_live_contract`: 7 passed.
- Adjacent group HTTP, group E2EE send/decrypt, and low-level websocket proxy
  contracts passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1105 lines, `group_ws.rs` 367 lines, and
  `msg_ws_group_live_contract.rs` 880 lines. No file-size exception is needed.

Scope:

- Wires ordinary non-E2EE group text send to Go's websocket-mode transport
  branch after the existing group E2EE auto-upgrade guard. Successful bridge
  sends call `group.send`, persist through the existing cache path, and return
  source `local_ws_cache`. Bridge failures fall back to signed HTTP
  `group.send`, record trace fallback `websocket_to_http`, append the visible
  websocket HTTP fallback warning, and return source `remote_http`.
- Wires `group messages` to Go's websocket-mode transport branch. Successful
  bridge reads call `group.list_messages`, then run the existing group E2EE
  decrypt hook, persistence, and cache projection with source default
  `local_ws_cache`.
- Preserves Go's group-message fallback order: bridge failure first returns a
  nonempty local SQLite group-message cache as `local_ws_cache_fallback` with
  summary `Loaded group messages from local cache` and no HTTP request; only
  empty cache falls back to signed HTTP `group.list_messages` with the visible
  websocket HTTP fallback warning.
- Preserves Go's WebSocket fallback error priority for both group send and
  group message listing: if HTTP auth/client preparation fails after a bridge
  failure, the original bridge error is returned; after HTTP preparation
  succeeds, RPC parameter build and RPC failures are returned as HTTP-side
  errors.
- Keeps group lifecycle/control commands HTTP-only in websocket mode, matching
  the existing Go `groupControlTransport` behavior.

Boundary note: this slice covers ordinary non-E2EE group send/list-message
transport orchestration. Group E2EE websocket/local bridge transport,
foreground listener group E2EE handling, attachment websocket transport, and
full awiki-system-test group websocket acceptance remain separate.

Parallelism note: GPT-5.5 xhigh Native Agents were launched for implementation
and test slices with non-overlapping write scopes, but both were shut down
before producing usable changes. The leader completed the slice locally to
avoid asynchronous write conflicts. Future code-writing Native Agents remain
constrained to GPT-5.5 xhigh with bounded, non-overlapping write scopes.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing std local bridge helper,
`WSProxyTransport`, authsdk/session, Rustls/std HTTP transport, group E2EE
decrypt hook, and approved `rusqlite + bundled` SQLite cache path. It does not
add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
crates, async runtimes, YAML crates, platform service libraries, ANP SDK
default/network features, pure-Rust SQLite optimization work, or a new SQLite
backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Decrypt Display Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_decrypt_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestMLSExecProviderCommands|TestHTTPTransportGroupMethodsUseExpectedRPCMethods' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_decrypt.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/tests/group_e2ee_decrypt_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_decrypt_contract`: 3 passed.
- `group_e2ee_send_contract`: adjacent outbound group E2EE path passed.
- `group_live_contract`: adjacent live group HTTP path passed.
- `message_group_e2ee_wire_contract`: adjacent hidden group E2EE wire builders passed.
- `group_e2ee_status_contract`: adjacent provider/status selection path passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1199 lines, `group_e2ee_decrypt.rs` 217 lines,
  `group_e2ee_provider.rs` 506 lines, and
  `group_e2ee_decrypt_contract.rs` 737 lines. No file-size exception is needed.

Scope:

- Wires Go `maybeDecryptGroupMessages` into the HTTP `group messages` path:
  `group.list_messages` results are decrypted before local persistence, and
  the CLI then reads the same cache projection as Go.
- Preserves Go cipher discovery shapes: top-level `group_cipher_object`, direct
  cipher content, nested `content.group_cipher_object`, and
  `body.group_cipher_object`.
- Preserves Go provider boundary for decrypt:
  `anp-mls message decrypt` receives `api_version=anp-mls/v1`, active agent
  DID, recipient DID equal to the active DID, selected device id, group DID,
  opaque cipher object, `private_message_b64u`, `group_state_ref`, sender DID,
  cipher content type, `security_profile=group-e2ee`, message ID, and
  operation ID.
- Preserves Go successful plaintext rewrite: `application_plaintext.text`
  becomes message content, `application_content_type` defaults to
  `text/plain`, and `decrypted=true` is persisted in the message metadata. The
  CLI output returns the decrypted cache projection.
- Preserves Go warning behavior for failed decrypts by compacting
  `Group E2EE decrypt failed for message <id>: <err>` warnings and continuing
  other messages.

Boundary note: this slice covers only HTTP `group messages` decrypt/display.
WebSocket/local bridge group message receive/decrypt, foreground listener group
E2EE handling, and full awiki-system-test group-E2EE acceptance remain
separate.

Parallelism note: a GPT-5.5 xhigh Native Agent was started for the independent
test-file slice but was shut down before producing a usable result; the leader
implemented the test locally to avoid concurrent write risk. Future
code-writing Native Agents remain constrained to GPT-5.5 xhigh with bounded,
non-overlapping write scopes.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
the external local ANP Rust SDK `anp-mls` binary, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, pure-Rust SQLite optimization work, or a new SQLite
backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Outbound Send Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_repair_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_update_key_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && rg -n 'TestBuildGroupE2EESend|GroupE2EESend|HTTPTransportGroupMethodsUseExpectedRPCMethods' internal/message internal/cli
cd ../awiki-cli && go test ./internal/message -run 'TestBuildGroupE2EESendRPCParamsSendsOnlyOpaqueCipherObject|TestHTTPTransportGroupMethodsUseExpectedRPCMethods' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestMsg' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_send.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/tests/group_e2ee_send_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_send_contract`: 1 passed.
- `group_live_contract`: 3 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `group_e2ee_repair_contract`: 1 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_e2ee_update_key_contract`: 1 passed.
- `group_e2ee_recover_member_contract`: 1 passed.
- `group_e2ee_add_contract`: 6 passed.
- `group_e2ee_remove_leave_contract`: 3 passed.
- `group_e2ee_create_contract`: 2 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1199 lines, `group_e2ee_send.rs` 345 lines,
  `group_e2ee_transport.rs` 266 lines, and
  `group_e2ee_send_contract.rs` 643 lines. No file-size exception is needed.

Scope:

- Wires live outbound group E2EE text send through Go's `sendGroup` routing:
  explicit `--secure on` requires a cached `group-e2ee` snapshot, while the
  ordinary group send path auto-upgrades when the cached snapshot uses group
  E2EE or local MLS state indicates active/pending crypto state.
- Preserves Go provider boundary for encryption: `anp-mls message encrypt`
  receives `api_version=anp-mls/v1`, active agent DID, device id, group DID,
  local `group_state_ref`, sender DID, generated message/operation IDs,
  `content_type=application/anp-group-cipher+json`,
  `security_profile=group-e2ee`, `message_type`, and plaintext text.
- Preserves Go hidden send wire shape: `group.e2ee.send` uses
  `anp.group.e2ee.v1`, `group-e2ee`, the cipher content type, origin proof
  auth, and only sanitized opaque cipher fields. Provider-only plaintext/debug
  fields are not sent.
- Preserves Go output decoration and local cache behavior: server-omitted
  `group_did`, `message_id`, and `operation_id` are backfilled, the local
  group message row is stored as E2EE, `message.secure=true`,
  `message.security_profile=group-e2ee`, `data.e2ee.encrypted=true`,
  `data.e2ee.group_state_ref`, `cipher_object_sent=true`, and summary
  `Sent a group text message with group E2EE`.
- The Go stale-epoch repair/retry branch is translated through the existing
  repair helper, including best-effort local pending-commit finalization and a
  retry after repair, but broader service edge-case/system coverage remains
  separate.

Boundary note: this slice still excludes group E2EE decrypt/receive/history
display, WebSocket/local bridge group E2EE transport, and full
awiki-system-test group-E2EE acceptance.

Parallelism note: no code-writing Native Agent output was used. Earlier
code-writing Native Agents were stopped because the user constraint requires
GPT-5.5 xhigh for code-writing agents, and the leader completed this slice
locally to avoid write-scope or model-setting ambiguity. Future code-writing
Native Agents remain constrained to GPT-5.5 xhigh with bounded,
non-overlapping write scopes.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, the
existing repair helper, and the approved `rusqlite + bundled` SQLite path. It
does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, async runtimes, YAML crates, platform service libraries, MLS
provider crates, ANP SDK default/network features, pure-Rust SQLite
optimization work, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Repair Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_repair_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_update_key_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestInspectGroupE2EEStatusComparesLocalEpochToServiceHead|TestGroupE2EEStatusForRecoveryScansNonDefaultDevice' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_e2ee_repair.rs crates/awiki-cli/src/message/group_e2ee_status.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_repair_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_repair_contract`: 1 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_update_key_contract`: 1 passed.
- `group_e2ee_recover_member_contract`: 1 passed.
- `group_e2ee_add_contract`: 6 passed.
- `group_e2ee_remove_leave_contract`: 3 passed.
- `group_e2ee_create_contract`: 2 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_e2ee_repair.rs` 561 lines, `group_e2ee_status.rs` 541 lines,
  `group_e2ee_transport.rs` 231 lines, `group_e2ee_provider.rs` 486 lines,
  `group_e2ee_handlers.rs` 573 lines, and
  `group_e2ee_repair_contract.rs` 578 lines. No file-size exception is needed.

Scope:

- Wires live hidden `group e2ee repair` through Go's pending notice repair
  path: active identity gate, hidden `group.e2ee.head` preflight warning,
  accepted local pending-commit finalization, hidden `group.e2ee.notice` pull,
  commit notice replay through external MLS `anp-mls commit process`, welcome
  notice replay through existing `anp-mls welcome process`, processed notice
  mark-delivered, final local status scan, recovery diagnosis/artifact, and Go
  live plan insertion.
- Preserves Go repair result shape and summary:
  `Replayed group E2EE pending notices`, with `processed`,
  `processed_count`, `finalized_pending_commits`, `finalized_pending_count`,
  `pending_count`, `delivered_result`, `group`, `local`, `local_device_id`,
  `service_head`, `diagnosis`, and `recovery_artifact`.
- Preserves Go mark-delivered request shape: the second hidden notice call uses
  `limit=len(notice_ids)`, not the original repair pull limit, and includes only
  the processed notice IDs.
- Preserves Go pending-commit finalize request shape: accepted local pending
  commits are finalized with only `pending_commit_id`, rather than passing the
  full provider pending-commit object.

Boundary note: this slice still excludes group E2EE send/decrypt,
WebSocket/local bridge group E2EE transport, broader commit/welcome replay
edge-case system coverage, and full awiki-system-test group-E2EE acceptance.

Parallelism note: one read-only Native Agent reviewed the repair parity diff
and found the mark-delivered `limit` mismatch that was fixed before final
verification. No code-writing Native Agent edited files in this slice; future
code-writing Native Agents remain constrained to GPT-5.5 xhigh with bounded,
non-overlapping write scope.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, and
the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Runtime Host-Notify Enable/Disable Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_openclaw_config_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_openclaw_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/config -run 'TestUpdateHostNotifyEnabledWritesBooleanPointer' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions' -count=1
```

Result: passed for the commands listed above.

Observed results:

- `runtime_host_notify_enable_disable_contract`: 2 passed.
- `runtime_contract`: 12 passed.
- `runtime_openclaw_config_contract`: 5 passed.
- `runtime_host_notify_sink_contract`: 10 passed.
- `runtime_openclaw_host_notify_contract`: 11 passed.
- `runtime_hermes_host_notify_contract`: 8 passed.
- Go focused config and runtime dry-run selectors passed.
- `cargo check`, structure check, whitespace check, and dependency audit passed.
- Changed Rust source/test files remain below the default 1200-line
  review-size cap: `runtime_handlers.rs` 721 lines, `cli/mod.rs` 451 lines,
  `cmdmeta/mod.rs` 301 lines, and
  `runtime_host_notify_enable_disable_contract.rs` 186 lines.

Scope:

- Adds Go catalog/dispatch parity for `runtime host-notify enable` and
  `runtime host-notify disable`.
- Preserves Go dry-run contract: summary
  `Dry run: host notify enablement change planned`, plan action
  `host_notify_enable_toggle`, `enabled`, and `config_file`.
- Preserves local config behavior: live toggles only
  `runtime.host_notify.enabled`, preserves the configured sink, and returns
  summaries `Host notify enabled` / `Host notify disabled` with `host_notify`
  plus listener-status context.

Boundary note: the current Rust runtime layer reports local listener status but
does not implement Go's full service-manager listener restart side effect for
host-notify changes. That remains part of the broader runtime listener/service
execution gap. `awiki-system-test` currently has no active selector for
`runtime host-notify enable` or `runtime host-notify disable`.

Parallelism note: a GPT-5.5 xhigh Native Agent added only the new focused test
file under a bounded, non-overlapping write scope. The leader implemented
source wiring, documentation, and final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This command pair uses existing config writer and runtime status
helpers and does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, async runtimes, YAML crates, platform service
libraries, ANP SDK network/default features, or a new SQLite backend. TLS
policy remains Rustls-first.

## 2026-05-16 Debug DB Handle-History Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test debug_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cd ../awiki-cli && go test ./internal/cli -run 'TestNormalizeDebugHandleTrimsPrefixesAndDomains|TestBuildHandleHistoryOwnersAggregatesByOwner' -count=1
```

Result: passed for the commands listed above.

Observed results:

- `debug_contract`: 4 passed.
- `core_contract`: 15 passed.
- Go focused `internal/cli` selector passed.
- Changed Rust source/test files remain below the default 1200-line
  review-size cap: `app.rs` 1024 lines, `app/debug_handlers.rs` 253 lines,
  `store/contacts.rs` 444 lines, and `debug_contract.rs` 310 lines.

Scope:

- Adds the Go `debug db handle-history <HANDLE>` command to the Rust command
  catalog and dispatcher.
- Preserves Go argument validation, handle normalization, empty-handle error,
  ordered `contact_handle_bindings` lookup, no-row `not_found` mapping with
  `sql: no rows in result set`, and success envelope data containing
  `database_file`, normalized `handle`, raw `rows`, and aggregated `owners`.
- Extracts existing debug DB CLI handlers into `app/debug_handlers.rs` so the
  main `app.rs` file remains below the 1200-line default while preserving the
  existing `debug db query` and `debug db import-v1` behavior.

Boundary note: `awiki-system-test` currently has no active
`debug db handle-history` selector under `tests_v2/debug`; this slice is
covered by Rust CLI integration tests plus the focused Go unit reference tests.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The command uses the existing approved `rusqlite + bundled` SQLite
lane and does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, async runtimes, YAML crates, platform service
libraries, ANP SDK network/default features, or a new SQLite backend. TLS
policy remains Rustls-first.

## 2026-05-16 Group E2EE Update-Key Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_update_key_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestUpdateGroupE2EEKeyUsesUpdateMethodWithoutGroupAdd|TestMLSExecProviderCommands|TestHTTPTransportGroupMethods' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_e2ee_update.rs crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_update_key_contract.rs
```

Result: passed for the commands listed above.

Scope:

- Wires live hidden `group e2ee update-key` through Go's active-member key
  rotation path: active identity, target DID resolution, `device_id=default`
  fallback, hidden `group.e2ee.head` owner/status preflight, hidden
  `group.e2ee.get_key_package` with `purpose=update`, external MLS
  `anp-mls group update-member-prepare`, hidden `group.e2ee.update`, update
  `group update-member-finalize`, E2EE summary persistence, and optional local
  welcome processing for a local target member.
- Preserves Go no-P4-mutation behavior: the command never calls public
  `group.add` or `group.e2ee.recover_member`, the hidden update body omits P4
  `member_did`, `role`, and `recovery_key_package_id`, and output includes
  `p4_membership_mutate=false`.
- Preserves Go result shape and summary:
  `Updated group E2EE member key without P4 membership mutation`, with
  `group`, `member`, `target`, redacted `update_key_package`, `mls_prepare`,
  `mls_finalize`, `delivery`, `argv_sensitive_fields`,
  `hidden_awiki_extension`, and the Go live plan.
- Uses update-specific provider finalize/abort methods rather than the generic
  commit finalize/abort used by recovery and remove.

Boundary note: this slice still excludes repair, commit replay beyond
finalize/abort and local welcome processing, group E2EE send/decrypt,
WebSocket/local bridge group E2EE transport, and full awiki-system-test
group-E2EE acceptance.

Parallelism note: a code-writing Native Agent was launched with GPT-5.5 xhigh
under a bounded new-test-file scope, but it was stopped before producing
changes because the source implementation and test file had to proceed without
waiting. The leader implemented source and tests locally and ran final
verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, and
the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Message Secure Direct Injectable Send Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/service.rs crates/awiki-cli/tests/message_secure_send_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation' -count=1
```

Result: passed.

Scope:

- Adds `message::send_secure_direct_with_sender` as an injectable service-level
  boundary for the local behavior of Go `sendSecureDirect`.
- Preserves active identity gating through the existing message identity helper.
- Preserves the exact Go missing-key error:
  `secure direct messaging requires DID signing and X25519 E2EE private keys`.
- Preserves target and text validation before invoking the injected sender.
- Preserves target DID resolution and passes the resolved DID plus generated
  `msg-` message/operation IDs to the injected sender.
- Preserves pending-confirmation error handling: queue one local E2EE outbox row
  through `queue_secure_outbox_record`, keep original plaintext/type, set
  `local_status=queued`, set credential name, and record
  `{"reason":"pending_confirmation"}` metadata.
- Preserves the Go queued result shape and summary:
  `Queued secure direct message pending peer confirmation`.
- Preserves successful secure direct send result shape with `secure=true`.
- Updates the shared direct-send persistence helper to match Go
  `persistSendResult`: `request.secure_mode == "on"` drives
  `message.secure=true` and `messages.is_e2ee=1`; ordinary direct live tests
  cover the unchanged non-secure path.
- Keeps files under the default review-size cap:
  `service.rs` and `message_secure_send_contract.rs` are both below 1200
  lines.

Boundary note: this slice intentionally does not wire production
`msg send --secure on`. The public CLI path still remains blocked by the
existing `SecureNotSupported` branch until the real secure sender is available.
It also does not construct or use a production `MessageServiceE2EEClient`, does
not publish/retrieve prekeys, does not encrypt `SendText`, does not mutate real
E2EE sessions, does not wire `msg secure init/repair/retry` production senders,
and does not implement incoming secure decrypt.

Parallelism note: a read-only Native Agent mapped the next secure-direct gaps.
A code-writing Native Agent was launched with GPT-5.5 xhigh under a bounded
test-file scope but was stopped before it produced changes because the leader
completed the source/test slice locally to avoid write-scope collision.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing identity, message, store, `serde_json`,
and approved `rusqlite + bundled` paths. It does not enable ANP SDK
`network`/default features and does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates,
platform service libraries, new E2EE provider dependencies, or a new SQLite
backend.

## 2026-05-16 Message Secure E2EE Client Preparation Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/secure_client.rs crates/awiki-cli/tests/message_secure_client_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSecureInitCreatesPendingSession|TestServiceSecureRetryMarksQueuedRecordSent|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1
```

Result: passed.

Scope:

- Adds `message::secure_client` as a focused local preparation translation for
  Go `NewSecureE2EEClientForRecord` before high-level client construction.
- Preserves Go manager/record required errors.
- Preserves identity path lookup through `Manager::paths_for_identity`.
- Preserves DID signing private-key parsing and E2EE agreement private-key
  parsing through the local ANP facade, including Go error prefixes:
  `parse DID signing private key:` and
  `parse E2EE agreement private key:`.
- Preserves Go P5 file-store root creation under the identity directory:
  `p5-e2ee-sessions`, `p5-signed-prekeys`, and `p5-one-time-prekeys`.
- Preserves returned owner DID and Go key ID construction:
  `<did>#key-1` and `<did>#key-3`.
- Preserves local DID document resolver precedence: current record document
  first for the current DID, then local manager list/load fallback for matching
  summaries.
- Preserves local resolver nil-equivalent behavior: missing manager, empty DID,
  list/load errors, missing identity, or missing DID document return no local
  document.
- Avoids deriving `Debug` for the prepared client context because it carries
  private key material.
- Keeps files under the default review-size cap:
  `secure_client.rs` and `message_secure_client_contract.rs` are both well
  below 1200 lines.

Boundary note: this slice intentionally stops before production
`MessageServiceE2EEClient` construction. It does not implement real
`SendText`, `SendJSON`, `PublishPrekeyBundle`, `ProcessIncoming`, remote DID
resolution through `anpsdk.ResolveDidDocument`, RPC/WebSocket transport,
production `msg secure retry/send/init/repair`, or awiki-system-test
secure-direct acceptance.

Parallelism note: a read-only Native Agent reviewed this slice for Go/Rust
boundary drift. No code-writing Native Agent changed code for this slice.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing local `../anp/rust` key material APIs
through the CLI facade and the existing CLI-side file-store facades. It does
not enable ANP SDK `network`/default features and does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, new E2EE provider
dependencies, or a new SQLite backend.

## 2026-05-16 Message Direct WebSocket Send Production Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
wc -l /home/ecs-user/awiki-space/awiki-cli/internal/message/service.go crates/awiki-cli/src/message/service.rs crates/awiki-cli/tests/msg_ws_proxy_live_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `msg_ws_proxy_live_contract`: 2 passed.
- `msg_live_contract`: 7 passed.
- `message_ws_proxy_contract`: 3 passed.
- `msg_contract`: 6 passed.
- `group_live_contract`: 3 passed.
- `cargo check` and `cargo fmt --check` passed.
- File-size check: Go `internal/message/service.go` is 1372 lines, Rust
  `service.rs` is 1192 lines, and the new focused test file is 524 lines.

Scope:

- Wires ordinary direct text `msg send --to --text` through the local
  WebSocket bridge when `runtime.mode=websocket`.
- Preserves Go bridge request shape: local bridge method `direct.send`, identity
  name copied from the selected identity, and params `target`, `text`, and
  `type`.
- Preserves Go HTTP fallback behavior for this direct-send branch: if bridge
  send fails, the service attempts the existing signed HTTP `/im/rpc`
  `direct.send`; successful fallback records trace fallback `websocket_to_http`
  but does not add `websocketHTTPFallbackWarning` to the user-visible result.
- Preserves Go double-failure behavior: if bridge send fails and HTTP fallback
  also fails, the original bridge/transport error is returned.
- Keeps direct send output shape compatible with Go by not adding a `data.source`
  field to direct send results.

Boundary note: this slice is intentionally narrow. It does not wire
WebSocket/local bridge execution for direct inbox/history, group send/messages,
group lifecycle, attachments, or secure direct E2EE. It also does
not implement foreground listener bridge dispatch, local-cache fallback, runtime
listener live `ProcessIncoming`, or awiki-system-test secure-direct acceptance.

Parallelism note: a read-only Native Agent mapped the Go WebSocket transport and
fallback behavior, and one GPT-5.5 xhigh code-writing Native Agent created the
focused test file under a bounded write scope. The leader corrected the test to
Go's actual `--to` flag and no-`data.source` direct-send shape after integrating
the Go parity mapping.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing std local bridge helper,
`WSProxyTransport`, Rustls/std HTTP fallback path, authsdk/session, local ANP
origin-proof helper, and approved `rusqlite + bundled` store lane. It does not
add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
crates, async runtimes, YAML crates, platform service libraries, ANP SDK
network/default features, or a new SQLite backend.

## 2026-05-16 Message Mark-Read WebSocket Bridge Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/message/mark_read.rs crates/awiki-cli/tests/msg_ws_mark_read_live_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestBuildMarkReadRPCParamsValidatesMessageIDs|TestBuildMarkReadRPCParamsRequiresMessageIDs|TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures' -count=1
go test ./internal/store -run 'TestMessageQueryHelpersLookupAndMarkReadRespectOwner|TestListDirectMessagesByPeerDIDsFiltersUnreadInboxOnlyAndDeduplicates' -count=1
go test ./internal/cli -run 'TestMsgDryRunPlansRenderStableContracts' -count=1
```

Result: passed. Rust formatting, package check, focused mark-read WebSocket
contract, adjacent message WebSocket/direct live regressions, CLI/group
regressions, structure check, whitespace check, Go reference selectors, and
dependency audit all passed. The dependency audit found no OpenSSL/native-tls,
`reqwest`, `hyper`, WebSocket crate, YAML crate, or platform service-manager
dependency; hits remained limited to the existing Rustls/ring/base64/sha2 stack
and approved `rusqlite + bundled` SQLite chain.

Scope:

- Extracted ordinary `msg mark-read` production behavior into
  `message::mark_read`, keeping `service.rs` below the default 1200-line file
  guideline.
- Preserves Go `Service.MarkRead` local ID classification: known group rows and
  local mail notification rows are handled locally, while direct/unknown rows go
  through the selected remote transport.
- In `runtime.mode=websocket`, direct IDs call the local bridge method
  `inbox.mark_read` with Go-shaped `message_ids` params and the selected
  identity name.
- If the bridge is unavailable, the service falls back to signed HTTP `/im/rpc`
  `inbox.mark_read`, records trace fallback `websocket_to_http`, and appends
  Go's visible WebSocket HTTP fallback warning.
- If bridge and HTTP fallback both fail, the original bridge/transport error is
  returned.
- After successful bridge or HTTP handling, local SQLite rows are marked read for
  direct, group, and local-only IDs. The Go `updated_count` fallback/addition
  rules are preserved.
- The focused live contract covers bridge success, HTTP fallback warning,
  double-failure error precedence, and group/mail local-only classification.

Boundary note: this slice does not wire WebSocket/local bridge execution for
direct inbox/history, non-E2EE group send/messages, group lifecycle, cache
fallback, foreground listener bridge serving, attachments, secure direct E2EE,
runtime listener live `ProcessIncoming`, or awiki-system-test secure-direct
acceptance.

Parallelism note: one GPT-5.5 xhigh code-writing Native Agent created the
initial focused test file under a bounded single-file write scope. A read-only
Native Agent mapped the Go mark-read tests and documentation update points. The
leader implemented the production module, added the local-only classification
case, integrated docs, and owned final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing std local bridge helper,
`WSProxyTransport`, Rustls/std HTTP fallback path, authsdk/session, and approved
`rusqlite + bundled` store lane. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates,
platform service libraries, ANP SDK network/default features, or a new SQLite
backend.

## 2026-05-16 Message History WebSocket Bridge Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_history_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/message/history.rs crates/awiki-cli/tests/msg_ws_history_live_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures|TestReadHistoryFromCacheByPeerDIDs|Test.*History' -count=1
go test ./internal/store -run 'TestListDirectMessagesByPeerDIDsFiltersUnreadInboxOnlyAndDeduplicates|TestMessageQueryHelpersLookupAndMarkReadRespectOwner' -count=1
go test ./internal/cli -run 'TestMsgDryRunPlansRenderStableContracts' -count=1
```

Result: passed. Rust formatting, package check, focused history WebSocket
contract, adjacent message WebSocket/direct live regressions, CLI/group
regressions, structure check, whitespace check, Go reference selectors, and
dependency audit all passed. The dependency audit found no OpenSSL/native-tls,
`reqwest`, `hyper`, WebSocket crate, YAML crate, or platform service-manager
dependency; hits remained limited to the existing Rustls/ring/base64/sha2 stack
and approved `rusqlite + bundled` SQLite chain.

Scope:

- Extracted ordinary `msg history --with` production behavior into
  `message::history`, keeping `service.rs` below the default 1200-line file
  guideline.
- Preserves Go `Service.History` validation and default limit behavior.
- Preserves active identity gating and target resolution, including the Go
  handle-resolution-failure fallback to `local_handle_history_cache` when local
  historical handle bindings can satisfy the request.
- In `runtime.mode=websocket`, history first calls local bridge method
  `direct.get_history` with Go-shaped params and selected identity name.
- Bridge success uses Go source defaulting for websocket mode:
  `local_ws_cache` when the bridge result has no source.
- Bridge failure checks local SQLite direct history first; handle targets widen
  to all cached historical DIDs for the handle when available.
- Usable local cache returns immediately with summary
  `Loaded history from local websocket cache`, source
  `local_ws_cache_fallback`, and Go's cache fallback warning. Pending direct
  E2EE wire rows suppress this cache fallback like Go.
- If no usable cache exists, the service falls back to signed HTTP `/im/rpc`
  `direct.get_history`, records trace fallback `websocket_to_http`, and appends
  Go's visible WebSocket HTTP fallback warning.
- If bridge and HTTP fallback both fail, the HTTP-side error is returned,
  matching Go history behavior.
- Successful remote bridge/HTTP results reuse the existing history persistence,
  direct E2EE display filtering, contact sync, and handle-history cache merge.

Boundary note: this slice does not wire WebSocket/local bridge execution for
direct inbox, non-E2EE group send/messages, group lifecycle, foreground listener
bridge serving, attachments, secure direct E2EE WebSocket execution, runtime
listener live `ProcessIncoming`, or awiki-system-test secure-direct acceptance.

Parallelism note: a read-only Native Agent mapped the Go/Rust history behavior
and tests. A GPT-5.5 xhigh code-writing Native Agent was launched with a
bounded single-test-file scope but was stopped before producing changes to avoid
write-scope collision; the leader completed the production module, test file,
docs, and verification locally.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing std local bridge helper,
`WSProxyTransport`, Rustls/std HTTP fallback path, authsdk/session, direct E2EE
display filtering, contact-handle cache helpers, and approved `rusqlite +
bundled` store lane. It does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates, platform
service libraries, ANP SDK network/default features, or a new SQLite backend.

## 2026-05-16 Message Inbox WebSocket Bridge Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_inbox_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_history_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/message/inbox.rs crates/awiki-cli/src/message/history.rs crates/awiki-cli/tests/msg_ws_inbox_live_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures|TestAllInboxMergesLocalMailNotifications|TestReadInboxFromCacheExcludesMailNotificationsForDirectInbox|TestReadUnifiedDirectInboxFromCacheIncludesNewStyleMailMetadataRows|Test.*Inbox|TestReadHistoryFromCacheByPeerDIDs' -count=1
go test ./internal/store -run 'TestListDirectMessagesByPeerDIDsFiltersUnreadInboxOnlyAndDeduplicates|TestMessageQueryHelpersLookupAndMarkReadRespectOwner' -count=1
go test ./internal/cli -run 'TestMsgDryRunPlansRenderStableContracts' -count=1
```

Result: passed. Rust formatting, package check, focused inbox WebSocket
contract, adjacent message WebSocket/direct live regressions, CLI/group
regressions, structure check, whitespace check, Go reference selectors, and
dependency audit all passed. The dependency audit found no OpenSSL/native-tls,
`reqwest`, `hyper`, WebSocket crate, YAML crate, or platform service-manager
dependency; hits remained limited to the existing Rustls/ring/base64/sha2 stack
and approved `rusqlite + bundled` SQLite chain.

Scope:

- Extracted ordinary direct `msg inbox` production behavior into
  `message::inbox`, keeping `service.rs` below the default 1200-line file
  guideline.
- Preserves Go direct/non-`all` `Service.Inbox` default limit/scope handling,
  active identity gating, group-scope rejection, target resolution, and
  handle-resolution-failure fallback to `local_handle_history_cache` when local
  historical handle bindings can satisfy the request.
- In `runtime.mode=websocket`, direct inbox first calls local bridge method
  `inbox.get` with Go-shaped params and selected identity name.
- Bridge success uses Go source defaulting for websocket mode:
  `local_ws_cache` when the bridge result has no source.
- Bridge failure checks local SQLite direct inbox first; handle targets widen to
  all cached historical DIDs for the handle when available.
- Usable local cache returns immediately with summary
  `Loaded inbox from local websocket cache`, source `local_ws_cache_fallback`,
  and Go's cache fallback warning. Pending direct E2EE wire rows suppress this
  cache fallback like Go.
- If no usable cache exists, the service falls back to signed HTTP `/im/rpc`
  `inbox.get`, records trace fallback `websocket_to_http`, and appends Go's
  visible WebSocket HTTP fallback warning.
- If bridge and HTTP fallback both fail, the HTTP-side error is returned,
  matching Go inbox behavior.
- Successful remote bridge/HTTP results reuse existing inbox persistence,
  direct E2EE display filtering, contact sync, handle-history cache merge,
  direct inbox filters, and `--mark-read` result mutation.

Boundary note: this slice covers ordinary direct/non-`all`/non-`group` inbox
only. Go defaults empty `scope` to `all`, then routes that path to `allInbox`,
which has separate unified direct inbox, local group, and mail-notification
cache merge semantics. Explicit `scope=group` is covered by the later group
inbox local-cache slice.

Parallelism note: a read-only Native Agent mapped the Go/Rust inbox behavior,
all-inbox boundary, and docs update points. A GPT-5.5 xhigh code-writing Native
Agent was launched with a bounded test-file scope but was stopped before
producing changes; the leader completed the production module, test file, docs,
and verification locally.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing std local bridge helper,
`WSProxyTransport`, Rustls/std HTTP fallback path, authsdk/session, direct E2EE
display filtering, contact-handle cache helpers, and approved `rusqlite +
bundled` store lane. It does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates, platform
service libraries, ANP SDK network/default features, or a new SQLite backend.

## 2026-05-16 Message All-Inbox Local Cache Merge Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_all_inbox_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_inbox_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_history_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_proxy_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
wc -l crates/awiki-cli/src/message/inbox.rs crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/store/messages.rs crates/awiki-cli/src/store/groups.rs crates/awiki-cli/tests/msg_all_inbox_live_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'TestAllInboxMergesLocalMailNotifications|TestReadInboxFromCacheExcludesMailNotificationsForDirectInbox|TestReadUnifiedDirectInboxFromCacheIncludesNewStyleMailMetadataRows|TestNormalizeMailNotificationMessageRecognizesMetadataSourceKind' -count=1
go test ./internal/store -run 'TestListGroupInboxMessages|TestListNotificationInboxMessages|TestListDirectMessagesByPeerDIDsFiltersUnreadInboxOnlyAndDeduplicates|TestMessageQueryHelpersLookupAndMarkReadRespectOwner' -count=1
go test ./internal/cli -run 'TestMsgDryRunPlansRenderStableContracts' -count=1
```

Result: passed. Rust formatting, package check, focused all-inbox contract,
adjacent message WebSocket/direct live regressions, CLI/group regressions,
structure check, whitespace check, Go reference selectors, and dependency audit
all passed. The dependency audit found no OpenSSL/native-tls, `reqwest`,
`hyper`, WebSocket crate, YAML crate, or platform service-manager dependency;
hits remained limited to the existing Rustls/ring/base64/sha2 stack and the
approved `rusqlite + bundled` SQLite chain.

Scope:

- Translates Go `Service.allInbox` for default `msg inbox` and explicit
  `scope=all`.
- Reads local group inbox cache first, preserving Go warning text on local
  group-cache read failures.
- In `runtime.mode=websocket`, reads the unified direct inbox cache with local
  mail notifications included and returns local direct/group cache results
  without calling local bridge or HTTP when that direct-cache read succeeds.
- On non-websocket mode or unified direct-cache failure, recursively calls the
  direct inbox path, reads local mail notifications separately, normalizes mail
  rows, and merges direct/mail/group rows.
- Preserves Go source strings:
  `local_direct_cache+local_group_cache` for websocket cache success and
  `remote_http+local_group_cache+local_mail_cache` for the recursive
  direct/mail fallback path.
- Adds Rust store helpers matching Go `ListGroupInboxMessages` and
  `ListNotificationInboxMessages`, including unread filtering, default limit
  20, `direction=0`, local mail notification predicate, and
  `COALESCE(sent_at, stored_at) DESC` ordering.
- Preserves Go mail notification normalization for rows identified by
  `content_type='mail.notification'` or metadata `source_kind=mail`: strips
  `mail:` and existing `[邮件] ` prefixes, defaults blank subjects to
  `(no subject)`, emits `source_kind=mail`, and rebuilds the Chinese title and
  content lines.
- Merges direct/mail/group rows by `sent_at` falling back to `stored_at`, then
  truncates by limit.
- Preserves Go all-inbox mark-read behavior: collect merged IDs, call
  `MarkRead` best-effort, send only direct IDs to the remote/local bridge path,
  locally mark known direct/group/mail rows when the mark-read operation
  succeeds, and mark returned rows read regardless of mark-read errors.

Boundary note: this slice does not claim explicit Go `scope=group` inbox.
That path is covered by the later group inbox local-cache slice; group
message/list execution remains in its dedicated group rows.

Parallelism note: Native Agent spawning was attempted for a read-only parity
review, but the session was already at the child-agent limit. No code-writing
Native Agent changed files in this slice.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing store/query helpers and the approved
`rusqlite + bundled` SQLite path, plus the existing direct inbox and mark-read
modules. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, async runtimes, YAML crates, platform service
libraries, ANP SDK network/default features, or a new SQLite backend. TLS
remains Rustls-first for later runtime/WebSocket transport work.

## 2026-05-16 Message Explicit Group Inbox Local Cache Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test msg_all_inbox_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_inbox_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_ws_mark_read_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
wc -l crates/awiki-cli/src/message/inbox.rs crates/awiki-cli/tests/msg_all_inbox_live_contract.rs
```

Go reference verification:

```bash
go test ./internal/message -run 'Test.*Inbox|TestMergeInboxMessagesSortsAndLimits' -count=1
go test ./internal/store -run 'TestListGroupInboxMessages|TestMessageQueryHelpersLookupAndMarkReadRespectOwner' -count=1
go test ./internal/cli -run 'TestMsgDryRunPlansRenderStableContracts' -count=1
```

Result: passed. Rust formatting, package check, focused group/all inbox
contract, adjacent message and group regressions, structure check, whitespace
check, Go reference selectors, and line-count checks all passed. Files remain
under the default review-size cap: `inbox.rs` is 761 lines and
`msg_all_inbox_live_contract.rs` is 859 lines after this slice.

Scope:

- Translates Go `Service.groupInbox` for explicit `msg inbox --scope group`.
- Preserves default limit/scope normalization before group routing and active
  identity gating before local cache access.
- Reads only local SQLite group inbox rows through `ListGroupInboxMessages`
  with `groupStorageKey(request.Group)`.
- Preserves empty `--group` behavior under `--scope group`: blank storage key
  returns all local group inbox rows, not an error.
- Preserves nonempty `--group` filtering by matching `group_did` or `group_id`.
- Preserves `--unread` filtering through the store helper.
- Preserves Go output shape: `messages`, `total`, `source=local_group_cache`,
  `group=request.Group`, and summary `Loaded N group inbox messages`.
- Preserves `--mark-read` behavior: collect returned IDs, call `MarkRead`
  best-effort only when IDs exist, mark every returned message `is_read=true`
  after that call, and rely on existing local group-row classification so group
  IDs are mutated locally without remote bridge/HTTP sends.
- Preserves Go routing for the subtle CLI boundary: `--group <did>` without
  `--scope group` still follows default `scope=all` and does not implicitly
  route to `groupInbox`.

Boundary note: this slice is local-cache only. It does not add group RPC,
WebSocket group transport, foreground listener dispatch, group E2EE/MLS,
attachment behavior, or system-test acceptance for remaining message runtime
lanes. Optimization was not mixed into the translation; any later consolidation
of inbox helpers should wait until broader parity is complete.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `rusqlite + bundled`, the translated
group storage key helper, and existing mark-read classification. It does not
add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
crates, async runtimes, YAML crates, platform service libraries, ANP SDK
network/default features, or a new SQLite backend.

## 2026-05-16 Message Secure Retry Injected Store Execution Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/message ./internal/runtime/listener -run 'TestServiceSecureRetryMarksQueuedRecordSent|TestServiceSecureFailedAndDropOperateOnOutbox|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1
```

Result: passed.

Scope:

- Adds a store-backed `flush_queued_secure_outbox_with_sender` executor on top
  of the previously translated row planner.
- Preserves Go queued row listing by active owner/credential and
  `local_status="queued"`.
- Preserves stable ascending `created_at` processing while executing each row's
  plan immediately, so sender callbacks, row mutations, and message-store
  writes happen in Go row order.
- Preserves peer filtering with the caller filter trimmed and exact raw
  `peer_did` row comparison.
- Preserves malformed queued-row skips for blank `outbox_id` or blank
  `peer_did`.
- Preserves injected send request values: outbox ID, target DID, defaulted
  original type, plaintext, and parsed JSON payloads for `original_type=json`.
- Preserves JSON send gating at the executable helper boundary: invalid JSON
  rows are marked `invalid_payload`/`drop` with detail metadata and do not call
  the injected sender.
- Preserves Go side-effect ordering for successful sends: current session ID is
  read only after send success, then the outbox row is marked sent, then the
  outgoing E2EE message is stored.
- Preserves send-error mutation with `send_failed`, `retry`, detail metadata,
  warning text, no session lookup, no mark-sent, and no message-store write.
- Preserves invalid JSON and unsupported original-type failure mutations via
  the row planner and actual DAO updates.
- Preserves mark-sent failure behavior: warning and no store-message write for
  that outbox ID.
- Preserves store-message failure behavior: row remains sent and a warning is
  appended after the store attempt.
- Adds `secure_retry_with_sender` for the Go `SecureRetry` local boundary:
  active identity gate, store open/schema, selected-row get before mutation,
  missing-row error before side effects, selected row status reset to `queued`,
  peer-filtered queued flush through the injected sender/session boundary,
  selected row reload with null fallback, summary
  `Retried secure outbox record <id>`, and warnings returned from flush.
- Keeps files under the default review-size cap: modified Rust source and test
  files remain below 1200 lines.

Boundary note: this slice intentionally exposes an injectable sender/session
lookup boundary instead of wiring production `msg secure retry` execution.
Real `NewSecureE2EEClientForRecord`, `MessageServiceE2EEClient`, DID
resolution, RPC/WebSocket transport, prekey publishing, `SecureInit`,
`SecureRepair`, and awiki-system-test secure-direct acceptance remain deferred
parity slices.

Dependency note: no dependency was added. The slice reuses existing store DAO,
message-store DAO, approved `rusqlite + bundled`, `serde_json`, and standard
library collections. It does not enable ANP SDK `network`/default features and
does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, Tokio, YAML crates, platform service libraries, new E2EE
provider dependencies, or a new SQLite backend.

## 2026-05-16 Message Secure Status/Failed/Drop Command Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/message -run 'TestServiceSecureStatusReturnsSessionAndOutboxSummary|TestServiceSecureFailedAndDropOperateOnOutbox' -count=1
```

Result: passed.

Scope:

- Adds `message::secure_commands` for the local SQLite-backed subset of Go
  `internal/message/secure_commands.go`.
- Wires non-dry-run `msg secure status`, `msg secure failed`, and
  `msg secure drop` through the app handler while leaving dry-run output
  unchanged.
- Preserves active/ready identity gating through the existing message service
  helper.
- Preserves `SecureStatus` local status behavior: optional `--with` resolution,
  `p5-e2ee-sessions/*.json` object loading, peer-DID filtering, session sort by
  `peer_did`, Go session status redaction fields, `skipped_key_count`, outbox
  owner/credential listing, peer filtering, blank `local_status` counting as
  `unknown`, outbox status row redaction, and summary/data shape. The focused
  contract test asserts that key material, skipped message key material,
  outbox plaintext, metadata, owner DID, and credential name do not leak through
  the status output.
- Preserves `SecureFailed` store behavior: open the local store, ensure schema,
  list `e2ee_outbox` rows for the active owner/credential with
  `local_status="failed"`, return full rows without status redaction, return
  `{ "failed": rows, "total": len }`, and summarize with
  `Loaded N failed secure outbox record(s)`.
- Preserves `SecureDrop` store behavior: verify the target outbox row belongs
  to the active owner/credential before mutation, set `local_status` to
  `dropped`, return `{ "outbox_id": id, "status": "dropped" }`, and summarize
  with `Dropped secure outbox record <id>`.
- Preserves missing-row parity with Go's `sql.ErrNoRows` path by surfacing the
  store `query returned no rows` error through the generic internal-error lane
  rather than mapping it to `message not found`.
- Keeps files under the default review-size cap: `secure_commands.rs` and the
  focused test file remain below 1200 lines.
- A code-writing Native Agent contributed the focused test expansion under the
  required GPT-5.5 xhigh configuration and a single-file write scope.

Boundary note: this slice does not implement `SecureRetry` queued flush
execution, `SecureInit`, `SecureRepair`, `queueSecureOutboxRecord`,
`currentSecureSessionID`, ANP SDK E2EE clients, session-store mutation,
WebSocket RPC, prekey publishing, or `awiki-system-test` secure-direct
acceptance.

Dependency note: no dependency was added. The slice reuses existing identity,
message, store, `serde_json`, filesystem, and approved `rusqlite + bundled`
APIs. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, Tokio, YAML crates, platform service libraries, ANP
SDK E2EE wiring, or new SQLite dependencies.

## 2026-05-16 Store E2EE Outbox DAO Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_e2ee_outbox_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_recover_merge_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/store/dao.go` E2EE outbox functions:
  `QueueE2EEOutbox`, `MarkE2EEOutboxSent`, `MarkE2EEOutboxFailed`,
  `UpdateE2EEOutboxStatus`, `SetE2EEOutboxFailureByID`, `GetE2EEOutbox`,
  and `ListE2EEOutbox`.
- Adjacent Go store guard: `go test ./internal/store -count=1`.

Result: passed.

Scope:

- Adds `store::e2ee_outbox` as a focused Rust module for the Go E2EE outbox
  DAO subset.
- Preserves queue insert SQL shape, generated `local-<nanos>` IDs, owner DID
  trimming, raw peer DID/plaintext preservation, optional string trim-to-null
  behavior, optional integer null behavior, metadata blank-to-null behavior,
  `original_type` defaulting to `text`, `local_status` defaulting to `queued`,
  shared `created_at`/`updated_at` default timestamp capture, and credential
  name trimming.
- Preserves mark-sent behavior: owner-only where clause, optional string/int
  `COALESCE`, `local_status="sent"`, attempt increment, `last_attempt_at` and
  `updated_at` refresh, and clearing of failure fields.
- Preserves mark-failed behavior: owner-only where clause, failed status,
  raw error code, optional retry/failed-message/server-seq/metadata `COALESCE`,
  updated timestamp, and no attempt/last-attempt mutation.
- Preserves status update and failure-by-ID owner-vs-credential fallback
  branches, including raw status/error-code values and credential-name trim in
  fallback mode.
- Preserves get/list branch behavior: owner branch wins when owner DID is
  nonblank, blank owner queries by credential, missing rows map to
  `StoreError::NotFound("query returned no rows")`, and all list branches sort
  by `updated_at DESC` without an extra tie-breaker.
- Preserves JSON row mapping for `SELECT *`: nulls become JSON null, integers
  become JSON numbers, text values become strings, and all schema columns remain
  visible to callers.
- Keeps files under the default review-size cap: `e2ee_outbox.rs` is 314 lines
  and the focused test file is 420 lines after the final local assertions were
  added.
- A code-writing Native Agent contributed the focused test file under the
  required GPT-5.5 xhigh configuration and a single-file write scope.

Boundary note: this is a local SQLite DAO slice. It does not implement message
secure execution, `queueSecureOutboxRecord`, `FlushQueuedSecureOutbox` real
store/client wiring, `currentSecureSessionID`, ANP SDK file session stores,
E2EE clients, WebSocket RPC, CLI secure commands, or `awiki-system-test`
secure-direct acceptance.

Dependency note: no dependency was added. The slice reuses existing
`rusqlite + bundled`, `serde_json`, and store helpers. It does not add
OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates,
Tokio, YAML crates, platform service libraries, ANP SDK wiring, E2EE provider
dependencies, file-store dependencies, or new SQLite dependencies.

## 2026-05-16 Message Secure Control Helper and Queued Outbox Row Planning Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/message/secure_control.go` pure helpers:
  `BuildSecureAckPayload`, `BuildSecureInitPayload`, `IsSecureAckPlaintext`,
  `IsSecureInitPlaintext`, `secureAckSessionID`, and
  `isPendingConfirmationError`.
- Go source parity for `internal/message/secure_control.go`
  `currentSecureSessionID`, `queueSecureOutboxRecord`, and
  `FlushQueuedSecureOutbox` queued-row sorting, filtering, payload handling,
  failure updates, send result handling, sent metadata, stored message shape,
  and compact warnings.
- Existing secure listener/message guards cover adjacent real E2EE consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1`.

Result: passed.

Scope:

- Adds `message::secure_control` as a pure helper module for the control
  payload and plaintext predicates used by Go secure-direct command/listener
  flows.
- Preserves `BuildSecureAckPayload`: fixed secure-ack system type, trimmed
  `session_id`, and trimmed `acked_message_id`.
- Preserves `BuildSecureInitPayload`: fixed secure-init system type and
  `reason="manual_init"`.
- Preserves `IsSecureAckPlaintext` and `IsSecureInitPlaintext`: exact
  `application/json` content-type check, Go `mapFromAny` object-or-JSON-string
  payload parsing, string-only `system_type` extraction, and ack/init type
  mismatch rejection.
- Preserves `secureAckSessionID`: Go `mapFromAny` payload parsing and
  string-only `session_id` extraction.
- Preserves `isPendingConfirmationError`: nil-equivalent false behavior plus
  case-insensitive matching for `pending confirmation` and
  `pending-confirmation`.
- Preserves `currentSecureSessionID`: nil-equivalent manager/record cases
  return blank, identity path/store lookup errors return blank,
  `p5-e2ee-sessions` is opened through the Go-shaped file session store,
  exact peer DID lookup is used, missing peers return blank, and returned
  session IDs are trimmed.
- Preserves `queueSecureOutboxRecord`: missing identity records return
  `identity record is required`, the local store is opened and schema ensured,
  current secure session ID is captured, blank `original_type` defaults to
  `text`, plaintext/peer DID/owner DID/credential name are inserted unchanged
  through the existing DAO normalization, `local_status` is `queued`, and
  metadata is exactly `{"reason":"pending_confirmation"}`.
- Adds `message::secure_outbox_flush` as a pure row-loop planning helper for Go
  `FlushQueuedSecureOutbox`.
- Preserves stable ascending sort by raw `created_at` string, including stable
  order for equal timestamps.
- Preserves peer filtering: trim the caller filter, then compare exact raw row
  `peer_did` values.
- Preserves silent skips for empty `outbox_id` or empty `peer_did`.
- Preserves `original_type` defaulting: blank or whitespace becomes `text`,
  while nonblank values retain their original string.
- Preserves text send planning and JSON-object send planning, including no
  injected send outcome lookup for invalid JSON or unsupported original types.
- Preserves invalid JSON failure update with `invalid_payload`, `drop`, detail
  metadata, parse warning, and continue.
- Preserves unsupported original-type failure update with
  `unsupported_original_type`, `drop`, original-type metadata, warning, and
  continue.
- Preserves send failure update with `send_failed`, `retry`, detail metadata,
  warning, and no mark-sent or store-message action.
- Preserves successful send behavior: message ID fallback to outbox ID, compact
  sent metadata with `target_did`, `operation_id`, `delivery_state`, and
  `flushed_from="queued"`, injected session ID, and mark-sent action.
- Preserves mark-sent error behavior: warning and no store-message action.
- Preserves store-message error behavior: store action is still planned and the
  warning is appended afterward.
- Preserves outgoing direct E2EE `MessageRecord` shape, including
  `Direction=1`, deterministic direct thread ID, `IsRead=true`, `IsE2EE=true`,
  credential name, original plaintext content, accepted-at timestamp, success
  metadata, and Go's current `json` content-type fallback to `text/plain`.
- Preserves `compactWarnings` trimming, empty-drop, deduplication, and first
  occurrence order.
- Keeps files under the default review-size cap: `secure_control.rs`,
  `secure_outbox_flush.rs`, and the focused test file are all below 1200
  lines.

Boundary note: this slice now includes local store open/schema/insert for
`queueSecureOutboxRecord` and local file-session lookup for
`currentSecureSessionID`. It still does not implement real
`FlushQueuedSecureOutbox` row listing/action execution, E2EE client
construction or sending, message store writes outside the planner, WebSocket
RPC, foreground listener execution, `SecureRetry`, `SecureInit`,
`SecureRepair`, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing message
helpers, store record types, approved `rusqlite + bundled`, `serde_json`, std
collections/filesystem APIs, and the existing local ANP facade file session
store. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, Tokio, YAML crates, platform service libraries, new
E2EE provider dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Notification Consume Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_notification_consume_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `consumeNotifications` ping, context-cancel, notification-channel close, and
  dispatch control flow.
- Adjacent Go listener reconnect guard:
  `go test ./internal/runtime/listener -run TestSessionLoopReconnectsAndStoresNotifications -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_notification_consume` as a pure step helper for Go
  `consumeNotifications` before foreground WebSocket notification execution is
  wired.
- Preserves Go ping timing constants: `sessionPingInterval` remains 60 seconds
  and the per-ping timeout remains 15 seconds.
- Preserves context cancellation behavior: exit with the context error and no
  planned side effects.
- Preserves ping-tick behavior: create a 15-second ping timeout, call ping, and
  cancel the timeout context after the ping attempt.
- Preserves ping error wrapping as `websocket ping failed: <err>`.
- Preserves notification dispatch behavior: a received notification is passed
  to `handleNotification` and the loop continues.
- Preserves closed notification channel behavior: prefer `ReaderError()` when
  present, otherwise return `websocket notification loop closed`.
- Keeps files under the default review-size cap:
  `listener_notification_consume.rs` is 75 lines and the focused test file is
  116 lines before subsequent formatting-independent changes.

Boundary note: this is a pure consume-step helper. It does not implement real
ticker/context/channel ownership, `WSClient.Ping`, `WSClient.Notifications`,
`ReaderError`, `handleNotification` side effects, foreground session execution,
host-notify dispatch, SQLite writes, local bridge I/O, or
`awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice uses only existing
`serde_json::Value` plus `std::time::Duration`. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio,
YAML crates, platform service libraries, E2EE provider dependencies, or new
SQLite dependencies.

## 2026-05-16 Runtime Listener Notification Route-Plan Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_notification_plan_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `handleNotification` branch ordering and side-effect sequencing after secure
  normalization.
- Focused Go listener/host-notify/secure guards:
  `go test ./internal/runtime/listener -run 'TestHandleNotificationDispatchesHostNotificationToSink|TestHandleNotificationStoresMessageWhenHostNotifyFails|TestHandleNotificationDispatchesMailNotificationToSink|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession|TestRecordsFromGroupStateChangedBuildsMemberAndSystemMessage' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_notification_plan` as a helper-only route/action plan
  for Go `handleNotification` before foreground listener execution.
- Preserves Go ordering: secure normalization first, host event normalization
  once on the post-secure notification, then branch matching in direct, mail,
  group, and group-state order.
- Preserves direct incoming planning: contact sync, host handle enrichment,
  message store, then host dispatch.
- Preserves contact-sync error handling in direct/group branches: errors are
  discarded and become blank sender handles.
- Preserves recipient handle normalization through the existing
  `normalize_listener_handle` helper.
- Preserves mail planning: store then dispatch, with no contact sync or handle
  enrichment.
- Preserves group incoming planning: group-scoped contact sync, host handle
  enrichment, message store, then host dispatch.
- Preserves group-state planning: upsert group, optional upsert member, store
  system message, then dispatch.
- Preserves unknown notification behavior: no side effects.
- Preserves secure direct control boundary through injected normalization
  outcomes: drop stops before host/store, replace uses the replacement
  notification, and keep-original falls back to secure-wire direct storage.
- Keeps files under the default review-size cap:
  `listener_notification_plan.rs` is 258 lines and the focused test file is 422
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure route-plan slice. It does not implement real
`normalizeDirectSecureNotification`, E2EE decrypt/ack/session-store mutation,
`FlushQueuedSecureOutbox`, `SendJSON`, local notification queue mutation,
SQLite writes, remote contact lookup, host sink delivery/status writes,
foreground WebSocket session execution, or `awiki-system-test` runtime listener
acceptance.

Dependency note: no dependency was added. The slice reuses existing parser,
host-notify, contact handle normalization, secure classification, store record
types, `serde_json`, and `time` code only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML
crates, platform service libraries, E2EE provider dependencies, or new SQLite
dependencies.

## 2026-05-16 Runtime Listener Connect Session Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_connect_session_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go` `connectSession`
  control-flow behavior before the actual WebSocket transport.
- Adjacent Go listener/wsclient guards:
  `go test ./internal/runtime/listener -run 'TestSessionLoopReconnectsAndStoresNotifications|TestWSClientConnectRefreshesExpiredBearerBeforeRetryingWebSocket|TestWSClientConnectBootstrapsBearerBeforeOpeningWebSocket' -count=1`.
- Adjacent Go identity-gating guard:
  `go test ./internal/cli -run TestIdentityGatingUsesFrozenErrorCode -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_connect_session` as a helper-only translation of Go
  `connectSession` decisions before foreground listener transport is wired.
- Preserves identity manager load error ordering: load errors return before
  readiness checks, path lookup, auth-session construction, or client creation.
- Preserves stored identity readiness gating with Go's registration/handle
  missing-state logic and registration-required error text.
- Preserves path lookup ordering after readiness and before auth-session plan
  construction.
- Preserves auth-session construction inputs as a plan: DID document path,
  key-1 private path, identity name, DID, and initial JWT.
- Preserves stored JWT bearer seeding: nonblank-after-trim tokens seed exactly
  three scopes in Go order while using the original untrimmed token string:
  service base URL, DID-auth RPC URL, and `/im/ws` request URL.
- Preserves blank stored JWT behavior: no bearer scopes are seeded.
- Preserves `NewWSClient` error behavior: return the construction error without
  closing an unconstructed client.
- Preserves connect behavior: use a 15-second timeout, close the constructed
  client on connect error, and return the connect error.
- Preserves success behavior: write `authSession.CurrentJWT()` into the returned
  record's JWT token and return connected.
- Keeps files under the default review-size cap:
  `listener_connect_session.rs` is 293 lines and the focused test file is 256
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure connect-session planning slice. It does not
implement real `identity.Manager`, `authsdk.NewSession`, JWT update callback
execution, `NewWSClient`, `WSClient.Connect`, timeout/context ownership,
WebSocket transport selection, foreground session loops, SQLite writes,
host-notify dispatch, local bridge I/O, or `awiki-system-test` runtime listener
acceptance.

Dependency note: no dependency was added. The slice reuses existing endpoint URL
helpers and `std::time::Duration` only. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates,
platform service libraries, E2EE provider dependencies, or new SQLite
dependencies.

## 2026-05-16 Runtime Listener Session Loop Backoff Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_loop_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go` `runSessionLoop`,
  `retryPublishSecurePrekeys`, `sleepWithContext`, and `minDuration` pure
  control-flow behavior.
- Focused Go runtime listener reconnect integration guard:
  `go test ./internal/runtime/listener -run TestSessionLoopReconnectsAndStoresNotifications -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_session_loop` as a helper-only translation of session
  loop retry/backoff decisions before foreground listener execution.
- Preserves Go constants: reconnect base delay one second, reconnect max delay
  30 seconds, and secure prekey retry delay one second.
- Preserves top-of-loop cancellation behavior as a close-current-client-and-exit
  decision.
- Preserves connect-failure behavior: mark disconnected and signal initial
  error happen in the caller boundary, then the helper sleeps the current delay,
  doubles delay, caps at 30 seconds, and retries only when the sleep completes.
- Preserves successful-connect behavior: delay resets to one second before the
  connected-session work sequence.
- Preserves connected action order for marking connected, one-shot initial
  success, status refresh, queued local notification flush, secure prekey retry
  start, unread secure direct inbox polling start, and notification consumption.
- Preserves consume completion order for child task cancellation, client close,
  mark disconnected, and status refresh.
- Preserves cancellation after consume: exit before sleeping or doubling.
- Preserves context-cancelled sleep behavior: exit without doubling the pending
  delay.
- Preserves `signalInitial` one-shot behavior through a pure `sync.Once`-like
  helper.
- Preserves `retryPublishSecurePrekeys` stop/retry decision: empty warnings
  finish; nonempty warnings log
  `listener secure prekey publish retry identity=<identity> warnings=<joined>`
  and retry after one second.
- Keeps files under the default review-size cap:
  `listener_session_loop.rs` is 197 lines and the focused test file is 184
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure control-flow helper slice. It does not implement
`Supervisor`, goroutine/task spawning, real context/timer ownership,
`connectSession`, `WSClient.Connect`, `consumeNotifications`, secure inbox
polling, `PublishSecurePrekeys`, SQLite writes, host-notify dispatch, local
bridge I/O, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice uses only
`std::time::Duration` and does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates, platform service
libraries, E2EE provider dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Secure Inbox Poll Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_inbox_poll_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `pollUnreadSecureDirectInbox` startup sync, ticker interval, tick sync order,
  and context-cancel exit.
- Existing secure listener integration guards cover adjacent consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_inbox_poll` as a pure polling step helper for
  Go `pollUnreadSecureDirectInbox` before foreground WebSocket secure inbox
  polling is wired.
- Preserves startup ordering: call `syncUnreadSecureDirectInbox`, then
  `syncPendingConfirmationSecureHistory`, then start the ticker.
- Preserves current Go ticker interval as 2 seconds.
- Preserves tick ordering: every ticker event calls
  `syncUnreadSecureDirectInbox` before `syncPendingConfirmationSecureHistory`
  and then continues.
- Preserves context cancellation behavior: stop the ticker and exit without
  another sync.
- Keeps files under the default review-size cap:
  `listener_secure_inbox_poll.rs` is 56 lines and the focused test file is 45
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure polling-control helper. It does not implement
real ticker/context ownership, `WSClient.SendRPC`, unread inbox RPC, pending
history RPC, replay filtering, SQLite lookup, `handleNotification`, secure
decrypt/ack, foreground session execution, host-notify dispatch, local bridge
I/O, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice uses only
`std::time::Duration`. It does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates, platform service
libraries, E2EE provider dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Local Secure Ack In-Process Planning Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_ack_in_process_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `deliverLocalSecureAckInProcess` skip ladder, encrypted ack notification
  shape, recipient process fallback ladder, sender/recipient session save
  ordering, active recipient flush/log branch, managed queue branch, and network
  fallback branch.
- Existing secure listener/message guards cover adjacent real E2EE consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_ack_in_process` as a pure control-flow
  planning helper for Go `deliverLocalSecureAckInProcess`.
- Preserves nil sender behavior: return false before recipient lookup.
- Preserves missing local recipient behavior: lookup recipient DID, then return
  false when no local record is found.
- Preserves sender-side setup ordering: sender paths, sender session store,
  `FindByPeerDID`, and `EncryptFollowUp` skip in order on failure.
- Preserves encrypted ack request shape: `BuildSecureAckPayload` is used for
  the JSON plaintext, metadata uses sender DID, recipient target DID, ack
  message ID, `anp.direct.e2ee.v1`, `direct-e2ee`, and
  `application/anp-direct-cipher+json`.
- Preserves recipient client init failure before `ProcessIncoming`.
- Preserves recipient `ProcessIncoming` success gate: only exact
  `state="decrypted"` skips the fallback ladder.
- Preserves fallback ladder after process error/non-decrypted state: recipient
  paths, recipient session store, `LoadSession`, marshal ack body, unmarshal
  cipher body, `DecryptFollowUp`, and recipient session save, with false return
  at each failing step.
- Preserves sender session save after recipient processing and before active
  session lookup.
- Preserves active recipient branch: optional queued outbox flush when secure
  RPC exists, flush-warning log before delivered log, then true.
- Preserves managed inactive runtime branch: queue a `direct.incoming` wrapper
  whose `params` are the encrypted ack notification, then true.
- Preserves unmanaged runtime branch: log network fallback and return false.
- Keeps files under the default review-size cap:
  `listener_secure_ack_in_process.rs` is 458 lines and the focused test file is
  489 lines.

Boundary note: this is a pure planning slice. It does not implement real ANP
SDK file session stores, E2EE encrypt/decrypt, `message.NewSecureE2EEClientForRecord`,
`ProcessIncoming`, `FlushQueuedSecureOutbox`, `activeSessionByDID`,
`hasRuntimeSessionForDID`, local notification queue mutation, SQLite writes,
WebSocket RPC, host-notify dispatch, foreground listener execution, or
`awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing identity
types, `serde_json`, and `BuildSecureAckPayload`. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio,
YAML crates, platform service libraries, E2EE provider dependencies,
file-store dependencies, ANP SDK wiring, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Peer Queued Secure Outbox Flush Trigger Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `flushPeerQueuedSecureOutbox` session snapshot scan, owner-session selection,
  nil secure-RPC return, queued outbox flush trigger, and warning log behavior.
- Existing secure listener/message guards cover adjacent real E2EE consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_outbox_flush` as a pure Supervisor-level
  trigger helper for Go `flushPeerQueuedSecureOutbox`.
- Preserves session snapshot behavior by scanning the supplied snapshot and
  using that snapshot order. Go's source snapshot is built from a map, so no
  stronger ordering guarantee is claimed.
- Preserves nil current record and nonmatching owner DID skips.
- Preserves exact owner DID comparison without trimming.
- Preserves the first-owner-match nil secure-RPC behavior: return immediately
  without scanning later sessions.
- Preserves successful trigger behavior: call `FlushQueuedSecureOutbox` once
  for the owner record and peer DID, then log owner DID, peer DID, and warnings,
  then return.
- Preserves exact peer DID forwarding without trimming.
- Keeps files under the default review-size cap:
  `listener_secure_outbox_flush.rs` is 72 lines and the focused test file is
  135 lines.

Boundary note: this is a pure trigger-planning slice. It does not implement
real session locks, real `secureRPC`, `message.FlushQueuedSecureOutbox`,
`WSClient.SendRPC`, E2EE encryption/decryption, file session stores, SQLite
outbox mutation, foreground listener execution, host-notify dispatch, local
bridge I/O, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing identity
types only. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, Tokio, YAML crates, platform service libraries,
E2EE provider dependencies, file-store dependencies, or new SQLite
dependencies.

## 2026-05-16 Runtime Listener Secure Inbox/History Sync Planning Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_sync_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `syncUnreadSecureDirectInbox` and `syncPendingConfirmationSecureHistory`
  request planning, 15-second timeout ownership, cancel ordering, RPC error
  skip behavior, and replay-filter handoff.
- Existing secure listener/message guards cover adjacent real E2EE consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_sync` as a pure RPC/replay planning helper for
  Go `syncUnreadSecureDirectInbox` and
  `syncPendingConfirmationSecureHistory`.
- Preserves missing-current-record behavior: no RPC and no replay.
- Preserves unread inbox request planning: 15-second timeout, method
  `inbox.get`, Go request intent `Scope=direct`, `UnreadOnly=true`,
  `Limit=100`, and the current Go/Rust `BuildInboxRPCParams` behavior where
  `scope` and `unread_only` are not serialized into params.
- Preserves unread inbox `defer cancel` behavior by planning cancellation after
  RPC/replay handling.
- Preserves unread inbox RPC failure behavior by stopping before replay when no
  RPC result is injected.
- Preserves pending history behavior: no peers means no RPC; each peer gets a
  15-second context; empty peer targets trigger a build-error path that cancels
  and continues; successful builds plan `direct.get_history` with limit 50 and
  cancel after `SendRPC`.
- Preserves replay handoff by reusing existing secure replay filters for unread
  inbox and pending history messages, including store lookup outcomes and
  notification conversion.
- Keeps files under the default review-size cap:
  `listener_secure_sync.rs` is 142 lines and the focused test file is 198 lines.

Boundary note: this is a pure sync-planning slice. It does not implement real
`WSClient.SendRPC`, real context/timer ownership, SQLite `GetMessageByID`,
`handleNotification`, secure decrypt/ack, foreground polling, host-notify
dispatch, local bridge I/O, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing message
wire builders, secure replay helpers, `serde_json`, and `std::time::Duration`.
It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, Tokio, YAML crates, platform service libraries, E2EE provider
dependencies, file-store dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Secure Normalization Planning Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_normalize_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `normalizeDirectSecureNotification` early returns, decrypted notification
  mutation, secure ack/init method rewrites, and secure-init ack side-effect
  ordering.
- Existing secure listener/message guards cover adjacent real E2EE consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_normalize` as an injected-outcome planning
  helper for Go `normalizeDirectSecureNotification`.
- Preserves early returns for non-secure notifications, nil current record,
  missing secure RPC callback, E2EE client construction failure, `ProcessIncoming`
  error, non-`decrypted` state, and missing plaintext object.
- Preserves attempted action ordering for client construction and
  `ProcessIncoming` before those early returns.
- Preserves decrypted notification mutation: `meta.content_type` becomes
  `plaintext.application_content_type`, `params.body` becomes the whitelisted
  plaintext notification body, `secure_state` becomes `decrypted`,
  `secure_wire_content_type` stores the original wire content type, and
  `secure_wire_body` stores the original wire body.
- Preserves secure ack plaintext behavior: flush queued secure outbox for the
  sender DID, set method to `direct.secure.ack`, and return before init-ack
  planning.
- Preserves secure init plaintext behavior: set method to `direct.secure.init`.
- Preserves secure init wire ack planning only when original wire content type
  is `application/anp-direct-init+json` and both original body `session_id` plus
  meta `message_id` are nonempty strings.
- Preserves secure init ack side-effect order: try local in-process ack first;
  if not delivered, send secure ack JSON with `BuildSecureAckPayload`; if that
  send succeeds, plan local ack delivery; then flush peer queued outbox.
- Keeps files under the default review-size cap:
  `listener_secure_normalize.rs` is 289 lines and the focused test file is 384
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure planning slice. It does not implement real
`message.NewSecureE2EEClientForRecord`, `ProcessIncoming`,
`FlushQueuedSecureOutbox`, `SendJSON`, `deliverLocalSecureAckInProcess`,
`deliverLocalSecureAck` side effects, file session stores, E2EE encrypt/decrypt,
SQLite writes, host-notify dispatch, foreground session execution, or
`awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing
`serde_json`, secure notification helpers, and the local ack payload helper. It
does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, Tokio, YAML crates, platform service libraries, E2EE provider
dependencies, file-store dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Local Secure Ack Delivery Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_ack_delivery_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `deliverLocalSecureAck` active-session, ack-body, message-id fallback, and
  notification construction behavior.
- Go source parity for `internal/message/secure_control.go`
  `BuildSecureAckPayload` system type and trimming.
- Existing secure listener/message guards cover adjacent local ack consumers:
  `go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1`.

Result: passed.

Scope:

- Adds `runtime::listener_secure_ack_delivery` as a pure delivery-plan helper
  for Go `deliverLocalSecureAck` before foreground listener secure ack
  execution is wired.
- Preserves active-session gating: missing target session skips before reading
  the ack body.
- Preserves ack-body gating: missing, non-object, and empty bodies skip.
- Preserves Go `stringValue` and `fallbackString` message-ID behavior:
  non-string message IDs become empty, blank strings fall back to the caller
  fallback, and nonblank strings are preserved without trimming.
- Preserves delivered notification shape: method `direct.incoming`, sender DID,
  target agent DID, message ID, profile `anp.direct.e2ee.v1`, security profile
  `direct-e2ee`, content type `application/anp-direct-cipher+json`, and the ack
  body object.
- Preserves `BuildSecureAckPayload`: system type
  `awiki.direct.secure_ack.v1`, trimmed session ID, and trimmed acked message ID.
- Keeps files under the default review-size cap:
  `listener_secure_ack_delivery.rs` is 87 lines and the focused test file is
  137 lines before subsequent formatting-independent changes.

Boundary note: this is a pure local-ack delivery-plan slice. It does not
implement `deliverLocalSecureAckInProcess`, E2EE encrypt/decrypt, file session
stores, recipient `ProcessIncoming`, sender/recipient session persistence,
queued outbox flushing, local notification queue mutation, real active-session
lookup, `handleNotification` side effects, host-notify dispatch, SQLite writes,
foreground session execution, or `awiki-system-test` runtime listener
acceptance.

Dependency note: no dependency was added. The slice uses only existing
`serde_json`. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, Tokio, YAML crates, platform service libraries, E2EE
provider dependencies, file-store dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Secure Replay Filter Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_replay_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `syncUnreadSecureDirectInbox` and `syncPendingConfirmationSecureHistory`
  replay filters before `secureNotificationFromMessageView`.
- Existing secure listener integration guards cover the consuming decrypt/local
  ack paths; no standalone Go replay-filter helper test exists.

Result: passed.

Scope:

- Adds `runtime::listener_secure_replay` as a helper-only translation of secure
  backlog/history replay filtering.
- Preserves exact secure direct wire content-type gating.
- Preserves malformed-item skip for non-object message entries.
- Preserves unread inbox behavior: no sender-self skip; accepted secure message
  views use `(message_id, owner_did, credential_name)` for injected store lookup.
- Preserves pending history behavior: sender DID equal to the local identity DID
  is skipped before store lookup.
- Preserves owner DID fallback for store lookup: use `receiver_did` when present,
  otherwise local/session DID. This fallback is intentionally separate from
  notification conversion, which still requires a non-empty original
  `receiver_did`.
- Preserves store lookup tri-state behavior: existing row skips, non-NoRows
  lookup error skips, NoRows continues to conversion.
- Preserves conversion-error skip for malformed secure message views after store
  lookup, while continuing with later messages.
- Preserves input order for accepted candidates.
- Keeps files under the default review-size cap:
  `listener_secure_replay.rs` is 102 lines and the focused test file is 244
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure replay-filter slice. It does not implement
`WSClient.SendRPC`, inbox/history RPC param construction, periodic polling,
SQLite `GetMessageByID`, `handleNotification`, secure decrypt/ack, foreground
listener sessions, host-notify dispatch, local bridge I/O, or `awiki-system-test`
runtime listener acceptance.

Dependency note: no dependency was added. The slice reuses existing
`serde_json` and secure notification helper code only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML
crates, platform service libraries, E2EE provider dependencies, or new SQLite
dependencies.

## 2026-05-16 Runtime Listener Session DID Lookup Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_lookup_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `activeSessionByDID`, `recordByDID`, and `hasRuntimeSessionForDID`.
- Existing Go secure-listener integration guard:
  `TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession`.

Result: passed.

Scope:

- Adds `runtime::listener_session_lookup` as a helper-only translation of the
  Go listener's DID lookup logic used by local secure ack routing.
- Preserves blank-after-trim DID behavior: active lookup returns nil, record
  lookup returns nil, runtime-session lookup returns false, and manager
  callbacks are not invoked.
- Preserves `activeSessionByDID` scan behavior: sessions are scanned in the
  provided order and only the current in-memory record DID is compared.
- Preserves `recordByDID` manager behavior: nil manager or list failure returns
  nil; summaries are scanned in order; only the first matching DID summary is
  loaded; load failure returns nil without trying later matching summaries.
- Preserves `hasRuntimeSessionForDID` behavior: current record DID matches win
  before manager fallback; nil manager skips fallback loads; otherwise each
  session identity is loaded in scan order, load failures are ignored, and the
  first loaded record DID match returns true.
- Keeps files under the default review-size cap:
  `listener_session_lookup.rs` is 84 lines and the focused test file is 226
  lines before subsequent formatting-independent changes.

Boundary note: this is a pure lookup helper slice. It does not implement
`Supervisor`, mutex ownership, the real identity filesystem manager,
`deliverLocalSecureAckInProcess`, secure ack encryption/decryption, queued local
notification delivery, real WebSocket sessions, SQLite storage mutations,
host-notify dispatch, or `awiki-system-test` runtime listener acceptance.

Dependency note: no dependency was added. The slice uses plain structs and an
injected manager trait only. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates, platform
service libraries, E2EE provider dependencies, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Local Notification Queue Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_local_notifications_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go`
  `queueLocalNotification` and `flushQueuedLocalNotifications`.
- Existing Go secure-listener integration guard:
  `TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession`.

Result: passed.

Scope:

- Adds `runtime::listener_local_notifications` as a helper-only translation of
  the Go listener's in-memory local notification queue.
- Preserves queue skip behavior for blank-after-trim recipient DID and nil
  notification.
- Preserves Go's original-key behavior: recipient DID is trimmed only for the
  presence check, then queued under the original string.
- Preserves append order per recipient DID.
- Preserves flush skip behavior for nil session/current-record analogs and
  blank-after-trim current DID.
- Preserves exact-DID lookup and delete-on-flush behavior; later flushes for the
  same key return empty.
- Keeps files under the default review-size cap:
  `listener_local_notifications.rs` is 42 lines and the focused test file is
  104 lines before subsequent formatting-independent changes.

Boundary note: this is a pure in-memory helper slice. It does not implement
`Supervisor`, mutex ownership, `handleNotification`, secure ack encryption or
decryption, queued secure outbox flushing, real WebSocket sessions, SQLite
storage mutations, host-notify dispatch, or `awiki-system-test` runtime listener
acceptance.

Dependency note: no dependency was added. The slice uses existing
`serde_json::Map`/`Value` and std collections only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML
crates, platform service libraries, E2EE provider dependencies, or new SQLite
dependencies.

## 2026-05-16 Runtime Listener WebSocket Connect Decision Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestNewWSClientDerivesIMWebSocketEndpointFromServiceBaseURL|TestWSClientConnectRefreshesExpiredBearerBeforeRetryingWebSocket|TestWSClientConnectBootstrapsBearerBeforeOpeningWebSocket' -count=1
```

Result: passed.

Scope:

- Adds a pure constructor/Connect decision boundary for Go listener
  `wsclient.go` without adding an executable WebSocket client.
- Preserves `NewWSClient` remembered-scope input order:
  service base URL, DID-auth RPC URL, then `/im/ws` request URL.
- Preserves `dialBearer` bearer header trimming.
- Preserves `refreshBearer` precondition error text for missing auth session
  and blank DID-auth URL.
- Preserves Go `Connect` branch control with injected outcomes: initial token
  dials first; first dial success attaches and stops; non-401 first dial errors
  return formatted dial errors without refresh; 401 first dial refreshes once and
  retries; no-token startup refreshes before any dial; refresh errors are wrapped
  only when an initial token existed; blank refreshed JWT returns
  `did-auth did not return a websocket bearer token`; retry dial failures use
  the existing `formatDialError` body formatting.
- Keeps the files under the default review-size cap:
  `listener_wsclient.rs` is 390 lines and the focused test file is 616 lines
  before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement the real
`WSClient`, `websocket.Dial`, HTTP response ownership/body close behavior, actual
DID-auth HTTP `EnsureJWT`, response-header token capture, `readLoop`, pending RPC
channels, foreground listener session execution, or `awiki-system-test`
acceptance for runtime listener transport.

Dependency note: no dependency was added. The slice uses existing config,
string, enum, and injected-closure helpers only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML
crates, platform service libraries, or new SQLite dependencies. The future real
WebSocket transport remains a separate Rustls-first dependency decision.

## 2026-05-16 Runtime Listener WebSocket Dial Error Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/wsclient.go`
  `formatDialError`.
- No standalone Go `formatDialError` unit test exists.

Result: passed.

Scope:

- Adds the pure `formatDialError` formatting boundary from Go listener
  `wsclient.go`.
- Preserves nil-error behavior as `None`.
- Preserves missing or empty body behavior as the original error text.
- Preserves nonempty body behavior by appending `": "` and the trimmed response
  body.
- Preserves Go's `io.LimitReader(response.Body, 4096)` body cap.
- Keeps the files under the default review-size cap:
  `listener_wsclient.rs` is 235 lines and the focused test file is 291 lines
  before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement real
WebSocket dialing, HTTP response ownership, body read failures, JWT refresh
retry, `Connect`, or `awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice uses byte slicing and
existing string handling only. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates, platform
service libraries, or new SQLite dependencies. TLS policy remains Rustls-first
and unchanged.

## 2026-05-16 Runtime Listener WebSocket JSON-RPC Wire Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/wsclient.go` `SendRPC`,
  `readLoop`, and `failPending`.
- Existing Go `NewWSClient` endpoint test remains the listener wsclient Go
  focused test; there is no standalone Go test for these pure JSON-RPC helpers.

Result: passed.

Scope:

- Adds pure JSON-RPC helper boundaries from Go `WSClient.SendRPC`, `readLoop`,
  and `failPending`.
- Builds request envelopes with `jsonrpc="2.0"`, caller-supplied request ID,
  method, and Go-compatible params omission: `params` is absent for `None` and
  present as `{}` for an empty map.
- Decodes responses like Go `SendRPC`: only object-valued `error` triggers
  `json-rpc error <code>: <message>`, only object-valued `result` is returned,
  and missing/non-object results return an empty map.
- Builds pending-failure messages as Go `failPending` does:
  `{"error":{"message":...},"id":...}`.
- Classifies incoming messages as responses when raw `id` is present and
  notifications otherwise, reusing the existing Go-compatible request-ID
  coercion helper.
- Keeps the files under the default review-size cap:
  `listener_wsclient.rs` is 217 lines and the focused test file is 257 lines
  before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement real
`WSClient`, WebSocket dial/read/write, JWT refresh retry, pending channel
ownership, notification buffering, `formatDialError`, foreground listener
execution, or `awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice reuses existing
`serde_json` and `anyhow` only. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates, platform
service libraries, or new SQLite dependencies. TLS policy remains Rustls-first
and unchanged.

## 2026-05-16 Runtime Listener Session-State Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_state_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestSessionWarnings|TestHasDisconnectedSessions|TestMergeSavedRuntimeStatus' -count=1
```

Result: passed.

Scope:

- Adds a pure listener session-state helper for the local state transitions
  embedded in Go `internal/runtime/listener/server.go`.
- Preserves Go `markConnected` visible behavior: connected sessions set DID and
  clear `last_error`.
- Preserves Go `markDisconnected` visible behavior: nil, empty, or
  cancel-equivalent errors leave the previous `last_error` untouched; non-empty
  errors set it and mark the session disconnected.
- Preserves Go `recordSessionError` visible behavior: a missing session is
  created with the provided DID, while an existing session keeps its current
  record/DID and only records the disconnect/error state.
- Preserves Go `refreshStatus` identity naming: snapshots use the session map
  key as `identity_name`.
- Tracks `bridge_available` changes as a pure changed-bool helper matching
  Go's changed-only status write gate.
- Reuses existing Rust `listener::SessionStatus`, `session_warnings`, and
  `has_disconnected_sessions`.
- Uses `BTreeMap` for deterministic Rust helper snapshots; this is only a test
  stability choice and is stronger than Go's unspecified map iteration order.
- Keeps the files under the default review-size cap:
  `listener_session_state.rs` is 79 lines and the focused test file is 124
  lines before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement `Supervisor`,
locks, status file writes, identity manager lookup, reconnect loops, WebSocket
clients, foreground listener execution, message RPC execution, SQLite side
effects, or `awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice uses std collections and
existing listener status types only. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio, YAML crates,
platform service libraries, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged.

## 2026-05-16 Runtime Bridge Server Framing Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_bridge_dispatch_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

- Go source parity for `internal/runtime/listener/server.go` `handleConn`.
- No standalone Go `handleConn` unit test exists; do not treat a no-match
  `go test -run ...` result as evidence for this slice.

Result: passed.

Scope:

- Adds the server-side local bridge framing helper corresponding to Go
  `handleConn`.
- Reads exactly one newline-terminated JSON request frame and decodes it into
  the existing Rust `BridgeRequest`.
- Preserves Go `ReadBytes('\n')` behavior where EOF before newline, even after
  a syntactically valid JSON object, writes an error response instead of
  dispatching.
- Encodes exactly one newline-terminated `BridgeResponse`.
- Writes Go-shaped bridge error responses for read, JSON decode, and dispatch
  errors: `ok=false`, `error.message=<error>`, no `error.code`, and no
  `result`.
- Keeps extra bytes after the first newline out of the dispatch boundary, as Go
  handles only one request per connection.
- Keeps the files under the default review-size cap: `bridge.rs` is 444 lines
  and `runtime_bridge_contract.rs` is 732 lines before subsequent
  formatting-independent changes.

Boundary note: this is an injected-dispatch helper-only slice. It does not
implement `Supervisor`, `acceptLoop`, real foreground listener execution,
`handleBridgeRequest` integration, WebSocket sessions, message RPC execution,
SQLite side effects, Windows named-pipe I/O, or `awiki-system-test`
acceptance.

Dependency note: no dependency was added. The slice reuses existing std I/O,
`serde_json`, `anyhow`, and bridge wire types only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, Tokio,
YAML crates, platform service libraries, or new SQLite dependencies. TLS policy
remains Rustls-first and unchanged.

## 2026-05-16 Runtime Listener Message-Service DID Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_service_did_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_bridge_dispatch_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/message -run TestHTTPTransportGetMessageServiceDIDUsesConfiguredOrCapabilities -count=1
```

Result: passed.

Scope:

- Adds the pure service-DID helper boundary from Go
  `internal/runtime/listener/server.go` `fetchMessageServiceDID`.
- Builds the listener WebSocket capability request as method
  `anp.get_capabilities` with empty params, matching the listener helper rather
  than the HTTP transport's core-binding request shape.
- Decodes `service_did` exactly through Go listener `stringValue`: only JSON
  strings are accepted; missing, empty, null, numeric, boolean, array, or object
  values return `message service capabilities response is missing service_did`.
- Preserves the listener no-trim behavior: whitespace is non-empty and returned
  unchanged.
- Preserves the missing current-client/session error text
  `websocket session is not connected for identity <identity>`.
- Keeps the files under the default review-size cap:
  `listener_service_did.rs` is 33 lines and the focused test file is 78 lines
  before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement
`Supervisor.currentClient`, `WSClient.SendRPC`, configured `anp_service_did`
precedence from Go's separate HTTP transport, core-binding capability params,
foreground WebSocket sessions, bridge dispatch execution, or
`awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice reuses existing
`serde_json` and `anyhow` only. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML crates, platform service
libraries, or new SQLite dependencies. TLS policy remains Rustls-first and
unchanged.

## 2026-05-16 Runtime Listener Bridge Dispatch Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_bridge_dispatch_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run TestHandleBridgeRequestPreservesSkipForHistoryAndGroupMessages -count=1
```

Result: passed.

Scope:

- Adds the pure bridge dispatch helper boundary from Go
  `internal/runtime/listener/server.go` `handleBridgeRequest`.
- Maps local bridge methods to the same message RPC method names and existing
  Rust message builder functions for direct send, inbox, direct history,
  mark-read, non-E2EE group lifecycle, group send, and group local list/read
  methods.
- Preserves Go's weak bridge-parameter coercions for string, int, bool,
  optional bool, optional int64, patch maps, and `message_ids`.
- Returns `mark_read_message_ids` for the later Go-equivalent post-success
  `store.MarkMessagesRead` side effect.
- Takes `service_did` as explicit input for `group.create`, matching Go's
  required `fetchMessageServiceDID` pre-step without implementing remote
  capability fetch in this helper slice.
- Keeps the files under the default review-size cap:
  `listener_bridge_dispatch.rs` is 325 lines and the focused test file is 313
  lines before subsequent formatting-independent changes.

Boundary note: this is a helper-only slice. It does not implement `Supervisor`,
`ensureSession`, `client.SendRPC`, `fetchMessageServiceDID`, foreground
WebSocket sessions, real bridge request serving, SQLite mark-read mutation,
notification storage, host-notify dispatch, Windows named-pipe I/O, or
`awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice reuses existing message
wire builders and `serde_json` values only. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML
crates, platform service libraries, or new SQLite dependencies. TLS policy
remains Rustls-first and unchanged.

## 2026-05-16 Message WS Proxy Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/message -run 'TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures|TestDecodeMapIntoHandlesNilAndTypedDestination' -count=1
```

Result: passed.

Scope:

- Adds the Go `internal/message/ws_proxy_client.go` helper boundary as
  `message::ws_proxy`.
- Covers `WSProxyTransport` construction with the resolved runtime config and
  identity name.
- Covers bridge method and parameter mapping for `direct.send`, `group.send`,
  `inbox.get`, `direct.get_history`, `inbox.mark_read`, `group.create`,
  `group.get_info`, `group.join`, `group.add`, `group.remove`, `group.leave`,
  `group.get`, `group.list`, `group.list_members`,
  `group.list_messages`, `group.update_profile`, and
  `group.update_policy`.
- Preserves Go's positive-only `skip` emission for direct history and group
  message list calls.
- Preserves Go's `ErrTransportUnavailable` wrapper as
  `MessageError::TransportUnavailable`.
- Preserves Go `decodeMapInto` zero-value tolerance for direct/group send
  result structs by accepting only JSON strings for string fields, only JSON
  bools for bool fields, and leaving mismatched or absent fields at defaults.

Boundary note: this is a helper-only slice. It does not wire real
message-service or CLI execution to WebSocket mode, implement foreground
listener `handleBridgeRequest`, select or add a WebSocket crate, change the
existing HTTP fallback/cache behavior, implement Windows named-pipe bridge I/O,
or claim `awiki-system-test` acceptance.

Dependency note: no dependency was added. The slice reuses the already ported
local bridge I/O plus existing JSON types. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML
crates, platform service libraries, or new SQLite dependencies. TLS policy
remains Rustls-first and unchanged.

## 2026-05-16 Runtime Listener Pending Secure-Session Scan Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_sessions_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
# Use helper-specific Go tests if they are added. At the time this slice was
# documented, Go covered these helpers indirectly through listener/message
# secure tests.
go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1
```

Scope:

- Adds the pending secure-session scan helper boundary for Go
  `internal/runtime/listener/server.go`.
- Covers `pendingConfirmationPeerDIDs` and `readJSONFile`.
- Covers local `p5-e2ee-sessions/*.json` discovery, missing manager/blank
  identity no-op behavior, identity path/glob failure no-op behavior,
  unreadable or malformed JSON skip behavior, `status="pending-confirmation"`
  filtering, nonblank `peer_did` filtering, first-seen duplicate suppression,
  and peer ordering from the scanned entries.
- Keeps the slice helper-only. It does not implement
  `syncPendingConfirmationSecureHistory`, WebSocket/RPC `direct.get_history`
  fetches, secure direct decrypt/ack, SQLite/storage side effects, host-notify
  dispatch, local bridge I/O, or foreground listener session processing.

Dependency note: no dependency was added. The slice uses local filesystem/path
scanning and existing JSON decoding only; it does not add WebSocket crates,
HTTP/TLS clients, OpenSSL, `native-tls`, bundled OpenSSL, E2EE provider crates,
platform service libraries, or new SQLite dependencies.

## 2026-05-16 Runtime Listener Secure Direct Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_notifications_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestHandleNotificationDecryptsSecureDirectIncomingAndStoresPlaintext|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1
```

Scope:

- Adds the secure-direct helper-only boundary for Go
  `internal/runtime/listener/server.go`.
- Covers `isDirectSecureIncomingNotification`,
  `isSecureDirectWireContentType`, `secureNotificationFromMessageView`, and
  `plaintextBodyToNotificationBody`.
- Keeps the slice pure: notification classification, secure direct wire
  content-type recognition, message-view to notification conversion, and
  plaintext-body to notification-body conversion only.
- Go currently covers these helpers through secure listener integration tests;
  Rust adds direct helper-level contract coverage for the helper-only boundary.

Dependency note: no dependency was added. The slice reuses existing JSON/value
and listener helper surfaces in `runtime::listener_secure_notifications`; it
does not add WebSocket crates, HTTP/TLS clients, OpenSSL, `native-tls`, bundled
OpenSSL, E2EE provider crates, platform service libraries, or new SQLite
dependencies.

Boundary note: Go foreground WebSocket runtime processing, direct secure
decrypt/ack, message/group storage side effects, host-notify dispatch, local
bridge I/O, and runtime listener session orchestration are not claimed by this
helper-only slice.

## 2026-05-16 Runtime Listener WebSocket Client Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestNewWSClientDerivesIMWebSocketEndpointFromServiceBaseURL' -count=1
```

Additional parity probes:

```bash
# Go/Rust probes checked fmt("%.0f") half-even rounding and url.Parse Host/error
# behavior used by requestIDFromAny and hostForURL.
```

Scope:

- Adds `runtime::listener_wsclient` as a split helper translation of the
  deterministic, transport-free parts of Go `internal/runtime/listener/wsclient.go`.
- Adds `config::derive_websocket_url` to mirror Go `DeriveWebSocketURL` and
  adjusts `join_base_url` empty-path behavior to match Go `JoinBaseURL`.
- Covers `/im/ws` request/WebSocket endpoint derivation, DID-auth endpoint
  derivation, the Go empty-base `/im/ws` boundary, `requestIDFromAny`
  string/int/float formatting, `int64FromAny` truncation, and `hostForURL`
  `url.Parse` host/fallback behavior.

Dependency note: no dependency was added. This slice deliberately avoids a
WebSocket transport crate, OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, service-manager crates, YAML crates, and new SQLite dependencies.
Later WebSocket transport work must stay Rustls-first and receive a separate
dependency review.

Boundary note: Go `WSClient` construction with auth session side effects,
WebSocket dial/read/write, bearer refresh retry, pending RPC channel handling,
notification buffering, `formatDialError`, foreground listener session
execution, local bridge I/O, host-notify dispatch, and listener SQLite side
effects remain deferred.

## 2026-05-15 Runtime Listener Service Local Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_listener_service_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestWaitForServiceStatusWithWaitsForBridgeAvailability|TestWaitForServiceStatusWithWaitsForExpectedBootID' -count=1
```

Scope:

- Adds `runtime::listener_service` as a split local-helper translation of Go
  `internal/runtime/listener/service.go` deterministic helper behavior before
  selecting platform service-manager integration.
- Covers hashed service name generation, display-name derivation from workspace
  basename, service-mode detection via `AWIKI_LISTENER_SERVICE_MODE` and
  `runtime listener service-run` argv, boot-id generation/persistence/resolve
  fallback, cleanup of runtime pid/status/socket/expected-boot artifacts, and
  `waitForServiceStatusWith`/`serviceStatusReady` bridge plus boot-id readiness
  gates.

Dependency note: no dependency was added. The slice reuses existing `sha2`,
`rand`, std filesystem/env/time APIs, and existing listener file helpers; it
does not add `kardianos/service`, platform service-manager crates, OpenSSL,
`native-tls`, bundled OpenSSL, HTTP/WebSocket crates, YAML crates, or new SQLite
dependencies.

Boundary note: Go `serviceProgram`, `newService`, install/start/stop/restart/
uninstall execution, real OS service status, process supervision, foreground
WebSocket/session execution, and native platform service integration remain
deferred and dependency-reviewed.

## 2026-05-16 Runtime Hermes Host Notification Sink Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'Test(NewHermesHostNotifySinkRejectsInvalidNotifyURL|HermesHostNotifySinkNotifySignsRequest)' -count=1
```

Scope:

- Extends `runtime::hermes_host_notify` from the split helper translation into
  Go `internal/runtime/listener/hermes_host_notify.go` sink construction and
  HTTP delivery behavior.
- Covers HMAC-SHA256 over `timestamp + "." + raw_json_body`, lowercase hex
  signatures, `sha256=` header value construction, Go notify header constants,
  `http`/`https` notify URL validation, host requirement, malformed host/port
  parse rejection, config-file secret precedence, legacy webhook config
  fallback, new and legacy env secret fallback, whitespace trimming, and config
  read-error fallback to env.
- Covers required notify URL and missing-secret errors, 15-second POST timeout
  cap, `Content-Type: application/json`, timestamp/signature request headers,
  raw JSON body signing, 2xx success, non-2xx status/body error strings, and
  no-op close behavior.
- Corrects `host_notify_config_view` Hermes metadata so the legacy env key is
  Go's `AWIKI_HOST_NOTIFY_WEBHOOK_SECRET`, not the older incorrect
  `AWIKI_WEBHOOK_SECRET` label.

Dependency note: no dependency was added. The slice reuses existing `sha2` for
the fixed HMAC-SHA256 helper, the existing hand-written config parser, and the
already selected Rustls/std `transportcfg::HttpClient`; it does not add an
`hmac` crate, OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, YAML crates, platform service libraries, or new SQLite
dependencies.

Boundary note: Go `newHostNotifySink` wiring, `handleNotification`, foreground
session processing, actual SQLite storage, host-notify dispatch, local bridge
I/O, and WebSocket runtime execution are not claimed by this slice.

## 2026-05-16 Runtime Host Notification Sink Dispatcher Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Current result: passed locally. The focused contract test ran 10 tests covering
file sink errors/writes/appends/modes, disabled noop dispatch, sink
normalization, Hermes constructor error propagation, and OpenClaw constructor
dispatch/status. Full package tests, structure check, whitespace check, and
dependency audit passed; audit output stayed limited to the existing
Rustls/ring/sha/base64 paths and the user-approved bundled SQLite path.

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'Test(NewHermesHostNotifySinkRejectsInvalidNotifyURL|HermesHostNotifySinkNotifySignsRequest|BuildOpenClawHookRequestIncludesChannelDelivery|BuildOpenClawEventTextUsesMainAgentSessionFormat|BuildOpenClawEventTextUsesMailFormat|BuildOpenClawHookRequestIncludesMailPrompt)' -count=1
```

Current result: passed locally.

Scope:

- Adds `runtime::host_notify_sink` as a split translation of the sink and
  dispatcher layer from Go `internal/runtime/listener/host_notify.go`.
- Covers `HostNotifySink`-style notify/close dispatch, noop and log sinks, file
  sink creation, append-only JSONL writes, file sync, idempotent close, disabled
  notification noop behavior, status construction, blank sink defaulting to
  `log`, `webhook` alias normalization to `hermes`, unsupported sink errors,
  Hermes constructor dispatch, and OpenClaw constructor dispatch.
- Keeps foreground notification handling separate: Go `handleNotification`,
  foreground WebSocket session processing, SQLite storage side effects, local
  bridge I/O, and OS service execution are not claimed by this slice.

Dependency note: no dependency was added. The slice uses std filesystem
primitives, existing `serde_json`, the existing Hermes sink, and the existing
OpenClaw Rustls/std webhook sink; it does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML crates, platform
service libraries, or new SQLite dependencies.

## 2026-05-15 Runtime OpenClaw Host Notification Builder And Sink Slice

Status: unit verified.

Local Rust verification:

```bash
rustfmt +1.79.0 --edition 2021 --check crates/awiki-cli/src/runtime/openclaw_host_notify.rs crates/awiki-cli/src/runtime/openclaw_webhook.rs crates/awiki-cli/tests/runtime_openclaw_host_notify_contract.rs
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_openclaw_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli openclaw_webhook --locked
```

Current result: passed locally. The focused host-notify contract test ran 11
tests, including local HTTP-server coverage for partial success, all-route
failure aggregation, missing routes, bearer token forwarding, and payload
shape.

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestBuildOpenClawHookRequestIncludesChannelDelivery|TestBuildOpenClawEventTextUsesMainAgentSessionFormat|TestBuildOpenClawEventTextUsesMailFormat|TestBuildOpenClawHookRequestIncludesMailPrompt' -count=1
```

Scope:

- Adds `runtime::openclaw_host_notify` as a split translation of Go
  `internal/runtime/listener/openclaw_host_notify.go` hook-request/event-text
  builders and OpenClaw delivery sink behavior.
- Covers OpenClaw hook request JSON shape (`message`, `name`, `wakeMode`,
  `deliver`, `channel`, `to`), fixed `AWiki` hook name, `wakeMode=now`,
  route channel/to delivery fields, prompt header/security notice, direct,
  group, group-state, mail, and unknown-event prompt/text mapping, mail-like
  direct notification detection, mail metadata/content fallback, direct/group
  fallback content brackets, group-state content summaries, and JSON fallback
  for unknown events.
- Adds `new_openclaw_host_notify_sink`, settings re-resolution, hook URL/client
  preparation, route registry loading, no-route errors, route-by-route HTTP
  delivery, partial-success semantics, and Go-shaped aggregate failure strings.
- Dispatcher construction is covered by the separate host-notify sink row.

Dependency note: no dependency was added. The slice reuses existing `serde` and
`serde_json` plus the existing Rustls/std `transportcfg` HTTP client; it does
not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
crates, YAML crates, platform service libraries, or new SQLite dependencies.

Boundary note: Go `handleNotification`, foreground session processing, actual
SQLite storage, host-notify dispatch from listener notifications, local bridge
I/O, and WebSocket runtime execution are not claimed by this sink slice.

## 2026-05-15 Runtime Host Notification Normalizer Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestNormalizeHostNotificationDirectIncomingKeepsMinimalFields|TestNormalizeHostNotificationGroupIncomingOmitsPayloadBody|TestNormalizeHostNotificationMailNotificationBuildsMailEvent|TestNormalizeHostNotificationGroupStateChangedInfersEventType' -count=1
```

Scope:

- Adds `runtime::host_notify` as a split helper translation of Go
  `internal/runtime/listener/host_notify.go` host-facing notification
  normalization.
- Covers exact method gating for direct/mail/group/group-state notifications,
  Go required field behavior, `version=1.0`, UTC RFC3339 `received_at`,
  direct/mail/group/group-state data shapes, `omitempty` JSON behavior, direct
  `source_kind=im`, mail `source_kind=mail`, mail text fallback ordering,
  group payload omission, string-like group sequence handling for host event
  IDs, SHA-256 generated `hostevt-<16 hex>` fallback IDs, group-state event-type
  inference, and `ApplyHostNotificationHandles` direct/group-only enrichment.
- Keeps the implementation helper-only because Rust does not yet implement the
  Go foreground listener WebSocket/session notification loop from
  `internal/runtime/listener/server.go`.

Dependency note: no dependency was added. The slice reuses existing `serde`,
`serde_json`, `sha2`, and `time`; it does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML crates, platform
service libraries, or new SQLite dependencies.

Boundary note: Go `handleNotification` integration remains deferred: foreground
session processing, incoming contact sync wiring, actual SQLite
message/group/member storage, file/log/OpenClaw/Hermes sink delivery,
host-notify dispatch, local bridge I/O, and WebSocket runtime execution are not
claimed by this helper-only slice.

## 2026-05-15 Runtime Listener Message Parser Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli listener_message_records --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestMessageRecordFromDirectIncomingUsesProtocolFieldsOnly|TestMessageRecordFromDirectIncomingRejectsNonDirectNotification|TestMessageRecordFromMailNotificationBuildsSystemMessage|TestMessageRecordFromGroupIncomingUsesProtocolFieldsOnly|TestRecordsFromGroupStateChangedBuildsMemberAndSystemMessage' -count=1
```

Scope:

- Adds `runtime::listener_message_records` as a split helper translation of
  Go `internal/runtime/listener/server.go` pure direct/mail/group incoming and
  group state-change record parser functions.
- Covers direct and group method/params gating, required field behavior,
  direct/group thread IDs, direct content fallback order, direct E2EE flag
  conditions, group self-sent direction/read status, group message-ID fallback
  including the numeric-seq pitfall, server-seq parsing, Go `text/plain`
  content-type defaults, mail summary/title/source-kind metadata, local mail
  fallback IDs, group state group/member/message record construction, membership
  status/content-type inference, and metadata limited to the Go helper scope.
- Keeps the implementation parser-only because Rust does not yet implement the
  Go foreground listener WebSocket/session notification loop from
  `internal/runtime/listener/server.go`.

Dependency note: no dependency was added. The slice uses `serde_json` and the
existing store helpers already present in the crate; it does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML
crates, platform service libraries, or new SQLite dependencies.

Boundary note: Go `handleNotification` integration remains deferred: foreground
session processing, direct secure decryption, actual SQLite message/group/member
storage, incoming contact sync wiring, host-notify enrichment/dispatch, local
bridge I/O, and WebSocket runtime execution are not claimed by this helper-only
slice.

## 2026-05-15 Runtime Listener Contact Sync Helper Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli listener_contact_sync --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Go reference verification:

```bash
go test ./internal/runtime/listener -run 'TestMessageRecordFromDirectIncomingUsesProtocolFieldsOnly|TestMessageRecordFromGroupIncomingUsesProtocolFieldsOnly|TestNormalizeHostNotificationDirectIncomingKeepsMinimalFields|TestNormalizeHostNotificationGroupIncomingOmitsPayloadBody|TestSessionWarningsReportsDisconnectedSessions|TestHasDisconnectedSessions|TestMergeSavedRuntimeStatus' -count=1
```

Scope:

- Adds `runtime::listener_contact_sync` as a split helper translation of Go
  `internal/runtime/listener/contact_sync.go`.
- Covers empty/self DID no-ops, local handle short-circuit, historical
  contact-handle-binding fallback through the shared store helper, no-remote
  no-op, remote error/nil/blank behavior, Go listener handle normalization,
  and successful direct/group incoming contact upserts with `messaged=true`,
  source metadata, UTC timestamps, and current handle bindings.
- Keeps the implementation helper-only because Rust does not yet implement the
  Go foreground listener WebSocket/session notification loop from
  `internal/runtime/listener/server.go`.

Dependency note: no dependency was added. The slice uses the existing
`rusqlite + bundled` store path and existing std/Rust code; it does not add
OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates,
YAML crates, platform service libraries, or new SQLite dependencies.

Boundary note: Go `server.go` integration remains deferred: foreground session
processing, incoming notification parsing/storage, host-notify handle
enrichment/dispatch, local bridge I/O, and WebSocket runtime execution are not
claimed by this helper-only slice.

## 2026-05-15 Direct Message Contact Sync Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test store_contact_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Go reference verification:

```bash
go test ./internal/store ./internal/message -run 'TestUpsertContactRebindsCurrentHandleAndPreservesHistory|TestListDIDsByHandleFallsBackToContactsWithoutHistoryBindings|TestListDirectMessagesByPeerDIDsFiltersUnreadInboxOnlyAndDeduplicates|TestSyncPeerHandle|TestReadHistoryFromCacheByPeerDIDsAggregatesHistoricalBindings' -count=1
```

Scope:

- Adds the Go `store` contact helper boundary in split Rust files:
  `ResolveContactHandleByDID`, `ListDIDsByHandle`, `UpsertContact`, and
  `ListDirectMessagesByPeerDIDs`.
- Adds message contact-sync helpers for inbox/history peer DID handling,
  local contact handle reuse, DID->handle remote lookup fallback, `wba://`
  and domain-trimming handle normalization, and handle-history DID merging.
- Wires direct inbox/history persistence to sync contacts and expands
  `msg history --with <handle>` through local handle-history cache rows.
- Keeps direct send contact behavior Go-compatible: send persists the outbound
  message but does not upsert `contacts`.

Dependency note: no dependency was added. The slice reuses approved
`rusqlite + bundled` for local SQLite, existing Rustls/std HTTP for remote
handle lookup, and existing auth/session wiring. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, YAML
crates, platform service libraries, or new SQLite dependencies.

Boundary note: runtime listener foreground/server wiring for incoming contact
sync remains a separate listener/server execution slice because the current Rust
runtime listener lane does not yet implement the Go foreground session/message
processing loop. Secure direct E2EE, WebSocket/local bridge fallback, and
deeper fallback trace phase parity remain deferred.

## 2026-05-15 Trace Timing Integration Slice

Status: unit verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test traceutil_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract trace_timing --locked
cargo +1.79.0 test -p awiki-cli --test mail_live_contract mail_inbox_trace_timing_reports_remote_rpc_phase --locked -- --exact
cargo +1.79.0 test -p awiki-cli --test mail_live_contract mail_inbox_trace_timing_reports_bootstrap_jwt_without_nested_get_me_rpc --locked -- --exact
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Local result: passed. Formatting, whitespace, package check, focused trace,
authsdk, and mail live contract tests, structure check, binary build, full
`awiki-cli` package tests, and dependency audit passed. `xtask
check-structure` reported no undocumented Rust files over 1200 lines.
Dependency audit showed only existing allowed hits: `base64`, `rustls`,
`rustls-webpki`, `webpki-roots`, `ring`, approved `rusqlite`,
`libsqlite3-sys`, and build helpers `cc`, `pkg-config`, and `vcpkg`; no
OpenSSL, `native-tls`, `reqwest`, `hyper`, WebSocket, YAML, platform service,
or new SQLite dependency was introduced.

Scope:

- `AWIKI_CLI_TRACE_TIMING` now creates a command trace run after CLI parse,
  records `resolve_config`, and emits the Go-compatible Chinese timing block
  to stderr after rendered success/error output.
- Raw completion output is exempt from trace stderr, matching Go completion
  handlers that bypass `renderSuccess`.
- Shared authenticated RPC/plain JSON helpers record Go-shaped remote RPC
  phases by RPC method or REST method/endpoint.
- DID-auth JWT refresh records caller-labeled `EnsureJWTPhase` names such as
  `mail_bootstrap`, `content_bootstrap`, `site_bootstrap`,
  `identity_refresh_token`, `identity_bootstrap`, `message_bootstrap`, and
  `message_service_retry`, while suppressing a nested `business_rpc:get_me`
  phase during the internal `get_me` request.
- Tests cover success JSON stdout plus trace stderr, JSON error prefix plus
  trace stderr, raw completion without trace stderr, mail RPC trace method
  labeling, and empty-token mail bootstrap JWT labeling.

Boundary note: local DB, handle lookup, contact/cache sync, and fallback trace
phase call sites are not fully threaded through every translated Go service
path yet. That remains trace-depth parity work and must not be mixed with
optimizations.

Parallelism note: a read-only Native Agent reviewed this trace slice and
flagged the JWT phase-label/nested-RPC and completion raw-output regressions.
A previous test-only code-writing Native Agent for this slice used GPT-5.5
xhigh under a bounded non-overlapping test scope.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses local traceutil and the existing Rustls/std
authsdk transport, and does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, YAML crates, platform service libraries,
or new SQLite dependencies.

## 2026-05-15 Direct/Group Attachment Live HTTP Slice

Status: system verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test attachment_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked
cargo +1.79.0 test -p awiki-cli --lib transportcfg::http::tests::close_delimited_response_is_complete_after_headers_like_go_net_http --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Local result: passed. Focused attachment contract coverage passed for group
attachment send, group attachment download, direct attachment send, attachment
error mapping, and non-dry-run forced-HTTP warnings when `runtime.mode` is
`websocket` for both attachment send and download. Full `awiki-cli` package
tests passed with no failed tests and no ignored/skipped lines in the output.
`xtask check-structure` reported
`structure ok: no undocumented Rust files over 1200 lines`. Binary build passed.
Dependency audit showed only allowed existing hits: `base64`, `rustls`,
`rustls-webpki`, `webpki-roots`, `ring`, approved `rusqlite`,
`libsqlite3-sys`, and build helpers `cc`, `pkg-config`, and `vcpkg`; no
OpenSSL, `native-tls`, `reqwest`, `hyper`, WebSocket, YAML, or platform service
dependency was present.

Focused remote system-test verification:

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
E2E_DID_DOMAIN=awiki.info \
NO_PROXY=awiki.info,www.awiki.info,localhost,127.0.0.1 \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
uv run --no-sync python -m pytest \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_send_and_download_group_attachments \
  -q
```

System-test result: passed, 1 passed, 0 failed, 0 skipped in 3.23s. Failed
cases: none. Skipped cases: none. Configuration context:
`AWIKI_SYSTEM_TEST_MODE=remote`,
`E2E_USER_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws`,
`E2E_DID_DOMAIN=awiki.info`, `AWIKI_CLI_UNDER_TEST=rust`,
`AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2`, and
`NO_PROXY=awiki.info,www.awiki.info,localhost,127.0.0.1`.

Scope:

- Live direct attachment send through `msg send --to ... --file ...`.
- Live group attachment send through `msg send --group ... --file ...`.
- Live direct/group attachment download through `msg attachment download
  --with/--group ...`.
- Attachment slot creation, object upload, object commit, manifest send,
  history-page attachment selection, download-ticket request, object download,
  output write, and direct/group cache updates where applicable.
- Attachment send/download force HTTP transport and warn when
  `runtime.mode=websocket`.

Dependency evidence:

- Reuses the existing Rustls/std `transportcfg::HttpClient` and
  `authsdk::Session`.
- Reuses the local ANP origin-proof helper and existing attachment wire/service
  discovery helpers.
- Reuses approved `rusqlite + bundled` SQLite for cache persistence.
- No new OpenSSL, `native-tls`, `reqwest`, `hyper`, WebSocket, YAML, platform
  service, or new SQLite dependency is introduced by this slice.

Boundary note: secure direct E2EE attachments, group E2EE/MLS attachments,
WebSocket/local bridge attachment transport, trace phase plumbing, and
optimization/refactor work remain later parity slices. Shared profile timeout
caps are covered by the later service profile-timeout slice.

## 2026-05-15 Non-E2EE Group Live HTTP Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_groups_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
```

Focused remote system-test verification:

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
E2E_DID_DOMAIN=awiki.info \
NO_PROXY=awiki.info,www.awiki.info,localhost,127.0.0.1 \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
uv run --no-sync python -m pytest \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_create_group_add_member_send_and_list_messages \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_update_members_remove_and_leave_groups \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_join_open_group_and_use_show_alias \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_get_returns_not_found_for_unknown_group \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: local Rust formatting, package check, focused message/group/store tests,
and structure check passed. The focused remote `awiki-system-test` group
selector set passed against `awiki.info`: 5 passed, 0 failed, 0 skipped in
8.48s. Configuration context: `AWIKI_SYSTEM_TEST_MODE=remote`,
`E2E_USER_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws`, and
`E2E_DID_DOMAIN=awiki.info`.

Scope:

- Wired ordinary non-E2EE group execution for `group create`, `group get`,
  `group show`, `group join`, `group add`, `group remove`, `group kick`,
  `group leave`, `group update`, `group list`, `group members`,
  `group messages`, and `msg send --group --text`.
- Preserved Go auth/session behavior through the existing `authsdk::Session`:
  active messaging identity gate, stored JWT seeding, DID-auth `get_me`
  bootstrap when no token is stored, captured JWT persistence, and service
  error conversion.
- Preserved Go message-service HTTP behavior for `/im/rpc` group methods,
  including member handle-to-DID lookup, group text send, list/read methods,
  update-profile/update-policy sequencing, and Go-shaped result summaries.
- Added split local group cache helpers in `store/groups.rs` for group,
  member, and group-message persistence plus local owner leave handling.
- Fixed signed message/group metadata timestamps to Go-compatible
  second-precision RFC3339 UTC text so message-service typed `Meta`
  reserialization verifies RFC9421 origin-proof `contentDigest`.
- Kept all touched Rust source/test files below the default 1200-line limit;
  `xtask check-structure` reports no undocumented oversized files.

Boundary note: group attachment send/download is covered by the later
attachment live HTTP slice recorded above. Group E2EE/MLS execution,
WebSocket/local bridge/runtime listener fallback, OpenClaw host notify, trace
phase plumbing, and deeper cache fallback behavior remain later parity slices.
Shared profile timeout caps are covered by the later service profile-timeout
slice.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, shared Rustls/std HTTP client,
authsdk session, message wire/proof helpers, and approved `rusqlite + bundled`
SQLite path. It does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP
SDK network/default features.

## 2026-05-15 Direct Message Live HTTP Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
```

Focused remote system-test verification:

```bash
cd ../awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
E2E_DID_DOMAIN=awiki.info \
NO_PROXY=awiki.info,www.awiki.info,localhost,127.0.0.1 \
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 \
uv run --no-sync python -m pytest \
  tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_can_send_direct_messages_and_mark_them_read \
  tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_inbox_scope_all_limit_and_mark_read_work \
  -q
```

Result: passed. Local Rust formatting, whitespace, package check, focused
message/authsdk tests, full package test, structure check, and binary build
passed. The two remote direct-message system-test selectors passed against
`awiki.info`.

Scope:

- Wired ordinary direct text message execution for `msg send --to
  --text/--text-file`, `msg inbox`, `msg history --with`, and
  `msg mark-read`.
- Preserved Go auth/session behavior through the existing `authsdk::Session`:
  active messaging identity gate, stored JWT seeding, DID-auth `get_me`
  bootstrap when no token is stored, captured JWT persistence, and service error
  conversion.
- Preserved Go message service HTTP behavior for `/im/rpc` methods
  `direct.send`, `inbox.get`, `direct.get_history`, and `inbox.mark_read`,
  including handle-to-DID lookup through the user-service handle RPC.
- Added split local cache helpers in `store/messages.rs` for Go-shaped message
  upserts, batch persistence, local read filters, and mark-read mutation.
- Kept all touched Rust source/test files below the default 1200-line limit;
  `xtask check-structure` reports no undocumented oversized files.

Boundary note: direct attachment send/download is covered by the later
attachment live HTTP slice recorded above. Secure direct E2EE, group
lifecycle/messages, WebSocket/local bridge/runtime listener transport, OpenClaw
host notify, and trace phase plumbing remain later parity slices. Shared
profile timeout caps are covered by the later service profile-timeout slice.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, shared Rustls/std HTTP client,
authsdk session, message wire/proof helpers, and approved `rusqlite + bundled`
SQLite path. It does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP
SDK network/default features.

## 2026-05-15 Identity Email Register Live Slice

Local Rust, Go reference, dependency, and focused system-test verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test identity_register_email_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/cmdmeta -run 'TestServiceRegisterEmail|TestServiceRegisterFullHandleUsesExplicitDomainForDID|TestServiceRegisterPhoneSendsNormalizedOTPRequest|TestRunIDRegisterDryRun|TestCatalogPublishesIDRegister|TestMail|Test.*Register' -count=1
cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws E2E_DID_DOMAIN=awiki.info NO_PROXY=awiki.info,www.awiki.info,localhost,127.0.0.1 AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_register_supports_phone_otp_send_and_email_activation -q
```

Result: Rust focused email-register live tests passed for unauthenticated
`email-status` plus `email-send`, no-wait activation output without local
identity writes, already-verified `--wait` registration without sending a new
activation email, and send-then-poll-then-register ordering. Existing identity
live, identity wire, and identity contract tests passed, preserving phone
register and authenticated bind/profile/resolve behavior. The focused
`awiki-system-test` selector passed against `awiki.info`, covering the system
visible `id register --handle --email` activation bootstrap path.

Scope:

- Wired Go `Service.Register` email activation into Rust `identity::register`
  using the existing identity REST/RPC builders: first
  `/user-service/auth/email-status` with the full handle query, then
  `/user-service/auth/email-send` when unverified, then optional wait polling
  and final `/user-service/did-auth/rpc` `register` with `method=email`.
- Preserved Go no-wait semantics: sending an activation email returns
  `send_registration_email` with `verification_state=email_sent` and does not
  create local identity state.
- Preserved Go `--wait` semantics for already-verified and newly verified
  email addresses, including skipping duplicate email sends and persisting the
  final registered identity/JWT after `did-auth.register`.
- Kept the implementation in existing split modules under the 1200-line rule:
  `identity/service.rs` 1053 lines, `identity/client.rs` 178 lines,
  `app.rs` 1086 lines, and new
  `identity_register_email_live_contract.rs` 442 lines.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, shared Rustls/std HTTP client,
identity wire builders, and service error conversion. It does not add
`reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL,
YAML crates, platform service libraries, or ANP SDK network/default features.

Boundary note: profile-timeout wrappers, trace phase emission, legacy
ANP-labeled k1 PEM compatibility conversion, workspace-migration-driven k1
replacement, and message/group service execution remain deferred parity slices.

## 2026-05-15 Identity Replace-DID Live Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test identity_replace_did_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_recover_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/store ./internal/authsdk ./internal/cmdmeta -run 'Test.*ReplaceDID|TestRunIDReplaceDIDDryRunWarnsAndTargetsIdentity|TestCatalogPublishesPublicDangerousReplaceDIDCommand|TestRebind|TestClearOwnerE2EEData|TestRefreshTokenUsesDIDAuthWithoutStoredBearerAndPersistsNewJWT|TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup -q
```

Result: Rust focused replace-DID live tests passed, including authenticated
`replace_did`, optional flag/null payload mapping, no-JWT `get_me` bootstrap,
store rebind/E2EE cleanup output, and backup-failure-before-remote ordering.
Existing identity live, identity wire, and store rebind focused tests passed
during the slice. Structure check passed with no undocumented Rust source/test
file over 1200 lines: `app.rs` 1084 lines,
`app/id_replace_did_handlers.rs` 95 lines, `identity/replace_did.rs` 417
lines, `identity/service.rs` 1014 lines, and
`identity_replace_did_live_contract.rs` 744 lines.

Scope:

- Wired non-dry-run `id replace-did` through the split
  `app/id_replace_did_handlers.rs` CLI layer and new
  `identity/replace_did.rs` service layer, keeping large existing files under
  the project file-size threshold.
- Preserved Go dry-run and live flag behavior for `--is-public`,
  `--is-agent`, changed `--role`, and changed `--endpoint-url`, including JSON
  `null` for empty role/endpoint values in the live `replace_did` RPC.
- Preserved Go live service ordering: load selected/default identity, require a
  handle-backed DID, generate a handle-path e1 DID/key bundle, create a
  `.legacy-backup/replace-did` backup under the identity store before auth or
  remote mutation, call authenticated `/user-service/did-auth/rpc`
  `replace_did`, write the new identity material, remove stale e2ee state, and
  remove the old identity directory after index update.
- Preserved Go auth behavior: stored bearer is reused when present; when JWT is
  missing, DID-auth `get_me` bootstrap runs first and the fresh token is used
  for `replace_did`.
- Preserved CLI-owned local SQLite post-processing:
  `store::rebind_local_identity_state_with_partial` exposes `store_rebind` and
  `e2ee_cleanup`; local SQLite failures keep the command successful after
  service replacement and append the Go-shaped warning while preserving partial
  count output.
- Preserved the public dangerous warning and public output sanitization.

Follow-up note: the 2026-05-15 identity key compatibility slice below now
translates Go's load-time legacy ANP and SEC1 private-key PEM migration.
Workspace-migration-driven k1 replacement remains deferred under the workspace
v2->v3 compatibility lane. Live email registration/wait polling,
profile-timeout wrappers, trace phase emission, and message/group service
execution remain deferred identity/transport parity slices.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, shared Rustls/std HTTP client,
authsdk session, identity wire builders, and approved `rusqlite + bundled`
SQLite path. It does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP
SDK network/default features.

## 2026-05-15 Identity Recover Live Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_recover_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/store ./internal/authsdk ./internal/cmdmeta -run 'Test.*Recover|TestRecoverStagesAndFinalizesSameHandleLiveIdentities|TestMergeRecoveredHandleLocalState|Test.*Bind|Test.*Profile|Test.*Resolve' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_register_resolve_profile_bind_and_recover_flow tests_v2/id/test_identity_cli.py::test_id_bind_email_send_requires_auth_and_supports_registered_identity tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh tests_v2/id/test_identity_cli.py::test_id_refresh_token_replaces_stale_local_jwt_for_registered_identity tests_v2/id/test_identity_cli.py::test_id_profile_set_rejects_conflicting_body_sources -q
```

Result: Rust focused recover, identity live, identity contract, identity wire,
and full `cargo +1.79.0 test -p awiki-cli --locked` all passed. Structure
check passed with no undocumented Rust source/test file over 1200 lines:
`app.rs` 1102 lines, `identity/recover.rs` 652 lines,
`identity/service.rs` 1014 lines, `identity_live_contract.rs` 1081 lines, and
the new `identity_recover_live_contract.rs` 461 lines. Go focused
recover/bind/profile/resolve/store/authsdk/cmdmeta tests and the focused
`awiki-system-test` id selector passed after final verification.

Scope:

- Wired non-dry-run `id recover` through `identity::recover` and the split
  `app/id_recover_handlers.rs` finalization layer, keeping `app.rs` under the
  project file-size threshold.
- Preserved Go no-OTP behavior: `/user-service/handle/rpc` `send_otp`, phone
  normalization, `send_recover_otp` result shape, and no local identity writes.
- Preserved Go OTP behavior: generate a handle-path e1 DID, create a recover
  backup under `.legacy-backup/recover-handle`, call
  `/user-service/did-auth/rpc` `recover_handle`, stage the recovered identity as
  a temporary identity, and persist recovered handle/full_handle/user_id/JWT.
- Wired CLI-owned finalization: merge old-owner SQLite state through
  `store::merge_recovered_handle_local_state`, promote the temporary identity
  to the final identity name, remove archived same-handle identities from the
  live index, update active/default identity state as Go does, expose
  `store_merge_counts` and `e2ee_cleanup_counts`, and hide `temp_identity_name`,
  `active_before`, and `old_dids` from public output.
- Preserved Go warnings for archived same-handle identities and ignored
  `--identity`; recover-specific merge/finalization failures include
  `backup_path`, `temp_identity_name`, and `new_did` details.
- Split live recover tests into `identity_recover_live_contract.rs` instead of
  growing `identity_live_contract.rs` past the 1200-line default.

Boundary note: non-dry-run `id replace-did`, live email registration/wait
polling, profile-timeout wrappers, trace phase emission, and message/group
service execution remain deferred identity/transport parity slices.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, shared Rustls/std HTTP client,
identity wire builders, and approved `rusqlite + bundled` SQLite path. It does
not add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default
features.

## 2026-05-15 Identity Bind Live Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/authsdk ./internal/cmdmeta -run 'Test.*Bind|Test.*Profile|Test.*Resolve|TestRunIDRefreshTokenDryRunPlansDidAuthRefresh|TestRefreshTokenUsesDIDAuthWithoutStoredBearerAndPersistsNewJWT|TestCatalogPublishesRefreshTokenCommand|TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_register_resolve_profile_bind_and_recover_flow tests_v2/id/test_identity_cli.py::test_id_bind_email_send_requires_auth_and_supports_registered_identity tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh tests_v2/id/test_identity_cli.py::test_id_refresh_token_replaces_stale_local_jwt_for_registered_identity tests_v2/id/test_identity_cli.py::test_id_profile_set_rejects_conflicting_body_sources -q
```

Result: local Rust checks passed, including the full
`cargo +1.79.0 test -p awiki-cli --locked` run. Focused Go bind/profile/resolve,
identity/authsdk, and cmdmeta tests passed. The dependency audit stayed on the
existing expected paths only: `base64`, approved `rusqlite`/`libsqlite3-sys`
with build helpers, and the existing Rustls/webpki/ring TLS path. File-size
check passed with `identity_live_contract.rs` at 1081 lines and no undocumented
Rust source file over the 1200-line default.

Focused `awiki-system-test` result: 4 passed, 1 failed at the next Rust port
gap. The end-to-end identity flow now progresses through live `id register`,
profile set/get, resolve, and the `id bind --phone ... --email ...`
`invalid_argument` validation. The focused registered-identity email bind test
also passes. The remaining failing selector stops at non-dry-run `id recover`,
which still returns the explicit deferred `not_implemented` error.

Scope:

- Wired `id bind` command parsing, cmdmeta, and dispatch for `--phone`,
  `--email`, `--otp`, and `--wait`.
- Preserved Go dry-run plan shape for phone OTP send, phone verify, email send,
  and email wait flows.
- Implemented live phone bind REST calls:
  `POST /user-service/auth/phone-bind-send` and
  `POST /user-service/auth/phone-bind-verify`, using authenticated DID-WBA JSON
  requests, Go phone normalization, and whitespace-stripped OTP codes.
- Implemented live email bind REST calls:
  authenticated `GET /user-service/auth/email-status` with bearer auth and no
  bind-flow `handle` query, 404-as-unverified behavior, authenticated
  `POST /user-service/auth/email-send`, no-wait `email_sent`, wait/pending, and
  already-verified `completed` result shapes.
- Added focused fake-server coverage for phone send, phone verify, email send,
  and already-verified email wait.

Boundary note: non-dry-run `id recover`, non-dry-run `id replace-did`, live
email registration/wait polling, profile-timeout wrappers, and trace phase
emission remain deferred identity/transport parity slices.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, `authsdk::Session`, and
Rustls/std `transportcfg::HttpClient`. It does not add `reqwest`, `hyper`,
WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform
service libraries, or ANP SDK network/default features.

## 2026-05-15 Identity Profile And Resolve Live Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/authsdk ./internal/cmdmeta -run 'Test.*Profile|Test.*Resolve|TestRunIDRefreshTokenDryRunPlansDidAuthRefresh|TestRefreshTokenUsesDIDAuthWithoutStoredBearerAndPersistsNewJWT|TestCatalogPublishesRefreshTokenCommand|TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_register_resolve_profile_bind_and_recover_flow tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh tests_v2/id/test_identity_cli.py::test_id_refresh_token_replaces_stale_local_jwt_for_registered_identity tests_v2/id/test_identity_cli.py::test_id_profile_set_rejects_conflicting_body_sources -q
```

Result: local Rust checks passed, including the full
`cargo +1.79.0 test -p awiki-cli --locked` run. Focused Go profile/resolve,
identity/authsdk, and cmdmeta tests passed. The dependency audit stayed on the
existing expected paths only: `base64`, approved `rusqlite`/`libsqlite3-sys`
with build helpers, and the existing Rustls/webpki/ring TLS path.

Focused `awiki-system-test` result: 3 passed, 1 failed at the known next Rust
port gap. The end-to-end identity flow now progresses through live
`id register`, `id profile set`, `id profile get --self`,
`id profile get --handle`, `id profile get --did`, `id resolve --handle`, and
`id resolve --did`; it then fails at `id bind --phone ... --email ...` because
the Rust port still returns `not_implemented` for `id bind`.

Scope:

- Wired non-dry-run `id profile set` to authenticated
  `/user-service/did/profile/rpc` `update_me`, including stored-JWT session
  seeding, empty-token DID-auth bootstrap, Go profile payload field mapping,
  raw `--markdown-file` content preservation, and local display-name writeback
  after remote success.
- Wired non-dry-run `id profile get` for self, handle, and DID targets:
  authenticated `get_me` for `--self`, handle lookup plus public profile for
  `--handle`, and direct public profile lookup for `--did`.
- Wired non-dry-run `id resolve` for handle and DID targets, including Go's
  handle normalization, handle lookup, profile best-effort warnings, resolve
  RPC calls, and command summary shape.
- Added command parser and cmdmeta entries for `id profile get` and
  `id resolve`, plus contract tests for command schema exposure.

Boundary note: live `id bind`, non-dry-run `id recover`, non-dry-run
`id replace-did`, live email registration/wait polling, profile-timeout
wrappers, and trace phase emission remain deferred identity/transport parity
slices.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK, `authsdk::Session`, and
Rustls/std `transportcfg::HttpClient`. It does not add `reqwest`, `hyper`,
WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform
service libraries, or ANP SDK network/default features.

## 2026-05-15 Identity Refresh Token Live Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_live_contract identity_refresh_token_live_posts_signed_get_me_and_persists_jwt_like_go --locked -- --nocapture
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/cli ./internal/identity ./internal/authsdk ./internal/cmdmeta -run 'TestRunIDRefreshTokenDryRunPlansDidAuthRefresh|TestRefreshTokenUsesDIDAuthWithoutStoredBearerAndPersistsNewJWT|TestCatalogPublishesRefreshTokenCommand|TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh tests_v2/page -q
```

Result: passed. Focused `identity_live_contract` passed with 3 passed, 0
failed, 0 skipped; the new refresh-token live test passed and verifies the
signed DID-auth refresh path. Full `cargo +1.79.0 test -p awiki-cli --locked`
passed. Focused `awiki-system-test` id/page selector passed with 4 passed, 0
failed, 0 skipped in 6.21s.

Scope:

- Wired non-dry-run `id refresh-token` through a translated live identity
  service path instead of returning `not_implemented`.
- Preserved Go identity selection: explicit `--identity` wins, otherwise the
  resolved active/default identity is loaded for mutation.
- Preserved Go refresh auth behavior: create an auth session with an empty
  initial JWT, remember service and did-auth scopes, do not seed the stale
  stored bearer, and POST signed JSON-RPC `get_me` to
  `/user-service/did-auth/rpc`.
- Reused `authsdk::Session::ensure_jwt` so result `access_token`,
  `Authentication-Info` tokens, and legacy response `Authorization: Bearer`
  tokens follow the same capture/persistence order as Go.
- Persisted the fresh JWT through `Manager::update_jwt` and rendered the Go
  `refresh_token` result shape with `previous_token_present` and
  `did_auth_get_me_without_stored_bearer`.
- Added focused fake-server coverage that first registers a local identity,
  rewrites `auth.json` to `stale-token`, runs `id refresh-token`, asserts the
  refresh request has signature headers and no stale bearer, and verifies the
  stored JWT becomes `fresh-token`.

Boundary note: this slice does not implement live email registration/wait
polling, non-dry-run `id recover`, non-dry-run `id replace-did`, real profile
RPC execution, per-call profile-timeout wrappers, or trace phase emission. Those
remain later identity/transport parity slices.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK plus Rustls/std transport and does
not add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default
features.

## 2026-05-15 Tenant Site Live RPC Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test site_contract --locked
cargo +1.79.0 test -p awiki-cli --test site_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test site_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/site ./internal/cli -run 'Test.*Site|TestGetRootCallsSiteRPC|TestDeletePageMapsRPCError|TestNormalizeDomainRejectsURLs' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/multi_tenant/test_awiki_cli_tenant_config.py -q
```

Result: passed. Focused `site_live_contract` passed with 4 passed, 0 failed,
0 skipped. Focused `awiki-system-test` multi-tenant CLI acceptance passed with
2 passed, 0 failed, 0 skipped in 1.91s. Command:
`AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/multi_tenant/test_awiki_cli_tenant_config.py -q`.
Configuration context: the selected tests use isolated local CLI workspaces and
do not call live remote services; direct `awiki-cli site ...` tests do not
currently exist in tests_v2.

Scope:

- Added split `site::client` and `site::service` modules for the live execution
  portion of Go `internal/site/service.go`.
- Preserved `/site/rpc` execution for `get_root`, `set_root`, `list_pages`,
  `get_page`, `create_page`, `update_page`, `rename_page`, and `delete_page`
  using the existing site wire builders and result renderers.
- Reused active identity loading, `authsdk::Session`, stored JWT bearer seeding,
  empty-token DID-auth `get_me` bootstrap, persisted JWT update, and the shared
  Rustls/std `transportcfg::HttpClient`.
- Wired non-dry-run `site root/page` commands through the live site service and
  mapped Go site service errors to `invalid_argument`, `auth_required`,
  `forbidden`, `not_found`, `conflict`, and `internal_error` exits.
- Added live contract tests for authenticated `/site/rpc` JSON-RPC payloads,
  domain/slug/body param normalization, RPC forbidden mapping, and initially
  empty JWT bootstrap plus persisted token reuse.

Boundary note: this slice does not implement Go's per-call profile timeout
wrappers, trace phase emission, or direct `awiki-system-test` site lifecycle
coverage. Those remain later parity work.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing Rustls-first authsdk transport and does not add
`reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL,
YAML crates, platform service libraries, or ANP SDK network/default features.

## 2026-05-15 Identity Phone Register And Page System Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_live_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/identity ./internal/cli -run 'Test.*Register|TestRunIDRegister|TestServiceRegister' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/page -q
```

Result: passed. Focused `awiki-system-test` page acceptance passed with
3 passed, 0 failed, 0 skipped in 5.43s. Command:
`AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/page -q`.
Configuration context: `AWIKI_SYSTEM_TEST_MODE=remote`,
`E2E_USER_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_URL=https://awiki.info`,
`E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws`, and
`E2E_DID_DOMAIN=awiki.info`.

Scope:

- Added live `id register --handle --phone` OTP-send execution through
  `/user-service/handle/rpc` method `send_otp`; this writes no local identity.
- Added live `id register --handle --phone --otp` execution through
  `/user-service/did-auth/rpc` method `register`; it generates a handle-path
  e1 DID, sends normalized phone and sanitized OTP, saves handle/full_handle,
  user_id, JWT, DID document, and key material, and returns Go-shaped command
  results.
- Added a split unauthenticated identity service client that reuses the shared
  Rustls/std `transportcfg::HttpClient` and existing identity wire builders.
- Extended DID generation with a path-segment helper so live registration uses
  Go's handle path prefix instead of the local-only `user` path.
- Updated the shared authsdk JSON-RPC decoder to treat `"error": null` as no
  error, matching Go's nullable error-pointer behavior observed from
  `awiki.info`.
- Updated the shared Rustls HTTP reader to tolerate missing TLS `close_notify`
  only after a complete HTTP-framed response has already been read, matching
  Go's effective behavior while preserving incomplete-response errors.
- Added `identity_live_contract` coverage for live phone OTP-send, register
  payload shape, handle-path DID generation, local persistence, and no-write
  OTP behavior.

Boundary note: this slice does not implement live email registration/wait
polling, non-dry-run `id replace-did`, real profile RPC execution, per-call
profile-timeout wrappers, or trace phase emission. Live `id refresh-token` is
covered by the later Identity Refresh Token Live Slice.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing local ANP Rust SDK plus Rustls/std transport and does
not add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default
features.

## 2026-05-15 Page Content Live RPC Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test page_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_contract --locked
cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/content ./internal/cli -run 'Test.*Page|Test.*Content|TestService' -count=1
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run --no-sync python -m pytest tests_v2/page -q
```

Result: local Rust and Go reference checks passed. At the time of this page
slice, focused `awiki-system-test` page acceptance failed in the prerequisite
`id register` step because live identity registration was not yet implemented.
The later "Identity Phone Register And Page System Slice" above resolves that
blocker and records `tests_v2/page` passing with 3 passed, 0 failed, 0 skipped
against `awiki.info`.

Scope:

- Added split `content::client` and `content::service` modules for the live
  execution portion of Go `internal/content/service.go`.
- Preserved `/content/rpc` execution for `create`, `list`, `get`, `update`,
  `rename`, and `delete` using existing content wire builders and result
  renderers.
- Reused active identity loading, `authsdk::Session`, stored JWT bearer
  seeding, empty-token DID-auth `get_me` bootstrap, persisted JWT update, and
  the shared Rustls/std `transportcfg::HttpClient`.
- Wired non-dry-run `page create/list/get/update/rename/delete` through the
  live content service and mapped Go content service errors to
  `invalid_argument`, `auth_required`, `not_found`, `conflict`, and
  `internal_error` exits.
- Added live contract tests for authenticated `/content/rpc` JSON-RPC payloads,
  create param normalization, RPC not-found mapping, and initially empty JWT
  bootstrap plus persisted token reuse.

Boundary note: this slice does not implement Go's per-call profile timeout
wrappers, trace phase emission, message service RPC/WebSocket execution, or full
all-domain `awiki-system-test` acceptance. Tenant site live RPC is covered by
the later "Tenant Site Live RPC Slice" above.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice reuses the existing Rustls-first authsdk transport and does not add
`reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL,
YAML crates, platform service libraries, or ANP SDK network/default features.

## 2026-05-16 Runtime Bridge Unix I/O Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|kardianos|service-manager|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime -run 'TestResolveShortensLongSocketPath|TestResolveKeepsShortSocketPath|TestResolveDefaultsToWebSocketMode' -count=1
cd ../awiki-cli && go test ./internal/message -run 'TestWSProxyTransportCallsLocalBridgeAndDecodesResponses|TestWSProxyTransportWrapsBridgeFailures' -count=1
```

Result: passed.

Scope:

- Added Unix local bridge I/O parity on top of the existing endpoint helper:
  `listen_bridge`, `bridge_health_probe`, private dial helper, and
  `call_local_bridge`.
- Preserved Go's `CallLocalBridge` sequence: websocket-mode validation, blank
  socket-path validation, endpoint preparation, separate health-probe
  connection, separate request connection, newline-delimited JSON request,
  write/read deadlines from `transportcfg::resolve`, response JSON decode, empty
  result map fallback, and `BridgeCallError` phase/display mapping for
  `bridge_health_probe`, `bridge_dial`, `bridge_write`, and `bridge_read`.
- Preserved Go's Unix `ListenBridge` behavior: parent-directory creation,
  stale socket path removal, and Unix listener bind without adding a new
  platform dependency.
- Tightened bridge JSON shape parity so missing request/response fields and
  `result:null` deserialize to Go-style zero values.
- Added Unix-only contract tests for the two-connection probe/request flow,
  failure response mapping, missing error details, invalid JSON decode errors,
  missing health probe target, health-probe direct failure, and stale socket
  replacement.

Boundary note: Windows named-pipe I/O remains intentionally deferred; this slice
keeps Windows endpoint validation/defaults but does not add a named-pipe crate or
other platform library. Foreground WebSocket listener service execution,
`handleBridgeRequest`, message service WS-proxy wiring, local bridge use from
real CLI message commands, trace phase emission, and `awiki-system-test`
foreground runtime acceptance are not claimed by this slice.

No dependency was added. Cargo manifests and lockfile remain unchanged; this
slice stays within the standard library plus existing `serde_json`, `sha2`, and
transport timeout helpers. It does not add OpenSSL, `native-tls`, bundled
OpenSSL, WebSocket crates, `reqwest`, `hyper`, YAML crates, platform service
libraries, Windows named-pipe crates, or new SQLite dependencies. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Runtime Bridge Endpoint Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
cd ../awiki-cli && go test ./internal/runtime -run 'TestResolveShortensLongSocketPath|TestResolveKeepsShortSocketPath|TestResolveDefaultsToWebSocketMode' -count=1
```

Result: passed.

Scope:

- Added a split `runtime::bridge` module for local websocket bridge endpoint
  helpers and request/response/error shapes.
- Preserved Go's Unix default `<state_dir>/message-daemon.sock` endpoint with
  workspace runtime fallback, Unix long socket path shortening to temp
  `awiki-cli-<sha256-prefix>.sock`, endpoint preparation error prefix,
  endpoint availability helper, and bridge error display strings.
- Added cfg-gated Windows named-pipe default/normalization/preparation shape
  without introducing a Windows named-pipe crate in this local helper slice.
- Routed `runtime::resolve` and listener path/status helpers through the split
  bridge helper so public runtime output uses the Go-shaped normalized endpoint.

Boundary note: this slice does not implement `CallLocalBridge`, Unix socket
dial/listen, Windows named-pipe I/O, bridge health probes, request deadlines,
listener foreground server execution, or WebSocket service execution. Those
remain in later runtime/message service slices.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add HTTP/TLS clients, WebSocket crates, OpenSSL, `native-tls`,
bundled OpenSSL, YAML crates, platform service libraries, or named-pipe crates.
TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Hermes Bridge Pure Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
```

Result: passed.

Scope:

- Added a split `runtime::hermes_bridge` module for the deterministic helper
  subset of Go `internal/runtime/hermesbridge/hermes_config.go`.
- Preserved Hermes route defaults, local notify URL validation, supported
  deliver target normalization, home-channel env key mapping, display names,
  Go-like `.env` parsing, fixed-target `deliver_extra` cleanup, legacy single
  `skills: ["notify"]` cleanup, and notify prompt migration predicates.
- Reused the new Hermes default constants from `runtime::resolve` and
  `host_notify_config_view` instead of carrying duplicate string literals.
- Used a read-only Native Agent for the parity checklist. No code-writing
  Native Agent modified this slice.

Boundary note: this slice does not implement `EnsureRoute`, `InspectRoute`,
Hermes `config.yaml` YAML read/write, route-secret generation, state warnings,
listener restart/refresh orchestration, bridge service execution, or
system-test Hermes route lifecycle acceptance. Those remain in a later YAML
parser/runtime orchestration slice.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `serde_yaml`, HTTP/TLS clients, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, platform service libraries, or ANP SDK
network/default features. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Identity ANP Service Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/identity -run 'TestGenerateIdentity|TestGenerateIdentityRejectsLoopbackANPServiceEndpoint|TestGenerateIdentityRejectsNonBareANPServiceDID' -count=1
```

Result: passed.

Scope:

- Added Go-shaped ANP service helpers to the existing `identity::did` module:
  `default_anp_service_endpoint`, `default_anp_service_did`,
  `validate_anp_service_endpoint`, `validate_anp_service_did`, and
  `build_agent_anp_message_service`.
- Made `generate_identity` reuse the helper so loopback endpoint and
  non-bare service DID validation follow Go's `BuildAgentANPMessageService`
  boundary.
- Preserved the `#message` ANPMessageService shape, trimmed endpoint/service
  DID values, profile list, and `transport-protected` security profile.

Follow-up note: the 2026-05-15 identity key compatibility slice below now
translates `identity/key_compat.go`. This ANP service helper slice still does
not implement real service calls, HTTP/TLS transport, ANP SDK network/default
features, or MLS provider execution.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice reuses the existing local `../anp/rust` SDK path with default features
disabled. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Identity Remote Wire Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/identity -count=1
```

Result: passed.

Scope:

- Added a split `identity::wire` module for the deterministic remote contract
  embedded in Go `internal/identity/client.go` and `service.go`.
- Preserved user-service endpoint constants for DID auth, handle lookup, DID
  profile, email status/send, and phone bind REST calls.
- Preserved identity JSON-RPC builders, REST call descriptors, transport
  profiles, params, fixed JSON-RPC envelope reuse, service RPC/HTTP error
  display, `authsdk` error conversion, phone/email/OTP/CSV normalization,
  handle lookup not-found normalization, profile update payload mapping,
  refresh-token `AuthRefresh` profile, and live replace-DID null semantics for
  empty optional `role` and `endpoint_url`.
- Preserved deterministic result shapes and summaries for registration,
  recovery OTP, bind phone/email, refresh token, resolve, profile get/update,
  and replace DID.
- Native Agent read-only parity review was used for the checklist; no
  code-writing Native Agent modified this slice.

Boundary note: this slice does not wire live HTTP/TLS/auth execution, generate
or persist identities, build auth sessions, poll email verification, mutate the
identity store, map service errors into CLI exits, or run identity lifecycle
system tests. Those remain in the later shared authsdk/session plus Rustls HTTP
client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Site RPC Wire Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test site_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test site_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/site -count=1
```

Result: passed.

Scope:

- Added a split `site` module for the pure Go
  `internal/site/{types.go,service.go}` RPC contract.
- Preserved `/site/rpc`, `/user-service/did-auth/rpc`, `get_root`,
  `set_root`, `list_pages`, `get_page`, `create_page`, `update_page`,
  `rename_page`, and `delete_page` method names and transport profiles.
- Preserved live-service domain normalization through the existing
  Go-equivalent `config::normalize_did_domain` helper, trim-only slug
  validation, explicit empty body allowance, params, summaries, and
  identity/root/page/list/rename/delete result shapes.
- Kept the existing `site` dry-run CLI boundary unchanged: dry-run remains
  trim-only for domain display and does not apply live bare-domain
  normalization, while the new site service wire tests cover stricter
  live-service rules.

Boundary note: this wire-only slice did not wire non-dry-run site commands,
implement `identity.RemoteClient`, bootstrap the auth session, refresh DID-auth
JWTs, perform HTTP transport, map site service errors into CLI exit codes, or
run site lifecycle system tests. The later "Tenant Site Live RPC Slice" now
covers non-dry-run command wiring, auth bootstrap, JWT refresh, shared Rustls
HTTP execution, and CLI error mapping; direct site system-test coverage remains
absent from the current tests_v2 inventory.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Content RPC Wire Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/content -count=1
```

Result: passed.

Scope:

- Added a split `content` module for the pure Go
  `internal/content/{types.go,service.go}` RPC contract.
- Preserved `/content/rpc`, `/user-service/did-auth/rpc`, `create`, `list`,
  `get`, `update`, `rename`, and `delete` method names and transport profiles.
- Preserved service-level slug/title/update-field/visibility validation,
  visibility normalization, params, summaries, and identity/page/list result
  shapes.
- Kept the existing `page` dry-run CLI boundary unchanged: dry-run remains
  permissive for raw visibility values and empty update plans, while the new
  content service wire tests cover the stricter live-service rules.

Boundary note: this slice does not wire non-dry-run page commands, implement
`identity.RemoteClient`, map content service errors into CLI exit codes, refresh
DID-auth JWTs, perform HTTP transport, or run content/page lifecycle system
tests. Those remain in the shared authsdk/session plus Rustls HTTP client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Authsdk JSON-RPC Wire/Result Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/authsdk -count=1
```

Result: passed.

Scope:

- Added a split `authsdk::wire` module for the pure Go
  `internal/authsdk/session.go` JSON-RPC and response helpers.
- Preserved the fixed JSON-RPC envelope (`jsonrpc=2.0`, `id=req-1`, method,
  params), RPC error precedence and data preservation, plain JSON body decode,
  HTTP status error body trimming, first-header-value flattening, and JSON
  content-type helper.
- Added a local `Session::ensure_jwt_from_result` helper for the non-network
  part of Go `EnsureJWT`: remember scope, persist non-empty body access token,
  fallback to a token captured from response headers before body decode,
  fallback to stored JWT, then emit the Go missing-token error string.

Boundary note: this slice does not implement real HTTP request execution,
`Headers`, `ChallengeHeaders`, 401 retry, proxy/timeout/TLS behavior, live
`DoJSONRPC`, live `DoJSON`, live `EnsureJWT`, or non-dry-run
`id refresh-token`. Those remain in the later shared authsdk/session plus
Rustls transport lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Mail Remote Wire Contract Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test mail_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/mail -count=1
```

Result: passed.

Scope:

- Added a split `mail::wire` module for Go `internal/mail/client.go` and the
  remote-method portions of `internal/mail/service.go`.
- Preserved `/mail/rpc`, `mail.getInbox`, `mail.getMessage`, `mail.markRead`,
  `mail.getMailbox`, `mail.getAttachment`, and `mail.send` request contracts.
- Preserved default inbox folder/limit, Go transport profiles, JSON params,
  validation errors, summary strings, and `ServiceError` display mapping for
  authsdk RPC and HTTP errors.
- Kept `mail inbox/read/mark-read/account/send/attachment download` non-dry-run
  execution at the existing deferred boundary.
- Native Agent read-only parity review found no blocking mismatch. It noted
  that `identity_name` correctly remains outside the RPC params, matching Go's
  service-layer identity resolution before the remote call.

Boundary note: this slice does not implement `NewClient`, HTTP execution,
DID-auth session construction, JWT refresh, CA bundle handling, attachment file
writes, or live local mail-service system tests. Those remain in the later
shared authsdk/session plus Rustls HTTP client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Local CLI Validation Selector Slice

Local Rust and offline system-test verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run pytest -q tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors tests_v2/multi_tenant/test_awiki_cli_tenant_config.py::test_awiki_cli_derives_anp_service_defaults_from_service_base_url tests_v2/multi_tenant/test_awiki_cli_tenant_config.py::test_awiki_cli_id_create_uses_tenant_did_domain_but_platform_message_service tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_msg_and_attachment_commands_validate_local_arguments tests_v2/id/test_identity_cli.py::test_id_profile_set_rejects_conflicting_body_sources
```

Result: passed.

Scope:

- Preserves Go `msg attachment download` target validation for missing
  `--with/--group` and conflicting `--with` plus `--group` before the deferred
  non-dry-run attachment transfer boundary.
- Adds `id profile set` to CLI dispatch/schema and preserves the Go
  `--markdown` plus `--markdown-file` conflict error before resolving service
  execution.
- Provides a Go-shaped `id profile set --dry-run` plan for local contract
  coverage while keeping real `did.profile.update_me` execution deferred to
  the authsdk/Rustls user-service slice.

No dependency was added. The slice is local CLI validation only and does not
introduce HTTP/TLS clients, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, or new platform/system dependencies. TLS policy remains Rustls-first
and unchanged.

## 2026-05-15 Authsdk Local Token Session Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/authsdk -run 'TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
```

Result: passed.

Scope:

- Adds a narrow Rust `anpsdk` facade for the Go ANP module path/version and the
  DID-WBA auth types needed by the first session slice.
- Translates the local bearer token scope and persistence behavior from Go
  `internal/authsdk/session.go`: remembered hostname scopes, bearer seeding,
  token capture from `Authentication-Info`, legacy `Authorization: Bearer`
  response compatibility, current-JWT updates only for persistent scopes,
  persistent callback invocation, persistent-scope token clearing, 401 retry
  policy delegation, and Go-shaped HTTP/RPC error strings.

Boundary note: this slice intentionally does not implement service transport,
`Headers`, `ChallengeHeaders`, `DoJSONRPC`, `EnsureJWT`, `DoJSON`,
non-dry-run `id refresh-token`, real identity-store JWT persistence, or the full
Go `internal/anpsdk/registry.go` alias surface. Those remain in later
authsdk/Rustls service slices.

Status update: the later Authsdk Rustls HTTP Execution and Identity Refresh
Token Live slices now cover `EnsureJWT`, live `id refresh-token`, and
identity-store JWT persistence.

No dependency was added. The code reuses the existing local `../anp/rust`
dependency with `default-features = false`; it does not enable ANP `network`,
add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, or bundled
OpenSSL. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Local V0 To V1 Apply Wiring Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestLoadLegacySettingsRejectsSplitServiceURLs|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Wires the already translated local v0->v1 apply composition into the default
  `Migration::apply` implementation.
- Lets `UpgradeIfNeeded` run local v0->v1 config/schema/import work and then
  continue into v1->v2 cleanup before stopping at the still-deferred v2->v3
  DID replacement boundary.
- Adds if-needed coverage for Go's legacy `config.json` migration path:
  legacy config is converted to canonical `config.yaml`, removed, journaled,
  and meta-stamped through v1->v2.
- Keeps imported legacy k1 identities at the explicit v0->v1 deferred boundary
  because Go immediately performs service-backed k1->e1 DID replacement after
  importing them.
- Preserves the existing backup reuse and lock-release behavior while the
  migration loop advances past v0->v1.

Boundary note: this slice does not implement service-backed k1->e1 DID
replacement for imported legacy identities, v2->v3 existing-workspace DID
replacement, rollback, or full awiki-system-test migration acceptance.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing local config, identity, bundled SQLite import/schema, journal,
backup, and lock helpers. It does not introduce HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock crate, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Journal Phase Loop Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestUpgradeIfNeededCleansLegacySkillArtifactsForExistingWorkspace|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation from backup setup into the
  migration journal phase loop.
- Reuses the existing journal `upgrade_id` when present; otherwise creates a
  new Go-style compact timestamp ID.
- Saves `checking`, `applying`, and `validating` journal phases for each
  planned migration, with the migration name, version range, backup directory,
  start time, and app version.
- Calls migration `is_done`; if the migration is not already complete, saves
  `applying` and calls `apply`; then saves `validating` and calls `validate`.
- Stamps `meta.json` after each successful migration with the target schema
  version, app version, RFC3339-seconds update time, last upgrade ID, backup
  dir, and accumulated warnings.
- Updates `Context.current_meta` after each successful migration and clears the
  journal after the full plan succeeds.
- Splits `workspace_upgrade_if_needed_*` tests into
  `workspace_upgrade_if_needed_contract.rs`, keeping all Rust test files under
  the default 1200-line structure limit.

Boundary note: this slice does not complete all migrations. v0->v1 default
apply remains deferred at
`workspace_0_to_1_bootstrap_local_state_upgrade`, and v2->v3 k1->e1 DID
replacement remains deferred at
`workspace_2_to_3_replace_existing_k1_handle_dids`. Rollback and
service-backed identity replacement remain later slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing upgrade modules, the documented direct file-lock FFI, and the
already approved `rusqlite + bundled` SQLite backup path. It does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock crate, or new SQLite dependencies.
TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Backup Setup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation past the lock/reinspect
  boundary into backup setup.
- Reloads the journal after the lock-inside second inspect, matching Go's
  `LoadJournal` call before backup selection.
- Reuses `journal.backup_dir` when present and records it in
  `Context.backup_dir`.
- Creates a new workspace backup with the existing translated backup helper
  when no journal backup is present, then records the created directory in
  `Context.backup_dir`.
- Keeps the current deferred execution error before the journal phase loop, so
  this slice does not execute migration `is_done`/`apply`/`validate` yet.

Boundary note: this slice does not implement journal phase writes,
migration-loop `is_done`/`apply`/`validate`, meta stamping after each migration,
rollback, v0->v1 default apply wiring, or v2->v3 k1->e1 DID replacement.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing upgrade modules, the already documented direct file-lock FFI,
and the already approved `rusqlite + bundled` SQLite backup path. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock crate, or new SQLite dependencies.
TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Lock Pre-Migration Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation past the initial
  no-op/current-version preflight and into the pre-migration lock boundary.
- Reused the translated `AcquireFileLock` helper before any real migration
  execution, matching Go's lock-before-backup/migration ordering.
- Preserved lock-held failure text:
  `workspace upgrade is already running: <path>`.
- Preserved stale-journal clearing for empty/latest workspaces before the lock.
- Added lock-inside second `inspect`/current-meta refresh before planning real
  migration execution.
- Verified the lock anchor is written before deferred real migration execution
  and the OS lock is released when the function returns with the current
  deferred execution error.

Boundary note: this slice still returns the existing deferred execution error
when a real migration plan is required. It does not implement backup
creation/reuse, journal phase writes, migration-loop `is_done`/`apply`/
`validate`, meta stamping after each migration, rollback, v0->v1 default apply
wiring, or v2->v3 k1->e1 DID replacement.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing upgrade modules and the already documented direct file-lock FFI.
It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk
session, platform service-manager, filesystem-copy, file-lock crate, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v1->v2 Legacy Cleanup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli migration_v1_to_v2 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededCleansLegacySkillArtifactsForExistingWorkspace|TestUpgradeIfNeededImportsLegacyWorkspace' -count=1
```

Result: passed.

Scope:

- Added `migration_v1_to_v2.rs` for Go
  `workspaceV1ToV2Migration.Apply` and `cleanupLegacySkillArtifacts`.
- Preserved the missing-context guard:
  `workspace upgrade context is required`.
- Removes both Go legacy skill install paths under `.openclaw`.
- Preserves platform artifact gates and command argv for macOS LaunchAgent,
  Linux `systemctl --user`, and Windows `schtasks`, using test injection so
  unit tests do not call host service managers.
- Preserves `OPENCLAW_WORKSPACE`, `XDG_CONFIG_HOME`, and `LOCALAPPDATA`
  fallback behavior for heartbeat/service paths.
- Removes the legacy heartbeat section only when the marked section references
  `awiki-agent-id-message`, preserves unrelated content, and collapses extra
  blank lines like Go.
- Wires direct `Migration::apply` for 1->2 and Go's no-op v1->v2
  `validate`; `UpgradeIfNeeded` still defers real migration phase execution.

Boundary note: this slice does not implement full `UpgradeIfNeeded` phase
execution, journal phase writes, backup/rollback, meta stamping through the
migration loop, v0->v1 default apply wiring, or v2->v3 k1->e1 DID replacement.
The platform service actions remain external command invocations, matching Go;
no service-manager crate or platform library is linked.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses only standard-library filesystem/process APIs and existing upgrade
structures. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, filesystem-copy, file-lock, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Durablefs Directory Sync Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli durablefs --locked
cargo +1.79.0 test -p awiki-cli --test config_writer_contract config_writer_uses_go_style_tempfile_permissions_and_cleanup --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/durablefs ./internal/config -run 'TestSyncDirectory|Test.*Durable|Test.*Write|Test.*Config' -count=1
```

Result: passed. The Rust `durablefs::sync_directory` tests cover the Go
contract for existing directories and missing-directory platform behavior:
non-Windows fails on a missing directory, while Windows is compiled as a no-op.
The focused config writer test proves the durable config write path still
cleans up temporary files and preserves Go-style Unix permissions while using
the shared helper. Go reference tests for `internal/durablefs` and focused
`internal/config` write behavior also passed.

Scope:

- Go `internal/durablefs/syncdir_unix.go` non-Windows directory open + sync.
- Go `internal/durablefs/syncdir_windows.go` intentional Windows no-op.
- Existing Rust config writer parent-directory sync call now delegates to the
  shared `durablefs` helper and preserves the Go `sync config dir` error prefix.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/durablefs.rs` is 76 lines,
`crates/awiki-cli/src/config/write.rs` is 445 lines, and
`crates/awiki-cli/tests/config_writer_contract.rs` is 264 lines. No file-size
exception is needed.

Boundary note: this slice does not implement Go `runtime/openclawnotify` route
registry writes or `internal/upgrade` filesystem helpers that also call
`durablefs.SyncDirectory`; those remain later file-level slices. Windows no-op
behavior is represented with conditional compilation, but Windows runtime
execution still requires Windows CI or manual evidence before full
cross-platform runtime parity claims.

No dependency was added. Cargo manifests and lockfile were unchanged; TLS and
SQLite dependency decisions remain unchanged.

## 2026-05-14 OpenClaw Route Registry Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli openclaw_routes --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/runtime/openclawnotify -run 'TestResolveRouteInput|TestAddAndRemoveRoutePersistRegistry|TestLoadRoutesMissingFileReturnsEmpty' -count=1
cd ../awiki-cli && go test ./internal/cli -run TestRuntimeDryRunPlansCoverStableActions -count=1
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_config_commands_work \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_validates_inputs_and_supports_dry_run \
    -q
```

Result: passed. Rust route helper tests, full runtime contract tests, full
`awiki-cli` crate tests, structure check, build, Go route registry tests, Go CLI
dry-run tests, and two existing runtime host-notify system-test selectors all
passed.

Scope:

- Go `internal/runtime/openclawnotify/routes.go` local route registry behavior:
  `RoutesPath`, `ResolveRouteInput`, `ParseSessionKey`, `NormalizeRoute`,
  `LoadRoutes`, `AddRoute`, `RemoveRoute`, and `WriteRoutes`.
- Go `internal/cli/runtime_host_notify_routes.go` route `add/list/remove`
  local CLI boundary and stable dry-run plan shapes.
- Rust `host_notify_config_view.routes` now loads the persisted route registry
  instead of returning a hard-coded empty list.

Structure note: the route registry was added as
`crates/awiki-cli/src/runtime/openclaw_routes.rs` rather than expanding
`runtime/mod.rs`. The changed Rust source and test files remain below the
default 1200-line source limit; no file-size exception is needed.

Follow-up note: the 2026-05-15 OpenClaw route confirmation slice below now
translates Go's post-persistence confirmation webhook behavior. The registry
slice itself still represents the earlier local route-registry boundary.

Dependency audit showed only the existing Rustls/ring update path and the
approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path. No OpenSSL/native-tls, new HTTP client, WebSocket, or platform service
dependency was introduced.

## 2026-05-15 OpenClaw Route Confirmation Webhook Slice

Status: locally verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli openclaw_webhook --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract host_notify_openclaw --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Result: passed locally. `runtime_contract` route tests cover successful
confirmation POST, bearer token forwarding, Go-shaped JSON request body,
auto-detected `OPENCLAW_GATEWAY_PORT` hook URLs, duplicate-route no-send
behavior, Go-style loopback URL validation, validation, and post-persistence
warning behavior when OpenClaw rejects the confirmation. `xtask
check-structure` reported no undocumented Rust files over 1200 lines.

Scope:

- Added `runtime::openclaw_webhook` for Go-compatible route confirmation
  message construction and webhook POST execution on the existing
  Rustls/std `transportcfg::HttpClient`.
- Wired non-dry-run `runtime host-notify openclaw route add` so newly persisted
  routes send one confirmation webhook using either an explicit AWiki config
  hook URL or the auto-detected `OPENCLAW_GATEWAY_PORT` default URL, while
  duplicate routes remain local.
- Preserved Go failure semantics: route persistence succeeds first; hook URL
  preparation failures and send/acceptance failures become warnings rather than
  command failures.
- Preserved Go response acceptance shape: HTTP 2xx, JSON `ok=true`, and a
  non-empty `runId`, surfaced as `data.confirmation.accepted/run_id`.

Follow-up note: the 2026-05-15 OpenClaw JSON config probe slice below now
translates Go's OpenClaw config-file probing through `OPENCLAW_CONFIG_PATH` and
`~/.openclaw/openclaw.json`. This route-confirmation slice itself remains the
webhook POST and warning-semantics boundary.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The implementation reuses the existing Rustls/std HTTP client and
does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`,
bundled OpenSSL, YAML crates, platform service libraries, or new SQLite
dependencies.

## 2026-05-15 OpenClaw JSON Config Probe Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_openclaw_config_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract host_notify_openclaw --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/runtime/openclawnotify -run 'TestProbeGatewayPortReadsEnvBeforeConfig|TestProbeGatewayPortReadsOpenClawConfig|TestResolveSettingsUsesAutoDetectedHookURLWhenConfigHookURLUnset|TestResolveSettingsPrefersConfigTokenOverEnvironmentAndOpenClawConfig|TestResolveSettingsPrefersEnvironmentTokenOverOpenClawConfig' -count=1
```

Result: passed locally. The focused Rust config probe contract covers
`OPENCLAW_GATEWAY_PORT` precedence over OpenClaw JSON port while preserving JSON
path/token, `HOME` fallback to `.openclaw/openclaw.json`, missing and malformed
JSON silent fallback, Go positive-`int` port behavior above `65535`, Go
typed-JSON unmarshal all-or-nothing fallback, Go-style `path.Clean`
normalization for hook paths including `/`, `.`, `..`, and `/a/../b/.`, token
source redaction, and auto-detected hook URL construction. The focused
`runtime_contract` OpenClaw tests continue to cover route confirmation with
config-probed path/token settings.

Scope:

- Translates Go `ProbeOpenClawConfig`, `OpenClawConfigPath`, hook token
  fallback, hook URL auto-detection, and hook endpoint path construction for the
  CLI-visible OpenClaw settings boundary.
- Keeps explicit AWiki config-file hook URL precedence over auto-detected
  OpenClaw settings, and keeps token precedence as AWiki config file,
  `OPENCLAW_HOOK_TOKEN`, OpenClaw JSON token, then unset.
- Preserves Go's silent fallback for missing/unreadable/invalid OpenClaw JSON
  config files.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The implementation uses existing `serde_json`, standard env/path
handling, and existing OpenClaw route/config helpers; it does not add `reqwest`,
`hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates,
platform service libraries, or new SQLite dependencies.

## 2026-05-14 Listener Status Files And Saved Status Merge Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli listener --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract listener_status_merges_saved_sessions_and_host_notify_state --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/runtime/listener -run 'TestSessionWarnings|TestHasDisconnectedSessions|TestMergeSavedRuntimeStatus' -count=1
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_listener_config_set_requires_flags_and_supports_dry_run \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_config_commands_work \
    -q
```

Result: passed. Rust listener helper tests, focused and full runtime contract
tests, full `awiki-cli` crate tests, structure check, build, Go listener helper
tests, dependency audit, and two runtime system-test selectors all passed.

Scope:

- Go `internal/runtime/listener/types.go`: local listener `Status`,
  `SessionStatus`, and `HostNotifyStatus` JSON shape.
- Go `internal/runtime/listener/files.go`: `listener.pid`,
  `listener.status.json`, and `listener.expected-boot-id` helpers, including
  Unix `0600` writes for helper-owned files.
- Go `internal/runtime/listener/status_helpers.go`: disconnected-session
  warning text and disconnected-session detection.
- Narrow Go `internal/runtime/listener/manager.go`: saved runtime status merge
  behavior, including PID mismatch skip, saved sessions/boot/start metadata,
  saved host-notify last error, and running-only host-notify override.
- Rust `runtime listener status` now merges saved `listener.status.json` data
  into the public CLI envelope.

Structure note: the listener status/files implementation lives in
`crates/awiki-cli/src/runtime/listener.rs` instead of expanding
`runtime/mod.rs`; changed files remain below the default 1200-line source
limit. No file-size exception is needed.

Boundary note: this slice does not translate Go `listener/service.go`,
`server.go`, `wsclient.go`, `host_notify.go`, or platform `sysproc_*` code.
Listener lifecycle commands continue to use the existing Rust local-state
facade rather than adding systemd/launchd/Windows-service dependencies.
No HTTP/TLS, WebSocket, authsdk session, OpenSSL, `native-tls`, or platform
service-manager dependency was added.

Dependency audit showed only the existing Rustls/ring update path and the
approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path. No OpenSSL/native-tls, new HTTP client, WebSocket, or platform service
dependency was introduced.

## 2026-05-14 CLI Error Hint Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli error_hints --locked
cargo +1.79.0 test -p awiki-cli internal_anyhow --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/cli -run 'TestIsWindowsDirSyncCompatibilityError|TestRuntimeExitRefinesWindowsDirSyncCompatibilityHint|TestConfigCommandExitRefinesWindowsDirSyncCompatibilityHint' -count=1
```

Result: passed. The Rust helper matches Go's Windows directory-sync
compatibility detector: it requires `Access is denied` plus one of
`sync config dir`, `sync route registry dir`, or `sync dir`, and it ignores
normal permission errors and unrelated parse failures. The `internal_anyhow`
focused tests prove the refined hint reaches the current Rust workspace/config
error envelope path while ordinary permission errors keep the existing doctor
fallback hint.

Scope:

- Go `internal/cli/error_hints.go` `windowsDirSyncCompatibilityHint`.
- Go `refineWorkspaceWriteHint` and `isWindowsDirSyncCompatibilityError`
  string-matching behavior.
- Rust generic workspace/config error hint refinement through `internal_anyhow`.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/app.rs` is 882 lines and
`crates/awiki-cli/src/app/error_hints.rs` is 72 lines. No file-size exception is
needed.

Boundary note: this slice does not implement full Go `runtimeExit`,
`configCommandExit`, workspace-upgrade execution, platform service-manager
behavior, route registry writes, or Windows-specific system service behavior.
It only ports the shared hint classifier and wires it into the current Rust
generic workspace/config error path.

No dependency was added. Cargo manifests and lockfile were unchanged; TLS and
SQLite dependency decisions remain unchanged.

## 2026-05-14 Buildinfo Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli buildinfo --locked
cargo +1.79.0 test -p awiki-cli --test core_contract version_reports_current_build_info --locked
cargo +1.79.0 test -p awiki-cli --test core_contract status_reports_phase_version_paths_state_and_config --locked
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/buildinfo
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/core/test_basic_commands.py::test_core_query_commands_return_structured_success \
    tests_v2/core/test_output_contracts_cli.py::test_output_formats_and_jq_filters_follow_cli_contracts \
    -q
```

Result: passed. The Rust buildinfo helper now preserves the Go metadata
defaults for `version`, `commit`, `build_date`, and `cgo_enabled`, returns an
independent owned snapshot, and renders Rust target names as Go-compatible
runtime names for visible `goos`/`goarch` fields. The `version` and `status`
core contracts verify the eight public JSON fields exposed by Go:
`version`, `commit`, `build_date`, `go_version`, `goos`, `goarch`, `compiler`,
and `cgo_enabled`. The doctor contract also passed, keeping the build check
compatible with the translated report shape. The accepted system-test selector
covered real CLI `status`, `version`, `config show`, `doctor`, representative
pretty/table/JQ output paths, and reported 2 passed, failed 0, skipped 0, in
0.37s. System-test configuration context: `AWIKI_CLI_UNDER_TEST=rust`,
`AWIKI_CLI_RUST_REPO=../awiki-cli-rs2`; no service URLs or live external
services were required for these core selectors.

Scope:

- Go `internal/buildinfo/buildinfo.go` metadata constants and `Current()`
  snapshot behavior.
- Go `internal/buildinfo/buildinfo_test.go` injected metadata and independent
  snapshot behavior, adapted to Rust with an injectable constructor.
- Core CLI `version` and `status` output contracts for buildinfo JSON fields.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/buildinfo.rs` is 174 lines and
`crates/awiki-cli/tests/core_contract.rs` is 589 lines. No file-size exception
is needed.

Boundary note: this slice does not implement release pipeline metadata wiring,
`build.rs`, CI injection, package metadata changes, update policy changes, or
system-test assertion changes. Future release/packaging work must intentionally
wire `AWIKI_CLI_VERSION`, `AWIKI_CLI_COMMIT`, `AWIKI_CLI_BUILD_DATE`, and
`AWIKI_CLI_CGO_ENABLED` equivalents without changing the public JSON field
surface.

No dependency was added. Cargo manifests and lockfile were unchanged. The
dependency policy remains Rustls-first for TLS: future HTTP/WebSocket/authsdk
slices must not prefer OpenSSL, `native-tls`, or bundled OpenSSL over
Rustls-backed options, and any non-Rustls exception must be separately recorded
with failed Rustls parity evidence.

## 2026-05-14 Identity Handle Input Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_contract identity_handle_input_helpers_match_go_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract full_handle --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/identity
cd ../awiki-cli && go test ./internal/message -run TestMessageServiceHelperContracts
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
    tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
    tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
    tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
    tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
    tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
    tests_v2/core/test_output_contracts_cli.py \
    -q
```

Result: passed. The focused Rust identity helper contract covers bare handle
normalization with configured DID domain, full handle normalization, `wba://`
handle normalization, DID input rejection for handle input, shared bare-handle
completion pass-through/expansion behavior, and full-handle derivation from
non-`user` DID path prefixes. The focused `full_handle` run covers Go's
`Manager.Load` backfill contract: handle-path DIDs persist `full_handle` into
both identity payload and index, while `user` DIDs do not backfill a full handle.
Focused Rust `msg` and `group` contracts also passed after those handlers
switched to the shared identity helper. Full Rust package tests, structure check,
binary build, Go `internal/identity` reference tests, Go message helper reference
tests, dependency audit, and the accepted-scope system regression also passed.
System regression context: `AWIKI_CLI_UNDER_TEST=rust`,
`AWIKI_CLI_RUST_REPO=../awiki-cli-rs2`; no external service URLs were required
for these local/dry-run selectors. Result: 11 passed, failed 0, skipped 0.

Scope:

- Added `crates/awiki-cli/src/identity/handle_input.rs` for Go
  `internal/identity/handle_input.go`.
- Preserved Go `NormalizeHandleInput` behavior: trim, lowercase, reject DID
  values, strip `wba://`, split explicit domains, normalize trailing-dot
  domains, and require `did_domain` for bare handles.
- Preserved Go `CompleteBareHandle` behavior: empty input returns empty string,
  DID values pass through with trimmed spelling, explicit full handles pass
  through, and bare or `wba://bare` handles expand to canonical full handles.
- Moved stored handle field derivation out of `identity/did.rs` and into the
  handle input module so identity store behavior shares the same normalization
  rules.
- Preserved Go `Manager.Load` stored-handle backfill side effects for handle-path
  DIDs: normalized handles/full handles are written back to `identity.json` and
  `index.json` only when non-empty.
- Updated `msg` and non-E2EE `group` dry-run handlers to call
  `identity::complete_bare_handle`, eliminating duplicated local completion
  logic.

Structure note: changed files remain well below the default 1200-line Rust
source limit: `identity/handle_input.rs` is 205 lines, `identity/did.rs` is 124
lines, `identity/store.rs` is 459 lines, `app/msg_handlers.rs` is 515 lines,
`app/group_handlers.rs` is 438 lines, and `tests/identity_contract.rs` is 456
lines. No file-size exception is needed.

Boundary note: this is a local helper consolidation and CLI dry-run normalization
cleanup. It does not implement remote identity register/bind/recover/profile/
resolve, non-dry-run `id refresh-token`, non-dry-run `id replace-did`, message
RPC execution, group RPC execution, authsdk session refresh, HTTP/WebSocket
transport, MLS provider execution, or cache mutation.

Status update: later identity live slices now cover phone registration and live
`id refresh-token`; remote bind/recover/profile/resolve, non-dry-run
`id replace-did`, message RPC execution, group RPC execution, WebSocket
transport, MLS provider execution, and cache mutation remain deferred.

No dependency was added. Cargo manifests and lockfile were unchanged, and this
slice does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, crypto,
MLS, or platform service-manager dependencies.

## 2026-05-14 Trace/Transport Foundation Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test traceutil_contract --test transportcfg_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/traceutil ./internal/transportcfg
```

Result: passed. Focused Rust contract tests: 9 passed. Full Rust package
verification: 118 local tests passed. Go reference tests for both source
packages passed. Structure check reported no undocumented Rust files over 1200
lines; current slice files are `traceutil.rs` 298 lines,
`transportcfg.rs` 320 lines, `traceutil_contract.rs` 123 lines, and
`transportcfg_contract.rs` 128 lines. Build passed. Dependency audit showed
only the existing Rustls/ring update path and approved `rusqlite + bundled`
SQLite path; this slice added no dependency and did not introduce OpenSSL,
`native-tls`, `reqwest`, `hyper`, or WebSocket crates.

Scope:

- Go `internal/traceutil/trace.go` pure timing trace helpers:
  `AWIKI_CLI_TRACE_TIMING`, run/phase/fallback capture, pretty Chinese output,
  known label humanization, and duration formatting.
- Go `internal/transportcfg/config.go` pure resolver behavior:
  bridge/HTTP/profile default durations, environment variable names, Go-style
  positive duration/int fallback, and profile timeout fallback.

Boundary note: Go `transportcfg.NewHTTPClient` is not translated in this
foundation slice. It requires a shared Rustls-first HTTP client decision for
TLS roots, custom CA bundle, HTTP/2, response-header timeout, idle pooling, and
TLS 1.2 minimum behavior. This slice adds no dependency; OpenSSL,
`native-tls`, and bundled OpenSSL remain disallowed as first-choice TLS paths.

No `awiki-system-test` selector is required for this internal-only foundation
slice because no new public CLI/service path is wired to these modules yet.

## 2026-05-14 Message Signed Wire Params Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 59 local tests after this slice. Focused
`message_contract` now has 10 tests, including signed direct send params,
signed direct attachment manifest params, signed group attachment manifest
params, and DID-document verification of each origin proof. Structure check
reported no undocumented Rust files over 1200 lines; `message/wire.rs` is 204
lines, `message/attachment.rs` is 394 lines, and `tests/message_contract.rs`
is 516 lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

Scope:

- Go `BuildDirectSendRPCParams` signed direct text params.
- Go `contentTypeForMessageType` normalization: trim/lowercase input,
  `attachment_manifest` maps to the attachment manifest content type, only
  `event` maps to `application/json`, and `json` falls back to `text/plain`.
- Go `BuildDirectAttachmentSendRPCParams` signed direct attachment manifest
  params.
- Go `BuildGroupAttachmentSendRPCParams` signed group attachment manifest
  params.
- Auth wrapper uses `anp-rfc9421-origin-proof-v1` and `origin_proof`, omits
  `sender_proof`, and verifies through the local ANP Rust SDK.

Boundary note: this slice still does not implement `authsdk` session/JWT
refresh, HTTP transport, WebSocket proxy transport, attachment transfer
execution, local cache mutation, secure direct E2EE, or group mutation RPC
execution. It adds no new dependency and keeps TLS/HTTP dependency selection
deferred to the shared Rustls service-client slice.

Accepted-scope system regression:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed, 37 tests. This regression selector covers the existing public
CLI surface; the signed wire builders are verified by Rust contract tests
because they are not yet wired into service-backed CLI execution.

## 2026-05-14 Message RFC9421 Origin-Proof Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 57 local tests after this slice. Focused
`message_contract` has 8 tests, including RFC9421 origin-proof generation,
Go-compatible verification-method selection, `anp-rfc9421-origin-proof-v1`
auth shape, canonical request digest comparison, and DID-document verification
through the local ANP Rust SDK. Structure check reported no undocumented Rust
files over 1200 lines; `message/proof.rs` is 72 lines and
`tests/message_contract.rs` is 385 lines. Dependency audit remained unchanged:
only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled
SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or
platform service dependency was added.

Scope:

- Go `internal/message/auth.go` local private-key material loading behavior.
- Go `verificationMethodID` behavior: first `authentication` string, falling
  back to first `verificationMethod.id`.
- Go `internal/message/proof.go` `buildOriginProof` auth fields:
  `contentDigest`, `signatureInput`, and `signature`.
- Serialization of the signed auth value as
  `scheme: anp-rfc9421-origin-proof-v1` plus `origin_proof`.

Boundary note: this slice does not implement `authsdk` session/JWT refresh,
HTTP transport, WebSocket proxy transport, attachment transfer execution, local
cache mutation, secure direct E2EE, or group mutation RPC execution. It adds no
new dependency and keeps TLS/HTTP dependency selection deferred to the shared
Rustls service-client slice.

Accepted-scope system regression:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed, 37 tests. This regression selector covers the existing public
CLI surface; the new origin-proof helper is verified by Rust contract tests
because it is not yet wired into service-backed CLI execution.

## 2026-05-14 Message Pure Foundation Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 55 local tests after this slice, including 6
focused `message_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; the largest new message source file is
`message/attachment.rs` at 328 lines and `tests/message_contract.rs` is 268
lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

System-test verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q

AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed. Focused core output selector: 2 passed. Accepted-scope
regression selector: 37 passed.

Scope:

- Pure Go `internal/message/wire.go` request builders for inbox, direct
  history, and mark-read RPC params.
- Direct text payload shape and message-type-to-content-type mapping needed by
  later signed direct sends.
- Pure attachment create-slot, commit-object, download-ticket params,
  attachment manifest construction, manifest JSON encoding, and message-content
  attachment selection.
- DID document attachment service selection from local JSON values, including
  profile/security-profile filtering and priority ordering.
- WebSocket HTTP/cache fallback warning text normalization.

Boundary note: RFC9421 origin-proof signing, private-key/auth context loading,
`authsdk` session refresh, HTTP transport, WebSocket proxy transport, local
cache mutation, secure direct E2EE, and group mutation RPC execution remain
deferred. The local ANP Rust SDK exposes proof helpers, but proof signing needs
a separate auth/proof slice so optimization and dependency decisions are not
mixed into this pure foundation translation.

## 2026-05-14 Site Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test site_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 49 local tests after this slice, including 5
focused `site_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; `app.rs` is 877 lines and the new
`app/site_handlers.rs` is 286 lines. Dependency audit remained unchanged: only
the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled
SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or
platform service dependency was added.

Scope:

- `site root get/set` and `site page list/get/create/update/rename/delete`
  command catalog entries and parser dispatch.
- Go dry-run plan shapes from `internal/cli/site.go`, including `/site/rpc`
  metadata, RPC method names, request fields, domain/slug trim behavior, and
  markdown-file byte counting.
- Go CLI boundary behavior for required flags, explicit body-source
  requirements, markdown source conflicts, local file-read errors, and
  non-dry-run deferral.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.11s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: non-dry-run tenant site RPC and full `internal/site/service.go`
remain deferred. Those require shared authsdk/session plus Rustls HTTP client
translation and service-backed tests. OpenSSL bundled is not the preferred TLS
fallback and would require a separate documented exception.

## 2026-05-14 Msg Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 44 local tests after this slice, including 6
focused `msg_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; `app.rs` stayed under the limit after `msg` moved into
`app/msg_handlers.rs`. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

Scope:

- `msg send`, `msg attachment download`, `msg inbox`, `msg history`,
  `msg mark-read`, and `msg secure status/init/repair/failed/retry/drop`
  command catalog entries and parser dispatch.
- Go dry-run plan shapes for direct send, group send, attachment send/download,
  inbox/history/mark-read, and secure direct commands.
- Go CLI boundary behavior for bare-handle completion, local `--text-file`
  reads, attachment metadata, `--type`/`--file` conflict handling, missing text
  handling, and Cobra-like required-flag errors.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.12s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Boundary note: non-dry-run message RPC, WebSocket proxy transport, attachment
transfer, secure direct E2EE execution, and `tests_v2/cli/test_awiki_cli_direct_local.py`
remain deferred. Those require shared authsdk/session plus Rustls HTTP/WS and
E2EE provider slices. OpenSSL bundled is not the preferred TLS fallback and
would require a separate documented exception.

## 2026-05-14 Page Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test page_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 38 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines; `app.rs` is 914 lines and
the new `app/page_handlers.rs` is 224 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, or
platform service dependency was added.

Scope:

- `page create/list/get/update/rename/delete` command catalog entries and
  parser dispatch.
- Dry-run plans from Go `internal/cli/page.go`, including `/content/rpc`
  metadata, request fields, trim behavior, and update `changed_fields`.
- Local dry-run-boundary validation from Go: create `slug`/`title` and
  markdown source conflict; markdown-file reads are local so `body_bytes`
  matches Go.
- Go dry-run boundary preservation: visibility choices are not validated and
  empty `page update --dry-run` is not rejected, because Go applies those checks
  in the non-dry-run content service.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.12s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.82s`.

Boundary note: `tests_v2/page/test_page_cli.py` was not run against Rust for
acceptance because it covers non-dry-run page CRUD through live `/content/rpc`.
That remains deferred to the shared authsdk/session plus Rustls HTTP client
slice.

## 2026-05-14 Identity Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc '
```

Result: passed. Rust tests: 13 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls path was present.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  -q
```

Result: `7 passed in 0.22s`.

## 2026-05-14 Runtime/Host-Notify Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 17 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/runtime/test_runtime_cli.py -q
```

Result: `9 passed in 0.70s`.

Accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.24s`.

Boundary note: running the full `tests_v2/id/test_identity_cli.py` file still
fails on remote identity registration/recovery/profile/replace-did selectors.
Those selectors exercise later authsdk/user-service work and are not claimed by
the local identity plus runtime/config slices.

## 2026-05-14 Mail Local Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 21 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

Scope:

- `mail inbox/read/mark-read/account/send/attachment download/notify` command
  catalog entries and parser dispatch.
- Dry-run plans and local validation messages from Go `internal/cli/mail.go`.
- Local `mail notify` SQLite query and notification normalization from Go
  `internal/mail/service.go` and `internal/store/query.go`.

Boundary note: non-dry-run remote mail RPC is not claimed by this slice. It
requires the shared authsdk/session plus Rustls HTTP/TLS dependency decision and
must be verified with local mail-service system tests in a later slice.

Accepted-scope regression verification was rerun after the mail slice:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.20s`.

## 2026-05-14 Config Set Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 23 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

Scope:

- `config set --did-domain` command catalog entry and parser dispatch.
- Go `config_set_did_domain` dry-run plan shape.
- Persistent `services.did_domain` write using the existing config writer.
- Go-compatible permissive DID-domain normalization for the tested command
  cases, including trailing-dot trimming and URL/path rejection.
- Side-effect guard that the command does not create runtime socket or pid
  artifacts.

No new dependency was added. Full YAML/config parser/serializer parity remains
a later slice. Go `write.go` durable-write mechanics are covered by the later
config writer helper/durable-write slice.

Accepted-scope regression verification was rerun after the config-set slice:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.24s`.

## 2026-05-14 Update/Upgrade Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 28 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls, HTTP/TLS, or platform service-manager dependency
was added.

Scope:

- `upgrade` command catalog entry and parser dispatch.
- Go `upgrade` data shape for current/latest/min-supported version,
  strict-disabled, dev-build, newer/block flags, metadata source, update check
  status, upgrade hint, and `upgrade_attempted`.
- Cache-only update metadata loading from
  `<workspace>/cache/update/metadata.json`.
- Env/config controls:
  `AWIKI_CLI_UPDATE_CACHE_ONLY`, `AWIKI_CLI_UPDATE_CACHE_TTL`,
  `AWIKI_CLI_DISABLE_STRICT_VERSION`, and
  `update.disable_strict_version`.
- Go-compatible dev-build and prerelease version comparison behavior for the
  verified slice.
- Go npm packaging surface copied into `package.json`, `scripts/install.js`,
  and `scripts/run.js`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/update -q
```

Result: `5 passed in 0.72s`.

Boundary update: the registry fetch/writeback and root update-policy preflight
guard deferred here were translated in later update slices. The root preflight
now lives in `crates/awiki-cli/src/app/update_preflight.rs` and reuses the
Rustls-backed `update::check` path.

## 2026-05-14 Identity/Group Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test identity_contract --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 30 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit still
only showed the expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, or platform
service-manager dependency was added.

Scope:

- Public `id replace-did` command catalog entry, parser dispatch, dangerous
  short text, side-effect flag, and Go dry-run plan/warning shape.
- `group create` and `group update` command catalog entries plus dry-run
  plans with Go PascalCase request fields.
- Go pointer semantics for group dry-run policy fields: absent bool/int flags
  render as JSON null; explicitly changed flags render concrete bool/int values.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `2 passed in 0.14s`.

Boundary note: non-dry-run DID replacement and group RPC/lifecycle behavior
remain deferred translation tasks. They require authsdk/message-service/store
rebind work and should be implemented in dedicated slices with service-backed
system tests.

## 2026-05-14 Group Non-E2EE Dry-Run Lifecycle Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 31 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present.

`group_contract` now covers the non-E2EE `internal/cli/group.go` dry-run surface
beyond create/update:

- `group get` and alias `group show` with canonical command path and
  `action: group.show`.
- `group join`, `group add`, `group remove` and alias `group kick`, and
  `group leave` request plans with Go PascalCase fields.
- Membership mutation `member_handle` completion for bare handles and no
  completion for explicit DID values.
- `group list`, `group members`, and `group messages` default/explicit limit
  behavior, including `Cursor` and `Skip: 0` in message-list plans.
- Schema children and alias metadata for the added non-E2EE group commands.

Go parity probes:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group show --dry-run --group did:wba:awiki.ai:groups:demo:e1_group
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group kick --dry-run --group did:wba:awiki.ai:groups:demo:e1_group --member bob
```

Result: Rust alias command paths and plans matched the observed Go behavior:
`show` renders command `awiki-cli group get`, and `kick` renders command
`awiki-cli group remove` with `action: group.kick`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice still does not implement non-dry-run group RPC, group
attachments, or `group e2ee ...`. Those remain dedicated service-backed and
MLS/provider translation tasks.

## 2026-05-14 Group E2EE Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 33 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present.

Scope:

- `group e2ee status` dry-run provider diagnostics, profile/security profile,
  hidden-discovery guard, and MLS data directory.
- `group e2ee publish-key-package` dry-run plan, including `--purpose`,
  `--recovery`, `--device`, `--group`, and `--contract-test`.
- `group e2ee pending` and `group e2ee repair` dry-run plan shape.
- `group e2ee process-leave-request`, `recover-member`, hidden `update-key`,
  and hidden `rejoin` dry-run plan shape and schema metadata.
- `group e2ee` command tree schema children and hidden/side-effect metadata.

Go parity probes:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee status --dry-run --group did:wba:example.com:groups:demo:e1_group
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee publish-key-package --dry-run --group did:wba:example.com:groups:demo:e1_group --purpose update --device bob-main --contract-test
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee rejoin --dry-run --group did:wba:example.com:groups:demo:e1_group --member bob
```

Result: Rust dry-run outputs matched the observed Go command path and plan
shape for the probed E2EE status, KeyPackage publish, and rejoin commands.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Boundary note: this slice does not execute `anp-mls`, call hidden P6
message-service RPCs, or validate MLS storage/security boundaries. Those remain
dedicated group-E2EE implementation and focused system-test tasks.

## 2026-05-14 Group Base/Local Wire Builder Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The full `awiki-cli` crate test suite reported 65 tests
passing after this slice. Structure check reported no undocumented Rust files
over 1200 lines; the new `message/group_wire.rs` is 527 lines and
`tests/message_group_wire_contract.rs` is 402 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `BuildGroupCreateRPCParams` request params, including service target,
  `group.create` origin proof, display-name validation, default policy,
  permissions, and E2EE policy security-profile override.
- Go `BuildGroupGetInfoRPCParams` unsigned base profile request params.
- Go `BuildGroupJoinRPCParams`, `BuildGroupAddRPCParams`,
  `BuildGroupRemoveRPCParams`, `BuildGroupLeaveRPCParams`,
  `BuildGroupUpdateProfileRPCParams`, and `BuildGroupUpdatePolicyRPCParams`
  signed control params.
- Go `BuildGroupSendRPCParams` signed group send params, message id generation,
  message-type content-type mapping, and original text body preservation.
- Go local `BuildGroupGetRPCParams`, `BuildGroupListRPCParams`,
  `BuildGroupMembersRPCParams`, and `BuildGroupMessagesRPCParams`, including
  local profile metadata, default limits, cursor-to-`since_seq`, `skip`
  omission when zero, and no generated local operation fields.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice constructs request params only. It does not execute
non-dry-run group RPC calls, refresh JWT sessions, select HTTP/WebSocket
transport, mutate local message cache, upload/download attachments, or run MLS
group E2EE provider logic. Those remain dedicated authsdk/message-service and
group-E2EE wire/service slices.

## 2026-05-14 Group E2EE Wire Builder Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The full `awiki-cli` crate test suite reported 72 tests
passing after this slice. Structure check reported no undocumented Rust files
over 1200 lines; `message/group_e2ee_wire.rs` is 845 lines and
`tests/message_group_e2ee_wire_contract.rs` is 526 lines. Dependency audit
remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `BuildGroupE2EECreateRPCParams`, including service target,
  `group.e2ee.create`, `group-e2ee` security profile, and state-ref propagation.
- Go `BuildGroupE2EEAddRPCParams`, `BuildGroupE2EERemoveRPCParams`,
  `BuildGroupE2EELeaveRequestRPCParams`, and `BuildGroupE2EELeaveRPCParams`,
  including membership commit bodies, reason/request ID handling, actor/subject
  fields, and hidden control-plane security-profile differences.
- Go `BuildGroupE2EESendRPCParams`, including
  `application/anp-group-cipher+json`, caller-provided operation/message IDs,
  and opaque cipher sanitization.
- Go KeyPackage request builders:
  `BuildGroupE2EEPublishKeyPackageRPCParams`,
  `BuildGroupE2EEGetKeyPackageRPCParams`,
  `BuildGroupE2EEGetRecoveryKeyPackageRPCParams`, and
  `BuildGroupE2EEGetUpdateKeyPackageRPCParams`, including transport-protected
  service targets, device defaults, and public KeyPackage sanitization.
- Go recovery/update/notice/head builders, including P4-field avoidance,
  recovery/update key package id selection, nested package purpose defaults,
  notice limit capping and ID trimming, and head state-ref shape.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice constructs hidden E2EE request params only. It does
not execute `anp-mls`, call hidden P6 message-service APIs, refresh JWT
sessions, select HTTP/WebSocket transport, mutate cache, or validate live MLS
storage/security behavior. Those remain dedicated group-E2EE service/provider
translation tasks.

## 2026-05-14 Doctor Local Diagnostics Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The focused doctor contract test covers the Go 10-check report
shape, counts/summary precedence, empty and initialized workspaces, invalid ANP
service configuration, SQLite contact-handle binding diagnostics, and a fake
`anp-mls system version --json-in -` compatibility probe with scoped MLS state.
The full `awiki-cli` crate test suite reported 76 tests passing after this
slice. Structure check reported no undocumented Rust files over 1200 lines;
`doctor/mod.rs` is 1002 lines and `tests/doctor_contract.rs` is 271 lines.
Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `Report`, `Check`, and `Counts` JSON contract.
- Go check order: `build`, `config_file`, `environment`, `anp_service`,
  `runtime`, `identity_store`, `sqlite`, `anp_mls`, `workspace_upgrade`, and
  `legacy_paths`.
- Local diagnostics through already translated config, identity, store, runtime,
  and legacy-scan modules.
- External `anp-mls` health probe through `std::process::Command`; no Rust
  dependency was added.

Boundary note: this slice does not implement real platform service-manager
status, auth/session checks, HTTP/WebSocket transport, MLS provider execution,
full Go YAML parse-error parity, or full `upgrade.Inspect` meta/journal
semantics. Those remain separate parity slices.

## 2026-05-14 Config Writer Helper Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test config_writer_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed for the focused helper tests plus existing core/runtime contract
tests. The full `awiki-cli` crate suite reported 82 tests passing after this
slice. Structure check reported no undocumented Rust files over 1200 lines;
after the follow-up file split, `config/mod.rs` is 829 lines,
`config/write.rs` is 453 lines, and `tests/config_writer_contract.rs` is 264
lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

The helper contract test covers Go `internal/config/write.go` behavior for
schema-version stamping, missing-config no-op schema upgrades, active identity
trimming, runtime/listener field writes, DID-domain normalization, host-notify
enabled false persistence, `webhook` alias normalization, OpenClaw hook/token
writes, Hermes notify/deliver/secret writes, one-shot Hermes host-notify setup,
and legacy webhook double-write behavior.

The same test file also covers the Go-style durable write path: config writes
use a same-directory `.config-*.tmp` temporary file, leave no temp file after a
successful replacement, create a missing config directory, and on Unix produce
`0600` config files plus `0700` newly-created config directories. The writer
uses standard-library file sync and parent-directory sync on Unix, with the same
intentional Windows parent-directory sync no-op as Go `internal/durablefs`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Scope:

- Helper-level config mutators only.
- No new crate dependency.
- Existing direct-helper boundaries are preserved where Go helpers differ from
  CLI validation, for example host-notify sink validation remains at the CLI
  boundary and the helper maps only the legacy `webhook` alias.

Boundary note: this slice does not claim full `yaml.v3` parser/serializer
parity, Hermes CLI command wiring, Hermes bridge management, listener refresh
orchestration, or platform service-manager behavior. Windows replacement
behavior still needs Windows CI/manual evidence before the durable writer is
claimed as cross-platform runtime complete.

## 2026-05-14 Update Registry Fetch/Writeback Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli update --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cd ../awiki-cli && go test ./internal/update
```

Result: passed. The Rust update-focused run covered 9 update module tests plus
the existing filtered contract tests; `update_contract` reported 3 passed. The
Go reference `internal/update` tests passed.

Live smoke on this host, using the configured proxy/VPN environment:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR=/tmp/awiki-cli-rs2-update-smoke-$$ \
cargo +1.79.0 run -p awiki-cli --bin awiki-cli --locked -- \
  upgrade --dry-run --format json
```

Result: passed. Output included `update_metadata_source: "network"`,
`update_check_status: "ok"`, `latest_version: "1.0.16"`, and
`min_supported_version: "1.0.16"`.

Dependency audit:

```bash
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Result: the update slice introduced `rustls`, `rustls-webpki`,
`rustls-pki-types`, and `webpki-roots`. No OpenSSL, `native-tls`, `reqwest`, or
`hyper` path was present. The audit also showed the already-approved bundled
SQLite path (`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`) and the
Rustls crypto provider build path (`ring -> cc`). `ring -> cc` is recorded in
`docs/dependency-decisions.md` as native build surface for Rustls, not as
OpenSSL/native-tls or a host TLS dependency.

Scope:

- Go `internal/update.fetchFromRegistry*` registry order and first-success
  behavior: npmjs first, npmmirror fallback.
- 3-second connect/read/write timeout on the blocking GET path.
- HTTP 200-only success, required npm `version`, optional
  `awikiCli.minSupportedVersion`, and combined all-registry error text.
- `CheckFresh` prefers network over fresh cache.
- `AWIKI_CLI_UPDATE_CACHE_ONLY` avoids network and uses cached metadata.
- Successful network fetch writes `<cache>/update/metadata.json`; on Unix the
  update cache directory is `0700` and the metadata file is `0600`.
- Network failure falls back to the cached metadata snapshot with
  `metadata_source = "cache_stale"` when one is available.
- `HTTP_PROXY`/`HTTPS_PROXY` and `NO_PROXY` are supported for the narrow update
  fetch path, including HTTP CONNECT for HTTPS registries.

Boundary note: this is a narrow blocking GET helper for npm registry metadata,
not the shared authsdk/service HTTP or WebSocket client. Broader mail, page,
site, message, attachment, and group service transports still need dedicated
Rustls-backed client decisions and service-backed tests.

## 2026-05-14 Store Shared Helpers Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_helpers_contract --test store_import_contract --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused helper contract covers direct/group/unknown
`MakeThreadID` behavior and UTC RFC3339 timestamp shape, and module-level unit
tests cover Go second-precision timestamp formatting, normalization helpers,
`defaultInt64Ptr`-style fallback behavior, and generated `local-<nanos>` IDs.
The import and rebind contract tests were re-run because they are the current
consumers of the extracted helpers. The Go reference `internal/store` tests
passed and remain the parity source for the helper behavior.

Scope:

- Added a split `store/helpers.rs` module for the already-translated Go
  `internal/store/helpers.go` primitives used by legacy import and rebind:
  UTC timestamps, thread ID construction, owner/credential trimming, nullable
  string/bool/int/float coercions, metadata trimming, bool/default helpers, and
  generated `local-<nanos>` IDs.
- Exported only `make_thread_id` and `now_utc` publicly because current tests
  and later modules need those stable helper contracts outside the store module;
  other helpers stay crate-internal through the `store::helpers` module.
- Reduced `crates/awiki-cli/src/store/import.rs` from 1197 to 1117 lines and
  kept `crates/awiki-cli/src/store/rebind.rs` small before starting the larger
  recover-merge translation.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

No `awiki-system-test` selector was run for this slice because these helpers
are store internals and are not newly exposed through a public CLI path. The
later CLI integration slice must use subprocess coverage when public commands
begin exercising recover/replace store merge behavior.

## 2026-05-14 Store Rebind Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused Rust contract has 5 tests covering missing DB soft
no-op behavior, existing DB missing-table error propagation, full owner-DID
rebind plus E2EE cleanup, `UPDATE OR IGNORE` conflict behavior, and
empty/same-owner no-op rules. The Go reference `internal/store` tests passed
and remain the parity source for `RebindLocalIdentityState`, `RebindOwnerDID`,
and `ClearOwnerE2EEData`.

Scope:

- Added a split `store/rebind.rs` module instead of expanding
  `store/import.rs`, which remains 1197 lines and close to the default
  1200-line review threshold.
- Preserved Go's local post-replace-DID store helper behavior:
  missing database returns zero counts without opening/creating SQLite, and the
  missing-DB `store_rebind` map has the same five legacy table keys as Go.
- Preserved the no implicit migration boundary: an existing SQLite file without
  the expected store tables returns the SQLite missing-table error instead of
  creating schema inside rebind.
- Preserved Go `RebindOwnerDID` behavior: trim old/new owners, no-op for empty
  or equal owners, transactionally count and `UPDATE OR IGNORE` `messages`,
  `contacts`, `contact_handle_bindings`, `relationship_events`, `groups`, and
  `group_members`, returning pre-update old-owner row counts.
- Preserved Go `ClearOwnerE2EEData` behavior: trim owner, no-op for empty owner,
  then count and delete old-owner rows from `e2ee_outbox` and `e2ee_sessions`
  while preserving new-owner E2EE rows.

Boundary note: this is a store-only slice. It does not implement non-dry-run
`id replace-did`, identity key replacement, `did-auth.replace_did`, remote
authsdk/session calls, or upgrade K1 replacement integration. Those remain
later identity/authsdk/service slices.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

No `awiki-system-test` selector was run for this slice because the translated
helpers are not yet wired into a public non-dry-run CLI command. The later
identity/authsdk integration slice must add subprocess coverage when
`id replace-did` begins calling these store helpers.

## 2026-05-14 Store Recover Merge Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_recover_merge_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused Rust contract has 6 tests covering missing
database soft no-op, existing empty database schema creation,
target-owner-only migration and old-owner E2EE cleanup, conflict merge
algebra, current contact-handle normalization by latest timestamp then DID
DESC, and global `relationship_events.event_id` conflict behavior. Full Rust
crate tests, structure check, binary build, Go `internal/store` tests, and
dependency audit also passed.

Scope:

- Added split recover-merge modules for Go `internal/store/recover_merge.go`:
  `store/recover_merge.rs` for orchestration, `store/recover_merge/records.rs`
  for row normalization and merge algebra, and `store/recover_merge/sql.rs`
  for query/upsert/count/delete helpers.
- Preserved Go's zero-count map shape for store merge and E2EE cleanup, and
  the missing-DB no-op that does not open or create SQLite.
- Preserved Go's existing-empty-DB behavior for this path: open the DB,
  `ensure_schema`, then perform a zero-count transaction.
- Preserved normalized old-owner filtering: trim, skip empty owners, skip the
  normalized new owner, and deduplicate while preserving input order.
- Preserved one transaction across message/contact/binding normalization,
  current-handle normalization, relationship event, group, member, and E2EE
  cleanup steps.
- Preserved table-specific merge behavior: message self-DID/thread remapping,
  incoming non-empty strings, max sequence/counts, later or earlier timestamp
  rules, bool OR, contact handle normalization, global relationship event
  conflict on `event_id`, group/member self-DID remapping, and old-owner E2EE
  delete-only cleanup.

Structure note: the Rust implementation is split into 354/794/617-line source
files, all under the default 1200-line limit. No file-size exception is needed.

Boundary note: this is a store-only slice. It does not wire non-dry-run
`id recover`, `id replace-did`, authsdk recovery/replace calls, or any public
CLI execution path. `awiki-system-test` is therefore deferred until the CLI
slice calls this helper as a subprocess-observable behavior.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies. TLS policy remains
Rustls-first; bundled OpenSSL is not used or newly approved by this slice.

## 2026-05-14 Store Legacy Import Row-Normalization Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract debug_db_import_v1 --locked
cd ../awiki-cli && go test ./internal/store
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
    -q
```

Result: passed for the Rust focused store-import contract tests and the existing
CLI dry-run/missing-path selector. The Go reference `internal/store` tests
passed and are the parity source for the three Rust scenarios. The full
`tests_v2/debug/test_debug_cli.py` file was not accepted for this slice because
two selectors require non-dry-run `msg send`, which is still deferred to the
message-service slice.

Scope:

- Replaced the temporary generic table-copy importer with Go `import.go` style
  per-table row normalization for `messages`, `e2ee_outbox`, `contacts`,
  `groups`, `group_members`, `relationship_events`, and `e2ee_sessions`.
- Preserved legacy scan behavior, including direct `.db` path handling,
  `<legacy_data_dir>/database/awiki.db` fallback, soft missing-file scan, schema
  version read, and sorted table list.
- Added a store-layer `LegacyOwnerLookup` input so the CLI can pass identity
  summaries without creating a Rust module dependency cycle. This preserves Go's
  `ownerByCredential` and default-owner inference behavior.
- Preserved Go import guards and defaults: pre-v6 imports require an inferred
  owner; missing tables are skipped and sorted; invalid rows with missing
  required keys are skipped where Go skips them; empty thread/content/status
  fields receive Go-equivalent defaults.
- Preserved contact handle-binding side effects for imported contacts.

Structure note: `crates/awiki-cli/src/store/import.rs` is currently 1197 lines.
That is below the default 1200-line Rust source limit and remains a deliberate
file-level translation of Go `internal/store/import.go`. If the next store slice
adds more behavior, split helper writers into a sibling store module before
expanding this file further.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

## 2026-05-14 Workspace Upgrade Inspection Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract config_show_reports_resolved_configuration_snapshot --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed. The focused Rust contract covers empty-workspace detection,
Go-compatible upgrade path resolution, `Meta`/`Journal` JSON load/save,
config/index/SQLite/legacy-settings detection, doctor warning precedence, and
Go zero-value JSON compatibility for partial `Meta`/`Journal` files. It also
covers `config show` embedding the inspection shape instead of the earlier stub
snapshot. The focused Go tests remain the reference for empty workspace
behavior, current-workspace metadata stamping, and legacy settings rejection
boundaries.

Scope:

- Added `crates/awiki-cli/src/upgrade/{types,meta,journal,detect}.rs` for Go
  `internal/upgrade/{types,meta,journal,detect}.go` read-only surfaces.
- Preserved Go workspace schema version `3` separately from the local SQLite
  schema version `12`.
- Preserved Go path shape for `upgrade/meta.json`,
  `upgrade/upgrade_journal.json`, `upgrade/upgrade.lock`, `upgrade/backups`,
  legacy `config.json`, and legacy `config/settings.json`.
- Preserved missing meta/journal behavior as `null`, parse/read failures as
  inspection errors, zero-value defaults for partial JSON files, and pretty JSON
  save behavior through an atomic local write plus durable parent-directory
  sync.
- Reused existing Rust identity index loading, legacy identity scanning,
  read-only SQLite opening, SQLite `user_version` reading, and legacy database
  scanning rather than duplicating those subsystems inside doctor or app code.
- Wired the shared inspection into `doctor` workspace-upgrade checks and
  `config show` workspace-upgrade snapshots.

Structure note: new Rust files are small and split by Go file responsibility;
no file-size exception is needed.

Boundary note: this slice does not implement full `UpgradeIfNeeded`, lock file
ownership, backup/rollback handling, legacy settings migration, identity
replacement RPC, legacy database import execution, cleanup migrations, or root
preflight workspace upgrade execution. Those remain dedicated workspace
migration/auth/service slices.

No dependency was added. Cargo manifests and lockfile were unchanged. This slice
uses existing serde/std helpers, the existing durable directory sync helper, the
existing identity/store modules, and the already-approved `rusqlite + bundled`
SQLite lane. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, or file-lock dependencies.

## 2026-05-14 Workspace Legacy Settings Parser Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run TestLoadLegacySettingsRejectsSplitServiceURLs -count=1
```

Result: passed. The focused Rust tests cover Go-compatible legacy settings JSON
parsing, service URL normalization, split `user_service_url`/`molt_message_url`
rejection, DID domain pass-through, websocket receive-mode detection, HTTP
fallback for non-websocket modes, and Go-style read/parse error prefixes. The Go
focused test remains the source of truth for the split service URL rejection
message.

Scope:

- Added `crates/awiki-cli/src/upgrade/settings.rs` for the pure
  `loadLegacySettings` helper in Go `internal/upgrade/migration_v0_to_v1.go`.
- Preserved Go's JSON field names: `user_service_url`, `molt_message_url`,
  `did_domain`, and `message_transport.receive_mode`.
- Preserved Go's URL behavior: trim and strip trailing slashes before comparing
  service URLs; choose `user_service_url` when present, otherwise
  `molt_message_url`.
- Preserved Go's runtime-mode behavior: `websocket` only when receive mode is
  `websocket` case-insensitively; all other values map to `http`.

Boundary note: this slice only parses legacy settings. It does not write the
canonical `config.yaml`, run `workspaceV0ToV1Migration`, import legacy
identities, import legacy SQLite, call DID replacement/auth/service RPCs,
create backups, acquire upgrade locks, write journal phases, or clean legacy
skill/listener artifacts.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing serde JSON support and the existing config base-URL normalizer. It
does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session,
platform service-manager, backup, or file-lock dependencies.

## 2026-05-14 Workspace Upgrade File Lock Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock' -count=1
```

Result: passed. The Rust tests mirror the Go `TestAcquireFileLock*` suite:
persistent metadata, reusable `upgrade.lock` anchor, concurrent OS-lock
rejection, residual new-format metadata overwrite, corrupt metadata overwrite,
dead legacy PID ignore, old live legacy PID ignore, and fresh live legacy PID
rejection without overwriting the legacy metadata.

Scope:

- Added `crates/awiki-cli/src/upgrade/lock.rs` for Go
  `internal/upgrade/{lock.go,lock_nonwindows.go,lock_windows.go}`.
- Added the Rust equivalent of Go's private `lockMetadata` JSON shape to
  `crates/awiki-cli/src/upgrade/types.rs`.
- Preserved Go's lock order: create parent dir, open/create `upgrade.lock`,
  acquire the nonblocking OS lock, inspect existing metadata, reject only active
  fresh legacy locks, write fresh metadata, and leave the file in place after
  release.
- Preserved Go's compatibility rules for legacy metadata: ignore empty/corrupt
  files, ignore `os_file_lock_v1` residual metadata, ignore dead or older than
  24-hour legacy PIDs, and reject live recent legacy PIDs.

Boundary note: this slice only translates the local file-lock primitive. It does
not wire the lock into full `UpgradeIfNeeded`, journal phase transitions,
backup/rollback creation, migration execution, identity replacement RPC, legacy
SQLite import, or cleanup migrations.

No dependency was added. Cargo manifests and lockfile were unchanged. Unix uses
direct FFI for `flock` and `kill(0)` to match Go's `syscall` behavior; Windows
source parity uses direct FFI for `LockFileEx`, `UnlockFileEx`, and
`OpenProcess`, but Windows runtime behavior still needs a later Windows host
validation pass. The slice does not introduce HTTP/TLS, OpenSSL, `native-tls`,
WebSocket, authsdk session, platform service-manager, backup, or file-lock crate
dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Workspace Upgrade Fsutil Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::fsutil --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. There is no direct Go unit test for private `fsutil.go`
helpers, so Rust adds focused module tests for the local helper contract and
keeps Go focused upgrade tests as reference coverage for existing upgrade helper
callers.

Scope:

- Added `crates/awiki-cli/src/upgrade/fsutil.rs` for Go
  `internal/upgrade/fsutil.go`.
- Moved the existing workspace meta/journal atomic writer out of `meta.rs` and
  into the shared upgrade fsutil module.
- Preserved Go path helper behavior: empty paths and wrong file types return
  false.
- Preserved Go atomic write behavior: create parent dir, same-directory
  `.upgrade-*.tmp`, write/sync/close temp file, chmod temp file on Unix, rename,
  remove temp file on failure, and sync the parent directory through
  `durablefs`.
- Preserved Go copy helper behavior for future backup execution: direct
  truncate-and-copy file writes, destination parent creation, destination file
  sync, missing tree source no-op, recursive tree copy, directory `0700`
  creation, and Unix source file mode preservation.

Boundary note: this slice does not implement `CreateBackup`,
`backupSQLiteDatabase`, SQLite `VACUUM INTO`, rollback, journal phase handling,
or full `UpgradeIfNeeded` execution. Those remain dedicated workspace migration
slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses standard library filesystem APIs, serde JSON, and the existing `durablefs`
helper. It does not introduce filesystem-copy crates, HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager, backup
runtime execution, or file-lock dependencies.

## 2026-05-14 Workspace Upgrade Backup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli backup --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. The Rust backup tests cover fixed backup IDs, Go backup file
names, absent input skipping, identity tree copying, meta/journal/config copies,
SQLite `.bak` generation via `VACUUM INTO`, destination replacement, and
single-quote escaping in the destination path. Go has no direct unit test for
private backup helpers, so focused Go upgrade tests remain reference coverage
for existing callers and local invariants.

Scope:

- Added `crates/awiki-cli/src/upgrade/backup.rs` for Go
  `internal/upgrade/backup.go`.
- Used existing `upgrade::Paths` as the Rust input boundary because Go
  `CreateBackup` only reads `uc.Paths` fields for backup assembly. A full
  upgrader `Context` wrapper remains a later execution slice.
- Preserved Go backup names: `config.yaml.bak`, `config.json.bak`,
  `identities/`, `awiki-cli.db.bak`, `meta.json.bak`, and
  `upgrade_journal.json.bak`.
- Preserved Go's skip-missing-input behavior and final sync of the parent of
  the backup directory.
- Preserved Go SQLite backup behavior: create destination parent, remove an
  existing destination, open source through the normal writable store path,
  escape single quotes by doubling them, and execute `VACUUM INTO`.

Boundary note: this slice does not wire backup execution into
`UpgradeIfNeeded`, journal phase transitions, rollback, migration application,
identity replacement RPC, legacy SQLite import, or cleanup migrations.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the existing fsutil module, existing store open path, and already-approved
`rusqlite + bundled` SQLite lane. It does not introduce filesystem-copy crates,
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, or file-lock dependencies. TLS policy remains Rustls-first and
unchanged.

## 2026-05-14 Workspace Upgrade Upgrader Plan Skeleton Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_default_upgrader_plan_matches_go_migration_chain --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_plan_errors_match_go_messages --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_context_and_is_done_use_go_paths_and_meta_version --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. Go has no standalone unit test for `Upgrader.Plan`; source
parity is checked against `internal/upgrade/types.go`, `upgrader.go`, and the
three migration identity methods in `migration_v0_to_v1.go`.

Scope:

- Added `crates/awiki-cli/src/upgrade/upgrader.rs` for the planning boundary in
  Go `internal/upgrade/types.go` and `internal/upgrade/upgrader.go`.
- Added a Rust `Context` matching the Go execution context fields:
  resolved config, upgrade paths, app version, inspection, backup dir, current
  meta, and warnings.
- Added a `Migration` trait with Go-equivalent `from`, `to`, `name`,
  `is_done`, `apply`, and `validate` methods.
- Added `new_default_upgrader` with the Go 0->1, 1->2, and 2->3 migration
  names and `LATEST_WORKSPACE_SCHEMA_VERSION` target.
- Preserved Go `Plan` behavior: same-version no-op, newer-than-target error,
  missing contiguous migration error, unexpected target error, and ordered
  contiguous migration output.
- Preserved Go migration `IsDone` semantics for this skeleton by reading
  workspace meta and comparing `workspace_schema_version >= migration.to`.

Boundary note: this slice does not implement `UpgradeIfNeeded`, journal phase
execution, lock acquisition, backup orchestration, migration `Apply`/`Validate`,
identity replacement RPC, legacy SQLite import, rollback, or cleanup. The
default migration `apply` and `validate` methods intentionally return an
explicit deferred-execution error until those file-level migration slices are
translated.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses `std::collections::BTreeMap`, existing `config::Resolved`, and existing
upgrade meta/path types. It does not introduce HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged.

## 2026-05-14 Workspace UpgradeIfNeeded No-Op/Current Preflight Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Extended `crates/awiki-cli/src/upgrade/upgrader.rs` with public
  `upgrade_if_needed`, `Upgrader::upgrade_if_needed`, and
  `Upgrader::upgrade_context_if_needed`.
- Preserved Go's `"upgrade context is required"` error through the explicit
  optional-context method used by tests. The normal Rust convenience function
  takes a non-null `config::Resolved` reference.
- Preserved the first Go `Inspect`/current-meta capture boundary before
  deciding whether work is needed.
- Preserved Go's newer-than-supported error:
  `"workspace schema version N is newer than supported 3"`.
- Preserved Go's empty-workspace no-op behavior: no meta file is created and a
  stale journal is cleared.
- Preserved Go's already-current behavior: if meta reports the latest workspace
  schema version, a stale journal is cleared and meta is left unchanged.
- Added an explicit deferred-execution error when a real migration plan would
  be needed so the partial orchestration cannot silently skip migration work.

Boundary note: this slice intentionally stops before the second inspect under
lock, lock acquisition, backup reuse/creation, journal phase execution,
migration `Apply`/`Validate`, meta stamping after each migration, identity
replacement RPC, legacy SQLite import, rollback, and cleanup. Those remain
dedicated migration-execution slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing inspect/meta/journal/planner helpers and does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Config Apply Branch Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test config_writer_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_config` for the pure config-file branch of
  Go `workspaceV0ToV1Migration.Apply`.
- Preserved existing-config behavior: if canonical `config.yaml` exists, stamp
  it to current config schema version while preserving existing values.
- Preserved legacy-config behavior: parse legacy `config.json`, stamp schema,
  write canonical `config.yaml`, and remove the legacy file.
- Preserved legacy-settings behavior for no-workspace legacy installs: load
  `settings.json`, write runtime mode, service base URL, and DID domain into a
  minimal canonical config file.
- Extended the existing `FileConfig` parser to accept JSON input for legacy
  `config.json`, matching Go's YAML parser accepting JSON, without adding a
  YAML dependency.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not import identities, does not import legacy
SQLite rows, does not ensure target store schema, does not refresh resolved
config after import, does not call k1->e1 replacement RPC, and does not enable
full `UpgradeIfNeeded` phase execution.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config writer plumbing and existing `serde_json`. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Validation Wiring Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Wired the default 0->1 migration `validate` path to a Rust
  `validate_workspace_v0_to_v1` helper matching Go
  `workspaceV0ToV1Migration.Validate`.
- Preserved Go's required-context guard through the optional Rust test hook:
  `workspace upgrade requires a resolved config`.
- Preserved config validation: existing `config.yaml` must have
  `schema_version` equal to the current config schema version.
- Preserved SQLite validation: existing target database must have current store
  schema version and pass `PRAGMA integrity_check` plus
  `PRAGMA foreign_key_check`.
- Preserved post-legacy-identity sanity validation: when inspection says no
  workspace existed but legacy identity existed, at least one imported identity
  must now be present in the local identity store.
- Kept 1->2 and 2->3 migration validation deferred.

Boundary note: this slice still does not implement v0->v1 `Apply`, identity
import, legacy SQLite import, legacy config write/remove, k1->e1 replacement
RPC, full `UpgradeIfNeeded` phase execution, journal phase writes, lock/backup
orchestration, meta stamping, rollback, or legacy skill/listener cleanup.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the existing hand-written config parser, local identity manager, already
approved `rusqlite + bundled` store path, and SQLite health helper. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Legacy Local Import Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_legacy_imports` for the local legacy import
  branch of Go `workspaceV0ToV1Migration.Apply`.
- Preserved the Go guard: import runs only when inspection reports no existing
  workspace and some legacy state exists.
- Preserved Go import order: scan/import legacy identities first, then scan the
  legacy SQLite database, open the target store, ensure target schema, build
  owner lookup from imported identities, and import legacy rows.
- Covered both no-op guards: existing workspace skips local imports, and
  `has_legacy=false` skips local imports even when legacy fixtures are present.
- Covered the order-sensitive pre-v6 success path: a schema v5 legacy SQLite
  message imports only after the preceding legacy identity import provides the
  owner DID needed by the store importer.
- Preserved the missing-context and missing-inspection errors used by the
  split Rust helper surface.
- Covered the pre-v6 owner guard from the store importer:
  `unsupported legacy sqlite schema version: legacy schema < 6 requires at
  least one imported identity so owner_did can be inferred`.
- Kept the helper separate from full migration execution so it can be wired
  into `WorkspaceMigration.Apply` later with lock/backup/journal/meta evidence.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not call `ensureTargetStoreSchema` after local
imports, does not refresh resolved config after import, does not call k1->e1
DID replacement RPC, and does not enable full `UpgradeIfNeeded` phase
execution.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing identity import helpers and the already-approved
`rusqlite + bundled` SQLite import path. It does not introduce HTTP/TLS,
OpenSSL, `native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged. The dependency audit showed no OpenSSL or
`native-tls`; it only reported existing Rustls/ring and approved bundled
SQLite build surfaces.

## 2026-05-15 Workspace v0->v1 Local Apply Composition Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_local_state` as a focused composition helper
  for the local portions of Go `workspaceV0ToV1Migration.Apply`.
- Preserved local Go ordering: config branch first, legacy identity/SQLite
  import second, target schema ensure when the target database exists, and
  resolved config/path refresh when legacy identities were imported.
- Covered the resolved-context refresh path from legacy settings: runtime mode,
  service base URL, DID domain, derived ANP endpoint/DID, and path refresh.
- Covered the final target schema ensure branch directly with an existing empty
  target database and no legacy SQLite import path.
- Preserved the intentional boundary that `Migration::apply` itself still
  returns the deferred execution error and `context.warnings` is untouched
  because automatic k1->e1 DID replacement is not translated in this slice.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not call automatic k1->e1 DID replacement, and
does not enable full `UpgradeIfNeeded` phase execution, journal phase writes,
backup/rollback, meta stamping, or legacy skill/listener cleanup.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config, identity, store, and refresh helpers. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged. The dependency audit showed no
OpenSSL or `native-tls`; it only reported existing Rustls/ring and approved
bundled SQLite build surfaces.

## 2026-05-14 Workspace SQLite Migration Helpers Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Result: passed. Go has no dedicated unit test for the private SQLite helper
functions; source parity is checked against
`internal/upgrade/migration_v0_to_v1.go`, and store schema version behavior is
validated through existing Go store tests.

Scope:

- Extended `crates/awiki-cli/src/upgrade/migration_v0_to_v1.rs` with
  `ensure_target_store_schema`, `validate_sqlite_health`, and Go-equivalent SQL
  expectation helpers.
- Preserved Go `ensureTargetStoreSchema` behavior by opening the target store
  through the existing writable store path and delegating to `store::ensure_schema`.
- Preserved store schema version errors for newer and too-old SQLite databases.
- Preserved Go `validateSQLiteHealth` behavior: run `PRAGMA integrity_check`,
  accept case-insensitive trimmed `ok` and an empty trimmed result, then require
  `PRAGMA foreign_key_check` to return no rows.
- Preserved Go helper error text for non-ok integrity and foreign-key
  violations.
- Split v0->v1 helper tests into
  `crates/awiki-cli/tests/workspace_migration_v0_to_v1_contract.rs` so
  `workspace_upgrade_contract.rs` stays under the 1200-line default file-size
  limit.

Boundary note: this slice still does not wire v0->v1 `Apply`/`Validate` into
`Migration`, run `UpgradeIfNeeded` phase execution, write/remove legacy config
files, import identities, import legacy SQLite rows, call identity replacement
RPC, or clean legacy skill/listener artifacts.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the already-approved `rusqlite + bundled` SQLite path and existing store
APIs. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, filesystem-copy, file-lock, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Workspace Refresh Resolved Config Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_refresh_resolved_config --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestRefreshResolvedConfigSyncsMailServiceURLFromConfig|TestRefreshResolvedConfigDerivesMailServiceURLFromServiceBaseURL' -count=1
```

Result: passed.

Scope:

- Added `crates/awiki-cli/src/upgrade/migration_v0_to_v1.rs` for the first
  pure helper from Go `internal/upgrade/migration_v0_to_v1.go`.
- Translated `refreshResolvedConfig` as `refresh_resolved_config` plus an
  optional-context test hook preserving Go's `"resolved config is required"`
  error.
- Preserved Go's config-file refresh behavior for config existence,
  `schema_version`, runtime mode/socket path, output format/no-color,
  service base URL, DID domain, ANP endpoint/DID, mail service URL, and CA
  bundle.
- Preserved Go's mail URL rules: explicit `services.mail_service_url` is
  normalized and copied from config; when config omits it, mail service URL is
  derived from `service_base_url` only if the current resolved mail URL is
  empty.
- Exposed the existing hand-written `config::read_file_config` as crate-local
  so upgrade helpers can reuse the same parser without adding a YAML
  dependency.

Boundary note: this slice does not implement v0->v1 `Apply`, config file
migration, legacy config removal, identity import, legacy SQLite import,
target-store schema enforcement, SQLite health validation, k1->e1 DID
replacement RPC, or `UpgradeIfNeeded` phase execution. Those remain separate
file-level migration slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config parsing and URL derivation helpers. It does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Root Update Preflight Guard Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli app::update_preflight --locked
cargo +1.79.0 test -p awiki-cli update --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
cd ../awiki-cli && go test ./internal/cli ./internal/update -run 'Test.*Update|Test.*Preflight|Test.*Root|TestIsUpdateExemptCommandAllowsListenerServiceRun' -count=1
```

Result: passed. The full local Rust suite, focused Go reference tests, structure check, build, and dependency audit all passed.

Scope:

- Added `crates/awiki-cli/src/app/update_preflight.rs` for Go
  `internal/cli/root.go` update preflight behavior while keeping `app.rs` below
  the default 1200-line review cap.
- Preserved Go ordering: global `--format` validation runs first, then the
  update policy guard runs before non-exempt command dispatch.
- Preserved Go exemptions for local/recovery commands represented by the
  current Rust parser: version, upgrade, init, docs, schema, config show,
  doctor, completion shells, hidden `runtime listener service-run`, and hidden
  Hermes bridge service-run command names.
- Preserved Go soft-fail behavior: config/update-check failures do not block
  normal command execution; `--verbose` prints
  `[awiki-cli] update check failed: ...` to stderr.
- Preserved Go unsupported-version behavior at the helper/decision boundary:
  non-dev versions below `min_supported_version` produce `version_unsupported`
  with exit code 3 and npm primary/mirror install hints.
- Preserved Go newer-version warning placement by prepending the update warning
  before command-specific warnings.
- Added test-only current-version override in `update/mod.rs` so the blocked
  path can be tested without changing default dev-build behavior.
- Subprocess CLI test helpers set `AWIKI_CLI_UPDATE_CACHE_ONLY=1` to keep
  unrelated CLI contract tests deterministic and offline after preflight became
  global. This is a test isolation choice only; production still follows
  `update::check` cache/network behavior.

No dependency was added. Cargo manifests and lockfile were unchanged. This
slice reuses the existing direct Rustls update registry path and the approved
`rusqlite + bundled` SQLite path. It does not introduce OpenSSL,
`native-tls`, reqwest, hyper, WebSocket, YAML, platform service-manager, or new
SQLite dependencies.

## 2026-05-15 AuthSDK Header/Challenge Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli authsdk --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
cd ../awiki-cli && go test ./internal/authsdk -count=1
```

Result: passed. The full local Rust suite, focused Go reference test, structure
check, build, and dependency audit all passed.

Scope:

- Added Go-shaped `Session::headers` and `Session::challenge_headers` wrappers
  in `crates/awiki-cli/src/authsdk/mod.rs`.
- Preserved Go's normal request header contract: start with
  `Content-Type: application/json`, delegate to the local ANP Rust SDK
  `DIDWbaAuthHeader::get_auth_header`, and merge returned auth headers into the
  JSON base header map.
- Preserved Go's cached bearer behavior: `force_new=false` can return cached
  `Authorization: Bearer ...`; `force_new=true` bypasses that cache and emits
  HTTP Message Signature headers.
- Preserved Go's challenge helper boundary: response headers are passed to the
  ANP helper, the JSON base headers are available for `Accept-Signature`
  covered-component normalization, and returned challenge headers remain
  auth-only like Go.
- Added no-network tests that generate real local DID documents and private
  keys through `../anp/rust`, proving `Signature-Input`, `Signature`,
  `Content-Digest`, key id, bearer cache reuse, force-new signing, and server
  nonce reuse without selecting an HTTP client.

No dependency was added. Cargo manifests and lockfile were unchanged. This
slice reuses the local ANP Rust SDK with default features disabled and does not
introduce HTTP/TLS clients, OpenSSL, `native-tls`, reqwest, hyper, WebSocket,
YAML, platform service-manager, or new SQLite dependencies.

## 2026-05-15 ANP SDK Registry Facade Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
cd ../awiki-cli && go test ./internal/anpsdk -count=1
```

Result: passed. The full local Rust suite, AuthSDK regression test, Go registry
compile check, structure check, build, and dependency audit all passed.

Scope:

- Expanded `crates/awiki-cli/src/anpsdk.rs` from the initial auth-header-only
  facade toward Go `internal/anpsdk/registry.go`, but limited the change to
  existing public local Rust SDK symbols.
- Re-exported authentication equivalents for DID profiles, DID document
  options/bundles, ANP message service options/builder, DID resolver functions,
  auth/signature helpers, verifier/config/options, and root key material.
- Re-exported proof equivalents for IM proof helpers, RFC9421 origin proof
  helpers, DID-WBA binding/group receipt helpers, `TargetKind`, and Rust-cased
  proof option/result structures.
- Re-exported direct-E2EE model/session state types that already exist in the
  Rust SDK, including `PrekeyBundle`, `SignedPrekey`, `OneTimePrekey`,
  `DirectSessionState`, and `DirectE2eeSession`.
- Added a focused facade contract test that exercises the re-exported symbols
  through `awiki_cli::anpsdk` instead of importing `anp` directly.

Deferred:

- Go `KeyType`, `GenerateKeyPairPEM`, free `PrivateKeyFromPEM` and
  `PublicKeyFromPEM`, `GeneratedKeyPairPEM`, file-backed direct-E2EE stores,
  and high-level `MessageServiceE2EEClient` still do not have exact public Rust
  SDK equivalents. They remain deferred until a consuming translated module
  requires an explicit SDK/facade lane.

No dependency was added. Cargo manifests and lockfile were unchanged. This
slice keeps local ANP SDK default features disabled and does not introduce
HTTP/TLS clients, OpenSSL, `native-tls`, reqwest, hyper, WebSocket, YAML,
platform service-manager, or new SQLite dependencies.

## 2026-05-16 ANP File Session Store Facade Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli && go test ./internal/anpsdk -count=1
cd ../awiki-cli && go test ./internal/message ./internal/runtime/listener -run 'TestServiceSecureStatusReturnsSessionAndOutboxSummary|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1
```

Result: passed after final verification.

Scope:

- Added `anpsdk::FileSessionStore` as the CLI facade equivalent of Go ANP SDK
  `NewFileSessionStore` for the direct-E2EE session store boundary.
- Preserved Go file behavior: constructor creates the root session directory,
  session files are named `<session_id>.json`, save writes pretty JSON without
  a trailing newline, load round-trips `DirectSessionState`, missing load maps
  to `DirectE2eeError::SessionNotFound`, delete ignores missing files, and
  `find_by_peer_did` scans sorted `*.json` paths.
- Preserved Go lookup semantics: exact `peer_did` equality, first
  lexicographic path match when multiple sessions share a peer DID,
  missing-root/no-match returning `None`, and malformed JSON in any scanned
  `*.json` aborting the lookup.
- Implemented the local ANP Rust `SessionStore` trait for the facade store so
  later secure-direct slices can depend on the trait methods for save/load/delete.
- Added focused contract tests through `awiki_cli::anpsdk` so downstream code
  does not need to import `anp` directly for this Go-shaped boundary.

Boundary note: this is a local facade helper slice, not the full Go
`direct_e2ee` SDK store/client port. Signed-prekey stores, one-time prekey
stores, pending outbound stores, `FindByPeerDID` on the upstream Rust SDK trait,
prekey publishing, no-session init, DID resolution, high-level
`MessageServiceE2EEClient`, WebSocket/RPC send execution, SQLite queued flush
mutation, `currentSecureSessionID`, and full `SecureRetry` remain deferred
parity slices.

Parallelism note: the focused `anpsdk_contract` tests were added by a
code-writing Native Agent launched with GPT-5.5 and xhigh reasoning under a
test-only write scope.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the local ANP Rust SDK with default features
disabled plus std filesystem APIs and existing `serde_json`. It does not
introduce ANP `network`/default features, `reqwest`, `hyper`, WebSocket crates,
OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service
libraries, or new SQLite dependencies.

## 2026-05-16 ANP File Prekey Store Facade Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/anpsdk -count=1
cd ../awiki-cli && go test ./internal/message ./internal/runtime/listener -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation|TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession' -count=1
```

Result: passed after final verification. `anpsdk_contract` now has 14 tests.

Scope:

- Added `anpsdk::FileSignedPrekeyStore` as the CLI facade equivalent of Go ANP
  SDK `NewFileSignedPrekeyStore` for the direct-E2EE signed-prekey boundary.
- Preserved Go file behavior: constructor creates the root directory,
  `save_signed_prekey` writes `<key_id>.pem`, `<key_id>.json`, and
  `latest.txt`; metadata JSON is pretty without a trailing newline;
  `load_signed_prekey` returns private PEM material plus metadata; missing PEM
  maps to `invalid field: signed prekey not found: <key_id>`; and
  `load_latest_signed_prekey` returns `None` for missing `latest.txt` while
  trimming whitespace before loading the latest key.
- Added `anpsdk::FileOneTimePrekeyStore` as the CLI facade equivalent of Go ANP
  SDK `NewFileOneTimePrekeyStore`.
- Preserved Go one-time-prekey behavior: constructor creates the root
  directory, save/load round-trips PEM plus metadata JSON, missing PEM maps to
  `invalid field: one-time prekey not found: <key_id>`, list reads `*.json` and
  sorts by `key_id`, and delete removes both PEM and JSON while ignoring
  missing files.
- Added `anpsdk::FilePendingOutboundStore` as the CLI facade equivalent of Go
  ANP SDK `NewFilePendingOutboundStore`.
- Preserved Go pending-outbound behavior: save/load/delete
  `<operation_id>.json`, pretty JSON without trailing newline, missing load
  maps to `DirectE2eeError::PendingOutboundNotFound`, and delete missing
  succeeds.
- Implemented the local ANP Rust `SignedPrekeyStore` and
  `PendingOutboundStore` traits where the local SDK already exposes matching
  trait surfaces. The local SDK does not currently expose a `OneTimePrekeyStore`
  trait, so the one-time prekey store is intentionally an inherent Go-shaped
  facade until a consuming E2EE client slice requires a trait adapter.

Boundary note: this is still a local facade helper slice, not the full Go
`direct_e2ee` SDK client port. `NewMessageServiceDirectE2eeClient`,
`MessageServiceE2EEClient`, prekey publishing, no-session init, DID resolution,
secure send execution, incoming `ProcessIncoming`, WebSocket/RPC transport,
SQLite queued flush mutation, `SecureRetry`, `SecureInit`, `SecureRepair`, and
awiki-system-test secure-direct acceptance remain deferred parity slices.

Parallelism note: part of the focused `anpsdk_contract` prekey-store tests were
contributed by a code-writing Native Agent launched with GPT-5.5 and xhigh
reasoning under a test-only write scope; the pending-outbound coverage and
integration fixes were completed in the leader lane.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the local ANP Rust SDK with default features
disabled plus std filesystem APIs and existing `serde_json`. It does not
introduce ANP `network`/default features, `reqwest`, `hyper`, WebSocket crates,
OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service
libraries, or new SQLite dependencies. Dependency audit showed only the
existing Rustls/ring chain and the approved bundled SQLite `rusqlite ->
libsqlite3-sys` path.

## 2026-05-15 Identity Recover Dry-Run Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli identity::recover --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/cli ./internal/identity -run 'TestRunIDRecoverDryRun|TestRecover' -count=1
```

Result: passed. Rust focused recover unit tests, the identity CLI contract
suite, the full `awiki-cli` Rust test suite, structure check, build, dependency
audit, and the Go focused recover tests all passed.

Scope:

- Added a split `crates/awiki-cli/src/identity/recover.rs` module for the local
  recover preview subset of Go `internal/identity/recover.go` and
  `internal/identity/service.go`.
- Preserved handle normalization, final identity name derivation, same-handle
  candidate detection, excluded identity conflict detection, stable candidate
  sorting, temporary recovery identity naming, and backup preview path shape.
- Added `id recover` to Rust command metadata, parser dispatch, and app handler
  for `--dry-run`.
- Preserved Go's dry-run branch split: with `--otp`, action is
  `recover_handle` with `did-auth.recover_handle` and local writes including
  `sqlite.recover_handle_merge`; without `--otp`, action is
  `send_recover_otp` with `handle.send_otp`, no local writes, and empty backup
  path.
- Preserved Go's public warning that global `--identity` is ignored by
  `awiki-cli id recover`.

Deferred:

- Non-dry-run OTP sending, `did-auth.recover_handle`, recovered DID/key
  generation and persistence, backup creation, SQLite recovered-state merge,
  promotion/finalization, and recover-specific internal error details remain
  deferred until the shared authsdk/Rustls identity-service execution lane.

No dependency was added. Cargo manifests and lockfile were unchanged. This
slice does not introduce HTTP/TLS clients, OpenSSL, `native-tls`, reqwest,
hyper, WebSocket, YAML, platform service-manager, or new SQLite dependencies.

## 2026-05-15 Workspace v2->v3 No-k1 Local Completion Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::migration_v2_to_v3 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket|serde_yaml|yaml'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/upgrade -run 'TestUpgradeIfNeeded' -count=1
```

Result: passed.

Scope:

- Added `crates/awiki-cli/src/upgrade/migration_v2_to_v3.rs` as the split
  Rust module for the local boundary of Go `workspaceV2ToV3Migration`.
- Wired migration `2 -> 3` so validation is a Go no-op and apply completes
  locally when the current identity store is missing, empty, or contains only
  non-k1 DID suffixes.
- Preserved the explicit deferred boundary for current identity indexes that
  contain a `k1_` DID suffix:
  `workspace_2_to_3_replace_existing_k1_handle_dids`.
- Updated `UpgradeIfNeeded` contract tests so no-k1 workspaces advance to
  workspace schema version 3, clear the journal, preserve backup
  creation/reuse behavior, and update `Context.current_meta`.
- Added current-k1 and current-e1 identity-index coverage for the v2->v3 path.

Boundary note: this slice does not implement service-backed k1->e1 DID
replacement, identity remote-service construction warning parity for non-empty
non-k1 identity stores, remote `did-auth.replace_did`, identity replacement
backups, SQLite owner-DID rebinding, rollback, or full awiki-system-test
migration acceptance. Those remain in the shared authsdk/Rustls
identity-service lane.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the existing identity manager, upgrade journal/meta/backup/lock loop, and
approved bundled SQLite paths only. It does not introduce HTTP/TLS clients,
OpenSSL, `native-tls`, reqwest, hyper, WebSocket, YAML, platform
service-manager, filesystem-copy, file-lock crate, or new SQLite dependencies.

## 2026-05-15 Transportcfg Rustls HTTP Client Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked
cargo +1.79.0 test -p awiki-cli --test transportcfg_contract --locked
cargo +1.79.0 test -p awiki-cli update --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/transportcfg ./internal/update -count=1
```

Result: passed.

Scope:

- Added `crates/awiki-cli/src/transportcfg/http.rs` as the Rustls/std shared
  HTTP/1.1 client boundary for Go `internal/transportcfg.NewHTTPClient`.
- Preserved config snapshot behavior from `transportcfg::resolve`, dial,
  TLS-handshake, and response-header timeout use, optional CA bundle loading,
  `read ca bundle:` / `invalid ca bundle:` error strings, default
  `User-Agent`, status/header/body response shape, and chunked response
  decoding.
- Kept `transportcfg::new_http_client` proxy-free to match Go's custom
  `http.Transport` in `NewHTTPClient`.
- Added `transportcfg::new_http_client_with_proxy_env` for callers that need
  Go default-client proxy semantics; the translated update registry fetch now
  uses that explicit constructor and preserves `HTTP_PROXY`/`HTTPS_PROXY` plus
  `NO_PROXY` bypass without duplicating the `User-Agent` header.
- Refactored update registry fetch to use the shared client while preserving
  existing npm metadata, fallback, cache writeback, and error-string contracts.

Boundary note: this slice does not implement HTTP/2 `ForceAttemptHTTP2`, exact
Go system-root-store parity, keepalive/pooling reuse from idle-connection
settings, streaming request/response bodies, response-header timeout without
reading full body, proxy authentication, HTTPS proxy integration tests, WebSocket
transport, or service-specific auth/error mapping. Those remain in later
service/authsdk/runtime transport slices.

No dependency was added. Cargo manifests and lockfile were unchanged. This slice
uses existing `rustls` + `webpki-roots` and standard-library sockets only; it
does not add `reqwest`, `hyper`, OpenSSL, `native-tls`, bundled OpenSSL,
WebSocket crates, YAML crates, platform service libraries, or new SQLite
dependencies.

## 2026-05-15 Authsdk Rustls HTTP Execution Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked
cargo +1.79.0 test -p awiki-cli update --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/authsdk -count=1
```

Result: passed.

Scope:

- Extended `authsdk::Session` with the generic HTTP execution boundary from Go
  `internal/authsdk/session.go`: `do_json_rpc`, optional JSON-RPC,
  `do_json`, optional plain JSON, `ensure_jwt`, and private request execution.
- Reused the shared Rustls/std `transportcfg::HttpClient` introduced in the
  transport slice instead of adding `reqwest`, `hyper`, OpenSSL, `native-tls`,
  or enabling ANP SDK network features.
- Preserved Go request behavior: JSON request headers, signed or cached-bearer
  first request, one 401 retry via challenge headers when the DID-WBA helper
  accepts the challenge, otherwise clearing the remembered token and retrying
  with force-new signed headers.
- Preserved Go response behavior: final `>=400` responses map to trimmed
  `HttpError`, JSON-RPC response errors map to `RpcError` with data preserved,
  successful response headers are captured before callers decode/fallback, and
  `EnsureJWT` persists body `access_token`, falls back to a captured header
  token, then to stored JWT, then emits the Go missing-token error.

Boundary note: this slice does not wire endpoint-specific live mail, page,
site, identity, or message clients; it does not add profile-timeout wrappers,
trace phases, identity-store command wiring, attachment transfer, WebSocket
transport, or full awiki-system-test service acceptance. Those remain in later
service/client slices now that the shared auth transport is available.

No dependency was added. Cargo manifests and lockfile were unchanged. This slice
uses existing local `../anp/rust` APIs with default features disabled and the
existing Rustls/std transport client. It does not add `reqwest`, `hyper`,
OpenSSL, `native-tls`, bundled OpenSSL, WebSocket crates, YAML crates, platform
service libraries, or new SQLite dependencies.

## 2026-05-15 Mail Live RPC Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test mail_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/mail ./internal/cli -run 'TestNewClientRequiresMailServiceURL|TestServiceSendValidatesRequiredFields|TestServiceAttachmentValidatesIndex|TestMail' -count=1
```

Result: passed after final full verification.

Scope:

- Added `mail::Client` as the Go `internal/mail/client.go` equivalent on top of
  the existing shared Rustls/std `transportcfg::HttpClient`.
- Wired non-dry-run `mail inbox`, `mail read`, `mail mark-read`, `mail account`,
  `mail send`, and `mail attachment download` through the translated
  `authsdk::Session::do_json_rpc` execution layer.
- Preserved Go auth bootstrap behavior: active identity resolution, messaging
  readiness gate, remembered service/did-auth/mail scopes, stored JWT bearer
  seeding, empty-token `get_me` refresh through DID-auth, and persisted JWT
  updates through the identity manager.
- Preserved Go service error mapping for mail CLI commands: bad params to
  `invalid_argument`, auth failures to `auth_required`, missing service objects
  to `not_found`, conflict codes to `conflict`, and remaining errors with the
  command-specific hint.
- Implemented Go attachment download behavior: `content_base64` decode, default
  filename fallback, optional output path, parent directory creation, output
  write, and success payload without echoing the raw attachment body.

Boundary note: this slice does not run full remote `awiki-system-test`
acceptance, and it does not yet translate per-call profile timeout wrappers or
trace phases around mail RPC calls. Page/site/identity/message live service
clients remain separate slices.

Dependency note: added `base64 = 0.22` as a direct pure Rust dependency for the
mail attachment decode path. The crate was already present transitively through
the local ANP SDK. No OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket, YAML, platform service, or new SQLite dependency is added.
Shared profile timeout caps are covered by the later service profile-timeout
slice; trace phases around mail RPC calls remain deferred.

## 2026-05-15 Shared Service Profile Timeout Slice

Status: locally verified.

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test site_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test attachment_live_contract --locked
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
```

Result: passed. Formatting, whitespace, package check, structure check,
focused transport/authsdk/service live contract tests, binary build, and
dependency audit all passed. The full `cargo +1.79.0 test -p awiki-cli
--locked` suite also passed with no failed tests. Focused test counts:
`transportcfg_http_contract` 10,
`authsdk_contract` 15, `mail_live_contract` 3, `page_live_contract` 4,
`site_live_contract` 4, `identity_live_contract` 15, `msg_live_contract` 2,
`group_live_contract` 3, and `attachment_live_contract` 4 all passed with zero
failures. `xtask check-structure` reported
`structure ok: no undocumented Rust files over 1200 lines`. Dependency audit
showed only allowed existing hits: `base64`, `rustls`, `rustls-webpki`,
`webpki-roots`, `ring`, approved `rusqlite`, `libsqlite3-sys`, and build
helpers `cc`, `pkg-config`, and `vcpkg`; no OpenSSL, `native-tls`, `reqwest`,
`hyper`, WebSocket, YAML, or platform service dependency was introduced.

Scope:

- Added an optional per-request timeout cap to the shared Rustls/std
  `transportcfg::HttpRequest` and applied it after request write for HTTP and
  HTTPS response reads.
- Preserved Go `WithProfileTimeout`'s "shorter deadline wins" behavior by
  making request profile timeouts shorten, not extend, the configured base
  HTTP response timeout.
- Added profile-aware authsdk helpers for JSON-RPC, plain JSON, and DID-auth
  `get_me` refresh.
- Wired mail, content, site, identity, and message clients so existing
  `AuthRefresh`, `RpcDefault`, and `RpcReadHeavy` profile selections now reach
  the shared HTTP request path.
- Wired the message JSON-RPC 1401 retry path so DID-auth retry refresh uses the
  `AuthRefresh` profile and the retried RPC keeps the original RPC profile.

Boundary note: the current Rust std blocking HTTP client applies these profile
caps to the response read path. Go context deadlines also cover dial,
TLS handshake, and request write; exact cross-phase deadline cancellation plus
trace phase emission remain later transport/trace parity work.

Parallelism note: the focused `transportcfg_http_contract` tests were added by
a code-writing Native Agent launched with GPT-5.5 and xhigh reasoning under a
test-only write scope, satisfying the recorded parallel-development constraint.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std HTTP client and does not
add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, YAML crates, platform service libraries, or new SQLite dependencies.

## 2026-05-15 Identity Key Compatibility Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cd ../anp/rust
cargo +1.79.0 fmt --check
cargo +1.79.0 test --locked --no-default-features --test key_pem_tests
cd ../../awiki-cli-rs2
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test identity_key_compat_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_replace_did_live_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|base64'
cd ../awiki-cli
go test ./internal/identity -run 'TestManagerLoadMigratesLegacyANPPrivateKeysToPKCS8|TestReplaceDIDConvertsLegacyANPK1KeyWhenJWTMissing' -count=1
```

Result: passed after final verification.

Additional ANP SDK note: the code-writing Native Agent also attempted
`cargo +1.79.0 test --manifest-path anp/rust/Cargo.toml --test key_pem_tests`
with default features. That broader SDK command was blocked before compilation
by a registry mirror TLS download failure for `zerovec-derive 0.10.3`; the
required no-default-features key PEM test above passed locally and keeps the CLI
dependency lane on the local ANP SDK without default/network features.

Scope:

- Added a separate ANP Rust compatibility parser for private-key PEM inputs
  used by Go `identity/key_compat.go`; the normal runtime
  `PrivateKeyMaterial::from_pem` parser remains PKCS#8-only and continues to
  reject legacy ANP labels.
- Supported standard `PRIVATE KEY`, SEC1 `EC PRIVATE KEY`, and legacy ANP
  private labels for Ed25519, X25519, secp256r1, and secp256k1, preserving Go
  scalar/length validation and rewriting migrated material to standard
  `-----BEGIN PRIVATE KEY-----` PKCS#8 PEM.
- Added `identity/key_compat.rs` and wired `Manager::load` to normalize key-1,
  E2EE signing, and E2EE agreement private-key files before reading stored
  identity values, matching Go's missing-file no-op and auth-required error
  boundary for empty/invalid/unsupported PEM files.
- Added focused CLI contract coverage for three-file legacy ANP migration,
  secp256k1 key-1 migration, and unsupported-label error shape.

Boundary note: this is a compatibility migration path, not a broader ANP
registry expansion. Missing Go convenience APIs such as `KeyType`,
`GenerateKeyPairPEM`, free PEM decode functions, file-backed E2EE stores, and
high-level message-service E2EE clients remain separate parity lanes.

Dependency note: no dependency was added. Cargo manifests and lockfiles remain
unchanged. This slice reuses existing pure Rust key/PKCS#8/base64 crates in the
local ANP SDK plus the existing identity secure-text writer. It does not add
OpenSSL, `native-tls`, bundled OpenSSL, HTTP/TLS crates, YAML crates, platform
libraries, or new SQLite dependencies.

## 2026-05-16 Message Secure E2EE Client Adapter Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSecureInitCreatesPendingSession' -count=1
cd ../anp/golang && go test ./direct_e2ee -run 'TestClientSendAndPendingHistoryProcessing|TestClientFallsBackToSignedPrekeyWhenOPKUnavailable' -count=1
```

Result: passed after final verification. Focused Rust test counts:
`message_secure_client_contract` 9, `anpsdk_contract` 14,
`message_secure_send_contract` 3, and `message_secure_outbox_flush_contract`
23 all passed with zero failures. `cargo check`, structure check, whitespace
check, dependency audit, and focused Go reference tests passed. The first ANP
Go command attempted with stale test names returned `[no tests to run]`; the
recorded command above was then rerun with the actual current test names and
passed.

Scope:

- Extended `message::secure_client` from the preparation-only boundary to a
  narrow high-level direct-E2EE client adapter mirroring the Go ANP SDK v0.8.7
  `MessageServiceDirectE2eeClient` send/publish behavior.
- Preserved Go `NewSecureE2EEClientForRecord` setup: required manager/record
  errors, identity path lookup, DID signing and E2EE agreement key parsing
  prefixes, P5 store roots, key IDs, local DID document resolver precedence,
  local-manager fallback, remote DID resolver fallback, and local
  `ANPMessageService.serviceDid` constructor errors.
- Added injected-RPC methods for `publish_prekey_bundle`, `send_text`, and
  `send_json`, preserving RPC method names, `meta` profiles/security profiles,
  service vs agent target shapes, `operation_id == message_id` validation,
  OPK retry markers, prekey bundle verification, init `direct.send` body,
  saved pending-confirmation sessions, and pending follow-up error handling.
- Preserved Go's explicit publish double-call behavior: `EnsureFreshPrekeyBundle`
  opportunistically publishes and `PublishPrekeyBundle` publishes again.
- Added focused contract tests for constructor service-DID failures, prekey
  generation/publication, no-session text init, JSON init with OPK fallback,
  mismatched operation/message IDs, and pending-confirmation follow-up.

Boundary note: production `msg send --secure on`, real authsdk/HTTP/WebSocket
transport wiring, `ProcessIncoming`, incoming decrypt/ack, `SecureInit`,
`SecureRepair`, runtime listener integration, and awiki-system-test
secure-direct acceptance remain deferred parity slices. This is an adapter and
contract boundary, not yet a production CLI path.

Dependency note: no dependency was added. A direct `x25519-dalek` dependency was
considered and removed; the adapter reuses the local ANP Rust SDK key material
and direct-E2EE primitives plus existing workspace `base64`/`rand` dependencies.
It does not enable ANP SDK default/network features and does not add HTTP/TLS,
OpenSSL, `native-tls`, bundled OpenSSL, WebSocket crates, YAML crates, platform
libraries, or a new SQLite dependency. TLS remains Rustls-first for later
production transport wiring.

## 2026-05-16 Message Secure E2EE Incoming Client Adapter Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_normalize_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_sync_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_ack_in_process_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../anp/golang && go test ./direct_e2ee -run 'TestClientSendAndPendingHistoryProcessing|TestJSONRoundTripForPendingResult|TestSessionInitAndFollowUpRoundTrip|TestSkippedMessageKeySurvivesFailedAuthentication|TestClientFallsBackToSignedPrekeyWhenOPKUnavailable|TestDirectInitBodyOmitsAbsentOPKAndLegacyStaticField|TestSharedP5Vectors|TestSharedP5VectorAADDoesNotContainApplicationContentType' -count=1
```

Result: passed. Focused Rust test counts: `message_secure_client_contract`
14, `anpsdk_contract` 14, `runtime_listener_secure_normalize_contract` 10,
`runtime_listener_secure_sync_contract` 7, and
`runtime_listener_secure_ack_in_process_contract` 13 all passed with zero
failures. `cargo check`, structure check, whitespace check, dependency audit,
and focused Go ANP SDK reference tests passed.

Scope:

- Extended the injected high-level `MessageServiceE2EEClient` adapter with
  `process_incoming` and `decrypt_history_page`, mirroring Go ANP SDK v0.8.7
  `MessageServiceDirectE2eeClient` inbound behavior.
- Preserved direct init decrypt: Go-style map parsing, sender DID document
  resolution, sender static X25519 extraction, signed-prekey private load,
  optional OPK private load, `AcceptIncomingInitWithOPK`, OPK deletion after
  successful decrypt, responder session save, and `{state:"decrypted",
  plaintext:{...}}` result shape.
- Preserved direct cipher decrypt: any session load error queues the original
  message in in-memory `pending_by_peer[sender_did]` and returns
  `{state:"pending"}`; decrypt failures save the possibly mutated session and
  return `{state:"undecryptable"}`; decrypt successes save the session and
  return decrypted plaintext.
- Preserved pending replay semantics: queues are keyed by sender DID, replay in
  insertion order after a successful init from that sender, include only
  recursive non-error results, and delete the sender queue after replay.
- Preserved unsupported content-type error text:
  `unsupported content type: <contentType>`.
- Preserved Go v0.8.7 `DecryptHistoryPage` behavior as implemented in the
  SDK: copy, stable-sort by numeric `server_seq` ascending, tie-break by
  `meta.message_id` lexicographically ascending, process in that sorted order,
  and return results in sorted processing order. There is no final reverse in
  the Go v0.8.7 implementation.

Boundary note: this slice is still an adapter/contract boundary. Production
`msg send --secure on`, inbox/history secure decrypt application, runtime
listener real `ProcessIncoming` wiring, network/local secure ACK delivery,
`SecureInit`, `SecureRepair`, and awiki-system-test secure-direct acceptance
remain deferred parity slices.

Dependency note: no dependency was added. The slice reuses local `../anp/rust`
direct-E2EE session primitives and existing file-store facades. It does not add
or enable HTTP/TLS, WebSocket, OpenSSL, `native-tls`, bundled OpenSSL, YAML,
platform service libraries, or a new SQLite path. TLS remains Rustls-first for
later production transport wiring.

## 2026-05-16 Message Secure Incoming Application Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_incoming_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestPollingInboxDecryptsDirectInitAndSendsSecureAck|TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation' -count=1
```

Result: passed after final verification. Focused Rust test counts:
`message_secure_incoming_contract` 8, `msg_live_contract` 4,
`message_secure_client_contract` 14, and `anpsdk_contract` 14 all passed with
zero failures. `cargo check`, structure check, whitespace check, dependency
audit, and focused Go reference tests passed.

Scope:

- Adds `message::secure_incoming` as the direct translation of the application
  helper subset of Go `internal/message/secure_incoming.go`.
- Preserves exact secure wire content-type detection:
  `application/anp-direct-init+json` and
  `application/anp-direct-cipher+json`.
- Preserves Go notification conversion from message views:
  `meta.sender_did`, agent target DID, `message_id`,
  `profile=anp.direct.e2ee.v1`, `security_profile=direct-e2ee`,
  `content_type`, `body`, and optional numeric-only `server_seq`.
- Preserves Go's numeric-only `server_seq` coercion. String values such as
  `"1"` are treated like zero/absent for notification shape and processing
  order.
- Preserves secure processing order in inbox/history pages: stable sort by
  numeric `server_seq` ascending, zero last, and top-level message `id`
  lexicographic tie-break.
- Preserves warning behavior:
  `Skipped secure direct message <id>: <err>` for malformed user messages,
  `Failed to decrypt secure direct message <id>: <err>` for processor errors,
  compact duplicate warnings, and suppress warnings for secure wire control
  messages.
- Preserves decrypted application rewrites: text becomes `type=text`, JSON
  payload becomes `type=json`, attachment manifest payload becomes
  `type=attachment_manifest`, binary payload becomes `type=binary`, and secure
  ack/init plaintext becomes hidden `secure_control`.
- Preserves display filtering: secure controls and secure wire messages with
  empty, `undecryptable`, or `failed` decryption state are hidden; Go's current
  `pending` state remains displayable.
- Wires `persist_inbox_messages` and `persist_history_messages` to process
  direct E2EE wire messages after local store/schema open, persist post-decrypt
  records while skipping secure control records, then return only displayable
  rows.
- Filters secure wire/control rows from handle-history cache merges before
  merging local cached rows with remote inbox/history results.

Boundary note: the initial application slice did not port the Go side effects in
`maybeFlushPollingSecureAck` or `maybeAckPollingDirectInit`; the follow-up below
adds them on the same dependency stack. `SecureInit`, `SecureRepair`, runtime
listener real `ProcessIncoming` wiring, WebSocket/local bridge execution,
production `msg send --secure on`, and awiki-system-test secure-direct
acceptance remain deferred parity slices.

Parallelism note: a read-only Native Agent reviewed Go/Rust parity for this
slice and found two concrete risks: handle-history cached secure wire rows
could leak through merge results, and string `server_seq` was parsed more
permissively than Go. Both were fixed and locked with focused Rust tests. No
code-writing Native Agent changed files for this slice.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing authsdk/message client infrastructure,
the local ANP Rust E2EE adapter, `serde_json`, store helpers, and the approved
`rusqlite + bundled` SQLite path. It does not enable ANP SDK
`network`/default features and does not add OpenSSL, `native-tls`, bundled
OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates,
platform service libraries, new E2EE provider dependencies, or a new SQLite
backend. TLS remains Rustls-first for later runtime/WebSocket transport work.

## 2026-05-16 Message Secure Incoming Polling ACK/Flush Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_incoming_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestPollingInboxDecryptsDirectInitAndSendsSecureAck|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1
```

Result: passed after final verification. Focused Rust test counts:
`message_secure_incoming_contract` 12, `message_secure_client_contract` 14,
`message_secure_outbox_flush_contract` 23, `msg_live_contract` 4, and
`anpsdk_contract` 14 all passed with zero failures. `cargo check`, structure
check, whitespace check, dependency audit, and focused Go reference tests passed.

Scope:

- Adds the Go polling side effects to `message::secure_incoming` without
  changing the public inbox/history contract.
- Preserves Go's side-effect order inside `maybeDecryptDirectE2EEMessages`:
  process incoming, run polling ACK/flush side effects, then apply the decrypted
  result to the message row.
- Preserves `maybeFlushPollingSecureAck`: only decrypted secure ACK plaintext
  flushes queued secure outbox rows, only the original `sender_did` is used as
  the peer DID, and blank/self peers are ignored.
- Preserves `maybeAckPollingDirectInit`: only decrypted
  `application/anp-direct-init+json` messages create `ack-<session_id>`, ACK is
  sent with `BuildSecureAckPayload(session_id, message_id)` via `SendJSON`, ACK
  send failure returns `Failed to send secure direct ACK for <message_id>: <err>`
  and does not flush queued rows, and flush runs only after ACK send success.
- Preserves `directInitSessionIDFromMessage`: reads object content or non-empty
  JSON object strings through the same map-like boundary, returns only string
  `session_id`, and does not trim the session ID.
- Production flush side effects reuse the existing secure outbox helper and the
  same high-level `MessageServiceE2EEClient` used for direct E2EE sends.

Boundary note: this slice still excludes `SecureInit`, `SecureRepair`, runtime
listener real `ProcessIncoming` wiring, WebSocket/local bridge execution,
production `msg send --secure on`, and awiki-system-test secure-direct
acceptance.

Parallelism note: a read-only Native Agent mapped Go/Rust side-effect parity for
this slice. No code-writing Native Agent changed files.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std authsdk/message client,
local ANP Rust E2EE adapter, secure outbox flush helper, store helpers, and the
approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, new E2EE provider
dependencies, or a new SQLite backend. TLS remains Rustls-first for later
runtime/WebSocket transport work.

## 2026-05-16 Message Secure Direct Production Send Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_send_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestServiceSendSecureDirectUsesP5KeyServiceTargetAndPersistsPendingSession|TestServiceSendSecureDirectQueuesFollowUpWhilePendingConfirmation' -count=1
```

Result: passed.

Observed results:

- `message_secure_send_contract`: 4 passed.
- `msg_contract`: 5 passed.
- `msg_live_contract`: 5 passed.
- `message_secure_client_contract`: 14 passed.
- `message_secure_outbox_flush_contract`: 23 passed.
- `cargo check`, structure check, and whitespace check passed.
- Dependency audit showed only existing allowed crypto/TLS/SQLite build
  dependencies: `rustls`, `webpki`, `ring`, `base64`, `sha2`, approved
  `rusqlite`/`libsqlite3-sys`, and build helpers `cc`, `pkg-config`, `vcpkg`;
  no OpenSSL, `native-tls`, `reqwest`, `hyper`, WebSocket, YAML, or platform
  service dependency was introduced.
- Go reference tests in `./internal/message` passed.

Scope:

- Wires `msg send --secure on` through the production direct send path instead
  of returning `SecureNotSupported`.
- Preserves Go's string-valued `--secure on` and `--secure=on` CLI parsing and
  passes `SecureMode` into `SendRequest`.
- Preserves Go's secure send ordering: entry-level target/text validation before
  `SecureMode == "on"` routing, active identity/key-material gate inside the
  secure path, target resolution, auth/RPC transport initialization,
  best-effort prekey publish warning collection, E2EE client construction,
  `SendText`, then success persistence or pending-confirmation queue.
- Reuses the verified `MessageServiceE2EEClient` adapter and existing Rustls/std
  authenticated `/im/rpc` client for production secure RPCs.
- Adds a local fake-server live smoke that proves CLI production secure send
  emits `direct.e2ee.publish_prekey_bundle`, `direct.e2ee.get_prekey_bundle`,
  and `direct.send` with `application/anp-direct-init+json`, returns
  `message.secure=true`, and persists `messages.is_e2ee=1`.

Boundary note: this slice still excludes production `msg secure retry` sender
wiring, `SecureInit`, `SecureRepair`, WebSocket/local bridge secure execution,
runtime listener live `ProcessIncoming`, and awiki-system-test secure-direct
acceptance.

Parallelism note: two read-only Native Agents mapped Go behavior and Rust gaps
for production secure send. No code-writing Native Agent changed files.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std authsdk/message client,
local ANP Rust E2EE adapter, secure outbox/store helpers, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, new E2EE provider dependencies, or a new
SQLite backend. TLS remains Rustls-first for later runtime/WebSocket transport
work.

## 2026-05-16 Group E2EE Status/Pending Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cd ../awiki-cli && go test ./internal/message -run 'TestInspectGroupE2EEStatusComparesLocalEpochToServiceHead|TestGroupE2EEStatusForRecoveryScansNonDefaultDevice' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, and whitespace check passed.
- Go focused status and dry-run references passed.
- Modified source/test files remain below the default 1200-line review-size
  cap: `group_e2ee_status.rs` 873 lines,
  `group_e2ee_handlers.rs` 406 lines,
  `group_e2ee_status_contract.rs` 507 lines, and
  `group_e2ee_pending_contract.rs` 404 lines.

Scope:

- Replaces the non-dry-run `group e2ee status` `not_implemented` boundary with
  Go-shaped status inspection.
- Preserves dry-run plan shape for all `group e2ee ...` commands and keeps
  sibling non-dry-run commands deferred.
- Implements the Go status provider subset: active identity gate, local
  `anp-mls group status --json-in -` execution, `AWIKI_ANP_MLS_BINARY`/PATH
  binary resolution, scoped MLS data dir
  `<workspace>/mls/agents/<mlsAgentKey(agentDID)>/<device>`, device candidate
  scan, status rank/epoch tie-break, default empty status, `device_id` mutation,
  and 15-second provider timeout.
- Reuses the existing Rustls/std message HTTP client for hidden
  `group.e2ee.head` and `group.e2ee.notice`, including auth session/JWT
  behavior through the shared client and the existing E2EE wire builders.
- Preserves Go result shape and strings: summary
  `Group E2EE recovery status inspected`, `available`, `mls`, `local`,
  `local_device_id`, `service_head`, `pending_notices`,
  `pending_notice_count`, `diagnosis`, `recovery_artifact`, `plan`, and the
  local/service warning prefixes.
- Adds fake-server/fake-MLS CLI coverage for the service-head vs local-epoch
  pending-notice diagnosis and non-default device scan.
- Replaces the non-dry-run `group e2ee pending` `not_implemented` boundary
  with Go-shaped pending notice pulling.
- Preserves the pending CLI plan shape and Go live result shape: active
  identity gate, hidden `group.e2ee.notice` RPC through the existing
  authsdk/session Rustls/std message client, limit `50`,
  `mark_delivered=false`, no `notice_ids`, optional trimmed group filter,
  `notices`, raw `pending_count`, `group`, `plan`, and summary
  `Pulled group E2EE pending notices`.
- Adds fake-server CLI coverage for a group-filtered pending pull and for the
  blank-group behavior where the RPC body omits `group_did`.

Boundary note: this slice does not implement non-dry-run
`publish-key-package`, `repair`, `recover-member`, `update-key`, `rejoin`,
commit/welcome replay, service-head mutation, MLS cache mutation,
WebSocket/local bridge group E2EE transport, or awiki-system-test group-E2EE
acceptance.

Parallelism note: read-only Native Agents mapped Go/Rust group E2EE status
surfaces for the previous status slice, and one GPT-5.5 xhigh code-writing
Native Agent added the isolated status test file. The pending slice was small
enough to finish locally; an attempted read-only Native Agent check was blocked
by the current session's agent limit, so the leader performed the parity check
and final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The slice uses only `std::process::Command`, existing `serde_json`,
existing `sha2`/`base64`, existing authsdk/session, existing Rustls/std
transport, existing group E2EE wire builders, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, MLS provider crates, or a new SQLite
backend. TLS remains Rustls-first.

## 2026-05-16 Message Secure Init Production Sender Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestServiceSecureInitCreatesPendingSession' -count=1
wc -l crates/awiki-cli/src/message/secure_commands.rs crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/app/msg_handlers.rs crates/awiki-cli/tests/message_secure_commands_contract.rs crates/awiki-cli/tests/msg_live_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `message_secure_commands_contract`: 11 passed.
- `msg_live_contract`: 7 passed.
- `msg_contract`: 5 passed.
- `message_secure_outbox_flush_contract`: 23 passed.
- `message_secure_client_contract`: 14 passed.
- `cargo check`, structure check, and whitespace check passed.
- Go reference `TestServiceSecureInitCreatesPendingSession` passed.
- Modified source/test files remain below the 1200-line review-size cap:
  `secure_commands.rs` 595 lines, `service.rs` 1125 lines,
  `msg_handlers.rs` 822 lines, `message_secure_commands_contract.rs` 1150
  lines, and `msg_live_contract.rs` 1191 lines.

Scope:

- Wires non-dry-run `msg secure init --with <peer>` through production secure
  init instead of returning `not_implemented`.
- Preserves Go `SecureInit` ordering: active identity gate, key-material
  requirement, target required/resolve, best-effort prekey publish warnings,
  existing session lookup, early reused-session result, authenticated RPC/E2EE
  client initialization, manual secure-init `SendJSON`, and post-send session
  reload.
- Preserves Go result shapes: redacted `target`, `session`, `reused=true` for
  existing sessions, `initialized=true`, `delivery.message_id`,
  `delivery.operation_id`, and `delivery.target_did` with fallback defaults for
  new init sends.
- Preserves Go CLI hint:
  `Make sure the target exists and the active identity has secure E2EE key material.`
- Adds service-level coverage for existing-session reuse, session redaction,
  prekey publisher warning preservation, and missing secure key material.
- Adds a local fake-server live smoke proving CLI production init emits
  `direct.e2ee.publish_prekey_bundle`, `direct.e2ee.get_prekey_bundle`, and
  `direct.send` with `application/anp-direct-init+json`, uses a generated
  `secure-init-` operation/message ID, and creates a pending session file.

Boundary note: this slice still excludes `SecureRepair`, WebSocket/local bridge
secure execution, runtime listener live `ProcessIncoming`, and
awiki-system-test secure-direct acceptance.

Parallelism note: two read-only Native Agents mapped Go behavior and Rust gaps
for production secure init. A follow-up verifier agent could not be spawned
because the Native Agent thread limit was reached. No code-writing Native Agent
changed files; the standing rule remains that any code-writing Native Agent
must use GPT-5.5 xhigh with a bounded, non-overlapping write scope.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std authsdk/message client,
local ANP Rust E2EE adapter, secure session/status helpers, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, new E2EE provider dependencies, or a new
SQLite backend. TLS remains Rustls-first for later runtime/WebSocket transport
work.

## 2026-05-16 Message Secure Repair Production Sender Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test msg_secure_repair_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestServiceSecureRepairResetsFailedOutboxAndStartsNewInit' -count=1
wc -l crates/awiki-cli/src/message/secure_commands.rs crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/app/msg_handlers.rs crates/awiki-cli/tests/msg_secure_repair_live_contract.rs crates/awiki-cli/tests/msg_live_contract.rs crates/awiki-cli/tests/message_secure_commands_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `msg_secure_repair_live_contract`: 1 passed.
- `message_secure_commands_contract`: 11 passed.
- `msg_contract`: 5 passed.
- `message_secure_outbox_flush_contract`: 23 passed.
- `message_secure_client_contract`: 14 passed.
- `cargo check`, structure check, and whitespace check passed.
- Go reference `TestServiceSecureRepairResetsFailedOutboxAndStartsNewInit`
  passed.
- Modified source/test files remain below the 1200-line review-size cap:
  `secure_commands.rs` 688 lines, `service.rs` 1125 lines,
  `msg_handlers.rs` 815 lines, `msg_secure_repair_live_contract.rs` 552
  lines, `msg_live_contract.rs` 1191 lines, and
  `message_secure_commands_contract.rs` 1150 lines.

Scope:

- Wires non-dry-run `msg secure repair --with <peer>` through production secure
  repair instead of returning `not_implemented`.
- Preserves Go `SecureRepair` ordering: shared secure peer preparation, local
  peer-state reset, then a fresh `SecureInit` call that performs Go-equivalent
  second preparation and prekey publishing.
- Preserves local reset side effects: deletes one matching `p5-e2ee-sessions`
  session when present, lists failed E2EE outbox rows by active identity, resets
  only same-peer failed rows to `queued`, and counts each deleted session or
  requeued row.
- Preserves Go result shapes: original init result plus `data.repair.peer_did`,
  `data.repair.peer_handle`, `data.repair.reset_records`, summary
  `Repaired secure session with <peer>`, compacted warnings with repair prepare
  warnings before init warnings, and the Go CLI repair hint.
- Adds a focused fake-server live smoke proving old session removal, same-peer
  failed outbox requeue, non-peer failed outbox preservation, direct-init
  `direct.send` with `application/anp-direct-init+json`, generated
  `secure-init-` operation/message ID, and new pending initiator session
  creation.

Boundary note: this slice still excludes WebSocket/local bridge secure
execution, runtime listener live `ProcessIncoming`, and awiki-system-test
secure-direct acceptance.

Parallelism note: a GPT-5.5 xhigh code-writing Native Agent created only the
new focused repair live test file inside its bounded write scope. A read-only
verifier agent compared the Go and Rust `SecureRepair` paths and found no
observable repair parity gap. The verifier also noted an older generic
`message_exit` error-classification difference for `SecureNotSupported` and
`TransportUnavailable`, but that mapper branch is not reached by the current
repair path and remains a separate follow-up parity item.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std authsdk/message client,
local ANP Rust E2EE adapter, `FileSessionStore`, secure session/status helpers,
and the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, new E2EE provider
dependencies, or a new SQLite backend. TLS remains Rustls-first for later
runtime/WebSocket transport work.

## 2026-05-16 Message CLI Exit Mapper Unsupported/Transport Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli app::msg_handlers::tests::message_exit_maps_transport_unavailable_like_go --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cd ../awiki-cli && go test ./internal/cli -run 'TestMsg' -count=1
```

Result: passed for the commands listed above.

Observed results:

- `msg_contract`: 6 passed.
- `group_contract`: 6 passed.
- Internal mapper filter:
  `app::msg_handlers::tests::message_exit_maps_transport_unavailable_like_go`
  passed.
- Go reference `./internal/cli -run TestMsg` passed.
- `cargo fmt --check`, structure check, and whitespace check passed.

Scope:

- Aligns Rust message/group CLI error mapping with Go `messageExit` for
  `ErrSecureNotSupported`, `ErrGroupE2EESelfLeaveUnsupported`, and
  `ErrTransportUnavailable`.
- Preserves Go exit codes and hints:
  `unsupported_mode` for unsupported secure messaging with
  `Secure messaging is currently supported only for direct text messaging.`,
  `unsupported_mode` for PR-A group E2EE self-leave with the owner-removal
  hint, and `transport_unavailable` for websocket transport errors with the
  listener/runtime hint.
- Adds a CLI contract for unsupported secure attachment send and group E2EE
  self-leave, plus an internal mapper unit test for the transport-unavailable
  branch because Rust currently exposes the websocket proxy helper but does not
  route production CLI message commands through it.

Boundary note: this slice only changes error envelope mapping. It does not wire
WebSocket/local bridge message execution, runtime listener live
`ProcessIncoming`, awiki-system-test secure-direct acceptance, or new command
execution paths.

Parallelism note: a read-only GPT-5.5 xhigh Native Agent compared Go and Rust
mapper branches. It confirmed parity for the three Go sentinels changed here
and confirmed `AttachmentNotSupported`/`GroupNotSupported` have no Go sentinel,
so they keep the existing Rust `not_implemented` mapping.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice does not add OpenSSL, `native-tls`, bundled OpenSSL,
`reqwest`, `hyper`, WebSocket crates, async runtimes, YAML crates, platform
service libraries, new E2EE provider dependencies, or a new SQLite backend.

## 2026-05-16 Message Secure Retry Production Sender Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked
cargo +1.79.0 check -p awiki-cli --locked
go test ./internal/message -run 'TestServiceSecureRetryMarksQueuedRecordSent|TestFlushQueuedSecureOutboxSendsCipherAfterConfirmation' -count=1
wc -l crates/awiki-cli/src/message/secure_commands.rs crates/awiki-cli/src/message/service.rs crates/awiki-cli/src/app/msg_handlers.rs crates/awiki-cli/tests/message_secure_commands_contract.rs crates/awiki-cli/tests/msg_live_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `message_secure_commands_contract`: 9 passed.
- `msg_live_contract`: 6 passed.
- `msg_contract`: 5 passed.
- `message_secure_outbox_flush_contract`: 23 passed.
- `message_secure_client_contract`: 14 passed.
- `cargo check` and whitespace check passed.
- Go reference tests in `./internal/message` passed.
- Modified source/test files remain below the 1200-line review-size cap:
  `secure_commands.rs` 461 lines, `service.rs` 1125 lines,
  `msg_handlers.rs` 796 lines, `message_secure_commands_contract.rs` 1032
  lines, and `msg_live_contract.rs` 1091 lines.

Scope:

- Wires non-dry-run `msg secure retry <OUTBOX_ID>` through the production retry
  sender instead of returning `not_implemented`.
- Preserves Go `SecureRetry` ordering: active identity gate, store open/schema,
  selected outbox row lookup, selected row status reset to `queued`, peer filter
  from the selected row, secure outbox sender initialization, queued peer-row
  flush, selected row reload, and Go summary/data shape.
- Preserves the Go initialization-failure boundary: sender initialization
  failure is returned as a warning after the selected row has been reset to
  `queued`; it does not mark the row as `send_failed`.
- Reuses the existing verified `MessageServiceE2EEClient` adapter and existing
  Rustls/std authenticated `/im/rpc` client for retry sends.
- Adds a local fake-server live smoke proving CLI production retry emits
  `direct.send` with `application/anp-direct-cipher+json`, uses the outbox ID as
  operation/message ID, marks the outbox row sent, records the current secure
  session ID, and persists a local E2EE outbound message.

Boundary note: this slice still excludes `SecureInit`, `SecureRepair`,
WebSocket/local bridge secure retry execution, runtime listener live
`ProcessIncoming`, and awiki-system-test secure-direct acceptance.

Parallelism note: two read-only Native Agents mapped Go behavior and Rust gaps
for production secure retry. No code-writing Native Agent changed files.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses the existing Rustls/std authsdk/message client,
local ANP Rust E2EE adapter, secure outbox/store helpers, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, new E2EE provider dependencies, or a new
SQLite backend. TLS remains Rustls-first for later runtime/WebSocket transport
work.

## 2026-05-16 Group E2EE KeyPackage Publish Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
cd ../awiki-cli && go test ./internal/message -run 'TestBuildGroupE2EEPublishKeyPackageRPCParamsStripsProviderOnlyFields|TestInspectGroupE2EEStatusComparesLocalEpochToServiceHead|TestGroupE2EEStatusForRecoveryScansNonDefaultDevice' -count=1
wc -l crates/awiki-cli/src/message/group_e2ee_status.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/message/group_e2ee_publish.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_publish_contract.rs docs/parity-matrix.md docs/dependency-decisions.md docs/verification/README.md
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_publish_contract`: 4 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_e2ee_status.rs` 541 lines, `group_e2ee_provider.rs` 352 lines,
  `group_e2ee_publish.rs` 293 lines, `group_e2ee_handlers.rs` 439 lines, and
  `group_e2ee_publish_contract.rs` 676 lines. No file-size exception is
  needed.

Scope:

- Replaces the non-dry-run `group e2ee publish-key-package` `not_implemented`
  boundary with Go-shaped KeyPackage publish execution.
- Extracts the reusable external `anp-mls` exec provider into
  `message/group_e2ee_provider.rs` so status and publish share binary lookup,
  data-dir scoping, timeout, JSON response handling, and error mapping.
- Preserves Go publish validation and provider request behavior: active
  identity gate, blank device defaulting to `default`, purpose normalization
  for `normal|recovery|update`, group requirement for `recovery`/`update`,
  top-level `contract_test_enabled` only when true, provider params
  `agent_did`, `device_id`, `owner_did`, and non-normal `purpose`/`group_did`.
- Preserves Go post-processing before service publish: tags non-normal
  KeyPackages with `purpose`, `group_did`, and defaulted `device_id`; verifies
  owner/binding DID match the active identity; requires
  `did_wba_binding.leaf_signature_key_b64u`, `issued_at`, and `expires_at`;
  signs the binding locally with the active DID document verification method
  and key-1 private key.
- Preserves service publish through the existing Rustls/std authenticated
  message client and existing wire builder: service DID is selected from
  resolved config when present with a capabilities fallback for empty configs,
  `group.e2ee.publish_key_package` is signed with transport-protected metadata,
  and private provider-only KeyPackage fields are stripped by the sanitizer.
- Preserves Go CLI output shape and strings: summary
  `Published group E2EE KeyPackage`, `data.mls`, `data.published`,
  `recovery`, `purpose`, `group`, `device_id`, `argv_safe`, `p4_mutates`,
  inserted `plan`, and the Go publish error hint.
- Adds fake-MLS/fake-server live CLI coverage for normal publish, recovery
  publish with `--contract-test`, missing recovery group, invalid purpose, and
  no leakage of `private_key_package_b64u` into the service publish body.

Boundary note: this slice still excludes `repair`, `recover-member`,
`update-key`, `rejoin`, `group add --e2ee`, commit/welcome replay,
service-head mutation, MLS cache mutation, WebSocket/local bridge group E2EE
transport, and awiki-system-test group-E2EE acceptance.

Parallelism note: two read-only Native Agents mapped Go publish behavior and
Rust/ANP reusable capabilities. One GPT-5.5 xhigh code-writing Native Agent
created the focused publish test file under a bounded, non-overlapping test
write scope; the leader implemented production code, corrected the tests to
Go/Rust config and ANP binding details, integrated documentation, and ran final
verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
`sha2`/`base64`, local ANP Rust proof/key APIs, existing authsdk/session,
existing Rustls/std message HTTP transport, existing group E2EE wire builders,
and the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, or a
new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Create Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
cd ../awiki-cli && go test ./internal/message -run 'TestBuildGroupE2EECreateRPCParams|TestBuildGroupE2EEPublishKeyPackageRPCParamsStripsProviderOnlyFields|TestInspectGroupE2EEStatusComparesLocalEpochToServiceHead|TestGroupE2EEStatusForRecoveryScansNonDefaultDevice' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_create.rs crates/awiki-cli/src/message/group_e2ee_create.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/message/group_e2ee_publish.rs crates/awiki-cli/tests/group_e2ee_create_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_create_contract`: 2 passed.
- `group_live_contract`: 3 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1157 lines, `group_create.rs` 66 lines,
  `group_e2ee_create.rs` 305 lines, `group_e2ee_transport.rs` 73 lines,
  `group_e2ee_provider.rs` 362 lines, `group_e2ee_publish.rs` 251 lines, and
  `group_e2ee_create_contract.rs` 674 lines. No file-size exception is needed.

Scope:

- Wires live `group create --e2ee` through the Go-shaped sequence: normal
  service `group.create`, group snapshot/member sync, external MLS
  `anp-mls group create`, hidden service `group.e2ee.create`, and local E2EE
  summary persistence.
- Splits `message/group_create.rs` out of `group_service.rs` to keep the large
  group service file under the default line-count cap while preserving the
  existing public `message::create_group` entry point.
- Extends the shared external MLS provider with `create_group` while preserving
  the previous `status` and `key-package generate` behavior.
- Adds a small shared `group_e2ee_transport` helper so publish and create share
  configured-service-DID-first behavior plus capabilities fallback without
  introducing a broad transport abstraction.
- Preserves Go warning downgrade behavior: MLS provider failure returns a
  successful created group with a warning and without `data.e2ee`; transport or
  delivery failure keeps `data.e2ee.mls` and records a create-delivery warning.
- Persists the Go-shaped group E2EE summary metadata into the existing group
  cache, including `message_security_profile`, `group_e2ee`, and
  `group_state_version` when available.
- Adds fake-MLS/fake-server live CLI coverage for successful create bootstrap
  and provider-failure warning parity.

Boundary note: this slice still excludes `group add/remove/leave --e2ee`,
`group e2ee rejoin`, `recover-member`, `update-key`, repair, commit/welcome
replay, group E2EE send/decrypt, WebSocket/local bridge group E2EE transport,
and full awiki-system-test group-E2EE acceptance.

Parallelism note: two read-only Native Agents confirmed `group create --e2ee`
as the smallest next group-E2EE live slice and ranked the remaining commands.
One GPT-5.5 xhigh code-writing Native Agent created the isolated create test
file under a bounded, non-overlapping test write scope; the leader implemented
production code, integrated documentation, and ran final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, and the approved `rusqlite + bundled` SQLite path.
It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
WebSocket crates, async runtimes, YAML crates, platform service libraries, MLS
provider crates, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Add/Rejoin Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestAddGroupMemberE2EEUsesOnlyServiceLeasedKeyPackageForMLS|TestLocalIdentityByDIDFindsStoredMemberForWelcomeProcessing|TestGroupE2EEWelcomeDeviceIDUsesPublicKeyPackageDevice' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_add.rs crates/awiki-cli/src/message/group_e2ee_create.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_add_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_add_contract`: 6 passed.
- `group_e2ee_create_contract`: 2 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_live_contract`: 3 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1176 lines, `group_e2ee_add.rs` 299 lines,
  `group_e2ee_create.rs` 305 lines, `group_e2ee_provider.rs` 382 lines,
  `group_e2ee_transport.rs` 101 lines, `group_e2ee_handlers.rs` 470 lines, and
  `group_e2ee_add_contract.rs` 1072 lines. No file-size exception is needed.

Scope:

- Wires live `group add --e2ee` through the Go-shaped sequence: normal P4
  `group.add`, group snapshot/member sync, hidden
  `group.e2ee.get_key_package`, external MLS `anp-mls group add-member`,
  hidden `group.e2ee.add`, local summary persistence, and optional local
  welcome processing when the added member is a stored local identity.
- Preserves Go automatic E2EE detection for `group add` without explicit
  `--e2ee`: request flag, pre-mutation snapshot, and post-mutation snapshot can
  trigger the E2EE add path. The detector now accepts both JSON-string and
  object-valued `metadata`, matching Go `decodeMetadataMap`.
- Preserves Go warning downgrade behavior: KeyPackage lookup failure leaves P4
  add successful without `data.e2ee`; MLS provider failure returns redacted
  `leased_key_package`; hidden delivery failure keeps `data.e2ee.mls` plus the
  redacted package; all paths append the matching warning prefix.
- Preserves service-leased KeyPackage usage: MLS add-member receives only the
  service-returned public `group_key_package`, `key_package_id`, full
  `target_key_package`, and cached `group_state_ref`; output redacts private
  material.
- Wires live hidden `group e2ee rejoin` as a wrapper over the same
  `message::add_group_member` path with `e2ee=true`, inserts the Go rejoin
  plan into live result data, and preserves the removed/left rejoin hint that
  directs users to fresh normal KeyPackage publication plus owner-only
  `group add --e2ee`.

Boundary note: this slice still excludes `group remove/leave --e2ee`,
`recover-member`, `update-key`, repair, commit replay beyond local welcome
processing, group E2EE send/decrypt, WebSocket/local bridge group E2EE
transport, and full awiki-system-test group-E2EE acceptance.

Parallelism note: one read-only Native Agent mapped Go add/rejoin behavior; one
GPT-5.5 xhigh code-writing Native Agent created the isolated add/rejoin test
file under a bounded, non-overlapping test write scope; one read-only verifier
reviewed the diff and found the metadata-object detection bug that was fixed
before final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, and
the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Recover-Member Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestRecoverGroupE2EEMemberUsesRecoverMemberWithoutGroupAdd|TestMLSExecProviderCommands|TestHTTPTransportGroupMethods' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_e2ee_recover.rs crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_remove.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_recover_member_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_recover_member_contract`: 1 passed.
- `group_e2ee_add_contract`: 6 passed.
- `group_e2ee_remove_leave_contract`: 3 passed.
- `group_e2ee_create_contract`: 2 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_e2ee_recover.rs` 175 lines, `group_service.rs` 1198 lines,
  `group_e2ee_remove.rs` 368 lines, `group_e2ee_transport.rs` 176 lines,
  `group_e2ee_provider.rs` 428 lines, `group_e2ee_handlers.rs` 523 lines, and
  `group_e2ee_recover_member_contract.rs` 725 lines. No file-size exception is
  needed.

Scope:

- Wires live hidden `group e2ee recover-member` through Go's active-member
  crypto recovery path: active identity, target DID resolution,
  `device_id=default` fallback, hidden `group.e2ee.head` recovery eligibility
  preflight, hidden `group.e2ee.get_key_package` with `purpose=recovery`,
  external MLS `anp-mls group recover-member-prepare`, hidden
  `group.e2ee.recover_member`, generic `group commit-finalize`, E2EE summary
  persistence, and optional local welcome processing for a local target member.
- Preserves Go no-P4-mutation behavior: the command never calls public
  `group.add`, the hidden recover body omits P4 `member_did` and `role`, and
  output includes `p4_membership_mutate=false`.
- Preserves Go result shape and summary:
  `Recovered active group E2EE member without P4 membership mutation`, with
  `group`, `member`, `target`, redacted `recovery_key_package`,
  `mls_prepare`, `mls_finalize`, `delivery`, `argv_sensitive_fields`, and the
  Go live plan.
- Reuses existing generic commit finalize/abort helpers from the remove/leave
  slice and existing local welcome/redacted KeyPackage helpers from the
  add/rejoin slice; no broad refactor or optimization is mixed into this
  translation slice.

Boundary note: this slice still excludes `update-key`, repair, commit replay
beyond finalize/abort and local welcome processing, group E2EE send/decrypt,
WebSocket/local bridge group E2EE transport, and full awiki-system-test
group-E2EE acceptance.

Parallelism note: a code-writing Native Agent was initially launched with an
intended bounded test-file scope but was stopped before editing because the
role-level reasoning configuration could not be proven xhigh. It then provided
only a read-only test outline. The leader implemented source and tests locally
and ran final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, and
the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Group E2EE Remove/Leave Live Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_create_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_publish_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_live_contract --locked
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/message -run 'TestLeaveGroupRejectsActiveOwnerFromCachedSnapshot|TestLeaveGroupE2EECreatesLeaveRequestWithoutLocalMLSLeave|TestUnsupportedGroupE2EESelfLeaveReasonDetectsNonAdvancingEpoch|TestGroupMemberMutationUsesPreMutationE2EESnapshot|TestShouldAbortGroupE2EEPendingCommitOnlyForDeterministicServiceRejection|TestPendingCommitTerminalParamsDoNotReusePrepareOperationID|TestBuildGroupE2EERemoveRPCParams|TestHTTPTransportGroupMethods|TestMLSExecProviderCommands' -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestGroupDryRunPlansRenderStableContracts' -count=1
wc -l crates/awiki-cli/src/message/group_service.rs crates/awiki-cli/src/message/group_e2ee_remove.rs crates/awiki-cli/src/message/group_e2ee_provider.rs crates/awiki-cli/src/message/group_e2ee_transport.rs crates/awiki-cli/src/app/group_e2ee_handlers.rs crates/awiki-cli/tests/group_e2ee_remove_leave_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `group_e2ee_remove_leave_contract`: 3 passed.
- `group_e2ee_add_contract`: 6 passed.
- `group_e2ee_create_contract`: 2 passed.
- `group_e2ee_publish_contract`: 4 passed.
- `group_e2ee_status_contract`: 2 passed.
- `group_e2ee_pending_contract`: 2 passed.
- `group_live_contract`: 3 passed.
- `group_contract`: 6 passed.
- `message_group_e2ee_wire_contract`: 7 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `group_service.rs` 1198 lines, `group_e2ee_remove.rs` 368 lines,
  `group_e2ee_provider.rs` 412 lines, `group_e2ee_transport.rs` 131 lines,
  `group_e2ee_handlers.rs` 495 lines, and
  `group_e2ee_remove_leave_contract.rs` 843 lines. No file-size exception is
  needed.

Scope:

- Wires live `group remove --e2ee` through Go's epoch-advancing remove path:
  active identity, member DID resolution, pre-mutation E2EE snapshot detection,
  external MLS `anp-mls group remove-member`, hidden `group.e2ee.remove`,
  local `group commit-finalize`, E2EE summary persistence, group state sync,
  and no normal P4 `group.remove`.
- Preserves Go app/service summary behavior: the service summary is
  `Removed member from group with group E2EE`, while the public CLI command
  keeps the Go `group remove` summary override `Removed member from group`.
- Wires live `group leave --e2ee` through Go's leave-request path only:
  cached active owners are rejected, P4 `group.leave` is not called, local MLS
  leave is not called, hidden `group.e2ee.leave_request` uses
  `transport-protected` security, and the owner-processing warning is returned.
- Wires live hidden `group e2ee process-leave-request` by defaulting the reason
  to `leave request processed by owner`, trimming `leave_request_id`, delegating
  to the same E2EE remove path, syncing group state, returning the Go summary
  `Processed group E2EE leave request with epoch-advancing remove`, and
  inserting the Go plan into live result data.
- Preserves Go terminal pending-commit params: finalize/abort include
  `agent_did`, `actor_did`, `device_id`, `group_did`, `commit_b64u`, and
  optional `pending_commit_id`, `subject_did`, `subject_status`, `from_epoch`,
  and `to_epoch`; they do not reuse the prepare `operation_id`.
- Preserves Go abort policy shape: abort is attempted only for deterministic
  service rejection, meaning HTTP 4xx or RPC code >= 2000, not HTTP 5xx,
  lower RPC codes, transport errors, or internal errors.

Boundary note: Go can return warnings/data alongside some failed
pending-commit submit paths through multiple return values. The current Rust
message service API returns `Result<CommandResult, MessageError>` and cannot
expose those side-channel warnings on error without a broader result type
change. This slice keeps the existing Rust error model and verifies the
success-path parity.

Parallelism note: one read-only Native Agent mapped the Go remove/leave
behavior. One GPT-5.5 xhigh code-writing Native Agent corrected the isolated
remove/leave test file under a bounded, non-overlapping test write scope. The
leader implemented production code, corrected remaining test fixture parity,
updated documentation, and ran final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `std::process::Command`, `serde_json`,
existing authsdk/session, existing Rustls/std message HTTP transport, existing
group E2EE wire builders, the external local ANP Rust SDK `anp-mls` binary, and
the approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Runtime Hermes Host-Notify Guide/Status Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue|TestRuntimeDryRunPlansCoverStableActions' -count=1
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/app/runtime_handlers.rs crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/mod.rs crates/awiki-cli/src/runtime/hermes_host_notify.rs crates/awiki-cli/tests/runtime_hermes_cli_contract.rs crates/awiki-cli/tests/runtime_hermes_bridge_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_host_notify_contract`: 8 passed.
- `runtime_contract`: 12 passed.
- `runtime_host_notify_sink_contract`: 10 passed.
- `runtime_host_notify_enable_disable_contract`: 2 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime_handlers.rs` 1003 lines, `hermes_bridge.rs` 878 lines,
  `runtime/mod.rs` 505 lines, `hermes_host_notify.rs` 267 lines,
  `runtime_hermes_cli_contract.rs` 336 lines, and
  `runtime_hermes_bridge_contract.rs` 370 lines. No file-size exception is
  needed.
- No active `awiki-system-test` selector currently covers
  `runtime host-notify hermes guide/status`; `rg` under `tests_v2` found no
  host-notify Hermes guide/status selector.

Scope:

- Adds command catalog, parser aliases, and dispatch for
  `runtime host-notify hermes guide`, `runtime host-notify webhook guide`,
  `runtime host-notify hermes status`, and
  `runtime host-notify webhook status`.
- Preserves Go guide behavior: `--deliver` override/defaulting, supported
  deliver validation, `invalid_argument` exit 2 for unsupported targets,
  `hermes_guide` output shape, recommended setup/verify commands, current
  host-notify snapshot, local Hermes route context when readable, and Go warning
  text for non-Hermes sink, missing secret, and `deliver: "log"`.
- Preserves Go status behavior for the read-only boundary: `host_notify`,
  `readiness`, `local_hermes`, `bridge`, `ready`, default summary, ready
  summary shape, route warning aggregation, bridge warning aggregation, and
  deduped warnings.
- Extends Hermes helper parity with read-only `InspectRoute` status fields:
  missing config defaults, configured webhook/route fields, route secret
  redaction, deliver/home-channel readiness, Feishu `.env` credential detection,
  and warnings for fixed `deliver_extra.chat_id` or missing home channel.
- Corrects Hermes host-notify config view secret metadata so config and env
  sources are reported like Go and secrets remain redacted.

Boundary note: this is intentionally a read-only Hermes slice. It does not
implement `runtime host-notify hermes setup`, `set`, `set-secret`,
`clear-secret`, route mutation, generated route secrets, full YAML
parser/serializer parity, listener refresh/restart, platform service-manager
status, bridge health probing, bridge process execution, or hidden bridge
`service-run`. Those side-effect and dependency-sensitive behaviors remain
separate parity slices.

Parallelism note: one GPT-5.5 xhigh code-writing Native Agent created the
isolated Hermes CLI contract test file under a bounded non-overlapping write
scope. The leader implemented production code, corrected the test expectation
where Go returns missing-config route state rather than an inspect warning,
added route-inspection helper coverage, updated documentation, and ran final
verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `serde_json`, std filesystem/env/path
handling, the existing hand-written awiki config parser, existing Hermes secret
helpers, existing Rustls/ring/base64/sha2 dependency paths, and the approved
`rusqlite + bundled` SQLite path. It does not add OpenSSL, `native-tls`,
bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async runtimes, YAML
crates, platform service libraries, ANP SDK default/network features, or a new
SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Runtime Hermes Host-Notify Local Write Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_config_write_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue' -count=1
cd ../awiki-cli && go test ./internal/config -run 'TestUpdateHermesSettingsDualWritesLegacyWebhook|TestSetAndClearHermesSecretDualWritesLegacyWebhook|TestHostNotifyMutatorsWriteSinkAndHermesConfig' -count=1
wc -l crates/awiki-cli/src/app/runtime_handlers.rs crates/awiki-cli/src/runtime/mod.rs crates/awiki-cli/src/cli/mod.rs crates/awiki-cli/src/cmdmeta/mod.rs crates/awiki-cli/tests/runtime_hermes_config_write_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `runtime_hermes_config_write_contract`: 11 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_host_notify_contract`: 8 passed.
- `runtime_contract`: 12 passed.
- `runtime_host_notify_sink_contract`: 10 passed.
- `runtime_host_notify_enable_disable_contract`: 2 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime_handlers.rs` 1134 lines, `runtime/mod.rs` 521 lines,
  `cli/mod.rs` 475 lines, `cmdmeta/mod.rs` 307 lines, and
  `runtime_hermes_config_write_contract.rs` 542 lines. No file-size exception
  is needed.
- No active `awiki-system-test` selector currently covers
  `runtime host-notify hermes set/set-secret/clear-secret`; `rg` under
  `tests_v2` found no host-notify Hermes write selector.

Scope:

- Adds command catalog, parser aliases, and dispatch for `runtime host-notify
  hermes set`, `runtime host-notify webhook set`, `runtime host-notify hermes
  set-secret`, `runtime host-notify webhook set-secret`, `runtime host-notify
  hermes clear-secret`, and `runtime host-notify webhook clear-secret`.
- Preserves Go `set` behavior: changed-flag detection, missing-flag
  `invalid_argument` exit 2 with hint `Use --notify-url or --deliver.`,
  `--deliver` normalization, dry-run `host_notify_hermes_set` plan before
  live deliver validation, non-dry-run unsupported-deliver rejection, persistent
  `notify_url`/`deliver` writes, summary `Hermes host notify config updated`,
  and local listener status context.
- Preserves Go sink boundary: `set` does not switch `runtime.host_notify.sink`.
  With the default `log` sink, Go returns an empty Hermes runtime object and
  the Rust command now does the same; with sink `hermes`, the runtime object
  includes the written `notify_url` and `deliver`.
- Preserves Go secret behavior: `set-secret` requires nonblank `--value`,
  dry-run reports only `configured=true`, live output never echoes the secret,
  and `clear-secret` reports `secret_configured=false`.
- Extends `host_notify_config_view` to preserve Go's file-config Hermes
  `deliver` fallback when the sink is not Hermes.

Boundary note: this slice intentionally implements only local awiki config
write commands. It does not implement `runtime host-notify hermes setup`, local
Hermes route mutation, generated route secrets, full YAML parser/serializer
parity, listener refresh/restart, platform service-manager status, bridge
health probing, bridge process execution, or hidden bridge `service-run`.

Parallelism note: one GPT-5.5 xhigh Native Agent created the isolated Hermes
write contract test file under a bounded non-overlapping write scope. A second
GPT-5.5 xhigh Native Agent added CLI metadata/parser/dispatch wiring under a
separate bounded source write scope. The leader implemented handlers, corrected
tests against Go source and live Go CLI probes, updated docs, and ran final
verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `serde_json`, std filesystem/env/path
handling, the existing hand-written awiki config parser/writer, existing Hermes
secret helpers, existing Rustls/ring/base64/sha2 dependency paths, and the
approved `rusqlite + bundled` SQLite path. It does not add OpenSSL,
`native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates, async
runtimes, YAML crates, platform service libraries, ANP SDK default/network
features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Runtime Hermes Host-Notify Setup Dry-Run Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_config_write_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue' -count=1
cd ../awiki-cli && go test ./internal/config -run 'TestUpdateHermesSettingsDualWritesLegacyWebhook|TestSetAndClearHermesSecretDualWritesLegacyWebhook|TestHostNotifyMutatorsWriteSinkAndHermesConfig' -count=1
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/app/runtime_handlers.rs crates/awiki-cli/src/app/runtime_hermes_handlers.rs crates/awiki-cli/tests/runtime_hermes_setup_dry_run_contract.rs crates/awiki-cli/src/app.rs crates/awiki-cli/src/cli/mod.rs crates/awiki-cli/src/cmdmeta/mod.rs
```

Result: passed for the commands listed above.

Observed results:

- `runtime_hermes_setup_dry_run_contract`: 11 passed.
- `runtime_hermes_config_write_contract`: 11 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_host_notify_contract`: 8 passed.
- `runtime_contract`: 12 passed.
- `runtime_host_notify_sink_contract`: 10 passed.
- `runtime_host_notify_enable_disable_contract`: 2 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  focused reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime_handlers.rs` 721 lines, `runtime_hermes_handlers.rs` 616
  lines, `runtime_hermes_setup_dry_run_contract.rs` 472 lines, `app.rs` 1025
  lines, `cli/mod.rs` 479 lines, and `cmdmeta/mod.rs` 308 lines. No file-size
  exception is needed.
- `rg` under `/home/ecs-user/awiki-space/awiki-system-test` found OpenClaw
  host-notify selectors but no Hermes setup selector; system-level Hermes setup
  acceptance remains unavailable in that repo for this dry-run slice.

Scope:

- Adds command catalog, parser aliases, and dispatch for
  `runtime host-notify hermes setup` and `runtime host-notify webhook setup`.
- Extracts Hermes app handlers and helpers into
  `crates/awiki-cli/src/app/runtime_hermes_handlers.rs` so
  `runtime_handlers.rs` stays well below the 1200-line cap.
- Preserves Go dry-run plan behavior: canonical command output for the
  `webhook` alias, summary `Dry run: Hermes host notify setup planned`,
  action `host_notify_hermes_setup`, default notify URL, default `feishu`
  deliver target, `previous_sink`, `host_notify_enabled=true`, awiki and
  Hermes config paths, `manages_local_hermes=true`, `starts_local_bridge=true`,
  and `route_uses_home_channel=true`.
- Preserves Go validation and source resolution: local notify URL validation,
  unsupported deliver rejection, non-empty explicit `--secret`, raw
  `runtime.host_notify.hermes.notify_url` fallback, legacy
  `runtime.host_notify.webhook.notify_url` fallback, raw
  `runtime.host_notify.hermes.deliver` fallback even when the current sink is
  not Hermes, and secret-source reporting for `flag`, `config_file`,
  `environment`, and `generated` without emitting secret values.
- Preserves Go setup secret error behavior: malformed config causes
  `internal_error` with hint `Check awiki-cli host notify secret sources.`
  instead of silently reporting `secret_source=generated`.
- Locks dry-run no-write behavior with filesystem assertions for awiki config,
  Hermes config, listener PID/status/expected-boot files, and bridge socket
  artifacts.

Boundary note: this slice intentionally stops at dry-run setup planning and
validation. Non-dry-run `runtime host-notify hermes setup` remains deferred and
currently returns the standard side-effect `not_implemented` error after
validation. The deferred Go behavior includes `ConfigureHermesHostNotify`,
local Hermes `EnsureRoute`, route-secret generation/persistence, Hermes YAML
mutation, listener refresh/restart, bridge `Apply`, bridge process/service
execution, health probing, and hidden bridge `service-run`.

Parallelism note: one GPT-5.5 xhigh code-writing Native Agent created the
initial isolated setup dry-run contract test file under a bounded
non-overlapping write scope. A read-only Native Agent then reviewed the diff
against Go and found raw config fallback and secret-source coverage gaps; the
leader corrected those gaps, added the missing tests, split the source file,
updated documentation, and ran final verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `serde_json`, std filesystem/env/path
handling, the existing hand-written awiki config parser, existing Hermes bridge
validation and secret env constants, existing Rustls/ring/base64/sha2
dependency paths, and the approved `rusqlite + bundled` SQLite path. It does
not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
crates, async runtimes, YAML crates, platform service libraries, ANP SDK
default/network features, or a new SQLite backend. TLS remains Rustls-first.

## 2026-05-16 Runtime Hermes EnsureRoute Local Writer Slice

Status: locally verified.

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_ensure_route_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_config_write_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_sink_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/route.rs crates/awiki-cli/tests/runtime_hermes_ensure_route_contract.rs
```

Result: passed for the commands listed above.

Observed results:

- `runtime_hermes_ensure_route_contract`: 8 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_setup_dry_run_contract`: 11 passed.
- `runtime_hermes_config_write_contract`: 11 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_host_notify_contract`: 8 passed.
- `runtime_contract`: 12 passed.
- `runtime_host_notify_sink_contract`: 10 passed.
- `runtime_host_notify_enable_disable_contract`: 2 passed.
- `cargo check`, structure check, whitespace check, dependency audit, and Go
  Hermes bridge reference tests passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime/hermes_bridge.rs` 882 lines,
  `runtime/hermes_bridge/route.rs` 686 lines, and
  `runtime_hermes_ensure_route_contract.rs` 401 lines. No file-size exception
  is needed.

Scope:

- Adds `runtime::hermes_bridge::EnsureRouteOptions` and
  `runtime::hermes_bridge::ensure_route` in a split
  `crates/awiki-cli/src/runtime/hermes_bridge/route.rs` module so the main
  Hermes bridge source file stays below the 1200-line review-size cap.
- Mirrors Go `EnsureRoute` option defaulting for Hermes home, route name,
  deliver target, webhook port, and prompt.
- Creates missing local Hermes `config.yaml` route state with
  `platforms.webhook.enabled=true`, default/positive webhook port handling,
  generated 24-byte hex route secrets, `events: []` when absent, current
  mail-aware Chinese notify prompt, and requested `deliver`.
- Preserves existing route secret, existing positive port, existing route
  events, custom prompt, custom non-notify skills, unmanaged top-level blocks,
  unmanaged platform blocks, unmanaged webhook/extra fields, and unmanaged
  sibling routes covered by the focused contract tests.
- Migrates legacy Go default prompts to the current default, removes only the
  single legacy `skills: ["notify"]` stanza, removes fixed
  `deliver_extra.chat_id`, `thread_id`, and `message_thread_id`, and preserves
  unrelated `deliver_extra` keys sorted through `BTreeMap` output.
- Writes through a same-directory `.hermes-config-*.tmp` file, syncs it,
  applies Unix `0600` temp-file permissions, renames atomically over
  `config.yaml`, creates the config directory with Unix `0700` when missing,
  and cleans up temporary files on failure.
- Returns the existing `InspectRoute` state so route warnings, home-channel
  readiness, Feishu credential detection, and notify webhook URL remain shared
  with the read-only status/guide path.

Boundary note: this is a pure local helper slice. It does not wire non-dry-run
`runtime host-notify hermes setup`, awiki config writes, listener
refresh/restart, bridge `Apply`, bridge process/service execution, platform
service-manager status, health probing, hidden bridge `service-run`, or
awiki-system-test acceptance. The narrow renderer does not claim full Go
`yaml.v3` round-trip parity for comments, anchors, complex scalars, or arbitrary
formatting; full Hermes YAML parser/serializer parity remains a separate
recorded dependency/format decision.

Parallelism note: two read-only Native Agents mapped the Go `EnsureRoute`
contract and the existing Rust helper surface. One GPT-5.5 xhigh code-writing
Native Agent created the isolated ensure-route contract test file under a
bounded non-overlapping write scope. The leader implemented the production
helper, added follow-up preservation guards, updated docs, and runs final
verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses std filesystem/string handling, the existing
hand-written scalar inspection helper, existing `rand`, existing
Rustls/ring/base64/sha2 dependency paths, and the approved `rusqlite + bundled`
SQLite path. It does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
`hyper`, WebSocket crates, async runtimes, YAML crates, platform service
libraries, ANP SDK default/network features, or a new SQLite backend. TLS
remains Rustls-first.

## 2026-05-16 Hermes bridge service local helper slice

Timestamp: 2026-05-16T14:59:57Z / 2026-05-16T22:59:57+0800.

Scope: translate the deterministic local subset of Go
`internal/runtime/hermesbridge/service.go` into Rust without introducing
platform service-manager dependencies or wiring live Hermes setup.

Commands run:

```text
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_ensure_route_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/route.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs
```

Observed results:

- `cargo fmt --check` passed.
- `cargo check -p awiki-cli --locked` passed.
- `runtime_hermes_bridge_service_contract`: 7 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_ensure_route_contract`: 8 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_setup_dry_run_contract`: 11 passed.
- `runtime_contract`: 12 passed.
- Go `./internal/runtime/hermesbridge`: passed.
- Structure check, whitespace check, and dependency audit passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime/hermes_bridge.rs` 879 lines,
  `runtime/hermes_bridge/route.rs` 686 lines,
  `runtime/hermes_bridge/service.rs` 156 lines, and
  `runtime_hermes_bridge_service_contract.rs` 291 lines. No file-size
  exception is needed.

Implemented behavior:

- Adds split module `crates/awiki-cli/src/runtime/hermes_bridge/service.rs`
  and re-exports its helper surface from `runtime::hermes_bridge`.
- Mirrors Go service naming with `awiki-cli-hermes-bridge` and
  `awiki-cli-hermes-bridge-<first 12 sha256 hex>` for nonblank workspaces,
  including the Go nil/default workspace hash behavior and the blank-workspace
  prefix fallback.
- Mirrors Go display names with `awiki-cli Hermes Bridge` and the workspace
  basename suffix.
- Captures the Go `newService` config shape as a pure
  `BridgeServiceConfigPlan`: hidden `runtime host-notify hermes bridge
  service-run` arguments, user service, keepalive, restart-on-failure,
  one-second failure delay, log output/log directory, workspace and
  `HERMES_HOME` env values, and Windows empty working directory behavior.
- Adds hidden service-run command detection for
  `runtime host-notify hermes bridge service-run`, matching the Go service
  argument boundary.
- Adds injected health probing that returns true only for 2xx status codes and
  false for blank URL, request errors, or non-2xx statuses.
- Adds `waitForStatus`-style polling with Go readiness semantics: starting
  requires `running && bridge_available`, stopping requires `!running`, status
  errors are ignored until timeout, and timeout returns the last observed
  status rather than an error.
- Adds a pure `Apply` decision helper for the Go branch order:
  install-then-start when not installed, restart when installed and running,
  start when installed and stopped.

Boundary note: this slice intentionally does not implement platform service
install/start/stop/restart/uninstall, real service-manager status,
`serviceProgram` adapter process lifecycle, owned HTTP client health probing,
`RunService`, bridge execution, listener refresh/restart, non-dry-run
`runtime host-notify hermes setup`, or awiki-system-test acceptance. Those
remain later runtime parity slices and dependency decisions.

Parallelism note: one GPT-5.5 xhigh Native Agent was launched with an isolated
code-writing scope for this helper/test slice, but it timed out without a
visible worktree change and was shut down. The leader completed the slice,
updated docs, and owns verification.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing `sha2` and std-only path/time/polling
helpers. It does not add a `kardianos/service` equivalent, systemd/launchd or
Windows service crates, `reqwest`, `hyper`, WebSocket crates, async runtimes,
OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, ANP SDK default/network
features, or a new SQLite backend. TLS remains Rustls-first, and the approved
SQLite lane remains `rusqlite + bundled`.

## 2026-05-16 Hermes bridge adapter command plan extension

Timestamp: 2026-05-16T15:09:48Z / 2026-05-16T23:09:48+0800.

Scope: extend the Hermes bridge service helper slice with a pure command plan
for Go `serviceProgram.Start` without starting the adapter process or selecting
platform service-management dependencies.

Commands run:

```text
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/route.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs
```

Observed results:

- `cargo fmt --check` passed.
- `cargo check -p awiki-cli --locked` passed.
- `runtime_hermes_bridge_service_contract`: 8 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- Go `./internal/runtime/hermesbridge`: passed.
- Structure check, whitespace check, and dependency audit passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime/hermes_bridge.rs` 880 lines,
  `runtime/hermes_bridge/route.rs` 686 lines,
  `runtime/hermes_bridge/service.rs` 189 lines, and
  `runtime_hermes_bridge_service_contract.rs` 367 lines. No file-size
  exception is needed.

Implemented behavior:

- Adds `BridgeAdapterCommandPlan` and
  `runtime::hermes_bridge::adapter_command_plan_for`.
- Mirrors the Go `exec.CommandContext` shape used by
  `internal/runtime/hermesbridge/service.go` `serviceProgram.Start`:
  Python executable, adapter script, `--host`, `--port`, `--notify-secret`,
  `--hermes-webhook-url`, `--hermes-route-secret`, and `--log-level INFO`.
- Preserves Go's environment behavior by planning `HERMES_HOME` only when
  `BridgeConfig.HermesHome` is nonempty.
- Records Go stdout/stderr behavior as parent-inherited stream flags.

Boundary note: this is still a pure helper. It does not implement
`exec.CommandContext`, adapter process start/wait/stop/kill, cancellation,
15-second stop timeout, `RunService`, platform service install/start/stop,
owned HTTP health probing, non-dry-run Hermes setup, or awiki-system-test
Hermes setup acceptance.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This extension reuses existing structs/std types and does not add a
process-supervision crate, platform service library, HTTP/TLS client,
OpenSSL/native-tls path, YAML crate, WebSocket crate, ANP SDK network feature,
or SQLite dependency.

## 2026-05-16 Hermes bridge status aggregation helper extension

Timestamp: 2026-05-16T15:15:34Z / 2026-05-16T23:15:34+0800.

Scope: extend the Hermes bridge service helper slice with injectable
`StatusFor` aggregation logic from Go `internal/runtime/hermesbridge/service.go`
without adding platform service-manager or HTTP-client dependencies.

Commands run:

```text
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/route.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs
```

Observed results:

- `cargo fmt --check` passed after formatting.
- `cargo check -p awiki-cli --locked` passed.
- `runtime_hermes_bridge_service_contract`: 12 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- Go `./internal/runtime/hermesbridge`: passed.
- Structure check, whitespace check, and dependency audit passed.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime/hermes_bridge.rs` 865 lines,
  `runtime/hermes_bridge/route.rs` 686 lines,
  `runtime/hermes_bridge/service.rs` 248 lines, and
  `runtime_hermes_bridge_service_contract.rs` 488 lines. No file-size
  exception is needed.

Implemented behavior:

- Adds `BridgeServiceStatusSnapshot` and
  `runtime::hermes_bridge::status_from_parts`.
- Preserves Go `StatusFor` config-error boundary: return a status with the
  service name and warning instead of failing the caller.
- Preserves service-status warning text:
  `Hermes bridge service status unavailable: <err>`.
- Preserves service-status field merge behavior when injected status is
  available: installed/running/platform/service-name replace the defaults.
- Preserves Go running-only health probing and warning behavior:
  `Hermes bridge health endpoint is not responding`.
- Preserves route-state warning propagation after service/health warnings.
- Keeps current Rust production boundary by making `status_for` call
  `status_from_parts(..., Ok(None), |_| false)`, so public CLI status still
  reports the existing `rust-local` no-platform-service state until a later
  service-manager slice supplies real status.

Boundary note: this remains a pure helper. It does not implement
`kardianos/service` parity, platform service lookup/control, owned HTTP health
client construction, adapter process lifecycle, non-dry-run Hermes setup,
or awiki-system-test Hermes setup acceptance.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This extension uses existing structs and injected closures only; it
does not add platform service libraries, HTTP/TLS clients, OpenSSL/native-tls,
bundled OpenSSL, YAML crates, WebSocket crates, process supervision crates, ANP
SDK network features, or SQLite dependencies.

## 2026-05-16 Hermes bridge lifecycle planner extension

Timestamp: 2026-05-16T15:21:49Z / 2026-05-16T23:21:49+0800.

Scope: extend the Hermes bridge service helper slice with pure lifecycle branch
plans for Go `EnsureInstalled`, `StartService`, `StopService`,
`RestartService`, `Uninstall`, and `Apply` without executing service-manager
operations.

Commands run:

```text
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_ensure_route_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/route.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs
```

Observed results:

- `cargo fmt --check`, `cargo check -p awiki-cli`, `xtask
  check-structure`, and `git diff --check` passed.
- `runtime_hermes_bridge_service_contract`: 14 passed.
- `runtime_hermes_bridge_contract`: 10 passed.
- `runtime_hermes_ensure_route_contract`: 8 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- `runtime_hermes_setup_dry_run_contract`: 11 passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime/hermes_bridge.rs` 867 lines,
  `runtime/hermes_bridge/route.rs` 686 lines,
  `runtime/hermes_bridge/service.rs` 336 lines, and
  `runtime_hermes_bridge_service_contract.rs` 581 lines. No file-size
  exception is needed.

Implemented behavior:

- Adds `BridgeServiceLifecycleOperation` and pure operation planners:
  `ensure_installed_plan`, `start_service_plan`, `stop_service_plan`,
  `restart_service_plan`, `uninstall_service_plan`, and `apply_service_plan`.
- Preserves Go `EnsureInstalled` branching: install only when missing, then
  return status.
- Preserves Go `StartService` branching: ensure install when missing, return
  status if already running, otherwise start then wait for running.
- Preserves Go `StopService` branching: return status when not installed, stop
  only when running, then wait for stopped.
- Preserves Go `RestartService` branching: fail when not installed, otherwise
  restart then wait for running.
- Preserves Go `Uninstall` branching: return status when not installed, stop
  first when running, uninstall, then return status.
- Preserves Go `Apply` branch selection as an operation plan:
  ensure-install/start when missing, restart when running, start when stopped.

Boundary note: this remains a pure planner. It does not call
`svc.Install/Start/Stop/Restart/Uninstall`, does not implement the
`exists`/`ErrNotInstalled` error exceptions, does not run `waitForStatus`
against a real service, does not perform platform service lookup/control, and
does not wire non-dry-run Hermes setup.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This extension is enum/vector planning only and does not add
platform service libraries, process supervisors, HTTP/TLS clients,
OpenSSL/native-tls, bundled OpenSSL, YAML crates, WebSocket crates, ANP SDK
network features, or SQLite dependencies.

## 2026-05-16 Hermes setup local transaction slice

Timestamp: 2026-05-16T15:38:09Z / 2026-05-16T23:38:09+0800.

Scope: extend `runtime host-notify hermes setup` beyond dry-run validation to
perform the local-file half of Go setup: write awiki host-notify config and
ensure the local Hermes notify route. This slice still does not execute
listener refresh/restart or bridge service install/start/restart.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_config_write_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_ensure_route_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/cli -run 'TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue|TestRuntimeDryRunPlansCoverStableActions' -count=1
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/app/runtime_hermes_handlers.rs crates/awiki-cli/tests/runtime_hermes_setup_dry_run_contract.rs
```

Observed results:

- `cargo fmt`, `cargo fmt --check`, `cargo check -p awiki-cli`, `xtask
  check-structure`, and `git diff --check` passed.
- `runtime_hermes_setup_dry_run_contract`: 12 passed.
- `runtime_hermes_config_write_contract`: 11 passed.
- `runtime_hermes_ensure_route_contract`: 8 passed.
- `runtime_hermes_bridge_service_contract`: 14 passed.
- `runtime_hermes_cli_contract`: 5 passed.
- Go focused `internal/cli` Hermes tests passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- Changed Rust source/test files remain below the default 1200-line review-size
  cap: `runtime_hermes_handlers.rs` 705 lines and
  `runtime_hermes_setup_dry_run_contract.rs` 612 lines. No file-size exception
  is needed.

Implemented behavior:

- Non-dry-run setup now calls the existing Rust
  `configure_hermes_host_notify` parity writer, preserving Go's
  `runtime.host_notify.enabled=true`, `sink=hermes`, Hermes notify URL,
  Hermes deliver target, and legacy webhook notify URL/secret mirroring.
- Setup re-resolves config after the awiki write, then calls the existing
  `ensure_route` parity helper for the default `notify` route under
  `$HERMES_HOME/config.yaml`.
- Output keeps Go's completed summary shape and includes `host_notify`,
  `local_hermes`, current `listener`, passive `bridge`, and `next_steps`.
- Secret values are persisted where Go persists them, but remain redacted from
  stdout/stderr and serialized route/status objects.
- Contract tests assert that non-dry-run setup creates awiki config and Hermes
  route files, supports explicit notify URL/deliver/secret flags, preserves
  Telegram's Go home-channel key `TELEGRAM_HOME_CHANNEL`, and does not create
  listener PID/status/expected-boot/socket artifacts.

Boundary note: this slice intentionally does not call Go-equivalent
`refreshListenerForHostNotifyChange` or `hermesbridge.Apply`. Rust setup emits
explicit warnings that listener refresh/restart and bridge service
install/start are deferred. Real platform service status/control, adapter
process lifecycle, owned HTTP health probing, hidden bridge `service-run`, and
full awiki-system-test Hermes setup acceptance remain deferred.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. This slice reuses existing std filesystem/env/path handling, the
existing awiki config writer, existing Hermes route writer, existing passive
status helpers, and existing `rand` for generated secrets. It does not add
platform service libraries, process supervisors, HTTP/TLS clients,
OpenSSL/native-tls, bundled OpenSSL, YAML crates, WebSocket crates, ANP SDK
network features, or SQLite dependencies.

## 2026-05-16 Hermes bridge Python executable lookup slice

Timestamp: 2026-05-16T15:49:47Z / 2026-05-16T23:49:47+0800.

Scope: adjust the Rust Hermes bridge `resolve_python_executable` helper to
match Go `exec.LookPath` behavior more closely before real bridge process
execution is wired.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli runtime::hermes_bridge::tests --lib --locked
cargo +1.79.0 test -p awiki-cli hermes_bridge::tests --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs
```

Observed results:

- `cargo fmt`, `cargo fmt --check`, `cargo check -p awiki-cli`, and `xtask
  check-structure` passed.
- Focused library `runtime::hermes_bridge::tests`: 2 passed.
- Broader filtered `hermes_bridge::tests` run passed the same 2 focused library
  tests; integration test binaries were compiled and had all unrelated tests
  filtered.
- `runtime_hermes_bridge_service_contract`: 14 passed.
- Go `internal/runtime/hermesbridge` tests passed.
- `runtime/hermes_bridge.rs` is 975 lines, below the default 1200-line
  review-size cap. No file-size exception is needed.

Implemented behavior:

- `resolve_python_executable` now returns the resolved executable path from the
  PATH search rather than the bare `python3` or `python` command name.
- Search priority remains Go-compatible: `python3` is preferred before
  `python`, even when `python` appears earlier in PATH.
- Unix lookup now ignores non-executable regular files, matching the important
  `exec.LookPath` permission boundary for local bridge adapter startup.
- Tests keep the lookup seam private to the module and avoid changing public
  CLI/API surface.

Boundary note: this is still a local helper. It does not start the Python
adapter, does not implement `serviceProgram.Start`/`Stop`, and does not add
platform service execution. Windows currently checks direct candidate plus
`.exe` candidate and does not claim full Go `PATHEXT` parity.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The slice uses only std PATH, filesystem, and Unix permission APIs.

## 2026-05-16 Hermes bridge adapter script lookup slice

Timestamp: 2026-05-16T15:55:03Z / 2026-05-16T23:55:03+0800.

Scope: make the Rust Hermes bridge adapter script resolver follow the Go
`resolveAdapterScriptPath` candidate order through a testable helper, without
starting the adapter or introducing service-manager/process dependencies.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 test -p awiki-cli runtime::hermes_bridge::tests --lib --locked
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs
```

Observed results:

- `cargo fmt`, focused library tests, `cargo check -p awiki-cli`, and `xtask
  check-structure` passed.
- Focused library `runtime::hermes_bridge::tests`: 4 passed, including the
  existing Python lookup tests plus adapter script candidate-order tests.
- `runtime_hermes_bridge_service_contract`: 14 passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- `runtime/hermes_bridge.rs` is 1024 lines, below the default 1200-line
  review-size cap. No file-size exception is needed.

Implemented behavior:

- Adapter script lookup now flows through a private helper that preserves the
  Go candidate order relative to the awiki-cli executable directory:
  `../scripts/hermes_notify_adapter.py`,
  `scripts/hermes_notify_adapter.py`, then
  `../../scripts/hermes_notify_adapter.py`.
- Existing file paths are canonicalized before being returned, matching Go's
  `filepath.Clean` intent more closely than returning raw `..`-containing
  paths.
- Tests lock both the candidate order and first-existing-file selection without
  invoking a Python process or the platform service manager.

Boundary note: this remains a local resolver slice. It does not implement
adapter process start/wait/stop/kill, hidden bridge `service-run`, platform
service install/start/restart, or health probing.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The slice uses only std path and filesystem APIs.

## 2026-05-16 Hermes bridge service-run dispatch boundary

Timestamp: 2026-05-16T16:01:43Z / 2026-05-17T00:01:43+0800.

Scope: route the hidden Hermes bridge `runtime host-notify hermes bridge
service-run` command to a dedicated deferred bridge-execution boundary, matching
the Go command dispatch shape without starting the bridge adapter or invoking a
platform service manager.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
git diff --check
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue' -count=1
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/app/runtime_hermes_handlers.rs crates/awiki-cli/src/cli/mod.rs crates/awiki-cli/tests/runtime_hermes_cli_contract.rs
```

Observed results:

- `cargo fmt`, `cargo fmt --check`, `cargo check -p awiki-cli`, `xtask
  check-structure`, and `git diff --check` passed.
- `runtime_hermes_cli_contract`: 6 passed, including the new hidden
  service-run dispatch test.
- `update_contract`: 6 passed, preserving the update-preflight exemptions for
  hidden service commands.
- `runtime_hermes_bridge_service_contract`: 14 passed.
- Go focused `internal/cli` Hermes tests passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- Touched source/test files remain below the default 1200-line review-size
  cap: `runtime_hermes_handlers.rs` 722 lines, `cli/mod.rs` 482 lines, and
  `runtime_hermes_cli_contract.rs` 366 lines. No file-size exception is needed.

Implemented behavior:

- The CLI dispatcher now maps
  `runtime.host-notify.hermes.bridge.service-run` to a dedicated app handler
  instead of falling through to the generic schema stub.
- The app handler resolves runtime config like Go's
  `runRuntimeHostNotifyHermesBridgeServiceRun` entry point, then returns an
  explicit `not_implemented` boundary for the still-deferred
  `hermesbridge.RunService` / `serviceProgram.Start` / `serviceProgram.Stop`
  translation.
- The focused CLI contract asserts the command exits at the dedicated bridge
  boundary and that the hint no longer references the generic schema stub.

Boundary note: this is intentionally dispatch-only. It does not start or stop
the Hermes adapter process, does not wait on adapter lifecycle, does not
install/start/restart/stop platform services, does not run owned bridge health
probing, and does not claim full Go `RunService` parity.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The slice reuses existing CLI dispatch, config resolution, and error
envelope code.

## 2026-05-16 Hermes bridge adapter process lifecycle helper slice

Timestamp: 2026-05-16T16:09:09Z / 2026-05-17T00:09:09+0800.

Scope: extend the Hermes bridge service helper slice with a std-only child
process helper for the local adapter portion of Go
`serviceProgram.Start`/`Stop`, without adding a platform service-manager
dependency or wiring hidden `RunService` execution.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli runtime::hermes_bridge::tests --lib --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
git diff --check
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs
```

Observed results:

- `cargo fmt`, `cargo fmt --check`, `cargo check -p awiki-cli`, `xtask
  check-structure`, and `git diff --check` passed.
- `runtime_hermes_bridge_service_contract`: 16 passed, including adapter
  process env/argument consumption and stop/kill behavior tests.
- Focused library `runtime::hermes_bridge::tests`: 4 passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- Source/test files remain below the default 1200-line review-size cap:
  `runtime/hermes_bridge.rs` 1025 lines, `runtime/hermes_bridge/service.rs`
  471 lines, and `runtime_hermes_bridge_service_contract.rs` 674 lines. No
  file-size exception is needed.

Implemented behavior:

- Adds `BridgeAdapterProcess`, a zero-value/defaultable helper that spawns the
  existing `BridgeAdapterCommandPlan`, matching the Go adapter child process
  shape without resolving config or service-manager state.
- Preserves Go `serviceProgram.Start` process semantics that are local to the
  adapter: executable plus arguments, inherited parent environment, optional
  `HERMES_HOME` override only when configured, and parent stdout/stderr
  inheritance when requested by the command plan.
- Adds wait/try-wait/running inspection and a stop path that kills a running
  child and waits until it exits, using the Go 15-second stop timeout and
  `Hermes bridge stop timed out` error text.
- Focused Unix contract tests use `/bin/sh` and `sleep` instead of Python or
  Hermes so the slice verifies lifecycle behavior without requiring a real
  Hermes installation.

Boundary note: this is still a helper slice. It does not implement
`kardianos/service` parity, platform service install/start/stop/restart/
uninstall, real service status lookup, lifecycle operation execution, owned
HTTP health probing, hidden `RunService` integration, or non-dry-run setup
bridge execution.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The slice uses only `std::process`, `std::env` inheritance semantics,
and std time/thread polling.

## 2026-05-16 Hermes bridge service-run preflight slice

Timestamp: 2026-05-16T16:18:20Z / 2026-05-17T00:18:20+0800.

Scope: make the hidden Hermes bridge `service-run` CLI entry follow Go
`runRuntimeHostNotifyHermesBridgeServiceRun` / `hermesbridge.RunService`
preflight before the still-deferred Rust execution boundary.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
git diff --check
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue' -count=1
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
wc -l crates/awiki-cli/src/app/runtime_hermes_handlers.rs crates/awiki-cli/tests/runtime_hermes_cli_contract.rs
```

Observed results:

- `cargo fmt`, `cargo fmt --check`, `cargo check -p awiki-cli`, `xtask
  check-structure`, and `git diff --check` passed.
- `runtime_hermes_cli_contract`: 7 passed, including missing-secret preflight
  before the deferred boundary and the configured preflight path that reaches
  the dedicated `not_implemented` boundary.
- `update_contract`: 6 passed, preserving update-preflight exemptions for
  hidden service commands.
- `runtime_hermes_bridge_service_contract`: 16 passed.
- Go focused `internal/cli` Hermes tests passed.
- Go `internal/runtime/hermesbridge` tests passed.
- Dependency audit output showed only existing allowed hits: Rustls/ring
  transport dependencies, `base64`/`sha2`, and the approved
  `rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
  (`cc`, `pkg-config`, `vcpkg`). No OpenSSL, `native-tls`, bundled OpenSSL,
  platform service, YAML, WebSocket, or new HTTP-client dependency was added.
- Touched source/test files remain below the default 1200-line review-size
  cap: `runtime_hermes_handlers.rs` 725 lines and
  `runtime_hermes_cli_contract.rs` 445 lines. No file-size exception is needed.

Implemented behavior:

- Hidden `runtime host-notify hermes bridge service-run` now resolves bridge
  config through `runtime::hermes_bridge::resolve_bridge_config` before it
  returns the Rust deferred execution boundary.
- The handler builds the adapter command plan from the resolved bridge config,
  matching the Go `RunService -> newService -> ResolveBridgeConfig` entry path
  far enough to surface missing notify secret, missing route secret, missing
  Python, or missing adapter script errors before `not_implemented`.
- The configured-path test provides a temporary Hermes route, temporary
  executable `python3`, and temporary adapter-script candidate, but still
  asserts that no process is launched and the handler returns the explicit
  deferred `RunService` boundary.

Boundary note: this is still preflight-only. It does not launch the adapter
process, does not call `BridgeAdapterProcess`, does not implement
`kardianos/service` parity, does not run owned health probing, and does not
claim full hidden `RunService` execution parity.

Dependency note: no dependency was added. Cargo manifests and lockfile remain
unchanged. The test creates temporary local files only.

## 2026-05-16 Hermes notify adapter script asset slice

Timestamp: 2026-05-16T16:26:10Z / 2026-05-17T00:26:10+0800.

Scope: copy the Go Hermes notify adapter Python runtime asset and its Python
unit tests into the Rust repository so the translated Go candidate-order
adapter script lookup can resolve a real `scripts/hermes_notify_adapter.py`
asset without rewriting adapter behavior in Rust.

Commands run:

```text
cmp -s ../awiki-cli/scripts/hermes_notify_adapter.py scripts/hermes_notify_adapter.py
cmp -s ../awiki-cli/scripts/test_hermes_notify_adapter.py scripts/test_hermes_notify_adapter.py
stat -c '%a %n' scripts/hermes_notify_adapter.py scripts/test_hermes_notify_adapter.py
wc -l scripts/hermes_notify_adapter.py scripts/test_hermes_notify_adapter.py
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
python3 -m py_compile scripts/hermes_notify_adapter.py
python3 -m unittest discover -s scripts -p 'test_hermes_notify_adapter.py'
cd ../awiki-cli && python3 -m py_compile scripts/hermes_notify_adapter.py
cd ../awiki-cli && python3 -m unittest discover -s scripts -p 'test_hermes_notify_adapter.py'
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64'
```

Observed results:

- The copied files are byte-identical to the Go repository sources.
- Permissions were preserved from the Go repository: adapter `775`, test `664`.
- Python file sizes remain below the requested review-size cap:
  `hermes_notify_adapter.py` 673 lines and `test_hermes_notify_adapter.py`
  187 lines.
- Rust formatting, check, focused Hermes CLI/service contract tests, Python
  compile/unit tests in both repositories, structure check, whitespace check,
  and dependency audit passed.

Implemented behavior:

- Adds `scripts/hermes_notify_adapter.py` as the same stdlib-only adapter that
  Go launches from `serviceProgram.Start`.
- Adds `scripts/test_hermes_notify_adapter.py` as the same Python unit test
  surface for adapter validation and host-event conversion helpers.
- Keeps the existing Rust service-run boundary unchanged: preflight can now
  resolve the repository-local adapter script asset, but hidden `service-run`
  still returns the explicit deferred execution boundary instead of launching
  the adapter process.

Boundary note: this slice copies the runtime asset exactly. It does not
translate the adapter to Rust, does not launch the adapter, does not implement
platform service-manager behavior, and does not verify final release archive
contents. Release archive inclusion for this asset remains a later packaging
parity check because the current repository surface only exposes the npm
install/run wrappers and no inspected archive manifest.

Dependency note: no Rust dependency was added. The copied adapter uses only
Python stdlib modules, matching Go repository behavior, and is outside the Rust
binary dependency graph.

## 2026-05-16 Hermes bridge service-run adapter execution slice

Timestamp: 2026-05-16T16:44:01Z / 2026-05-17T00:44:01+0800.

Scope: replace the hidden Hermes bridge `service-run` deferred boundary with a
local `RunService`-shaped adapter process loop. This follows Go
`RunService -> serviceProgram.Start -> wait -> serviceProgram.Stop` for the
adapter child process while still excluding platform service-manager
install/start/status/control.

Commands run:

```text
cargo +1.79.0 fmt
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cargo +1.79.0 test -p awiki-cli runtime::hermes_bridge::tests --lib --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64|libc'
cd ../awiki-cli && go test ./internal/runtime/hermesbridge -count=1
cd ../awiki-cli && go test ./internal/cli -run 'TestRuntimeDryRunPlansCoverStableActions|TestBuildHermesHostNotifyGuideViewPrefersHomeChannelGuidance|TestHostNotifyConfigViewRedactsHermesSecretValue' -count=1
wc -l crates/awiki-cli/src/runtime/hermes_bridge.rs crates/awiki-cli/src/runtime/hermes_bridge/service.rs crates/awiki-cli/src/app/runtime_hermes_handlers.rs crates/awiki-cli/tests/runtime_hermes_bridge_service_contract.rs crates/awiki-cli/tests/runtime_hermes_cli_contract.rs
```

Observed results:

- `cargo check`, formatting, whitespace, structure check, focused Rust
  Hermes/update tests, focused Go Hermes tests, and dependency audit passed.
- `runtime_hermes_bridge_service_contract`: 17 passed, including the new
  injected stop-condition service-run helper test.
- `runtime_hermes_cli_contract`: 7 passed, including a subprocess test that
  starts the hidden CLI service-run path with a fake `python3`, observes the
  adapter marker file, sends SIGTERM to the CLI, and verifies clean exit with
  no success/error envelope.
- Source/test files remain below the default 1200-line cap:
  `hermes_bridge.rs` 1025 lines, `hermes_bridge/service.rs` 537 lines,
  `runtime_hermes_handlers.rs` 721 lines,
  `runtime_hermes_bridge_service_contract.rs` 716 lines, and
  `runtime_hermes_cli_contract.rs` 475 lines.

Implemented behavior:

- Adds `runtime::hermes_bridge::run_bridge_service` and an injected
  `run_bridge_service_with_stop` helper that starts `BridgeAdapterProcess`,
  waits for shutdown, and stops the child with the Go 15-second timeout/error
  text.
- Wires hidden `runtime host-notify hermes bridge service-run` through
  `resolve_bridge_config`, `adapter_command_plan_for`, and the local adapter
  service loop instead of returning `not_implemented`.
- Preserves Go preflight ordering: missing awiki Hermes secret, missing route
  secret, missing Python, and missing adapter script still fail before process
  start.
- Adds Unix SIGTERM/SIGINT handling for the hidden service-run command so a
  local foreground/service-manager run stops the adapter child before exiting.

Boundary note: this still does not adopt `kardianos/service` or a Rust platform
service-manager crate. It does not implement install/start/stop/restart/
uninstall through systemd/launchd/Windows SCM, real platform service status
lookup, lifecycle operation execution, owned HTTP health probing, Windows SCM
stop-control cleanup, non-dry-run setup bridge `Apply`, or final release archive
verification for the adapter asset.

Dependency note: no dependency was added. The Unix signal handling uses std FFI
to `signal(2)`, matching the existing repo preference for direct platform FFI
in narrow local helpers instead of adding a service/signal crate. The dependency
audit continues to show only existing allowed hits: Rustls/ring transport
dependencies, `base64`/`sha2`, and the approved
`rusqlite`/`libsqlite3-sys` bundled-SQLite toolchain entries
(`cc`, `pkg-config`, `vcpkg`), plus pre-existing transitive `libc` under ANP
crypto/random dependencies.

## 2026-05-16 Host-notify webhook helper script asset slice

Timestamp: 2026-05-16T16:53:27Z / 2026-05-17T00:53:27+0800.

Scope: copy the Go `scripts/host_notify_webhook_server.py` helper into the
Rust repository as a byte-identical local host-notify webhook/callback fan-out
asset. This follows the Go repository asset used by Hermes host-notify
architecture notes and local/manual testing without rewriting the helper in
Rust or wiring it into runtime listener/service-manager behavior.

Commands run:

```text
cp -p ../awiki-cli/scripts/host_notify_webhook_server.py scripts/host_notify_webhook_server.py
cmp -s ../awiki-cli/scripts/host_notify_webhook_server.py scripts/host_notify_webhook_server.py
stat -c '%a %n' scripts/host_notify_webhook_server.py
wc -l scripts/host_notify_webhook_server.py
python3 -m py_compile scripts/host_notify_webhook_server.py
cd ../awiki-cli && python3 -m py_compile scripts/host_notify_webhook_server.py
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64|libc'
```

Observed results:

- The copied `host_notify_webhook_server.py` file is byte-identical to the Go
  repository source.
- Permissions were preserved from the Go repository: `664`.
- The script is 535 lines, below the default 1200-line review-size cap, so no
  file-size exception is needed.
- Python compile checks passed in both the Rust and Go repositories.

Boundary note: this script is a dev/test helper asset. It does not implement
Rust runtime listener foreground dispatch, OpenClaw/Hermes sink delivery,
platform service-manager behavior, Hermes bridge orchestration, owned health
probing, or release archive verification.

Dependency note: no Rust dependency was added. Cargo manifests and lockfile
remain unchanged. The copied helper uses Python stdlib modules only, matching
the Go repository behavior and staying outside the Rust binary dependency
graph.

## 2026-05-16 README asset slice

Timestamp: 2026-05-16T17:09:07Z / 2026-05-17T01:09:07+0800.

Scope: copy the Go `README.md` public repository documentation into the Rust
repository as a byte-identical asset. This preserves the same project overview,
installation/onboarding commands, quick links, layout notes, config-template
pointer, and support guidance without rewriting documentation for Rust-specific
architecture.

Commands run:

```text
cp -p ../awiki-cli/README.md README.md
cmp -s ../awiki-cli/README.md README.md
stat -c '%a %n' README.md
wc -l README.md
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64|libc'
```

Observed results:

- `README.md` is byte-identical to the Go repository source.
- Permissions were preserved from the Go repository: `664`.
- The README is 65 lines, below the default 1200-line review-size cap, so no
  file-size exception is needed.

Boundary note: this is a documentation asset parity slice. It does not modify
Rust CLI behavior, command schema, config parsing, package install scripts, or
release packaging. README links to onboarding and architecture documents are
left unchanged for 1:1 parity; copying those linked documents remains a
separate docs parity lane.

Dependency note: no Rust dependency was added. Cargo manifests and lockfile
remain unchanged.

## 2026-05-16 Config template asset slice

Timestamp: 2026-05-16T17:02:08Z / 2026-05-17T01:02:08+0800.

Scope: copy the Go `config.template.yaml` public user-config template into the
Rust repository as a byte-identical asset. This preserves the documented
canonical `config.yaml` shape and defaults without changing Rust config parsing
or runtime behavior.

Commands run:

```text
cp -p ../awiki-cli/config.template.yaml config.template.yaml
cmp -s ../awiki-cli/config.template.yaml config.template.yaml
stat -c '%a %n' config.template.yaml
wc -l config.template.yaml
cargo +1.79.0 fmt --check
cargo +1.79.0 check -p awiki-cli --locked
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
tmp="$(mktemp -d)" && mkdir -p "$tmp" && cp config.template.yaml "$tmp/config.yaml" && AWIKI_CLI_UPDATE_CACHE_ONLY=1 AWIKI_CLI_WORKSPACE_HOME_DIR="$tmp" target/debug/awiki-cli config show --format json
cargo +1.79.0 run --bin xtask --locked -- check-structure
git diff --check
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|openssl-sys|openssl-probe|openssl-src|reqwest|hyper|rustls|webpki|aws-lc|ring|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|tungstenite|websocket|serde_yaml|yaml|hmac|sha2|base64|libc'
```

Observed results:

- `config.template.yaml` is byte-identical to the Go repository source.
- Permissions were preserved from the Go repository: `664`.
- The template is 35 lines.
- The copied template can be used as `<workspace>/config.yaml` by the current
  Rust CLI `config show --format json` path.

Boundary note: this is a repository asset parity slice. It does not implement
full Go `yaml.v3` parser/serializer parity, does not change init/config writer
behavior, and does not copy Go `Makefile`, release scripts, GitHub workflows,
or architecture docs; those remain separate tooling/docs lanes.

Dependency note: no Rust dependency was added. Cargo manifests and lockfile
remain unchanged.
