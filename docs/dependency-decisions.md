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

## Parallel Development Constraint

Native Agents may be used for independent module/file slices when they improve
throughput. Any code-writing Native Agent must use GPT-5.5 with xhigh reasoning
and a bounded, non-overlapping write scope. Read-only Native Agents may use
lighter reasoning/model settings. This is an execution constraint, not a reason
to merge unrelated translation goals into one slice.

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

Implementation rule: bundled OpenSSL must not be used as the first answer to
runtime portability. It is not equivalent to a Rustls-backed stack because it
still adds an OpenSSL native C dependency to the build and security update
surface. For every new HTTPS, WebSocket, mail, authsdk, runtime bridge, or
service-client lane, start with `rustls` or a crate configured for Rustls and
only consider OpenSSL after recording a concrete Rustls parity failure.

2026-05-14 user clarification: for all future TLS lanes, `rustls` is the
recommended implementation path. Do not prioritize bundled OpenSSL for
portability; bundled OpenSSL may only be introduced as a documented exception
after a Rustls-backed implementation is proven unable to preserve required
Go-observed behavior.

Execution note: when a future service/client slice needs HTTPS or WebSocket TLS,
start from a Rustls-backed stack such as `reqwest`/`tokio-tungstenite` configured
without `native-tls`. Do not add OpenSSL, including bundled OpenSSL, as the
initial solution for portability.

Bundling OpenSSL only removes a runtime package-install prerequisite; it does
not make the TLS stack Rust-native. Treat bundled OpenSSL as a last-resort
native exception, not as the preferred cross-platform answer.

User clarification: for TLS, the recommended implementation is `rustls`.
Do not prioritize OpenSSL bundled/vendored builds for SSL/TLS portability. Any
future OpenSSL path, bundled or system-linked, must first document why a
Rustls-backed implementation failed to preserve Go-observed behavior and must
include dependency-tree and system-test evidence. In other words,
`openssl` with `vendored`/bundled features is not an acceptable first-choice
substitute for a Rustls-backed stack.

## Initial Decisions

