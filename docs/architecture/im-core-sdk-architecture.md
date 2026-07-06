# im-core SDK Architecture

## 1. Positioning

`crates/im-core` is the reusable Rust IM SDK for awiki. It owns product capabilities that used to be spread through the CLI: identity, auth/session, directory, messages, groups, attachments, secure, realtime, email, content/site, and local state.

The SDK is not a collection of wire helpers, RPC parameter builders, SQLite helpers, or crypto utilities. Public callers construct `ImCore`, bind an identity into `ImClient`, then call high-level services.

```text
CLI / Flutter / App / Agent
        |
        v
ImCore                    # environment-level entrypoint
        |
        v
ImClient                  # identity-bound product client
        |
        +-- auth()
        +-- identity()
        +-- directory()
        +-- messages()
        +-- groups()
        +-- attachments()
        +-- secure()
        +-- realtime()
        +-- email()
        +-- content() / site()
```

## 2. Crate Boundaries

```text
crates/im-core       # SDK product capability layer
crates/awiki-cli     # CLI thin shell
crates/im-core-dart  # Rust-Dart facade
packages/awiki_im_core
                    # Flutter/Dart package and platform loader
```

Dependency direction is fixed:

```text
awiki-cli      -> im-core
im-core-dart   -> im-core
awiki_im_core  -> im-core-dart native library
```

`im-core` must not depend on `awiki-cli`, CLI command parsing, CLI config resolution, CLI workspace discovery, OpenClaw/Hermes UX, or service manager types.

## 3. Host vs SDK Responsibilities

| Layer | Owns | Does not own |
| --- | --- | --- |
| `im-core` | Product flows, auth retry, target resolution, local owner binding, remote transport, local projection, secure/realtime orchestration | CLI flags, stdout/stderr, exit code, workspace discovery, service install/start/stop |
| `awiki-cli` | Command parsing, config/workspace/path resolution, permission checks, dry-run, output envelope, daemon/service UX, OpenClaw/Hermes setup | Business flows, raw wire payload construction, auth retry, secure/MLS internals |
| `im-core-dart` / `awiki_im_core` | Dart-friendly facade, FFI lifecycle, platform native library loading | App UI/cache DTOs, `awiki-me` gateway policy, Flutter Web runtime |

The CLI handler target shape is:

```text
parse flags -> build ImCore/ImClient -> call SDK -> render output
```

CLI may parse `--to`, `--group`, `--text-file`, `--file`, and `--secure`; it passes `MessageTarget`, `MessageBody`, `AttachmentInput`, and `MessageSecurityMode` to SDK services.

## 4. Identity Model

`ImCore` is environment-level and does not bind a current identity. `ImClient` binds one identity and automatically carries actor, auth runtime, local owner, and identity-scoped state.

```rust
let core = ImCore::new(config, paths)?;
let client = core.client(IdentitySelector::Default)?;
client.messages().send(request)?;
```

Rules:

- Do not use mutable global "current identity" inside SDK.
- `Default` is one `IdentitySelector`, not hidden process state.
- CLI credential names map to `IdentitySelector::LocalAlias`.
- auth/session, local state, direct secure state, and MLS state must be identity-scoped.
- Business queries inject owner internally; callers do not hand-write owner filters.

## 5. Paths and Configuration

Hosts pass explicit `ImCoreConfig` and `ImCorePaths`.

Host responsibilities:

- workspace and `config.yaml` resolution.
- identity root/default/registry path selection.
- DID document, key, auth/session, SQLite, runtime, cache, and temp paths.
- directory creation, chmod, backup, cleanup, and migration timing.

SDK responsibilities:

- read/write only the explicit paths passed by the host.
- bind paths to the selected identity.
- initialize and migrate local state through `CoreBootstrap`.
- avoid CLI workspace auto-discovery and CLI config parsing.

## 6. Public/Internal Boundary

Public API expresses product intent. Internal implementation owns wire, store, crypto, and transport details.

