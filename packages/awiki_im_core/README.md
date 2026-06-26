# awiki_im_core

General-purpose Flutter/Dart SDK package for Awiki `im-core`, backed by the Rust facade crate `im-core-dart`.

This package is not an `awiki-me` adapter and intentionally exposes DTOs that follow `im-core` semantics rather than app UI/cache models.

Native support in v0.1 targets Android, iOS, macOS, and Linux. Flutter Web receives a stub that throws `UnsupportedError` at runtime.

Build Linux native artifacts on a Linux host before running a Flutter Linux app:

```bash
scripts/flutter/build-sdk-native.sh --linux-only
```

The command writes `packages/awiki_im_core/linux/lib/libawiki_im_core.so`.
That file is a generated native artifact and is not committed to git.

Realtime is exposed as transport-agnostic Dart streams:

- `AwikiImClient.events`
- `AwikiImClient.connectionStates`
- `AwikiImClient.realtime.start/stop/status/capability`

Apps should not depend on WebSocket URLs, raw frames, bearer headers, ping/pong, or reconnect internals.

Thread-level mark-read is exposed through the message API:

```dart
final result = await client.messages.markThreadRead(
  const ThreadRef.direct('did:example:bob'),
  maxMessageIds: 100,
);
```

`markThreadRead` delegates unread-id lookup to `im-core` local state. App code
must not page through `history()` just to discover unread message ids.
