# Dependency Decisions

## Platform Dependency Policy

Default rule for this port: prefer pure Rust dependencies and avoid linking to
platform libraries. This applies especially to persistence, TLS/SSL, crypto, and
networking. The goal is repeatable cross-platform builds with fewer host
package prerequisites.

Required review questions before adding a dependency:

1. Does this crate pull in OpenSSL, native-tls, system SQLite, libdbus, system
   keychain libraries, or another platform package?
2. Is there a mature pure Rust alternative that preserves Go `awiki-cli`
   black-box behavior?
3. If exact parity requires a non-Rust component, can it be bundled or vendored
   instead of resolved from the host system?
4. Is the exception documented here and in `docs/parity-matrix.md` with the
   commands/tests that prove the choice?

Undocumented native/system linkage is a completion blocker.

### TLS Policy

TLS dependencies must be Rustls-first. Use Rustls-backed client stacks for
HTTP/WebSocket/service integrations whenever they can preserve the Go CLI's
observable protocol behavior. Do not choose OpenSSL or `native-tls` by default,
and do not treat bundled OpenSSL as the preferred portability fallback. Bundled
OpenSSL is still native C dependency surface, so it is allowed only as a
documented exception after Rustls-compatible options fail a parity requirement.
That exception must include the failed Rustls evidence, the exact Go behavior
being preserved, dependency-tree output, and the system tests that cover the
choice.

Recommended default: use `rustls` directly or choose crates configured with
Rustls features. Do not add `openssl`, `native-tls`, or OpenSSL
`vendored`/bundled features as a first implementation path. A bundled OpenSSL
build only changes how OpenSSL is delivered; it does not satisfy the project's
Rust-native TLS preference.

Execution note: when a future service/client slice needs HTTPS or WebSocket TLS,
start from a Rustls-backed stack such as `reqwest`/`tokio-tungstenite` configured
without `native-tls`. Do not add OpenSSL, including bundled OpenSSL, as the
initial solution for portability.

Bundling OpenSSL only removes a runtime package-install prerequisite; it does
not make the TLS stack Rust-native. Treat bundled OpenSSL as a last-resort
native exception, not as the preferred cross-platform answer.

2026-05-14 clarification: for TLS, the recommended implementation is Rustls.
Do not prioritize OpenSSL bundled/vendored builds for SSL/TLS portability. Any
future OpenSSL path, bundled or system-linked, must first document why a
Rustls-backed implementation failed to preserve Go-observed behavior and must
include dependency-tree and system-test evidence.

## Initial Decisions

