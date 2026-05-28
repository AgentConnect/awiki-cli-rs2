# Slice 03 Status: Identity, Bootstrap, and Auth Async Foundation

## Status

Slice 03 foundation is implemented.

This slice added async entrypoints for core open/client loading, identity
registry reads, handle registration/recovery network flows, bootstrap local
state setup, and auth/session operations. Existing sync public entrypoints are
kept as staged compatibility facades so CLI, Dart, and still-sync business
services continue to compile while later slices migrate domain APIs.

## Implementation

- Added `ImCore::open(config, paths).await`.
- Added `ImCore::client_async(selector).await`.
- Kept `ImCore::new` and `ImCore::client` as sync compatibility entrypoints.
  `ImCore::new` only validates config and constructs state; it does not perform
  filesystem, network, or SQLite I/O.
- Added async bootstrap methods:
  - `validate_paths_async`
  - `initialize_local_state_async`
  - `migrate_local_state_async`
- Added async identity registry methods:
  - `list_async`
  - `default_identity_async`
  - `resolve_async`
  - `register_handle_async`
  - `recover_handle_async`
  - `recover_handle_plan_async`
  - `plan_default_identity_change_async`
- Added async session/auth methods:
  - `FileSessionProvider::snapshot_async`
  - `AsyncSessionProvider::ensure_session`
  - `AsyncSessionProvider::refresh_session`
  - `AsyncSessionProvider::status`
  - `AuthService::login_async`
  - `AuthService::ensure_session_async`
  - `AuthService::refresh_session_async`
  - `AuthService::status_async`
- Added true async identity registration transport path using
  `AsyncRpcTransport` and `AsyncRestTransport`.
- Added true async identity recovery transport path using `AsyncRpcTransport`.
- Added async identity save helper that isolates current filesystem/index writes
  with the runtime blocking worker.
- Added tests for async registry, async core/client/auth/bootstrap,
  async registration, and async recovery.

## Compatibility Strategy

The final public API target remains async-first:

```rust
let core = ImCore::open(config, paths).await?;
let client = core.client(selector).await?;
```

During staged migration, Rust cannot expose both sync and async inherent methods
with the same name. This slice therefore uses:

```text
ImCore::open(...)
ImCore::client_async(...)
AuthService::*_async(...)
IdentityRegistry::*_async(...)
```

The legacy sync methods remain only to keep existing CLI, Dart bridge, tests,
and not-yet-migrated business services compiling. Slice 13 must either remove
them, cfg-gate them as test/internal compatibility, or explicitly document any
remaining stable sync accessor that is pure memory and not I/O.

## Temporary Blocking Boundaries

The following temporary boundaries remain because slice 04 DB actor and later
domain slices are not complete yet:

```text
CoreBootstrap::initialize_local_state_async
CoreBootstrap::migrate_local_state_async
  - use run_blocking around direct rusqlite schema/migration work.
  - must move to LocalStateDbActor after slice 04.

IdentityStore::save_identity_async
IdentityRecoveryRuntime::recover_with_local_finalize_async
  - use run_blocking around credential/index filesystem writes and local
    recovery SQLite merge/promote work.
  - credential filesystem writes remain identity-store owned.
  - SQLite merge/promote state must be revisited after slice 04 introduces the
    DB actor command surface.

IdentityRegistry::recover_handle_plan_async
  - currently delegates to the sync local planning implementation.
  - it only performs local credential/index planning and does not perform
    network I/O.
  - should be migrated to async filesystem helpers or a worker boundary when
    local identity planning is revisited.
```

These boundaries are explicit staging points, not the final async architecture.

## CLI/Dart Impact

No CLI, FRB, Dart, or Flutter files were changed in this slice. Existing sync
entrypoints remain available, so the workspace still checks.

CLI async host migration remains slice 11. FRB/Dart async bridge migration
remains slice 12.

## Validation

Passed:

```bash
cargo test -p im-core identity --locked
cargo test -p im-core auth --locked
cargo test -p im-core bootstrap --locked
cargo test -p im-core registry --locked
cargo test -p im-core recovery --locked
cargo test -p im-core recover_handle_async --locked
cargo check -p im-core --locked
cargo check --workspace --locked
cargo fmt --all -- --check
```

## Acceptance

```text
1. ImCore::open async entrypoint is available.
2. ImCore::client_async loads identity runtime through async registry helpers.
3. Identity registry read/default/resolve/default-change paths have async
   entrypoints.
4. Handle registration and recovery network flows use true async transport.
5. Auth/session public I/O paths have async entrypoints.
6. Bootstrap filesystem paths use tokio::fs where possible and isolate current
   SQLite schema/migration work with the blocking worker until slice 04.
7. Credential path and registry semantics remain unchanged.
8. CLI/Dart public callers continue to compile through compatibility sync
   entrypoints.
```

## Remaining Work

- Move bootstrap SQLite initialization/migration to `LocalStateDbActor` in
  slice 04.
- Revisit local recovery merge/promote SQLite operations after slice 04 adds DB
  actor commands.
- Migrate IdentityService profile/update/bind_contact methods in slice 06.
- Migrate CLI async host in slice 11.
- Migrate FRB/Dart async bridge in slice 12.
- Remove or cfg-gate production sync compatibility paths in slice 13.
