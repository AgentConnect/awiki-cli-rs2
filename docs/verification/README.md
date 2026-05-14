# Verification Reports

Store command transcripts and summary reports for parity, structure, Rust unit tests, ANP SDK tests, and `awiki-system-test` runs here.

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