| Area | Decision | Rationale | Verification |
| --- | --- | --- | --- |
| ANP SDK | Use workspace-local `../anp/rust` as a path dependency with `default-features = false` for the identity slice. | User requested the local Rust ANP SDK. `id create` needs Go-compatible e1 DID/key material, but the CLI should not pull network or MLS features until those modules are translated. | `cargo +1.79.0 test -p awiki-cli --locked`; 7 identity `awiki-system-test` selectors; dependency tree audit. No OpenSSL/native-tls path was present. |
| SQLite | Use `rusqlite = 0.32.1` with the `bundled` feature for the current store/debug lane. | The user explicitly approved trying `rusqlite + bundled` on 2026-05-14 because compiling SQLite into the binary also solves runtime compatibility. This is a documented exception to the pure Rust preference: it avoids host SQLite at runtime, but it compiles bundled C SQLite through `libsqlite3-sys`. | Temporary probe `/tmp/awiki_rusqlite_probe`: `CARGO_HOME=/tmp/awiki_sqlite_cargo_home cargo +1.79.0 run` and `cargo +1.79.0 run --locked` passed PRAGMA `user_version`, `sqlite_master`, tables, partial unique indexes, views, `ON CONFLICT`, and `ROW_NUMBER() OVER (...)`. Dependency tree shows `rusqlite -> libsqlite3-sys` plus `cc/pkg-config/vcpkg`, and no OpenSSL/native-tls. |
| SSL/TLS | Prefer Rustls-based TLS stacks and avoid OpenSSL/native-tls. The update registry slice uses `rustls` + `webpki-roots` directly for its narrow blocking GET requirement; future service clients may still choose a higher-level Rustls-backed client after a separate dependency review. Do not prefer bundled OpenSSL over Rustls; bundled OpenSSL remains a documented native exception if ever needed. | User explicitly called out SSL/system dependency constraints and later clarified that Rustls is preferred, not OpenSSL bundled. Rustls avoids OpenSSL package and ABI drift across Linux/macOS/Windows while staying aligned with the portability goal. Direct Rustls was chosen for update because loose `reqwest` 0.12 resolution pulled Rust/Cargo 1.79-incompatible transitive crates in this environment, while the update fetch contract only needs GET, status, JSON body, timeout, proxy CONNECT, and cache writeback. | Dependency-tree review, cargo feature audit, local mock registry tests, Go `internal/update` tests, and a dry-run live network smoke for `upgrade`. |
| Crypto | Prefer pure Rust crypto crates unless Go parity or ANP protocol compatibility requires otherwise. | Identity, DID proofs, direct E2EE, and group E2EE must be portable and reproducible without relying on platform crypto libraries. | Golden proof/signature tests against Go behavior and relevant ANP SDK tests. |
| Cargo 1.79 compatibility pins | Pin `time = "=0.3.36"` in the workspace and keep `base64ct` locked to `1.6.0`. | Newer `time` and `base64ct` releases require Cargo edition2024 support, which is incompatible with the current Rust/Cargo 1.79 validation lane. This is a toolchain compatibility pin, not a behavior optimization. | `cargo +1.79.0 test -p awiki-cli --locked` after lockfile update. |
| CLI core slice dependencies | Keep non-storage dependencies minimal: `anyhow`, `serde`, `serde_json`, `sha2`; add `rusqlite + bundled` only for the store/debug lane. | Current local cargo mirror/toolchain has network and Cargo 1.79 compatibility constraints. Storage is the first documented bundled-native exception; other lanes still need explicit dependency review. | `cargo +1.79.0 test -p awiki-cli --locked`, `cargo +1.79.0 run --bin xtask --locked -- check-structure`, dependency tree audit, and focused core/debug `awiki-system-test` run. |
| CLI error hint slice | Add no dependency for Go `internal/cli/error_hints.go`; use small std-only string matching under `app::error_hints`. | The Go helper only classifies a narrow Windows directory-sync compatibility failure and swaps the hint. Translating it as a local app helper avoids broad error-type refactors and preserves the current output/error model. TLS, HTTP, SQLite, authsdk, and platform service decisions are unchanged. | `cargo +1.79.0 test -p awiki-cli error_hints --locked`; `cargo +1.79.0 test -p awiki-cli internal_anyhow --locked`; Go focused `internal/cli` error-hint tests; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| Buildinfo metadata slice | Add no dependency for Go `internal/buildinfo/buildinfo.go`; use `option_env!`, `std::env::consts`, local target-name normalization, and existing `serde` serialization only. | Buildinfo is a pure metadata snapshot used by public envelopes, `status`, `version`, doctor build diagnostics, and version/update policy. Keeping release metadata wiring out of this slice preserves file-level parity without mixing packaging/build-script decisions into the helper translation. TLS policy remains unchanged: future TLS work must start from Rustls and must not choose OpenSSL, `native-tls`, or bundled OpenSSL as the default portability path. | `cargo +1.79.0 test -p awiki-cli buildinfo --locked`; focused `core_contract` version/status tests; `cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked`; `go test ./internal/buildinfo`; 2 focused core `awiki-system-test` selectors; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| Durablefs directory sync slice | Add no dependency for Go `internal/durablefs`; use `std::fs::File::open(...).sync_all()` on non-Windows and a Windows no-op. | The Go helper exists to keep durable rename parent-directory sync Unix-only while avoiding Windows `Access is denied` failures. Extracting it from `config::write` creates a traceable file-level Rust module without changing config writer behavior or introducing platform service libraries. | `cargo +1.79.0 test -p awiki-cli durablefs --locked`; focused config writer durable test; Go `internal/durablefs` and focused `internal/config` tests; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| OpenClaw route registry slice | Add no dependency for Go `internal/runtime/openclawnotify/routes.go` or the local route CLI boundary; use serde JSON plus std-only atomic file replacement and the existing `durablefs` helper. | Route registry load/add/remove/list is local JSON state. Go route add also sends a confirmation webhook, but implementing that would force HTTP/TLS client selection. Per the TLS policy, webhook confirmation is deferred to a dedicated Rustls HTTP/OpenClaw webhook slice instead of introducing OpenSSL/native-tls or ad hoc network code here. | `cargo +1.79.0 test -p awiki-cli openclaw_routes --locked`; `cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked`; Go route registry and CLI dry-run tests; dependency audit unchanged. No dependency was added. |
| Listener status/files slice | Add no dependency for Go `internal/runtime/listener/{types,files,status_helpers}.go` or the saved-status merge helper; use serde JSON and std filesystem helpers only. | Listener status file parsing, PID/boot-id files, session warnings, and saved-status merge are local state behavior. Translating them does not require platform service-manager libraries, WebSocket clients, HTTP/TLS, auth sessions, or native OS service APIs. | `cargo +1.79.0 test -p awiki-cli listener --locked`; focused runtime contract test; Go listener helper tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace upgrade inspection slice | Add no dependency for Go `internal/upgrade/{types,meta,journal,detect}.go`; use serde JSON, std filesystem helpers, existing `durablefs`, existing identity/store scanners, and the approved `rusqlite + bundled` SQLite lane. | Workspace upgrade meta/journal/detection is local read-only state plus JSON persistence. Translating it now improves doctor/config parity without selecting HTTP/TLS, authsdk sessions, platform service-manager libraries, file-lock crates, or migration execution dependencies. Full legacy migration and identity replacement behavior remains separate. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs`; `crates/awiki-cli/tests/doctor_contract.rs`; focused core config-show test; Go focused `internal/upgrade` tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace upgrade file lock slice | Add no file-lock dependency for Go `internal/upgrade/{lock,lock_nonwindows,lock_windows}.go`; use standard-library file handles plus minimal platform FFI for Unix `flock`/`kill(0)` and Windows `LockFileEx`/`UnlockFileEx`/`OpenProcess`. | The Go lock helper is a local concurrency primitive. Direct FFI keeps the port traceable to Go's OS calls, avoids adding a cross-platform locking crate before full upgrade execution exists, and does not link OpenSSL/native-tls, HTTP/TLS, service-manager, or host SQLite dependencies. Windows FFI is included for source parity but still needs future Windows host validation. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs` lock tests; Go `go test ./internal/upgrade -run 'TestAcquireFileLock' -count=1`; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace legacy settings parser slice | Add no dependency for the `loadLegacySettings` helper in Go `internal/upgrade/migration_v0_to_v1.go`; use serde JSON plus the existing config URL normalizer. | Legacy settings parsing is pure local JSON/string normalization used by the future v0->v1 workspace migration. Translating it separately avoids mixing parser parity with config writes, identity import, DID replacement RPC, legacy SQLite import, lock handling, backups, or cleanup commands. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs::workspace_upgrade_legacy_settings_parser_matches_go_contract`; Go `go test ./internal/upgrade -run TestLoadLegacySettingsRejectsSplitServiceURLs -count=1`; full verification recorded in `docs/verification/`. No dependency was added. |
| Runtime/listener local slice | Do not add platform service-manager dependencies for the current runtime/config slice. Use workspace-local listener state to satisfy the verified CLI contract while the full service-manager translation remains deferred. | Go uses platform service management through a library path. The user asked to avoid platform/system libraries where possible. The current system tests validate command shape, config writes, and listener lifecycle JSON contract; those can be met without linking systemd/launchd/Windows service libraries in this slice. | `crates/awiki-cli/tests/runtime_contract.rs`; `tests_v2/runtime/test_runtime_cli.py` passed. Dependency tree still has no OpenSSL/native-tls path and no new platform service library. |
| Mail local command slice | Add no HTTP/TLS dependency in the first mail slice. Translate CLI validation/dry-run contracts and local `mail notify` SQLite behavior only. | Non-dry-run mail RPC requires the authsdk/DID-WBA session chain plus an HTTP/TLS client choice. The project constraint says to avoid system SSL; that dependency decision should be made once for authsdk/mail/message service integrations, not hidden inside a dry-run/local-cache mail slice. | `crates/awiki-cli/tests/mail_contract.rs` passed. Dependency tree remains unchanged except the existing bundled SQLite path. |
| Config set slice | Add no dependency for `config set --did-domain`; use the existing hand-written config parser/writer. | Go behavior is a small config-file mutation with bare-domain normalization. A YAML dependency decision should be made for full config parity later, not introduced for this narrow command. | `crates/awiki-cli/tests/core_contract.rs` passed for dry-run, persistent write, validation, and side-effect checks. Dependency tree unchanged. |
| Config writer helper and durable-write slice | Add no dependency for `internal/config/write.go`; extend the existing hand-written config parser/writer with std-only durable replacement. | This bounded slice translates Go writer helper field mutation behavior, schema-version stamping, Hermes persistence, legacy webhook double-writes, and durable config replacement without changing the YAML dependency surface. The Rust writer uses standard-library same-directory temp files, file sync, Unix chmod/fsync behavior, and Windows parent-directory sync no-op to mirror Go `durablefs`. Full YAML parser/serializer parity remains a separate dependency/format decision. | `crates/awiki-cli/tests/config_writer_contract.rs`, `core_contract`, and `runtime_contract` passed. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls or new platform dependency was added. |
| Doctor local diagnostics slice | Add no dependency for `internal/doctor/doctor.go`; use existing config/identity/store/runtime modules plus `std::process::Command` for the local `anp-mls system version --json-in -` probe. | Go doctor is a local diagnostic aggregator. Translating its report contract now improves core parity without selecting HTTP/TLS, authsdk session, platform service-manager, or MLS provider crates. The external `anp-mls` check remains a health probe only, not group-E2EE provider execution. | `crates/awiki-cli/tests/doctor_contract.rs` passed. Dependency tree remains unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, WebSocket, or platform service-manager dependency was added. |
| Update/upgrade cache-only slice | Add no HTTP/TLS dependency for the first `upgrade` slice. Implement local cache parsing, version policy, and npm command boundary only. | Go `upgrade` normally fetches npm registry metadata over HTTPS, but the current system tests seed local cache and set `AWIKI_CLI_UPDATE_CACHE_ONLY=1`. Deferring network fetch avoids choosing an HTTP/TLS stack before the shared Rustls-based service integration decision. | `crates/awiki-cli/tests/update_contract.rs` and `tests_v2/update` passed. Dependency tree remains unchanged except the existing bundled SQLite path. |
| Update registry fetch/writeback slice | Add `rustls = 0.23` with `default-features = false, features = ["ring", "std", "tls12"]` plus `webpki-roots = 0.26` for Go `internal/update.fetchFromRegistry*`. | The Go update fetch is intentionally small and synchronous. A direct Rustls GET keeps TLS Rustls-first, avoids OpenSSL/native-tls and host cert-store coupling, preserves Go proxy behavior through `HTTP_PROXY`/`HTTPS_PROXY` and `NO_PROXY`, and avoids the Rust 1.79 lockfile churn seen with higher-level HTTP clients. `ring -> cc` is present as the Rustls crypto provider build dependency; this is not OpenSSL/native-tls but is recorded as native build surface. | `cargo +1.79.0 test -p awiki-cli update --locked`; `cargo +1.79.0 test -p awiki-cli --test update_contract --locked`; `go test ./internal/update`; dependency audit showed `rustls`, `rustls-webpki`, `webpki-roots`, and `ring -> cc`, with no OpenSSL/native-tls/reqwest/hyper. Live dry-run smoke returned network metadata from npm registry. |
| Store shared helpers slice | Add no dependency for Go `internal/store/helpers.go`; move already-translated helper primitives into `store/helpers.rs`. | Shared store helpers are standard-library/time/string coercions already used by the legacy import and owner-rebind lanes. Extracting them before recover-merge keeps the file-by-file translation traceable and avoids growing `store/import.rs` past the default 1200-line review threshold. | `crates/awiki-cli/tests/store_helpers_contract.rs`, `store_import_contract`, `store_rebind_contract`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| Store rebind slice | Add no dependency for Go `internal/store/rebind.go` and the owner-DID rebind/E2EE cleanup helpers from `internal/store/dao.go`; reuse the existing `rusqlite + bundled` SQLite lane. | The rebind helpers are local SQLite updates/deletes used after identity replacement. They do not need authsdk, HTTP/TLS, crypto, WebSocket, or platform service-manager crates, and should not force any service dependency decision before non-dry-run identity replacement is translated. | `crates/awiki-cli/tests/store_rebind_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No new dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| Store recover-merge slice | Add no dependency for Go `internal/store/recover_merge.go`; reuse the existing `rusqlite + bundled` SQLite lane. | Recover merge is a local SQLite normalization/upsert/delete transaction used after handle/DID recovery. It needs no authsdk, HTTP/TLS, WebSocket, crypto, or platform service-manager dependency. Keeping it store-only avoids mixing CLI/service execution with translation and keeps TLS/OpenSSL decisions unchanged. | `crates/awiki-cli/tests/store_recover_merge_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No new dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| npm install script parity | Copy Go `package.json`, `scripts/install.js`, and `scripts/run.js` for the package/install surface. | `awiki-system-test` validates the Node installer against the selected Rust repo. The Go package contract uses Node, curl, tar, and PowerShell on Windows; changing it would not be a Rust port optimization and would break 1:1 packaging behavior. | `tests_v2/update/test_install_script.py` passed with a local mirror archive and fake curl failure. |
| Identity/group dry-run CLI slice | Add no dependency for `id replace-did --dry-run` or `group create/update --dry-run`; use static plan builders and existing config resolution. | These commands are currently verified as CLI contract surfaces. Real replace-did and group RPC execution require authsdk/message-service/store-rebind decisions that should not be mixed into dry-run translation. | `crates/awiki-cli/tests/identity_contract.rs`, `crates/awiki-cli/tests/group_contract.rs`, and the two focused `awiki-system-test` selectors passed. Dependency tree unchanged. |
| Identity handle input helper slice | Add no dependency for Go `internal/identity/handle_input.go`; move handle normalization/completion and stored-handle derivation into `identity::handle_input`. | The Go helper is pure string/DID-path normalization used by identity storage and CLI handle completion. Consolidating `msg` and non-E2EE `group` callers on the identity helper removes duplicated, divergent local logic without selecting authsdk, HTTP/TLS, WebSocket, crypto, MLS, or platform dependencies. | `crates/awiki-cli/tests/identity_contract.rs::identity_handle_input_helpers_match_go_contract`, `identity_contract full_handle`, full identity contract test, full `cargo +1.79.0 test -p awiki-cli --locked`, structure check, build, and dependency audit passed. Dependency tree unchanged except existing approved bundled SQLite and existing Rustls/update paths; no OpenSSL/native-tls, HTTP/TLS client, WebSocket, or platform service dependency was added. |
| Group non-E2EE dry-run lifecycle slice | Add no dependency for `group get/join/add/remove/leave/list/members/messages --dry-run`; use static plan builders and existing config resolution. | The Go dry-run contracts do not require network/auth execution. Real group RPC and group E2EE require shared authsdk/message-service/MLS dependency decisions and should stay out of this dry-run slice. | `crates/awiki-cli/tests/group_contract.rs` passed. Dependency tree unchanged. |
| Group base/local wire builder slice | Add no dependency for `internal/message/group_wire.go` base/local request builders; reuse existing local ANP SDK proof generation and current message helper crates only. | This slice constructs JSON-RPC params and RFC9421 origin-proof auth values but does not execute service calls. Transport, JWT refresh, WebSocket, cache mutation, and MLS provider execution remain deferred to the shared Rustls/authsdk/group-E2EE slices. | `crates/awiki-cli/tests/message_group_wire_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, `xtask check-structure`, build, dependency audit, and accepted `awiki-system-test` selector set passed. Dependency tree remained limited to the already approved bundled SQLite path; no OpenSSL/native-tls/TLS client path was added. |
| Group E2EE wire builder slice | Add no dependency for `internal/message/group_wire.go` E2EE request builders; reuse the existing local ANP SDK proof generation and JSON helper crates only. | These builders construct signed hidden E2EE JSON-RPC params and sanitize opaque provider artifacts, but they do not invoke `anp-mls`, call message service RPCs, refresh auth sessions, select transport, or mutate cache. MLS/provider execution remains a separate local-ANP-SDK/service slice. | `crates/awiki-cli/tests/message_group_e2ee_wire_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, `xtask check-structure`, build, dependency audit, and accepted `awiki-system-test` selector set passed. Dependency tree remained limited to the already approved bundled SQLite path; no OpenSSL/native-tls/TLS client path was added. |
| Group E2EE dry-run CLI slice | Add no dependency for `group e2ee ... --dry-run`; model provider metadata and plans without invoking `anp-mls`. | Go dry-run plans expose the intended MLS/provider orchestration without executing the provider. Real MLS execution should be implemented with the local ANP Rust tooling and focused security/system tests, not hidden inside static CLI plan translation. | `crates/awiki-cli/tests/group_contract.rs` passed. Dependency tree unchanged. |
| Page dry-run CLI slice | Add no HTTP/TLS dependency for `page create/list/get/update/rename/delete --dry-run`; use static plan builders and local markdown-file reads only. | Go dry-run page contracts expose `/content/rpc` request metadata without making network calls. Real page CRUD requires active identity auth, DID-auth JWT refresh, and content RPC over HTTP, so it belongs in the shared authsdk + Rustls HTTP slice rather than this CLI-contract translation. | `crates/awiki-cli/tests/page_contract.rs` passed. Dependency tree unchanged. |
| Msg dry-run CLI slice | Add no HTTP/TLS, WebSocket, or E2EE execution dependency for `msg send/attachment download/inbox/history/mark-read/secure ... --dry-run`; use static plan builders and local text-file reads only. | Go dry-run contracts expose service intent without executing message RPC, WebSocket proxy transport, attachment transfer, or secure direct E2EE. Those paths require authsdk/session, message-service clients, Rustls HTTP/WS dependency selection, and E2EE provider decisions, so they should not be hidden inside this CLI-boundary translation. | `crates/awiki-cli/tests/msg_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Site dry-run CLI slice | Add no HTTP/TLS dependency for `site root/page ... --dry-run`; use static plan builders and local markdown-file reads only. | Go dry-run site contracts expose `/site/rpc` request metadata without making network calls. Real tenant site RPC requires active identity auth, DID-auth JWT refresh, and service RPC over HTTP, so it belongs in the shared authsdk + Rustls HTTP slice rather than this CLI-contract translation. | `crates/awiki-cli/tests/site_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Message pure foundation slice | Add no dependency for message request builders, attachment manifest/selection, DID-document service selection, or fallback warning text. | These helpers are pure JSON/value transformations and validation logic in Go. Porting them before real transport reduces risk for the later message-service slice while staying within the no-new-dependency lane. | `crates/awiki-cli/tests/message_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, WebSocket, or new crypto dependency was added. |
| Message RFC9421 origin-proof slice | Reuse the existing local ANP Rust SDK proof/key APIs; add no new dependency. | Go `internal/message/proof.go` signs direct payloads through ANP helpers. The Rust port can preserve this local proof boundary with the already-approved local `../anp/rust` path dependency, without introducing auth session refresh, HTTP/TLS, WebSocket, or additional crypto crates. | `crates/awiki-cli/tests/message_contract.rs` passed with origin-proof generation, canonical digest comparison, and DID-document verification. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Message signed wire params slice | Add no dependency for signed direct text and direct/group attachment manifest request params; reuse the local origin-proof helper. | Go wire builders return signed JSON params before transport. Translating that boundary now proves signed payload shape while still deferring authsdk session refresh, HTTP/WS clients, attachment transfer, and cache mutation to service slices. | `crates/awiki-cli/tests/message_contract.rs` passed with signed direct send and signed attachment manifest proof verification. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, or WebSocket crate was added. |
| Trace/transport config foundation slice | Add no dependency for Go `internal/traceutil/trace.go` or the pure timeout/profile resolver from `internal/transportcfg/config.go`. Defer `NewHTTPClient` to a dedicated Rustls-first client slice. | Trace formatting and timeout env resolution are pure std/local behavior. Implementing `NewHTTPClient` now would mix translation with HTTP/TLS root-store, custom CA bundle, HTTP/2, and pooling dependency decisions. The user clarified TLS should be Rustls-first and bundled OpenSSL must not be the preferred portability path. | `crates/awiki-cli/tests/traceutil_contract.rs`, `crates/awiki-cli/tests/transportcfg_contract.rs`, full Rust test/build/structure checks, Go `go test ./internal/traceutil ./internal/transportcfg`, and dependency audit. No dependency was added; future HTTP/TLS work must keep OpenSSL/native-tls out unless a documented Rustls parity failure exists. |

