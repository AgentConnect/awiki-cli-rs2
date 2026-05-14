# Verification Reports

Store command transcripts and summary reports for parity, structure, Rust unit tests, ANP SDK tests, and `awiki-system-test` runs here.

## 2026-05-15 Site RPC Wire Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test site_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test site_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/site -count=1
```

Result: passed.

Scope:

- Added a split `site` module for the pure Go
  `internal/site/{types.go,service.go}` RPC contract.
- Preserved `/site/rpc`, `/user-service/did-auth/rpc`, `get_root`,
  `set_root`, `list_pages`, `get_page`, `create_page`, `update_page`,
  `rename_page`, and `delete_page` method names and transport profiles.
- Preserved live-service domain normalization through the existing
  Go-equivalent `config::normalize_did_domain` helper, trim-only slug
  validation, explicit empty body allowance, params, summaries, and
  identity/root/page/list/rename/delete result shapes.
- Kept the existing `site` dry-run CLI boundary unchanged: dry-run remains
  trim-only for domain display and does not apply live bare-domain
  normalization, while the new site service wire tests cover stricter
  live-service rules.

Boundary note: this slice does not wire non-dry-run site commands, implement
`identity.RemoteClient`, bootstrap the auth session, refresh DID-auth JWTs,
perform HTTP transport, map site service errors into CLI exit codes, or run
site lifecycle system tests. Those remain in the shared authsdk/session plus
Rustls HTTP client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Content RPC Wire Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test page_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/content -count=1
```

Result: passed.

Scope:

- Added a split `content` module for the pure Go
  `internal/content/{types.go,service.go}` RPC contract.
- Preserved `/content/rpc`, `/user-service/did-auth/rpc`, `create`, `list`,
  `get`, `update`, `rename`, and `delete` method names and transport profiles.
- Preserved service-level slug/title/update-field/visibility validation,
  visibility normalization, params, summaries, and identity/page/list result
  shapes.
- Kept the existing `page` dry-run CLI boundary unchanged: dry-run remains
  permissive for raw visibility values and empty update plans, while the new
  content service wire tests cover the stricter live-service rules.

Boundary note: this slice does not wire non-dry-run page commands, implement
`identity.RemoteClient`, map content service errors into CLI exit codes, refresh
DID-auth JWTs, perform HTTP transport, or run content/page lifecycle system
tests. Those remain in the shared authsdk/session plus Rustls HTTP client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Authsdk JSON-RPC Wire/Result Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/authsdk -count=1
```

Result: passed.

Scope:

- Added a split `authsdk::wire` module for the pure Go
  `internal/authsdk/session.go` JSON-RPC and response helpers.
- Preserved the fixed JSON-RPC envelope (`jsonrpc=2.0`, `id=req-1`, method,
  params), RPC error precedence and data preservation, plain JSON body decode,
  HTTP status error body trimming, first-header-value flattening, and JSON
  content-type helper.
- Added a local `Session::ensure_jwt_from_result` helper for the non-network
  part of Go `EnsureJWT`: remember scope, persist non-empty body access token,
  fallback to a token captured from response headers before body decode,
  fallback to stored JWT, then emit the Go missing-token error string.

Boundary note: this slice does not implement real HTTP request execution,
`Headers`, `ChallengeHeaders`, 401 retry, proxy/timeout/TLS behavior, live
`DoJSONRPC`, live `DoJSON`, live `EnsureJWT`, or non-dry-run
`id refresh-token`. Those remain in the later shared authsdk/session plus
Rustls transport lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Mail Remote Wire Contract Slice

Local Rust verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test mail_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test mail_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/mail -count=1
```

Result: passed.

Scope:

- Added a split `mail::wire` module for Go `internal/mail/client.go` and the
  remote-method portions of `internal/mail/service.go`.
- Preserved `/mail/rpc`, `mail.getInbox`, `mail.getMessage`, `mail.markRead`,
  `mail.getMailbox`, `mail.getAttachment`, and `mail.send` request contracts.
- Preserved default inbox folder/limit, Go transport profiles, JSON params,
  validation errors, summary strings, and `ServiceError` display mapping for
  authsdk RPC and HTTP errors.
- Kept `mail inbox/read/mark-read/account/send/attachment download` non-dry-run
  execution at the existing deferred boundary.
- Native Agent read-only parity review found no blocking mismatch. It noted
  that `identity_name` correctly remains outside the RPC params, matching Go's
  service-layer identity resolution before the remote call.

Boundary note: this slice does not implement `NewClient`, HTTP execution,
DID-auth session construction, JWT refresh, CA bundle handling, attachment file
writes, or live local mail-service system tests. Those remain in the later
shared authsdk/session plus Rustls HTTP client lane.

No dependency was added. Cargo manifests and lockfile were unchanged; this
slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
`native-tls`, bundled OpenSSL, or ANP SDK network/default features. TLS policy
remains Rustls-first and unchanged.

## 2026-05-15 Local CLI Validation Selector Slice

Local Rust and offline system-test verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-system-test && AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/awiki-cli-rs2 uv run pytest -q tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors tests_v2/multi_tenant/test_awiki_cli_tenant_config.py::test_awiki_cli_derives_anp_service_defaults_from_service_base_url tests_v2/multi_tenant/test_awiki_cli_tenant_config.py::test_awiki_cli_id_create_uses_tenant_did_domain_but_platform_message_service tests_v2/cli/test_awiki_cli_direct_local.py::test_awiki_cli_msg_and_attachment_commands_validate_local_arguments tests_v2/id/test_identity_cli.py::test_id_profile_set_rejects_conflicting_body_sources
```

Result: passed.

Scope:

- Preserves Go `msg attachment download` target validation for missing
  `--with/--group` and conflicting `--with` plus `--group` before the deferred
  non-dry-run attachment transfer boundary.
- Adds `id profile set` to CLI dispatch/schema and preserves the Go
  `--markdown` plus `--markdown-file` conflict error before resolving service
  execution.
- Provides a Go-shaped `id profile set --dry-run` plan for local contract
  coverage while keeping real `did.profile.update_me` execution deferred to
  the authsdk/Rustls user-service slice.

No dependency was added. The slice is local CLI validation only and does not
introduce HTTP/TLS clients, WebSocket crates, OpenSSL, `native-tls`, bundled
OpenSSL, or new platform/system dependencies. TLS policy remains Rustls-first
and unchanged.

## 2026-05-15 Authsdk Local Token Session Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/authsdk -run 'TestCaptureTokenPersistsOnlyConfiguredScopes|TestCaptureTokenStillAcceptsLegacyAuthorizationResponseHeader' -count=1
```

Result: passed.

Scope:

- Adds a narrow Rust `anpsdk` facade for the Go ANP module path/version and the
  DID-WBA auth types needed by the first session slice.
- Translates the local bearer token scope and persistence behavior from Go
  `internal/authsdk/session.go`: remembered hostname scopes, bearer seeding,
  token capture from `Authentication-Info`, legacy `Authorization: Bearer`
  response compatibility, current-JWT updates only for persistent scopes,
  persistent callback invocation, persistent-scope token clearing, 401 retry
  policy delegation, and Go-shaped HTTP/RPC error strings.

Boundary note: this slice intentionally does not implement service transport,
`Headers`, `ChallengeHeaders`, `DoJSONRPC`, `EnsureJWT`, `DoJSON`,
non-dry-run `id refresh-token`, real identity-store JWT persistence, or the full
Go `internal/anpsdk/registry.go` alias surface. Those remain in later
authsdk/Rustls service slices.

No dependency was added. The code reuses the existing local `../anp/rust`
dependency with `default-features = false`; it does not enable ANP `network`,
add `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, or bundled
OpenSSL. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Local V0 To V1 Apply Wiring Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestLoadLegacySettingsRejectsSplitServiceURLs|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Wires the already translated local v0->v1 apply composition into the default
  `Migration::apply` implementation.
- Lets `UpgradeIfNeeded` run local v0->v1 config/schema/import work and then
  continue into v1->v2 cleanup before stopping at the still-deferred v2->v3
  DID replacement boundary.
- Adds if-needed coverage for Go's legacy `config.json` migration path:
  legacy config is converted to canonical `config.yaml`, removed, journaled,
  and meta-stamped through v1->v2.
- Keeps imported legacy k1 identities at the explicit v0->v1 deferred boundary
  because Go immediately performs service-backed k1->e1 DID replacement after
  importing them.
- Preserves the existing backup reuse and lock-release behavior while the
  migration loop advances past v0->v1.

Boundary note: this slice does not implement service-backed k1->e1 DID
replacement for imported legacy identities, v2->v3 existing-workspace DID
replacement, rollback, or full awiki-system-test migration acceptance.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing local config, identity, bundled SQLite import/schema, journal,
backup, and lock helpers. It does not introduce HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock crate, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Journal Phase Loop Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestUpgradeIfNeededCleansLegacySkillArtifactsForExistingWorkspace|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation from backup setup into the
  migration journal phase loop.
- Reuses the existing journal `upgrade_id` when present; otherwise creates a
  new Go-style compact timestamp ID.
- Saves `checking`, `applying`, and `validating` journal phases for each
  planned migration, with the migration name, version range, backup directory,
  start time, and app version.
- Calls migration `is_done`; if the migration is not already complete, saves
  `applying` and calls `apply`; then saves `validating` and calls `validate`.
- Stamps `meta.json` after each successful migration with the target schema
  version, app version, RFC3339-seconds update time, last upgrade ID, backup
  dir, and accumulated warnings.
- Updates `Context.current_meta` after each successful migration and clears the
  journal after the full plan succeeds.
- Splits `workspace_upgrade_if_needed_*` tests into
  `workspace_upgrade_if_needed_contract.rs`, keeping all Rust test files under
  the default 1200-line structure limit.

Boundary note: this slice does not complete all migrations. v0->v1 default
apply remains deferred at
`workspace_0_to_1_bootstrap_local_state_upgrade`, and v2->v3 k1->e1 DID
replacement remains deferred at
`workspace_2_to_3_replace_existing_k1_handle_dids`. Rollback and
service-backed identity replacement remain later slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing upgrade modules, the documented direct file-lock FFI, and the
already approved `rusqlite + bundled` SQLite backup path. It does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock crate, or new SQLite dependencies.
TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Backup Setup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock|TestCreateBackupCopiesWorkspaceState' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation past the lock/reinspect
  boundary into backup setup.
- Reloads the journal after the lock-inside second inspect, matching Go's
  `LoadJournal` call before backup selection.
- Reuses `journal.backup_dir` when present and records it in
  `Context.backup_dir`.
- Creates a new workspace backup with the existing translated backup helper
  when no journal backup is present, then records the created directory in
  `Context.backup_dir`.
- Keeps the current deferred execution error before the journal phase loop, so
  this slice does not execute migration `is_done`/`apply`/`validate` yet.

Boundary note: this slice does not implement journal phase writes,
migration-loop `is_done`/`apply`/`validate`, meta stamping after each migration,
rollback, v0->v1 default apply wiring, or v2->v3 k1->e1 DID replacement.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
reuses existing upgrade modules, the already documented direct file-lock FFI,
and the already approved `rusqlite + bundled` SQLite backup path. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock crate, or new SQLite dependencies.
TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace UpgradeIfNeeded Lock Pre-Migration Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock' -count=1
```