| Module | Public API expresses | Internal only |
| --- | --- | --- |
| core | `ImCore`, `ImClient`, config, paths, bootstrap, errors | `ClientIdentityRuntime`, path expansion, store handles |
| identity | selectors, summaries, registration, recovery, profile, DID replacement plan | private key material, DID writer, raw identity store rows |
| auth | login, ensure, refresh, status | proof builder, JWT file format, bearer header handling |
| directory | peer resolve, handle lookup, contacts, relationships | user-service raw request/response, contact store rows |
| messages | send, inbox, history, mark-read, conversations, reliable sync | message RPC params, wire DTOs, raw notification frames, checkpoint load/store |
| groups | lifecycle, members, profile/policy, group reads | group wire helpers, raw group receipts |
| attachments | send/download, source/destination DTOs | upload slots, object commit, ticket params, encrypted manifest internals |
| secure | status, prepare, repair, outbox summary, secure send policy | ciphertext, prekeys, KeyPackage, MLS private state, provider IO |
| realtime | status, runner, event stream, normalized `ImEvent` | WebSocket frame, request id, ping/pong, dispatch queues |
| email | account, inbox, read, mark-read, send, attachment, notifications | mail RPC params, raw JSON payload, auth headers |
| content/site | page/site product operations | content/site RPC envelope and wire normalization |

## 7. Module Map

- `core`: environment entrypoint, identity-bound client, bootstrap, errors, common IDs and paging types.
- `identity`: local registry, default identity, handle registration/recovery, profile, contact binding, DID replacement plan.
- `auth`: DID auth, session/JWT persistence, refresh, status, and retry support for business services.
- `local_state`: SQLite schema, owner isolation, messages, contacts, groups, email notification, secure outbox, realtime projection, and reliable sync checkpoints.
- `discovery`: endpoint and capability selection from config, DID documents, profile, and service metadata.
- `directory`: DID/Handle lookup, public profile, contact projection, relationship APIs.
- `messages`: direct/group send, inbox, history, conversations, mark-read, retry plan, local message projection.
- `groups`: group lifecycle, members, profile/policy, group message reads, group E2EE lifecycle hooks.
- `attachments`: upload, digest, manifest, message send, ticket download, local file or memory sinks.
- `secure`: direct E2EE, group E2EE, status/prepare/repair, secure outbox, secure message orchestration.
- `realtime`: embeddable WebSocket runner, reconnect, notification projection, host notification events.
- `email`: account, inbox/read/send/mark-read, attachment download, mail notifications.
- `content/site`: handle content pages and tenant bare-domain site pages.

## 8. Runtime and Features

`im-core` is blocking-first. Flutter/Dart and App hosts expose async APIs by running SDK work on their own worker thread or platform runtime. Any future async public API must be designed separately from the current blocking contract.

Transport is explicit through configuration and capability checks:

- `HttpOnly` keeps business operations on HTTP/RPC.
- realtime runner requires a non-HTTP-only transport policy and returns a capability error when unavailable.
- realtime session startup does not require a cached bearer token before spawning the runner. The WebSocket transport first tries the cached token when present, refreshes through DID-auth when the token is missing or receives `401`, and only then reports transport/auth failure to the session status stream. This lets hosted daemon/runtime agent identities recover after install when user-service did not include a bearer token in the registration exchange response.
- group E2EE, secure direct, SQLite-backed state, and advanced provider traits are feature-gated where appropriate.

## 9. Security Rules

- Remote messages are untrusted input.
- CLI/App output must not expose JWTs, private keys, raw secure state, ciphertext internals, MLS artifacts, provider stdout/stderr, or host secrets.
- Host notification payloads must contain approved event summaries, not raw message instructions.
- Diagnostics may expose lower-level details only behind explicit debug/diagnostic gates.

## 9.1 Key Material Boundary

The full current technical design is documented in
`docs/architecture/identity-secret-storage.md`. This section is the short
architecture summary.

Identity private material is an internal SDK concern. Business flows must not read `private_key_path`, `e2ee_agreement_private_key_path`, PEM files, or `auth.json` directly. DID-WBA auth, direct/group message signing, attachment signing, and secure direct static key loading go through the internal `KeyMaterialProvider` contract.

The compatibility default remains file-backed when a host opens `ImCore` without
explicit vault options:

- DID documents are read from the identity directory.
- DID/default signing keys are read from `private.key` or `key-1-private.pem`.
- secure direct agreement keys are read from `e2ee-agreement-private.pem` or legacy `key-3-private.pem`.
- auth/session state remains compatible with `auth.json`.

