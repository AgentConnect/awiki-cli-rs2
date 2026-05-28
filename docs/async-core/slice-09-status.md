# Slice 09 Status: E2EE / Secure Services Async Safety

## Scope Completed In This Increment

This increment moves the secure outbox queue/list/retry/drop and delivery-state
update paths plus the direct secure status read path behind the `LocalStateDb`
actor, adds async public service methods where public APIs need them, and
establishes a direct E2EE session revision boundary for later send/receive
mutation work without replacing the existing SDK surface.

Completed:

- Added `LocalStateDb` actor commands for:
  - `ListE2eeOutbox`
  - `QueueE2eeOutbox`
  - `MarkE2eeOutboxSent`
  - `MarkE2eeOutboxSentAndStoreMessage`
  - `SetE2eeOutboxFailure`
  - `RetryE2eeOutbox`
  - `DropE2eeOutbox`
  - `DirectSecureStatus`
  - `DirectSecureRepair`
  - `PrepareDirectSecurePrekeys`
  - `GetDirectSecureSession`
  - `DirectInitSessionMaterial`
  - `SaveIncomingDirectInitSession`
  - `SaveOutgoingDirectInitSession`
  - `SaveDirectSecureSessionIfRevision`
- Added async actor methods:
  - `list_e2ee_outbox`
  - `queue_e2ee_outbox`
  - `mark_e2ee_outbox_sent`
  - `mark_e2ee_outbox_sent_and_store_message`
  - `set_e2ee_outbox_failure`
  - `retry_e2ee_outbox`
  - `drop_e2ee_outbox`
  - `direct_secure_status`
  - `direct_secure_repair`
  - `prepare_direct_secure_prekeys`
  - `get_direct_secure_session`
  - `direct_init_session_material`
  - `save_incoming_direct_init_session`
  - `save_outgoing_direct_init_session`
  - `save_direct_secure_session_if_revision`
- Added `direct_e2ee_sessions.revision` and bumped local state schema version
  from 15 to 16. Existing tables are migrated in place with
  `ALTER TABLE ... ADD COLUMN`.
- Added direct session compare-and-swap store support:
  - `DirectSessionRecord::revision`
  - `DirectSessionCasResult`
  - `SqliteDirectSecureStateStore::save_session_if_revision`
- Added `DirectSecureConversation` async methods:
  - `status_async`
  - `prepare_async`
  - `repair_async`
- Added `SecureOutboxService` async methods:
  - `list_failed_async`
  - `retry_async`
  - `drop_async`
- Added an actor-backed async queued secure outbox flush helper:
  - `flush_queued_secure_outbox_with_sender_async`
- Added an actor/CAS-backed async direct secure follow-up send path for
  already-established sessions:
  - `AsyncDirectSecureTextSender::send_follow_up_if_ready`
  - loads the latest direct session through `LocalStateDb`
  - performs follow-up encryption on the blocking worker
  - saves the mutated session through `SaveDirectSecureSessionIfRevision`
  - sends the resulting `direct.send` request through async authenticated
    transport
  - returns local projection to the existing deferred actor-backed persistence
    path
- Added an actor-backed async direct secure init-session send path for
  no-existing-session sends:
  - `AsyncDirectSecureTextSender::send_async_if_ready`
  - resolves the peer DID document through async directory transport, with async
    local DID document cache fallback
  - fetches and verifies the peer prekey bundle through async authenticated
    transport, including the existing retry-without-OPK compatibility behavior
  - reads the local agreement private key through async file I/O
  - performs X3DH init-session creation on the blocking worker
  - saves the pending-confirmation initiator session through
    `SaveOutgoingDirectInitSession` before sending the direct-init request
  - sends the resulting `direct.send` init request through async authenticated
    transport
  - returns local projection to the existing deferred actor-backed persistence
    path
- Added an async direct secure pending-confirmation queue path for already
  pending local sessions:
  - detects `SESSION_STATUS_PENDING_CONFIRMATION` after loading the latest
    direct session through `LocalStateDb`
  - returns the same queued SDK result shape and secure outbox local effect as
    the legacy sender
  - avoids network transport and crypto work for a state that can only queue
    until peer confirmation arrives
  - relies on the existing deferred `LocalStateDb::queue_e2ee_outbox`
    persistence path
- Added an actor/CAS-backed async direct secure receive path for
  already-established cipher messages:
  - `AsyncDirectSecureIncomingProcessor::process_cipher_if_ready`
  - loads the latest direct session through `LocalStateDb`
  - performs follow-up decryption on the blocking worker
  - saves the mutated session through `SaveDirectSecureSessionIfRevision`
  - returns the same decrypted/undecryptable projection shape as the legacy
    incoming processor
  - falls back for init-session, missing-session, non-established-session, and
    session-id mismatch cases
- Added an actor-backed async direct secure init receive path for async
  inbox/history projection:
  - `AsyncDirectSecureIncomingProcessor::process_init_if_ready`
  - loads existing session/prekey material through `LocalStateDb`
  - reads the local agreement private key through async file I/O
  - resolves the sender DID document through async directory transport, with
    async local DID document cache fallback
  - performs X3DH init acceptance on the blocking worker
  - commits the new session and one-time-prekey consumption in one actor-owned
    SQLite transaction
  - preserves read-only semantics: it decrypts and stores the responder session,
    but does not send ACK or flush pending direct outbox from inbox/history
    reads
- Added an async direct realtime control side-effect seam:
  - `maybe_normalize_direct_e2ee_notification_with_processor_async`
  - `DirectRealtimeControlSideEffect`
