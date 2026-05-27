# 02. Core Interface

本文定义 Phase 1 必须可编码的 core 接口。

## 1. 基础 ID 类型

`ids.rs`：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdentityId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerRef(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupRef(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageLimit(pub u32);
```

P1 构造函数建议：

```rust
impl Did {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

impl Handle {
    pub fn parse(input: impl AsRef<str>, default_domain: &str) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

impl PeerRef {
    pub fn parse(input: impl AsRef<str>, default_domain: &str) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

impl GroupRef {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}
```

P1 可以先让 `PeerRef` / `GroupRef` 内部包 `String`，但必须通过构造函数 normalize，避免业务层到处裸 `String`。

## 2. Config

`config.rs`：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEndpoint(String);

impl ServiceEndpoint {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCoreConfig {
    pub service_base_url: ServiceEndpoint,
    pub did_domain: String,
    pub user_service_endpoint: Option<ServiceEndpoint>,
    pub message_service_endpoint: Option<ServiceEndpoint>,
    pub transport_policy: MessageTransportPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,

    // Reserved for Phase 5.
    RealtimePreferred,
}
```

为什么 P1 不用 `Url`：

- 当前 workspace 还没有 `url` dependency。
- P1 目标是最小改动跑通 SDK 主链路。
- endpoint 校验可以先在 `ServiceEndpoint::parse` 做基础校验；后续若需要再引入 `url::Url`。

## 3. Paths

`paths.rs`：

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: LocalStatePaths,
    pub runtime: RuntimePaths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRegistryPaths {
    pub identity_root_dir: PathBuf,
    pub registry_path: PathBuf,
    pub default_identity_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStatePaths {
    pub sqlite_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePaths {
    pub cache_dir: PathBuf,
    pub temp_dir: PathBuf,
}
```

P1 业务 API 不允许出现：

```rust
fn send(paths: LocalStatePaths, ...)
fn refresh(auth_path: PathBuf, ...)
fn inbox(owner_did: String, sqlite_path: PathBuf, ...)
```

paths 只进入 `ImCore::new`、`bootstrap()` 和内部 runtime resolution。

## 4. Error

`error.rs`：

```rust
use std::fmt;

pub type ImResult<T> = Result<T, ImError>;

#[derive(Debug)]
pub enum ImError {
    InvalidInput { field: Option<String>, message: String },

    IdentityRequired,
    IdentityNotFound { selector: String },
    DefaultIdentityMissing,
    IdentityNotReady { identity: String, missing: Vec<String> },

    AuthRequired,
    SessionExpired,
    PermissionDenied,

    PeerNotFound { peer: String },
    GroupNotFound { group: String },
    MessageNotFound { message_id: String },

    TransportUnavailable { detail: String },
    UnsupportedCapability { capability: String },

    LocalStateUnavailable { detail: String },
    PathUnavailable { path_kind: String, detail: String },
    CredentialFileUnreadable { path_kind: String, detail: String },

    Service { status_code: Option<u16>, code: Option<String>, message: String },
    Serialization { detail: String },
    Io { detail: String },
    Internal { message: String },
}

impl fmt::Display for ImError { /* required */ }
impl std::error::Error for ImError {}
```

P1 要求：

- `ImError` 不包含 CLI exit code。
- CLI 用 adapter 映射 exit code 和 hint。
- 低层 `MessageError`、`IdentityError` 等通过 `From` 或 adapter 转成 `ImError`，不直接暴露。

## 5. Core Types

`core/mod.rs`：

```rust
use std::sync::Arc;

pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

pub(crate) struct ImCoreInner {
    pub(crate) config: crate::config::ImCoreConfig,
    pub(crate) paths: crate::paths::ImCorePaths,
}

pub struct ImClient {
    core: Arc<ImCoreInner>,
    identity: crate::identity::IdentitySummary,
    runtime: Arc<crate::internal::identity_runtime::ClientIdentityRuntime>,
}

impl ImCore {
    pub fn new(config: crate::ImCoreConfig, paths: crate::ImCorePaths) -> crate::ImResult<Self>;

    pub fn identities(&self) -> crate::identity::IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;
    pub fn client(&self, selector: crate::identity::IdentitySelector) -> crate::ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &crate::identity::IdentitySummary;
    pub fn did(&self) -> &crate::ids::Did;
    pub fn handle(&self) -> Option<&crate::ids::Handle>;

    pub fn auth(&self) -> crate::auth::AuthService<'_>;
    pub fn messages(&self) -> crate::messages::MessageService<'_>;

    pub(crate) fn runtime(&self) -> &crate::internal::identity_runtime::ClientIdentityRuntime;
    pub(crate) fn core_inner(&self) -> &ImCoreInner;
}
```

P1 不提供：

```rust
pub fn actor_context(&self) -> ActorContext
pub fn paths(&self) -> &IdentityRuntimePaths
pub fn raw_session(&self) -> &Session
pub fn sqlite_connection(&self) -> Connection
```

## 6. Bootstrap

`core/bootstrap.rs`：

```rust
use serde::{Deserialize, Serialize};

pub struct CoreBootstrap<'a> {
    core: &'a ImCore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathValidationReport {
    pub checked: Vec<PathCheck>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCheck {
    pub kind: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
    pub writable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStateStatus {
    pub sqlite_path: String,
    pub initialized: bool,
    pub schema_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub sqlite_path: String,
    pub from_version: Option<u32>,
    pub to_version: u32,
    pub applied: Vec<String>,
}

impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> crate::ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> crate::ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> crate::ImResult<MigrationReport>;
}
```

P1 `initialize_local_state` 可以只确保 sqlite path parent exists 并初始化最小 schema。完整 message/conversation projection 放 Phase 3。

## 7. P1 Core Invariants

- `ImCore::new` 不读取 CLI config。
- `ImCore::new` 不自动发现 workspace。
- `ImCore::client` 解析身份后，`ImClient` 后续调用不再传 `identity_name`。
- `ImClient` 内部绑定 owner，App/CLI 不拼 owner condition。
- P1 所有 public 方法 blocking。