Result: passed.

Scope:

- Advanced the Go `UpgradeIfNeeded` translation past the initial
  no-op/current-version preflight and into the pre-migration lock boundary.
- Reused the translated `AcquireFileLock` helper before any real migration
  execution, matching Go's lock-before-backup/migration ordering.
- Preserved lock-held failure text:
  `workspace upgrade is already running: <path>`.
- Preserved stale-journal clearing for empty/latest workspaces before the lock.
- Added lock-inside second `inspect`/current-meta refresh before planning real
  migration execution.
- Verified the lock anchor is written before deferred real migration execution
  and the OS lock is released when the function returns with the current
  deferred execution error.

Boundary note: this slice still returns the existing deferred execution error
when a real migration plan is required. It does not implement backup
creation/reuse, journal phase writes, migration-loop `is_done`/`apply`/
`validate`, meta stamping after each migration, rollback, v0->v1 default apply
wiring, or v2->v3 k1->e1 DID replacement.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing upgrade modules and the already documented direct file-lock FFI.
It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk
session, platform service-manager, filesystem-copy, file-lock crate, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v1->v2 Legacy Cleanup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli migration_v1_to_v2 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededCleansLegacySkillArtifactsForExistingWorkspace|TestUpgradeIfNeededImportsLegacyWorkspace' -count=1
```

Result: passed.

Scope:

- Added `migration_v1_to_v2.rs` for Go
  `workspaceV1ToV2Migration.Apply` and `cleanupLegacySkillArtifacts`.
- Preserved the missing-context guard:
  `workspace upgrade context is required`.
- Removes both Go legacy skill install paths under `.openclaw`.
- Preserves platform artifact gates and command argv for macOS LaunchAgent,
  Linux `systemctl --user`, and Windows `schtasks`, using test injection so
  unit tests do not call host service managers.
- Preserves `OPENCLAW_WORKSPACE`, `XDG_CONFIG_HOME`, and `LOCALAPPDATA`
  fallback behavior for heartbeat/service paths.
- Removes the legacy heartbeat section only when the marked section references
  `awiki-agent-id-message`, preserves unrelated content, and collapses extra
  blank lines like Go.
- Wires direct `Migration::apply` for 1->2 and Go's no-op v1->v2
  `validate`; `UpgradeIfNeeded` still defers real migration phase execution.

Boundary note: this slice does not implement full `UpgradeIfNeeded` phase
execution, journal phase writes, backup/rollback, meta stamping through the
migration loop, v0->v1 default apply wiring, or v2->v3 k1->e1 DID replacement.
The platform service actions remain external command invocations, matching Go;
no service-manager crate or platform library is linked.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses only standard-library filesystem/process APIs and existing upgrade
structures. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, filesystem-copy, file-lock, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Durablefs Directory Sync Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli durablefs --locked
cargo +1.79.0 test -p awiki-cli --test config_writer_contract config_writer_uses_go_style_tempfile_permissions_and_cleanup --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/durablefs ./internal/config -run 'TestSyncDirectory|Test.*Durable|Test.*Write|Test.*Config' -count=1
```

Result: passed. The Rust `durablefs::sync_directory` tests cover the Go
contract for existing directories and missing-directory platform behavior:
non-Windows fails on a missing directory, while Windows is compiled as a no-op.
The focused config writer test proves the durable config write path still
cleans up temporary files and preserves Go-style Unix permissions while using
the shared helper. Go reference tests for `internal/durablefs` and focused
`internal/config` write behavior also passed.

Scope:

- Go `internal/durablefs/syncdir_unix.go` non-Windows directory open + sync.
- Go `internal/durablefs/syncdir_windows.go` intentional Windows no-op.
- Existing Rust config writer parent-directory sync call now delegates to the
  shared `durablefs` helper and preserves the Go `sync config dir` error prefix.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/durablefs.rs` is 76 lines,
`crates/awiki-cli/src/config/write.rs` is 445 lines, and
`crates/awiki-cli/tests/config_writer_contract.rs` is 264 lines. No file-size
exception is needed.

Boundary note: this slice does not implement Go `runtime/openclawnotify` route
registry writes or `internal/upgrade` filesystem helpers that also call
`durablefs.SyncDirectory`; those remain later file-level slices. Windows no-op
behavior is represented with conditional compilation, but Windows runtime
execution still requires Windows CI or manual evidence before full
cross-platform runtime parity claims.

No dependency was added. Cargo manifests and lockfile were unchanged; TLS and
SQLite dependency decisions remain unchanged.

## 2026-05-14 OpenClaw Route Registry Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli openclaw_routes --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/runtime/openclawnotify -run 'TestResolveRouteInput|TestAddAndRemoveRoutePersistRegistry|TestLoadRoutesMissingFileReturnsEmpty' -count=1
cd ../awiki-cli && go test ./internal/cli -run TestRuntimeDryRunPlansCoverStableActions -count=1
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_config_commands_work \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_validates_inputs_and_supports_dry_run \
    -q
```

Result: passed. Rust route helper tests, full runtime contract tests, full
`awiki-cli` crate tests, structure check, build, Go route registry tests, Go CLI
dry-run tests, and two existing runtime host-notify system-test selectors all
passed.

Scope:

- Go `internal/runtime/openclawnotify/routes.go` local route registry behavior:
  `RoutesPath`, `ResolveRouteInput`, `ParseSessionKey`, `NormalizeRoute`,
  `LoadRoutes`, `AddRoute`, `RemoveRoute`, and `WriteRoutes`.
- Go `internal/cli/runtime_host_notify_routes.go` route `add/list/remove`
  local CLI boundary and stable dry-run plan shapes.
- Rust `host_notify_config_view.routes` now loads the persisted route registry
  instead of returning a hard-coded empty list.

Structure note: the route registry was added as
`crates/awiki-cli/src/runtime/openclaw_routes.rs` rather than expanding
`runtime/mod.rs`. The changed Rust source and test files remain below the
default 1200-line source limit; no file-size exception is needed.

Boundary note: Go non-dry-run route add sends one OpenClaw confirmation
webhook after persisting a new route. This slice intentionally does not
translate `internal/runtime/openclawnotify/webhook.go`; Rust records a warning
on new route add and defers confirmation sending to a future Rustls-first HTTP
client slice. No HTTP/TLS, WebSocket, OpenSSL, `native-tls`, or bundled OpenSSL
dependency was added.

Dependency audit showed only the existing Rustls/ring update path and the
approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path. No OpenSSL/native-tls, new HTTP client, WebSocket, or platform service
dependency was introduced.

## 2026-05-14 Listener Status Files And Saved Status Merge Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli listener --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract listener_status_merges_saved_sessions_and_host_notify_state --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/runtime/listener -run 'TestSessionWarnings|TestHasDisconnectedSessions|TestMergeSavedRuntimeStatus' -count=1
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_listener_config_set_requires_flags_and_supports_dry_run \
    tests_v2/runtime/test_runtime_cli.py::test_runtime_host_notify_config_commands_work \
    -q
```

Result: passed. Rust listener helper tests, focused and full runtime contract
tests, full `awiki-cli` crate tests, structure check, build, Go listener helper
tests, dependency audit, and two runtime system-test selectors all passed.

Scope:

- Go `internal/runtime/listener/types.go`: local listener `Status`,
  `SessionStatus`, and `HostNotifyStatus` JSON shape.
- Go `internal/runtime/listener/files.go`: `listener.pid`,
  `listener.status.json`, and `listener.expected-boot-id` helpers, including
  Unix `0600` writes for helper-owned files.
- Go `internal/runtime/listener/status_helpers.go`: disconnected-session
  warning text and disconnected-session detection.
- Narrow Go `internal/runtime/listener/manager.go`: saved runtime status merge
  behavior, including PID mismatch skip, saved sessions/boot/start metadata,
  saved host-notify last error, and running-only host-notify override.
- Rust `runtime listener status` now merges saved `listener.status.json` data
  into the public CLI envelope.

Structure note: the listener status/files implementation lives in
`crates/awiki-cli/src/runtime/listener.rs` instead of expanding
`runtime/mod.rs`; changed files remain below the default 1200-line source
limit. No file-size exception is needed.

Boundary note: this slice does not translate Go `listener/service.go`,
`server.go`, `wsclient.go`, `host_notify.go`, or platform `sysproc_*` code.
Listener lifecycle commands continue to use the existing Rust local-state
facade rather than adding systemd/launchd/Windows-service dependencies.
No HTTP/TLS, WebSocket, authsdk session, OpenSSL, `native-tls`, or platform
service-manager dependency was added.

Dependency audit showed only the existing Rustls/ring update path and the
approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path. No OpenSSL/native-tls, new HTTP client, WebSocket, or platform service
dependency was introduced.

## 2026-05-14 CLI Error Hint Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli error_hints --locked
cargo +1.79.0 test -p awiki-cli internal_anyhow --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/cli -run 'TestIsWindowsDirSyncCompatibilityError|TestRuntimeExitRefinesWindowsDirSyncCompatibilityHint|TestConfigCommandExitRefinesWindowsDirSyncCompatibilityHint' -count=1
```

Result: passed. The Rust helper matches Go's Windows directory-sync
compatibility detector: it requires `Access is denied` plus one of
`sync config dir`, `sync route registry dir`, or `sync dir`, and it ignores
normal permission errors and unrelated parse failures. The `internal_anyhow`
focused tests prove the refined hint reaches the current Rust workspace/config
error envelope path while ordinary permission errors keep the existing doctor
fallback hint.

Scope:

- Go `internal/cli/error_hints.go` `windowsDirSyncCompatibilityHint`.
- Go `refineWorkspaceWriteHint` and `isWindowsDirSyncCompatibilityError`
  string-matching behavior.
