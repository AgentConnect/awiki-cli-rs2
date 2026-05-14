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

## Initial Decisions

| Area | Decision | Rationale | Verification |
| --- | --- | --- | --- |
| ANP SDK | Use workspace-local `../anp/rust` path dependency when ANP-backed CLI modules are implemented. | User requested the local Rust ANP SDK; CLI parity may require SDK changes. The first core CLI slice intentionally does not link ANP yet. | `cargo metadata`, `cargo test --workspace`, and focused `anp/rust` tests when SDK changes occur. |
| SQLite | Use `rusqlite = 0.32.1` with the `bundled` feature for the current store/debug lane. | The user explicitly approved trying `rusqlite + bundled` on 2026-05-14 because compiling SQLite into the binary also solves runtime compatibility. This is a documented exception to the pure Rust preference: it avoids host SQLite at runtime, but it compiles bundled C SQLite through `libsqlite3-sys`. | Temporary probe `/tmp/awiki_rusqlite_probe`: `CARGO_HOME=/tmp/awiki_sqlite_cargo_home cargo +1.79.0 run` and `cargo +1.79.0 run --locked` passed PRAGMA `user_version`, `sqlite_master`, tables, partial unique indexes, views, `ON CONFLICT`, and `ROW_NUMBER() OVER (...)`. Dependency tree shows `rusqlite -> libsqlite3-sys` plus `cc/pkg-config/vcpkg`, and no OpenSSL/native-tls. |
| SSL/TLS | Prefer Rustls-based TLS stacks, for example `reqwest` with `rustls-tls`, and avoid OpenSSL/native-tls. | User explicitly called out SSL/system dependency constraints; Rustls avoids OpenSSL package and ABI drift across Linux/macOS/Windows. | Dependency-tree review, cargo feature audit, and system tests for service-backed HTTP/WebSocket flows. |
| Crypto | Prefer pure Rust crypto crates unless Go parity or ANP protocol compatibility requires otherwise. | Identity, DID proofs, direct E2EE, and group E2EE must be portable and reproducible without relying on platform crypto libraries. | Golden proof/signature tests against Go behavior and relevant ANP SDK tests. |
| CLI core slice dependencies | Keep non-storage dependencies minimal: `anyhow`, `serde`, `serde_json`, `sha2`; add `rusqlite + bundled` only for the store/debug lane. | Current local cargo mirror/toolchain has network and Cargo 1.79 compatibility constraints. Storage is the first documented bundled-native exception; other lanes still need explicit dependency review. | `cargo +1.79.0 test -p awiki-cli --locked`, `cargo +1.79.0 run --bin xtask --locked -- check-structure`, dependency tree audit, and focused core/debug `awiki-system-test` run. |

## Known Deferred Decisions

| Area | Deferred Decision | Required Evidence Before Adoption |
| --- | --- | --- |
| Full YAML config parsing | Choose a parser that preserves Go YAML behavior without introducing unnecessary native dependencies. | Go config fixture parity tests, environment override tests, and dependency-tree review. |
| SQLite crate/backend | Current accepted lane is `rusqlite + bundled` for exact SQLite behavior and runtime portability. Keep pure Rust alternatives recorded for later optimization review, not mixed into this parity translation. | Exact schema/migration parity, query behavior parity, no host SQLite dependency, and system-test debug/store evidence. |
| HTTP/WebSocket client stack | Select Rustls-based HTTP/WebSocket crates for service integrations. | Feature audit showing no OpenSSL/native-tls path and service-backed system tests. |

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