| Area | Decision | Rationale | Verification |
| --- | --- | --- | --- |
| ANP SDK | Use workspace-local `../anp/rust` as a path dependency with `default-features = false` for the identity slice. | User requested the local Rust ANP SDK. `id create` needs Go-compatible e1 DID/key material, but the CLI should not pull network or MLS features until those modules are translated. | `cargo +1.79.0 test -p awiki-cli --locked`; 7 identity `awiki-system-test` selectors; dependency tree audit. No OpenSSL/native-tls path was present. |
| Identity ANP service helper slice | Add no dependency for Go `internal/identity/anp_service.go`; reuse the already-selected local `../anp/rust` SDK with `default-features = false`. | Endpoint/DID defaulting, endpoint/DID validation, and `ANPMessageService` JSON construction are deterministic local helpers. Translating them inside the existing DID module keeps `generate_identity` on the Go validation path without enabling ANP network/MLS features, HTTP/TLS clients, OpenSSL, `native-tls`, or bundled OpenSSL. | `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked` passed before full verification; Go focused identity DID/service tests passed. Cargo manifests and lockfile are unchanged. |
| Identity recover dry-run slice | Add no dependency for Go `internal/identity/recover.go` plan construction and `id recover --dry-run`. Keep OTP/recover RPC execution and finalization deferred. | The recover preview path is deterministic local identity-store planning plus command metadata. Translating it separately fills a visible CLI/schema gap while avoiding user-service HTTP/TLS, auth session refresh, generated remote recovery writeback, and SQLite merge orchestration decisions. | `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked`; `cargo +1.79.0 test -p awiki-cli identity::recover --locked`; Go focused `internal/cli`/`internal/identity` recover tests passed. Cargo manifests and lockfile unchanged; no HTTP/TLS/WebSocket/OpenSSL/native-tls dependency was added. |
| Authsdk local token/session slice | Add no dependency for the first Go `internal/authsdk/session.go` slice; wrap the already-present local ANP Rust `DIDWbaAuthHeader` with CLI-side bearer scope and token persistence semantics. | Go token capture/persistence is local state around ANP headers and can be translated before selecting a real service HTTP client. Keeping this slice transport-free avoids enabling ANP `network`, `reqwest`, WebSocket, OpenSSL, or `native-tls` while still unlocking the auth session state machine one file at a time. The `anpsdk` facade records the Go module path/version and only exposes the DID-WBA auth types needed by this slice; full registry aliases stay deferred until consumers need them. | `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`; Go focused `internal/authsdk` tests; full verification recorded in `docs/verification/`. No dependency was added; real JSON-RPC/EnsureJWT/HTTP transport remains a Rustls-first future lane. |
| ANP SDK registry facade slice | Add no dependency for expanding Go `internal/anpsdk/registry.go` aliases and the Go direct-E2EE file-store boundaries; re-export public symbols already present in the local `../anp/rust` SDK with default features still disabled, and add small CLI-side adapters for session, signed-prekey, one-time-prekey, and pending-outbound stores. | Downstream file-by-file translation needs a stable local ANP facade for secure direct work. The local Rust SDK already provides direct-E2EE models, `DirectE2eeError`, `SessionStore`, `SignedPrekeyStore`, `PendingOutboundStore`, and PEM key material APIs, so the Go file-backed stores can be translated with std filesystem/serde JSON only, without enabling ANP `network`/default features or changing dependency selection. Keeping the high-level message-service E2EE client deferred avoids mixing RPC, DID resolution, prekey publishing, and secure send execution into this helper slice. | `cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked`; AuthSDK regression test; Go `internal/anpsdk` compile guard and secure session/prekey store usage guards; full verification/dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no ANP `network`/default features, reqwest, hyper, WebSocket, OpenSSL, `native-tls`, bundled OpenSSL, YAML, platform service, or new SQLite path was added. |
| ANP SDK key-material facade helper slice | Add no dependency for Go `internal/anpsdk/registry.go` `KeyType`, `GenerateKeyPairPEM`, `GeneratedKeyPairPEM`, `PrivateKeyFromPEM`, and `PublicKeyFromPEM`; expose a CLI-side facade over existing local `../anp/rust` key material and DID-document generation APIs. | The local ANP Rust SDK already exposes `PrivateKeyMaterial`, `PublicKeyMaterial`, standard PEM serialization/parsing, and DID-document generation that produces ed25519, secp256k1, secp256r1, and x25519 keys. A small facade wrapper can preserve Go's public helper boundary and standard PEM/legacy-label behavior without adding crypto crates, changing local ANP SDK features, accepting legacy labels in runtime parsers, or touching network/transport code. | `cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked`; follow-up full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Authsdk JSON-RPC wire/result slice | Add no dependency for Go `internal/authsdk/session.go` pure JSON-RPC, header, HTTP/RPC error, and `EnsureJWT` result handling. At the time of that slice, `doRequest`, real HTTP execution, 401 retry, and live `id refresh-token` were deferred. | These helpers are deterministic JSON/header/value transformations around the existing local ANP Rust SDK. Translating them before selecting service transport gave mail/page/site/message clients a shared error/payload contract without adding `reqwest`, `hyper`, WebSocket, OpenSSL, `native-tls`, or enabling ANP `network` features. The later Authsdk Rustls HTTP execution slice now consumes these helpers for generic authenticated HTTP I/O; endpoint-specific service lanes are still separate. | `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked` passed before full verification. Cargo manifests and lockfile were unchanged; later service lanes must continue plugging these helpers into the Rustls-first shared client instead of creating duplicate auth wrappers. |
| Authsdk JSON-RPC null-error compatibility | Add no dependency for remote JSON-RPC envelopes containing `"error": null`; keep the fix inside the existing `authsdk::wire` decoder. | Go's JSON decoder treats a nullable error pointer as absent. The `awiki.info` user service can return `error: null` with a valid `result`, and Rust must decode that as success rather than constructing a fake RPC error. This is a shared wire-compatibility fix for all service clients, not an identity-specific workaround. | `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`; `tests_v2/page` passed after the fix. Cargo manifests and lockfile unchanged. |
| Authsdk Rustls HTTP execution slice | Add no dependency for Go `internal/authsdk/session.go` `DoJSONRPC`, `DoJSON`, `doRequest`, one-shot 401 retry, and live `EnsureJWT`; reuse the existing shared `transportcfg::HttpClient`. | The transport dependency decision was already made in the Rustls/std `transportcfg.NewHTTPClient` slice. Authsdk can now compose the existing local ANP DID-WBA header helper, JSON/RPC wire helpers, and shared Rustls client without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, or enabling ANP SDK `network` features. This keeps endpoint-specific service clients as later translation work while preventing each service from creating its own auth retry/token-capture wrapper. | `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`, transport/update focused regression tests, full verification, Go `go test ./internal/authsdk -count=1`, and dependency audit. Cargo manifests and lockfile remain unchanged; audit stays limited to existing Rustls/webpki/ring and approved bundled SQLite paths. |
| Mail live RPC slice | Add `base64 = 0.22` as a direct pure Rust dependency for Go `mail attachment download` base64 decoding; reuse the existing shared Rustls/std `transportcfg::HttpClient` and authsdk session execution for remote mail RPC. | Go mail live execution needs attachment `content_base64` decode before writing the requested output file. The crate was already present transitively through the local ANP Rust SDK, is pure Rust, and avoids platform libraries. Remote RPC reuses the Rustls-first authsdk transport instead of adding `reqwest`, `hyper`, OpenSSL, `native-tls`, WebSocket crates, or ANP network/default features. | `cargo +1.79.0 test -p awiki-cli --test mail_wire_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test mail_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`; dependency audit after full verification. Cargo lockfile changes only add `base64` to `awiki-cli` direct dependencies; no new native/system dependency path is expected. |
| Identity remote wire contract slice | Add no dependency for the pure Go `internal/identity/client.go` and remote-method portions of `internal/identity/service.go`. Keep `RemoteClient` execution, auth session bootstrap, DID generation, identity persistence, polling timers, and non-dry-run id command wiring deferred. | Identity register/bind/recover/profile/resolve/replace/refresh has a large deterministic endpoint/method/profile/params/result boundary that can be translated before selecting the shared Rustls HTTP stack. Keeping this slice transport-free preserves Go live-service semantics, including `AuthRefresh` for token refresh and JSON `null` for empty live replace-DID role/endpoint values, without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, or ANP SDK network/default features. | `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked` passed before full verification. Cargo manifests and lockfile are unchanged; future live identity execution must reuse `identity::wire` with the shared Rustls-first authsdk/session client. |
| Identity live phone register slice | Add no dependency for Go `internal/identity.Service.Register` phone OTP execution or `internal/cli/id.go` `id register` command wiring; reuse the existing shared Rustls/std `transportcfg::HttpClient` directly with the existing identity wire builders. | Phone registration is an unauthenticated user-service JSON-RPC path, so it can reuse the shared Rustls transport without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, or ANP SDK network/default features. DID generation reuses the local `../anp/rust` SDK and adds only a handle path-prefix variant for Go parity. | `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; full `cargo +1.79.0 test -p awiki-cli --locked`; Go focused register tests; `tests_v2/page` passed. Cargo manifests and lockfile unchanged. |
| Identity live email register slice | Add no dependency for Go `internal/identity.Service.Register` email activation, wait/polling, and final email-backed `did-auth.register`; reuse the existing shared Rustls/std `transportcfg::HttpClient`, identity REST/RPC wire builders, and local ANP Rust DID generation. | Email registration uses existing user-service REST endpoints plus the existing unauthenticated `did-auth.register` JSON-RPC path. A small unauthenticated REST POST helper was added on the existing Rustls/std client so the slice can preserve Go `email-status`, `email-send`, optional wait, and final register behavior without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features. | `cargo +1.79.0 test -p awiki-cli --test identity_register_email_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked`; Go focused register/email identity tests; focused `awiki-system-test` email-registration selector against `awiki.info`; dependency audit. Cargo manifests and lockfile unchanged. |
| Identity live refresh-token slice | Add no dependency for Go `internal/identity.Service.RefreshToken` or `internal/cli/id.go` `id refresh-token`; reuse the existing shared Rustls/std `transportcfg::HttpClient`, local ANP Rust DID-WBA header helper, and `authsdk::Session::ensure_jwt`. | Explicit refresh must bypass stale stored bearer while still producing signed DID-auth `get_me` requests, capturing body or header tokens, and persisting the selected identity's `auth.json`. The shared Rustls/authsdk path already provides that behavior without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features. | `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`; full `cargo +1.79.0 test -p awiki-cli --locked`; Go focused refresh/authsdk/cmdmeta tests; focused `awiki-system-test` id/page selector; dependency audit. Cargo manifests and lockfile unchanged. |
| Identity live profile/resolve slice | Add no dependency for Go `internal/identity.Service` profile/resolve execution or `internal/cli/id.go` `id profile get/set` and `id resolve` wiring; reuse the existing shared Rustls/std `transportcfg::HttpClient`, local ANP Rust DID-WBA auth helper, `authsdk::Session`, and identity wire builders. | Profile set/get and resolve need authenticated DID profile RPC for self/update paths, unauthenticated handle/public-profile lookups, active identity loading, JWT bootstrap, stored-bearer seeding, and local display-name persistence. The existing Rustls/authsdk/transport stack already preserves those behaviors, so adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features would only expand the dependency surface during a 1:1 translation slice. | `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked`; full `cargo +1.79.0 test -p awiki-cli --locked`; Go focused profile/resolve/authsdk/cmdmeta tests; dependency audit. Cargo manifests and lockfile unchanged; focused `awiki-system-test` progressed through profile/resolve and stopped at the separate `id bind` port gap. |
| Identity live bind slice | Add no dependency for Go `internal/identity.Service.Bind` or `internal/cli/id.go` `id bind`; reuse the existing shared Rustls/std `transportcfg::HttpClient`, local ANP Rust DID-WBA auth helper, `authsdk::Session`, and identity wire builders. | Bind uses service REST endpoints, not a new protocol stack: authenticated JSON POST for phone bind send/verify and email send, plus a bearer-authenticated GET for email status. The existing Rustls/std HTTP client and authsdk session already preserve the required auth/JWT behavior, so adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features would expand dependencies without improving parity. A tiny local query encoder was used for the email-status URL to keep the manifest unchanged. | `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked`; full `cargo +1.79.0 test -p awiki-cli --locked`; Go focused bind/profile/resolve/authsdk/cmdmeta tests; dependency audit. Cargo manifests and lockfile unchanged; focused `awiki-system-test` passed the `id bind` assertions and stopped at the separate non-dry-run `id recover` port gap. |
| Identity live recover slice | Add no dependency for Go `internal/identity.Service.Recover`, `FinalizeRecoveredHandle`, or `internal/cli/id.go` `id recover`; reuse the existing shared Rustls/std `transportcfg::HttpClient`, identity wire builders, local ANP Rust DID generation, and the approved bundled SQLite store merge helpers. | Recover uses existing identity user-service JSON-RPC endpoints and local filesystem/SQLite state; it does not need a new protocol or dependency. The OTP send and `recover_handle` calls reuse `identity::Client`, recovered DID/key generation reuses the local `../anp/rust` SDK, and local state merge reuses `rusqlite + bundled`. Adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features would expand dependencies without improving 1:1 Go parity. | `cargo +1.79.0 test -p awiki-cli --test identity_recover_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked`; full `cargo +1.79.0 test -p awiki-cli --locked`; Go focused recover/bind/profile/resolve/store/authsdk/cmdmeta tests; focused `awiki-system-test` id selector; dependency audit. Cargo manifests and lockfile unchanged. |
| Identity live replace-DID slice | Add no dependency for Go `internal/identity.Service.ReplaceDID`, `internal/identity.Manager.BackupIdentityForDIDReplacement`, `ReplaceIdentity`, or `internal/cli/id.go` `id replace-did`; reuse the existing shared Rustls/std `transportcfg::HttpClient`, authsdk DID-WBA session, identity wire builders, local ANP Rust DID generation, and the approved bundled SQLite store rebind helpers. | Replace-DID uses the existing authenticated `/user-service/did-auth/rpc` JSON-RPC path and local filesystem/SQLite state. It does not need a new protocol stack: auth/JWT bootstrap reuses `authsdk::Session`, `replace_did` payloads reuse `identity::wire`, new e1 DID/key generation reuses local `../anp/rust`, and owner-state rebinding reuses the existing `rusqlite + bundled` store helper. Adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features would expand dependencies without improving 1:1 Go parity. | `cargo +1.79.0 test -p awiki-cli --test identity_replace_did_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_wire_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked`; Go focused replace-did/store/authsdk/cmdmeta tests; dependency audit. Cargo manifests and lockfile unchanged. |
| Content RPC wire contract slice | Add no dependency for Go `internal/content/{types.go,service.go}` request builders, visibility normalization, result summaries, and service-level validation. At the time of this wire-only slice, `identity.RemoteClient`, DID-auth refresh, real HTTP execution, and non-dry-run page commands were deferred; the later content/page live RPC slice now covers that execution boundary. | Content CRUD has a deterministic wire/summary boundary that can be translated before live transport wiring. Keeping it separate preserved the important difference between Go dry-run planning, which is intentionally permissive, and live service validation, which rejects invalid visibility and empty updates. | `cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked` passed before full verification. Cargo manifests and lockfile were unchanged; no HTTP/TLS/WebSocket/OpenSSL/native-tls dependency was added. |
| Content/page live RPC slice | Add no dependency for Go `internal/content/service.go` live execution or `internal/cli/page.go` non-dry-run page command wiring; reuse the existing shared Rustls/std `transportcfg::HttpClient` and authsdk session execution. | Page CRUD now needs active identity loading, DID-auth JWT refresh, bearer seeding, JSON-RPC execution, service error conversion, and CLI exit-code mapping. The already-selected Rustls-first authsdk transport preserves those live semantics without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, or ANP SDK network/default features. | `cargo +1.79.0 test -p awiki-cli --test page_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test page_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test content_wire_contract --locked`; dependency audit after full verification. Cargo manifests and lockfile remain unchanged; this slice adds no new native/system dependency path. |
| Site RPC wire contract slice | Add no dependency for Go `internal/site/{types.go,service.go}` request builders, domain normalization, slug validation, result summaries, and live-service result shapes. At the time of this wire-only slice, `identity.RemoteClient`, auth-session bootstrap, DID-auth refresh, real HTTP execution, and non-dry-run site commands were deferred; the later tenant site live RPC slice now covers that execution boundary. | Tenant site RPC has a deterministic wire/summary boundary that can be translated before live transport wiring. Keeping it separate preserved the important difference between Go dry-run planning, which only trims domain text, and live service validation, which uses `NormalizeDIDDomain` and rejects non-bare domains. | `cargo +1.79.0 test -p awiki-cli --test site_wire_contract --locked` passed before full verification. Cargo manifests and lockfile were unchanged; no HTTP/TLS/WebSocket/OpenSSL/native-tls dependency was added. |
| Tenant site live RPC slice | Add no dependency for Go `internal/site/service.go` live execution or `internal/cli/site.go` non-dry-run site command wiring; reuse the existing shared Rustls/std `transportcfg::HttpClient` and authsdk session execution. | Tenant site root/page commands need active identity loading, DID-auth JWT refresh, bearer seeding, JSON-RPC execution, service error conversion, and CLI exit-code mapping including Go's `forbidden` mapping for HTTP 403 / RPC -32001. The already-selected Rustls-first authsdk transport preserves those live semantics without adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or ANP SDK network/default features. | `cargo +1.79.0 test -p awiki-cli --test site_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test site_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test site_wire_contract --locked`; dependency audit after full verification. Cargo manifests and lockfile remain unchanged; this slice adds no new native/system dependency path. |
| SQLite | Use `rusqlite = 0.32.1` with the `bundled` feature for the current store/debug lane. | The user explicitly approved trying `rusqlite + bundled` on 2026-05-14 because compiling SQLite into the binary also solves runtime compatibility. This is a documented exception to the pure Rust preference: it avoids host SQLite at runtime, but it compiles bundled C SQLite through `libsqlite3-sys`. | Temporary probe `/tmp/awiki_rusqlite_probe`: `CARGO_HOME=/tmp/awiki_sqlite_cargo_home cargo +1.79.0 run` and `cargo +1.79.0 run --locked` passed PRAGMA `user_version`, `sqlite_master`, tables, partial unique indexes, views, `ON CONFLICT`, and `ROW_NUMBER() OVER (...)`. Dependency tree shows `rusqlite -> libsqlite3-sys` plus `cc/pkg-config/vcpkg`, and no OpenSSL/native-tls. |
| Debug handle-history slice | Add no dependency for Go `internal/cli/debug.go` `debug db handle-history`; reuse the existing approved `rusqlite + bundled` store/debug lane. | The command is local SQLite inspection only and needs no network, TLS, YAML, platform service, or SDK dependency. A dedicated parameterized store helper was added rather than extending the safe ad-hoc SQL helper, preserving the Go query shape without introducing another query abstraction or dependency. | `cargo +1.79.0 test -p awiki-cli --test debug_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test core_contract --locked`; Go focused `internal/cli` tests; `git diff --check`. Cargo manifests and lockfile remain unchanged. |
| SSL/TLS | Prefer Rustls-based TLS stacks and avoid OpenSSL/native-tls. The update registry slice uses `rustls` + `webpki-roots` directly for its narrow blocking GET requirement; future service clients may still choose a higher-level Rustls-backed client after a separate dependency review. Do not prefer bundled OpenSSL over Rustls; bundled OpenSSL remains a documented native exception if ever needed. | User explicitly called out SSL/system dependency constraints and later clarified that Rustls is preferred, not OpenSSL bundled. Rustls avoids OpenSSL package and ABI drift across Linux/macOS/Windows while staying aligned with the portability goal. Direct Rustls was chosen for update because loose `reqwest` 0.12 resolution pulled Rust/Cargo 1.79-incompatible transitive crates in this environment, while the update fetch contract only needs GET, status, JSON body, timeout, proxy CONNECT, and cache writeback. | Dependency-tree review, cargo feature audit, local mock registry tests, Go `internal/update` tests, and a dry-run live network smoke for `upgrade`. |
| Crypto | Prefer pure Rust crypto crates unless Go parity or ANP protocol compatibility requires otherwise. | Identity, DID proofs, direct E2EE, and group E2EE must be portable and reproducible without relying on platform crypto libraries. | Golden proof/signature tests against Go behavior and relevant ANP SDK tests. |
| Cargo 1.79 compatibility pins | Pin `time = "=0.3.36"` in the workspace and keep `base64ct` locked to `1.6.0`. | Newer `time` and `base64ct` releases require Cargo edition2024 support, which is incompatible with the current Rust/Cargo 1.79 validation lane. This is a toolchain compatibility pin, not a behavior optimization. | `cargo +1.79.0 test -p awiki-cli --locked` after lockfile update. |
| CLI core slice dependencies | Keep non-storage dependencies minimal: `anyhow`, `serde`, `serde_json`, `sha2`; add `rusqlite + bundled` only for the store/debug lane. | Current local cargo mirror/toolchain has network and Cargo 1.79 compatibility constraints. Storage is the first documented bundled-native exception; other lanes still need explicit dependency review. | `cargo +1.79.0 test -p awiki-cli --locked`, `cargo +1.79.0 run --bin xtask --locked -- check-structure`, dependency tree audit, and focused core/debug `awiki-system-test` run. |
| CLI error hint slice | Add no dependency for Go `internal/cli/error_hints.go`; use small std-only string matching under `app::error_hints`. | The Go helper only classifies a narrow Windows directory-sync compatibility failure and swaps the hint. Translating it as a local app helper avoids broad error-type refactors and preserves the current output/error model. TLS, HTTP, SQLite, authsdk, and platform service decisions are unchanged. | `cargo +1.79.0 test -p awiki-cli error_hints --locked`; `cargo +1.79.0 test -p awiki-cli internal_anyhow --locked`; Go focused `internal/cli` error-hint tests; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| Buildinfo metadata slice | Add no dependency for Go `internal/buildinfo/buildinfo.go`; use `option_env!`, `std::env::consts`, local target-name normalization, and existing `serde` serialization only. | Buildinfo is a pure metadata snapshot used by public envelopes, `status`, `version`, doctor build diagnostics, and version/update policy. Keeping release metadata wiring out of this slice preserves file-level parity without mixing packaging/build-script decisions into the helper translation. TLS policy remains unchanged: future TLS work must start from Rustls and must not choose OpenSSL, `native-tls`, or bundled OpenSSL as the default portability path. | `cargo +1.79.0 test -p awiki-cli buildinfo --locked`; focused `core_contract` version/status tests; `cargo +1.79.0 test -p awiki-cli --test doctor_contract --locked`; `go test ./internal/buildinfo`; 2 focused core `awiki-system-test` selectors; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| Durablefs directory sync slice | Add no dependency for Go `internal/durablefs`; use `std::fs::File::open(...).sync_all()` on non-Windows and a Windows no-op. | The Go helper exists to keep durable rename parent-directory sync Unix-only while avoiding Windows `Access is denied` failures. Extracting it from `config::write` creates a traceable file-level Rust module without changing config writer behavior or introducing platform service libraries. | `cargo +1.79.0 test -p awiki-cli durablefs --locked`; focused config writer durable test; Go `internal/durablefs` and focused `internal/config` tests; full verification and dependency audit recorded in `docs/verification/`. No dependency was added. |
| Runtime bridge endpoint helper slice | Add no dependency for the pure helper subset of Go `internal/runtime/bridge_unix.go`, `bridge_windows.go`, and bridge shapes from `internal/runtime/config.go`; use std filesystem/path handling, `sha2`, and existing serde JSON. | Default endpoint selection, Unix socket-path shortening, endpoint preparation, Unix endpoint availability, Windows named-pipe default/blank-normalization/prefix-validation helpers, and request/response/error JSON shapes are local deterministic behavior and can be translated before selecting Windows named-pipe I/O and listener service execution details. Keeping this slice local avoids HTTP/TLS, WebSocket crates, Windows named-pipe crates, platform service-manager crates, OpenSSL, `native-tls`, or bundled OpenSSL. | `cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked`; runtime contract tests; Go focused runtime resolve tests; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. |
| Runtime bridge server framing helper slice | Add no dependency for the server-side local bridge framing subset of Go `internal/runtime/listener/server.go` `handleConn`; reuse existing `runtime::bridge` wire types and std `Read`/`Write`. | Go `handleConn` reads exactly one newline-terminated JSON request with `bufio.Reader.ReadBytes('\n')`, dispatches once, writes one JSON-encoded bridge response plus newline, and closes. This can be translated as a pure injected-dispatch helper before wiring real `Supervisor.handleBridgeRequest` or foreground listener accept loops. Adding WebSocket crates, Tokio/async framing, platform service libraries, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, or new SQLite dependencies would mix later transport/session execution into a local JSON framing boundary. | `cargo +1.79.0 test -p awiki-cli --test runtime_bridge_contract --locked`; adjacent message WS proxy and bridge dispatch tests; Go source parity for `handleConn`; Cargo manifests and lockfile unchanged. No dependency was added. |
| Runtime host-notify enable/disable slice | Add no dependency for Go `internal/cli/runtime.go` `runtime host-notify enable` and `runtime host-notify disable`; reuse existing config writer and runtime status helpers. | The command pair only toggles `runtime.host_notify.enabled` and renders the existing host-notify config/status view. Implementing this local CLI surface does not require platform service-manager crates, WebSocket crates, HTTP/TLS changes, YAML parser changes, OpenSSL/`native-tls`, bundled OpenSSL, ANP SDK network/default features, or a new SQLite backend. Go's full listener restart application remains a later runtime execution boundary. | `cargo +1.79.0 test -p awiki-cli --test runtime_host_notify_enable_disable_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked`; neighboring host-notify sink/OpenClaw/Hermes contract tests; Go focused `internal/config` and `internal/cli` dry-run tests. Cargo manifests and lockfile unchanged. |
| Runtime Hermes host-notify guide/status slice | Add no dependency for Go `runtime host-notify hermes guide/status`; reuse existing config writer/parser, existing Hermes helpers, `serde_json`, std filesystem/env handling, and existing `sha2` for service-name hashing. | The implemented surface is read-only: command routing, guide/status rendering, secret-source metadata, local Hermes route inspection, and bridge-status warning aggregation. A small scalar-only YAML reader is sufficient for the current `InspectRoute` status view and avoids choosing `serde_yaml` or a platform service library inside this read-only CLI slice. Full YAML parser/serializer selection, `EnsureRoute`, generated route secrets, Hermes setup/set/secret mutation, listener refresh, platform service-manager status, bridge process execution, health checks, WebSocket crates, OpenSSL/`native-tls`, bundled OpenSSL, or new SQLite dependencies remain separate dependency decisions. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_cli_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked`; adjacent runtime/Hermes tests; Go focused CLI/Hermes bridge tests; structure check, whitespace check, and dependency audit. Cargo manifests and lockfile unchanged. |
| Runtime Hermes host-notify local write slice | Add no dependency for Go `runtime host-notify hermes set/set-secret/clear-secret`; reuse the existing config writer helpers, hand-written config parser, runtime config view, and local listener-status snapshot. | These commands mutate only awiki config fields and render local status. They do not require full Hermes route YAML mutation, bridge process execution, health checks, listener restart orchestration, platform service-manager crates, WebSocket crates, new HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, ANP SDK network/default features, a new SQLite backend, or a YAML parser. The approved `rusqlite + bundled` and Rustls-first policies remain unchanged; Go's full listener refresh side effect is intentionally deferred to a later runtime execution slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_config_write_contract --locked`; adjacent runtime/Hermes tests; Go focused CLI/config writer tests; structure check, whitespace check, and dependency audit. Cargo manifests and lockfile unchanged. |
| Runtime Hermes host-notify setup dry-run slice | Add no dependency for Go `runtime host-notify hermes setup --dry-run`; reuse the existing hand-written config parser, Hermes bridge validation helpers, Hermes secret-source env constants, `serde_json`, std env/path handling, and `rand` already present in the workspace. | The dry-run surface is a local plan renderer plus validation: it reads raw awiki config fallback fields and reports secret-source metadata, but it must not write awiki config, write or parse local Hermes route YAML, start/restart listeners, run bridge processes, health-probe Hermes, or integrate a platform service manager. A full non-dry-run setup will need a separate dependency decision for YAML mutation and bridge/service execution; this slice deliberately avoids `serde_yaml`, platform service crates, WebSocket crates, new HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, ANP SDK network/default features, or a new SQLite backend. The existing approved `rusqlite + bundled` and Rustls-first policies are unchanged. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked`; adjacent runtime/Hermes tests; Go focused CLI/config/Hermes bridge tests; structure check, whitespace check, and dependency audit. Cargo manifests and lockfile unchanged. |
| Listener service local helper slice | Add no dependency for the deterministic helper subset of Go `internal/runtime/listener/service.go`, `run_foreground_*`, and `sysproc_*`; use existing `sha2`, `rand`, std filesystem/env/time APIs, and the existing listener file helpers. | Service naming, display-name derivation, service-mode detection, boot-id handling, artifact cleanup, readiness polling, foreground signal platform selection, and child-process detach platform selection are local behavior and can be translated before selecting any platform service-manager, signal-loop, or process-spawn approach. Adding `kardianos/service`, systemd/launchd/Windows service crates, `signal-hook`, Windows named-pipe/process crates, WebSocket crates, or OS service libraries here would mix dependency-sensitive service execution with pure helper parity. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_service_contract --locked`; Go focused listener service readiness tests; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. |
| Listener WebSocket client helper slice | Add no dependency for Go `internal/runtime/listener/wsclient.go` deterministic helper behavior and `config.DeriveWebSocketURL`; use existing config/message/identity constants plus `serde_json::Value` only. | Endpoint derivation, request-ID coercion, int64 coercion, and host extraction can be translated before selecting a WebSocket transport crate. Adding `coder/websocket`, `tokio-tungstenite`, `tungstenite`, `reqwest`, `hyper`, OpenSSL/`native-tls`, or platform service libraries here would mix dependency-sensitive network execution with pure helper parity. The later real WebSocket transport must be Rustls-first and separately dependency-reviewed. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked`; Go focused `NewWSClient` endpoint derivation test; Go/Rust probes for float formatting and `url.Parse` edge behavior; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. |
| Listener WebSocket JSON-RPC wire helper slice | Add no dependency for the pure JSON-RPC envelope/result helper subset of Go `internal/runtime/listener/wsclient.go` `SendRPC`, `readLoop`, and `failPending`; use existing `serde_json` values only. | Request envelopes, response/error result decoding, pending-failure response shape, and response-vs-notification classification are deterministic JSON transformations. They can be translated before selecting a real Rust WebSocket transport. Adding `coder/websocket`, `tokio-tungstenite`, `tungstenite`, async runtime crates, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix transport/session execution into a pure wire-boundary slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked`; Go source parity for `SendRPC`/`readLoop`/`failPending`; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener WebSocket dial-error helper slice | Add no dependency for Go `internal/runtime/listener/wsclient.go` `formatDialError`; use byte slicing and string trimming only. | Dial error body decoration is deterministic local formatting: nil error returns nil, missing/empty response body returns the original error text, nonempty body appends a trimmed body preview, and body reads are capped at 4096 bytes. Translating it before real WebSocket dialing avoids premature transport dependency selection. Adding WebSocket crates, async runtime crates, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix later dial execution into a pure formatting boundary. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked`; Go source parity for `formatDialError`; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener WebSocket connect decision helper slice | Add no dependency for the branch-control subset of Go `internal/runtime/listener/wsclient.go` `NewWSClient`, `dialBearer`, `refreshBearer`, and `Connect`; model dial and refresh as injected outcomes. | Constructor scope remembering, bearer header trimming, refresh precondition errors, initial-token dial, 401-triggered refresh/retry, no-token bootstrap, refresh error wrapping, empty refreshed-token error, and formatted dial failures are deterministic once dial/refresh outcomes are injected. Translating this before real `coder/websocket` replacement avoids selecting a WebSocket crate, async runtime, or extra HTTP/TLS stack prematurely. The later executable `Connect` must use a Rustls-first transport decision and separately document any non-Rust dependency exception. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_wsclient_contract --locked`; Go focused connect tests for endpoint derivation, expired-token 401 refresh/retry, and no-token bootstrap; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener connect-session helper slice | Add no dependency for the deterministic pre-transport subset of Go `internal/runtime/listener/server.go` `connectSession`; use existing URL constants and `std::time::Duration` only. | Identity load ordering, user-readiness gating, paths lookup ordering, auth-session construction inputs, stored-JWT scope seeding, `NewWSClient` error handling, 15-second connect timeout, connect-error close, and successful current-JWT writeback can be translated with injected outcomes before a real WebSocket client exists. Adding WebSocket crates, async runtime crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, E2EE provider dependencies, or SQLite access would mix foreground listener execution and transport selection into this helper-only slice. Later real `connectSession` wiring must still use a Rustls-first WebSocket transport decision. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_connect_session_contract --locked`; Go source parity plus adjacent listener reconnect, wsclient connect, and identity-gating guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener consume-notifications helper slice | Add no dependency for the deterministic subset of Go `internal/runtime/listener/server.go` `consumeNotifications`; use `serde_json::Value` and `std::time::Duration` only. | Ping scheduling, 15-second ping timeout ownership, context-cancel exit, ping error wrapping, closed notification-channel error precedence, and notification dispatch can be represented as a pure event-to-action step before a real foreground WebSocket client exists. Adding WebSocket crates, async runtime crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, E2EE provider dependencies, or SQLite access would mix real channel/ticker/session execution into this helper-only slice. Later executable notification consumption must still use the Rustls-first WebSocket transport decision. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_notification_consume_contract --locked`; Go source parity plus adjacent listener reconnect guard; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener notification route-plan helper slice | Add no dependency for the pure route/action-planning subset of Go `internal/runtime/listener/server.go` `handleNotification`; reuse existing listener parser, host-notify, secure-classification, and contact-handle helpers. | `handleNotification` ordering is deterministic once direct-secure normalization and contact-sync outcomes are injected: secure normalization runs first, host event normalization happens once on the post-secure notification, direct/mail/group/group-state branches plan fixed store/upsert/dispatch actions, and contact-sync errors are ignored. Representing those side effects as actions avoids opening SQLite, calling remote handle lookup, decrypting E2EE, flushing secure outbox, sending host notifications, mutating local queues, selecting WebSocket crates, adding async runtime crates, or adding platform service/TLS dependencies in this helper slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_notification_plan_contract --locked`; Go focused `handleNotification`/host-notify/secure-listener guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener bridge dispatch helper slice | Add no dependency for the pure bridge-request dispatch subset of Go `internal/runtime/listener/server.go` `handleBridgeRequest`; reuse existing message wire builders and JSON coercion helpers. | The dispatch boundary maps already-framed local bridge requests into message RPC method/params and records the `inbox.mark_read` IDs needed for the later post-success SQLite side effect. It does not open a WebSocket, own a session, fetch remote capabilities, mutate SQLite, or dispatch notifications. Adding `coder/websocket`, `tokio-tungstenite`, `tungstenite`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix the later foreground listener transport into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_bridge_dispatch_contract --locked`; Go source parity plus existing listener bridge history/group-message coverage; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener message-service DID helper slice | Add no dependency for the pure service-DID fetch boundary of Go `internal/runtime/listener/server.go` `fetchMessageServiceDID`; use `serde_json::Map` only. | The listener WebSocket path fetches `group.create` target service DID by sending `anp.get_capabilities` with empty params, then accepting only a string `service_did`. This can be locked as a pure helper before adding real WebSocket sessions, `WSClient.SendRPC`, or `Supervisor` state. It intentionally does not reuse the HTTP transport configured-DID precedence or core-binding params because Go's listener helper does neither. Adding `coder/websocket`, `tokio-tungstenite`, `tungstenite`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix a later transport/session slice into this parse/request-shape boundary. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_service_did_contract --locked`; Go source parity for `fetchMessageServiceDID` plus adjacent Go `TestHTTPTransportGetMessageServiceDIDUsesConfiguredOrCapabilities` context; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener session-state helper slice | Add no dependency for the pure session/status mutation subset of Go `internal/runtime/listener/server.go` `refreshStatus`, `recordSessionError`, `setBridgeAvailable`, and `session` mark-connected/disconnected helpers; use std collections only. | Listener session status mutation is local state behavior and can be translated before wiring real `Supervisor`, locks, filesystem status writes, WebSocket clients, or reconnect loops. The Rust helper uses a deterministic `BTreeMap` for stable tests while preserving Go-observable fields and documenting that Go map iteration order is not stable. Adding WebSocket crates, async runtime crates, platform service libraries, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, or new SQLite dependencies would mix later runtime execution into pure state parity. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_state_contract --locked`; Go source parity for session/status helpers plus existing listener status helper tests; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener session-loop backoff helper slice | Add no dependency for the pure control-flow subset of Go `internal/runtime/listener/server.go` `runSessionLoop`, `retryPublishSecurePrekeys`, `sleepWithContext`, and `minDuration`; use `std::time::Duration` only. | Reconnect backoff, initial-signal one-shot behavior, post-connect action order, post-consume cleanup order, cancellation branching, and fixed secure-prekey retry delay are deterministic once connect/consume/sleep outcomes are injected. Translating this before foreground listener execution avoids adding WebSocket crates, async runtime crates, platform service-manager libraries, SQLite access, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, or E2EE provider dependencies. Real task spawning, `WSClient.Connect`, `consumeNotifications`, secure inbox polling, and `PublishSecurePrekeys` side effects remain separate slices. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_loop_contract --locked`; Go source parity plus focused Go runtime listener reconnect integration guard; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener local notification queue helper slice | Add no dependency for Go `internal/runtime/listener/server.go` `queueLocalNotification` and `flushQueuedLocalNotifications`; use existing `serde_json::Map`/`Value` plus std collections only. | Local notification queuing is an in-memory helper used by secure ack recovery before foreground listener sessions are fully wired. It can preserve Go's trim-for-presence/original-key, append-order, exact-DID flush, and delete-on-flush behavior without opening a WebSocket, decrypting E2EE payloads, writing SQLite, or dispatching host notifications. Adding WebSocket crates, E2EE provider dependencies, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix later foreground listener execution into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_local_notifications_contract --locked`; Go source parity for local queue helpers plus existing secure-ack integration guard `TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession`; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener session DID lookup helper slice | Add no dependency for Go `internal/runtime/listener/server.go` `activeSessionByDID`, `recordByDID`, and `hasRuntimeSessionForDID`; use plain structs and an injected manager trait only. | Session/identity DID lookup is deterministic control flow around in-memory session records and identity-manager list/load calls. Translating it as pure data plus injected manager results preserves nil-manager, list-error, load-error, first-match, scan-order, and exact-DID behavior without constructing a real `Supervisor`, taking locks, reading identity files, opening WebSockets, running E2EE, writing SQLite, or dispatching host notifications. Adding WebSocket crates, E2EE provider dependencies, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix later foreground listener execution into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_session_lookup_contract --locked`; Go source parity for session lookup helpers plus existing secure-ack integration guard `TestDeliverLocalSecureAckInProcessPromotesPendingInitiatorSession`; Cargo manifests and lockfile unchanged. No dependency was added. |
| OpenClaw route registry slice | Add no dependency for Go `internal/runtime/openclawnotify/routes.go` or the local route CLI boundary; use serde JSON plus std-only atomic file replacement and the existing `durablefs` helper. | Route registry load/add/remove/list is local JSON state. Go route add also sends a confirmation webhook, but implementing that would force HTTP/TLS client selection. Per the TLS policy, webhook confirmation is deferred to a dedicated Rustls HTTP/OpenClaw webhook slice instead of introducing OpenSSL/native-tls or ad hoc network code here. | `cargo +1.79.0 test -p awiki-cli openclaw_routes --locked`; `cargo +1.79.0 test -p awiki-cli --test runtime_contract --locked`; Go route registry and CLI dry-run tests; dependency audit unchanged. No dependency was added. |
| Hermes bridge pure helper slice | Add no dependency for the deterministic helper subset of Go `internal/runtime/hermesbridge/hermes_config.go`; use std filesystem/string handling and existing `serde_json::Map` for route-map cleanup helpers. | Defaults, local notify URL validation, deliver/home-channel mapping, `.env` parsing, cleanup helpers, and prompt replacement predicates can be translated before selecting a YAML parser or runtime bridge orchestration design. Keeping this slice pure avoids adding `serde_yaml`, HTTP/TLS clients, platform service-manager crates, OpenSSL, `native-tls`, bundled OpenSSL, or ANP SDK network features. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked`; Go `go test ./internal/runtime/hermesbridge -count=1`; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. |
| Hermes host notification sink slice | Add no new dependency for Go `internal/runtime/listener/hermes_host_notify.go`; use existing `sha2` for the fixed HMAC-SHA256 helper, the existing hand-written config parser for secret fallback, and the already selected Rustls/std `transportcfg::HttpClient` for HTTP delivery. | Hermes delivery is a small synchronous POST with a 15-second timeout, signed headers, and status/body error mapping. Reusing the existing Rustls-first HTTP client preserves TLS policy without adding `reqwest`, `hyper`, OpenSSL, `native-tls`, bundled OpenSSL, WebSocket crates, YAML crates, or platform service libraries. A generic `hmac` crate would be acceptable later if broader HMAC usage appears, but for this single fixed algorithm it would only widen the dependency tree. Foreground dispatch and Hermes bridge YAML orchestration remain separate slices. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_host_notify_contract --locked`; Go focused listener Hermes tests; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. |
| Listener status/files slice | Add no dependency for Go `internal/runtime/listener/{types,files,status_helpers}.go` or the saved-status merge helper; use serde JSON and std filesystem helpers only. | Listener status file parsing, PID/boot-id files, session warnings, and saved-status merge are local state behavior. Translating them does not require platform service-manager libraries, WebSocket clients, HTTP/TLS, auth sessions, or native OS service APIs. | `cargo +1.79.0 test -p awiki-cli listener --locked`; focused runtime contract test; Go listener helper tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Listener secure-direct pure helper slice | Add no dependency for the pure secure-direct helper subset of Go `internal/runtime/listener/server.go`; use existing JSON/value handling only. | Direct secure notification classification, secure wire content-type recognition, message-view conversion, and plaintext-body conversion are deterministic local helpers. They do not open WebSockets, decrypt payloads, acknowledge messages, write SQLite/storage state, or dispatch host notifications, so adding E2EE provider crates, WebSocket crates, HTTP/TLS clients, OpenSSL/`native-tls`, platform service libraries, or new SQLite dependencies would widen this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_notifications_contract --locked`; Go secure listener integration guard for decrypt/local-ack paths that consume these helpers; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener secure normalization planning slice | Add no dependency for the pure planning subset of Go `internal/runtime/listener/server.go` `normalizeDirectSecureNotification`; reuse existing secure notification helpers and local ack payload helper. | Secure normalization has a large dependency-sensitive execution surface, but its observable branch order can be translated with injected process/ack outcomes: early returns, decrypted notification rewrites, secure ack/init method changes, and init-ack side-effect ordering. Adding WebSocket crates, async runtime crates, E2EE provider crates, file session-store access, SQLite access, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or YAML crates would mix real `ProcessIncoming`, `SendJSON`, queued-outbox flushing, and foreground listener execution into this planning slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_normalize_contract --locked`; Go source parity plus existing secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener local secure ack in-process planning slice | Add no dependency for the pure control-flow subset of Go `internal/runtime/listener/server.go` `deliverLocalSecureAckInProcess`; reuse existing identity types, `serde_json`, and `BuildSecureAckPayload`. | The in-process ack function crosses ANP SDK file session stores, E2EE encryption/decryption, recipient processing, local session lookup, queued outbox flushing, and local notification queues. The observable branch order can be locked with injected outcomes: early skips, encrypted notification shape, recipient fallback ladder, sender/recipient save ordering, active-session flush/log, managed queue, and network fallback. Adding ANP SDK wiring, WebSocket crates, async runtime crates, file-store dependencies, E2EE provider crates, SQLite access, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or YAML crates would mix real secure execution into this planning slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_ack_in_process_contract --locked`; Go source parity plus existing secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener local secure ack delivery helper slice | Add no dependency for the pure `deliverLocalSecureAck` subset of Go `internal/runtime/listener/server.go` and `BuildSecureAckPayload` from Go `internal/message/secure_control.go`; use existing `serde_json` only. | The local ack delivery boundary is deterministic once active-session presence and ack result JSON are injected: inactive sessions skip, empty/non-object ack bodies skip, message IDs use Go's string-only fallback behavior, and delivered acks build a direct secure cipher notification for `handleNotification`. Adding WebSocket crates, async runtime crates, E2EE provider crates, file session-store access, SQLite access, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or YAML crates would mix `deliverLocalSecureAckInProcess` and foreground listener execution into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_ack_delivery_contract --locked`; Go source parity plus existing secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener peer queued secure outbox flush trigger slice | Add no dependency for the pure Supervisor-level trigger subset of Go `internal/runtime/listener/server.go` `flushPeerQueuedSecureOutbox`; use existing identity types only. | The trigger boundary is deterministic once the sessions snapshot and injected flush warnings are supplied: scan current records, exact-match owner DID, return on nil secure RPC for the first owner match, otherwise plan one queued-outbox flush and warning log. Adding WebSocket crates, async runtime crates, E2EE provider crates, file session-store access, SQLite access, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or YAML crates would mix real `message.FlushQueuedSecureOutbox`, `WSClient.SendRPC`, database mutation, and foreground listener execution into this trigger-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_outbox_flush_contract --locked`; Go source parity plus existing secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener secure inbox poll helper slice | Add no dependency for Go `internal/runtime/listener/server.go` `pollUnreadSecureDirectInbox`; use `std::time::Duration` only. | Initial sync ordering, Go's current 2-second ticker interval, repeated tick sync ordering, and context-cancel exit are deterministic once the two sync helpers are represented as actions. Adding WebSocket crates, async runtime crates, HTTP/TLS clients, E2EE provider crates, SQLite access, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix real secure inbox/history RPC execution into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_inbox_poll_contract --locked`; Go source parity plus existing secure listener integration guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener secure inbox/history sync planning slice | Add no dependency for the pure RPC planning subset of Go `internal/runtime/listener/server.go` `syncUnreadSecureDirectInbox` and `syncPendingConfirmationSecureHistory`; reuse existing message wire builders and secure replay filters. | Secure sync execution crosses WebSocket RPC, context ownership, SQLite dedupe, and notification dispatch, but its request shape and replay handoff are deterministic once RPC results and store lookups are injected. Adding WebSocket crates, async runtime crates, HTTP/TLS clients, E2EE provider crates, SQLite access beyond injected lookup, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or YAML crates would mix real foreground listener execution into this planning slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_sync_contract --locked`; Go source parity plus existing secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener secure replay filter helper slice | Add no dependency for the pure secure backlog/history replay filtering subset of Go `internal/runtime/listener/server.go` `syncUnreadSecureDirectInbox` and `syncPendingConfirmationSecureHistory`; reuse existing secure notification helpers and injected store lookup outcomes. | Secure replay filtering is deterministic once RPC results and store lookup outcomes are injected: only secure direct wire content types are considered, pending history skips self-sent messages before store lookup, receiver DID fallback is used only for store lookup, existing or store-error messages are skipped, malformed secure message views are skipped after lookup, and accepted views are converted through the already translated secure notification helper. Adding WebSocket crates, HTTP/TLS clients, E2EE provider crates, SQLite access, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix later foreground listener execution into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_replay_contract --locked`; Go source parity for replay filters plus existing secure listener integration guards; Cargo manifests and lockfile unchanged. No dependency was added. |
| Listener pending secure-session scan helper slice | Add no dependency for Go `internal/runtime/listener/server.go` `pendingConfirmationPeerDIDs` and `readJSONFile`; use std filesystem/path handling plus existing JSON decoding only. | Pending-confirmation peer discovery is local identity-directory scanning. It reads `p5-e2ee-sessions/*.json`, skips read/decode errors, filters `status="pending-confirmation"`, trims only for blank-peer detection, and deduplicates peer DIDs without performing history sync. Adding WebSocket crates, HTTP/TLS clients, E2EE provider crates, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, or new SQLite dependencies would mix the later secure-session recovery workflow into this helper-only slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_listener_secure_sessions_contract --locked`; Go fallback guard with existing listener/message secure tests when no helper-specific Go tests exist; Cargo manifests and lockfile unchanged. No dependency was added. |
| Message WS proxy helper slice | Add no dependency for Go `internal/message/ws_proxy_client.go`; reuse the already ported local bridge helper and existing JSON types. | `WSProxyTransport` only packages method names, params, identity name, local bridge calls, and send-result decoding. Adding a WebSocket crate, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or a new SQLite dependency would mix later foreground listener/transport selection into this helper-only translation. Real CLI WebSocket routing and `handleBridgeRequest` remain separate dependency-reviewed slices and must stay Rustls-first. | `cargo +1.79.0 test -p awiki-cli --test message_ws_proxy_contract --locked`; Go focused WS proxy and `decodeMapInto` tests; full verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged. No dependency was added. |
| Workspace upgrade inspection slice | Add no dependency for Go `internal/upgrade/{types,meta,journal,detect}.go`; use serde JSON, std filesystem helpers, existing `durablefs`, existing identity/store scanners, and the approved `rusqlite + bundled` SQLite lane. | Workspace upgrade meta/journal/detection is local read-only state plus JSON persistence. Translating it now improves doctor/config parity without selecting HTTP/TLS, authsdk sessions, platform service-manager libraries, file-lock crates, or migration execution dependencies. Full legacy migration and identity replacement behavior remains separate. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs`; `crates/awiki-cli/tests/doctor_contract.rs`; focused core config-show test; Go focused `internal/upgrade` tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace upgrade fsutil slice | Add no dependency for Go `internal/upgrade/fsutil.go`; use `std::fs`, `std::io`, serde JSON, and the existing `durablefs` helper. | The helper is local filesystem plumbing shared by meta/journal persistence and future backup execution. Translating it separately keeps backup SQLite `VACUUM INTO`, rollback, and migration execution out of this slice while avoiding filesystem-copy crates such as `walkdir` or `fs_extra`. | `cargo +1.79.0 test -p awiki-cli upgrade::fsutil --locked`; focused workspace upgrade contract test; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace upgrade backup slice | Add no dependency for Go `internal/upgrade/backup.go`; reuse the existing fsutil helpers, existing store open path, and the already-approved `rusqlite + bundled` SQLite lane. | Backup is local file/SQLite state. SQLite backup must use `VACUUM INTO` rather than byte copying, and the project already documents `rusqlite + bundled` as the SQLite compatibility exception. This slice does not require HTTP/TLS, authsdk sessions, platform service-manager libraries, file-lock crates, or new filesystem-copy crates. | `cargo +1.79.0 test -p awiki-cli backup --locked`; workspace backup contract tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace upgrade upgrader plan skeleton slice | Add no dependency for Go `internal/upgrade/{types.go,upgrader.go}` planning boundaries; use `std::collections::BTreeMap`, existing `config::Resolved`, and existing upgrade types. | The slice fixes the Rust execution architecture boundary before full migration execution: Go-shaped `Context`, `Migration`, `Upgrader`, default 0->1->2->3 registration, plan ordering, and plan error strings. Migration `apply`/`validate`, `UpgradeIfNeeded`, lock/backup wiring, identity replacement RPC, legacy SQLite import, and cleanup stay deferred so dependency-sensitive service/runtime decisions are not mixed into planning parity. | `cargo +1.79.0 test -p awiki-cli upgrade::upgrader --locked`; focused workspace upgrader plan contract tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace UpgradeIfNeeded local v0->v1 apply wiring slice | Add no dependency for wiring already translated local v0->v1 migration behavior into the Go `UpgradeIfNeeded` phase loop; reuse existing inspect/meta/journal/backup/lock helpers and local migration helpers. | This slice lets the default migration chain execute local v0->v1 config/schema/import work and then continue into v1->v2 cleanup and v2->v3. It originally preserved a deferred imported-k1 boundary; the later shared workspace k1 replacement slice now removes that boundary by reusing the identity-service path. This local wiring slice itself does not introduce authsdk, HTTP/TLS, WebSocket, or rollback behavior. | `cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked`; full verification recorded in `docs/verification/`. No dependency was added; audit remains limited to existing Rustls/update paths and the approved bundled SQLite path. |
| Workspace shared k1 replacement slice | Add no dependency for Go `replaceImportedLegacyK1DIDs`, `replaceExistingWorkspaceK1DIDs`, and `replaceK1DIDsForSummaries`; reuse the already selected shared Rustls/std authsdk identity client, local identity replacement implementation, local ANP Rust DID/key generation, and the approved `rusqlite + bundled` store rebind helper. | Both v0->v1 imported legacy k1 replacement and v2->v3 current-workspace k1 replacement can be wired by composing existing Rust parity pieces: imported/current identity summaries, local manager load, handle-DID preflight, `identity::replace_did` for authenticated `did-auth.replace_did`, replacement backup and identity writeback, and `store::rebind_local_identity_state` for owner-DID rebinding. Adding a new HTTP stack, WebSocket crate, OpenSSL/`native-tls`, bundled OpenSSL, YAML parser, platform service library, or alternate SQLite backend would duplicate existing decisions and widen the migration slice beyond Go parity. | `cargo +1.79.0 test -p awiki-cli upgrade::migration_v2_to_v3 --locked`; `cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_if_needed_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test identity_replace_did_live_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test store_rebind_contract --locked`; dependency audit. No dependency was added; Cargo manifests and lockfile are unchanged. |
| Workspace k1 service-construction warning edge slice | Add no dependency for Go's `replaceK1DIDsForSummaries` service-construction ordering; reuse the existing Rustls/std `transportcfg::new_http_client` CA-bundle preflight already used by identity clients. | Go constructs `identity.NewService` before filtering k1 summaries, so a non-empty all-non-k1 identity list can still produce `Automatic k1 to e1 DID replacement was skipped: invalid ca bundle: ...`. Rust only needs to preflight the already-selected HTTP client construction before the helper loop; adding a new identity service object, HTTP stack, OpenSSL/`native-tls`, bundled OpenSSL, YAML parser, WebSocket crate, platform service library, or alternate SQLite backend would widen a warning-only ordering slice without improving parity. | Focused workspace upgrade-if-needed contract test plus existing k1 replacement, upgrade, identity replace-DID, store rebind, Go upgrade, and dependency audit verification recorded in `docs/verification/`. No dependency was added; Cargo manifests and lockfile are unchanged. |
| Workspace upgrade file lock slice | Add no file-lock dependency for Go `internal/upgrade/{lock,lock_nonwindows,lock_windows}.go`; use standard-library file handles plus minimal platform FFI for Unix `flock`/`kill(0)` and Windows `LockFileEx`/`UnlockFileEx`/`OpenProcess`. | The Go lock helper is a local concurrency primitive. Direct FFI keeps the port traceable to Go's OS calls, avoids adding a cross-platform locking crate before full upgrade execution exists, and does not link OpenSSL/native-tls, HTTP/TLS, service-manager, or host SQLite dependencies. Windows FFI is included for source parity but still needs future Windows host validation. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs` lock tests; Go `go test ./internal/upgrade -run 'TestAcquireFileLock' -count=1`; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace refresh resolved config helper slice | Add no dependency for Go `internal/upgrade/migration_v0_to_v1.go` `refreshResolvedConfig`; reuse the existing hand-written config parser and URL derivation helpers. | The helper is local config-state refresh performed after legacy identity import and before DID replacement. It does not execute service calls, mutate SQLite, perform TLS, or require a YAML dependency decision beyond the existing parser boundary. Keeping it separate avoids mixing config refresh parity with v0->v1 `Apply`, legacy SQLite import, identity replacement RPC, or `UpgradeIfNeeded` phase execution. | `cargo +1.79.0 test -p awiki-cli --test workspace_upgrade_contract workspace_upgrade_refresh_resolved_config --locked`; Go focused refresh tests; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace SQLite migration helper slice | Add no dependency for Go `internal/upgrade/migration_v0_to_v1.go` `ensureTargetStoreSchema` and `validateSQLiteHealth`; reuse existing `rusqlite + bundled` store APIs. | These helpers are local SQLite checks used by v0->v1 validation and schema preparation. The project already approved `rusqlite + bundled` for SQLite parity; this slice adds no new database crate and does not touch authsdk, HTTP/TLS, WebSocket, identity replacement RPC, or platform service-manager choices. | `cargo +1.79.0 test -p awiki-cli upgrade::migration_v0_to_v1 --locked`; `cargo +1.79.0 test -p awiki-cli --test workspace_migration_v0_to_v1_contract --locked`; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace v0->v1 validation wiring slice | Add no dependency for Go `workspaceV0ToV1Migration.Validate`; reuse the existing hand-written config parser, bundled SQLite store path, SQLite health helper, and local identity manager. | This validation path is pure local state checking after v0->v1 apply has run: config schema version, SQLite schema version, SQLite health pragmas, and imported-identity sanity when legacy identity was detected. It must be wired before full migration execution but does not require lock/backup orchestration, identity import, legacy SQLite import, DID replacement RPC, HTTP/TLS, WebSocket, or platform cleanup. | Focused v0->v1 migration contract tests and `upgrade::upgrader` tests passed; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace v0->v1 config apply branch slice | Add no dependency for the config-file branch of Go `workspaceV0ToV1Migration.Apply`; reuse the existing config writer and `serde_json` already in the workspace to parse legacy `config.json`. | Go reads legacy `config.json` through the YAML parser because YAML accepts JSON. Rust's hand-written YAML subset would otherwise ignore JSON, so this slice adds JSON parsing to the existing `FileConfig` shape without introducing a YAML dependency. It only handles existing config schema stamping, legacy config migration/removal, and legacy settings to minimal config. Identity import, legacy SQLite import, DID replacement RPC, HTTP/TLS, WebSocket, and full upgrade execution stay separate. | Focused v0->v1 migration contract tests and config writer tests passed; full verification recorded in `docs/verification/`. No dependency was added. |
| Workspace v0->v1 legacy local import slice | Add no dependency for the legacy identity + legacy SQLite import branch of Go `workspaceV0ToV1Migration.Apply`; reuse the existing local identity manager and the already-approved bundled SQLite store importer. | Go imports legacy identities before importing legacy SQLite so owner DIDs can be inferred for older schemas. This helper is local filesystem/SQLite state only. It should stay separate from config refresh, k1->e1 DID replacement RPC, lock/backup/journal/meta orchestration, HTTP/TLS, WebSocket, authsdk sessions, and cleanup migrations. | Focused v0->v1 migration contract tests, identity/store import contract tests, full `cargo +1.79.0 test -p awiki-cli --locked`, Go focused upgrade tests, structure check, build, and dependency audit passed. No new dependency was added; audit remains limited to existing Rustls/update paths and the approved `rusqlite + bundled` SQLite path. |
| Workspace v0->v1 local apply composition slice | Add no dependency for composing already translated local v0->v1 steps; reuse config writer, identity import, bundled SQLite import/schema helpers, and resolved-config refresh. | This slice proves Go ordering across local operations without selecting HTTP/TLS/authsdk dependencies or enabling remote k1->e1 DID replacement. The composition is now wired into default `Migration::apply` for local non-k1 cases; service-backed DID replacement remains separate because it requires auth/service dependencies and live RPC parity. | Focused v0->v1 migration contract tests, related identity/store/upgrade tests, full `cargo +1.79.0 test -p awiki-cli --locked`, Go focused upgrade tests, structure check, build, and dependency audit passed. No new dependency was added; audit remains limited to existing Rustls/update paths and the approved `rusqlite + bundled` SQLite path. |
| Workspace v1->v2 legacy cleanup slice | Add no dependency for Go `workspaceV1ToV2Migration.Apply`; use std filesystem and process command APIs with test seams for platform command shape. | The Go cleanup is local artifact removal plus external `launchctl`, `systemctl --user`, or `schtasks` command execution. Translating it directly does not require a service-manager crate, HTTP/TLS, authsdk, WebSocket, SQLite, or platform library linkage. Real host service behavior remains represented as command invocation, while portable tests inject a runner and verify paths/warnings. | Focused `migration_v1_to_v2` unit tests and `workspace_upgrade_contract` passed; full verification recorded in `docs/verification/`. No dependency was added; dependency policy remains Rustls-first and no OpenSSL/native-tls path was introduced. |
| Workspace legacy settings parser slice | Add no dependency for the `loadLegacySettings` helper in Go `internal/upgrade/migration_v0_to_v1.go`; use serde JSON plus the existing config URL normalizer. | Legacy settings parsing is pure local JSON/string normalization used by the future v0->v1 workspace migration. Translating it separately avoids mixing parser parity with config writes, identity import, DID replacement RPC, legacy SQLite import, lock handling, backups, or cleanup commands. | `crates/awiki-cli/tests/workspace_upgrade_contract.rs::workspace_upgrade_legacy_settings_parser_matches_go_contract`; Go `go test ./internal/upgrade -run TestLoadLegacySettingsRejectsSplitServiceURLs -count=1`; full verification recorded in `docs/verification/`. No dependency was added. |
| Runtime/listener local slice | Do not add platform service-manager dependencies for the current runtime/config slice. Use workspace-local listener state to satisfy the verified CLI contract while the full service-manager translation remains deferred. | Go uses platform service management through a library path. The user asked to avoid platform/system libraries where possible. The current system tests validate command shape, config writes, and listener lifecycle JSON contract; those can be met without linking systemd/launchd/Windows service libraries in this slice. | `crates/awiki-cli/tests/runtime_contract.rs`; `tests_v2/runtime/test_runtime_cli.py` passed. Dependency tree still has no OpenSSL/native-tls path and no new platform service library. |
| Mail local command slice | Add no HTTP/TLS dependency in the first mail slice. Translate CLI validation/dry-run contracts and local `mail notify` SQLite behavior only. | Non-dry-run mail RPC requires the authsdk/DID-WBA session chain plus an HTTP/TLS client choice. The project constraint says to avoid system SSL; that dependency decision should be made once for authsdk/mail/message service integrations, not hidden inside a dry-run/local-cache mail slice. | `crates/awiki-cli/tests/mail_contract.rs` passed. Dependency tree remains unchanged except the existing bundled SQLite path. |
| Config set slice | Add no dependency for `config set --did-domain`; use the existing hand-written config parser/writer. | Go behavior is a small config-file mutation with bare-domain normalization. A YAML dependency decision should be made for full config parity later, not introduced for this narrow command. | `crates/awiki-cli/tests/core_contract.rs` passed for dry-run, persistent write, validation, and side-effect checks. Dependency tree unchanged. |
| Config writer helper and durable-write slice | Add no dependency for `internal/config/write.go`; extend the existing hand-written config parser/writer with std-only durable replacement. | This bounded slice translates Go writer helper field mutation behavior, schema-version stamping, Hermes persistence, legacy webhook double-writes, and durable config replacement without changing the YAML dependency surface. The Rust writer uses standard-library same-directory temp files, file sync, Unix chmod/fsync behavior, and Windows parent-directory sync no-op to mirror Go `durablefs`. Full YAML parser/serializer parity remains a separate dependency/format decision. | `crates/awiki-cli/tests/config_writer_contract.rs`, `core_contract`, and `runtime_contract` passed. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls or new platform dependency was added. |
| Doctor local diagnostics slice | Add no dependency for `internal/doctor/doctor.go`; use existing config/identity/store/runtime modules plus `std::process::Command` for the local `anp-mls system version --json-in -` probe. | Go doctor is a local diagnostic aggregator. Translating its report contract now improves core parity without selecting HTTP/TLS, authsdk session, platform service-manager, or MLS provider crates. The external `anp-mls` check remains a health probe only, not group-E2EE provider execution. | `crates/awiki-cli/tests/doctor_contract.rs` passed. Dependency tree remains unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, WebSocket, or platform service-manager dependency was added. |
| Update/upgrade cache-only slice | Add no HTTP/TLS dependency for the first `upgrade` slice. Implement local cache parsing, version policy, and npm command boundary only. | Go `upgrade` normally fetches npm registry metadata over HTTPS, but the current system tests seed local cache and set `AWIKI_CLI_UPDATE_CACHE_ONLY=1`. Deferring network fetch avoids choosing an HTTP/TLS stack before the shared Rustls-based service integration decision. | `crates/awiki-cli/tests/update_contract.rs` and `tests_v2/update` passed. Dependency tree remains unchanged except the existing bundled SQLite path. |
| Update registry fetch/writeback slice | Add `rustls = 0.23` with `default-features = false, features = ["ring", "std", "tls12"]` plus `webpki-roots = 0.26` for Go `internal/update.fetchFromRegistry*`. | The Go update fetch is intentionally small and synchronous. A direct Rustls GET keeps TLS Rustls-first, avoids OpenSSL/native-tls and host cert-store coupling, preserves Go proxy behavior through `HTTP_PROXY`/`HTTPS_PROXY` and `NO_PROXY`, and avoids the Rust 1.79 lockfile churn seen with higher-level HTTP clients. `ring -> cc` is present as the Rustls crypto provider build dependency; this is not OpenSSL/native-tls but is recorded as native build surface. | `cargo +1.79.0 test -p awiki-cli update --locked`; `cargo +1.79.0 test -p awiki-cli --test update_contract --locked`; `go test ./internal/update`; dependency audit showed `rustls`, `rustls-webpki`, `webpki-roots`, and `ring -> cc`, with no OpenSSL/native-tls/reqwest/hyper. Live dry-run smoke returned network metadata from npm registry. |
| Root update preflight slice | Add no dependency for Go `internal/cli/root.go` update preflight; reuse the existing Rustls-backed `update::check` implementation and split the root guard into `app/update_preflight.rs` to keep `app.rs` under the review-size cap. | The guard is orchestration around existing config and update policy: command exemptions, soft-fail logging, `version_unsupported` errors, npm hint text, and warning injection. It should not select a new HTTP/TLS stack or alter registry fetch behavior. Subprocess tests set `AWIKI_CLI_UPDATE_CACHE_ONLY=1` only to keep unrelated CLI tests deterministic and offline; production still follows the normal cache/network policy. | Focused preflight helper tests, update contract tests, full `cargo +1.79.0 test -p awiki-cli --locked`, Go focused CLI/update tests, structure check, build, and dependency audit passed. No dependency was added; audit remained limited to existing Rustls/update paths plus the approved bundled SQLite path. |
| AuthSDK header/challenge helper slice | Add no dependency for Go `internal/authsdk/session.go` `Headers`/`ChallengeHeaders`; reuse the existing local `../anp/rust` SDK `DIDWbaAuthHeader` APIs. | Header generation is still transport-free: it constructs signed request headers, cached bearer headers, and challenge response auth headers but does not execute HTTP, retry a 401, or select service HTTP/WebSocket crates. Keeping this slice on the local ANP SDK avoids duplicating DID-WBA signing logic and preserves the Rustls-first service-client decision for the later transport lane. | `cargo +1.79.0 test -p awiki-cli --test authsdk_contract --locked`, `cargo +1.79.0 test -p awiki-cli authsdk --locked`, full verification, Go `go test ./internal/authsdk -count=1`, and dependency audit. No dependency was added; manifests and lockfile remained unchanged, and no OpenSSL/native-tls/reqwest/hyper/WebSocket path was introduced. |
| Store shared helpers slice | Add no dependency for Go `internal/store/helpers.go`; move already-translated helper primitives into `store/helpers.rs`. | Shared store helpers are standard-library/time/string coercions already used by the legacy import and owner-rebind lanes. Extracting them before recover-merge keeps the file-by-file translation traceable and avoids growing `store/import.rs` past the default 1200-line review threshold. | `crates/awiki-cli/tests/store_helpers_contract.rs`, `store_import_contract`, `store_rebind_contract`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| Store E2EE outbox DAO slice | Add no dependency for Go `internal/store/dao.go` E2EE outbox queue/status/query helpers; reuse the existing approved `rusqlite + bundled` store path and shared store normalization helpers. | These functions are local SQLite row insert/update/query helpers that unblock later secure-direct execution. They do not require authsdk, HTTP/TLS, WebSocket, ANP SDK file session stores, E2EE provider crates, platform service-manager libraries, YAML crates, or new SQLite dependencies. Translating them separately keeps the secure message execution lane from mixing storage parity with E2EE/transport decisions. | `cargo +1.79.0 test -p awiki-cli --test store_e2ee_outbox_contract --locked`; adjacent store regression and full verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Message secure status/failed/drop command slice | Add no dependency for Go `internal/message/secure_commands.go` `SecureStatus`, `SecureFailed`, and `SecureDrop`; reuse the existing message identity gate, filesystem JSON session status files, optional existing target resolver, and approved `rusqlite + bundled` outbox DAO path. | These command methods are local SQLite/filesystem status/list/mutation behavior around active identity state. `SecureStatus` only reads `p5-e2ee-sessions/*.json` and redacts data; `SecureFailed`/`SecureDrop` only list/update local outbox rows. They do not need authsdk message RPC execution beyond already-existing handle resolution, Rustls transport changes, WebSocket/local bridge routing, ANP SDK E2EE clients, session-store mutation, prekey publishing, or new crypto dependencies. Keeping them separate prevents `SecureRetry`, `SecureInit`, and `SecureRepair` network/E2EE concerns from being mixed into a local status command slice. | `cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked`; Go `go test ./internal/message -run 'TestServiceSecureStatusReturnsSessionAndOutboxSummary|TestServiceSecureFailedAndDropOperateOnOutbox' -count=1`; full verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Message secure retry injected store execution slice | Add no dependency for the store-backed portion of Go `SecureRetry` and `FlushQueuedSecureOutbox`; reuse the existing outbox DAO, message store DAO, approved `rusqlite + bundled` store path, and injected sender/session callbacks. | This slice proves the local side effects around retry without selecting the production E2EE/RPC dependency lane: it requeues the selected row, lists queued rows, filters by peer, validates JSON payloads before sender invocation, sends through an injected boundary, marks rows sent/failed, persists outgoing E2EE message rows, and preserves warning/data shapes. Real `NewSecureE2EEClientForRecord`, message-service E2EE transport, DID resolution, prekey publishing, WebSocket/RPC routing, and CLI non-dry-run retry wiring remain separate. Adding WebSocket crates, async runtimes, new E2EE provider crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or a new SQLite backend here would mix later transport/E2EE choices into a store-parity slice. | `cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked`; Go secure retry/flush focused guards; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added beyond existing approved SQLite/message store paths. |
| Message secure E2EE client local preparation slice | Add no dependency for the local preparation boundary of Go `NewSecureE2EEClientForRecord`; reuse the local `../anp/rust` key parser through the existing CLI ANP facade and the existing file-store facades for sessions, signed prekeys, and one-time prekeys. | This slice only prepares local key material, Go P5 store roots, key IDs, and local DID document resolution before production secure-direct client construction. It should not select or enable network/default ANP SDK features, WebSocket/RPC transport, new E2EE provider crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform libraries, or a new SQLite backend. The high-level `MessageServiceE2EEClient`, remote DID resolver fallback, prekey publishing, `SendText`/`SendJSON`, incoming processing, and CLI production secure execution remain separate parity slices. | `cargo +1.79.0 test -p awiki-cli --test message_secure_client_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test anpsdk_contract --locked`; Go secure-direct focused guards; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Message secure direct injectable service send slice | Add no dependency for Go `sendSecureDirect` service-level validation, queued pending-confirmation result, and secure sent-message persistence; reuse existing target resolution, approved `rusqlite + bundled` store helpers, and injected sender callbacks. | This slice intentionally avoids choosing the missing production E2EE/RPC client dependency lane. It proves the local service behavior around secure direct send inputs and results while deferring real `MessageServiceE2EEClient`, prekey publishing/retrieval, encryption/session mutation, WebSocket/RPC routing, and live CLI secure send wiring. Adding WebSocket crates, async runtimes, new E2EE provider crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or a new SQLite backend here would mix later transport/E2EE choices into a service-contract slice. | `cargo +1.79.0 test -p awiki-cli --test message_secure_send_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked`; Go secure direct send focused guards; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added beyond existing approved SQLite/message store paths. |
| Message secure init production sender slice | Add no dependency for Go `SecureInit` production execution; reuse the existing Rustls/std authsdk/message client, high-level `MessageServiceE2EEClient`, local ANP Rust E2EE adapter, and filesystem session-status helpers. | Manual secure init uses the same already-selected direct E2EE HTTP/RPC path as production secure send and retry: active identity loading, best-effort prekey publishing, session-file reuse checks, `SendJSON` over authenticated `/im/rpc`, and local P5 session stores. Adding WebSocket crates, async runtimes, new E2EE provider crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or a new SQLite backend would expand scope without improving this 1:1 production command slice. | `cargo +1.79.0 test -p awiki-cli --test message_secure_commands_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test msg_live_contract --locked`; Go `go test ./internal/message -run 'TestServiceSecureInitCreatesPendingSession' -count=1`; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Message secure repair production sender slice | Add no dependency for Go `SecureRepair` production execution; reuse the existing Rustls/std authsdk/message client, high-level `MessageServiceE2EEClient`, local ANP Rust E2EE adapter, `FileSessionStore`, and approved `rusqlite + bundled` outbox DAO path. | Secure repair composes already-selected pieces: shared secure peer preparation, local P5 session deletion, local failed-outbox requeue, and a second production `SecureInit` over authenticated `/im/rpc`. It should not add a new SQLite backend, WebSocket crate, async runtime, HTTP/TLS client, OpenSSL/`native-tls`, bundled OpenSSL, YAML crate, platform service library, or new E2EE provider dependency. WebSocket/local bridge repair execution and runtime listener acceptance remain separate transport/runtime slices. | `cargo +1.79.0 test -p awiki-cli --test msg_secure_repair_live_contract --locked`; Go `go test ./internal/message -run 'TestServiceSecureRepairResetsFailedOutboxAndStartsNewInit' -count=1`; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Message/group CLI exit mapper parity slice | Add no dependency for Go `internal/cli/msg.go` `messageExit` unsupported-mode and transport-unavailable branches; update only the existing `msg_handlers` mapper and CLI contract tests. | This is CLI error-shape parity, not transport implementation. It maps existing Rust `MessageError` variants to Go codes/hints and adds tests around already-translated message/group commands plus an internal mapper branch for `TransportUnavailable`. It should not add WebSocket crates, async runtimes, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or new SQLite dependencies. Rust-only `AttachmentNotSupported` and `GroupNotSupported` keep the existing `not_implemented` mapping because Go has no equivalent sentinels. | `cargo +1.79.0 test -p awiki-cli --test msg_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test group_contract --locked`; `cargo +1.79.0 test -p awiki-cli app::msg_handlers::tests::message_exit_maps_transport_unavailable_like_go --locked`; Go `go test ./internal/cli -run 'TestMsg' -count=1`; full verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Store rebind slice | Add no dependency for Go `internal/store/rebind.go` and the owner-DID rebind/E2EE cleanup helpers from `internal/store/dao.go`; reuse the existing `rusqlite + bundled` SQLite lane. | The rebind helpers are local SQLite updates/deletes used after identity replacement. They do not need authsdk, HTTP/TLS, crypto, WebSocket, or platform service-manager crates, and should not force any service dependency decision before non-dry-run identity replacement is translated. | `crates/awiki-cli/tests/store_rebind_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No new dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| Store recover-merge slice | Add no dependency for Go `internal/store/recover_merge.go`; reuse the existing `rusqlite + bundled` SQLite lane. | Recover merge is a local SQLite normalization/upsert/delete transaction used after handle/DID recovery. It needs no authsdk, HTTP/TLS, WebSocket, crypto, or platform service-manager dependency. Keeping it store-only avoids mixing CLI/service execution with translation and keeps TLS/OpenSSL decisions unchanged. | `crates/awiki-cli/tests/store_recover_merge_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, Go `go test ./internal/store`, structure check, build, and dependency audit. No new dependency was added; audit stayed limited to existing Rustls/update paths and the approved bundled SQLite path. |
| README asset parity | Add no dependency for Go `README.md`; copy the public README byte-for-byte as a repository documentation asset. | The README is user-facing documentation for install/onboarding, project layout, config template location, and support links. Copying it preserves the Go public documentation contract without changing Rust runtime behavior, package metadata, parser behavior, or release wiring. Missing linked docs/assets are tracked as later documentation parity slices rather than optimized or rewritten here. | `cmp -s ../awiki-cli/README.md README.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Onboarding guide asset parity | Add no dependency for Go `onboarding.md`; copy the first-install guide byte-for-byte as a repository documentation asset. | The onboarding guide is user-facing procedure documentation and does not participate in Rust compilation, CLI dispatch, config parsing, or packaging. Copying it preserves the Go documentation contract while keeping link cleanup, localization, and README reference fixes as later documentation-maintenance work rather than translation work. | `cmp -s ../awiki-cli/onboarding.md onboarding.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Installation guide asset parity | Add no dependency for Go `docs/installation.md`; copy the installation/local-development guide byte-for-byte as a repository documentation asset. | The installation guide documents Go 1.22, pure-Go SQLite, runtime setup, build/test commands, and troubleshooting. Copying it preserves source documentation without changing Rust dependency choices, Cargo metadata, SQLite implementation, TLS policy, runtime behavior, or package scripts. Rust-specific installation guidance should be added later as docs-maintenance work, not mixed into 1:1 translation. | `cmp -s ../awiki-cli/docs/installation.md docs/installation.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Publish guide asset parity | Add no dependency for Go `docs/publish.md`; copy the release/rollback handbook byte-for-byte as a repository documentation asset. | The publish guide documents release scripts, tags, npm publication, Gitee sync, rollback, and CI expectations, but copying the handbook does not make those release paths active in the Rust repo. Release scripts, `.goreleaser.yml`, and GitHub workflows need separate parity slices and must not be executed as part of this documentation copy. | `cmp -s ../awiki-cli/docs/publish.md docs/publish.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Command architecture doc asset parity | Add no dependency for Go `docs/architecture/awiki-command-v2.md`; copy the README-linked command architecture document byte-for-byte. | The architecture document explains command grouping, output protocol, schema, dry-run, shortcuts, and Skill splitting. It is a documentation asset and does not participate in Rust compilation or CLI dispatch. Although the source text says the implementation and release model switched to Go, preserving that wording is part of 1:1 documentation parity; Rust-specific caveats and cleanup belong in a later docs-maintenance lane. | `cmp -s ../awiki-cli/docs/architecture/awiki-command-v2.md docs/architecture/awiki-command-v2.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| V2 system architecture doc asset parity | Add no dependency for Go `docs/architecture/awiki-v2-architecture.md`; copy the README-linked system architecture document byte-for-byte. | The v2 architecture document records design goals, module boundaries, Go single-binary selection, distribution, migration, Skill architecture, and runtime mode concepts. It is a documentation asset and does not alter Rust module structure, dependencies, or package behavior. The Go-specific language is preserved for source parity; Rust-specific caveats and cleanup belong in a later docs-maintenance lane. | `cmp -s ../awiki-cli/docs/architecture/awiki-v2-architecture.md docs/architecture/awiki-v2-architecture.md`; preserved mode/line-count checks; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Config template asset parity | Add no dependency for Go `config.template.yaml`; copy the public YAML template byte-for-byte as a repository asset. | The template documents canonical user-facing config keys and defaults, but copying it does not change Rust config parsing or runtime behavior. Keeping it as an asset slice avoids mixing documentation/template parity with the separate full YAML parser/serializer decision. | `cmp -s ../awiki-cli/config.template.yaml config.template.yaml`; copied-template `config show --format json` smoke through the existing Rust parser; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| npm install script parity | Copy Go `package.json`, `scripts/install.js`, and `scripts/run.js` for the package/install surface. | `awiki-system-test` validates the Node installer against the selected Rust repo. The Go package contract uses Node, curl, tar, and PowerShell on Windows; changing it would not be a Rust port optimization and would break 1:1 packaging behavior. | `tests_v2/update/test_install_script.py` passed with a local mirror archive and fake curl failure. |
| Host-notify webhook helper asset parity | Add no Rust dependency for Go `scripts/host_notify_webhook_server.py`; copy the Python stdlib-only helper byte-for-byte as a script asset. | The helper is a local OpenClaw/Hermes host-notify fan-out server used by Go architecture notes and manual/local tests. Rewriting it in Rust or wiring it into the CLI would mix a dev/test asset parity slice with runtime listener/service-manager work. Keep it as the same Python script and verify byte identity plus Python compilation. | `cmp -s ../awiki-cli/scripts/host_notify_webhook_server.py scripts/host_notify_webhook_server.py`; `python3 -m py_compile scripts/host_notify_webhook_server.py`; Go repo same Python compile check; structure, whitespace, and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Identity/group dry-run CLI slice | Add no dependency for `id replace-did --dry-run` or `group create/update --dry-run`; use static plan builders and existing config resolution. | These commands are currently verified as CLI contract surfaces. Real replace-did and group RPC execution require authsdk/message-service/store-rebind decisions that should not be mixed into dry-run translation. | `crates/awiki-cli/tests/identity_contract.rs`, `crates/awiki-cli/tests/group_contract.rs`, and the two focused `awiki-system-test` selectors passed. Dependency tree unchanged. |
| Identity handle input helper slice | Add no dependency for Go `internal/identity/handle_input.go`; move handle normalization/completion and stored-handle derivation into `identity::handle_input`. | The Go helper is pure string/DID-path normalization used by identity storage and CLI handle completion. Consolidating `msg` and non-E2EE `group` callers on the identity helper removes duplicated, divergent local logic without selecting authsdk, HTTP/TLS, WebSocket, crypto, MLS, or platform dependencies. | `crates/awiki-cli/tests/identity_contract.rs::identity_handle_input_helpers_match_go_contract`, `identity_contract full_handle`, full identity contract test, full `cargo +1.79.0 test -p awiki-cli --locked`, structure check, build, and dependency audit passed. Dependency tree unchanged except existing approved bundled SQLite and existing Rustls/update paths; no OpenSSL/native-tls, HTTP/TLS client, WebSocket, or platform service dependency was added. |
| Group non-E2EE dry-run lifecycle slice | Add no dependency for `group get/join/add/remove/leave/list/members/messages --dry-run`; use static plan builders and existing config resolution. | The Go dry-run contracts do not require network/auth execution. Real group RPC and group E2EE require shared authsdk/message-service/MLS dependency decisions and should stay out of this dry-run slice. | `crates/awiki-cli/tests/group_contract.rs` passed. Dependency tree unchanged. |
| Group base/local wire builder slice | Add no dependency for `internal/message/group_wire.go` base/local request builders; reuse existing local ANP SDK proof generation and current message helper crates only. | This slice constructs JSON-RPC params and RFC9421 origin-proof auth values but does not execute service calls. Transport, JWT refresh, WebSocket, cache mutation, and MLS provider execution remain deferred to the shared Rustls/authsdk/group-E2EE slices. | `crates/awiki-cli/tests/message_group_wire_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, `xtask check-structure`, build, dependency audit, and accepted `awiki-system-test` selector set passed. Dependency tree remained limited to the already approved bundled SQLite path; no OpenSSL/native-tls/TLS client path was added. |
| Group non-E2EE live HTTP slice | Add no dependency for ordinary non-E2EE group lifecycle, member, list, message-list, and text-send execution; reuse existing authsdk, Rustls/std `transportcfg::HttpClient`, local ANP origin-proof helper, and approved `rusqlite + bundled` store path. | The shared authsdk/Rustls transport already satisfies Go `/im/rpc` HTTP parity for `group.create`, `group.get`, `group.join`, `group.add`, `group.remove`, `group.leave`, `group.update_profile`, `group.update_policy`, `group.list`, `group.list_members`, `group.list_messages`, and `group.send`. Adding `reqwest`, `hyper`, WebSocket crates, OpenSSL/native-tls, YAML crates, platform libraries, or ANP SDK network/default features would expand scope without improving this parity slice. Group attachments, group E2EE/MLS execution, WebSocket/local bridge/runtime listener fallback, OpenClaw host notify, and deeper local DB/handle/fallback trace wiring remain separate decisions. | Focused local Rust checks and five remote `awiki-system-test` group selectors against `awiki.info` passed. Cargo manifests and lockfile are unchanged; dependency tree remains on existing Rustls/webpki/ring and approved bundled SQLite paths, with no OpenSSL/native-tls, `reqwest`, `hyper`, WebSocket, YAML, or platform service dependency added. Later shared profile-timeout verification is recorded separately. |
| Shared service profile-timeout slice | Add no dependency for Go `transportcfg.WithProfileTimeout` integration across `authsdk` plus mail/content/site/identity/message clients; reuse the existing Rustls/std `transportcfg::HttpClient`. | Go service clients already choose `AuthRefresh`, `RpcDefault`, and `RpcReadHeavy` profiles. Rust clients already carried those profile values, so the parity fix is to pass them into a per-request timeout cap on the existing pure-Rust/Rustls HTTP path instead of adding `reqwest`, `hyper`, async runtimes, OpenSSL, `native-tls`, or bundled OpenSSL. The std blocking client can cap response reads without widening the base HTTP timeout; full Go context coverage for dial/TLS/write phases remains a later transport-depth item. | `cargo +1.79.0 test -p awiki-cli --test transportcfg_http_contract --locked`; `cargo +1.79.0 check -p awiki-cli --locked`; later full verification commands recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Group E2EE wire builder slice | Add no dependency for `internal/message/group_wire.go` E2EE request builders; reuse the existing local ANP SDK proof generation and JSON helper crates only. | These builders construct signed hidden E2EE JSON-RPC params and sanitize opaque provider artifacts, but they do not invoke `anp-mls`, call message service RPCs, refresh auth sessions, select transport, or mutate cache. MLS/provider execution remains a separate local-ANP-SDK/service slice. | `crates/awiki-cli/tests/message_group_e2ee_wire_contract.rs`, full `cargo +1.79.0 test -p awiki-cli --locked`, `xtask check-structure`, build, dependency audit, and accepted `awiki-system-test` selector set passed. Dependency tree remained limited to the already approved bundled SQLite path; no OpenSSL/native-tls/TLS client path was added. |
| Group E2EE dry-run CLI slice | Add no dependency for `group e2ee ... --dry-run`; model provider metadata and plans without invoking `anp-mls`. | Go dry-run plans expose the intended MLS/provider orchestration without executing the provider. Real MLS execution should be implemented with the local ANP Rust tooling and focused security/system tests, not hidden inside static CLI plan translation. | `crates/awiki-cli/tests/group_contract.rs` passed. Dependency tree unchanged. |
| Group E2EE status/pending live slice | Add no dependency for non-dry-run `group e2ee status` or `group e2ee pending`; use `std::process::Command` only for the external status `anp-mls group status --json-in -` boundary and the existing authsdk + Rustls/std message HTTP client for hidden `group.e2ee.head` and `group.e2ee.notice`. | Go status is a diagnostic/recovery-inspection command around the external `anp-mls` CLI plus existing message-service RPCs. Go pending is only an authenticated hidden `group.e2ee.notice` pull with limit `50`, `mark_delivered=false`, no `notice_ids`, and optional group filtering; it does not invoke MLS. Reusing `std::process`, existing `serde_json`, existing `sha2`/`base64`, existing authsdk/session, existing Rustls/std transport, and the existing group E2EE wire builders avoids adding MLS provider crates, async runtimes, `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or a new SQLite backend. The slice deliberately keeps publish/repair/recover/update/rejoin provider mutations separate. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_status_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test group_e2ee_pending_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test group_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test message_group_e2ee_wire_contract --locked`; Go focused status and dry-run references passed. Cargo manifests and lockfile are unchanged; dependency audit remains limited to existing Rustls/webpki/ring and approved bundled SQLite paths. Later publish/add/remove/recover slices reuse the same dependency boundary instead of adding new transport/MLS crates. |
| Group E2EE add/rejoin live slice | Add no dependency for live `group add --e2ee` or hidden `group e2ee rejoin`; reuse the existing external local ANP Rust SDK binary boundary, authsdk/session, Rustls/std message HTTP client, group E2EE wire builders, and approved `rusqlite + bundled` store path. | Go add/rejoin does not require a new in-process MLS crate or transport stack. It performs normal P4 `group.add`, syncs group state, leases a service-verified KeyPackage through hidden `group.e2ee.get_key_package`, invokes `anp-mls group add-member --json-in - --data-dir <scoped-dir>`, submits hidden `group.e2ee.add`, persists local summary metadata, and optionally runs `anp-mls welcome process` only for a local added identity. Reusing the existing provider and transport helpers keeps TLS Rustls-first and avoids adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, async runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK default/network features, or a new SQLite backend. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_add_contract --locked`; adjacent group-E2EE/group/wire tests, structure check, Go focused references, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; audit remained limited to existing Rustls/webpki/ring/sha2/base64 and approved bundled SQLite paths. |
| Group E2EE remove/leave live slice | Add no dependency for live `group remove --e2ee`, live `group leave --e2ee`, or hidden `group e2ee process-leave-request`; reuse the existing external local ANP Rust SDK binary boundary, authsdk/session, Rustls/std message HTTP client, group E2EE wire builders, and approved `rusqlite + bundled` store path. | Go remove/leave does not require a new in-process MLS crate or transport stack. Removal prepares an epoch-advancing commit through `anp-mls group remove-member --json-in - --data-dir <scoped-dir>`, submits hidden `group.e2ee.remove`, finalizes with `group commit-finalize`, persists local summary metadata, and syncs group state. E2EE self-leave creates only hidden `group.e2ee.leave_request`; it does not run local MLS leave or P4 `group.leave`. Reusing the existing provider and transport helpers keeps TLS Rustls-first and avoids adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, async runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK default/network features, or a new SQLite backend. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_remove_leave_contract --locked`; adjacent group-E2EE/group/wire tests, structure check, Go focused references, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; audit remained limited to existing Rustls/webpki/ring/sha2/base64 and approved bundled SQLite paths. |
| Group E2EE recover-member live slice | Add no dependency for live hidden `group e2ee recover-member`; reuse the existing external local ANP Rust SDK binary boundary, authsdk/session, Rustls/std message HTTP client, group E2EE wire builders, and approved `rusqlite + bundled` store path. | Go active-member recovery does not require a new in-process MLS crate or transport stack. It checks hidden service head eligibility, leases a recovery KeyPackage through `group.e2ee.get_key_package` with `purpose=recovery`, prepares an epoch-advancing recovery commit through `anp-mls group recover-member-prepare --json-in - --data-dir <scoped-dir>`, submits hidden `group.e2ee.recover_member`, finalizes with generic `group commit-finalize`, persists local summary metadata, and optionally processes a local welcome. Reusing the existing provider, transport, summary, and welcome helpers keeps TLS Rustls-first and avoids adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, async runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK default/network features, or a new SQLite backend. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_recover_member_contract --locked`; adjacent group-E2EE/group/wire tests, structure check, Go focused references, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; audit remained limited to existing Rustls/webpki/ring/sha2/base64 and approved bundled SQLite paths. |
| Group E2EE repair live slice | Add no dependency for live `group e2ee repair`; reuse the existing external local ANP Rust SDK binary boundary, authsdk/session, Rustls/std message HTTP client, group E2EE wire builders, status/recovery helpers, and approved `rusqlite + bundled` store path. | Go repair is a replay/recovery command around existing service notices and local MLS state. It preflights hidden `group.e2ee.head`, finalizes already accepted local pending commits through `anp-mls group commit-finalize`, pulls hidden `group.e2ee.notice`, replays commit notices through `anp-mls commit process`, replays welcome/recovery/update welcomes through `anp-mls welcome process`, marks processed notice IDs delivered through a second hidden notice call, and reports local recovery diagnosis. Reusing the existing provider/transport/status pieces keeps TLS Rustls-first and avoids adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, async runtimes, YAML crates, platform service libraries, MLS provider crates, ANP SDK default/network features, or a new SQLite backend. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_repair_contract --locked`; adjacent group-E2EE/group/wire tests, structure check, Go focused status references, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; audit remained limited to existing Rustls/webpki/ring/sha2/base64 and approved bundled SQLite paths. |
| Group E2EE outbound send live slice | Add no dependency for live outbound `msg send --group --secure on --text` or cached/local E2EE auto-upgrade; reuse the existing external local ANP Rust SDK binary boundary, authsdk/session, Rustls/std message HTTP client, group E2EE wire builders, repair helper, and approved `rusqlite + bundled` store path. | Go outbound group E2EE send shells out to `anp-mls message encrypt`, posts hidden `group.e2ee.send`, persists a local group message, and retries once after stale epoch repair. It does not require a new in-process MLS crate, async runtime, transport stack, or SQLite backend. Reusing the existing provider and transport helpers keeps TLS Rustls-first and avoids adding `reqwest`, `hyper`, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, MLS provider crates, ANP SDK default/network features, or pure-Rust SQLite optimization work during this parity slice. | `cargo +1.79.0 test -p awiki-cli --test group_e2ee_send_contract --locked`; adjacent group-E2EE/group/wire tests, structure check, Go focused references where present, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; audit remained limited to existing Rustls/webpki/ring/sha2/base64 and approved bundled SQLite paths. |
| Local CLI validation selector slice | Add no dependency for Go-shaped `msg attachment download` target validation and `id profile set` body-source validation. Keep real profile RPC and attachment transfer deferred. | These checks are command/service-boundary argument validation in Go and can run before auth, HTTP/TLS, WebSocket, or attachment transfer code. Translating them separately unlocks offline `awiki-system-test` selectors without forcing a shared service transport dependency decision. | `crates/awiki-cli/tests/msg_contract.rs`, `crates/awiki-cli/tests/identity_contract.rs`, and the focused offline system-test selector batch passed. Dependency tree unchanged except existing approved bundled SQLite and existing Rustls/update paths; no OpenSSL/native-tls, HTTP/TLS client, WebSocket, or bundled OpenSSL path was added. |
| Mail remote wire contract slice | Add no dependency for the pure `internal/mail/client.go` and `service.go` RPC wire/error/summary contract. Keep `NewClient`, HTTP execution, DID-auth session construction, JWT refresh, CA bundle handling, and attachment file writes deferred. | The Go mail service methods have a useful pure boundary: endpoint, method names, transport profiles, JSON params, validation errors, result summaries, and RPC/HTTP `ServiceError` display can be translated and unit-tested before selecting the shared Rustls HTTP stack. Wiring fake non-dry-run mail execution would break parity, and wiring real execution here would duplicate the pending authsdk/session transport decision. | `cargo +1.79.0 test -p awiki-cli --test mail_wire_contract --locked` passed before full verification. Cargo manifests and lockfile are unchanged; this slice adds no `reqwest`, `hyper`, WebSocket crate, OpenSSL, `native-tls`, bundled OpenSSL, or ANP SDK network/default feature. Future live mail RPC must reuse these builders with the Rustls-first shared client. |
| Page dry-run CLI slice | Add no HTTP/TLS dependency for `page create/list/get/update/rename/delete --dry-run`; use static plan builders and local markdown-file reads only. | Go dry-run page contracts expose `/content/rpc` request metadata without making network calls. Real page CRUD requires active identity auth, DID-auth JWT refresh, and content RPC over HTTP, so it belongs in the shared authsdk + Rustls HTTP slice rather than this CLI-contract translation. | `crates/awiki-cli/tests/page_contract.rs` passed. Dependency tree unchanged. |
| Msg dry-run CLI slice | Add no HTTP/TLS, WebSocket, or E2EE execution dependency for `msg send/attachment download/inbox/history/mark-read/secure ... --dry-run`; use static plan builders and local text-file reads only. | Go dry-run contracts expose service intent without executing message RPC, WebSocket proxy transport, attachment transfer, or secure direct E2EE. Those paths require authsdk/session, message-service clients, Rustls HTTP/WS dependency selection, and E2EE provider decisions, so they should not be hidden inside this CLI-boundary translation. | `crates/awiki-cli/tests/msg_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Direct message live HTTP slice | Add no dependency for ordinary direct text message execution; reuse existing authsdk, Rustls/std `transportcfg::HttpClient`, local ANP proof, and approved `rusqlite + bundled` store path. | The shared authsdk/Rustls transport already satisfies Go `/im/rpc` HTTP parity for `direct.send`, `inbox.get`, `direct.get_history`, and `inbox.mark_read`. Adding `reqwest`, `hyper`, WebSocket crates, OpenSSL/native-tls, YAML crates, platform libraries, or ANP SDK network/default features would expand scope without improving this parity slice. Direct attachments are covered by the current attachment live HTTP decision; secure direct E2EE, group execution, and WebSocket/local bridge transport remain separate dependency decisions. | Focused local Rust checks and two remote `awiki-system-test` direct selectors against `awiki.info` passed. Cargo manifests and lockfile are unchanged; dependency tree remains on existing Rustls/webpki/ring and approved bundled SQLite paths, with no OpenSSL/native-tls, `reqwest`, `hyper`, WebSocket, YAML, or platform service dependency added. |
| Direct msg mark-read WebSocket/local bridge slice | Add no dependency for ordinary direct mark-read websocket-mode execution; reuse the existing std local bridge helper, `WSProxyTransport`, Rustls/std HTTP fallback path, authsdk session, and approved `rusqlite + bundled` store path. | Go `Service.MarkRead` routes direct IDs through `transportFor(record)`, which selects the local bridge in `runtime.mode=websocket`, then falls back to HTTP `/im/rpc inbox.mark_read` with a visible WebSocket HTTP fallback warning before applying the local SQLite read mutation. The bridge is already exposed through the local runtime socket helper, so adding `tungstenite`, `tokio-tungstenite`, `coder/websocket`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, ANP SDK network/default features, or a new SQLite backend would expand scope without improving this parity slice. | Focused Rust/Go mark-read verification, adjacent message regressions, structure check, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; no dependency was added. |
| Direct msg history WebSocket/local bridge slice | Add no dependency for ordinary direct history websocket-mode execution; reuse the existing std local bridge helper, `WSProxyTransport`, Rustls/std HTTP fallback path, authsdk session, direct E2EE display filtering, contact-handle cache helpers, and approved `rusqlite + bundled` store path. | Go `Service.History` selects the local bridge in `runtime.mode=websocket`, falls back first to local SQLite cache when usable, then to HTTP `/im/rpc direct.get_history` with a visible WebSocket HTTP fallback warning. All required pieces already exist in the current CLI: local bridge framing, signed HTTP RPC execution, cache reads, and handle-history merge. Adding `tungstenite`, `tokio-tungstenite`, `coder/websocket`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, ANP SDK network/default features, or a new SQLite backend would expand scope without improving this parity slice. | Focused Rust/Go history verification, adjacent message regressions, structure check, whitespace check, and dependency audit passed. Cargo manifests and lockfile are unchanged; no dependency was added. |
| Direct msg inbox WebSocket/local bridge slice | Add no dependency for ordinary direct/non-`all` inbox websocket-mode execution; reuse the existing std local bridge helper, `WSProxyTransport`, Rustls/std HTTP fallback path, authsdk session, direct E2EE display filtering, contact-handle cache helpers, and approved `rusqlite + bundled` store path. | Go direct `Service.Inbox` selects the local bridge in `runtime.mode=websocket`, falls back first to local SQLite direct inbox cache when usable, then to HTTP `/im/rpc inbox.get` with a visible WebSocket HTTP fallback warning. The direct path already has all required Rust pieces: local bridge framing, signed HTTP RPC execution, cache reads, handle-history merge, and mark-read mutation. Adding `tungstenite`, `tokio-tungstenite`, `coder/websocket`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, ANP SDK network/default features, or a new SQLite backend would expand scope without improving this direct parity slice. `scope=all` remains a separate Go `allInbox` cache-merge path and is not covered by this dependency decision. | Focused Rust direct inbox websocket verification, adjacent message regressions, structure check, whitespace check, Go reference selectors, and dependency audit passed. Cargo manifests and lockfile are unchanged; no dependency was added. |
| Ordinary group send/messages WebSocket/local bridge slice | Add no dependency for ordinary non-E2EE group text send or group message listing websocket-mode execution; reuse the existing std local bridge helper, `WSProxyTransport`, Rustls/std HTTP fallback path, authsdk session, group E2EE decrypt hook, and approved `rusqlite + bundled` store path. | Go `sendGroup` and `GroupMessages` select the local bridge in `runtime.mode=websocket`. Ordinary group send falls back to HTTP `/im/rpc group.send` with a visible WebSocket HTTP fallback warning and source `remote_http`; group messages falls back first to local SQLite group-message cache when usable, then to HTTP `/im/rpc group.list_messages` with the same warning. The Rust CLI already has all required pieces: local bridge framing, signed HTTP RPC execution, cache persistence/projection, warning helpers, trace fallback, and group E2EE decrypt-before-persist. Adding `tungstenite`, `tokio-tungstenite`, `coder/websocket`, `reqwest`, `hyper`, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, ANP SDK network/default features, or a new SQLite backend would expand scope without improving this parity slice. Group lifecycle/control commands intentionally remain HTTP-only in websocket mode, matching Go. | Focused Rust group websocket contract, adjacent group/E2EE/message regressions, structure check, whitespace check, Go reference selectors, and dependency audit passed. Cargo manifests and lockfile are unchanged; no dependency was added. |
| Explicit group inbox local-cache slice | Add no dependency for Go `Service.groupInbox`; reuse the existing approved `rusqlite + bundled` store lane and existing `MarkRead` local classification for group rows. | Go `groupInbox` is local-cache only: it requires an active identity, reads `ListGroupInboxMessages` through `groupStorageKey(request.Group)`, optionally filters unread rows, and best-effort marks returned IDs read. The current Rust CLI already has the needed local store helper and mark-read classification, so adding WebSocket crates, HTTP clients, OpenSSL/`native-tls`, bundled OpenSSL, YAML crates, platform service libraries, ANP SDK network/default features, or another SQLite backend would mix unrelated transport work into this local-cache parity slice. The route boundary is also dependency-free: `--group` alone still follows Go's default `scope=all` path instead of implying `scope=group`. | Focused Rust group/all inbox contract tests, adjacent message/group regressions, structure check, whitespace check, Go inbox/store/CLI reference selectors, and dependency audit passed. Cargo manifests and lockfile are unchanged; no dependency was added. |
| Secure control helper and queued outbox local helper slice | Add no dependency for Go `internal/message/secure_control.go` control helpers, `currentSecureSessionID`, `queueSecureOutboxRecord`, or the deterministic row-loop subset of `FlushQueuedSecureOutbox`; reuse existing `serde_json`, approved `rusqlite + bundled` store helpers, and the existing local ANP facade `FileSessionStore`. | The control helpers are JSON/string coercion, current-session lookup is local file-store scanning, queued outbox insertion is existing SQLite DAO composition, and the queued flush row loop can be locked with injected queued rows/send/store outcomes. Translating these local helpers still avoids constructing a real E2EE client, sending WebSocket RPCs, or executing full queued flush before the corresponding secure-direct execution slices are ready. Adding WebSocket crates, async runtime crates, new E2EE provider crates, HTTP/TLS clients, OpenSSL/`native-tls`, bundled OpenSSL, platform service libraries, YAML crates, or new SQLite dependencies would mix later secure-direct transport/execution into this helper slice. | `cargo +1.79.0 test -p awiki-cli --test message_secure_outbox_flush_contract --locked`; Go source parity plus focused secure listener/message guards; Cargo manifests and lockfile unchanged. No dependency was added beyond the existing approved SQLite and local ANP facade paths. |
| Direct/group attachment live HTTP slice | Add no dependency for direct or group attachment send/download execution; reuse existing authsdk, Rustls/std `transportcfg::HttpClient`, local ANP proof, and approved `rusqlite + bundled` store path. | Attachment upload/download can be translated on the already-approved HTTP stack: attachment slot and commit RPCs use authsdk, object upload uses HTTP `PUT`, download ticket lookup uses authsdk, and object download uses the same Rustls/std client with bearer ticket headers. Under `runtime.mode=websocket`, attachment send/download force HTTP and warn rather than selecting a new WebSocket transport in this parity slice. Adding `reqwest`, `hyper`, WebSocket crates, OpenSSL/native-tls, bundled OpenSSL, YAML crates, platform libraries, or ANP SDK network/default features would expand dependencies without improving current 1:1 attachment parity. | Focused local attachment tests, full `awiki-cli` tests, structure check, binary build, dependency audit, and remote `awiki-system-test` selector `tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_send_and_download_group_attachments` passed. Cargo manifests and lockfile are unchanged; dependency tree remains on existing Rustls/webpki/ring and approved bundled SQLite paths, with no OpenSSL/native-tls, `reqwest`, `hyper`, WebSocket, YAML, or platform service dependency added. |
| Site dry-run CLI slice | Add no HTTP/TLS dependency for `site root/page ... --dry-run`; use static plan builders and local markdown-file reads only. | Go dry-run site contracts expose `/site/rpc` request metadata without making network calls. The later tenant site live RPC slice now wires real execution through the existing shared authsdk + Rustls HTTP stack instead of changing the dry-run dependency boundary. | `crates/awiki-cli/tests/site_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Message pure foundation slice | Add no dependency for message request builders, attachment manifest/selection, DID-document service selection, or fallback warning text. | These helpers are pure JSON/value transformations and validation logic in Go. Porting them before real transport reduces risk for the later message-service slice while staying within the no-new-dependency lane. | `crates/awiki-cli/tests/message_contract.rs` passed. Dependency tree unchanged except the existing approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, WebSocket, or new crypto dependency was added. |
| Message RFC9421 origin-proof slice | Reuse the existing local ANP Rust SDK proof/key APIs; add no new dependency. | Go `internal/message/proof.go` signs direct payloads through ANP helpers. The Rust port can preserve this local proof boundary with the already-approved local `../anp/rust` path dependency, without introducing auth session refresh, HTTP/TLS, WebSocket, or additional crypto crates. | `crates/awiki-cli/tests/message_contract.rs` passed with origin-proof generation, canonical digest comparison, and DID-document verification. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls or HTTP/TLS crate was added. |
| Message signed wire params slice | Add no dependency for signed direct text and direct/group attachment manifest request params; reuse the local origin-proof helper. | Go wire builders return signed JSON params before transport. Translating that boundary now proves signed payload shape while still deferring authsdk session refresh, HTTP/WS clients, attachment transfer, and cache mutation to service slices. | `crates/awiki-cli/tests/message_contract.rs` passed with signed direct send and signed attachment manifest proof verification. Dependency tree unchanged except the approved bundled SQLite path; no OpenSSL/native-tls, HTTP/TLS, or WebSocket crate was added. |
| Trace/transport config foundation slice | Add no dependency for Go `internal/traceutil/trace.go` or the pure timeout/profile resolver from `internal/transportcfg/config.go`. Defer `NewHTTPClient` to a dedicated Rustls-first client slice. | Trace formatting and timeout env resolution are pure std/local behavior. Implementing `NewHTTPClient` now would mix translation with HTTP/TLS root-store, custom CA bundle, HTTP/2, and pooling dependency decisions. The user clarified TLS should be Rustls-first and bundled OpenSSL must not be the preferred portability path. | `crates/awiki-cli/tests/traceutil_contract.rs`, `crates/awiki-cli/tests/transportcfg_contract.rs`, full Rust test/build/structure checks, Go `go test ./internal/traceutil ./internal/transportcfg`, and dependency audit. No dependency was added; future HTTP/TLS work must keep OpenSSL/native-tls out unless a documented Rustls parity failure exists. |
| Trace timing integration slice | Add no dependency for Go trace timing production wiring across CLI execution and shared service RPC/JWT paths; reuse the existing local `traceutil` module and Rustls/std authsdk transport. | Go trace integration is orchestration and instrumentation around already-translated command rendering, config resolution, RPC calls, and JWT refresh. Adding logging/tracing crates, async runtimes, HTTP clients, OpenSSL/native-tls, WebSocket crates, YAML crates, or platform libraries would expand scope without improving 1:1 visible trace output. The Rust port uses a thread-local current run instead of Go `context.Context` because this CLI path is synchronous. | `cargo +1.79.0 test -p awiki-cli --test core_contract trace_timing --locked`; `cargo +1.79.0 test -p awiki-cli --test mail_live_contract mail_inbox_trace_timing_reports_remote_rpc_phase --locked -- --exact`; `cargo +1.79.0 test -p awiki-cli --test mail_live_contract mail_inbox_trace_timing_reports_bootstrap_jwt_without_nested_get_me_rpc --locked -- --exact`; full verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Transportcfg Rustls HTTP client slice | Add no dependency for Go `internal/transportcfg.NewHTTPClient`; reuse existing `rustls` + `webpki-roots` and the standard library instead of adding `reqwest`, `hyper`, OpenSSL, `native-tls`, or WebSocket crates. | The immediate parity need is a shared HTTP/1.1 boundary for future service clients: `Resolve()` timeout snapshot, optional PEM CA bundle appending, Rustls TLS, JSON request/response bytes, chunked decoding, and Go-like CA bundle error strings. Direct std sockets keep the dependency tree unchanged while preserving the Rustls-first TLS policy. Go `transportcfg.NewHTTPClient` uses a custom `http.Transport` without `Proxy`, so Rust `new_http_client` remains proxy-free; update's separate default-client proxy behavior is exposed through an explicit proxy-env constructor. The `awiki.info` page system test exposed a Rustls EOF difference when a peer omits TLS `close_notify`; the shared client now tolerates that only after an HTTP-framed response is complete, matching Go's effective behavior without swallowing incomplete responses. HTTP/2, exact system-root parity, keepalive/pooling reuse, streaming bodies, and full service-client behavior are recorded as deferred parity work, not optimized into this slice. | `crates/awiki-cli/tests/transportcfg_http_contract.rs`, `crates/awiki-cli/tests/transportcfg_contract.rs`, `cargo +1.79.0 test -p awiki-cli update --locked`, full Rust test/build/structure checks, Go `go test ./internal/transportcfg ./internal/update`, `tests_v2/page`, and dependency audit. Cargo manifests and lockfile remain unchanged; allowed audit hits stay limited to existing Rustls/webpki/ring and approved bundled SQLite paths. |
| OpenClaw route confirmation webhook slice | Add no dependency for Go `internal/runtime/openclawnotify/webhook.go`; reuse the existing Rustls/std `transportcfg::HttpClient` and route/config helpers. | Route confirmation is a single HTTP JSON POST using the already-translated loopback hook URL settings, explicit AWiki config hook URLs, `OPENCLAW_GATEWAY_PORT` auto-detection, and optional config/env bearer token. Adding `reqwest`, `hyper`, OpenSSL, `native-tls`, bundled OpenSSL, WebSocket crates, YAML crates, platform service libraries, or new SQLite dependencies would expand scope without improving 1:1 route-add confirmation parity. | Focused `openclaw_webhook` unit test, focused `runtime_contract` route tests, package check, full runtime contract test, structure check, whitespace check, and dependency audit are recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| OpenClaw JSON config probe slice | Add no dependency for Go `internal/runtime/openclawnotify/config.go`; use existing `serde_json`, std filesystem/env path handling, and the existing route/config helpers. | OpenClaw JSON probing only needs local JSON decode of `gateway.port`, `hooks.path`, and `hooks.token`, plus env/home path resolution and URL path normalization. Reusing `serde_json` avoids YAML/parser or HTTP-client additions while preserving Go's typed JSON parse fallback, positive-`int` port behavior, and config/env/OpenClaw token precedence. Adding a config parser, `url`/path crate, HTTP client, TLS stack, platform library, OpenSSL/native-tls, bundled OpenSSL, or SQLite dependency would expand scope without improving this local probe parity. | Focused `runtime_openclaw_config_contract`, focused `runtime_contract host_notify_openclaw`, package check, full runtime contract test, structure check, whitespace check, Go `openclawnotify` config tests, and dependency audit are recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Hermes route ensure local writer slice | Add no dependency for the pure/local subset of Go `internal/runtime/hermesbridge.EnsureRoute`; use std filesystem/string handling, the existing scalar inspection helper, existing `rand`, and a narrow deterministic renderer instead of adding `serde_yaml`. | The immediate parity need is local Hermes `config.yaml` route setup before live CLI setup wiring: create the webhook route when missing, generate or preserve a route secret, preserve existing positive port and existing events, migrate legacy notify prompts, remove fixed `deliver_extra` target keys, remove only the legacy single `skills: ["notify"]`, preserve custom prompts/skills and unmanaged blocks, write through a same-directory temp file, and return the existing `InspectRoute` state. Adding a YAML crate in this slice would widen the dependency tree before the full parser/serializer decision; the narrow writer documents that it is not full `yaml.v3` round-trip parity for comments, anchors, complex scalars, or arbitrary formatting. Adding HTTP/TLS clients, WebSocket crates, platform service libraries, OpenSSL, `native-tls`, bundled OpenSSL, or SQLite dependencies would also mix later live setup/bridge orchestration into a local helper slice. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_ensure_route_contract --locked`; `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_contract --locked`; Go `go test ./internal/runtime/hermesbridge -count=1`; final package/structure/dependency verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Hermes bridge service local helper slice | Add no dependency for the deterministic local subset of Go `internal/runtime/hermesbridge/service.go`; use existing `sha2` plus std path/time/polling/PATH/filesystem lookup APIs. | Service naming, display-name derivation, service config shape, adapter command planning, Python executable lookup, adapter script lookup, status aggregation with injected service/health outcomes, hidden `runtime host-notify hermes bridge service-run` detection, health/readiness polling, lifecycle branch planning, and pure `Apply` branch selection can be translated before selecting any platform service-manager, process supervisor, or HTTP health-probe client. Adding a `kardianos/service` equivalent, systemd/launchd/Windows service crates, process-supervision abstraction, new HTTP/TLS clients, WebSocket crates, OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, or SQLite dependencies here would mix dependency-sensitive bridge execution with pure helper parity. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_bridge_service_contract --locked`; `cargo +1.79.0 test -p awiki-cli runtime::hermes_bridge::tests --lib --locked`; adjacent Hermes bridge/CLI/setup tests; Go `go test ./internal/runtime/hermesbridge -count=1`; full verification and dependency audit recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Hermes setup local transaction slice | Add no dependency for the local-file half of Go `runtime host-notify hermes setup`; reuse the existing awiki config writer, existing Hermes route writer, and passive local status helpers. | Go setup combines local writes with listener restart and bridge service application. This slice intentionally translates only the dependency-free local transaction: validate inputs, resolve/generate/redact secret, write `runtime.host_notify` as Hermes, mirror legacy webhook notify URL/secret, ensure the local Hermes `notify` route, and return current listener/passive bridge status plus explicit deferred warnings. Adding service-manager crates, process supervisors, HTTP health clients, WebSocket crates, YAML crates, OpenSSL, `native-tls`, bundled OpenSSL, or SQLite dependencies would mix platform/runtime execution into the file-write parity step. | `cargo +1.79.0 test -p awiki-cli --test runtime_hermes_setup_dry_run_contract --locked`; adjacent Hermes config/route/service tests; Go CLI/config/Hermes bridge focused tests; final package/structure/dependency verification recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |
| Identity key compatibility slice | Add no dependency for Go `internal/identity/key_compat.go`; use existing local ANP Rust SDK key crates, std filesystem handling, and the existing identity secure-text writer. | The compatibility pass is a local PEM migration before identity load. It can parse supported legacy ANP private labels and SEC1 EC private-key DER with already-present Rust crypto/PKCS#8 crates in `../anp/rust`, then rewrite standard PKCS#8. Adding OpenSSL/native-tls, bundled OpenSSL, system crypto libraries, a PEM/ASN.1 crate, HTTP/TLS crates, platform libraries, or new SQLite dependencies would expand scope without improving this local identity parity. The standard ANP runtime parser remains PKCS#8-only. | Focused ANP Rust key PEM tests, focused Rust CLI identity key-compat tests, package check, structure check, whitespace check, Go identity key-compat tests, and dependency audit are recorded in `docs/verification/`. Cargo manifests and lockfile unchanged; no dependency was added. |