- Added an actor/CAS-backed async direct realtime normalization seam for
  already-established cipher notifications:
  - `try_normalize_direct_e2ee_notification_for_client_async`
  - normalizes decrypted realtime notifications without opening SQLite directly
  - uses `AsyncDirectSecureIncomingProcessor` for session load/decrypt/save
  - processes direct init notifications when the sender DID document is
    available in the async local identity cache
  - returns explicit fallback for missing-session/non-established-session cases
    so remaining compatibility paths still use the staged legacy decryptor
- Moved production direct secure realtime projection to an async-first path:
  - `AsyncFirstSecureRealtimeNotificationProjector`
  - creates the internal async transport/DB actor bridge inside the existing
    sync realtime runner boundary
  - uses the actor/CAS async normalizer before falling back to the legacy sync
    normalizer for unsupported compatibility states
  - keeps public realtime APIs and event DTOs unchanged
- Decoupled `LocalStateDb::open` actor lifetime from short-lived Tokio runtimes:
  - the actor now runs on an independent std thread instead of a
    runtime-owned blocking task
  - sync compatibility bridges can create and drop temporary runtimes without
    stopping or hanging the DB actor they just opened
  - SQLite access remains serialized behind the actor; this is a lifecycle
    fix, not a new parallel persistence implementation
- Added async direct realtime control side-effect execution for production
  direct secure realtime normalization:
  - secure-init control plaintext sends a secure ACK through async authenticated
    transport using the actor/CAS follow-up send helper
  - secure ACK plaintext flushes pending secure outbox rows through the actor
    and async direct follow-up sender
  - control notifications remain dropped from the public event stream
- Moved the async inbox/history direct secure projection path to first process
  direct init messages and established direct cipher messages through the
  actor-backed async receive path in server order, then send only unprocessed
  direct E2EE messages through the staged legacy decryptor for compatibility.
- Moved the public `MessageService::send_async` direct secure branch to first
  try the actor/CAS-backed async follow-up path for already-established direct
  sessions, then the actor-backed async init-session path when no local session
  exists, and directly queue pending-confirmation sends when the local session
  is waiting for peer confirmation. It falls back to the staged blocking-worker
  legacy sender only for remaining non-established compatibility states not yet
  decomposed into async actor operations.
- Moved the public `MessageService::send_async` group E2EE text send happy path
  onto async network/local-state boundaries under the `group-e2ee` feature:
  - async credential loading uses Tokio file I/O
  - service-head resolution uses async authenticated transport
  - local group snapshot lookup for state-ref resolution uses `LocalStateDb`
  - state-ref local MLS status resolution is isolated on the runtime blocking
    worker instead of executing directly on the async task
  - MLS encryption is isolated on the runtime blocking worker
  - encrypted `group.e2ee.send` uses async authenticated transport
  - local outgoing group E2EE projection uses `LocalStateDb::store_messages`
  - epoch-mismatch repair/retry remains on the staged sync compatibility path
    until group repair/lifecycle are decomposed into async actor/worker pieces
- Added async group secure status/prepare APIs:
  - `GroupE2eeStatusRuntime::status_async`
  - `GroupSecureConversation::status_async`
  - `GroupSecureConversation::prepare_async`
  - async status uses async session readiness, async credential file I/O, and
    async authenticated `group.e2ee.head` / `group.e2ee.notice` RPCs
  - local MLS status in the async path is isolated on the runtime blocking
    worker instead of executing directly on the async task
  - status diagnosis, DTO shape, redaction behavior, and unsupported/default
    feature behavior stay aligned with the existing sync status path
- Added an actor-backed group E2EE summary projection helper and moved group
  repair async work onto async public boundaries:
  - lifecycle and repair now share one group summary `GroupRecord` builder
  - sync lifecycle/repair keep the legacy SQLite compatibility write path
  - `GroupE2eeRepairRuntime::repair_async` uses async session readiness, async
    credential file I/O, async authenticated `group.e2ee.head` /
    `group.e2ee.notice` RPCs, and `LocalStateDb::upsert_group` for repaired
    summary persistence
  - `GroupSecureConversation::repair_async` is now a public async group repair
    API under `group-e2ee`; the default feature build keeps the unsupported
    capability error shape
  - local MLS repair status snapshots, accepted-pending finalize checks, commit
    notice processing, welcome processing, and duplicate/already-applied notice
    checks are isolated on the runtime blocking worker; they still use the
    existing sync provider and are not rewritten in this increment
- Added an async group E2EE leave-request lifecycle path and wired it into the
  public `GroupService::leave_async` secure-required member branch:
  - `ensure_group_e2ee_service_available_async` uses async session readiness,
    async credential file I/O, and async authenticated `group.e2ee.head` /
    optional `group.e2ee.get_key_package` preflight RPCs
  - `leave_secure_group_request_async` sends `group.e2ee.leave_request` through
    async authenticated transport and preserves the existing redacted public
    delivery shape
  - the member leave-request path avoids initializing the local MLS provider
    because it does not prepare or finalize a local MLS commit
  - cached owner leave now returns the same `group owner cannot leave the group`
    `invalid_input` error as the existing sync public path without routing
    through the staged blocking worker
- Added an async group E2EE create lifecycle path and wired it into
  `GroupService::create_async` for secure-required group creation:
  - public async group create now uses the existing async group lifecycle RPC
    path before running the group E2EE create step
  - `GroupE2eeLifecycleRuntime::create_secure_group_async` uses async session
    readiness, async credential file I/O, async local group snapshot lookup for
    state-ref metadata, and async authenticated `group.e2ee.create`
  - group E2EE create summary persistence uses `LocalStateDb::upsert_group`
    through `persist_group_e2ee_summary_async`
  - local MLS create/finalize operations are isolated on the runtime blocking
    worker; they still use the existing sync provider and are not rewritten in
    this increment
