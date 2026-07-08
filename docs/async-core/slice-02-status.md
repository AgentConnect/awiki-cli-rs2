# Slice 02 Status: Async HTTP and Transport

## Status

In progress with the async transport foundation implemented.

This slice now has a true async HTTP path based on `reqwest` with rustls TLS and
internal async transport traits/impls. The existing sync transport path remains
as a staged legacy compatibility path for business services that have not yet
been migrated to async public APIs.

## Implementation

- Added workspace `reqwest` with `default-features = false` and rustls TLS.
- Added `reqwest` to `crates/im-core`.
- Added `HttpClient::execute_async` using `reqwest::Client`.
- Preserved `HttpClient::execute` as a marked legacy sync path until service and
  runtime callers are migrated.
- Added internal async transport traits:
  - `AsyncAuthenticatedRpcTransport`
  - `AsyncAttachmentObjectTransport`
  - `AsyncRawJsonTransport`
  - `AsyncRpcTransport`
  - `AsyncRestTransport`
  - `AsyncAuthenticatedRestTransport`
- Added async implementations for:
  - `CoreHttpTransport`
  - `CorePlainTransport`
  - `UnavailableTransport`
  - `&mut T` for authenticated RPC transport forwarding
- Preserved endpoint routing, JWT capture/persist, 401 retry/challenge handling,
  JSON-RPC envelope construction, and error mapping in the async path.
- Added JSON-RPC envelope golden tests for:
  - direct send
  - group send
  - inbox
  - history
  - mark read
  - auth refresh
- Added async unavailable transport tests to confirm async trait error shape
  matches the legacy sync transport shape.

## Validation

Passed:

```bash
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo check --workspace --locked
cargo test -p im-core transport --locked
cargo test -p im-core json_rpc --locked
cargo tree -p im-core --locked | rg -i "openssl|openssl-sys|native-tls"; test ${PIPESTATUS[1]} -eq 1
```

Dependency check summary:

```text
reqwest v0.12.28
hyper v1.9.0
hyper-rustls v0.27.9
tokio-rustls v0.26.4
rustls v0.23.40
webpki-roots
```

No `openssl`, `openssl-sys`, or `native-tls` dependency was found for
`im-core`.

## Grep Fence

Command:

```bash
rg -n "std::net::TcpStream|StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
```

Current output:

```text
crates/im-core/src/internal/attachment_runtime/atomic_write.rs: std::io::Write
crates/im-core/src/internal/http.rs: StreamOwned legacy sync HTTP path
crates/im-core/src/internal/realtime/ws_transport.rs: StreamOwned legacy realtime WebSocket path
crates/im-core/src/internal/secure_direct/file_runtime.rs: std::io::Write
```

Notes:

- `internal/http.rs` still contains the legacy sync `execute` path because
  business service public APIs have not yet moved to async. It is marked in code
  and must be removed or restricted to test-only in slice 13.
- `internal/realtime/ws_transport.rs` remains blocking until slice 10.
- `atomic_write.rs` and `secure_direct/file_runtime.rs` are file I/O paths and
  will be handled in later filesystem/E2EE slices where applicable.

## Remaining Work

- Migrate business service/runtime callers from legacy sync transport traits to
  async transport traits in slices 03, 05, 06, 07, 08, 09, and 10.
- Replace attachment object transfer `Vec<u8>` body/response with streaming in
  slice 08.
- Remove or cfg-gate legacy sync HTTP and transport traits in slice 13.