## Known Deferred Decisions

| Area | Deferred Decision | Required Evidence Before Adoption |
| --- | --- | --- |
| Full YAML config parsing | Choose a parser that preserves Go YAML behavior without introducing unnecessary native dependencies. | Go config fixture parity tests, environment override tests, and dependency-tree review. |
| SQLite crate/backend | Current accepted lane is `rusqlite + bundled` for exact SQLite behavior and runtime portability. Keep pure Rust alternatives recorded for later optimization review, not mixed into this parity translation. | Exact schema/migration parity, query behavior parity, no host SQLite dependency, and system-test debug/store evidence. |
| HTTP/WebSocket service client stack | Select Rustls-based HTTP/WebSocket crates for service integrations beyond the update registry GET helper. Bundled OpenSSL is not the default fallback and requires a separate exception record if Rustls cannot meet parity. | Feature audit showing no OpenSSL/native-tls path and service-backed system tests. |
| Content page RPC client | Translate `internal/content/service.go` after the shared authsdk/session and Rustls HTTP stack are selected. | Must preserve DID-auth JWT refresh, `/content/rpc` method names, status/RPC error mapping, visibility normalization, page lifecycle system tests, and no OpenSSL/native-tls path. |
| Tenant site RPC client | Translate `internal/site/service.go` after the shared authsdk/session and Rustls HTTP stack are selected. | Must preserve DID-auth JWT refresh, `/site/rpc` method names, status/RPC error mapping, domain normalization/rejection, page lifecycle system tests, and no OpenSSL/native-tls path. |
| Mail RPC client | Translate `internal/mail/client.go` after the shared authsdk/session and Rustls HTTP stack are selected. | Must preserve DID-auth JWT refresh, bearer scope behavior, JSON-RPC error mapping, CA bundle handling, and local mail-service system tests without adding OpenSSL/native-tls. |
| Message RPC, WebSocket, attachment, and secure direct clients | Translate `internal/message/service.go`, direct/group message client paths, attachment transfer, and secure direct E2EE execution after the shared authsdk/session, Rustls HTTP/WS stack, and E2EE provider decisions are selected. | Must preserve message RPC status/error mapping, runtime-mode transport behavior, local cache writes, attachment manifest/upload/download semantics, secure outbox retry/drop behavior, and no OpenSSL/native-tls or bundled OpenSSL path without a separate documented exception. |
| Auth/session-backed signed service calls | Combine the verified local RFC9421 origin-proof helper with `authsdk` session/JWT refresh and service transport in a later slice. | Must preserve bearer/JWT refresh semantics, signed direct/group/attachment request execution, status/error mapping, and no OpenSSL/native-tls path. |
| `transportcfg.NewHTTPClient` | Translate Go `NewHTTPClient` only in the shared Rustls HTTP client lane, not in pure config slices. | Must preserve dial timeout, keepalive, TLS handshake timeout, response-header timeout, idle connection timeout, max idle conns, per-host idle conns, HTTP/2 intent, optional CA bundle appended to trust roots, TLS 1.2 minimum, and no OpenSSL/native-tls or bundled OpenSSL path unless a separate documented exception proves Rustls cannot match required parity. |
| Platform service-manager integration | Decide whether to translate Go listener service control with a cross-platform Rust crate, direct per-platform code, or a no-platform-dependency supervisor strategy. | Must compare native/platform dependencies, service behavior parity, and `AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1` behavior before adoption. Do not mix this choice into unrelated runtime config translation. |

