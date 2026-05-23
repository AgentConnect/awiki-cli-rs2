# awiki_im_core

General-purpose Flutter/Dart SDK package for Awiki `im-core`, backed by the Rust facade crate `im-core-dart`.

This package is not an `awiki-me` adapter and intentionally exposes DTOs that follow `im-core` semantics rather than app UI/cache models.

Native support in v0.1 targets Android, iOS, and macOS. Flutter Web receives a stub that throws `UnsupportedError` at runtime.

Realtime is exposed as transport-agnostic Dart streams:

- `AwikiImClient.events`
- `AwikiImClient.connectionStates`
- `AwikiImClient.realtime.start/stop/status/capability`

Apps should not depend on WebSocket URLs, raw frames, bearer headers, ping/pong, or reconnect internals.
