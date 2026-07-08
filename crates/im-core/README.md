# im-core

> English is the default language for this README. For Chinese, see [README.zh-CN.md](./README.zh-CN.md).

`im-core` is the reusable Rust IM SDK for the Awiki client stack. It owns the
shared product flows behind the Awiki CLI, the Flutter/Dart SDK bridge, and
native app integrations: identity, DID-WBA authentication, messaging, groups,
attachments, realtime notifications, E2EE-ready secure messaging, local state,
email, and content/site APIs.

This crate is intentionally a **product SDK**, not a CLI helper library. Hosts
construct an [`ImCore`](#minimal-rust-example), bind an identity into an
`ImClient`, and call high-level services such as `messages()`, `groups()`,
`directory()`, or `realtime()`.

## Status

- Current crate version: `0.1.0`.
- Rust toolchain: Rust `1.88.0` or newer, matching this workspace's
  `rust-toolchain.toml`.
- Public API is still evolving while the Awiki client stack is being split into
  a stable Rust SDK, an FFI facade, and Flutter/Dart bindings.
- The crate is designed for native Rust hosts. Flutter/Dart apps should usually
  consume [`packages/awiki_im_core`](../../packages/awiki_im_core) instead of
  linking this crate directly.

## Relationship with ANP

Awiki IM builds on Agent Network Protocol (ANP) primitives for agent identity,
DID-WBA authentication, proof generation, service interoperability, and secure
communication flows.

Useful ANP links:

- ANP official site: <https://agent-network-protocol.com/>
- ANP protocol/specification repository:
  <https://github.com/agent-network-protocol/AgentNetworkProtocol>
- DID-WBA method specification:
  <https://github.com/agent-network-protocol/AgentNetworkProtocol/blob/main/03-did-wba-method-design-specification.md>
- AgentConnect multi-language ANP SDK repository:
  <https://github.com/agent-network-protocol/AgentConnect>
- Rust `anp` crate on crates.io: <https://crates.io/crates/anp>
- Rust `anp` docs: <https://docs.rs/anp>

`im-core` depends on the Rust `anp` SDK for low-level ANP protocol machinery, but
it keeps those details behind Awiki-oriented product APIs. Application code
should normally call `im-core` services rather than constructing raw ANP wire
payloads itself.

## What this crate provides

| Area | Public entry point | Purpose |
| --- | --- | --- |
| Core lifecycle | `ImCore`, `CoreBootstrap` | Open an environment, validate paths, initialize and migrate local state. |
| Identity | `IdentityRegistry`, `IdentityService` | List/select local identities, register or recover handles, manage profiles and identity vault status. |
| Authentication | `AuthService` | DID-WBA session status, refresh, and auth-scoped operations. |
| Directory | `DirectoryService` | Resolve handles/DIDs, load public profiles, contacts, and relationship state. |
| Messages | `MessageService` | Send direct/group messages, read inbox/history, local-first conversations, mark-read, sync, and runtime patches. |
| Groups | `GroupService` | Group lifecycle, members, policy/profile updates, group reads, and group E2EE hooks. |
| Attachments | `AttachmentService` | Upload, encrypted manifest handling, send, and download helpers. |
| Secure messaging | `SecureService` | Direct secure state, group secure state, prepare/repair, and secure outbox surfaces. |
| Realtime | `RealtimeService` | WebSocket session status, subscriptions, normalized events, and host notification events. |
| Email | `EmailService` | Mail account, inbox/read/send/mark-read, attachments, and notifications. |
| Content/site | `ContentService`, `SiteService` | Awiki page/content and site operations. |

## Crate boundaries

The dependency direction is fixed:

```text
awiki-cli      -> im-core
im-core-dart   -> im-core
awiki_im_core  -> im-core-dart native library
awiki-me       -> awiki_im_core
```

`im-core` owns product behavior and local state. It does **not** own:

- CLI argument parsing, terminal output, exit codes, or workspace discovery.
- Flutter widget state, app presentation DTOs, or UI cache models.
- Raw stdout/stderr envelopes for agents.
- Service installation, daemon process supervision, or OS-specific app UX.
- Generic ANP protocol specification work; that belongs in ANP/AgentConnect.

## Installation

After publication to crates.io, add the crate to your Rust project:

```toml
[dependencies]
im-core = "0.1"
```

For local workspace development, use a path dependency:

```toml
[dependencies]
im-core = { path = "../awiki-cli-rs2/crates/im-core" }
```

To enable additional capability groups:

```toml
[dependencies]
im-core = { version = "0.1", features = ["group-e2ee", "realtime", "attachments"] }
```

## Feature flags

| Feature | Default | Notes |
| --- | --- | --- |
| `sqlite` | yes | Enables SQLite-backed local state through `rusqlite`. |
| `http` | yes | Enables HTTP/RPC transport-oriented SDK flows. |
| `blocking` | no | Enables selected synchronous helpers where the host expects blocking calls. |
| `attachments` | no | Attachment upload/download and message attachment helpers. |
| `realtime` | no | Realtime/WebSocket host integration surfaces. |
| `secure-direct` | no | Direct E2EE secure-message surfaces. |
| `group-e2ee` | no | Group E2EE/MLS surfaces; also enables `sqlite` and the ANP `mls` feature. |
| `email` | no | Email/mail product namespace. |
| `provider-traits` | no | Reserved provider extension points. |
| `mcp-trusted-registration` | no | Internal trusted-registration integration surface. |
| `internal-test-helpers` | no | Test-only helpers; do not enable in downstream production builds. |

The default feature set is `sqlite` + `http`.

## Minimal Rust example

```rust
use std::path::PathBuf;

use im_core::prelude::*;

#[tokio::main]
async fn main() -> ImResult<()> {
    let config = ImCoreConfig::new(
        ServiceEndpoint::parse("https://awiki.info")?,
        "awiki.info",
    )?;

    let state_root = PathBuf::from("/tmp/awiki-im-core-example");
    let paths = ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: state_root.join("identities"),
            registry_path: state_root.join("identities/registry.json"),
            default_identity_path: Some(state_root.join("identities/default")),
        },
        local_state: LocalStatePaths {
            sqlite_path: state_root.join("state/im_core.sqlite"),
        },
        runtime: RuntimePaths {
            cache_dir: state_root.join("cache"),
            temp_dir: state_root.join("tmp"),
        },
    };

    let core = ImCore::open(config, paths).await?;
    core.bootstrap().validate_paths_async().await?;
    core.bootstrap().initialize_local_state_async().await?;

    let client = core.client_async(IdentitySelector::Default).await?;
    let result = client
        .messages()
        .send_async(SendMessageRequest {
            target: MessageTarget::Direct(PeerRef::parse("bob", client.did_domain())?),
            body: MessageBody::Text {
                text: "hello from im-core".to_owned(),
                kind: MessageKind::Text,
            },
            security: MessageSecurityMode::DefaultPlain,
            client_message_id: None,
            delivery: MessageDeliveryOptions::default(),
            delegated_signing: None,
        })
        .await?;

    println!("sent message: {}", result.message.id.as_str());
    Ok(())
}
```

The example assumes that `IdentitySelector::Default` already resolves to a local
identity under the configured identity paths. New hosts can create local
identities through `core.identities().register_handle_async(...)` or recover an
existing handle through the recovery APIs.

## Configuration model

Hosts pass all environment configuration explicitly through `ImCoreConfig`:

- `service_base_url`: base Awiki service URL, for example `https://awiki.info`.
- `did_domain`: default DID/handle domain, for example `awiki.info`.
- Optional service overrides: `user_service_endpoint`, `message_service_endpoint`,
  `mail_service_endpoint`, and `anp_service_endpoint`.
- Optional `anp_service_did` for flows that need an explicit ANP service DID.
- Optional `ca_bundle` path for custom TLS trust roots.
- `transport_policy`: `Auto`, `HttpOnly`, or `RealtimePreferred`.

The SDK does not discover CLI workspaces or read app config files by itself. CLI,
Flutter, daemon, and test hosts are responsible for resolving configuration and
passing normalized values into `im-core`.

## Storage and paths

Hosts also pass all storage paths explicitly through `ImCorePaths`:

- `IdentityRegistryPaths.identity_root_dir`: local identity directories.
- `IdentityRegistryPaths.registry_path`: identity registry JSON file.
- `IdentityRegistryPaths.default_identity_path`: optional default identity marker.
- `LocalStatePaths.sqlite_path`: SQLite local projection and sync state.
- `RuntimePaths.cache_dir`: cache and snapshot data.
- `RuntimePaths.temp_dir`: temporary files and transfer staging.

`CoreBootstrap` can validate paths and initialize/migrate the local SQLite state.
Hosts remain responsible for directory creation policy, file permissions, backup,
cleanup, and choosing when migration runs.

## Identity secret storage

By default, `ImCore::new` / `ImCore::open` use `IdentitySecretStoragePolicy::FileCompat`
for compatibility with existing identity directories. Production app hosts should
prefer explicit SecretVault options:

```rust
let open_options = ImCoreOpenOptions::file_compat().with_identity_secret_vault(
    IdentitySecretStoragePolicy::VaultRequired,
    ImCoreSecretVaultOptions::new(
        root_key,
        "/private/app/vault",
        "awiki-me-production",
        "device-001",
    ),
);

let core = ImCore::open_with_options(config, paths, open_options).await?;
```

Security rules:

- Never log root keys, private keys, JWTs, bearer tokens, raw `SecretRef` values,
  ciphertext internals, or MLS private state.
- `VaultRequired` is fail-closed: if the host cannot provide a valid vault
  context, opening the SDK fails instead of silently falling back to plaintext.
- Status, migration, and verification APIs expose only redacted metadata and
  warnings.

## Realtime and local-first reads

`im-core` treats committed local projection as the durable source for fast UI
reads. Conversation snapshots and patch streams are acceleration layers over
SQLite state, not separate sources of truth.

Typical app flow:

1. Open `ImCore` with explicit config and paths.
2. Bind an identity with `core.client_async(IdentitySelector::Default)`.
3. Use `client.messages().conversations_async(...)` or local timeline APIs for
   first paint.
4. Start `client.realtime()` when realtime transport is available.
5. Apply SDK conversation/message patches only after committed projection events.
6. Use remote history/sync APIs as background freshness/reconciliation paths.

## Error handling

Most public APIs return:

```rust
pub type ImResult<T> = Result<T, ImError>;
```

`ImError` represents SDK/domain failures such as invalid input, missing identity,
auth expiration, permission failure, missing peer/group/message, local state
failure, unsupported capability, transport failure, service error, or internal
error. CLI and app hosts should map `ImError` into their own exit codes, UI
messages, telemetry, and retry policies without exposing secrets.

## Development

From the workspace root:

```bash
cargo test -p im-core --locked
cargo test --workspace --locked
cargo package -p im-core --allow-dirty
cargo publish -p im-core --dry-run
```

For publishing, every non-dev dependency must include a crates.io `version`.
Path dependencies may be kept for local development only when paired with a
published version, for example:

```toml
anp = { version = "0.8.8", path = "../anp/anp/rust", default-features = false }
```

## Related documentation

- Workspace README: [../../README.md](../../README.md)
- SDK architecture: [../../docs/architecture/im-core-sdk-architecture.md](../../docs/architecture/im-core-sdk-architecture.md)
- Public API overview: [../../docs/api/im-core-public-api.md](../../docs/api/im-core-public-api.md)
- Interface-level API docs: [../../docs/api/im-core-interface/README.md](../../docs/api/im-core-interface/README.md)
- Flutter/Dart SDK docs: [../../docs/flutter-sdk/awiki-im-core-flutter-sdk.md](../../docs/flutter-sdk/awiki-im-core-flutter-sdk.md)
- Rust-Dart facade crate: [../im-core-dart](../im-core-dart)
- Flutter package: [../../packages/awiki_im_core](../../packages/awiki_im_core)

## License

This crate is distributed under the MIT license, consistent with the workspace
license configuration.