## Mail Slice Notes

2026-05-14:

- Added a split `mail` module for command-plan data and local notification
  service behavior, plus `app/mail_handlers.rs` for the Go
  `internal/cli/mail.go` CLI boundary.
- No new dependency was added. Remote mail RPC remains deferred until the shared
  authsdk/HTTP client slice chooses a Rustls-based stack and verifies that no
  OpenSSL/native-tls path is introduced.
- Implemented local `mail notify` on top of the existing bundled SQLite store.
  This follows the Go predicate for legacy `content_type = "mail.notification"`
  rows and current `metadata.source_kind = "mail"` rows.
- Verification: `cargo +1.79.0 test -p awiki-cli --test mail_contract --locked`
  passed; full workspace verification is recorded in `docs/verification/`.

## Doctor Slice Notes

2026-05-14:

- Added a split `doctor` module for Go `internal/doctor/doctor.go` instead of
  keeping the diagnostic surface inside `app.rs`.
- Preserved Go's fixed check order and report shape for `build`,
  `config_file`, `environment`, `anp_service`, `runtime`, `identity_store`,
  `sqlite`, `anp_mls`, `workspace_upgrade`, and `legacy_paths`.
- Added a local `anp-mls` binary resolution/version probe through the standard
  library only. This does not invoke MLS provider operations, hidden group-E2EE
  service APIs, HTTP/WebSocket transport, or auth sessions.
