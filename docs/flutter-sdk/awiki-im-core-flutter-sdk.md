# Awiki IM Core Flutter SDK

`packages/awiki_im_core` is a general-purpose Flutter/Dart SDK for `crates/im-core`. It is not an `awiki-me` adapter and must not expose app UI/cache DTOs such as `ChatMessage` or `ConversationSummary`.

## Layers

```text
Flutter app
  -> packages/awiki_im_core (Dart API, platform loader, generated FRB Dart glue)
  -> crates/im-core-dart (Rust-Dart facade, DTO mapping, lifecycle)
  -> crates/im-core (pure Rust IM SDK)
```

`crates/im-core` remains pure Rust. Flutter, Dart, FFI, codegen, and platform packaging belong only in the Dart package and `im-core-dart` facade.

## Supported platforms

v0.1 targets native Flutter on Android, iOS, macOS, and Linux:

- Android: `arm64-v8a`, `x86_64`, optional `armeabi-v7a`.
- iOS: device and simulator static-library XCFramework slices.
- macOS: `aarch64`/`x86_64` XCFramework slices.
- Linux: `x86_64-unknown-linux-gnu` shared library bundled through the Flutter Linux FFI plugin.

Windows is not declared as a v0.1 Flutter plugin platform. Flutter Web has a stub so dependent apps can analyze; calling `AwikiImCore.open` on Web throws `UnsupportedError` because `dart:ffi` cannot load the native Rust backend there.

## Lifecycle and blocking model

The Rust facade exposes opaque `DartImCore` and `DartImClient` objects. Each object has an explicit close/dispose path. After close, Rust calls return `DartImError` with code `object_closed`; the Dart wrapper mirrors this with `AwikiImCoreException`.

`im-core` is blocking-first. Public Dart APIs are `Future<T>` and must not expose synchronous IO, SQLite, or HTTP calls into widget build paths. FRB generated calls should use the worker-thread model and the wrapper must keep App-facing methods async.

### Local-state upgrade before Core open

Applications upgrading from release/0710 must inspect and, when required, run
the canonical local-state upgrade before `AwikiImCore.open`:

```dart
final inspection = await AwikiImCore.inspectLocalStateUpgrade(paths: paths);
if (inspection.eligibility == LocalStateUpgradeEligibility.required) {
  final result = await AwikiImCore.upgradeLocalState(paths: paths);
  assert(result.status == LocalStateUpgradeStatus.completed);
}
final core = await AwikiImCore.open(config: config, paths: paths);
```

Inspection is read-only. Upgrade performs the Core-owned cross-process lock,
SQLite online backup (including committed WAL state), shadow migration,
conservation/invariant validation, and cutover. A missing SQLite file is a fresh
install and returns `notRequired` without creating files. Ordinary Core open
continues to fail closed with `local_state_upgrade_required`; hosts must not
delete, archive, or recreate the database to bypass this gate. Public reports
contain schema versions, aggregate counts, and owner-scoped legacy-to-canonical
conversation mappings required by the App overlay migration. They never expose
backup paths or message content. The mapping remains available after cutover so
an App crash between the Core and overlay journals can resume idempotently.

If a canary must be downgraded after cutover, dispose Core first and restore
the complete verified release/0710 backup with the new SDK/tooling:

```dart
final restored = await AwikiImCore.restoreLocalStateBackup(paths: paths);
assert(restored.restoredSchemaVersion == 27);
assert(restored.targetSafetyCopyAvailable);
```

Restore is not an in-process rollback for an open Core. It only accepts a
completed canonical-upgrade journal, keeps the schema 28 target as a private
safety copy, and is idempotent across interruption. The public result exposes
versions/availability only, never filesystem backup paths.

## DTO policy

Facade DTOs follow `im-core` public DTO semantics and use Dart-friendly primitives at the boundary. Time values remain ISO-8601 strings. The Dart wrapper may add convenience getters such as `AuthStatus.authenticated`, but it must not rename Rust DTO semantics such as `has_session` into a different facade meaning.

## Identity registration and recovery

The SDK exposes `registerHandleWithPhone`, `registerHandleWithEmail`, and `recoverHandle` on `AwikiImCore`. These calls are core-level identity registry operations that map to `im-core` public identity DTOs; they do not depend on any `awiki-me` account gateway or UI model.

