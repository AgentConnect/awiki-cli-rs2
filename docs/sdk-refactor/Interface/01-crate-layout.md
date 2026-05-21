# 01. Phase 1 Crate Layout

## 1. Workspace 改动

P1 增加 `crates/im-core`：

```toml
[workspace]
members = [
    "crates/im-core",
    "crates/awiki-cli",
    "xtask",
]
resolver = "2"
```

`crates/awiki-cli/Cargo.toml` 增加：

```toml
[dependencies]
im-core = { path = "../im-core" }
```

## 2. P1 `im-core` Cargo.toml

第一阶段不要新增不必要依赖。当前 workspace 已有 `serde`、`serde_json`、`rusqlite`、`time` 等依赖；P1 建议先复用这些，不强制引入 `url`、`tokio`、`async-trait`。

```toml
[package]
name = "im-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = ["blocking", "sqlite", "http"]
blocking = []
sqlite = ["dep:rusqlite"]
http = []
internal-test-helpers = []

# Reserved for later phases.
attachments = []
realtime = []
secure-direct = []
group-e2ee = []
provider-traits = []

[dependencies]
serde.workspace = true
serde_json.workspace = true
time.workspace = true
rusqlite = { workspace = true, optional = true }
```

说明：

- P1 接口中 endpoint 暂用 `ServiceEndpoint(String)`，不使用 `url::Url`，避免第一阶段新增依赖。
- P1 API blocking-first，不引入 async runtime。
- `internal-test-helpers` 只给测试使用，不进 default feature。

## 3. P1 文件结构

```text
crates/im-core/src/
  lib.rs
  prelude.rs

  error.rs
  ids.rs
  config.rs
  paths.rs

  core/
    mod.rs
    client.rs
    bootstrap.rs

  identity/
    mod.rs
    dto.rs
    registry.rs

  auth/
    mod.rs
    dto.rs
    service.rs

  messages/
    mod.rs
    dto.rs
    service.rs

  internal/
    mod.rs
    identity_runtime.rs
    transport.rs
    wire.rs
    store.rs

    # P1-beta 之后才允许出现；必须是已复制/迁移到 im-core 内部的代码。
    legacy_identity.rs
    legacy_auth.rs
    legacy_messages.rs
```

P1-alpha 不要求在 `im-core/internal/legacy_*` 中实现业务。P1-alpha 的旧实现调用应发生在 `awiki-cli::im_core_adapter` 中，而不是发生在 `im-core` 中。

P1-beta 如果需要 `internal/legacy_*`，这些模块只能调用已经复制或迁移到 `crates/im-core` 内部的代码，不能依赖 `crates/awiki-cli`。

## 4. `lib.rs` 导出规则

```rust
pub mod auth;
pub mod config;
pub mod core;
pub mod error;
pub mod identity;
pub mod ids;
pub mod messages;
pub mod paths;
pub mod prelude;

mod internal;
```

不要导出：

```rust
pub mod internal;
pub mod wire;
pub mod store;
pub mod transport;
pub mod legacy_messages;
```

P1 默认 public API 不导出：

```rust
pub fn groups(&self) -> GroupService<'_>
pub fn attachments(&self) -> AttachmentService<'_>
pub fn realtime(&self) -> RealtimeService<'_>
pub fn secure(&self) -> SecureDiagnosticsService<'_>
```

这些 service 可以在后续阶段按 feature 或阶段加入；如果为了前向兼容提前提供 placeholder，必须放在 non-default feature 或 experimental API 中，并返回 `UnsupportedCapability`。

## 5. `prelude.rs`

```rust
pub use crate::auth::{AuthScope, AuthService, AuthStatus, SessionBundle, SessionUpdate};
pub use crate::config::{ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};
pub use crate::core::{CoreBootstrap, ImClient, ImCore};
pub use crate::error::{ImError, ImResult};
pub use crate::identity::{
    DefaultIdentityChange, HandleRegistrationResult, IdentityReadiness, IdentityRegistry,
    IdentitySelector, IdentitySummary, RegisterHandleRequest,
};
pub use crate::ids::{
    Cursor, Did, GroupRef, Handle, IdentityId, MessageId, Page, PageLimit, PeerRef,
};
pub use crate::messages::{
    HistoryQuery, InboxQuery, Message, MessageBody, MessageBodyView, MessageDeliveryOptions,
    MessageDirection, MessageKind, MessageMetadata, MessageSecurityMode, MessageService,
    MessageTarget, SendMessageRequest, SendMessageResult, ThreadRef,
};
pub use crate::paths::{ImCorePaths, IdentityRegistryPaths, LocalStatePaths, RuntimePaths};
```

## 6. Compile fence

增加一个 xtask 或测试，确保 `crates/im-core/src` 不出现 CLI 类型：

```text
ParsedCommand
GlobalOptions
ExitError
config::Resolved
identity::Manager
crate::app
crate::cli
crate::output
awiki_cli
```

也不允许默认 public API 暴露：

```text
ActorContext
StoredIdentity
ClientIdentityRuntime
IdentityRuntimePaths
LocalStatePaths as business parameter
build_*_rpc_params
SQLite connection
raw serde_json payload as Message public field
```

## 7. P1 编译目标

P1A 完成后至少满足：

```bash
cargo test -p im-core --locked
cargo test -p awiki-cli --locked
cargo +1.79.0 fmt --check
```

如果当前工具链以 workspace `rust-version = 1.78` 为准，接口不得依赖高于 1.78/1.79 的 Rust 语法。