- Current boundary: workspace-upgrade deep meta/journal inspection and full Go
  YAML parse-error parity remain separate config/upgrade slices.

## Group Wire Slice Notes

2026-05-14:

- Added a split `message::group_wire` module for Go
  `internal/message/group_wire.go` base/local request construction. E2EE RPC
  wire builders remain a later split so the default Rust source file size stays
  well below the 1200-line limit.
- Reused the existing local ANP Rust SDK origin-proof helper. No HTTP, WebSocket,
  TLS, authsdk session, cache mutation, MLS, or new dependency was introduced.
- Preserved Go's request-shape boundaries for signed group control operations,
  signed group sends, unsigned group info/local reads, policy defaults, and
  validation messages.

## Group E2EE Wire Slice Notes

2026-05-14:

- Added a split `message::group_e2ee_wire` module for the hidden E2EE request
  builders in Go `internal/message/group_wire.go`. The file remains under the
  default 1200-line limit and keeps MLS-specific request construction out of the
  base/local group wire module.
- Reused the existing local ANP Rust SDK origin-proof helper. No HTTP, WebSocket,
  TLS, authsdk session, cache mutation, MLS provider call, or new dependency was
  introduced.
- Preserved Go's E2EE request-shape boundaries for control-plane vs
  group-e2ee security profiles, caller-provided operation/message IDs, cipher
  object sanitization, KeyPackage sanitization, recovery/update target objects,
  notice/head requests, and `group_state_ref` augmentation.