- Added an async group E2EE add-member lifecycle path and wired it into
  `GroupService::add_member_async` for secure-required member additions:
  - public async member add now resolves the member DID through the existing async
    directory path and uses the existing async group lifecycle RPC before the
    group E2EE add step
  - `GroupE2eeLifecycleRuntime::add_secure_member_async` uses async session
    readiness, async credential file I/O, async authenticated
    `group.e2ee.get_key_package`, async local group snapshot lookup for
    state-ref metadata, and async authenticated `group.e2ee.add`
  - group E2EE add summary persistence uses `LocalStateDb::upsert_group` through
    `persist_group_e2ee_summary_async`
  - local MLS add/finalize operations are isolated on the runtime blocking
    worker; they still use the existing sync provider and are not rewritten in
    this increment
- Added an async group E2EE remove-member lifecycle path and wired it into
  `GroupService::remove_member_async` for secure-required member removals:
  - public async member remove now resolves the member DID through the existing
    async directory path and uses the existing async group lifecycle RPC before
    the group E2EE remove step
  - `GroupE2eeLifecycleRuntime::remove_secure_member_async` uses async session
    readiness, async credential file I/O, async local group snapshot lookup for
    state-ref metadata, and async authenticated `group.e2ee.remove`
  - group E2EE remove summary persistence uses `LocalStateDb::upsert_group`
    through `persist_group_e2ee_summary_async`
  - local MLS remove/finalize operations are isolated on the runtime blocking
    worker; deterministic service-rejection abort also runs on the worker. They
    still use the existing sync provider and are not rewritten in this increment
- Added an async group E2EE process-leave-request lifecycle path and exposed it
  through `GroupService::process_e2ee_leave_request_async`:
  - public async owner leave-request processing now uses async service
    availability preflight, async member resolution, async authenticated
    `group.e2ee.process_leave_request`, and then the async group E2EE remove
    lifecycle
  - the final remove summary persistence uses `LocalStateDb::upsert_group`
    through `persist_group_e2ee_summary_async`
  - the public delivery merge keeps the existing
    `secure_group_process_leave_request` shape while preserving redaction of MLS
    artifacts
  - local MLS remove/finalize operations reuse the worker-isolated async remove
    lifecycle and still use the existing sync provider
- Added async group E2EE update-key and recover-member lifecycle paths and
  exposed them through `GroupService::update_member_key_async` and
  `GroupService::recover_member_async`:
  - public async key replacement/recovery now uses async service availability
    preflight, async member resolution, async key-package lookup, async local
    group snapshot lookup for state-ref metadata, and async authenticated
    `group.e2ee.update` / `group.e2ee.recover_member`
  - group E2EE update/recovery summary persistence uses
    `LocalStateDb::upsert_group` through `persist_group_e2ee_summary_async`
  - the public delivery actions stay aligned with the existing
    `secure_group_update_key` and `secure_group_recover_member` shapes
  - local MLS update/recover/finalize operations are isolated on the runtime
    blocking worker; they still use the existing sync provider and are not
    rewritten in this increment
- Added a public async group E2EE key-package publish API:
  - `GroupService::publish_key_package_async`
  - async publish uses async session readiness, async credential file I/O, and
    async authenticated `group.e2ee.publish_key_package`
  - the existing DTO shape, DID-WBA binding signing, wire payload, and sync
    compatibility method stay aligned with `publish_key_package`
  - local MLS key-package generation in the async publish path is isolated on
    the runtime blocking worker; it still uses the existing sync provider
    boundary and is not rewritten in this increment
- Added worker-isolated async group E2EE incoming decrypt helpers and wired them
  into real async read/realtime callers:
  - `maybe_decrypt_group_e2ee_messages_for_client_async`
  - `maybe_decrypt_group_e2ee_messages_with_provider_async`
  - `maybe_normalize_group_e2ee_notification_for_client_async`
  - `maybe_normalize_group_e2ee_notification_with_provider_async`
  - `MessageReadRuntime::history_async` group history and
    `GroupReadRuntime::messages_async` now keep async network/projection
    boundaries and move local MLS decrypt work to the runtime blocking worker
	  - the production async-first realtime projector now runs group incoming E2EE
	    normalization through the async worker-isolated helper before projecting the
	    public realtime event
	  - group E2EE notice notifications in that projector now call
	    `maybe_process_group_e2ee_notice_notification_for_client_async`, so notice
	    repair uses `GroupSecureConversation::repair_async` inside the current sync
	    runner boundary instead of the staged sync `repair` method
	  - the sync inbox/history/group/realtime compatibility paths stay on the
	    existing sync helper until their callers are migrated; the full Tokio
	    realtime runner replacement remains slice 10 work
- Split direct secure send local persistence into legacy SQLite and deferred
  effects. The async public branch still uses the staged blocking worker for the
  legacy E2EE client internals, but local successful projection and
  pending-confirmation outbox enqueue now return to the async caller and execute
  through `LocalStateDb` actor commands.
- Moved `DirectSecureConversation::prepare_async` onto an async-specific path:
  auth/session readiness and credential reads use async file/runtime APIs, local
  prekey generation and persistence execute on `LocalStateDb`, and prekey bundle
  publish uses async authenticated transport. The sync `prepare` method remains
  as a staged compatibility path.
- Moved the repair async path's local session deletion/outbox requeue onto the
  DB actor, and its post-repair local prekey preparation now reuses the
  actor-backed async prepare path instead of wrapping the sync prepare helper in
  the blocking worker.