- Rust generic workspace/config error hint refinement through `internal_anyhow`.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/app.rs` is 882 lines and
`crates/awiki-cli/src/app/error_hints.rs` is 72 lines. No file-size exception is
needed.

Boundary note: this slice does not implement full Go `runtimeExit`,
`configCommandExit`, workspace-upgrade execution, platform service-manager
behavior, route registry writes, or Windows-specific system service behavior.
It only ports the shared hint classifier and wires it into the current Rust
generic workspace/config error path.

No dependency was added. Cargo manifests and lockfile were unchanged; TLS and
SQLite dependency decisions remain unchanged.

## 2026-05-14 Buildinfo Contract Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli buildinfo --locked
cargo +1.79.0 test -p awiki-cli --test core_contract version_reports_current_build_info --locked
cargo +1.79.0 test -p awiki-cli --test core_contract status_reports_phase_version_paths_state_and_config --locked
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/buildinfo
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/core/test_basic_commands.py::test_core_query_commands_return_structured_success \
    tests_v2/core/test_output_contracts_cli.py::test_output_formats_and_jq_filters_follow_cli_contracts \
    -q
```

Result: passed. The Rust buildinfo helper now preserves the Go metadata
defaults for `version`, `commit`, `build_date`, and `cgo_enabled`, returns an
independent owned snapshot, and renders Rust target names as Go-compatible
runtime names for visible `goos`/`goarch` fields. The `version` and `status`
core contracts verify the eight public JSON fields exposed by Go:
`version`, `commit`, `build_date`, `go_version`, `goos`, `goarch`, `compiler`,
and `cgo_enabled`. The doctor contract also passed, keeping the build check
compatible with the translated report shape. The accepted system-test selector
covered real CLI `status`, `version`, `config show`, `doctor`, representative
pretty/table/JQ output paths, and reported 2 passed, failed 0, skipped 0, in
0.37s. System-test configuration context: `AWIKI_CLI_UNDER_TEST=rust`,
`AWIKI_CLI_RUST_REPO=../awiki-cli-rs2`; no service URLs or live external
services were required for these core selectors.

Scope:

- Go `internal/buildinfo/buildinfo.go` metadata constants and `Current()`
  snapshot behavior.
- Go `internal/buildinfo/buildinfo_test.go` injected metadata and independent
  snapshot behavior, adapted to Rust with an injectable constructor.
- Core CLI `version` and `status` output contracts for buildinfo JSON fields.

Structure note: changed Rust files remain below the default 1200-line source
limit: `crates/awiki-cli/src/buildinfo.rs` is 174 lines and
`crates/awiki-cli/tests/core_contract.rs` is 589 lines. No file-size exception
is needed.

Boundary note: this slice does not implement release pipeline metadata wiring,
`build.rs`, CI injection, package metadata changes, update policy changes, or
system-test assertion changes. Future release/packaging work must intentionally
wire `AWIKI_CLI_VERSION`, `AWIKI_CLI_COMMIT`, `AWIKI_CLI_BUILD_DATE`, and
`AWIKI_CLI_CGO_ENABLED` equivalents without changing the public JSON field
surface.

No dependency was added. Cargo manifests and lockfile were unchanged. The
dependency policy remains Rustls-first for TLS: future HTTP/WebSocket/authsdk
slices must not prefer OpenSSL, `native-tls`, or bundled OpenSSL over
Rustls-backed options, and any non-Rustls exception must be separately recorded
with failed Rustls parity evidence.

## 2026-05-14 Identity Handle Input Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test identity_contract identity_handle_input_helpers_match_go_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract full_handle --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --test msg_contract --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/identity
cd ../awiki-cli && go test ./internal/message -run TestMessageServiceHelperContracts
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
    tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
    tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
    tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
    tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
    tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
    tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
    tests_v2/core/test_output_contracts_cli.py \
    -q
```

Result: passed. The focused Rust identity helper contract covers bare handle
normalization with configured DID domain, full handle normalization, `wba://`
handle normalization, DID input rejection for handle input, shared bare-handle
completion pass-through/expansion behavior, and full-handle derivation from
non-`user` DID path prefixes. The focused `full_handle` run covers Go's
`Manager.Load` backfill contract: handle-path DIDs persist `full_handle` into
both identity payload and index, while `user` DIDs do not backfill a full handle.
Focused Rust `msg` and `group` contracts also passed after those handlers
switched to the shared identity helper. Full Rust package tests, structure check,
binary build, Go `internal/identity` reference tests, Go message helper reference
tests, dependency audit, and the accepted-scope system regression also passed.
System regression context: `AWIKI_CLI_UNDER_TEST=rust`,
`AWIKI_CLI_RUST_REPO=../awiki-cli-rs2`; no external service URLs were required
for these local/dry-run selectors. Result: 11 passed, failed 0, skipped 0.

Scope:

- Added `crates/awiki-cli/src/identity/handle_input.rs` for Go
  `internal/identity/handle_input.go`.
- Preserved Go `NormalizeHandleInput` behavior: trim, lowercase, reject DID
  values, strip `wba://`, split explicit domains, normalize trailing-dot
  domains, and require `did_domain` for bare handles.
- Preserved Go `CompleteBareHandle` behavior: empty input returns empty string,
  DID values pass through with trimmed spelling, explicit full handles pass
  through, and bare or `wba://bare` handles expand to canonical full handles.
- Moved stored handle field derivation out of `identity/did.rs` and into the
  handle input module so identity store behavior shares the same normalization
  rules.
- Preserved Go `Manager.Load` stored-handle backfill side effects for handle-path
  DIDs: normalized handles/full handles are written back to `identity.json` and
  `index.json` only when non-empty.
- Updated `msg` and non-E2EE `group` dry-run handlers to call
  `identity::complete_bare_handle`, eliminating duplicated local completion
  logic.

Structure note: changed files remain well below the default 1200-line Rust
source limit: `identity/handle_input.rs` is 205 lines, `identity/did.rs` is 124
lines, `identity/store.rs` is 459 lines, `app/msg_handlers.rs` is 515 lines,
`app/group_handlers.rs` is 438 lines, and `tests/identity_contract.rs` is 456
lines. No file-size exception is needed.

Boundary note: this is a local helper consolidation and CLI dry-run normalization
cleanup. It does not implement remote identity register/bind/recover/profile/
resolve, non-dry-run `id refresh-token`, non-dry-run `id replace-did`, message
RPC execution, group RPC execution, authsdk session refresh, HTTP/WebSocket
transport, MLS provider execution, or cache mutation.

No dependency was added. Cargo manifests and lockfile were unchanged, and this
slice does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, crypto,
MLS, or platform service-manager dependencies.

## 2026-05-14 Trace/Transport Foundation Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test traceutil_contract --test transportcfg_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Go reference verification:

```bash
cd ../awiki-cli
go test ./internal/traceutil ./internal/transportcfg
```

Result: passed. Focused Rust contract tests: 9 passed. Full Rust package
verification: 118 local tests passed. Go reference tests for both source
packages passed. Structure check reported no undocumented Rust files over 1200
lines; current slice files are `traceutil.rs` 298 lines,
`transportcfg.rs` 320 lines, `traceutil_contract.rs` 123 lines, and
`transportcfg_contract.rs` 128 lines. Build passed. Dependency audit showed
only the existing Rustls/ring update path and approved `rusqlite + bundled`
SQLite path; this slice added no dependency and did not introduce OpenSSL,
`native-tls`, `reqwest`, `hyper`, or WebSocket crates.

Scope:

- Go `internal/traceutil/trace.go` pure timing trace helpers:
  `AWIKI_CLI_TRACE_TIMING`, run/phase/fallback capture, pretty Chinese output,
  known label humanization, and duration formatting.
- Go `internal/transportcfg/config.go` pure resolver behavior:
  bridge/HTTP/profile default durations, environment variable names, Go-style
  positive duration/int fallback, and profile timeout fallback.

Boundary note: Go `transportcfg.NewHTTPClient` is not translated in this
foundation slice. It requires a shared Rustls-first HTTP client decision for
TLS roots, custom CA bundle, HTTP/2, response-header timeout, idle pooling, and
TLS 1.2 minimum behavior. This slice adds no dependency; OpenSSL,
`native-tls`, and bundled OpenSSL remain disallowed as first-choice TLS paths.

No `awiki-system-test` selector is required for this internal-only foundation
slice because no new public CLI/service path is wired to these modules yet.

## 2026-05-14 Message Signed Wire Params Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 59 local tests after this slice. Focused
`message_contract` now has 10 tests, including signed direct send params,
signed direct attachment manifest params, signed group attachment manifest
params, and DID-document verification of each origin proof. Structure check
reported no undocumented Rust files over 1200 lines; `message/wire.rs` is 204
lines, `message/attachment.rs` is 394 lines, and `tests/message_contract.rs`
is 516 lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

Scope:

- Go `BuildDirectSendRPCParams` signed direct text params.
- Go `contentTypeForMessageType` normalization: trim/lowercase input,
  `attachment_manifest` maps to the attachment manifest content type, only
  `event` maps to `application/json`, and `json` falls back to `text/plain`.
- Go `BuildDirectAttachmentSendRPCParams` signed direct attachment manifest
  params.
- Go `BuildGroupAttachmentSendRPCParams` signed group attachment manifest
  params.
- Auth wrapper uses `anp-rfc9421-origin-proof-v1` and `origin_proof`, omits
  `sender_proof`, and verifies through the local ANP Rust SDK.

Boundary note: this slice still does not implement `authsdk` session/JWT
refresh, HTTP transport, WebSocket proxy transport, attachment transfer
execution, local cache mutation, secure direct E2EE, or group mutation RPC
execution. It adds no new dependency and keeps TLS/HTTP dependency selection
deferred to the shared Rustls service-client slice.

Accepted-scope system regression:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed, 37 tests. This regression selector covers the existing public
CLI surface; the signed wire builders are verified by Rust contract tests
because they are not yet wired into service-backed CLI execution.

## 2026-05-14 Message RFC9421 Origin-Proof Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 57 local tests after this slice. Focused
`message_contract` has 8 tests, including RFC9421 origin-proof generation,
Go-compatible verification-method selection, `anp-rfc9421-origin-proof-v1`
auth shape, canonical request digest comparison, and DID-document verification
through the local ANP Rust SDK. Structure check reported no undocumented Rust
files over 1200 lines; `message/proof.rs` is 72 lines and
`tests/message_contract.rs` is 385 lines. Dependency audit remained unchanged:
only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled
SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or
platform service dependency was added.

Scope:

- Go `internal/message/auth.go` local private-key material loading behavior.
- Go `verificationMethodID` behavior: first `authentication` string, falling
  back to first `verificationMethod.id`.
- Go `internal/message/proof.go` `buildOriginProof` auth fields:
  `contentDigest`, `signatureInput`, and `signature`.