`recoverHandle` always requests the same canonical local-finalize behavior as the CLI. A
successful recovery of an existing full Handle rotates its DID without changing the stable local
identity ID, records old/current DID history, refreshes owner-DID snapshots, and enqueues any
Handle-backed group member rebind work. Dart hosts must not persist the returned DID as a new local
identity owner or supply a separate generated identity.

## Identity secret storage

Native hosts can open `AwikiImCore` with explicit identity SecretVault options:

```dart
final core = await AwikiImCore.open(
  config: config,
  paths: paths,
  openOptions: AwikiImCoreOpenOptions.vaultRequired(
    identitySecretVault: ImCoreSecretVaultOptions(
      rootKey: DeviceVaultRootKey.fromList(rootKeyBytes),
      vaultDir: vaultDir,
      workspaceId: workspaceId,
      deviceId: deviceId,
    ),
  ),
);
```

`AwikiImCoreOpenOptions.fileCompat()` keeps the compatibility default. Use
`vaultPreferred` only as a migration-period mode; production App safety should
use `vaultRequired` and fail closed when the host cannot provide the root key or
matching vault context.

The Dart SDK does not generate, persist, rotate, or back up the host root key.
The host must get it from its own no-prompt secure storage path and pass it only
for `open`. `DeviceVaultRootKey.toString()` is redacted, and App code must not
log generated DTOs or `openOptions` that could contain root key bytes. The SDK
does not expose generic `open_secret()`, private key, or raw `SecretRef` APIs.
Auth APIs may still return `bearerToken` in session DTOs for existing flows;
callers must treat it as sensitive and must not log or persist it outside the
SDK-owned auth/session storage path.

Identity vault diagnostics are exposed as narrow facade methods:

```dart
final status = await core.identityVaultStatus(selector);
final migration = await core.migrateIdentityVault(selector);
final verification = await core.verifyIdentityVault(selector);
```

Status/migration/verification DTOs report backend, metadata, warnings, and
plaintext compatibility retention only. They do not expose root key material,
JWTs, bearer tokens, or secret refs. Flutter Web remains a stub and cannot run
the native vault-backed backend.

Native hosts can read the current identity's safe device projection without
opening any private key:

```dart
final device = await core.identityDeviceSummary(selector);
```

The result distinguishes legacy/member/admin readiness and exposes only the
protocol device ID and public key IDs. It intentionally omits Vault references,
root-key presence flags, and internal Document/Registry/auth checkpoints.

## Directory profile metadata

`client.directory.resolvePeer(handle)` and `client.directory.lookupHandle(handle)` return a
`DirectoryResolution.conversationId` together with the resolved DID/Handle. When the directory
provides a stable user ID and full Handle, this is the canonical `dm:peer-scope:v1:*` identity and
must be used by App start-conversation flows before the first message arrives. The same result can
also carry `DirectoryResolution.profile` populated from the WNS Handle Resolution Document
`profile` object. This profile is a DID Subject Profile projection, not routing or security metadata.

The Dart `UserProfile` model uses these standard display fields:

- `displayName`
- `avatarUri`
- `profileUri`
- `description`
- `subjectType`
- `versionId`
- `ttl`

Legacy compatibility fields remain available:

- `bio` maps to / from `description` where needed.
- `avatarUrl` maps to / from `avatarUri` where needed.
- Older service inputs such as `nick_name`, `name`, `avatar_url`, and `avatar` are normalized by `im-core`.

Display fields must not be used for routing, authentication, authorization, service endpoint selection, E2EE binding, or security-profile negotiation. Apps should keep Handle or DID visible on profile and recipient-confirmation surfaces, especially for high-risk operations.

`client.directory.hydrateDisplayProfiles(peers)` reads only the local `im-core` contact/profile cache. It does not call WNS or User Service, and is intended for hot UI paths such as conversation lists, contact lists, and member lists. A returned `DisplayProfile` has `cacheHit = false` when the peer is absent locally; the app should fall back to `displayName -> handle -> did` without blocking list rendering. Remote refresh must be explicit through `resolvePeer`, `lookupHandle`, `loadPublicProfile`, or the send-time security verification path.