- Preserved existing sync secure outbox methods for staged CLI and Dart/FRB
  compatibility.
- Kept existing secure DTOs, secure outbox redaction behavior, wire format, and
  owner scoping.
- Adjusted the default non-`group-e2ee` group secure status stub to report
  `Unavailable` with an `Unsupported` problem, matching the public API shape for
  a capability that is not enabled.

## DB Actor Commands

The new commands are narrow wrappers around the existing
`internal/store/e2ee_outbox.rs` and `internal/secure_direct/status.rs` SQL
helpers. They execute on the actor-owned SQLite connection and retain existing
owner scoping behavior:

```text
ListE2eeOutbox(scope, local_status)
QueueE2eeOutbox(record)
MarkE2eeOutboxSent(scope, outbox_id, session_id, sent_msg_id, sent_server_seq, metadata)
MarkE2eeOutboxSentAndStoreMessage(scope, outbox_id, session_id, sent_msg_id, sent_server_seq, metadata, message)
SetE2eeOutboxFailure(scope, outbox_id, error_code, retry_hint, metadata)
RetryE2eeOutbox(scope, outbox_id)
DropE2eeOutbox(scope, outbox_id)
DirectSecureStatus(scope, peer)
DirectSecureRepair(scope, peer)
PrepareDirectSecurePrekeys(input)
GetDirectSecureSession(owner_identity_id, peer_did)
DirectInitSessionMaterial(owner_identity_id, peer_did, session_id, signed_prekey_id, one_time_prekey_id)
SaveIncomingDirectInitSession(commit)
SaveDirectSecureSessionIfRevision(record, expected_revision)
```

These commands cover the public secure outbox API, pending-confirmation outbox
enqueueing, outbox delivery-state transitions needed by later async send
flushing, the direct secure status/repair APIs, async direct prekey local
preparation, established-session direct follow-up send mutation, and
established-session direct receive mutation. The direct init receive material
and commit commands keep signed/one-time-prekey reads and session/OPK mutation
on the actor-owned SQLite connection. The actor-owned direct session load/save
CAS primitive is now wired into the async follow-up direct send path and async
inbox/history established-cipher receive path. Direct init receive uses a
narrower actor transaction because it creates a responder session and consumes
an OPK rather than mutating an existing established session. It is not yet wired
into direct init-session send, group E2EE state mutation, or production realtime
receive-side projection.

## Transaction Boundaries

This increment does not introduce new multi-step E2EE transaction semantics.
Each actor command maps to one existing outbox SQL operation:

- list failed outbox records
- insert one queued outbox record
- update one outbox record to `sent`
- update one outbox record to `sent` and store the local message projection in a
  single SQLite transaction, after first confirming the outbox row belongs to
  the requested owner scope
- update one outbox record to `failed`
- update one outbox record to `queued`
- update one outbox record to `dropped`
- read direct secure local status from existing direct session/prekey/outbox
  tables
- delete direct secure sessions and requeue failed outbox entries during repair
- generate/persist direct signed and one-time prekeys on the actor-owned
  connection and return a redacted publish request for async transport
- load direct E2EE session records with their current revision
- load direct init receive material through the actor, including existing
  session-by-id, current peer-session revision, signed prekey private material,
  and optional one-time-prekey private material
- save an accepted incoming direct init responder session and mark the consumed
  one-time prekey in one actor-owned SQLite transaction; if the init session id
  already exists for the same peer, the commit is idempotent and does not
  consume the OPK again
- save direct E2EE session records only when the expected revision still matches
  the actor-owned SQLite row

The direct session CAS path is the first stale-session protection primitive for
slice 09. It prevents a worker using an old session snapshot from silently
overwriting a newer session state. It is now wired into established-session
async direct follow-up sends and established-session async inbox/history receive
decryption. Async inbox/history direct init receive now has its own actor
transaction boundary for create-session plus OPK consumption. Direct init-session
send, pending-confirmation retry/flush, and production realtime mutation still
need equivalent actor-backed boundaries.

The broader slice 09 transaction requirements remain open for the direct/group
E2EE mutation paths. Group E2EE still needs equivalent stale-session protection.
The actor now has the local outbox/projection transaction primitive needed by
async direct outbox flush, and `secure_direct::outbox` now has actor-backed
async flush plan execution helpers that use that primitive. Production direct
secure realtime ACK handling now uses the async actor-backed flush path through
the async-first realtime projector. Legacy sync flush remains only for staged
sync compatibility paths.

`secure_direct::incoming` now has async control side-effect execution for
decrypted secure ACK/init control messages. ACK handling flushes queued secure
outbox through the actor; init-control handling sends an encrypted ACK via the
actor/CAS direct follow-up helper, then flushes queued outbox for that peer when
ACK send succeeds.

`secure_direct::incoming` also now has an actor/CAS-backed async realtime
normalization path for established cipher notifications and direct init
notifications with cached sender documents. The existing public sync realtime
runner is still present, but its production secure-direct projector now calls
this async-first path before legacy fallback.

## Crypto Worker Status

The public `MessageService::send_async` direct secure branch now uses a real
async direct-send path for established follow-up sends and first-message init
sends, and an actor-backed queue path for pending-confirmation sends:

- local session load happens through `LocalStateDb`
- follow-up encryption runs on the runtime blocking worker
- mutated session save uses `SaveDirectSecureSessionIfRevision`
- when no local session exists, peer DID document resolution and prekey-bundle
  fetch use async transports