## Known Deferred Decisions

| Area | Deferred Decision | Required Evidence Before Adoption |
| --- | --- | --- |
| Full YAML config parsing | Choose a parser that preserves Go YAML behavior without introducing unnecessary native dependencies. | Go config fixture parity tests, environment override tests, and dependency-tree review. |
| Hermes live setup and bridge orchestration | Wire non-dry-run `runtime host-notify hermes setup` after local route ensure, awiki config write, listener refresh/restart, bridge apply/start, service-run, and health probing are each translated. | Must preserve Go setup output, route-state warnings, listener refresh behavior, bridge process/service behavior, health probes, and dependency-tree evidence showing no OpenSSL/native-tls or unnecessary platform libraries. |
| Full Hermes YAML round-trip parity | Decide whether the narrow Hermes route writer is sufficient or whether a full parser/serializer is needed for comments, anchors, complex scalars, and arbitrary existing Hermes config formatting. | Must compare Go `yaml.v3` fixtures against Rust output, prove preservation of unmanaged config, and document dependency impact before adding a YAML crate. |
| SQLite crate/backend | Current accepted lane is `rusqlite + bundled` for exact SQLite behavior and runtime portability. Keep pure Rust alternatives recorded for later optimization review, not mixed into this parity translation. | Exact schema/migration parity, query behavior parity, no host SQLite dependency, and system-test debug/store evidence. |
| HTTP/WebSocket service client stack | Select Rustls-based HTTP/WebSocket crates for service integrations beyond the update registry GET helper. Bundled OpenSSL is not the default fallback and requires a separate exception record if Rustls cannot meet parity. | Feature audit showing no OpenSSL/native-tls path and service-backed system tests. |
| Message WebSocket/local bridge and secure direct clients | Translate WebSocket/local bridge runtime transport and secure direct E2EE execution after the shared authsdk/session, Rustls HTTP/WS stack, and E2EE provider decisions are selected. Direct/group HTTP attachment transfer is covered by the current attachment live HTTP slice; remaining transport work is about WebSocket/local bridge behavior and secure outbox execution. | Must preserve message RPC status/error mapping, runtime-mode transport behavior, local cache writes, HTTP attachment manifest/upload/download semantics already ported for the HTTP path, secure outbox retry/drop behavior, and no OpenSSL/native-tls or bundled OpenSSL path without a separate documented exception. |
| Auth/session-backed signed service calls | Combine the verified local RFC9421 origin-proof helper with `authsdk` session/JWT refresh and service transport in a later slice. | Must preserve bearer/JWT refresh semantics, signed direct/group/attachment request execution, status/error mapping, and no OpenSSL/native-tls path. |
| Local websocket bridge I/O | Translate the remaining Windows named-pipe dial/listen/availability/health I/O and listener foreground service I/O after the shared runtime/message service boundary is ready. Unix socket `CallLocalBridge`, dial/listen, and health probe behavior is covered by the Unix I/O slice; deterministic Windows endpoint helper behavior is covered by the dependency-free bridge helper slice. | Must preserve Go bridge error phases, deadlines/timeouts from `transportcfg`, request/response JSON framing, listener cache fallback behavior, and platform-specific I/O without adding OpenSSL/native-tls or unnecessary service-manager dependencies. Any Windows named-pipe crate or direct Windows API dependency needs a separate dependency review. |
| Remaining `transportcfg.NewHTTPClient` depth | Extend the current Rustls/std client only where future service slices require deeper parity. | Still-deferred evidence areas: HTTP/2 `ForceAttemptHTTP2` behavior, exact Go system-root-store parity versus `webpki-roots`, keepalive/pooling reuse from `IdleConnTimeout`/`MaxIdleConns`/`MaxIdleConnsPerHost`, streaming request/response bodies, and integration with authsdk/mail/content/site/message service error mapping. No OpenSSL/native-tls or bundled OpenSSL path unless a separate documented exception proves Rustls cannot match required parity. |
| Platform service-manager integration | Decide whether to translate Go listener service control with a cross-platform Rust crate, direct per-platform code, or a no-platform-dependency supervisor strategy. | Must compare native/platform dependencies, service behavior parity, and `AWIKI_ENABLE_LISTENER_SERVICE_TESTS=1` behavior before adoption. Do not mix this choice into unrelated runtime config translation. |