`client.directory.relationStatus(peer)` is the authenticated remote relationship status query despite its retained Dart method name. `RelationStatus` exposes `isFollowing`, `isFollower`, `isFriend`, `isBlocked`, `isBlockedBy`, `isContact`, and `messaged` independently. Its nullable `relationship` field is only the caller's outbound local projection (`following` or `none`) and must not be treated as the combined relationship state. A consumer derives `friend` only when both directional flags and `isFriend` agree; missing or contradictory directional truth fails closed.

## Group display metadata

`CreateGroupRequest.avatarUri` maps to `group_profile.avatar_uri`; `CreateGroupRequest.name` remains the Flutter convenience input for `group_profile.display_name`. `GroupSummary` and `GroupSnapshot` expose `displayName` and `avatarUri`; the old `name` field is retained as a compatibility projection of `displayName`.

Group creation service DID is resolved from `AwikiImCoreConfig.anpServiceDid`. If it is absent, group create returns `invalid_input(field = anp_service_did)`.

These display fields are UI metadata only. They must not be used for routing, authorization, membership checks, E2EE binding, or service endpoint selection.

## Message retry

`retryMessage` is explicitly unsupported in v0.1 and returns `unsupported_capability("message-retry")`. The SDK must not rebuild a send request from display message DTOs because those DTOs can lose target, body, security, idempotency, and retry-plan information.

## Local-first message reads

`client.messages.conversations(...)` returns durable local conversations from `im-core`. Schema version 27 reads conversation existence from `conversation_registry` and left-joins the message-derived `conversation_summaries`, so a validated empty conversation remains visible. Protocol/control records (including group lifecycle records) do not materialize a message summary; until the first user-visible message, `messageCount` remains `0` and `lastMessage` remains `null`. The API is paged: pass `cursor: page.nextCursor` to continue, and stop when `hasMore` is false or `nextCursor` is null. A single page is capped at 100 items by `PageLimit::new`. The cursor is opaque and follows `activity_at DESC, conversation_id DESC`; callers must not parse it or treat it as an offset.

Before opening a newly resolved Direct conversation or a newly created/joined Group conversation, commit its existence:

```dart
await client.messages.ensureConversation(canonicalConversationId);
```

The call is idempotent and fail-closed: Direct requires an owner-scoped
peer-scope route bound to a verified `peerPersonaId`; Group requires an active
local membership addressed by canonical Group DID. App-local rows may be used
only as a temporary optimistic overlay while this call completes.

Generated `DartConversation` and `DartConversationSnapshotItem` now expose a
required `conversationId`, optional `peerPersonaId` / `canonicalGroupDid`, and a
required `resolutionState`. A resolved Direct must have `peerPersonaId`; a
resolved Group must have `canonicalGroupDid`. New App code must not fall back to
`threadId` when any of these canonical facts is missing. `DartGroupMember`
separates `membershipId`, `peerPersonaId`, and `credentialDid`; the credential
DID is not the membership identity. Conversation and snapshot projections may
carry an optional `title`; for Group rows this is the committed local Group
profile display name, not an App-local custom title.

Conversation list startup and realtime updates use snapshot / patch helpers under
the same `client.messages` namespace:

```dart
final snapshot = await client.messages.loadConversationSnapshot();
await client.messages.clearConversationSnapshot();
final patches = client.messages.watchConversationPatches();
final repair = await client.messages.repairConversationStore();
final timelinePatches = client.messages.watchConversationTimelinePatches(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  limit: 100,
);
final timelineRepair = await client.messages.repairConversationTimelineStore(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  limit: 100,
);
```

