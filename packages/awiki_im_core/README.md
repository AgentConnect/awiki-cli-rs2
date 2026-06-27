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

Local-first message history is exposed separately from remote history:

```dart
final page = await client.messages.localHistory(
  const ThreadRef.direct('did:example:bob'),
  limit: 50,
);
```

`localHistory` only reads the local `im-core` projection and is intended for
fast first paint. Use `history()` afterwards only when the app wants remote
reconcile/freshness.

Reliable message sync is exposed through high-level message APIs:

```dart
final delta = await client.messages.syncDelta(
  const SyncDeltaRequest(limit: 100, reason: 'app_resumed'),
);

final page = await client.messages.syncThreadAfter(
  const SyncThreadAfterRequest(
    thread: ThreadRef.direct('did:example:bob'),
    afterServerSeq: '991',
    limit: 100,
  ),
);
```

`syncDelta` lets Rust `im-core` read and advance the global reliable checkpoint
inside SQLite after events are applied. Dart callers can choose diagnostics such
as `limit`, `deviceId`, and `reason`, but cannot pass `since_event_seq` or store
the checkpoint. `syncThreadAfter` is thread-local and uses `afterServerSeq`; it
does not read or advance the global checkpoint.

Realtime events may include a readonly `RealtimeSyncHint`. Apps may use it to
schedule `syncDelta` after dirty/gap detection, but receiving realtime metadata
does not advance the reliable checkpoint.