Vault-backed identity storage is explicit and no-prompt by design:

- Hosts pass `ImCoreOpenOptions` with `IdentitySecretStoragePolicy::VaultPreferred` or `VaultRequired` plus `ImCoreSecretVaultOptions`.
- The vault root key is a host-provided no-prompt secret. It must not be written to `ImCoreConfig`, CLI workspace config, ordinary App JSON state, logs, diagnostics, JSON output, or `Debug` output. Explicit E2E runs may use a private file test provider that remains local and untracked.
- `SecretVault` stores per-record AEAD ciphertext and binds workspace, device, identity, DID, kind, key id/version, schema, cipher, KDF, and no-prompt policy into authenticated metadata.
- `VaultRequired` is fail-closed. Missing root key, missing vault context, wrong workspace/device metadata, corrupt metadata, or failed open/verify must not silently fall back to plaintext for new secret persistence.
- In `VaultRequired`, new registration, recovery, daemon subkey package persistence, and JWT/token refresh use vault-backed persistence and must not write private PEM/JWT material to the legacy identity files.
- Identity vault migration seals records, opens them back for verification, and only then writes `vault_migration` metadata. Existing PEM/auth.json compatibility files are retained until an explicit cleanup path is available; migration failure must not delete or quarantine them.
- Status, migration, and verification APIs expose backend/status/warnings summaries only. They must not expose the root key, private key, JWT, full `SecretRef`, or ciphertext internals.

Process boundaries matter. App, CLI, and daemon run as separate hosts and must each unlock or provide their own vault context for their own state root. Do not assume one OS keychain item is readable across all processes.

Current host integration status:

- Plain `ImCore::new` / `open` remains FileCompat for compatibility. Secure callers must pass explicit vault options.
- `awiki-cli` resolves `secret_storage.mode`, `vault_dir`, `workspace_id`, and `device_id` from workspace config. The root key is read from `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` when present, otherwise from `vault_dir/root-key.b64u`; normal live paths may create that local private root-key file, while status/dry-run surfaces only report a redacted plan. `id vault status`, `id vault migrate`, `id vault cleanup-plaintext`, and doctor output are redacted.
- `im-core-dart` / `packages/awiki_im_core` expose optional Dart open options plus identity vault status/migrate/verify facade methods. The Dart package does not generate or persist host root keys.
- `awiki-me` opens `im-core` with `VaultRequired`. Production and custom state-root runs use `SecureAppKeyValueStore` for the App-local root key; only explicit E2E state roots use a private file test provider.
- `awiki-deamon` stores daemon/runtime `agent_identity` private keys and `user_delegated_identity` private keys as SecretVault refs in `daemon.db`; the legacy PEM columns keep a sentinel for compatibility. Older plaintext rows are read only as a migration bridge and are re-sealed when a daemon vault root key is available.

Known residual risks after the App/CLI/daemon vault integration:

- CLI root keys supplied through `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` are visible to the process environment; CLI root keys stored in `vault_dir/root-key.b64u` rely on private local file permissions. A platform wrapping/root-key backend and rotation/backup story remain follow-up work.
- App root key rotation, backup, recovery UX, and secure deletion of old plaintext compatibility files are not implemented.
- `id vault cleanup-plaintext` is a migration-gated/preflight surface unless a CLI-safe live cleanup API is added. Do not document it as deleting legacy files in this build.
- Explicit delegated `key_ref` flows support `vault:` refs and should use them for new daemon-owned delegated keys. `file:` / `local:` / bare path refs remain compatibility inputs and can still read caller-provided delegated private key files.
- The daemon Message/im-core SDK main path uses hosted in-memory identity material and no longer writes `private.key`, `e2ee-agreement-private.pem`, or `auth.json` for that path. Legacy DID-auth compatibility helpers may still create those files for user-service inventory/auth paths and should be treated as compatibility-only.
- The App bootstrap path can still receive a daemon subkey private key plaintext DTO. This is a temporary compatibility exception and should be replaced by an encrypted bootstrap envelope in a separate change.
- Direct E2EE session/prekey local state is encrypted at rest through SecretVault envelopes. Group MLS private state is outside this hardening pass.
- `awiki-deamon` `agent_auth_state` bearer tokens are persisted as daemon SecretVault refs with a sentinel in the `jwt_token` column; do not log or expose them.
- External key-agent IPC, public signing APIs, and DID child-key scope/revocation semantics are outside this boundary.