- Serialization of the signed auth value as
  `scheme: anp-rfc9421-origin-proof-v1` plus `origin_proof`.

Boundary note: this slice does not implement `authsdk` session/JWT refresh,
HTTP transport, WebSocket proxy transport, attachment transfer execution, local
cache mutation, secure direct E2EE, or group mutation RPC execution. It adds no
new dependency and keeps TLS/HTTP dependency selection deferred to the shared
Rustls service-client slice.

Accepted-scope system regression:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed, 37 tests. This regression selector covers the existing public
CLI surface; the new origin-proof helper is verified by Rust contract tests
because it is not yet wired into service-backed CLI execution.

## 2026-05-14 Message Pure Foundation Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. Rust tests: 55 local tests after this slice, including 6
focused `message_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; the largest new message source file is
`message/attachment.rs` at 328 lines and `tests/message_contract.rs` is 268
lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

System-test verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q

AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: passed. Focused core output selector: 2 passed. Accepted-scope
regression selector: 37 passed.

Scope:

- Pure Go `internal/message/wire.go` request builders for inbox, direct
  history, and mark-read RPC params.
- Direct text payload shape and message-type-to-content-type mapping needed by
  later signed direct sends.
- Pure attachment create-slot, commit-object, download-ticket params,
  attachment manifest construction, manifest JSON encoding, and message-content
  attachment selection.
- DID document attachment service selection from local JSON values, including
  profile/security-profile filtering and priority ordering.
- WebSocket HTTP/cache fallback warning text normalization.

Boundary note: RFC9421 origin-proof signing, private-key/auth context loading,
`authsdk` session refresh, HTTP transport, WebSocket proxy transport, local
cache mutation, secure direct E2EE, and group mutation RPC execution remain
deferred. The local ANP Rust SDK exposes proof helpers, but proof signing needs
a separate auth/proof slice so optimization and dependency decisions are not
mixed into this pure foundation translation.

## 2026-05-14 Site Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test site_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 49 local tests after this slice, including 5
focused `site_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; `app.rs` is 877 lines and the new
`app/site_handlers.rs` is 286 lines. Dependency audit remained unchanged: only
the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled
SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or
platform service dependency was added.

Scope:

- `site root get/set` and `site page list/get/create/update/rename/delete`
  command catalog entries and parser dispatch.
- Go dry-run plan shapes from `internal/cli/site.go`, including `/site/rpc`
  metadata, RPC method names, request fields, domain/slug trim behavior, and
  markdown-file byte counting.
- Go CLI boundary behavior for required flags, explicit body-source
  requirements, markdown source conflicts, local file-read errors, and
  non-dry-run deferral.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.11s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: non-dry-run tenant site RPC and full `internal/site/service.go`
remain deferred. Those require shared authsdk/session plus Rustls HTTP client
translation and service-backed tests. OpenSSL bundled is not the preferred TLS
fallback and would require a separate documented exception.

## 2026-05-14 Msg Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test msg_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 44 local tests after this slice, including 6
focused `msg_contract` tests. Structure check reported no undocumented Rust
files over 1200 lines; `app.rs` stayed under the limit after `msg` moved into
`app/msg_handlers.rs`. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present. No OpenSSL/native-tls, Rustls, HTTP, WebSocket, or platform service
dependency was added.

Scope:

- `msg send`, `msg attachment download`, `msg inbox`, `msg history`,
  `msg mark-read`, and `msg secure status/init/repair/failed/retry/drop`
  command catalog entries and parser dispatch.
- Go dry-run plan shapes for direct send, group send, attachment send/download,
  inbox/history/mark-read, and secure direct commands.
- Go CLI boundary behavior for bare-handle completion, local `--text-file`
  reads, attachment metadata, `--type`/`--file` conflict handling, missing text
  handling, and Cobra-like required-flag errors.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.12s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Boundary note: non-dry-run message RPC, WebSocket proxy transport, attachment
transfer, secure direct E2EE execution, and `tests_v2/cli/test_awiki_cli_direct_local.py`
remain deferred. Those require shared authsdk/session plus Rustls HTTP/WS and
E2EE provider slices. OpenSSL bundled is not the preferred TLS fallback and
would require a separate documented exception.

## 2026-05-14 Page Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test page_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 38 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines; `app.rs` is 914 lines and
the new `app/page_handlers.rs` is 224 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present. No OpenSSL/native-tls, Rustls, HTTP, or
platform service dependency was added.

Scope:

- `page create/list/get/update/rename/delete` command catalog entries and
  parser dispatch.
- Dry-run plans from Go `internal/cli/page.go`, including `/content/rpc`
  metadata, request fields, trim behavior, and update `changed_fields`.
- Local dry-run-boundary validation from Go: create `slug`/`title` and
  markdown source conflict; markdown-file reads are local so `body_bytes`
  matches Go.
- Go dry-run boundary preservation: visibility choices are not validated and
  empty `page update --dry-run` is not rejected, because Go applies those checks
  in the non-dry-run content service.

Accepted-scope system verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/core/test_output_contracts_cli.py -q
```

Result: `2 passed in 0.12s`.

Broader accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.82s`.

Boundary note: `tests_v2/page/test_page_cli.py` was not run against Rust for
acceptance because it covers non-dry-run page CRUD through live `/content/rpc`.
That remains deferred to the shared authsdk/session plus Rustls HTTP client
slice.

## 2026-05-14 Identity Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc '
```

Result: passed. Rust tests: 13 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls path was present.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  -q
```

Result: `7 passed in 0.22s`.

## 2026-05-14 Runtime/Host-Notify Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 17 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/runtime/test_runtime_cli.py -q
```

Result: `9 passed in 0.70s`.

Accepted-scope regression verification:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.24s`.

Boundary note: running the full `tests_v2/id/test_identity_cli.py` file still
fails on remote identity registration/recovery/profile/replace-did selectors.
Those selectors exercise later authsdk/user-service work and are not claimed by
the local identity plus runtime/config slices.

## 2026-05-14 Mail Local Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 21 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

Scope:

- `mail inbox/read/mark-read/account/send/attachment download/notify` command
  catalog entries and parser dispatch.
- Dry-run plans and local validation messages from Go `internal/cli/mail.go`.
- Local `mail notify` SQLite query and notification normalization from Go
  `internal/mail/service.go` and `internal/store/query.go`.

Boundary note: non-dry-run remote mail RPC is not claimed by this slice. It
requires the shared authsdk/session plus Rustls HTTP/TLS dependency decision and
must be verified with local mail-service system tests in a later slice.

Accepted-scope regression verification was rerun after the mail slice:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.20s`.

## 2026-05-14 Config Set Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 23 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls or platform service-manager dependency was added.

Scope:

- `config set --did-domain` command catalog entry and parser dispatch.
- Go `config_set_did_domain` dry-run plan shape.
- Persistent `services.did_domain` write using the existing config writer.
- Go-compatible permissive DID-domain normalization for the tested command
  cases, including trailing-dot trimming and URL/path rejection.
- Side-effect guard that the command does not create runtime socket or pid
  artifacts.

No new dependency was added. Full YAML/config parser/serializer parity remains
a later slice. Go `write.go` durable-write mechanics are covered by the later
config writer helper/durable-write slice.

Accepted-scope regression verification was rerun after the config-set slice:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  -q
```

Result: `30 passed in 1.24s`.

## 2026-05-14 Update/Upgrade Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd'
```

Result: passed. Rust tests: 28 passed. Structure check reported no
undocumented Rust files over 1200 lines. Dependency audit still only showed the
expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite
path; no OpenSSL/native-tls, HTTP/TLS, or platform service-manager dependency
was added.

Scope:

- `upgrade` command catalog entry and parser dispatch.
- Go `upgrade` data shape for current/latest/min-supported version,
  strict-disabled, dev-build, newer/block flags, metadata source, update check
  status, upgrade hint, and `upgrade_attempted`.
- Cache-only update metadata loading from
  `<workspace>/cache/update/metadata.json`.
- Env/config controls:
  `AWIKI_CLI_UPDATE_CACHE_ONLY`, `AWIKI_CLI_UPDATE_CACHE_TTL`,
  `AWIKI_CLI_DISABLE_STRICT_VERSION`, and
  `update.disable_strict_version`.
- Go-compatible dev-build and prerelease version comparison behavior for the
  verified slice.
- Go npm packaging surface copied into `package.json`, `scripts/install.js`,
  and `scripts/run.js`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest tests_v2/update -q
```

Result: `5 passed in 0.72s`.

Boundary note: registry fetch/writeback and the root update-policy preflight
guard remain deferred translation tasks. They require the shared Rustls HTTP
dependency decision and broader CLI-root parity tests.

## 2026-05-14 Identity/Group Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test identity_contract --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 30 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit still
only showed the expected `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, or platform
service-manager dependency was added.

Scope:

- Public `id replace-did` command catalog entry, parser dispatch, dangerous
  short text, side-effect flag, and Go dry-run plan/warning shape.
- `group create` and `group update` command catalog entries plus dry-run
  plans with Go PascalCase request fields.
- Go pointer semantics for group dry-run policy fields: absent bool/int flags
  render as JSON null; explicitly changed flags render concrete bool/int values.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `2 passed in 0.14s`.

Boundary note: non-dry-run DID replacement and group RPC/lifecycle behavior
remain deferred translation tasks. They require authsdk/message-service/store
rebind work and should be implemented in dedicated slices with service-backed
system tests.

## 2026-05-14 Group Non-E2EE Dry-Run Lifecycle Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 31 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present.

`group_contract` now covers the non-E2EE `internal/cli/group.go` dry-run surface
beyond create/update:

- `group get` and alias `group show` with canonical command path and
  `action: group.show`.
- `group join`, `group add`, `group remove` and alias `group kick`, and
  `group leave` request plans with Go PascalCase fields.
- Membership mutation `member_handle` completion for bare handles and no
  completion for explicit DID values.
- `group list`, `group members`, and `group messages` default/explicit limit
  behavior, including `Cursor` and `Skip: 0` in message-list plans.
- Schema children and alias metadata for the added non-E2EE group commands.

Go parity probes:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group show --dry-run --group did:wba:awiki.ai:groups:demo:e1_group
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group kick --dry-run --group did:wba:awiki.ai:groups:demo:e1_group --member bob
```

Result: Rust alias command paths and plans matched the observed Go behavior:
`show` renders command `awiki-cli group get`, and `kick` renders command
`awiki-cli group remove` with `action: group.kick`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice still does not implement non-dry-run group RPC, group
attachments, or `group e2ee ...`. Those remain dedicated service-backed and
MLS/provider translation tasks.

