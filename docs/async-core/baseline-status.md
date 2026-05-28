# Async Core Baseline Status

> Recorded before code changes for the async-core cutover.

## Test Baseline

| Command | Result | Notes |
|---|---|---|
| `cargo test -p im-core --locked` | Failed | 1 existing failure: `secure_service_api_shape_is_available_from_client` in `crates/im-core/tests/secure_api.rs`; expected `Unavailable`, got `Unknown`. |
| `cargo test -p awiki-cli --locked` | Failed | 1 existing failure: `group_e2ee_internal_live_commands_stay_unsupported` in `crates/awiki-cli/tests/group_e2ee_cutover_policy_contract.rs`; expected `unsupported_capability`, got `identity_required`. |
| `cargo check -p im-core-dart --locked` | Passed | Completed successfully. |
| `cargo check --workspace --locked` | Passed | Completed successfully. |

These failures are treated as baseline issues for slice 00. They are not introduced by the async runtime foundation work.

## Blocking I/O Baseline

Current grep findings before async cutover:

```bash
rg "std::net::TcpStream|std::thread::spawn|std::sync::mpsc" crates/im-core/src
```

Findings:

```text
crates/im-core/src/realtime/handle.rs: std::sync::mpsc
crates/im-core/src/realtime/runner.rs: std::sync::mpsc
crates/im-core/src/internal/realtime/transport.rs: std::sync::mpsc
```

```bash
rg "StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
```

Representative findings:

```text
crates/im-core/src/internal/http.rs: rustls::StreamOwned over std socket
crates/im-core/src/internal/realtime/ws_transport.rs: rustls::StreamOwned over std socket
crates/im-core/src/internal/attachment_runtime/atomic_write.rs: std::io::Write
crates/im-core/src/internal/secure_direct/file_runtime.rs: std::io::Write
```

`std::io::Write` in local file writers may become an allowed worker/file-I/O exception later, but HTTP/WebSocket socket I/O must be replaced by async transports.

```bash
rg "std::fs::read|std::fs::write|std::fs::File" crates/im-core/src
```

Representative findings:

```text
message direct/group credential loading
attachment upload/download local file handling
transport JWT auth state persistence
auth session state reading
secure direct credential/session file paths
identity/group E2EE local state helpers
```

```bash
rg "rusqlite::Connection|Connection::open|open_writable" crates/im-core/src
```

Representative findings:

```text
message local projection
message mark_read/conversations
contact store
group cache/projection
secure direct send/incoming/status/prepare
realtime runner projection
directory service contact cache
core bootstrap local state migration
```

These SQLite findings are expected before slice 04 and should be reduced to `internal/local_state/**` plus tests by the final cutover.

## Slice 00 Acceptance

```text
1. Total plan and slice documents exist.
2. Baseline test results are recorded.
3. Blocking I/O grep baseline is recorded.
4. No production code behavior was changed in slice 00.
```
