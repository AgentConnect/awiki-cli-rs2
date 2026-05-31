# Slice 04 Status: LocalStateDbActor

## Status

In progress with the actor core, bootstrap/schema commands, message projection
commands, group projection/cache command surface, and contact store command
surface implemented.

This is the first slice 04 increment. It establishes a cloneable local state DB
handle backed by a single blocking actor loop that owns the `rusqlite`
connection. Bootstrap async schema initialization and owner identity backfill now
go through typed actor commands instead of opening SQLite directly from the async
bootstrap path.

Message actor commands exist for the core persistence/read projection
operations that slice 05 needs. Current sync message service call sites still
use the legacy direct SQLite path because they cannot await the actor until the
message service itself becomes async in slice 05.

Group actor commands now cover the read/write SQL helper surface that slice 07
needs. Current sync group runtime call sites still use the legacy direct SQLite
path because they cannot await the actor until the group service/runtime becomes
async.

Contact/realtime/secure projection commands are not fully migrated yet and
remain later slice 04/09/10 work.

Contact actor commands now cover the store helpers that slice 06 needs for
directory save/list/status and relationship projection. Current sync directory
and contact-store call sites still use direct SQLite because they cannot await
the actor until directory/profile/relationship services become async.

## Implementation

- Added `crates/im-core/src/internal/local_state/actor.rs`.
- Added `LocalStateDb`, a cloneable async handle.
- The actor:
  - opens one writable SQLite connection with existing `open_writable`;
  - therefore preserves WAL, foreign keys, busy timeout, schema version, and
    migration behavior;
  - owns the connection on a blocking actor loop;
  - serializes accepted commands;
  - returns deterministic `LocalStateUnavailable` when closed;
  - supports explicit shutdown for tests.
- Added typed actor commands:
  - `current_schema_version`
  - `store_messages`
  - `upsert_contact`
  - `get_contact_by_did`
  - `get_current_contact_by_handle`
  - `list_contacts`
  - `list_contact_dids_for_message_history_recovery`
  - `resolve_contact_handle_by_did`
  - `list_dids_by_handle`
  - `append_relationship_event`
  - `classify_mark_read_ids`
  - `mark_messages_read`
  - `list_conversations`
  - `upsert_group`
  - `replace_group_members`
  - `mark_group_left`
  - `get_group_snapshot`
  - `list_cached_group_members`
  - `list_group_messages`
  - `list_active_group_refs`
  - `backfill_owner_identity_ids`
  - `shutdown`
- Added an `ImCoreInner` shared actor handle, initialized lazily behind a Tokio
  mutex so clone/concurrent callers share the same actor.
- Updated async bootstrap:
  - `initialize_local_state_async` gets schema version via `LocalStateDb`.
  - `migrate_local_state_async` performs owner identity backfill via
    `LocalStateDb`.
- Sync bootstrap compatibility methods still use the existing direct SQLite path
  for now.
- Added actor tests covering message store, mark-read classification, local
  mark-read update, and conversation projection through existing SQL helpers.
- Added actor tests covering group upsert, member replacement, group message
  projection, cached group reads, active group refs, and mark-left behavior
  through existing SQL helpers.
- Added actor tests covering contact upsert, did/handle lookup, contact list,
  message-history recovery candidates, handle resolution, handle DID history,
  and relationship event append through existing contact-store helpers.

## Validation

Passed:

```bash
cargo test -p im-core db_actor --locked
cargo test -p im-core local_state --locked
cargo test -p im-core bootstrap --locked
cargo check -p im-core --locked
cargo check --workspace --locked
cargo fmt --all -- --check
```

## Grep Fence

Command:

```bash
rg -n "rusqlite::Connection|open_writable|Connection::open" crates/im-core/src/messages crates/im-core/src/groups crates/im-core/src/directory crates/im-core/src/realtime crates/im-core/src/attachments crates/im-core/src/secure
```

Current output:

```text
crates/im-core/src/directory/service.rs: contact_store direct SQLite access
crates/im-core/src/realtime/runner.rs: local_state direct SQLite access and test opens
crates/im-core/src/secure/service.rs: local_state direct SQLite access
crates/im-core/src/internal/group_runtime/cache.rs: sync group cache direct SQLite access
crates/im-core/src/internal/group_runtime/projection.rs: sync group projection direct SQLite access
crates/im-core/src/internal/contact_store/**: contact store direct SQLite access
crates/im-core/src/internal/secure_direct/**: secure direct SQLite helpers/direct access
crates/im-core/src/internal/group_e2ee/**: group E2EE SQLite helpers/direct access
```

These are expected remaining slice 04/09/10 migration targets and are not yet
accepted as final exceptions.

## Remaining Work

- Add slice-05 async message service/runtime methods that call the new actor
  commands instead of the current sync local projection functions.
- Extend actor commands for outgoing group projection touches and attachment
  message projection once slice 05/08 call sites are async.
- Migrate group cache/projection call sites to the group actor commands once
  slice 07 makes the group runtime async.
- Migrate directory/contact-store call sites to contact actor commands once
  slice 06 makes directory/profile/relationship services async.
- Add actor-backed E2EE session/outbox operations for slice 09.
- Migrate realtime local projection to the actor in slice 10.
- Decide how sync compatibility paths are removed or cfg-gated in slice 13.