## Mail Slice Notes

2026-05-15:

- Added the live mail RPC client and CLI execution slice after the shared
  authsdk/session plus Rustls HTTP client lane became available.
- Reused the existing Rustls/std `transportcfg::HttpClient` and authsdk session
  execution for non-dry-run `mail inbox/read/mark-read/account/send/attachment
  download`.
- Added only direct `base64 = 0.22` for attachment `content_base64` decoding;
  the crate is pure Rust and was already present transitively through the local
  ANP SDK path.
- Verification is recorded in `docs/verification/`; no OpenSSL/native-tls,
  WebSocket, `reqwest`, `hyper`, or ANP network/default feature was added.

## Direct Message Slice Notes

2026-05-15:

- Added the ordinary direct text message live HTTP slice for Go
  `internal/cli/msg.go`, `internal/message/service.go`,
  `internal/message/http_client.go`, and the message-cache helpers from
  `internal/store/dao.go` and `internal/store/query.go`.
- Reused the existing Rustls/std `transportcfg::HttpClient`,
  `authsdk::Session`, local ANP origin-proof helper, and approved
  `rusqlite + bundled` SQLite lane for `msg send --to --text/--text-file`,
  `msg inbox`, `msg history --with`, and `msg mark-read`.
- No dependency was added. Cargo manifests and lockfile remain unchanged; this
  slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
  `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or
  ANP SDK network/default features.
- Remaining message-service work is deliberately split: secure direct E2EE,
  group E2EE/MLS, WebSocket/local bridge/runtime listener transport, OpenClaw
  host notify, and deeper local DB/handle/cache/fallback trace wiring remain
  later parity slices. Direct/group
  attachments are now covered by the attachment live HTTP slice below, ordinary
  non-E2EE group lifecycle/messages are covered by the dedicated group live
  slice, and shared profile timeout caps are covered by the service
  profile-timeout slice.

2026-05-16:

- Added the first production WebSocket/local bridge execution slice for
  ordinary direct text `msg send --to --text` only.
- No dependency was added. The slice reuses the already-translated std Unix
  local bridge helper, existing `WSProxyTransport`, existing Rustls/std HTTP
  fallback path, authsdk/session, local ANP origin-proof helper, and approved
  `rusqlite + bundled` store path.
- The direct-send fallback preserves Go's visible asymmetry: bridge failure
  followed by successful HTTP fallback records trace fallback
  `websocket_to_http` but does not add the `websocketHTTPFallbackWarning`
  warning used by inbox/history/mark-read/group paths.
- This slice does not add `reqwest`, `hyper`, WebSocket crates, async runtimes,
  OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service
  libraries, ANP SDK network/default features, or a new SQLite backend.
- Remaining WebSocket work stays split into later parity slices: direct
  inbox/history local bridge and cache fallback, non-E2EE group
  send/messages fallback, foreground listener bridge dispatch, secure direct
  runtime acceptance, and awiki-system-test secure-direct coverage.
- Added the ordinary direct `msg mark-read` WebSocket/local bridge slice in a
  dedicated module with no dependency changes. It reuses `WSProxyTransport`,
  Rustls/std HTTP fallback, authsdk/session, and approved `rusqlite + bundled`
  store mutation, while preserving Go's HTTP fallback warning and original
  bridge-error-on-double-failure behavior.
- Added the ordinary direct/non-`all` `msg inbox --scope direct`
  WebSocket/local bridge slice in a dedicated module with no dependency
  changes. It reuses `WSProxyTransport`, Rustls/std HTTP fallback,
  authsdk/session, direct E2EE display filtering, contact-handle cache helpers,
  and approved `rusqlite + bundled` store reads/mutation. This records only the
  direct path; Go `scope=all` routes to `allInbox` with separate unified
  direct/group/mail cache semantics and remains a later parity slice.
- Added the default `msg inbox` / `scope=all` `allInbox` local-cache merge
  slice without adding a dependency. It reuses the already-approved
  `rusqlite + bundled` store lane for unified direct inbox, group inbox, and
  mail notification cache queries, plus existing direct inbox and mark-read
  modules for the fallback path. No manifest or lockfile change was needed.
- Added the explicit `msg inbox --scope group` local-cache slice without adding
  a dependency. It reuses the same approved SQLite lane, `groupStorageKey`
  helper, `ListGroupInboxMessages`, and existing local mark-read
  classification. It also records the Go routing detail that `--group` alone
  does not imply `scope=group`; default empty scope still becomes `all`.
- This `allInbox` slice deliberately keeps optimization separate from
  translation. It duplicates the Go-shaped mail notification normalization in
  the message inbox module for 1:1 parity instead of consolidating it with the
  mail module during the port. A later optimization pass may deduplicate that
  logic after parity evidence is complete.
- The slice does not add `reqwest`, `hyper`, WebSocket crates, async runtimes,
  OpenSSL, `native-tls`, bundled OpenSSL, YAML crates, platform service
  libraries, ANP SDK network/default features, or a new SQLite backend. TLS
  policy remains Rustls-first for later runtime/WebSocket transport work.

## Attachment Live HTTP Slice Notes

2026-05-15:

- Added the direct/group attachment live HTTP slice for Go
  `internal/cli/msg.go`, `internal/message/service.go`, attachment wire/control
  helpers, and attachment-aware direct/group send/download flows.
- Reused the existing Rustls/std `transportcfg::HttpClient`,
  `authsdk::Session`, local ANP origin-proof helper, attachment service
  discovery helpers, and approved `rusqlite + bundled` SQLite lane for
  `msg send --to/--group --file` and `msg attachment download --with/--group`.
- Attachment send/download intentionally force HTTP transport and emit a warning
  when `runtime.mode` is `websocket`; WebSocket/local bridge attachment
  transport remains a later parity slice.
- No dependency was added. Cargo manifests and lockfile remain unchanged; this
  slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
  `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or
  ANP SDK network/default features.