- init-session encryption runs on the runtime blocking worker
- pending-confirmation initiator session save uses
  `SaveOutgoingDirectInitSession`
- `direct.send` follow-up and init requests use async authenticated transport
- when the latest local session is already pending peer confirmation,
  `send_async` skips network/crypto work and returns a secure outbox queue
  effect for `LocalStateDb::queue_e2ee_outbox`
- successful outgoing plaintext projection uses the existing
  `persist_direct_e2ee_outgoing_async` DB actor path

When the decomposed async path cannot handle another staged non-established
local session state, `send_async` still executes the legacy direct secure sender
through the runtime blocking worker.

The legacy fallback branch still defers direct send local persistence side
effects out of that worker:

- successful outgoing plaintext projection uses
  `persist_direct_e2ee_outgoing_async`, which writes through `LocalStateDb`
  `store_messages`;
- pending-confirmation outbox enqueue uses `LocalStateDb::queue_e2ee_outbox`.

The async inbox/history direct receive projection now also has a real
direct init receive path and established-cipher path:

- direct init receive material loads happen through `LocalStateDb`
- local agreement private key reads use async file I/O
- sender DID document resolution uses async directory transport with async local
  cache fallback
- X3DH init acceptance runs on the runtime blocking worker
- new responder session persistence and optional one-time-prekey consumption
  happen in one actor-owned SQLite transaction
- local session load happens through `LocalStateDb`
- follow-up decryption runs on the runtime blocking worker
- mutated session save uses `SaveDirectSecureSessionIfRevision`
- decrypted and undecryptable projections reuse the legacy SDK-visible result
  shape
- async inbox/history now queues missing-session and non-established-session
  direct cipher messages inside the per-page async receive processor and replays
  them after a later direct init creates the actor-backed session; replayed cipher
  projections are applied back to their original message rows
- unreplayed missing-session, non-established-session, and session-id mismatch
  cipher cases remain on the staged legacy decryptor

The async realtime direct init path with explicit transports now also resolves an
uncached sender DID document through the async directory transport before falling
back to the async local identity cache. The cache-only async helper still returns
an explicit fallback for uncached direct init notifications because it has no
directory transport boundary.

The production async-first direct secure realtime projector now keeps one
`AsyncDirectSecureIncomingProcessor` across notifications. Missing-session or
non-established direct cipher notifications can be queued in that processor,
then replayed after a later direct init establishes the actor-backed session.
Replayed plaintext is emitted as additional realtime events and flows through
the local-state realtime projector before the current notification event is
returned.

This remains a temporary compatibility boundary for non-established local
direct-session send states beyond no-session init and pending-confirmation
queueing, cache-only realtime direct init notifications without a directory
transport, the full Tokio realtime runner replacement scheduled for slice 10,
and group E2EE realtime normalization. Group E2EE send/receive crypto-heavy
paths still require equivalent worker isolation or async-safe decomposition.

`prepare_async` no longer wraps the sync direct secure prepare helper in the
blocking worker. Its local prekey DB work is now actor-backed, while the
remaining cryptographic key generation runs on the actor's blocking thread as
part of that command. This is a narrower transition step than full send/receive
CAS migration: it does not change direct E2EE wire format, public DTOs, or the
sync compatibility method.

## Tests Added Or Updated

Added actor coverage:

- `db_actor_e2ee_outbox_commands_use_existing_store_helpers`
- `db_actor_e2ee_outbox_queue_uses_existing_store_helper`
- `db_actor_e2ee_outbox_delivery_updates_are_owner_scoped`
- `db_actor_e2ee_outbox_mark_sent_and_store_message_is_transactional`
- `db_actor_direct_secure_status_uses_existing_store_helpers`
- `db_actor_direct_secure_repair_removes_session_and_requeues_failed_outbox`
- `db_actor_prepare_direct_secure_prekeys_persists_local_state_and_returns_publish_request`
- `db_actor_direct_secure_session_cas_rejects_stale_updates`
- `db_actor_incoming_direct_init_session_saves_session_and_consumes_opk_atomically`

Added schema/store coverage:

- `local_state_schema_adds_direct_session_revision_to_existing_tables`
- `sqlite_store_cas_rejects_stale_direct_session_updates`

Added public async service coverage:

- `secure_direct_status_async_uses_db_actor`
- `secure_direct_prepare_async_initializes_send_state_and_returns_redacted_dto`
- `secure_outbox_async_failed_retry_drop_uses_db_actor`

Added async outbox flush coverage:

- `secure_outbox_async_flush_marks_sent_and_stores_local_message_via_actor`
- `realtime_notification_async_side_effect_flushes_outbox_after_ack_via_actor`

Added direct secure async send local-effect coverage:

- `deferred_direct_e2ee_success_projection_uses_db_actor`
- `deferred_direct_e2ee_pending_outbox_uses_db_actor`

Added direct secure established-session async send coverage:

- `encrypt_follow_up_from_record_mutates_session_and_builds_cipher_request`
- `encrypt_follow_up_from_record_rejects_pending_confirmation_session`
- `async_direct_secure_sender_uses_actor_cas_and_async_transport_for_follow_up`
- `async_direct_secure_sender_initializes_session_via_actor_and_async_transport`
- `async_direct_secure_sender_queues_when_session_pending_confirmation`

Added direct secure established-session async receive coverage:

- `decrypt_follow_up_from_record_mutates_session_and_returns_plaintext`
- `decrypt_follow_up_from_record_returns_undecryptable_without_mutation_for_bad_cipher`
- `decrypt_follow_up_from_record_rejects_pending_confirmation_session`
- `async_direct_receive_processor_uses_actor_cas_for_established_cipher`
- `async_direct_receive_processor_falls_back_without_established_session`
- `async_direct_receive_processor_accepts_init_via_actor_and_consumes_opk`
- `async_direct_receive_processor_replays_pending_cipher_after_init`
- `messages_read_async_projects_direct_init_without_legacy_fallback`
- `messages_read_async_replays_pending_direct_cipher_after_init`

Added direct secure async realtime normalization coverage:

- `realtime_notification_client_async_normalizer_uses_actor_cas_for_established_cipher`
- `realtime_notification_client_async_normalizer_falls_back_without_established_session`
- `realtime_notification_client_async_normalizer_redacts_bad_cipher_without_saving`
- `realtime_notification_client_async_normalizer_sends_ack_after_secure_init_control`
- `realtime_notification_client_async_normalizer_resolves_init_sender_document_over_async_directory`
- `realtime_async_first_projector_uses_actor_cas_for_direct_cipher`
- `realtime_async_first_projector_replays_pending_direct_cipher_after_init`
- `realtime_async_first_projector_uses_async_group_e2ee_normalizer`
- `realtime_notification_projection_async_replaces_wire_body_with_plaintext`

Updated existing public secure API coverage:

- `secure_service_api_shape_is_available_from_client`

## Validation

Commands run and passed:

```bash
cargo fmt --all -- --check
cargo test -p im-core direct_session --locked
cargo test -p im-core outbox --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo check -p im-core --locked
cargo check --workspace --locked
```

Additional validation run after adding actor-backed outbox delivery-state
commands:

```bash
cargo fmt --all -- --check
cargo test -p im-core outbox --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo check -p im-core --locked
```

Additional validation run after adding actor-backed async queued outbox flush:

```bash
cargo fmt --all -- --check
cargo test -p im-core outbox --locked
```

Additional validation run after adding the async realtime ACK side-effect seam:

```bash
cargo fmt --all -- --check
cargo test -p im-core secure --locked
cargo test -p im-core outbox --locked
cargo test -p im-core e2ee --locked
cargo check -p im-core --locked
```

Additional validation run after deferring async direct secure send local
effects to the DB actor:

```bash
cargo test -p im-core direct_e2ee --locked
cargo check -p im-core --locked
```

Additional validation run after moving async direct secure prepare local prekey
work to the DB actor:

```bash
cargo fmt --all -- --check
cargo test -p im-core secure_direct_prepare_async --locked
cargo test -p im-core db_actor_prepare_direct_secure_prekeys --locked
cargo test -p im-core direct_secure --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
```

Additional validation run after adding actor/CAS-backed async direct follow-up
send:

```bash
cargo fmt --all -- --check
cargo test -p im-core async_direct_secure_sender --locked
cargo test -p im-core encrypt_follow_up_from_record --locked
cargo test -p im-core direct_secure --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
```

Additional validation run after adding actor/CAS-backed async direct receive:

```bash
cargo fmt --all
cargo test -p im-core async_direct_receive_processor --locked
cargo test -p im-core decrypt_follow_up_from_record --locked
cargo fmt --all -- --check
cargo test -p im-core direct_secure --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
```

Additional validation run after adding actor/CAS-backed async direct realtime
normalization:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo test -p im-core realtime_notification_client_async_normalizer --locked
cargo test -p im-core async_direct_receive_processor --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
```

Additional validation run after adding actor-backed async direct init receive:

```bash
cargo fmt --all
cargo test -p im-core async_direct_receive_processor_accepts_init_via_actor_and_consumes_opk --locked
cargo test -p im-core messages_read_async_projects_direct_init_without_legacy_fallback --locked
cargo test -p im-core db_actor_incoming_direct_init_session_saves_session_and_consumes_opk_atomically --locked
cargo test -p im-core async_direct_receive_processor --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
git diff --check
```

Additional validation run after adding actor-backed async direct init send:

```bash
cargo fmt --all
cargo test -p im-core async_direct_secure_sender_initializes_session_via_actor_and_async_transport --locked
cargo test -p im-core async_direct_secure_sender --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async pending-confirmation queue handling:

```bash
cargo fmt --all
cargo test -p im-core async_direct_secure_sender_queues_when_session_pending_confirmation --locked
cargo test -p im-core async_direct_secure_sender --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after wiring production direct realtime to the
async-first direct secure normalizer and adding async ACK/outbox side effects:

```bash
cargo fmt --all
cargo test -p im-core realtime_async_first --locked
cargo test -p im-core realtime_notification_client_async_normalizer_sends_ack_after_secure_init_control --locked
cargo test -p im-core realtime_notification_client_async_normalizer --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after decoupling `LocalStateDb` actor lifetime from
short-lived Tokio runtimes:

```bash
cargo fmt --all
cargo test -p im-core realtime_async_first --locked
cargo test -p im-core db_actor --locked
cargo test -p im-core realtime_notification_client_async_normalizer --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding the async group E2EE public send happy
path with actor-backed local projection:

```bash
cargo fmt --all
cargo check -p im-core --locked
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core public_group_e2ee_send_async --features group-e2ee --locked
cargo test -p im-core public_group_e2ee_send --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group secure status/prepare:

