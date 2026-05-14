# Verification Reports

Store command transcripts and summary reports for parity, structure, Rust unit tests, ANP SDK tests, and `awiki-system-test` runs here.

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

No new dependency was added. Full YAML/config parity remains a later slice,
including Go `write.go` durable-write mechanics such as temp-file write,
fsync/chmod, rename, and directory sync.

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