- Focused local attachment tests, full `awiki-cli` tests, structure check,
  binary build, dependency audit, and remote `awiki-system-test` selector
  `tests_v2/cli/test_awiki_cli_group_local.py::test_awiki_cli_can_send_and_download_group_attachments`
  passed. The remote selector ran in `AWIKI_SYSTEM_TEST_MODE=remote` against
  `https://awiki.info` / `wss://awiki.info/im/ws` with
  `AWIKI_CLI_UNDER_TEST=rust` and reported 1 passed, 0 failed, 0 skipped in
  3.23s.
- Focused local attachment tests also assert the non-dry-run forced-HTTP warning
  strings for both attachment send and attachment download when
  `runtime.mode=websocket`.

## Group Live Slice Notes

2026-05-15:

- Added the ordinary non-E2EE group live HTTP slice for Go
  `internal/cli/group.go`, `internal/message/group_service.go`,
  `internal/message/group_wire.go`, and the group/message cache helpers from
  `internal/store/dao.go` and `internal/store/query.go`.
- Reused the existing Rustls/std `transportcfg::HttpClient`,
  `authsdk::Session`, local ANP origin-proof helper, and approved
  `rusqlite + bundled` SQLite lane for group lifecycle/read/member/message
  operations and `msg send --group --text`.