```bash
cargo fmt --all
cargo test -p im-core status_async_reports_ready_using_async_transport --features group-e2ee --locked
cargo test -p im-core secure_group_status_async_api_shape_is_available --locked
cargo check -p im-core --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after adding actor-backed async group repair summary
persistence:

```bash
cargo fmt --all -- --check
git diff --check
cargo check -p im-core --locked
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core secure_group_repair_async_api_shape_is_available --locked
cargo test -p im-core secure_group_repair_async_api_shape_is_available --features group-e2ee --locked
cargo test -p im-core lifecycle_create_prepares_delivers_finalizes_and_persists_summary --features group-e2ee --locked
cargo test -p im-core repair_processes_commit_notice_and_marks_delivered_without_public_raw_notice --features group-e2ee --locked
cargo test -p im-core repair_async_uses_async_transport_and_db_actor_summary --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
```

Additional validation run after adding async group E2EE leave-request lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_leave_request_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE create lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_create_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE add-member lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_add_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE remove-member lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_remove_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after removing the ineffective group service
`run_group_blocking` fallback:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE process-leave-request
lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_process_leave_request_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE update-key and
recover-member lifecycle:

```bash
cargo fmt --all
cargo check -p im-core --features group-e2ee --locked
cargo test -p im-core lifecycle_update_key_async --features group-e2ee --locked
cargo test -p im-core lifecycle_recover_member_async --features group-e2ee --locked
cargo test -p im-core group_e2ee --features group-e2ee --locked
cargo check -p im-core --locked
cargo fmt --all -- --check
git diff --check
```

Additional validation run after adding async group E2EE key-package publish:

```bash
cargo fmt --all
cargo test -p im-core publish_key_package_async_helper --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after moving async group E2EE key-package generation
to the blocking worker:

```bash
cargo fmt --all
cargo test -p im-core publish_key_package_async_helper --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after moving async group secure local MLS status to
the blocking worker:

```bash
cargo fmt --all
cargo test -p im-core status_async_reports_ready_using_async_transport --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after moving async group E2EE send encryption to the
blocking worker:

```bash
cargo fmt --all
cargo test -p im-core public_group_e2ee_send_async --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after moving async group E2EE state-ref local MLS
status to the blocking worker:

```bash
cargo fmt --all
cargo test -p im-core state_ref --features group-e2ee --locked
```

Additional validation run after moving async group E2EE lifecycle MLS
prepare/finalize/abort calls to the blocking worker:

```bash
cargo fmt --all
cargo test -p im-core lifecycle_ --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after moving async group E2EE repair MLS
status/finalize/process operations to the blocking worker:

```bash
cargo fmt --all
cargo test -p im-core repair --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
```

Additional validation run after adding worker-isolated async group E2EE incoming
decrypt helpers and wiring async group read callers to them:

```bash
cargo fmt --all
cargo test -p im-core incoming --features group-e2ee --locked
cargo test -p im-core groups_read_runtime_builds_get_list_members_and_messages_rpc_async --features group-e2ee --locked
cargo test -p im-core messages_read_runtime_builds_group_history_rpc --features group-e2ee --locked
```

Additional validation run after adding async-directory sender DID resolution for
explicit-transports direct realtime init normalization:

```bash
cargo fmt --all -- --check
cargo test -p im-core realtime_notification_client_async_normalizer --locked
cargo check -p im-core --locked
git diff --check
```

Additional validation run after adding async inbox/history direct pending-cipher
replay after direct init:

```bash
cargo fmt --all
cargo check -p im-core --locked
cargo test -p im-core messages_read_async_replays_pending_direct_cipher_after_init --locked
cargo test -p im-core async_direct_receive_processor_replays_pending_cipher_after_init --locked
cargo test -p im-core async_direct_receive_processor --locked
cargo test -p im-core messages_read_async_projects_direct --locked
cargo test -p im-core realtime_notification_client_async_normalizer --locked
```

Additional validation run after adding production async-first realtime
cross-notification direct pending-cipher replay:

```bash
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo test -p im-core realtime_async_first_projector_replays_pending_direct_cipher_after_init --locked
cargo test -p im-core realtime_async_first_projector --locked
cargo test -p im-core realtime_notification_client_async_normalizer --locked
cargo test -p im-core async_direct_receive_processor --locked
cargo test -p im-core messages_read_async_replays_pending_direct_cipher_after_init --locked
git diff --check
```

Additional validation run after wiring production async-first realtime group
incoming E2EE normalization to the worker-isolated async helper:

```bash
cargo fmt --all -- --check
cargo test -p im-core realtime_async_first_projector_uses_async_group_e2ee_normalizer --features group-e2ee --locked
cargo test -p im-core realtime_async_first_projector --features group-e2ee --locked
cargo test -p im-core realtime_notification_projection_async_replaces_wire_body_with_plaintext --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
cargo check -p im-core --locked
```

Additional validation run after wiring production async-first realtime group
E2EE notice processing to the async repair path:

```bash
cargo fmt --all -- --check
cargo test -p im-core group_e2ee_notice_notification --features group-e2ee --locked
cargo test -p im-core realtime_async_first_projector_uses_async_group_e2ee_normalizer --features group-e2ee --locked
cargo test -p im-core realtime_async_first_projector --features group-e2ee --locked
cargo check -p im-core --features group-e2ee --locked
cargo check -p im-core --locked
git diff --check
```

## Known Remaining Slice 09 Work

The following secure paths are still staged compatibility boundaries and must not
be treated as complete slice 09 coverage:

- `SecureService::direct().prepare/repair` still exist as sync compatibility
  methods. `prepare_async` now uses async session/file/transport APIs and the DB
  actor for local prekey persistence; `repair_async` uses the DB actor for local
  repair and the actor-backed async prepare path afterward.
- Direct secure init-session send now has an async public `send_async` path for
  no-existing-session sends, including async DID resolution, async prekey-bundle
  fetch, worker-isolated X3DH init creation, and actor-backed pending session
  save. Existing pending-confirmation local sessions now queue through the async
  public send path and actor-backed deferred outbox persistence. The legacy
  worker path still remains for other non-established local session states that
  have not yet been decomposed into actor-backed async operations.