`loadConversationSnapshot` reads a non-authoritative redb snapshot generated from
committed `conversation_summaries`. `clearConversationSnapshot` only clears that
discardable snapshot cache for the current owner; it does not clear SQLite local
projection, runtime store, read state, or reliable checkpoint. `watchConversationPatches` streams versioned
`ConversationStorePatch` values (`reset`, `upsert`, `remove`, `reorder`,
`repairRequired`) emitted only after the underlying local projection commit
succeeds. The conversation store is keyed only by canonical `conversationId`;
`remove` and `reorder` carry that ID instead of thread kind/id or a legacy alias.
Snapshot format v3 invalidates older discardable redb snapshots and rebuilds them
from SQLite so Group first paint retains the committed profile title across a
restart. `repairConversationStore` returns a reset/repair patch after lag,
overflow, stream close, or version gaps. `watchConversationTimelinePatches` and
`repairConversationTimelineStore` expose the same committed-projection rule for an
opened conversation timeline keyed by `ConversationReadRef.conversationId`;
remote history best-effort pages or realtime hints must not become authoritative
timeline patches before persistence succeeds. `watchThreadPatches(ThreadRef)` and
`repairThreadStore(ThreadRef)` remain compatibility adapters for CLI/legacy paths,
not the AWiki Me display-chain owner. A
realtime incoming message becomes patch-visible only after `im-core` has
committed its SQLite local projection; failed or skipped realtime projection
does not emit an authoritative conversation/thread patch.

Remote history, conversation catch-up, and realtime incoming messages share one
Core canonical-ingress gate. A Direct wire DID must resolve to a verified
Persona before the message row is committed. Until then Core stores the record
in its durable resolution backlog and exposes neither a `dm:<DID>` conversation
nor a timeline patch; verified Persona projection later replays it under the
single canonical conversation ID.

For an online first inbound Direct, Core resolves the wire peer DID through the
Handle authority and commits that verified Persona projection before the
message. A missing, conflicting, malformed, or DID-mismatched lookup remains in
the durable backlog and emits no authoritative patch; the SDK never synthesizes
a Persona from the DID itself.

These APIs currently live under `client.messages` for SDK compatibility. If a
future `client.conversations` namespace is added, it must wrap the same core
store and DTOs rather than introducing another source of truth.

Conversation snapshot / store DTOs are core-only. They must not carry
`awiki-me` App-only fields such as `hidden`, `pinned`, `muted`, `customTitle`,
`avatarSeed`, `peerLifecycleState`, `ConversationSummary`, or `ChatMessage`.
AWiki Me applies those presentation fields in its own application layer; see
`awiki-me/docs/conversation-presentation-ownership.md`.

`client.messages.localConversationTimeline(conversation, limit: ..., cursor: ...)`
is the App timeline first-paint API. It reads the committed local projection by
canonical `conversationId` and does not call remote history. `localHistory(thread, ...)`
and `history(thread, ...)` remain migration adapters:

- `localConversationTimeline` / `localHistory` read only the local SQLite projection and return an opaque local cursor;
- it does not call message-service history RPCs, directory lookup, or remote E2EE projection;
- it is the correct API for chat first paint before background reconcile;
- `history` keeps remote history + projection/reconcile semantics and should be called in the background when freshness is required.

Both APIs are async `Future<MessagePage>` methods. Apps must not bypass the SDK or read SQLite directly.

## Conversation send and local echo

Conversation UI sends should use the conversationId-first APIs when the App already has a selected conversation:

```dart
final sent = await client.messages.sendConversationText(
  const SendConversationTextRequest(
    conversation: ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    text: 'hello',
  ),
);

final payload = await client.messages.sendConversationPayload(
  const SendConversationPayloadRequest(
    conversation: ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    payloadJson: '{"text":"@agents summarize","mentions":[]}',
  ),
);
```

`im-core` resolves the conversation to the storage route, persists a pending local
projection row, emits patches after the local transaction succeeds, and updates
the same durable row to accepted/sent/failed after network send. Apps render
`Message.metadata.sendState` / retry data from the SDK message DTO. They must not
create a second durable optimistic message store for text or payload sends.

Conversation UI attachment sends use the attachment namespace with the same
conversation identity rule:

```dart
final attachment = await client.attachments.sendConversation(
  SendConversationAttachmentRequest(
    conversation: const ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    input: const AttachmentInput.bytes(
      filename: 'note.txt',
      mimeType: 'text/plain',
      bytes: [104, 101, 108, 108, 111],
    ),
    caption: 'hello',
    clientMessageId: 'msg-app-attachment-001',
    idempotencyKey: 'op-msg-app-attachment-001',
  ),
);
```