## Update/Upgrade Slice Notes

2026-05-14:

- Added a split `update` module for Go `internal/update/update.go` cache-only
  decision behavior and `app/update_handlers.rs` for the Go
  `internal/cli/upgrade.go` command boundary.
- No HTTP/TLS crate was added. The first slice intentionally covers seeded
  cache metadata, strict-disable controls, dev-build behavior, semver-like
  prerelease comparison, and npm install command shape only.
- Registry fetching and cache writeback are deferred to the shared Rustls HTTP
  client decision so update, mail, authsdk, and service clients do not each
  pick ad hoc TLS dependencies.
- Copied `package.json`, `scripts/install.js`, and `scripts/run.js` from the Go
  repository to satisfy the npm package/install surface. These scripts keep the
  Go packaging dependency behavior: Node plus `curl` and `tar`/PowerShell.
- Verification: `cargo +1.79.0 test -p awiki-cli --locked` and
  `tests_v2/update` passed.

## Runtime/Listener Slice Notes

2026-05-14:

- Added the local/offline runtime slice without adding a new dependency.
- `runtime status/apply/setup/mode get/set`, listener config/lifecycle, and
  host-notify/OpenClaw config commands use the existing Rust standard library,
  `serde_json`, and existing store/config modules.
- Listener lifecycle commands currently persist a workspace-local
  `listener.local-state.json` under the runtime state directory. This is a
  deliberate parity-slice boundary: it preserves the system-tested CLI JSON
  contract without introducing platform service libraries in the translation
  pass.