- No dependency was added. Cargo manifests and lockfile remain unchanged; this
  slice does not add `reqwest`, `hyper`, WebSocket crates, OpenSSL,
  `native-tls`, bundled OpenSSL, YAML crates, platform service libraries, or
  ANP SDK network/default features.
- `created_at` in signed message/group metadata is now emitted with
  Go-compatible second-precision RFC3339 UTC text so message-service typed
  `Meta` reserialization verifies the RFC9421 origin-proof digest.
- Remaining group work is deliberately split: group E2EE/MLS execution,
  WebSocket/local bridge/runtime listener fallback, OpenClaw host notify, trace
  phase plumbing, and cache fallback depth remain later parity slices. Group
  attachment send and download over forced HTTP are covered by the attachment
  live HTTP slice above; shared profile timeout caps are covered by the later
  service profile-timeout slice.

Earlier 2026-05-15 wire-only slice:

- Added `mail::wire` for the pure remote RPC contract from Go
  `internal/mail/client.go` and `internal/mail/service.go`.
- Preserved `/mail/rpc`, method names, read-heavy/default transport profiles,
  params, service-boundary validation errors, result summary strings, and
  RPC/HTTP `ServiceError` display text without executing network calls.
- Verification: `cargo +1.79.0 test -p awiki-cli --test mail_wire_contract
  --locked` passed before the full verification run recorded in
  `docs/verification/`.