## 10. API References

Stable API references live under `docs/api/`:

- `docs/api/im-core-public-api.md`
- `docs/api/im-core-interface/*`

These files describe the SDK public surface and interface-level contracts. They should only change when the API changes; architecture-only cleanup should update this document and related feature docs instead.

## 11. Local Conversation Summary Projection

The SQLite local state keeps `messages` as the durable message projection truth. Conversation list reads must not aggregate all owner messages on every refresh. Schema version 18 adds `conversation_summaries` as a rebuildable materialized projection for chat-list summaries, schema version 19 adds an owner/conversation/timestamp hot index for local-first message history pagination, schema version 20 adds `sync_state`, and current schema version 21 carries the current conversation/read/send projection contract:

- primary key: `(owner_identity_id, conversation_id)`;
- hot index: `idx_conversation_summaries_owner_last(owner_identity_id, last_message_at DESC, conversation_id)`;
- unread index: `idx_conversation_summaries_owner_unread_last(owner_identity_id, unread_count, last_message_at DESC)`.

`list_conversations_for_owner_identity()` reads `conversation_summaries` by owner and joins only the stored `last_message_id` back to `messages`. The legacy `threads` SQLite view remains available for debugging and compatibility, but it is no longer the chat-list hot path. Incremental writes update touched summaries inside the same SQLite transaction as message/read-state projection; rebuild/repair paths remain available when a gap, migration, or debug check requires recomputing owner summaries from durable `messages` and `thread_read_state`.

Summary rows are derived state and may be rebuilt from `messages`, but hot writes are incremental after the performance work:

- schema open creates the table/indexes and backfills v17 stores when summaries are absent;
- ordinary message insert/update updates `conversation_summaries` by delta in the same SQLite write transaction;
- bounded mark-read, `mark_conversation_read`, and legacy `mark_thread_read` update unread / unread mention counters by delta where the previous state is known;
- fallback rebuild remains for message conversation moves, legacy DID-to-peer-scope direct merges, last-message ambiguity, missing/corrupt summary rows, first unread mention ambiguity, and explicit owner repair;
- committed invalidation and runtime store patches are emitted only after the local projection transaction commits;
- peer-scope direct compatibility uses a SQLite TEMP, owner-scoped memo per local-state connection: after a legacy DID fold, or after a peer handle has been recognized, later upserts in the same actor/session do not rescan all legacy DID rows or rerun the large UPDATE; late legacy rows that match the memoized DID/handle are normalized into the peer-scope conversation before insert.

`ConversationIdentity.conversation_id` is the SDK-level routing key for message display. Conversation list rows, message metadata, timeline patches, read-state updates, conversation-scoped send, and local repair must carry or derive from this canonical identity. `ThreadRef::{Direct, Group, Thread}` remains a compatibility / adapter surface for CLI migration, legacy callers, and low-level diagnostics. New AWiki Me and Flutter SDK message-display paths must not reconstruct a route from DID, handle, or legacy direct aliases when a canonical `conversation_id` is available.

Because summaries contain message preview fields, diagnostics and tests should treat them as local private state. Do not expose message content, payload JSON, or sender details in public logs; only log counts, durations, and redacted identifiers.

## 12. Conversation Snapshot And Runtime Store

Conversation snapshot and patch APIs are non-authoritative acceleration layers on top of committed local projection:

- `messages.load_conversation_snapshot()` / Dart `client.messages.loadConversationSnapshot()` reads a redb snapshot generated from `conversation_summaries`.
- Snapshot entries use `ConversationSnapshotItem`, a core-only DTO containing thread identity, participants, last message projection, unread counts, message count, and last message time.
- `messages.watch_conversation_patches()` / Dart `client.messages.watchConversationPatches()` streams versioned `ConversationStorePatch` values from an in-memory runtime store seeded by snapshot/local projection.
- `messages.repair_conversation_store()` / Dart `client.messages.repairConversationStore()` returns a reset/repair patch and the current runtime store version after lag, overflow, stream close, or version gaps.
- `messages.watch_conversation_timeline_patches(conversation, limit)` / Dart `client.messages.watchConversationTimelinePatches(conversation, limit: ...)` streams versioned `ThreadMessageStorePatch` values for the currently opened canonical conversation timeline.
- `messages.repair_conversation_timeline_store(conversation, limit)` / Dart `client.messages.repairConversationTimelineStore(conversation, limit: ...)` returns a reset/repair patch for the conversation timeline runtime store.
- `messages.watch_thread_patches(thread, limit)` / Dart `client.messages.watchThreadPatches(thread, limit: ...)` and `messages.repair_thread_store(thread, limit)` / Dart `client.messages.repairThreadStore(thread, limit: ...)` remain compatibility adapters for CLI / legacy `ThreadRef` paths, not the AWiki Me display-chain owner.
- Patch notifications are emitted only after the underlying local projection commit succeeds; `snapshot_required=true` or failed sync apply must not emit an authoritative patch.
- Realtime incoming messages follow the same committed-projection rule: a WebSocket hint or decoded event is not authoritative by itself, but once its message projection is committed to SQLite, `im-core` emits conversation and conversation-timeline patches for active subscribers.

The public APIs currently live under `messages()` / `client.messages` for compatibility with the existing SDK grouping. A future `conversations()` / `client.conversations` namespace may wrap the same core store, but both names must not expose divergent DTOs or ownership semantics.

`ConversationSnapshotItem` and `ConversationStorePatch` must remain SDK/core DTOs. They must not include `awiki-me` App-only presentation fields such as `hidden`, `pinned`, `muted`, `customTitle`, `avatarSeed`, `peerLifecycleState`, `ConversationSummary`, or `ChatMessage`. AWiki Me composes those fields in its own application layer; see `awiki-me/docs/conversation-presentation-ownership.md`.

Because snapshots and patches contain message preview fields, diagnostics and tests should treat them as local private state. Do not expose message content, payload JSON, or sender details in public logs; only log counts, durations, and redacted identifiers.

## 13. Local-first Message History

`messages.history()` keeps its remote history + projection/reconcile semantics. AWiki Me first paint should use `messages.local_conversation_timeline()` / Dart `client.messages.localConversationTimeline(...)` with a `ConversationReadRef`. Hot compatibility paths that only need already-projected local messages can still use `messages.local_history()` / Dart `client.messages.localHistory(...)`.

Local conversation timeline:

- reads only the local SQLite `messages` projection through `owner_identity_id` and canonical `ConversationReadRef.conversation_id`;
- does not call `direct.get_history`, `group.list_messages`, `inbox.get`, directory lookup, or E2EE remote projection;
- returns newest-first `MessagePage` items and an opaque `local-history:v1:*` cursor for paging older local messages;
- supports direct, group, and raw thread-backed conversations through the same owner-scoped conversation-id normalization as conversation mark-read.

The API is for fast first paint. Apps should show local conversation timeline rows immediately, then run `sync_conversation_after()` or a documented repair path in the background when freshness is needed. Remote history/backfill results are not UI truth until they have been persisted to the local projection and reloaded or emitted through the conversation timeline store.

## 13.1 Conversation Send And Local Echo

Conversation-surface sends should use `messages.send_conversation_text()` / Dart `client.messages.sendConversationText(...)`, `messages.send_conversation_payload()` / Dart `client.messages.sendConversationPayload(...)`, or `attachments.send_conversation()` / Dart `client.attachments.sendConversation(...)` when the caller already has a `ConversationReadRef`. `im-core` resolves the canonical conversation to the storage route, writes a durable pending projection row before network send, updates the row to accepted/sent/failed as the network result arrives, and emits committed patches only after the SQLite transaction succeeds.

`MessageMetadata.send_state`, `MessageMetadata.retry_plan`, and `MessageMetadata.conversation_identity` are the SDK facts for pending/accepted/sent/failed presentation. AWiki Me may render those states, but it must not create a second durable optimistic message store or decide send correctness from memory-only pending rows. Attachment local file preview may exist only as transient UI state during upload; list/detail timeline truth, retry correctness, and final send state must come from the SDK durable projection. Secure/E2EE conversation-surface local echo remains fail-closed where unsupported by the secure route.