`client.attachments.sendConversation(...)` resolves the canonical
`ConversationReadRef.conversationId` inside `im-core`, writes the durable message
projection, and returns the same `AttachmentSendResult` shape as the legacy
target API. AWiki Me conversation UI should use this API for initial attachment
sends and retries. Local file previews may be rendered as transient UI state
while upload is in progress, but list/detail/send correctness must come from the
SDK projection and patch stream.

## Conversation read watermark

Conversation-level mark-read is exposed as a watermark-first message API:

```dart
final result = await client.messages.markConversationRead(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  watermark: const ReadWatermark(
    lastReadThreadSeq: '991',
    lastReadMessageId: 'msg_direct_991',
  ),
  fallbackMaxMessageIds: 500,
);
```

`watermark` is optional. When the App omits it, the SDK computes the highest
visible committed thread watermark from `im-core` local projection / thread
store. App code must not page through `history()`, read SQLite, or collect
unread message ids just to clear unread state.

Watermark semantics:

- direct threads use direct message `server_seq`;
- group threads use the group thread-local `server_seq` projection, which may be
  backed by `group_event_seq` on the service side;
- neither value is the account-level reliable sync `event_seq`;
- `lastReadMessageId` is for idempotency and diagnostics, not the ordering source;
- `readAt` is an audit/display timestamp and does not participate in
  authorization or checkpoint logic.

The SDK first uses the service `read_state.mark_read` contract. When the service
does not support the endpoint, the SDK falls back to local unread-id lookup and
legacy direct `inbox.mark_read(message_ids)`. Group fallback on an old service is
local/pending only. Results must expose `updatedCount`, `remoteAcknowledged`,
`partial`, `fallbackUsed`, `pendingRemoteAck`, and `warnings`; any returned
message ids are legacy fallback diagnostics only.

`markConversationRead` does not expose or advance the reliable sync checkpoint.
`remoteAcknowledged` and `pendingRemoteAck` describe only read-ack state.
`markThreadRead(ThreadRef)` remains available as a CLI/legacy adapter, but AWiki Me
must route visible conversations by `ConversationReadRef.conversationId`.

## Reliable message sync

Reliable sync is exposed as high-level async message APIs. The Dart SDK must not expose
SQLite, WebSocket frames, `since_event_seq`, `next_event_seq`, or checkpoint
load/store primitives.

Expected public shape:

```dart
final delta = await client.messages.syncDelta(
  const SyncDeltaRequest(
    limit: 100,
    reason: 'app_resumed',
  ),
);

final page = await client.messages.syncConversationAfter(
  SyncConversationAfterRequest(
    conversation: const ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    afterServerSeq: '991',
    limit: 100,
  ),
);
```

`syncDelta` semantics:

- Rust `im-core` reads the current global message checkpoint from local SQLite.
- Rust `im-core` sends `sync.delta` to the home message service and injects
  `since_event_seq` internally.
- Rust `im-core` applies all returned events and advances the checkpoint only after
  the local apply transaction succeeds.
- If the service returns `snapshot_required=true`, the SDK returns a failed-closed
  result: no checkpoint advance, no local projection wipe, and diagnostic fields for
  the App to surface a degraded sync state.
- Dart callers can choose `limit` and `reason`; they cannot choose or store the
  reliable checkpoint.

`syncConversationAfter` semantics:

- It is a thread-local freshness API for direct/group chat surfaces.
- `afterServerSeq` is a thread-local message sequence, not the account-level
  `event_seq`.
- It does not read or advance the reliable global checkpoint.
- The returned page must contain only `server_seq > afterServerSeq` messages in
  ascending `server_seq` order.
- Returned messages are not UI truth until `im-core` persists them to the local
  projection and the App reloads/repairs through the conversation timeline.
`syncThreadAfter(ThreadRef)` remains a compatibility wrapper and should not be the
AWiki Me display-chain routing owner.

Realtime integration:

- Realtime events may include a readonly `RealtimeSyncHint` with `eventId`,
  `eventSeq`, and `eventType`.
- App code may use the hint to schedule `syncDelta` after duplicate/gap/dirty
  detection.
- Receiving a realtime hint or successfully projecting a realtime notification must
  not advance the reliable checkpoint.
