# Slice 10 Status: Realtime Runner Async Session

## Status

Implemented in-place for the `im-core` realtime async session path.

This increment does not introduce a parallel SDK or duplicate realtime service
layer. It keeps the existing realtime DTOs, wire projection semantics, reconnect
policy shape, heartbeat/status API, and legacy `RealtimeHandle` compatibility
path while adding a true async `RealtimeService::start_async` path inside the
existing modules.

## Async Public API Added

- `RealtimeService::start_async`
- `RealtimeSession`
- `RealtimeEventStream`

`RealtimeSession` provides:

- single-consumer event stream via `subscribe`
- status snapshot via `status`
- status watch via `status_updates`
- async `stop`
- async `join`
- deterministic drop behavior that requests shutdown

The sync `connect`, `run_until_shutdown`, and `RealtimeHandle` APIs remain as
staged legacy compatibility for CLI, Dart/FRB, and not-yet-migrated callers.
They must be removed, renamed, or cfg-gated in slice 13 after slices 11 and 12
migrate the upper layers.

## Async Runner

- Added `AsyncRealtimeRunnerTransport`.
- Added `AsyncRealtimeNotificationProjector`.
- Added `run_realtime_async_transport_until_shutdown`.
- Added Tokio-backed event/status handling:
  - bounded `tokio::sync::mpsc` event stream
  - `tokio::sync::watch` status updates
  - `tokio::sync::oneshot` exit result
  - async shutdown polling in the select loop
- `spawn_default_async` now runs on a Tokio task and uses the async runner loop
  instead of `std::thread::spawn`.
- Reconnect, connection state events, buffer-full behavior, closed-stream exit,
  shutdown exit, and warning accumulation preserve the existing runner
  semantics.

## Async WebSocket Transport

- Added `internal::realtime::async_ws_transport::AsyncWsTransport`.
- Uses `tokio-tungstenite` with rustls/webpki TLS.
- Preserves bearer authorization header construction.
- Supports configured custom CA bundles using the existing SDK `ca_bundle`
  setting.
- Preserves HTTP handshake status code for auth retry decisions instead of
  string-matching error messages.
- Handles text and binary JSON object messages, ping/pong, close frames, and
  async pings.

The production async path reads the auth token with async file I/O, connects the
WebSocket asynchronously, refreshes the auth session asynchronously after HTTP
401, rereads the token asynchronously, and retries once.

## Local Projection

The `RealtimeSession` async path projects local realtime updates through the
`LocalStateDb` actor:

- message records are stored through `LocalStateDb::store_messages`
- realtime-discovered contacts are stored through `LocalStateDb::upsert_contact`
- group touch/update records are stored through `LocalStateDb::upsert_group`

The legacy sync `RealtimeHandle` path still uses the old synchronous local
projection helper as compatibility. This is intentionally left for slice 13
cleanup together with the sync runner itself.

## Secure Notification Normalization

- The async session path uses `AsyncSecureRealtimeNotificationProjector`.
- Direct secure realtime notifications use
  `normalize_direct_e2ee_notification_with_async_processor_and_directory`
  directly from the async runner task.
- Group E2EE realtime notification normalization and group E2EE notice repair
  use their async helpers directly on the async path.
- Direct pending cipher replay cache in `AsyncDirectSecureIncomingProcessor`
  now uses a short-held `std::sync::Mutex` instead of `RefCell`, so the async
  realtime task future is `Send`. The mutex is not held across `.await`.

The sync realtime compatibility path still uses the staged async-first projector
with an internal current-thread runtime bridge where needed. That bridge remains
part of the legacy sync path and must be removed or cfg-gated in slice 13.

## Tests Added Or Updated

- `realtime_start_async_exposes_session_stream_and_keeps_validation`
- `realtime_session_allows_single_event_stream_and_stop_request`
- `realtime_async_runner_uses_tokio_channels_and_status_watch`
- `realtime_async_runner_stops_on_shutdown_signal`
- `realtime_async_local_state_projector_uses_db_actor_for_message_projection`
- `async_ws_http_error_preserves_status_code_for_auth_retry`

Existing realtime tests continue to cover:

- native sync connect compatibility
- endpoint derivation
- legacy 401 refresh simulation
- event DTO shape
- realtime frame contracts
- reconnect loop behavior
- projection behavior
- secure direct realtime normalization

## Dependency Compatibility

Added workspace dependencies:

- `tokio-tungstenite = 0.29` with `connect` and
  `rustls-tls-webpki-roots`
- `futures-util = 0.3`

The dependency check for `im-core` shows no `openssl`, `openssl-sys`, or
`native-tls` matches. WebSocket dependencies resolve through
`tokio-tungstenite`, `tungstenite`, `rustls`, `tokio-rustls`, and
`webpki-roots`.

## Temporary Boundaries

- CLI is not migrated in this slice. CLI async host migration remains slice 11.
- FRB/Dart/Flutter are not migrated in this slice. Async bridge migration
  remains slice 12.
- Sync `RealtimeHandle`, sync `connect`, sync `run_until_shutdown`,
  `std::thread::spawn` realtime worker, blocking native WebSocket transport,
  and sync local projection remain as staged legacy compatibility until
  slice 13.
- `start_async` is the transitional async entrypoint because `start` does not
  exist yet and the legacy sync public API still occupies the current realtime
  surface. Final naming and legacy removal remain slice 13 work.
- HTTP heartbeat/status APIs remain separate and available. End-to-end degraded
  realtime plus HTTP fallback behavior still needs system-level validation after
  CLI and Dart callers migrate.

## Validation

Passed:

```bash
cargo test -p im-core realtime_async_runner --locked
cargo test -p im-core realtime_session --locked
cargo test -p im-core realtime_start_async --locked
cargo test -p im-core async_ws_http_error_preserves_status_code_for_auth_retry --locked
cargo test -p im-core realtime_async_local_state_projector --locked
cargo test -p im-core realtime --locked
cargo test -p im-core async_direct_receive_processor_replays_pending_cipher_after_init --locked
cargo check -p im-core --locked
cargo check -p im-core --features group-e2ee --locked
cargo fmt --all -- --check
cargo tree -p im-core --locked | rg -i "openssl|openssl-sys|native-tls"
cargo tree -p im-core --locked | rg -i "tokio-tungstenite|tungstenite|rustls|webpki"
git diff --check
```

Notes:

- The `openssl|openssl-sys|native-tls` dependency check returned no matches.
- The rustls/webpki dependency check returned the expected rustls,
  tokio-rustls, tokio-tungstenite, tungstenite, and webpki-roots entries.