- Direct realtime/ACK outbox flushing now uses the actor-backed async flush path
  through the async-first direct secure realtime projector. Legacy sync flush is
  still present for sync compatibility callers outside this production realtime
  path.
- Direct receive for async inbox/history now uses actor-backed init receive and
  actor/CAS established cipher processing, and direct pending-cipher replay
  within the same inbox/history batch after a later direct init creates the
  session. Direct realtime normalization now uses the actor/CAS async-first path
  in the production sync runner for established cipher notifications,
  cached-sender direct init notifications, secure ACK outbox flush, and
  secure-init ACK send. The production async-first projector retains its direct
  receive processor across notifications, so missing-session direct cipher
  notifications can replay after a later direct init and emit additional
  realtime events that are also projected to local state. The explicit-transports
  async normalizer now resolves uncached direct-init sender DID documents through
  the async directory transport with async local-cache fallback; the no-transport
  helper remains cache-only. Full replacement of the current sync realtime
  runner with a Tokio session/stream remains slice 10 work.
- Group incoming E2EE realtime notifications in the production async-first
  projector now use the worker-isolated async group normalization helper before
  event projection, and group E2EE notice notifications in that projector use the
  async group repair path. Sync realtime/group read compatibility callers still
  retain the existing sync helpers until their callers are migrated.
- Group E2EE status and repair now have public async methods and async
  network/file boundaries. Local MLS status/finalize/process operations in
  these async paths are worker-isolated, but still use the existing sync
  provider paths under the `group-e2ee` feature.
- Group E2EE member leave-request through `GroupService::leave_async` now uses
  async preflight/session/file/network boundaries and does not touch the local
  MLS provider. Cached owner leave returns the same public `invalid_input` owner
  leave rejection as the sync path and no longer uses a blocking-worker fallback.
- Group E2EE create through `GroupService::create_async` now uses async
  group-create network/projection boundaries and async E2EE create
  network/file/summary boundaries. Local MLS create/finalize is worker-isolated
  but still uses the existing sync provider.
- Group E2EE add-member through `GroupService::add_member_async` now uses async
  member resolution, group-add network/projection boundaries, async E2EE
  key-package lookup, async E2EE add network/file/summary boundaries, and no
  whole-operation `run_group_blocking` fallback. Local MLS add/finalize is
  worker-isolated but still uses the existing sync provider.
- Group E2EE remove-member through `GroupService::remove_member_async` now uses
  async member resolution, group-remove network/projection boundaries, async E2EE
  remove network/file/summary boundaries, and no whole-operation
  `run_group_blocking` fallback. Local MLS remove/finalize/abort is
  worker-isolated but still uses the existing sync provider.
- Group E2EE owner process-leave-request through
  `GroupService::process_e2ee_leave_request_async` now uses async member
  resolution, async `group.e2ee.process_leave_request`, the async group E2EE
  remove lifecycle, and actor-backed summary persistence. Local MLS
  remove/finalize is worker-isolated through that remove lifecycle but still
  uses the existing sync provider.
- Group E2EE update-member-key and recover-member through
  `GroupService::update_member_key_async` and `GroupService::recover_member_async`
  now use async member resolution, async key-package lookup, async
  `group.e2ee.update` / `group.e2ee.recover_member`, and actor-backed summary
  persistence. Local MLS update/recover/finalize is worker-isolated but still
  uses the existing sync provider.
- Group E2EE key-package publish through `GroupService::publish_key_package_async`
  now uses async session/file/network boundaries for
  `group.e2ee.publish_key_package`. Local MLS key-package generation is
  worker-isolated in the async publish path, but still uses the existing sync
  provider rather than `LocalStateDb`.
- Group E2EE incoming decrypt in async group history/read callers is
  worker-isolated, but still uses the existing sync provider. Production
  async-first realtime group incoming normalization now also uses the
  worker-isolated async helper inside the current sync runner boundary. Group
  E2EE notices in that same async-first projector now use async repair. Sync
  compatibility read/realtime callers still use the sync incoming
  decrypt/normalization and notice helpers until their callers are migrated.
- Group E2EE session/state load/save commands are not yet behind the
  `LocalStateDb` actor.
- Group secure status/prepare have async public methods and async network/file
  boundaries. Local MLS status is worker-isolated in the async status path, but
  it still uses the existing sync provider rather than `LocalStateDb`.
- Group E2EE public async send now uses async network/credential/local
  projection on the non-repair happy path. State-ref MLS status resolution and
  MLS encryption are worker-isolated, but still use the existing sync provider.
  Epoch-mismatch repair/retry still uses the existing sync provider/runtime
  boundaries.
- Direct receive stale-session protection is wired for async inbox/history
  direct init creation, established cipher messages, and same-batch pending
  cipher replay, and for production realtime established cipher normalization
  plus cross-notification pending direct cipher replay through the async-first
  projector.
- Crypto-heavy work is isolated for established async direct follow-up send,
  async direct init send, direct init receive in async inbox/history, and
  established async direct follow-up receive including same-batch pending replay,
  plus the staged legacy direct sender. Direct realtime init handling with
  explicit transports now resolves uncached sender DID documents asynchronously;
  the no-transport helper remains cache-only. Production direct realtime
  cross-notification pending replay and group incoming E2EE async normalization
  are covered by the async-first projector, but the full Tokio realtime runner
  replacement remains pending.
- CLI and Dart/FRB callers were not migrated in this increment. They remain
  scheduled for slices 11 and 12.