2026-05-14:

- Added a split `mail` module for command-plan data and local notification
  service behavior, plus `app/mail_handlers.rs` for the Go
  `internal/cli/mail.go` CLI boundary.
- No new dependency was added. At the time of this local slice, remote mail RPC
  was deferred until the shared authsdk/HTTP client slice chose a Rustls-based
  stack; the later live mail RPC slice now reuses that Rustls path.
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
- Later slices translated registry fetch/writeback with direct Rustls and the
  root update preflight guard without adding another dependency. The preflight
  guard reuses `update::check`, keeps Go's soft-fail behavior, and leaves
  service-client HTTP/WebSocket dependency decisions separate.
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

## Direct E2EE Client Adapter Notes

2026-05-16:

- Added the first high-level secure direct E2EE client adapter without adding
  a new crate or enabling local ANP SDK default/network features.
- The adapter reuses the existing local `../anp/rust` direct-E2EE primitives,
  file stores, DID document builders, key material types, `base64`, and `rand`
  dependencies already present in the workspace dependency graph.
- A direct `x25519-dalek` dependency was considered during implementation but
  rejected because the local ANP SDK already exposes enough key-generation and
  key-material APIs for this slice. Avoiding the direct dependency keeps the
  CLI manifest unchanged and preserves the "do not add dependencies unless
  required" constraint.
- The slice does not add HTTP/TLS, WebSocket, OpenSSL, `native-tls`, bundled
  OpenSSL, platform service libraries, YAML crates, or a new SQLite path. TLS
  policy remains Rustls-first for later production transport wiring.
- The incoming `ProcessIncoming`/`DecryptHistoryPage` adapter was added in a
  follow-up slice without changing the dependency graph. Production
  `msg send --secure on` wiring, inbox/history secure decrypt application,
  listener secure decrypt/ack integration, and awiki-system-test secure-direct
  acceptance remain separate translation slices.

## Direct E2EE Incoming Application Notes

2026-05-16:

- Added the message-service incoming direct E2EE application layer without
  adding a new crate, changing Cargo manifests, or enabling local ANP SDK
  default/network features.
- The slice reuses the existing Rustls/std authsdk/message client for
  authenticated `/im/rpc` calls, the existing high-level
  `MessageServiceE2EEClient` adapter, existing `serde_json`, existing message
  store helpers, and the approved `rusqlite + bundled` SQLite lane.
- No TLS/WebSocket dependency decision changed. The slice does not add
  OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates,
  async runtimes, YAML crates, platform service libraries, new E2EE provider
  crates, or a new SQLite backend. TLS remains Rustls-first for later
  WebSocket/runtime transport work.
- The polling secure ACK/flush follow-up wires the Go
  `maybeFlushPollingSecureAck`, `maybeAckPollingDirectInit`, and
  `directInitSessionIDFromMessage` side effects onto the same selected stack:
  existing Rustls/std authsdk/message client, existing local ANP E2EE adapter,
  existing secure outbox flush helper, and approved `rusqlite + bundled`
  SQLite lane.