## 2026-05-14 Group E2EE Dry-Run CLI Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test group_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls'
```

Result: passed. Rust tests: 33 local tests after this slice. Structure check
reported no undocumented Rust files over 1200 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present.

Scope:

- `group e2ee status` dry-run provider diagnostics, profile/security profile,
  hidden-discovery guard, and MLS data directory.
- `group e2ee publish-key-package` dry-run plan, including `--purpose`,
  `--recovery`, `--device`, `--group`, and `--contract-test`.
- `group e2ee pending` and `group e2ee repair` dry-run plan shape.
- `group e2ee process-leave-request`, `recover-member`, hidden `update-key`,
  and hidden `rejoin` dry-run plan shape and schema metadata.
- `group e2ee` command tree schema children and hidden/side-effect metadata.

Go parity probes:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee status --dry-run --group did:wba:example.com:groups:demo:e1_group
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee publish-key-package --dry-run --group did:wba:example.com:groups:demo:e1_group --purpose update --device bob-main --contract-test
AWIKI_CLI_WORKSPACE_HOME_DIR="$(mktemp -d)" \
  go run ./cmd/awiki-cli group e2ee rejoin --dry-run --group did:wba:example.com:groups:demo:e1_group --member bob
```

Result: Rust dry-run outputs matched the observed Go command path and plan
shape for the probed E2EE status, KeyPackage publish, and rejoin commands.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Boundary note: this slice does not execute `anp-mls`, call hidden P6
message-service RPCs, or validate MLS storage/security boundaries. Those remain
dedicated group-E2EE implementation and focused system-test tasks.

## 2026-05-14 Group Base/Local Wire Builder Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The full `awiki-cli` crate test suite reported 65 tests
passing after this slice. Structure check reported no undocumented Rust files
over 1200 lines; the new `message/group_wire.rs` is 527 lines and
`tests/message_group_wire_contract.rs` is 402 lines. Dependency audit remained
unchanged: only the approved `rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`
bundled SQLite path was present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `BuildGroupCreateRPCParams` request params, including service target,
  `group.create` origin proof, display-name validation, default policy,
  permissions, and E2EE policy security-profile override.
- Go `BuildGroupGetInfoRPCParams` unsigned base profile request params.
- Go `BuildGroupJoinRPCParams`, `BuildGroupAddRPCParams`,
  `BuildGroupRemoveRPCParams`, `BuildGroupLeaveRPCParams`,
  `BuildGroupUpdateProfileRPCParams`, and `BuildGroupUpdatePolicyRPCParams`
  signed control params.
- Go `BuildGroupSendRPCParams` signed group send params, message id generation,
  message-type content-type mapping, and original text body preservation.
- Go local `BuildGroupGetRPCParams`, `BuildGroupListRPCParams`,
  `BuildGroupMembersRPCParams`, and `BuildGroupMessagesRPCParams`, including
  local profile metadata, default limits, cursor-to-`since_seq`, `skip`
  omission when zero, and no generated local operation fields.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice constructs request params only. It does not execute
non-dry-run group RPC calls, refresh JWT sessions, select HTTP/WebSocket
transport, mutate local message cache, upload/download attachments, or run MLS
group E2EE provider logic. Those remain dedicated authsdk/message-service and
group-E2EE wire/service slices.

## 2026-05-14 Group E2EE Wire Builder Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_group_wire_contract --locked
cargo +1.79.0 test -p awiki-cli --test message_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The full `awiki-cli` crate test suite reported 72 tests
passing after this slice. Structure check reported no undocumented Rust files
over 1200 lines; `message/group_e2ee_wire.rs` is 845 lines and
`tests/message_group_e2ee_wire_contract.rs` is 526 lines. Dependency audit
remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `BuildGroupE2EECreateRPCParams`, including service target,
  `group.e2ee.create`, `group-e2ee` security profile, and state-ref propagation.
- Go `BuildGroupE2EEAddRPCParams`, `BuildGroupE2EERemoveRPCParams`,
  `BuildGroupE2EELeaveRequestRPCParams`, and `BuildGroupE2EELeaveRPCParams`,
  including membership commit bodies, reason/request ID handling, actor/subject
  fields, and hidden control-plane security-profile differences.
- Go `BuildGroupE2EESendRPCParams`, including
  `application/anp-group-cipher+json`, caller-provided operation/message IDs,
  and opaque cipher sanitization.
- Go KeyPackage request builders:
  `BuildGroupE2EEPublishKeyPackageRPCParams`,
  `BuildGroupE2EEGetKeyPackageRPCParams`,
  `BuildGroupE2EEGetRecoveryKeyPackageRPCParams`, and
  `BuildGroupE2EEGetUpdateKeyPackageRPCParams`, including transport-protected
  service targets, device defaults, and public KeyPackage sanitization.
- Go recovery/update/notice/head builders, including P4-field avoidance,
  recovery/update key package id selection, nested package purpose defaults,
  notice limit capping and ID trimming, and head state-ref shape.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.83s`.

Boundary note: this slice constructs hidden E2EE request params only. It does
not execute `anp-mls`, call hidden P6 message-service APIs, refresh JWT
sessions, select HTTP/WebSocket transport, mutate cache, or validate live MLS
storage/security behavior. Those remain dedicated group-E2EE service/provider
translation tasks.

## 2026-05-14 Doctor Local Diagnostics Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed. The focused doctor contract test covers the Go 10-check report
shape, counts/summary precedence, empty and initialized workspaces, invalid ANP
service configuration, SQLite contact-handle binding diagnostics, and a fake
`anp-mls system version --json-in -` compatibility probe with scoped MLS state.
The full `awiki-cli` crate test suite reported 76 tests passing after this
slice. Structure check reported no undocumented Rust files over 1200 lines;
`doctor/mod.rs` is 1002 lines and `tests/doctor_contract.rs` is 271 lines.
Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

Scope:

- Go `Report`, `Check`, and `Counts` JSON contract.
- Go check order: `build`, `config_file`, `environment`, `anp_service`,
  `runtime`, `identity_store`, `sqlite`, `anp_mls`, `workspace_upgrade`, and
  `legacy_paths`.
- Local diagnostics through already translated config, identity, store, runtime,
  and legacy-scan modules.
- External `anp-mls` health probe through `std::process::Command`; no Rust
  dependency was added.

Boundary note: this slice does not implement real platform service-manager
status, auth/session checks, HTTP/WebSocket transport, MLS provider execution,
full Go YAML parse-error parity, or full `upgrade.Inspect` meta/journal
semantics. Those remain separate parity slices.

## 2026-05-14 Config Writer Helper Slice

Local Rust verification in `awiki-cli-rs2`:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test config_writer_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract --locked
cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|tungstenite|websocket'
```

Result: passed for the focused helper tests plus existing core/runtime contract
tests. The full `awiki-cli` crate suite reported 82 tests passing after this
slice. Structure check reported no undocumented Rust files over 1200 lines;
after the follow-up file split, `config/mod.rs` is 829 lines,
`config/write.rs` is 453 lines, and `tests/config_writer_contract.rs` is 264
lines. Dependency audit remained unchanged: only the approved
`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg` bundled SQLite path was
present, with no OpenSSL/native-tls/HTTP/TLS client path.

The helper contract test covers Go `internal/config/write.go` behavior for
schema-version stamping, missing-config no-op schema upgrades, active identity
trimming, runtime/listener field writes, DID-domain normalization, host-notify
enabled false persistence, `webhook` alias normalization, OpenClaw hook/token
writes, Hermes notify/deliver/secret writes, one-shot Hermes host-notify setup,
and legacy webhook double-write behavior.

The same test file also covers the Go-style durable write path: config writes
use a same-directory `.config-*.tmp` temporary file, leave no temp file after a
successful replacement, create a missing config directory, and on Unix produce
`0600` config files plus `0700` newly-created config directories. The writer
uses standard-library file sync and parent-directory sync on Unix, with the same
intentional Windows parent-directory sync no-op as Go `internal/durablefs`.

System verification in `awiki-system-test`:

```bash
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
PYTHONDONTWRITEBYTECODE=1 \
uv run --no-sync pytest \
  tests_v2/core/test_basic_commands.py \
  tests_v2/core/test_output_contracts_cli.py \
  tests_v2/debug/test_debug_cli.py::test_debug_db_query_rejects_unsafe_sql_and_supports_table_output \
  tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
  tests_v2/id/test_identity_cli.py::test_id_create_list_current_use_and_status \
  tests_v2/id/test_identity_cli.py::test_id_create_and_use_support_dry_run_and_argument_validation \
  tests_v2/id/test_identity_cli.py::test_id_use_unknown_identity_returns_not_found \
  tests_v2/id/test_identity_cli.py::test_id_refresh_token_public_command_dry_run_exposes_did_auth_refresh \
  tests_v2/id/test_identity_cli.py::test_id_replace_did_public_command_dry_run_warns_about_danger_and_backup \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_imports_flat_legacy_identity \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_all_imports_flat_and_indexed_legacy_identities \
  tests_v2/id/test_identity_cli.py::test_id_import_v1_reports_missing_legacy_layout \
  tests_v2/runtime/test_runtime_cli.py \
  tests_v2/update \
  tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_group_commands_support_dry_run_for_extended_policy_fields \
  -q
```

Result: `37 passed in 1.84s`.

Scope:

- Helper-level config mutators only.
- No new crate dependency.
- Existing direct-helper boundaries are preserved where Go helpers differ from
  CLI validation, for example host-notify sink validation remains at the CLI
  boundary and the helper maps only the legacy `webhook` alias.

Boundary note: this slice does not claim full `yaml.v3` parser/serializer
parity, Hermes CLI command wiring, Hermes bridge management, listener refresh
orchestration, or platform service-manager behavior. Windows replacement
behavior still needs Windows CI/manual evidence before the durable writer is
claimed as cross-platform runtime complete.

## 2026-05-14 Update Registry Fetch/Writeback Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli update --locked
cargo +1.79.0 test -p awiki-cli --test update_contract --locked
cd ../awiki-cli && go test ./internal/update
```

Result: passed. The Rust update-focused run covered 9 update module tests plus
the existing filtered contract tests; `update_contract` reported 3 passed. The
Go reference `internal/update` tests passed.

Live smoke on this host, using the configured proxy/VPN environment:

```bash
AWIKI_CLI_WORKSPACE_HOME_DIR=/tmp/awiki-cli-rs2-update-smoke-$$ \
cargo +1.79.0 run -p awiki-cli --bin awiki-cli --locked -- \
  upgrade --dry-run --format json