## 14. Reliable Message Sync

Reliable message sync is split between service-owned event logs and
`im-core`-owned local recovery state. The service API is documented in
`message-service/docs/api/ANP-client-server-api-sync.md`; this document records
the SDK architecture boundary.

`im-core` Rust/SQLite owns the global reliable checkpoint:

- `messages.sync_delta()` / Dart `client.messages.syncDelta(...)` are high-level
  calls. Rust reads the current checkpoint from local `sync_state`, injects
  `since_event_seq` into the wire request, applies the returned page, and writes
  the new checkpoint only after the local apply transaction succeeds.
- Public Rust, Dart, Flutter, CLI, and App APIs must not expose
  `loadGlobalCheckpoint`, `storeGlobalCheckpoint`, raw `since_event_seq`, raw
  `next_event_seq`, or equivalent manual checkpoint advance.
- `snapshot_required=true` is fail-closed until a documented repair API exists:
  no checkpoint advance and no local projection wipe.

`messages.sync_conversation_after()` / Dart `client.messages.syncConversationAfter(...)` is the conversationId-first catch-up API for AWiki Me and the Flutter SDK display chain. It resolves `ConversationReadRef.conversation_id` to the syncable storage thread/ref, uses `after_server_seq`, and does not read or advance the account-level checkpoint. `messages.sync_thread_after()` / Dart `client.messages.syncThreadAfter(...)` remains a legacy / debug adapter. Implementations must not return a locally merged `history_async` page as a catch-up result; they use a raw remote path or strictly filter `server_seq > after_server_seq`.

Realtime notification parsing may expose a readonly `RealtimeSyncHint` from the
top-level WebSocket `sync` member. The hint is scheduling metadata for
duplicate/gap/dirty detection and for deciding when to call `sync_delta`.
Realtime projection is allowed to keep the UI fresh, but receiving a realtime
hint or applying a realtime projection does not advance the reliable checkpoint.
If a realtime incoming message cannot be projected or its local SQLite write
fails, it must not emit an authoritative conversation/timeline patch; the next
reliable sync or repair path remains responsible for convergence.

Schema version 20 adds `sync_state` with owner-scoped checkpoint rows:

- key: `(owner_identity_id, scope, checkpoint_kind)`;
- value: decimal string `event_seq`, plus `owner_did`, `updated_at`, and optional
  `metadata_json`;
- index: `idx_sync_state_owner_kind(owner_identity_id, checkpoint_kind,
  updated_at DESC)`.

`sync_state` is private local recovery state. Diagnostics should report counts,
durations, redacted owner/thread identifiers, and checkpoint age rather than raw
message payloads or sensitive E2EE material.

## 15. Conversation Read State

Conversation-level read state is separate from reliable sync checkpoints:

- `messages.mark_conversation_read()` / Dart `client.messages.markConversationRead(...)` accepts `ConversationReadRef` and an optional `ReadWatermark`; this is the AWiki Me / Flutter SDK display-chain read ack path.
- `messages.mark_thread_read()` / Dart `client.messages.markThreadRead(...)` remains a compatibility adapter for CLI / legacy `ThreadRef` callers.
- If no watermark is provided, `im-core` computes the highest visible committed thread-local sequence from local projection / thread store.
- Direct read watermarks use direct thread-local `server_seq`.
- Group read watermarks use the group thread view `server_seq`; the service may map it from group host `group_event_seq`, but public SDK/API callers do not submit `read_up_to_group_event_seq`.
- Local truth lives in `thread_read_state`; `conversation_summaries` caches unread/read display projection but is not the only source of truth.
- Remote ack uses `message-service` `read_state.mark_read` with profile `anp.read_state.local.v1`.
  The wire thread is resolved by `im-core` to direct / group; raw canonical storage
  `conversation_id` values are never serialized as `kind: "thread"`. Legacy direct
  `inbox.mark_read(message_ids)` remains only as fallback for unsupported services.
- `message.read_state_updated` sync events are not emitted by the current service-compatible phase. Adding that event requires first making stable clients treat the type as known or explicitly ignore-safe, because unknown required `sync.delta` events fail closed.