- Successfully projecting a realtime incoming message to local SQLite does emit
  committed conversation/thread patches for active subscribers; the hint alone
  is never an authoritative patch source.
- Before reliable sync supplies a thread-local `serverSequence`, an incoming
  realtime projection uses the recipient-side receive timestamp rather than the
  sender-provided `sentAt`. Once two timeline rows both have `serverSequence`,
  consumers must order them by that sequence and use time only as the fallback
  for mixed or legacy rows.

The SDK must not add public APIs named `loadGlobalCheckpoint`, `storeGlobalCheckpoint`,
`setGlobalCheckpoint`, or equivalents. Any checkpoint inspection needed for debugging
must stay behind internal diagnostics or test-only interfaces.

## Message payloads and ANP P9 mentions

`SendPayloadRequest.payloadJson` accepts any JSON object up to the SDK payload
size limit. It no longer requires a top-level `schema` field. Existing
`awiki.agent.*` control payloads may continue to include `schema`, but App code
must not add a fake `schema`/`protocol` field merely to send ANP-P9 mention
payloads.

The Dart SDK exposes ANP-P9 mention DTO helpers:

```dart
final payload = MessageMentionPayload(
  text: '@agents please summarize this discussion.',
  mentions: const [
    MessageMention(
      id: 'men_1',
      range: MessageMentionRange(start: 0, end: 7),
      target: MessageMentionTarget.groupSelector(MessageMentionSelector.agents),
    ),
  ],
);

validateMessageMentionPayloadJson(payload.toPayloadJson());
await client.messages.sendPayload(
  SendPayloadRequest(
    target: const MessageTarget.group('did:wba:example.com:group:team'),
    security: MessageSecurityMode.defaultPlain,
    payloadJson: payload.toPayloadJson(),
  ),
);
```

P9 mention DTOs intentionally do not add sender, proof, profile, content type, or
selector expansion fields. Single-target identity is the target DID; optional
`displayName` is only a UI snapshot and must not be used for routing,
authentication, authorization, E2EE binding, or runtime policy decisions.

Flutter/App integration gates:

```bash
cd packages/awiki_im_core && flutter test test/message_payload_api_test.dart
cd ../.. && scripts/flutter/codegen-check.sh
```

AWiki Me adds App-level composer, mapper, and highlight coverage in
`awiki-me` focused tests. The desktop App + CLI peer group scenario sends a
schema-less `@agents` P9 payload through `MessagingService.sendMentionText` and
verifies the projected payload text can be read back by both the App and CLI
history. Daemon prompt execution is validated separately by
`cargo test -p awiki-deamon --locked mention` and the dedicated Agent IM /
daemon integration gate.

## Realtime ownership

The native SDK exposes realtime as a high-level session and event stream:

```dart
final capability = await client.realtime.capability();
if (capability.runnerExposed) {
  final session = await client.realtime.start();
  final eventsSub = client.events.listen((event) {
    // MessageReceived / GroupUpdated / HostNotification / connection state, etc.
  });
  final stateSub = client.connectionStates.listen((state) {
    // connected / reconnecting / closed, without transport details.
  });

  await session.stop();
  await eventsSub.cancel();
  await stateSub.cancel();
}
```

WebSocket remains an `im-core` internal transport concern. Transport details such as WebSocket URLs, raw frames, ping/pong, request IDs, bearer headers, and dispatch queues are internal to `im-core` and must not become Dart public API. App code should configure only `AwikiImCoreConfig.transportPolicy` and consume `client.events` / `client.connectionStates`.

Flutter Web still receives a stub and does not support native realtime.

## Attachments And E2EE

`client.attachments.sendConversation(SendConversationAttachmentRequest(...))` is the conversationId-first attachment send API for apps that already have a selected conversation. `client.attachments.send(AttachmentSendRequest(...))` remains a high-level target-first compatibility facade for CLI, daemon, legacy callers, or surfaces that do not yet hold a canonical `ConversationReadRef`. `AttachmentSendRequest.security` defaults to `MessageSecurityMode.defaultPlain`; callers can set `MessageSecurityMode.e2eeRequired` for direct or group E2EE attachment messages.