```

Result: passed. Output included `update_metadata_source: "network"`,
`update_check_status: "ok"`, `latest_version: "1.0.16"`, and
`min_supported_version: "1.0.16"`.

Dependency audit:

```bash
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Result: the update slice introduced `rustls`, `rustls-webpki`,
`rustls-pki-types`, and `webpki-roots`. No OpenSSL, `native-tls`, `reqwest`, or
`hyper` path was present. The audit also showed the already-approved bundled
SQLite path (`rusqlite -> libsqlite3-sys -> cc/pkg-config/vcpkg`) and the
Rustls crypto provider build path (`ring -> cc`). `ring -> cc` is recorded in
`docs/dependency-decisions.md` as native build surface for Rustls, not as
OpenSSL/native-tls or a host TLS dependency.

Scope:

- Go `internal/update.fetchFromRegistry*` registry order and first-success
  behavior: npmjs first, npmmirror fallback.
- 3-second connect/read/write timeout on the blocking GET path.
- HTTP 200-only success, required npm `version`, optional
  `awikiCli.minSupportedVersion`, and combined all-registry error text.
- `CheckFresh` prefers network over fresh cache.
- `AWIKI_CLI_UPDATE_CACHE_ONLY` avoids network and uses cached metadata.
- Successful network fetch writes `<cache>/update/metadata.json`; on Unix the
  update cache directory is `0700` and the metadata file is `0600`.
- Network failure falls back to the cached metadata snapshot with
  `metadata_source = "cache_stale"` when one is available.
- `HTTP_PROXY`/`HTTPS_PROXY` and `NO_PROXY` are supported for the narrow update
  fetch path, including HTTP CONNECT for HTTPS registries.

Boundary note: this is a narrow blocking GET helper for npm registry metadata,
not the shared authsdk/service HTTP or WebSocket client. Broader mail, page,
site, message, attachment, and group service transports still need dedicated
Rustls-backed client decisions and service-backed tests.

## 2026-05-14 Store Shared Helpers Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_helpers_contract --test store_import_contract --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused helper contract covers direct/group/unknown
`MakeThreadID` behavior and UTC RFC3339 timestamp shape, and module-level unit
tests cover Go second-precision timestamp formatting, normalization helpers,
`defaultInt64Ptr`-style fallback behavior, and generated `local-<nanos>` IDs.
The import and rebind contract tests were re-run because they are the current
consumers of the extracted helpers. The Go reference `internal/store` tests
passed and remain the parity source for the helper behavior.

Scope:

- Added a split `store/helpers.rs` module for the already-translated Go
  `internal/store/helpers.go` primitives used by legacy import and rebind:
  UTC timestamps, thread ID construction, owner/credential trimming, nullable
  string/bool/int/float coercions, metadata trimming, bool/default helpers, and
  generated `local-<nanos>` IDs.
- Exported only `make_thread_id` and `now_utc` publicly because current tests
  and later modules need those stable helper contracts outside the store module;
  other helpers stay crate-internal through the `store::helpers` module.
- Reduced `crates/awiki-cli/src/store/import.rs` from 1197 to 1117 lines and
  kept `crates/awiki-cli/src/store/rebind.rs` small before starting the larger
  recover-merge translation.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

No `awiki-system-test` selector was run for this slice because these helpers
are store internals and are not newly exposed through a public CLI path. The
later CLI integration slice must use subprocess coverage when public commands
begin exercising recover/replace store merge behavior.

## 2026-05-14 Store Rebind Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused Rust contract has 5 tests covering missing DB soft
no-op behavior, existing DB missing-table error propagation, full owner-DID
rebind plus E2EE cleanup, `UPDATE OR IGNORE` conflict behavior, and
empty/same-owner no-op rules. The Go reference `internal/store` tests passed
and remain the parity source for `RebindLocalIdentityState`, `RebindOwnerDID`,
and `ClearOwnerE2EEData`.

Scope:

- Added a split `store/rebind.rs` module instead of expanding
  `store/import.rs`, which remains 1197 lines and close to the default
  1200-line review threshold.
- Preserved Go's local post-replace-DID store helper behavior:
  missing database returns zero counts without opening/creating SQLite, and the
  missing-DB `store_rebind` map has the same five legacy table keys as Go.
- Preserved the no implicit migration boundary: an existing SQLite file without
  the expected store tables returns the SQLite missing-table error instead of
  creating schema inside rebind.
- Preserved Go `RebindOwnerDID` behavior: trim old/new owners, no-op for empty
  or equal owners, transactionally count and `UPDATE OR IGNORE` `messages`,
  `contacts`, `contact_handle_bindings`, `relationship_events`, `groups`, and
  `group_members`, returning pre-update old-owner row counts.
- Preserved Go `ClearOwnerE2EEData` behavior: trim owner, no-op for empty owner,
  then count and delete old-owner rows from `e2ee_outbox` and `e2ee_sessions`
  while preserving new-owner E2EE rows.

Boundary note: this is a store-only slice. It does not implement non-dry-run
`id replace-did`, identity key replacement, `did-auth.replace_did`, remote
authsdk/session calls, or upgrade K1 replacement integration. Those remain
later identity/authsdk/service slices.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

No `awiki-system-test` selector was run for this slice because the translated
helpers are not yet wired into a public non-dry-run CLI command. The later
identity/authsdk integration slice must add subprocess coverage when
`id replace-did` begins calling these store helpers.

## 2026-05-14 Store Recover Merge Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_recover_merge_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/store
```

Result: passed. The focused Rust contract has 6 tests covering missing
database soft no-op, existing empty database schema creation,
target-owner-only migration and old-owner E2EE cleanup, conflict merge
algebra, current contact-handle normalization by latest timestamp then DID
DESC, and global `relationship_events.event_id` conflict behavior. Full Rust
crate tests, structure check, binary build, Go `internal/store` tests, and
dependency audit also passed.

Scope:

- Added split recover-merge modules for Go `internal/store/recover_merge.go`:
  `store/recover_merge.rs` for orchestration, `store/recover_merge/records.rs`
  for row normalization and merge algebra, and `store/recover_merge/sql.rs`
  for query/upsert/count/delete helpers.
- Preserved Go's zero-count map shape for store merge and E2EE cleanup, and
  the missing-DB no-op that does not open or create SQLite.
- Preserved Go's existing-empty-DB behavior for this path: open the DB,
  `ensure_schema`, then perform a zero-count transaction.
- Preserved normalized old-owner filtering: trim, skip empty owners, skip the
  normalized new owner, and deduplicate while preserving input order.
- Preserved one transaction across message/contact/binding normalization,
  current-handle normalization, relationship event, group, member, and E2EE
  cleanup steps.
- Preserved table-specific merge behavior: message self-DID/thread remapping,
  incoming non-empty strings, max sequence/counts, later or earlier timestamp
  rules, bool OR, contact handle normalization, global relationship event
  conflict on `event_id`, group/member self-DID remapping, and old-owner E2EE
  delete-only cleanup.

Structure note: the Rust implementation is split into 354/794/617-line source
files, all under the default 1200-line limit. No file-size exception is needed.

Boundary note: this is a store-only slice. It does not wire non-dry-run
`id recover`, `id replace-did`, authsdk recovery/replace calls, or any public
CLI execution path. `awiki-system-test` is therefore deferred until the CLI
slice calls this helper as a subprocess-observable behavior.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies. TLS policy remains
Rustls-first; bundled OpenSSL is not used or newly approved by this slice.

## 2026-05-14 Store Legacy Import Row-Normalization Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract debug_db_import_v1 --locked
cd ../awiki-cli && go test ./internal/store
cd ../awiki-system-test && \
  AWIKI_CLI_UNDER_TEST=rust AWIKI_CLI_RUST_REPO=../awiki-cli-rs2 \
  PYTHONDONTWRITEBYTECODE=1 \
  uv run --no-sync pytest \
    tests_v2/debug/test_debug_cli.py::test_debug_db_import_v1_supports_dry_run_and_missing_path_errors \
    -q
```

Result: passed for the Rust focused store-import contract tests and the existing
CLI dry-run/missing-path selector. The Go reference `internal/store` tests
passed and are the parity source for the three Rust scenarios. The full
`tests_v2/debug/test_debug_cli.py` file was not accepted for this slice because
two selectors require non-dry-run `msg send`, which is still deferred to the
message-service slice.

Scope:

- Replaced the temporary generic table-copy importer with Go `import.go` style
  per-table row normalization for `messages`, `e2ee_outbox`, `contacts`,
  `groups`, `group_members`, `relationship_events`, and `e2ee_sessions`.
- Preserved legacy scan behavior, including direct `.db` path handling,
  `<legacy_data_dir>/database/awiki.db` fallback, soft missing-file scan, schema
  version read, and sorted table list.
- Added a store-layer `LegacyOwnerLookup` input so the CLI can pass identity
  summaries without creating a Rust module dependency cycle. This preserves Go's
  `ownerByCredential` and default-owner inference behavior.
- Preserved Go import guards and defaults: pre-v6 imports require an inferred
  owner; missing tables are skipped and sorted; invalid rows with missing
  required keys are skipped where Go skips them; empty thread/content/status
  fields receive Go-equivalent defaults.
- Preserved contact handle-binding side effects for imported contacts.

Structure note: `crates/awiki-cli/src/store/import.rs` is currently 1197 lines.
That is below the default 1200-line Rust source limit and remains a deliberate
file-level translation of Go `internal/store/import.go`. If the next store slice
adds more behavior, split helper writers into a sibling store module before
expanding this file further.

No dependency was added. The slice continues to use the previously approved
`rusqlite + bundled` SQLite lane and does not introduce HTTP/TLS, OpenSSL,
`native-tls`, or platform service-manager dependencies.

## 2026-05-14 Workspace Upgrade Inspection Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked
cargo +1.79.0 test -p awiki-cli --test core_contract config_show_reports_resolved_configuration_snapshot --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed. The focused Rust contract covers empty-workspace detection,
Go-compatible upgrade path resolution, `Meta`/`Journal` JSON load/save,
config/index/SQLite/legacy-settings detection, doctor warning precedence, and
Go zero-value JSON compatibility for partial `Meta`/`Journal` files. It also
covers `config show` embedding the inspection shape instead of the earlier stub
snapshot. The focused Go tests remain the reference for empty workspace
behavior, current-workspace metadata stamping, and legacy settings rejection
boundaries.

Scope:

- Added `crates/awiki-cli/src/upgrade/{types,meta,journal,detect}.rs` for Go
  `internal/upgrade/{types,meta,journal,detect}.go` read-only surfaces.
- Preserved Go workspace schema version `3` separately from the local SQLite
  schema version `12`.
- Preserved Go path shape for `upgrade/meta.json`,
  `upgrade/upgrade_journal.json`, `upgrade/upgrade.lock`, `upgrade/backups`,
  legacy `config.json`, and legacy `config/settings.json`.
