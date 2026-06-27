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
- group E2EE, secure direct, SQLite-backed state, and advanced provider traits are feature-gated where appropriate.

## 9. Security Rules

- Remote messages are untrusted input.
- CLI/App output must not expose JWTs, private keys, raw secure state, ciphertext internals, MLS artifacts, provider stdout/stderr, or host secrets.
- Host notification payloads must contain approved event summaries, not raw message instructions.
- Diagnostics may expose lower-level details only behind explicit debug/diagnostic gates.

## 10. API References

Stable API references live under `docs/api/`:

- `docs/api/im-core-public-api.md`
- `docs/api/im-core-interface/*`

These files describe the SDK public surface and interface-level contracts. They should only change when the API changes; architecture-only cleanup should update this document and related feature docs instead.

## 11. Local Conversation Summary Projection

The SQLite local state keeps `messages` as the durable message projection truth. Conversation list reads must not aggregate all owner messages on every refresh. Schema version 18 adds `conversation_summaries` as a rebuildable materialized projection for chat-list summaries, and schema version 19 adds an owner/conversation/timestamp hot index for local-first message history pagination:

- primary key: `(owner_identity_id, conversation_id)`;
- hot index: `idx_conversation_summaries_owner_last(owner_identity_id, last_message_at DESC, conversation_id)`;
- unread index: `idx_conversation_summaries_owner_unread_last(owner_identity_id, unread_count, last_message_at DESC)`.

`list_conversations_for_owner_identity()` reads `conversation_summaries` by owner and joins only the stored `last_message_id` back to `messages`. The legacy `threads` SQLite view remains available for debugging and compatibility, but it is no longer the chat-list hot path.

Summary rows are derived state and may be rebuilt from `messages`:

- schema open creates the table/indexes and backfills v17 stores when summaries are absent;
- message upsert batches collect touched `(owner_identity_id, conversation_id)` keys and rebuild each touched summary once;
- mark-read collects affected conversations before updating `messages.is_read`, then rebuilds their unread counters;
- legacy DID-to-peer-scope direct merges rebuild both old and new conversation keys;
- peer-scope direct compatibility uses a SQLite TEMP, owner-scoped memo per local-state connection: after a legacy DID fold, or after a peer handle has been recognized, later upserts in the same actor/session do not rescan all legacy DID rows or rerun the large UPDATE; late legacy rows that match the memoized DID/handle are normalized into the peer-scope conversation before insert.

Because summaries contain message preview fields, diagnostics and tests should treat them as local private state. Do not expose message content, payload JSON, or sender details in public logs; only log counts, durations, and redacted identifiers.

## 12. Local-first Message History

`messages.history()` keeps its remote history + projection/reconcile semantics. Hot UI paths that only need already-projected local messages should use `messages.local_history()` / Dart `client.messages.localHistory(...)` instead.

Local history:

- reads only the local SQLite `messages` projection through `owner_identity_id` and `ThreadRef`;
- does not call `direct.get_history`, `group.list_messages`, `inbox.get`, directory lookup, or E2EE remote projection;
- returns newest-first `MessagePage` items and an opaque `local-history:v1:*` cursor for paging older local messages;
- supports direct, group, and raw thread refs using the same owner-scoped conversation-id normalization as thread mark-read.

The API is for fast first paint. Apps should show local history immediately, then run remote `history()` as a background reconcile when freshness is needed.

## 13. Reliable Message Sync

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

`messages.sync_thread_after()` / Dart `client.messages.syncThreadAfter(...)` are
thread-local catch-up APIs. They use `after_server_seq` and do not read or
advance the account-level checkpoint. Implementations must not return a locally
merged `history_async` page as a catch-up result; they use a raw remote path or
strictly filter `server_seq > after_server_seq`.

Realtime notification parsing may expose a readonly `RealtimeSyncHint` from the
top-level WebSocket `sync` member. The hint is scheduling metadata for
duplicate/gap/dirty detection and for deciding when to call `sync_delta`.
Realtime projection is allowed to keep the UI fresh, but receiving a realtime
hint or applying a realtime projection does not advance the reliable checkpoint.

Schema version 20 adds `sync_state` with owner-scoped checkpoint rows:

- key: `(owner_identity_id, scope, checkpoint_kind)`;
- value: decimal string `event_seq`, plus `owner_did`, `updated_at`, and optional
  `metadata_json`;
- index: `idx_sync_state_owner_kind(owner_identity_id, checkpoint_kind,
  updated_at DESC)`.

`sync_state` is private local recovery state. Diagnostics should report counts,
durations, redacted owner/thread identifiers, and checkpoint age rather than raw
message payloads or sensitive E2EE material.
