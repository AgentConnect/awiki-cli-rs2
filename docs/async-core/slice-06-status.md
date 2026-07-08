# Slice 06 Status: Directory, Profile, Content/Site/Email Async

## Status

Implemented in-place for `im-core` service/runtime paths.

This slice keeps existing DTOs, wire builders, compat modules, and sync public
methods intact as staged compatibility. New async methods reuse the existing
runtime modules and transport/session abstractions instead of introducing a
parallel SDK.

## Async Methods Added

Directory service:

- `resolve_peer_async`
- `lookup_handle_async`
- `public_profile_async`
- `save_contact_async`
- `contacts_async`
- `relation_status_async`
- `follow_async`
- `unfollow_async`
- `relationship_status_async`
- `followers_async`
- `following_async`

Identity service:

- `profile_async`
- `update_profile_async`
- `bind_contact_async`
- `bind_email_status_async`
- `replace_did_plan_async`

Content service:

- `create_page_async`
- `list_pages_async`
- `get_page_async`
- `update_page_async`
- `rename_page_async`
- `delete_page_async`

Site service:

- `get_root_async`
- `set_root_async`
- `list_pages_async`
- `get_page_async`
- `create_page_async`
- `update_page_async`
- `rename_page_async`
- `delete_page_async`

Email service:

- `account_async`
- `inbox_async`
- `read_async`
- `mark_read_async`
- `send_async`
- `download_attachment_async`
- `notifications_async`

## Implementation

- Added async runtime paths to the existing modules:
  - `internal::directory_runtime::DirectoryRuntime`
  - `internal::profile_runtime::ProfileReader`
  - `internal::relationship_runtime::RelationshipRuntime`
  - `internal::identity_bind_runtime::ContactBindingRuntime`
  - `content::service::ContentRuntime`
  - `site::service::SiteRuntime`
  - `internal::email_runtime::EmailRuntime`
- Async network paths use existing async internal traits:
  - `AsyncSessionProvider`
  - `AsyncRpcTransport`
  - `AsyncAuthenticatedRpcTransport`
  - `AsyncAuthenticatedRestTransport`
- Contact projection now has actor-backed async paths:
  - directory resolution/profile projection uses `project_directory_resolution_async`
  - save/list/relation-status use `LocalStateDb` contact commands
  - follow/unfollow local projection writes contact and relationship event rows via `LocalStateDb`
- Email local notifications now have an actor command:
  - `LocalStateDb::list_mail_notifications`
  - `email_runtime::notifications_async` uses the actor instead of opening SQLite directly
- Message async handle resolution now uses `DirectoryService::lookup_handle_async` in:
  - `send_async`
  - `history_with_metadata_async`
- Existing sync methods remain as compatibility. Their direct SQLite and sync
  transport usage is still scheduled for slice 13 cleanup/cfg-gating after CLI
  and Dart callers are migrated.

## Content/Site/Email Scope

Content and site network paths were modified to add async methods. Wire builders,
endpoint paths, method names, params, and normalizers were not changed.

Email network paths were modified to add async methods. Email notification local
cache reads were also moved behind the DB actor for the async path.

## Temporary Boundaries

- CLI and Dart/FRB callers are not migrated in this slice. They remain scheduled
  for slices 11 and 12.
- Sync service methods still exist and still use legacy sync transport/local DB
  paths for staged compatibility.
- `update_profile_async` still updates identity display-name projection through
  the existing `IdentityStore` helper, isolated with the blocking worker.
- `replace_did_plan_async` is worker-isolated because the existing plan helper
  does read-only SQLite inspection and filesystem metadata checks.
- E2EE/secure message flows remain sync-compatible boundaries for slice 09.

## Validation

Passed:

```bash
cargo fmt --all -- --check
cargo check -p im-core --locked
cargo test -p im-core directory --locked
cargo test -p im-core profile --locked
cargo test -p im-core relationships --locked
cargo test -p im-core relationship --locked
cargo test -p im-core content --locked
cargo test -p im-core site --locked
cargo test -p im-core email --locked
cargo test -p im-core messages --locked
```

## CLI/Dart Sync Status

No CLI or Dart files were changed in this slice. Because sync compatibility
methods remain available, existing upper layers are not forced onto async yet.
Full CLI async host migration remains slice 11. FRB/Dart async bridge migration
remains slice 12.

## Remaining Work

- Slice 07 should convert group service/runtime paths to async and replace the
  remaining sync group-list fallback in async conversations.
- Slice 08 should convert attachments to streaming async transfer.
- Slice 09 should move secure direct/group E2EE DB and crypto-heavy paths behind
  the DB actor and worker model.
- Slice 13 should remove, cfg-gate, or document the remaining sync blocking
  compatibility paths after upper layers are migrated.
