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

Conversation list startup and updates are exposed as local projection helpers:

```dart
final firstPage = await client.messages.conversations(limit: 100);
final secondPage = firstPage.hasMore && firstPage.nextCursor != null
    ? await client.messages.conversations(
        limit: 100,
        cursor: firstPage.nextCursor,
      )
    : null;
final snapshot = await client.messages.loadConversationSnapshot();
await client.messages.clearConversationSnapshot();
final patches = client.messages.watchConversationPatches();
final repair = await client.messages.repairConversationStore();
final threadPatches = client.messages.watchThreadPatches(
  const ThreadRef.direct('did:example:bob'),
  limit: 100,
);
final threadRepair = await client.messages.repairThreadStore(
  const ThreadRef.direct('did:example:bob'),
  limit: 100,
);
```

These APIs currently live under `client.messages` for SDK compatibility. The
conversation page is backed by committed local `conversation_summaries` and is
ordered newest-first by `last_message_at DESC, conversation_id DESC`. A single
page is capped at 100 items; load 500/1000 conversation lists by passing the
opaque `nextCursor` back as `cursor` until `hasMore` is false or `nextCursor` is
null. Do not parse the cursor or treat it as an offset. The snapshot is a
non-authoritative redb cache generated from committed `conversation_summaries`;
`clearConversationSnapshot` only clears that discardable cache for the current
owner. Conversation and thread patch streams
emit versioned reset/upsert/remove/reorder/repair events only after the local
projection commit succeeds, and `repairConversationStore` / `repairThreadStore`
provide reset/repair baselines after lag, stream close, or version gaps. DTOs
are core-only and must not include `awiki-me` App presentation fields such as
`hidden`, `pinned`, `muted`, `customTitle`, `avatarSeed`, `peerLifecycleState`,
`ConversationSummary`, or `ChatMessage`.

Thread-level mark-read is exposed through a watermark-first message API:

```dart
final result = await client.messages.markThreadRead(
  const ThreadRef.direct('did:example:bob'),
  watermark: const ReadWatermark(lastReadThreadSeq: '991'),
  fallbackMaxMessageIds: 100,
);
```

`watermark` is optional. If omitted, `im-core` computes the highest visible
committed thread watermark from local projection. Direct uses thread-local
`server_seq`; group uses the group thread-local `server_seq` projection backed by
`group_event_seq`. Neither is the global reliable sync `event_seq`.

`markThreadRead` first uses the service `read_state.mark_read` contract. Old
services fall back to local unread-id lookup and legacy direct
`inbox.mark_read(message_ids)`; group fallback is local/pending only. App code
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
