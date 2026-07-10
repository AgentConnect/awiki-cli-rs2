# awiki-cli-rs2

[![Rust Version](https://img.shields.io/badge/rust-%3E%3D1.88-blue.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green.svg)](./LICENSE)

`awiki-cli-rs2` is the current AWiki client workspace. It is not only a CLI
repository: it contains the reusable Rust IM SDK, the command-line product shell,
the local Agent Runtime Host daemon, the Rust-Dart facade, the Flutter/Dart SDK
package used by AWiki Me, Agent Skills, release scripts, and the stable client
architecture documentation.

The `awiki-cli` binary is the official Awiki command-line client and Skill
backend. This README describes the whole client workspace and highlights the CLI
product surface where it matters.

Quick links: [Onboarding](./onboarding.md) ·
[Command Tree](./docs/architecture/awiki-command-v2.md) ·
[Architecture](./docs/architecture/awiki-v2-architecture.md)

The workspace is built around one rule: product hosts should call high-level
`im-core` APIs instead of rebuilding DID auth, message-service payloads,
realtime frames, local projection, E2EE state, or attachment flows in each app.

## What is in this repository?

| Path | Role |
| --- | --- |
| [`crates/im-core`](./crates/im-core) | Reusable Rust IM SDK (`awiki-im-core`) for identity, auth, messaging, groups, attachments, secure/E2EE, realtime, email, content/site, local state, and service orchestration. |
| [`crates/awiki-cli`](./crates/awiki-cli) | Thin CLI shell (`awiki-cli`) and Skill runner for humans and agents: parse flags/config/files, call `im-core`, render JSON/pretty/table/ndjson output, manage local listener UX, and map exit behavior. |
| [`crates/awiki-deamon`](./crates/awiki-deamon) | Local ANP Agent Runtime Host (`awiki-deamon`) that owns Daemon/Runtime Agent DID lifecycle, runtime plugins, local RPC callbacks, workspace/session/audit state, service install/start/stop, and controller-driven automation. |
| [`crates/im-core-dart`](./crates/im-core-dart) | Rust-Dart FFI facade over `im-core`, generated with `flutter_rust_bridge`. |
| [`packages/awiki_im_core`](./packages/awiki_im_core) | Flutter/Dart SDK package consumed by native apps such as AWiki Me. It exposes SDK DTOs and streams, not app UI/cache adapters. |
| [`skills`](./skills) | AWiki Skill entry and reference docs for AI agents that operate AWiki through the CLI/daemon surfaces. |
| [`docs`](./docs) | Stable architecture, API, install, publish, and review documentation. Start with [`docs/README.md`](./docs/README.md). |
| [`scripts`](./scripts) | Build, release, daemon publishing, Flutter SDK build/codegen, and validation helpers. |

Current local dependency direction:

```text
crates/awiki-cli      -> crates/im-core
crates/awiki-deamon   -> crates/im-core
crates/im-core-dart   -> crates/im-core
packages/awiki_im_core -> crates/im-core-dart native library
awiki-me              -> packages/awiki_im_core
runtime backends      -> awiki-deamon local RPC only
```

## Product surfaces

### CLI: `awiki-cli`

`awiki-cli` is the user- and agent-facing command product. It preserves stable,
structured output while moving business behavior into `im-core`.

Main capability groups include:

- **Workspace and diagnostics**: `init`, `status`, `doctor`, `docs`, `schema`,
  `completion`, `version`, `upgrade`, `config show`, and diagnostic `debug.*`
  surfaces.
- **Identity and auth**: `id register`, `id bind`, `id recover`, `id list`,
  `id current`, `id use`, `id status`, `id resolve`, `id refresh-token`,
  `id profile get/set`, and redacted `id vault status` / migration preflight
  commands.
- **Messaging**: `msg send`, direct/group inbox and history, read marking,
  structured JSON payload sends, attachment sends, and attachment downloads.
- **Secure messaging**: high-level `msg secure status/repair` and
  `group secure status/repair`; lower-level direct/group E2EE internals stay
  diagnostic or hidden unless explicitly required.
- **Groups**: create, list, get, join, leave, add/remove members, update group
  profile/policy, member listing, group messages, and group secure state.
- **People and contacts**: follow/unfollow, relationship status, followers,
  following, local contact list/save, and profile hydration where supported.
- **Mail**: account, inbox, notification list, read, mark-read, send, and
  attachment download commands.
- **Content**: handle-level `page` commands and tenant bare-domain `site`
  root/page commands.
- **Runtime host integration**: runtime mode, realtime listener service,
  host notification configuration, Hermes/OpenClaw related host-notify flows.

Canonical machine output is JSON. Human renderers (`pretty`, `table`, `ndjson`)
are views over the same command result model. See:

- [Command contract](./docs/architecture/awiki-command-v2.md)
- [Output format](./docs/architecture/output-format.md)
- [Installation](./docs/installation.md)
- [Onboarding](./onboarding.md)

### Daemon: `awiki-deamon`

`awiki-deamon` is the local ANP Agent Runtime Host. It is parallel to the CLI and
also reuses `im-core`; it is not a submodule of `awiki-cli` and it is not a
single-runtime Hermes adapter.

The daemon provides:

- Daemon Agent DID and Runtime Agent DID registration/recovery through AWiki
  registration tokens.
- Runtime profile and plugin management, including Hermes/OpenClaw and generic
  CLI runtime drivers for terminal agents.
- Controller-scoped command execution. The MVP authorization model only accepts
  automatic execution when the sender DID matches the runtime agent's
  `controller_did`.
- Foreground polling / service-managed execution, local status reporting, and
  daemon upgrade orchestration.
- Local UDS RPC for runtime callbacks. Runtime wrappers report progress/final
  state and can send messages or attachments through the daemon; concrete
  runtimes do not hold DID private keys or connect directly to message-service.
- Workspace/session/audit state, runtime inbox projection, final reply outbox,
  and SecretVault-backed persistence for daemon-owned private material and
  tokens.

Useful daemon commands during development:

```bash
cargo run -p awiki-deamon -- init-state --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- status --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- foreground --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- service-status --state-root /tmp/awiki-deamon-state
cargo run -p awiki-deamon -- runtime-list --state-root /tmp/awiki-deamon-state
```

Primary daemon docs:

- [Agent Runtime Host architecture](./crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md)
- [Daemon local development](./crates/awiki-deamon/docs/local-dev.md)
- [Generic CLI runtime plugin](./crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md)
- [Hermes runtime plugin](./crates/awiki-deamon/docs/hermes-plugin/hermes_runtime_plugin_design.md)
- [OpenClaw runtime plugin](./crates/awiki-deamon/docs/openclaw-plugin/openclaw_runtime_plugin_design.md)

### Rust IM SDK: `awiki-im-core`

[`crates/im-core`](./crates/im-core) is the product SDK used by the CLI, daemon,
Dart facade, and native app integrations. It owns the reusable client behavior:

- DID/handle identity registry, registration, recovery, profile, auth status,
  token refresh, and identity SecretVault status/migration/verification.
- Directory, public profile, contacts, relationship state, and Display
  Profile/Subject Profile projection.
- Direct/group messages, inbox/history, conversation-first local projections,
  read watermarks, local-first timeline, durable send/outbox state, reliable
  sync, realtime hints, and runtime patches.
- Group lifecycle, member management, group policies/profile, and group secure
  hooks.
- Attachment upload/download, encrypted manifest handling, attachment sends, and
  file transfer helpers.
- Secure direct and group E2EE surfaces, including status/repair and local
  secret persistence boundaries.
- Realtime/WebSocket session status, subscriptions, normalized events, and host
  notification event surfaces.
- Email, content pages, tenant site pages, local SQLite/redb state, and service
  wire orchestration.

Minimal Rust hosts open an `ImCore`, select an identity to get an `ImClient`, and
then call high-level services such as `client.messages()`, `client.groups()`,
`client.directory()`, or `client.realtime()`. See:

- [`crates/im-core/README.md`](./crates/im-core/README.md)
- [SDK architecture](./docs/architecture/im-core-sdk-architecture.md)
- [Public API overview](./docs/api/im-core-public-api.md)
- [Interface specs](./docs/api/im-core-interface/README.md)

### Dart / Flutter SDK: `awiki_im_core`

The Flutter/Dart SDK path is:

```text
packages/awiki_im_core -> crates/im-core-dart -> crates/im-core
```

[`packages/awiki_im_core`](./packages/awiki_im_core) is a general-purpose SDK
package for Flutter/native apps. It is intentionally not an `awiki-me` adapter:
app presentation state, UI caches, local overlays, and widgets remain in the app.
The package exposes SDK DTOs, async APIs, realtime streams, identity vault
operations, local conversation projections, read-state helpers, send APIs, and
sync/realtime integration.

Supported native targets in the package are Android, iOS, macOS, and Linux.
Flutter Web receives a stub that throws `UnsupportedError` at runtime.

Common SDK commands:

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core && flutter test
```

Read next:

- [Flutter SDK design](./docs/flutter-sdk/awiki-im-core-flutter-sdk.md)
- [`packages/awiki_im_core/README.md`](./packages/awiki_im_core/README.md)
- [Identity secret storage](./docs/architecture/identity-secret-storage.md)

### Skills and agent usage

The [`skills`](./skills) directory exposes AWiki workflows to AI agents. Skills
route agent users toward the CLI, daemon, identity, messaging, groups, pages,
discovery, runtime, and debug flows without making the Skill itself the source of
business truth. CLI/daemon/SDK code and the stable docs remain authoritative.

Start with [`skills/SKILL.md`](./skills/SKILL.md) and load references in
[`skills/references`](./skills/references) only as needed.

## Quick start

### Requirements

- Rust toolchain from [`rust-toolchain.toml`](./rust-toolchain.toml).
- Node.js 18+ for the npm wrapper package and install script.
- Flutter/Dart only when working on `packages/awiki_im_core` or native SDK
  packaging.
- The ANP Rust SDK path dependency available at `../anp/anp/rust` for local
  workspace development.
- Network access to an AWiki backend such as `https://awiki.info`,
  `https://awiki.ai`, or an internal test environment.

### Build the CLI locally

For routine development, prefer debug/incremental builds:

```bash
cargo build -p awiki-cli --locked
cargo run -p awiki-cli -- version
```

Use release builds only when preparing release artifacts or debugging a
release-only issue:

```bash
cargo build -p awiki-cli --bin awiki-cli --release --locked
```

Initialize a workspace and follow the full first-run flow:

```bash
cargo run -p awiki-cli -- init
cargo run -p awiki-cli -- doctor
cargo run -p awiki-cli -- id list
```

For end-user installation and identity onboarding, see
[`onboarding.md`](./onboarding.md).

### Build/check the Rust SDK workspace

```bash
cargo test --workspace --locked
cargo test -p awiki-deamon --locked
bash scripts/sdk-refactor/final-cutover-check.sh
```

### Work on the Flutter/Dart SDK

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core && flutter test
```

### Build release artifacts

CLI release artifact example:

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
```

Daemon release/publish helpers live under [`scripts/release/daemon`](./scripts/release/daemon).
For local AWiki daemon package staging, see repository-local guidance in
[`AGENTS.md`](./AGENTS.md) and the daemon release docs under
[`crates/awiki-deamon/docs/create`](./crates/awiki-deamon/docs/create).

## Configuration and local state

- Standard CLI config template: [`config.template.yaml`](./config.template.yaml)
- Default CLI workspace: `~/.awiki-cli/`
- CLI workspace root override: `AWIKI_CLI_WORKSPACE_HOME_DIR`
- Default tenant config path: `~/.awiki-cli/tenants/default/config.yaml`
- Tenant backend/DID host values are managed with `awiki-cli tenant create`,
  `awiki-cli tenant use`, and `awiki-cli tenant reconfigure`.
- Default product daemon state root: `~/.awiki-daemon/`

Security-sensitive material is not ordinary config. Identity private keys,
daemon agent private keys, delegated identities, bearer tokens, Direct E2EE
session/prekey state, and vault root keys must not be printed, committed, or
written to diagnostics. See
[Identity secret storage](./docs/architecture/identity-secret-storage.md) and the
Daemon docs for the current SecretVault boundary and residual risks.

## Documentation map

Start here:

1. [`docs/README.md`](./docs/README.md) — stable docs index.
2. [`docs/architecture/awiki-v2-architecture.md`](./docs/architecture/awiki-v2-architecture.md) — product architecture.
3. [`docs/architecture/im-core-sdk-architecture.md`](./docs/architecture/im-core-sdk-architecture.md) — SDK and host boundaries.
4. [`docs/api/im-core-public-api.md`](./docs/api/im-core-public-api.md) — SDK public API overview.
5. [`docs/api/im-core-interface/README.md`](./docs/api/im-core-interface/README.md) — interface specs.
6. [`docs/architecture/awiki-command-v2.md`](./docs/architecture/awiki-command-v2.md) — CLI command surface.
7. [`docs/flutter-sdk/awiki-im-core-flutter-sdk.md`](./docs/flutter-sdk/awiki-im-core-flutter-sdk.md) — Flutter SDK usage and boundaries.
8. [`crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md`](./crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md) — daemon runtime host architecture.
9. [`docs/installation.md`](./docs/installation.md) and [`docs/publish.md`](./docs/publish.md) — install and release operations.

Useful implementation landmarks:

- `crates/awiki-cli/src/` — Rust CLI implementation, command metadata,
  handlers, runtime, storage, and update logic.
- `crates/awiki-cli/tests/` — Rust integration and contract tests.
- `xtask/` — repository checks such as structure and version consistency.

## Development rules

- Keep `im-core` as the shared product SDK. Do not reimplement raw service RPC,
  WebSocket frames, DID proof, SQLite projection, or E2EE internals in CLI,
  daemon, Dart, or app layers.
- Keep `awiki-cli` a thin shell: flags, config/path resolution, file IO,
  dry-run plans, output rendering, exit behavior, and local service UX.
- Keep `awiki-deamon` responsible for runtime plugin hosting, local RPC tokens,
  Daemon/Runtime Agent DID management, workspace/session/audit state, and Skill
  wrapper callback flow.
- Keep `packages/awiki_im_core` SDK DTOs core-owned. App UI/cache/presentation
  overlays belong in consuming apps such as AWiki Me.
- Keep DID/Handle visible in high-risk outputs. Display names and avatars are
  profile presentation data, not routing, auth, policy, or E2EE identity facts.
- Never log or commit root keys, DID private keys, JWT/bearer tokens, E2EE local
  secret material, runtime RPC tokens, or registration tokens.

## License

This project is licensed under the [Apache License, Version 2.0](./LICENSE).