- No TLS/WebSocket dependency decision changed for that follow-up. It still does
  not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket
  crates, async runtimes, YAML crates, platform service libraries, new E2EE
  provider crates, or a new SQLite backend. TLS remains Rustls-first for later
  WebSocket/runtime transport work.

## Direct E2EE Production Send Notes

2026-05-16:

- Wired production `msg send --secure on` to the existing high-level direct E2EE
  adapter and the existing Rustls/std authenticated message HTTP client without
  adding a new crate, changing Cargo manifests, or enabling local ANP SDK
  default/network features.
- The slice reuses the same selected dependency stack as the incoming
  application/ACK slices: local `../anp/rust` E2EE primitives and file stores,
  existing authsdk/session/Rustls HTTP transport, existing `serde_json`, existing
  store helpers, and the approved `rusqlite + bundled` SQLite lane.
- No TLS/WebSocket dependency decision changed. This slice does not add
  OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates,
  async runtimes, YAML crates, platform service libraries, new E2EE provider
  crates, or a new SQLite backend. TLS remains Rustls-first for later
  WebSocket/local bridge secure transport work.

## Direct E2EE Production Retry Notes

2026-05-16:

- Wired production `msg secure retry <OUTBOX_ID>` to the same high-level direct
  E2EE adapter and existing Rustls/std authenticated message HTTP client used by
  production secure direct send.
- No new crate, manifest change, or local ANP SDK feature change was needed.
  The slice reuses local `../anp/rust` E2EE session/prekey stores, existing
  authsdk/session/Rustls HTTP transport, existing `serde_json`, existing secure
  outbox/store helpers, and the approved `rusqlite + bundled` SQLite lane.
- Go's initialization boundary is preserved: if the secure outbox sender cannot
  be initialized after the selected row is reset to `queued`, retry returns a
  warning and leaves the row queued instead of marking it as send-failed.
- No TLS/WebSocket dependency decision changed. This slice does not add
  OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`, WebSocket crates,
  async runtimes, YAML crates, platform service libraries, new E2EE provider
  crates, or a new SQLite backend. TLS remains Rustls-first for later
  WebSocket/local bridge secure transport work.

## Group E2EE Status/Pending Live Notes

2026-05-16:

- Wired non-dry-run `group e2ee status` without adding a crate or changing
  Cargo manifests. The MLS provider boundary uses `std::process::Command` to
  invoke the external `anp-mls group status --json-in -` command, matching the
  Go provider's stdin/stdout JSON contract, scoped data-dir layout, binary
  resolution order, executable checks, and 15-second timeout.
- Hidden service status calls reuse the existing authsdk/session and Rustls/std
  message HTTP client, plus the already translated `group.e2ee.head` and
  `group.e2ee.notice` wire builders.
- Wired non-dry-run `group e2ee pending` without adding a crate or changing
  Cargo manifests. The command reuses the same authsdk/session and Rustls/std
  message HTTP client, calls the hidden `group.e2ee.notice` method with limit
  `50`, leaves `mark_delivered` false, sends no `notice_ids`, omits
  `group_did` when the CLI filter is blank, and does not invoke `anp-mls`.
- No TLS, SQLite, WebSocket, platform, or ANP SDK dependency decision changed.
  This slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, or a new SQLite backend. TLS remains
  Rustls-first for message-service HTTP, and SQLite remains on the approved
  `rusqlite + bundled` path.
- `repair`, `update-key`, E2EE membership mutation, active-member recovery,
  commit/welcome replay, and MLS state mutation remain separate translation
  slices.

## Group E2EE KeyPackage Publish Live Notes

2026-05-16:

- Wired non-dry-run `group e2ee publish-key-package` without adding a crate,
  changing Cargo manifests, or enabling local ANP SDK MLS features in-process.
- The MLS provider boundary remains the external local ANP Rust SDK binary:
  `anp-mls key-package generate --json-in - --data-dir <scoped-dir>`. The
  shared exec provider now lives in `message/group_e2ee_provider.rs` so status
  and publish reuse the same binary resolution order, scoped data-dir layout,
  executable checks, stdin/stdout JSON contract, and 15-second timeout without
  growing `group_e2ee_status.rs`.
- The publish slice reuses existing local ANP Rust proof/key APIs through the
  CLI facade for DID-WBA binding signing: `load_private_key_material`,
  `verification_method_id_from_document`, and `generate_did_wba_binding`.
- Service publish reuses the existing authsdk/session and Rustls/std message
  HTTP client, plus the existing `group.e2ee.publish_key_package` wire builder
  and sanitizer. No TLS dependency decision changed; TLS remains Rustls-first.
- No SQLite, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, or a new SQLite backend. SQLite remains on
  the approved `rusqlite + bundled` path.
- `repair`, `update-key`, group membership mutation, active-member recovery,
  commit/welcome replay, service-head mutation, and MLS cache mutation remain
  separate translation slices. Any later optimization of the provider boundary
  must be recorded separately and not mixed into the 1:1 translation lane.

## Group E2EE Create Live Notes

2026-05-16:

- Wired live `group create --e2ee` without adding a crate, changing Cargo
  manifests, enabling local ANP SDK MLS features in-process, or changing the
  approved SQLite/TLS lanes.
- The MLS bootstrap remains an external-provider parity boundary:
  `anp-mls group create --json-in - --data-dir <scoped-dir>`. It reuses the
  same `message/group_e2ee_provider.rs` binary resolution order, scoped data-dir
  layout, executable checks, stdin/stdout JSON contract, and 15-second timeout
  as the status and publish slices.
- Hidden create delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client. A small shared `group_e2ee_transport` helper now owns the
  configured-service-DID-first plus capabilities-fallback behavior used by both
  publish and create; this is a translation-support extraction, not a broader
  transport redesign.
- Local E2EE summary persistence reuses the existing group SQLite cache and the
  approved `rusqlite + bundled` lane. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`, `hyper`,
  WebSocket crates, async runtimes, YAML crates, platform service libraries, MLS
  provider crates, or a new SQLite backend. TLS remains Rustls-first for
  message-service HTTP and future WebSocket/local bridge work.
- `group remove/leave --e2ee`, active-member recovery, `update-key`,
  repair, commit/welcome replay, group E2EE send/decrypt, and full
  awiki-system-test group-E2EE acceptance remain separate translation slices.
  Any provider-boundary optimization should be recorded later and not mixed into
  the 1:1 translation lane.

## Group E2EE Add/Rejoin Live Notes

2026-05-16:

- Wired live `group add --e2ee` and live hidden `group e2ee rejoin` without
  adding a crate, changing Cargo manifests, enabling local ANP SDK MLS features
  in-process, or changing the approved SQLite/TLS lanes.
- The MLS add-member boundary remains external-provider parity:
  `anp-mls group add-member --json-in - --data-dir <scoped-dir>`. The existing
  `message/group_e2ee_provider.rs` now also exposes `add_member` and
  `process_welcome` wrappers while preserving binary resolution order, scoped
  data-dir layout, executable checks, stdin/stdout JSON contract, and 15-second
  timeout.
- The service flow mirrors Go's two-plane mutation: first normal P4
  `group.add`, then group snapshot/member sync, then hidden
  `group.e2ee.get_key_package`, MLS add-member, hidden `group.e2ee.add`,
  summary persistence, and optional local welcome processing for a local added
  identity.
- Hidden RPC delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client through `message/group_e2ee_transport.rs`; TLS remains
  Rustls-first.
- Local E2EE summary persistence reuses the existing group SQLite cache and the
  approved `rusqlite + bundled` lane. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- The `group e2ee rejoin` app handler is a Go-shaped wrapper over
  `message::add_group_member` with `e2ee=true`; it inserts the Go plan into the
  live result and keeps the removed/left rejoin hint distinct from
  `recover-member`.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend.
- `group remove/leave --e2ee`, active-member recovery, `update-key`, repair,
  commit replay beyond local welcome processing, group E2EE send/decrypt,
  WebSocket local bridge group E2EE transport, and full awiki-system-test
  group-E2EE acceptance remain separate translation slices. Any
  provider-boundary optimization should be recorded later and not mixed into the
  1:1 translation lane.

## Group E2EE Remove/Leave Live Notes

2026-05-16:

- Wired live `group remove --e2ee`, live `group leave --e2ee`, and live hidden
  `group e2ee process-leave-request` without adding a crate, changing Cargo
  manifests, enabling local ANP SDK MLS features in-process, or changing the
  approved SQLite/TLS lanes.
- The MLS remove boundary remains external-provider parity:
  `anp-mls group remove-member --json-in - --data-dir <scoped-dir>`, followed
  by `anp-mls group commit-finalize --json-in - --data-dir <scoped-dir>` after
  hidden service acceptance. The existing `message/group_e2ee_provider.rs`
  now also exposes `remove_member`, `commit_finalize`, and `commit_abort`
  wrappers while preserving binary resolution order, scoped data-dir layout,
  executable checks, stdin/stdout JSON contract, and 15-second timeout.
- Hidden RPC delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client through `message/group_e2ee_transport.rs`; TLS remains
  Rustls-first. `group.e2ee.remove` uses `group-e2ee` security, while
  `group.e2ee.leave_request` uses `transport-protected` security like Go.
- Local E2EE summary persistence and post-remove sync reuse the existing group
  SQLite cache and the approved `rusqlite + bundled` lane. No pure-Rust SQLite
  optimization is mixed into this translation slice.
- E2EE self-leave follows Go's owner-mediated request model: it does not call
  P4 `group.leave`, does not run local MLS leave, and only creates a hidden
  leave request with the owner-processing warning.
- Hidden `group e2ee process-leave-request` follows Go by defaulting the
  reason to `leave request processed by owner`, trimming the leave request id,
  delegating to epoch-advancing remove, and inserting the Go plan into the live
  result data.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend.
- Residual Rust API-shape note: Go can return warnings/data alongside some
  failed pending-commit submit paths through multiple return values. The
  current Rust message-service API returns `Result<CommandResult, MessageError>`
  and cannot expose those side-channel warnings on error without a broader
  error-result type change, so this slice keeps the existing error model and
  verifies the success-path parity.
- `update-key`, repair, commit replay beyond finalize/abort and local welcome
  processing, group E2EE send/decrypt, WebSocket local bridge group E2EE
  transport, and full awiki-system-test group-E2EE acceptance remain separate
  translation slices. Any provider-boundary optimization should be recorded
  later and not mixed into the 1:1 translation lane.

## Group E2EE Update-Key Live Notes

2026-05-16:

- Wired live hidden `group e2ee update-key` without adding a crate, changing
  Cargo manifests, enabling local ANP SDK MLS features in-process, or changing
  the approved SQLite/TLS lanes.
- The MLS update boundary remains external-provider parity:
  `anp-mls group update-member-prepare --json-in - --data-dir <scoped-dir>`,
  followed by `anp-mls group update-member-finalize --json-in - --data-dir
  <scoped-dir>` after hidden service acceptance. Deterministic service
  rejection uses the update-specific `anp-mls group update-member-abort`
  command. The shared provider preserves binary resolution order, scoped
  data-dir layout, executable checks, stdin/stdout JSON contract, and
  15-second timeout.
- Hidden RPC delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client through `message/group_e2ee_transport.rs`; TLS remains
  Rustls-first. Update-key uses service head preflight, an update KeyPackage
  lease with `purpose=update`, and hidden `group.e2ee.update` with
  `group-e2ee` security.
- Local E2EE summary persistence and optional local welcome processing reuse
  the existing group SQLite cache, `rusqlite + bundled` lane, and welcome
  helper from the add/rejoin slice. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- Active-member update follows Go's no-P4-membership-mutation model: it does
  not call public `group.add`, does not call `group.e2ee.recover_member`, does
  not put P4 `member_did` or `role` fields in the hidden update body, and
  returns `p4_membership_mutate=false`.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend.
- Repair, commit replay beyond finalize/abort and local welcome processing,
  group E2EE send/decrypt, WebSocket local bridge group E2EE transport, and
  full awiki-system-test group-E2EE acceptance remain separate translation
  slices. Any provider-boundary optimization should be recorded later and not
  mixed into the 1:1 translation lane.

## Group E2EE Repair Live Notes

2026-05-16:

- Wired live hidden `group e2ee repair` without adding a crate, changing Cargo
  manifests, enabling local ANP SDK MLS features in-process, or changing the
  approved SQLite/TLS lanes.
- The repair boundary follows Go's notice replay model: best-effort hidden
  `group.e2ee.head` preflight, accepted local pending-commit finalization,
  hidden `group.e2ee.notice` pull, commit replay through `anp-mls commit
  process --json-in - --data-dir <scoped-dir>`, welcome/recovery/update
  welcome replay through `anp-mls welcome process --json-in - --data-dir
  <scoped-dir>`, and a second hidden `group.e2ee.notice` call to mark only
  processed notice IDs delivered.
- The mark-delivered request preserves Go's dedicated transport helper shape:
  `limit` is the number of processed `notice_ids`, not the original pull
  limit. This keeps the signed RPC body and service semantics aligned with
  `MarkGroupE2EENoticesDelivered`.
- Local E2EE summary persistence, final status scan, diagnosis, and recovery
  artifact reuse existing group E2EE status/create helpers and the approved
  `rusqlite + bundled` cache path. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend. TLS remains Rustls-first.
- Group E2EE send/decrypt, WebSocket local bridge group E2EE transport,
  broader commit/welcome replay edge-case system coverage, and full
  awiki-system-test group-E2EE acceptance remain separate translation slices.
  Any provider-boundary optimization should be recorded later and not mixed
  into the 1:1 translation lane.

## Group E2EE Outbound Send Live Notes

2026-05-16:

- Wired live outbound group E2EE text send without adding a crate, changing
  Cargo manifests, enabling local ANP SDK MLS features in-process, or changing
  the approved SQLite/TLS lanes.
- The routing follows Go `sendGroup`: explicit `msg send --group <did>
  --secure on --text <text>` requires a cached group snapshot using
  `group-e2ee`; ordinary group text send auto-upgrades when the cached snapshot
  uses group E2EE or local MLS status is active/pending/has a crypto group id.
- The MLS encryption boundary remains external-provider parity:
  `anp-mls message encrypt --json-in - --data-dir <scoped-dir>`, after the
  existing device-scoped status selection. The request keeps Go fields for
  `agent_did`, `device_id`, `group_did`, `group_state_ref`, `sender_did`,
  `content_type=application/anp-group-cipher+json`,
  `security_profile=group-e2ee`, generated message/operation IDs,
  `message_type`, and plaintext text.
- Hidden RPC delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client through `message/group_e2ee_transport.rs`; TLS remains
  Rustls-first. The signed `group.e2ee.send` body posts only the sanitized
  opaque cipher fields, with service-omitted `group_did`, `message_id`, and
  `operation_id` backfilled like Go.
- Local message persistence reuses the approved `rusqlite + bundled` cache
  path and marks the row as E2EE. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- The stale-epoch repair/retry path is translated through the existing repair
  helper, but broader service edge-case and awiki-system-test coverage remains
  a later verification slice.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend.
- Group E2EE decrypt/receive/history display, WebSocket local bridge group
  E2EE transport, and full awiki-system-test group-E2EE acceptance remain
  separate translation slices. Any provider-boundary optimization should be
  recorded later and not mixed into the 1:1 translation lane.

## Group E2EE Decrypt Display Live Notes

2026-05-16:

- Wired HTTP `group messages` group E2EE decrypt/display without adding a
  crate, changing Cargo manifests, enabling local ANP SDK MLS features
  in-process, or changing the approved SQLite/TLS lanes.
- The MLS decrypt boundary remains external-provider parity:
  `anp-mls message decrypt --json-in - --data-dir <scoped-dir>`, using the
  same scoped data-dir and candidate-device scan as the translated status/send
  slices. The request keeps Go fields for `agent_did`, `recipient_did`,
  `device_id`, `group_did`, `group_cipher_object`, `private_message_b64u`,
  `group_state_ref`, `sender_did`,
  `content_type=application/anp-group-cipher+json`,
  `security_profile=group-e2ee`, `message_id`, and `operation_id`.
- `GroupMessages` now follows Go's order for the HTTP path: fetch
  `group.list_messages`, decrypt group cipher objects in-memory, compact
  decrypt warnings, persist the decrypted result to the existing SQLite cache,
  then read the cache projection for CLI output.
- Cipher extraction follows Go's accepted shapes: top-level
  `group_cipher_object`, direct or nested `content.group_cipher_object`, and
  `body.group_cipher_object`. Successful `application_plaintext` rewrites the
  message `content`, `content_type`, and `decrypted=true` before persistence.
- Local message persistence reuses the approved `rusqlite + bundled` cache
  path. No pure-Rust SQLite optimization is mixed into this translation slice.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend. TLS remains Rustls-first.
- This slice covers only HTTP `group messages` decrypt/display. WebSocket local
  bridge group message receive/decrypt, foreground listener group E2EE
  handling, broader service edge-case coverage, and full awiki-system-test
  group-E2EE acceptance remain separate translation slices. Any
  provider-boundary optimization should be recorded later and not mixed into
  the 1:1 translation lane.

## Group E2EE Recover-Member Live Notes

2026-05-16:

- Wired live hidden `group e2ee recover-member` without adding a crate,
  changing Cargo manifests, enabling local ANP SDK MLS features in-process, or
  changing the approved SQLite/TLS lanes.
- The MLS recovery boundary remains external-provider parity:
  `anp-mls group recover-member-prepare --json-in - --data-dir <scoped-dir>`,
  followed by `anp-mls group commit-finalize --json-in - --data-dir
  <scoped-dir>` after hidden service acceptance. The existing
  `message/group_e2ee_provider.rs` now also exposes `recover_member_prepare`
  while preserving binary resolution order, scoped data-dir layout, executable
  checks, stdin/stdout JSON contract, and 15-second timeout.
- Hidden RPC delivery reuses the existing authsdk/session and Rustls/std
  message HTTP client through `message/group_e2ee_transport.rs`; TLS remains
  Rustls-first. Recovery uses service head preflight, a recovery KeyPackage
  lease with `purpose=recovery`, and hidden `group.e2ee.recover_member` with
  `group-e2ee` security.
- Local E2EE summary persistence and optional local welcome processing reuse
  the existing group SQLite cache, `rusqlite + bundled` lane, and welcome
  helper from the add/rejoin slice. No pure-Rust SQLite optimization is mixed
  into this translation slice.
- Active-member recovery follows Go's no-P4-membership-mutation model: it does
  not call public `group.add`, does not put P4 `member_did` or `role` fields
  in the hidden recover body, and returns `p4_membership_mutate=false`.
- No TLS, WebSocket, platform, or ANP SDK dependency decision changed. This
  slice does not add OpenSSL, `native-tls`, bundled OpenSSL, `reqwest`,
  `hyper`, WebSocket crates, async runtimes, YAML crates, platform service
  libraries, MLS provider crates, ANP SDK default/network features, or a new
  SQLite backend.
- `update-key`, repair, commit replay beyond finalize/abort and local welcome
  processing, group E2EE send/decrypt, WebSocket local bridge group E2EE
  transport, and full awiki-system-test group-E2EE acceptance remain separate
  translation slices. Any provider-boundary optimization should be recorded
  later and not mixed into the 1:1 translation lane.