- Preserved missing meta/journal behavior as `null`, parse/read failures as
  inspection errors, zero-value defaults for partial JSON files, and pretty JSON
  save behavior through an atomic local write plus durable parent-directory
  sync.
- Reused existing Rust identity index loading, legacy identity scanning,
  read-only SQLite opening, SQLite `user_version` reading, and legacy database
  scanning rather than duplicating those subsystems inside doctor or app code.
- Wired the shared inspection into `doctor` workspace-upgrade checks and
  `config show` workspace-upgrade snapshots.

Structure note: new Rust files are small and split by Go file responsibility;
no file-size exception is needed.

Boundary note: this slice does not implement full `UpgradeIfNeeded`, lock file
ownership, backup/rollback handling, legacy settings migration, identity
replacement RPC, legacy database import execution, cleanup migrations, or root
preflight workspace upgrade execution. Those remain dedicated workspace
migration/auth/service slices.

No dependency was added. Cargo manifests and lockfile were unchanged. This slice
uses existing serde/std helpers, the existing durable directory sync helper, the
existing identity/store modules, and the already-approved `rusqlite + bundled`
SQLite lane. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, or file-lock dependencies.

## 2026-05-14 Workspace Legacy Settings Parser Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run TestLoadLegacySettingsRejectsSplitServiceURLs -count=1
```

Result: passed. The focused Rust tests cover Go-compatible legacy settings JSON
parsing, service URL normalization, split `user_service_url`/`molt_message_url`
rejection, DID domain pass-through, websocket receive-mode detection, HTTP
fallback for non-websocket modes, and Go-style read/parse error prefixes. The Go
focused test remains the source of truth for the split service URL rejection
message.

Scope:

- Added `crates/awiki-cli/src/upgrade/settings.rs` for the pure
  `loadLegacySettings` helper in Go `internal/upgrade/migration_v0_to_v1.go`.
- Preserved Go's JSON field names: `user_service_url`, `molt_message_url`,
  `did_domain`, and `message_transport.receive_mode`.
- Preserved Go's URL behavior: trim and strip trailing slashes before comparing
  service URLs; choose `user_service_url` when present, otherwise
  `molt_message_url`.
- Preserved Go's runtime-mode behavior: `websocket` only when receive mode is
  `websocket` case-insensitively; all other values map to `http`.

Boundary note: this slice only parses legacy settings. It does not write the
canonical `config.yaml`, run `workspaceV0ToV1Migration`, import legacy
identities, import legacy SQLite, call DID replacement/auth/service RPCs,
create backups, acquire upgrade locks, write journal phases, or clean legacy
skill/listener artifacts.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing serde JSON support and the existing config base-URL normalizer. It
does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session,
platform service-manager, backup, or file-lock dependencies.

## 2026-05-14 Workspace Upgrade File Lock Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock' -count=1
```

Result: passed. The Rust tests mirror the Go `TestAcquireFileLock*` suite:
persistent metadata, reusable `upgrade.lock` anchor, concurrent OS-lock
rejection, residual new-format metadata overwrite, corrupt metadata overwrite,
dead legacy PID ignore, old live legacy PID ignore, and fresh live legacy PID
rejection without overwriting the legacy metadata.

Scope:

- Added `crates/awiki-cli/src/upgrade/lock.rs` for Go
  `internal/upgrade/{lock.go,lock_nonwindows.go,lock_windows.go}`.
- Added the Rust equivalent of Go's private `lockMetadata` JSON shape to
  `crates/awiki-cli/src/upgrade/types.rs`.
- Preserved Go's lock order: create parent dir, open/create `upgrade.lock`,
  acquire the nonblocking OS lock, inspect existing metadata, reject only active
  fresh legacy locks, write fresh metadata, and leave the file in place after
  release.
- Preserved Go's compatibility rules for legacy metadata: ignore empty/corrupt
  files, ignore `os_file_lock_v1` residual metadata, ignore dead or older than
  24-hour legacy PIDs, and reject live recent legacy PIDs.

Boundary note: this slice only translates the local file-lock primitive. It does
not wire the lock into full `UpgradeIfNeeded`, journal phase transitions,
backup/rollback creation, migration execution, identity replacement RPC, legacy
SQLite import, or cleanup migrations.

No dependency was added. Cargo manifests and lockfile were unchanged. Unix uses
direct FFI for `flock` and `kill(0)` to match Go's `syscall` behavior; Windows
source parity uses direct FFI for `LockFileEx`, `UnlockFileEx`, and
`OpenProcess`, but Windows runtime behavior still needs a later Windows host
validation pass. The slice does not introduce HTTP/TLS, OpenSSL, `native-tls`,
WebSocket, authsdk session, platform service-manager, backup, or file-lock crate
dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Workspace Upgrade Fsutil Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::fsutil --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. There is no direct Go unit test for private `fsutil.go`
helpers, so Rust adds focused module tests for the local helper contract and
keeps Go focused upgrade tests as reference coverage for existing upgrade helper
callers.

Scope:

- Added `crates/awiki-cli/src/upgrade/fsutil.rs` for Go
  `internal/upgrade/fsutil.go`.
- Moved the existing workspace meta/journal atomic writer out of `meta.rs` and
  into the shared upgrade fsutil module.
- Preserved Go path helper behavior: empty paths and wrong file types return
  false.
- Preserved Go atomic write behavior: create parent dir, same-directory
  `.upgrade-*.tmp`, write/sync/close temp file, chmod temp file on Unix, rename,
  remove temp file on failure, and sync the parent directory through
  `durablefs`.
- Preserved Go copy helper behavior for future backup execution: direct
  truncate-and-copy file writes, destination parent creation, destination file
  sync, missing tree source no-op, recursive tree copy, directory `0700`
  creation, and Unix source file mode preservation.

Boundary note: this slice does not implement `CreateBackup`,
`backupSQLiteDatabase`, SQLite `VACUUM INTO`, rollback, journal phase handling,
or full `UpgradeIfNeeded` execution. Those remain dedicated workspace migration
slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses standard library filesystem APIs, serde JSON, and the existing `durablefs`
helper. It does not introduce filesystem-copy crates, HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager, backup
runtime execution, or file-lock dependencies.

## 2026-05-14 Workspace Upgrade Backup Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli backup --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. The Rust backup tests cover fixed backup IDs, Go backup file
names, absent input skipping, identity tree copying, meta/journal/config copies,
SQLite `.bak` generation via `VACUUM INTO`, destination replacement, and
single-quote escaping in the destination path. Go has no direct unit test for
private backup helpers, so focused Go upgrade tests remain reference coverage
for existing callers and local invariants.

Scope:

- Added `crates/awiki-cli/src/upgrade/backup.rs` for Go
  `internal/upgrade/backup.go`.
- Used existing `upgrade::Paths` as the Rust input boundary because Go
  `CreateBackup` only reads `uc.Paths` fields for backup assembly. A full
  upgrader `Context` wrapper remains a later execution slice.
- Preserved Go backup names: `config.yaml.bak`, `config.json.bak`,
  `identities/`, `awiki-cli.db.bak`, `meta.json.bak`, and
  `upgrade_journal.json.bak`.
- Preserved Go's skip-missing-input behavior and final sync of the parent of
  the backup directory.
- Preserved Go SQLite backup behavior: create destination parent, remove an
  existing destination, open source through the normal writable store path,
  escape single quotes by doubling them, and execute `VACUUM INTO`.

Boundary note: this slice does not wire backup execution into
`UpgradeIfNeeded`, journal phase transitions, rollback, migration application,
identity replacement RPC, legacy SQLite import, or cleanup migrations.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the existing fsutil module, existing store open path, and already-approved
`rusqlite + bundled` SQLite lane. It does not introduce filesystem-copy crates,
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, or file-lock dependencies. TLS policy remains Rustls-first and
unchanged.

## 2026-05-14 Workspace Upgrade Upgrader Plan Skeleton Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_default_upgrader_plan_matches_go_migration_chain --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_plan_errors_match_go_messages --locked
cargo +1.79.0 test -p awiki-cli workspace_upgrade_context_and_is_done_use_go_paths_and_meta_version --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed. Go has no standalone unit test for `Upgrader.Plan`; source
parity is checked against `internal/upgrade/types.go`, `upgrader.go`, and the
three migration identity methods in `migration_v0_to_v1.go`.

Scope:

- Added `crates/awiki-cli/src/upgrade/upgrader.rs` for the planning boundary in
  Go `internal/upgrade/types.go` and `internal/upgrade/upgrader.go`.
- Added a Rust `Context` matching the Go execution context fields:
  resolved config, upgrade paths, app version, inspection, backup dir, current
  meta, and warnings.
- Added a `Migration` trait with Go-equivalent `from`, `to`, `name`,
  `is_done`, `apply`, and `validate` methods.
- Added `new_default_upgrader` with the Go 0->1, 1->2, and 2->3 migration
  names and `LATEST_WORKSPACE_SCHEMA_VERSION` target.
- Preserved Go `Plan` behavior: same-version no-op, newer-than-target error,
  missing contiguous migration error, unexpected target error, and ordered
  contiguous migration output.
- Preserved Go migration `IsDone` semantics for this skeleton by reading
  workspace meta and comparing `workspace_schema_version >= migration.to`.

Boundary note: this slice does not implement `UpgradeIfNeeded`, journal phase
execution, lock acquisition, backup orchestration, migration `Apply`/`Validate`,
identity replacement RPC, legacy SQLite import, rollback, or cleanup. The
default migration `apply` and `validate` methods intentionally return an
explicit deferred-execution error until those file-level migration slices are
translated.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses `std::collections::BTreeMap`, existing `config::Resolved`, and existing
upgrade meta/path types. It does not introduce HTTP/TLS, OpenSSL,
`native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged.

## 2026-05-14 Workspace UpgradeIfNeeded No-Op/Current Preflight Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_if_needed --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededSkipsEmptyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestAcquireFileLock|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Extended `crates/awiki-cli/src/upgrade/upgrader.rs` with public
  `upgrade_if_needed`, `Upgrader::upgrade_if_needed`, and
  `Upgrader::upgrade_context_if_needed`.
- Preserved Go's `"upgrade context is required"` error through the explicit
  optional-context method used by tests. The normal Rust convenience function
  takes a non-null `config::Resolved` reference.
- Preserved the first Go `Inspect`/current-meta capture boundary before
  deciding whether work is needed.
- Preserved Go's newer-than-supported error:
  `"workspace schema version N is newer than supported 3"`.
- Preserved Go's empty-workspace no-op behavior: no meta file is created and a
  stale journal is cleared.
- Preserved Go's already-current behavior: if meta reports the latest workspace
  schema version, a stale journal is cleared and meta is left unchanged.
- Added an explicit deferred-execution error when a real migration plan would
  be needed so the partial orchestration cannot silently skip migration work.

Boundary note: this slice intentionally stops before the second inspect under
lock, lock acquisition, backup reuse/creation, journal phase execution,
migration `Apply`/`Validate`, meta stamping after each migration, identity
replacement RPC, legacy SQLite import, rollback, and cleanup. Those remain
dedicated migration-execution slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing inspect/meta/journal/planner helpers and does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Config Apply Branch Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test config_writer_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_config` for the pure config-file branch of
  Go `workspaceV0ToV1Migration.Apply`.