- Full platform service management is recorded as a later dependency decision.
  It should be translated in a dedicated slice after evaluating whether Rust
  can preserve Go behavior without increasing system-library coupling.
- Verification: `cargo +1.79.0 test -p awiki-cli --locked`,
  `cargo +1.79.0 run --bin xtask --locked -- check-structure`, and
  `tests_v2/runtime/test_runtime_cli.py` passed.

## ANP Identity Slice Notes

2026-05-14:

- Added `anp = { path = "../anp/rust", default-features = false }` for local
  identity creation.
- The CLI calls `anp::authentication::create_did_wba_document` with e1 DID
  profile, user path segment, generated challenge, and an ANP message service
  entry matching the Go `BuildAgentANPMessageService` profile/security values.
- Default ANP SDK features are intentionally disabled for this slice so the CLI
  does not pull `reqwest`, Rustls, MLS, or the ANP SDK's optional `rusqlite`
  path until the corresponding Go modules are translated.
- Verification: `cargo +1.79.0 test -p awiki-cli --locked`, full local build,
  structure check, dependency tree audit, and 7 focused `tests_v2/id`
  system-test selectors passed.

## SQLite Pure Rust Trial Log

2026-05-14:

- `turso = "0.5.3"` with `default-features = false` builds with `cargo +stable`
  when `CARGO_HOME` is isolated from the global USTC mirror configuration. It
  passed the probe through PRAGMA `user_version`, table creation, partial unique
  index creation, views, `ON CONFLICT`, and `sqlite_master` introspection, but
  failed the required window-function probe with
  `Parse error: no such function: ROW_NUMBER`. This is not sufficient for Go
  store parity because `internal/store/schema.go` uses
  `ROW_NUMBER() OVER (...)` when backfilling contact handle bindings.
