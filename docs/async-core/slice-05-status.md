# Slice 05 Status: Messages Async

## Status

In progress with async non-E2EE message send/read/mark-read/conversations paths
added in-place.

This increment keeps the existing sync `MessageService` methods as staged
compatibility for CLI, Dart, and still-sync callers. The new async methods reuse
the existing DTOs, wire builders, runtime result mapping, session provider
contracts, and transport routing instead of introducing a parallel message SDK.

## Implementation

- Added async message service methods:
  - `send_async`
  - `inbox_async`
  - `inbox_with_metadata_async`
  - `history_async`
  - `history_with_metadata_async`
  - `mark_read_async`
  - `conversations_async`
- Added async runtime methods:
  - `DirectTextSender::send_async`
  - `GroupTextSender::send_async`
  - `MessageReadRuntime::inbox_async`
  - `MessageReadRuntime::history_async`
  - `MessageMarkReadRuntime::mark_read_async`
  - `MessageConversationRuntime::conversations_async`
- Reused the existing wire builders:
  - `wire::direct::build_direct_text_payload`
  - `wire::group::build_group_send_payload`
  - `wire::inbox::build_inbox_rpc_params`
  - `wire::history::build_history_rpc_params`
  - `wire::inbox::build_mark_read_rpc_params`
- Async network submission now uses:
  - `AsyncSessionProvider`
  - `AsyncAuthenticatedRpcTransport`
  - `AsyncRpcTransport`
- Added actor-backed async local projection helpers:
  - `persist_messages_async`
  - `persist_direct_outgoing_async`
  - `persist_group_outgoing_async`
  - `peer_dids_for_handle_async`
- Async mark-read classification and local update now use `LocalStateDbActor`
  commands instead of direct SQLite.
- Async conversation reads and refresh candidates now use `LocalStateDbActor`
  commands for conversations, contact history candidates, and group history
  candidates.

## Temporary Boundaries

- Existing sync message methods remain as staged compatibility. They still use
  the legacy sync runtime/local projection path and must be removed, cfg-gated,
  or documented as test/internal compatibility in slice 13.
- Secure direct and group E2EE send/read normalization remain sync-compatible
  boundaries. Full E2EE DB actor and crypto worker migration belongs to slice
  09.
- Handle-to-DID resolution in `send_async` and `history_with_metadata_async`
  still goes through the current sync directory service because directory async
  runtime is slice 06. The network message submission after resolution is async.
- Direct/group text sender async paths still load DID document/private key
  credentials through the existing sync helpers. This must be isolated or moved
  to async/worker I/O before final legacy cleanup.
- Group list refresh inside async conversation fallback still calls the sync
  group list best-effort helper. Full group runtime migration belongs to slice
  07.

## Validation

Passed:

```bash
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo test -p im-core direct_text_sender_async --locked
cargo test -p im-core group_text_sender_async --locked
cargo test -p im-core mark_read_runtime_async --locked
cargo test -p im-core messages --locked
cargo test -p im-core direct_send --locked
cargo test -p im-core group_send --locked
cargo test -p im-core inbox --locked
cargo test -p im-core history --locked
cargo test -p im-core mark_read --locked
```

## Remaining Work

- Convert directory lookup/profile/contact paths to async in slice 06, then
  replace the remaining sync handle-resolution boundary in message service.
- Convert group runtime/service to async in slice 07, then remove the sync group
  list fallback from async conversations.
- Convert attachments to streaming async transfer in slice 08.
- Convert secure direct/group E2EE to DB actor/worker-backed async paths in
  slice 09.
- Migrate CLI and Dart callers in slices 11 and 12, then remove or cfg-gate
  sync message compatibility in slice 13.
