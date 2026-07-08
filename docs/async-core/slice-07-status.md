# Slice 07 Status: Groups Async

## Status

Implemented in-place for `im-core` group service/runtime paths.

This slice keeps existing group DTOs, wire builders, group policy semantics,
service DID injection, compat modules, and sync public methods intact as staged
compatibility. New async group methods reuse the existing runtime modules and
projection helpers instead of introducing a parallel group SDK.

## Async Methods Added

Group service:

- `create_async`
- `join_async`
- `leave_async`
- `add_member_async`
- `remove_member_async`
- `update_profile_async`
- `update_policy_async`
- `update_async`
- `get_async`
- `list_async`
- `members_async`
- `messages_async`

Group runtime:

- `GroupLifecycleRuntime::create_async`
- `GroupLifecycleRuntime::join_async`
- `GroupLifecycleRuntime::leave_async`
- `GroupLifecycleRuntime::add_member_async`
- `GroupLifecycleRuntime::remove_member_async`
- `GroupLifecycleRuntime::update_profile_async`
- `GroupLifecycleRuntime::update_policy_async`
- `GroupReadRuntime::get_async`
- `GroupReadRuntime::list_async`
- `GroupReadRuntime::members_async`
- `GroupReadRuntime::messages_async`

## Implementation

- Group lifecycle and read runtimes now support `AsyncSessionProvider` and
  `AsyncAuthenticatedRpcTransport` while retaining the existing sync runtime
  methods as staged compatibility.
- Group lifecycle async credential loading uses `tokio::fs` for DID document and
  private key reads.
- Group member handle resolution uses `DirectoryService::lookup_handle_async`.
- Group local projection has actor-backed async helpers:
  - `project_group_snapshot_async`
  - `project_group_summaries_async`
  - `project_group_members_async`
  - `project_group_messages_async`
  - `project_group_left_async`
- Group cache reads use the DB actor through `cached_group_snapshot_async`.
- Async message conversations now refresh group summaries through
  `client.groups().list_async(...)` instead of the sync group list path.

## Tests Added

- Async group lifecycle payload test covering `group.create`, `group.join`, and
  `group.leave`.
- Async service DID validation test verifying missing `anp_service_did` returns
  `InvalidInput` with field `anp_service_did` and does not submit an RPC.
- Async group read payload test covering `group.get`, `group.list`,
  `group.list_members`, and `group.list_messages`.

Existing actor tests cover group projection commands through the DB actor and
existing group wire contract tests continue to cover the underlying payload
builders.

## Temporary Boundaries

- CLI and Dart/FRB callers are not migrated in this slice. They remain scheduled
  for slices 11 and 12.
- Sync group service methods still exist for staged compatibility and still use
  legacy sync paths until slice 13 cleanup/cfg-gating.
- Group E2EE full async transaction work remains scheduled for slice 09. In this
  slice, async group service paths preserve the existing unsupported behavior
  when `group-e2ee` is not enabled. When `group-e2ee` is enabled and a secure
  group lifecycle operation is required, the secure branch delegates to the
  existing sync-compatible group path through the blocking worker.
- Origin proof construction remains synchronous CPU work. It does not introduce
  new blocking I/O on the async path.

## Validation

Passed:

```bash
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo test -p im-core groups --locked
cargo test -p im-core group_lifecycle --locked
cargo test -p im-core group_contract --locked
cargo test -p im-core --test group_wire_contract --locked
cargo test -p im-core messages --locked
cargo check --workspace --locked
```

Notes:

- `cargo test -p im-core group_contract --locked` completed successfully but the
  filter matched zero tests in the current test names.
- `cargo test -p im-core group_wire --locked` also matched zero tests, so the
  actual integration test target was run explicitly with
  `cargo test -p im-core --test group_wire_contract --locked`; it passed 3
  tests.

## CLI/Dart Sync Status

No CLI, FRB, Dart, or Flutter files were changed in this slice. Because sync
compatibility methods remain available, existing upper layers continue to
compile. Full CLI async host migration remains slice 11. FRB/Dart async bridge
migration remains slice 12.

## Remaining Work

- Slice 08 should convert attachment upload/download to async streaming and
  remove full-file `Vec<u8>` transfer from LocalFile paths.
- Slice 09 should move secure direct/group E2EE DB and crypto-heavy paths behind
  the DB actor and worker model.
- Slice 10 should replace the blocking realtime runner with a Tokio
  task/session/stream model.
- Slice 13 should remove, cfg-gate, or document remaining sync blocking
  compatibility paths after upper layers are migrated.