- `turso = "0.6.0-pre.30"` with `default-features = false` builds and passes
  the same probe with `cargo +stable` (`cargo 1.95.0`, `rustc 1.95.0`):
  PRAGMA `user_version`, schema object creation, partial unique index behavior,
  views, `ON CONFLICT`, `sqlite_master` introspection, and
  `ROW_NUMBER() OVER (...)`.
- Dependency audit caveat: `turso = "0.6.0-pre.30"` does not pull in
  `libsqlite3-sys`, `rusqlite`, `openssl-sys`, or `native-tls` in the local
  probe. The registry crate includes `bindgen`/`clang-sys` as build-dependency
  metadata for `turso_sdk_kit`, but its published manifest sets `build = false`
  and a verbose clean build used the pre-generated bindings instead of running
  bindgen. However, `turso_core` has an unconditional target dependency on
  `simsimd` for non-wasm/non-Windows-aarch64 targets, and `simsimd` compiles
  packaged C code through `cc`. That is bundled native code and is not an
  acceptable default under the project dependency policy unless patched out or
  documented as a last-resort exception.
- The earlier `turso = "0.6.0-pre.30"` failure under the default toolchain was
  a tooling constraint: the workspace default was Cargo/Rust 1.79 and the
  dependency graph includes crates requiring newer edition support. Direct
  access through the local VPN/proxy works; Cargo downloads became reliable when
  using an isolated temporary `CARGO_HOME` instead of the global USTC mirror
  configuration.

Current pure Rust follow-up: `turso = "0.6.0-pre.30"` remains a later
optimization candidate if the project wants to revisit a non-SQLite-C backend.
Do not mix that optimization with the 1:1 translation lane.

## SQLite Bundled Trial Log

2026-05-14:

- User instruction update: `rusqlite + bundled` is approved as a first store
  lane trial. The rationale is that compiling SQLite into the CLI binary
  removes runtime host-SQLite compatibility concerns.
- `rusqlite = "0.32"` resolved to `rusqlite v0.32.1` and
  `libsqlite3-sys v0.30.1` under Cargo/Rust 1.79.
- Probe command:
  `CARGO_HOME=/tmp/awiki_sqlite_cargo_home cargo +1.79.0 run` in
  `/tmp/awiki_rusqlite_probe`.
- Locked probe command:
  `CARGO_HOME=/tmp/awiki_sqlite_cargo_home cargo +1.79.0 run --locked`.
- SQL probe passed: PRAGMA `journal_mode`, `foreign_keys`, `busy_timeout`,
  PRAGMA `user_version`, table/index/view creation, partial unique index,
  `ON CONFLICT`, `sqlite_master` introspection, and the
  `ROW_NUMBER() OVER (...)` window function used by the Go schema backfill.
- Dependency-tree result:
  `rusqlite -> libsqlite3-sys`, with `cc`, `pkg-config`, and `vcpkg` as build
  helpers. No `openssl-sys` or `native-tls` path was present. This is a bundled
  native SQLite exception, not a pure Rust dependency.