- Preserved existing-config behavior: if canonical `config.yaml` exists, stamp
  it to current config schema version while preserving existing values.
- Preserved legacy-config behavior: parse legacy `config.json`, stamp schema,
  write canonical `config.yaml`, and remove the legacy file.
- Preserved legacy-settings behavior for no-workspace legacy installs: load
  `settings.json`, write runtime mode, service base URL, and DID domain into a
  minimal canonical config file.
- Extended the existing `FileConfig` parser to accept JSON input for legacy
  `config.json`, matching Go's YAML parser accepting JSON, without adding a
  YAML dependency.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not import identities, does not import legacy
SQLite rows, does not ensure target store schema, does not refresh resolved
config after import, does not call k1->e1 replacement RPC, and does not enable
full `UpgradeIfNeeded` phase execution.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config writer plumbing and existing `serde_json`. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Validation Wiring Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Wired the default 0->1 migration `validate` path to a Rust
  `validate_workspace_v0_to_v1` helper matching Go
  `workspaceV0ToV1Migration.Validate`.
- Preserved Go's required-context guard through the optional Rust test hook:
  `workspace upgrade requires a resolved config`.
- Preserved config validation: existing `config.yaml` must have
  `schema_version` equal to the current config schema version.
- Preserved SQLite validation: existing target database must have current store
  schema version and pass `PRAGMA integrity_check` plus
  `PRAGMA foreign_key_check`.
- Preserved post-legacy-identity sanity validation: when inspection says no
  workspace existed but legacy identity existed, at least one imported identity
  must now be present in the local identity store.
- Kept 1->2 and 2->3 migration validation deferred.

Boundary note: this slice still does not implement v0->v1 `Apply`, identity
import, legacy SQLite import, legacy config write/remove, k1->e1 replacement
RPC, full `UpgradeIfNeeded` phase execution, journal phase writes, lock/backup
orchestration, meta stamping, rollback, or legacy skill/listener cleanup.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the existing hand-written config parser, local identity manager, already
approved `rusqlite + bundled` store path, and SQLite health helper. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.

## 2026-05-15 Workspace v0->v1 Legacy Local Import Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestLoadLegacySettingsRejectsSplitServiceURLs|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_legacy_imports` for the local legacy import
  branch of Go `workspaceV0ToV1Migration.Apply`.
- Preserved the Go guard: import runs only when inspection reports no existing
  workspace and some legacy state exists.
- Preserved Go import order: scan/import legacy identities first, then scan the
  legacy SQLite database, open the target store, ensure target schema, build
  owner lookup from imported identities, and import legacy rows.
- Covered both no-op guards: existing workspace skips local imports, and
  `has_legacy=false` skips local imports even when legacy fixtures are present.
- Covered the order-sensitive pre-v6 success path: a schema v5 legacy SQLite
  message imports only after the preceding legacy identity import provides the
  owner DID needed by the store importer.
- Preserved the missing-context and missing-inspection errors used by the
  split Rust helper surface.
- Covered the pre-v6 owner guard from the store importer:
  `unsupported legacy sqlite schema version: legacy schema < 6 requires at
  least one imported identity so owner_did can be inferred`.
- Kept the helper separate from full migration execution so it can be wired
  into `WorkspaceMigration.Apply` later with lock/backup/journal/meta evidence.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not call `ensureTargetStoreSchema` after local
imports, does not refresh resolved config after import, does not call k1->e1
DID replacement RPC, and does not enable full `UpgradeIfNeeded` phase
execution.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing identity import helpers and the already-approved
`rusqlite + bundled` SQLite import path. It does not introduce HTTP/TLS,
OpenSSL, `native-tls`, WebSocket, authsdk session, platform service-manager,
filesystem-copy, file-lock, or new SQLite dependencies. TLS policy remains
Rustls-first and unchanged. The dependency audit showed no OpenSSL or
`native-tls`; it only reported existing Rustls/ring and approved bundled
SQLite build surfaces.

## 2026-05-15 Workspace v0->v1 Local Apply Composition Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test store_import_contract --locked
cargo +1.79.0 test -p awiki-cli --test identity_contract --locked
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestUpgradeIfNeededImportsLegacyWorkspace|TestUpgradeIfNeededMigratesLegacyConfigJSON|TestUpgradeIfNeededStampsCurrentWorkspaceMetadata|TestLoadLegacySettingsRejectsSplitServiceURLs' -count=1
```

Result: passed.

Scope:

- Added `apply_workspace_v0_to_v1_local_state` as a focused composition helper
  for the local portions of Go `workspaceV0ToV1Migration.Apply`.
- Preserved local Go ordering: config branch first, legacy identity/SQLite
  import second, target schema ensure when the target database exists, and
  resolved config/path refresh when legacy identities were imported.
- Covered the resolved-context refresh path from legacy settings: runtime mode,
  service base URL, DID domain, derived ANP endpoint/DID, and path refresh.
- Covered the final target schema ensure branch directly with an existing empty
  target database and no legacy SQLite import path.
- Preserved the intentional boundary that `Migration::apply` itself still
  returns the deferred execution error and `context.warnings` is untouched
  because automatic k1->e1 DID replacement is not translated in this slice.

Boundary note: this slice still does not wire v0->v1 `Apply` into the default
Migration implementation, does not call automatic k1->e1 DID replacement, and
does not enable full `UpgradeIfNeeded` phase execution, journal phase writes,
backup/rollback, meta stamping, or legacy skill/listener cleanup.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config, identity, store, and refresh helpers. It does not
introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged. The dependency audit showed no
OpenSSL or `native-tls`; it only reported existing Rustls/ring and approved
bundled SQLite build surfaces.

## 2026-05-14 Workspace SQLite Migration Helpers Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked
cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
```

Result: passed. Go has no dedicated unit test for the private SQLite helper
functions; source parity is checked against
`internal/upgrade/migration_v0_to_v1.go`, and store schema version behavior is
validated through existing Go store tests.

Scope:

- Extended `crates/awiki-cli/src/upgrade/migration_v0_to_v1.rs` with
  `ensure_target_store_schema`, `validate_sqlite_health`, and Go-equivalent SQL
  expectation helpers.
- Preserved Go `ensureTargetStoreSchema` behavior by opening the target store
  through the existing writable store path and delegating to `store::ensure_schema`.
- Preserved store schema version errors for newer and too-old SQLite databases.
- Preserved Go `validateSQLiteHealth` behavior: run `PRAGMA integrity_check`,
  accept case-insensitive trimmed `ok` and an empty trimmed result, then require
  `PRAGMA foreign_key_check` to return no rows.
- Preserved Go helper error text for non-ok integrity and foreign-key
  violations.
- Split v0->v1 helper tests into
  `crates/awiki-cli/tests/workspace_migration_v0_to_v1_contract.rs` so
  `workspace_upgrade_contract.rs` stays under the 1200-line default file-size
  limit.

Boundary note: this slice still does not wire v0->v1 `Apply`/`Validate` into
`Migration`, run `UpgradeIfNeeded` phase execution, write/remove legacy config
files, import identities, import legacy SQLite rows, call identity replacement
RPC, or clean legacy skill/listener artifacts.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses the already-approved `rusqlite + bundled` SQLite path and existing store
APIs. It does not introduce HTTP/TLS, OpenSSL, `native-tls`, WebSocket,
authsdk session, platform service-manager, filesystem-copy, file-lock, or new
SQLite dependencies. TLS policy remains Rustls-first and unchanged.

## 2026-05-14 Workspace Refresh Resolved Config Helper Slice

Local Rust and Go reference verification:

```bash
cargo +1.79.0 fmt --check
git diff --check
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_refresh_resolved_config --locked
cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked
cargo +1.79.0 test -p awiki-cli --locked
cargo +1.79.0 run --bin xtask --locked -- check-structure
cargo +1.79.0 build -p awiki-cli --bin awiki-cli --locked
cargo +1.79.0 tree --workspace --locked | rg -i 'openssl|native-tls|libsqlite3-sys|sqlite|pkg-config|vcpkg|cc |systemd|dbus|launchd|reqwest|hyper|rustls|webpki|aws-lc|ring|tungstenite|websocket'
cd ../awiki-cli && go test ./internal/upgrade -run 'TestRefreshResolvedConfigSyncsMailServiceURLFromConfig|TestRefreshResolvedConfigDerivesMailServiceURLFromServiceBaseURL' -count=1
```

Result: passed.

Scope:

- Added `crates/awiki-cli/src/upgrade/migration_v0_to_v1.rs` for the first
  pure helper from Go `internal/upgrade/migration_v0_to_v1.go`.
- Translated `refreshResolvedConfig` as `refresh_resolved_config` plus an
  optional-context test hook preserving Go's `"resolved config is required"`
  error.
- Preserved Go's config-file refresh behavior for config existence,
  `schema_version`, runtime mode/socket path, output format/no-color,
  service base URL, DID domain, ANP endpoint/DID, mail service URL, and CA
  bundle.
- Preserved Go's mail URL rules: explicit `services.mail_service_url` is
  normalized and copied from config; when config omits it, mail service URL is
  derived from `service_base_url` only if the current resolved mail URL is
  empty.
- Exposed the existing hand-written `config::read_file_config` as crate-local
  so upgrade helpers can reuse the same parser without adding a YAML
  dependency.

Boundary note: this slice does not implement v0->v1 `Apply`, config file
migration, legacy config removal, identity import, legacy SQLite import,
target-store schema enforcement, SQLite health validation, k1->e1 DID
replacement RPC, or `UpgradeIfNeeded` phase execution. Those remain separate
file-level migration slices.

No dependency was added. Cargo manifests and lockfile were unchanged. The slice
uses existing config parsing and URL derivation helpers. It does not introduce
HTTP/TLS, OpenSSL, `native-tls`, WebSocket, authsdk session, platform
service-manager, filesystem-copy, file-lock, or new SQLite dependencies. TLS
policy remains Rustls-first and unchanged.