Plain attachment messages use `application/anp-attachment-manifest+json` with a JSON
manifest payload. Realtime, read, sync, conversation snapshots, and local
projection paths must expose that manifest as `MessageBodyView.payload`, while
also attaching the attachment summary and download action when available. The
manifest content type is not an unsupported body type.

Secure attachment sends do not expose P7 control-plane calls, download tickets, object keys, nonces, raw ciphertext, secure session state, or MLS provider paths to Dart. `AttachmentSendResult.manifestJson` is the public redacted manifest projection. For E2EE attachments it may include `encryption_info.mode = object-e2ee`, `object_cipher`, and `plaintext_size`, but must not contain `object_key_b64u` or `nonce_b64u`.

`UploadedAttachment.sizeBytes` / `size` describe the uploaded object bytes. For `object-e2ee` this is ciphertext size. `UploadedAttachment.plaintextSizeBytes` carries the original plaintext size when available.

## Codegen

Generated files are committed so the package can be checked out and analyzed without requiring codegen first:

- `crates/im-core-dart/src/frb_generated.rs`
- `packages/awiki_im_core/lib/src/generated/bridge_generated.dart`

Run:

```bash
scripts/flutter/codegen-check.sh
```

`codegen-check.sh` runs the bridge generator and fails if the committed generated Rust/Dart files are not already in sync. If `flutter_rust_bridge_codegen` CLI flags change, update this script but keep the same input/output paths.

## Build commands

Rebuild all native SDK artifacts after Rust SDK changes:

```bash
scripts/flutter/build-sdk-native.sh
```

The one-step script runs:

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-apple.sh
scripts/flutter/build-android.sh
```

Linux native artifacts are host-specific and are built explicitly on Linux.
Single-platform builds remain available:

```bash
scripts/flutter/build-sdk-native.sh --macos-only
scripts/flutter/build-sdk-native.sh --ios-only
scripts/flutter/build-sdk-native.sh --android-only
scripts/flutter/build-sdk-native.sh --linux-only
```

Full Android builds require `cargo-ndk`. Full Apple builds must run on macOS with Xcode and Rust Apple targets installed. Linux native builds must run on Linux with the Flutter Linux desktop prerequisites available in the consuming app, such as `clang`, `cmake`, `ninja-build`, `pkg-config`, GTK development headers, and Xvfb for headless integration tests. Use `--dry-run` to print the selected build steps without compiling native artifacts.

The iOS XCFramework targets iOS 13+ for physical devices and x86_64 simulators. The arm64 simulator slice targets iOS 14+, matching the platform's availability. `build-apple.sh` passes those minimum versions to C dependencies as well as Cargo and rejects an archive containing a higher minimum OS before packaging. Override them only for an explicit compatibility test with `AWIKI_IOS_DEPLOYMENT_TARGET` and `AWIKI_IOS_ARM64_SIMULATOR_DEPLOYMENT_TARGET`.

Linux builds generate:

```text
packages/awiki_im_core/linux/lib/libawiki_im_core.so
```

The file is copied from `target/<target>/release/libawiki_im_core.so` and is ignored by git, matching the existing Android and Apple native artifact policy. The package's `linux/CMakeLists.txt` adds the generated `.so` to `awiki_im_core_bundled_libraries`, so Flutter installs it into the app bundle's `lib/` directory. The Dart loader opens it as `libawiki_im_core.so`, relying on the Flutter Linux runner's `$ORIGIN/lib` rpath.

## Common local errors

- Missing `../anp/anp/rust` sibling checkout: this workspace depends on a sibling ANP Rust crate.
- Missing `cargo-ndk`: required for full Android native library builds.
- Linux `libawiki_im_core.so` missing from the app bundle or native smoke fails to load it: run `scripts/flutter/build-sdk-native.sh --linux-only` before testing a Flutter Linux app that calls `AwikiImCore.open`.
- Linux dynamic library load error: verify the app bundle contains `lib/libawiki_im_core.so` and that the Flutter Linux runner preserves `$ORIGIN/lib` in `CMAKE_INSTALL_RPATH`.
- iOS symbols not found: verify the podspec vendored XCFramework path and `-force_load` slice path.
- FRB generated files stale: run `scripts/flutter/codegen-check.sh`.
