# core 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P1 起始模块  
**职责**：基础类型、统一入口、错误、路径、bootstrap、身份绑定。

## 1. 目标

`core` 是 `im-core` 的公共入口层。它不承载具体业务流程，但为 identity、auth、messages、后续 groups/attachments/secure/realtime/local_state/discovery 提供统一配置、路径、身份绑定、错误和结果类型。

## 2. P1 职责

- `ImCore`：环境级总入口，不绑定单个身份。
- `ImClient`：绑定单个身份后的高层业务入口。
- `ImCoreConfig`：服务地址、DID domain、transport policy 等不含本地路径的配置。
- `ImCorePaths`：Phase A 路径参数总入口。
- `IdentityRegistry`：多身份枚举、解析、default 选择、Handle 注册入口。
- `IdentitySummary`：对外展示的身份摘要，不包含私钥、auth、secure、本地状态路径。
- `ClientIdentityRuntime` / `ActorContext`：内部身份运行时，只在 `im-core` 内部使用。
- `CoreBootstrap`：路径校验、本地状态初始化/迁移等生命周期入口。
- `ImError` / `ImResult<T>`。
- 分页、cursor、时间戳、message id、group id 等基础类型。

## 3. 不负责

- CLI flag。
- 配置文件路径。
- stdout/stderr。
- process exit code。
- workspace 自动发现。
- CLI `ParsedCommand` / `ExitError` / `GlobalOptions`。
- systemd/launchd/Windows service。
- OpenClaw/Hermes。

## 4. 接口草案

```rust
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

pub struct ImClient {
    core: Arc<ImCoreInner>,
    identity: IdentitySummary,
    runtime: Arc<ClientIdentityRuntime>,
}

pub struct ImCoreConfig {
    pub service_base_url: Url,
    pub did_domain: String,
    pub user_service_endpoint: Option<Url>,
    pub message_service_endpoint: Option<Url>,
    pub transport_policy: MessageTransportPolicy,
}

pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,
    RealtimePreferred, // P5+
}

pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: LocalStatePaths,
    pub runtime: RuntimePaths,
}

impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;
    pub fn identities(&self) -> IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;
    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn did(&self) -> &Did;
    pub fn handle(&self) -> Option<&Handle>;

    pub fn auth(&self) -> AuthService<'_>;
    pub fn messages(&self) -> MessageService<'_>;

    // P2+
    pub fn identity(&self) -> IdentityService<'_>;
    pub fn directory(&self) -> DirectoryService<'_>;

    // P3+
    pub fn groups(&self) -> GroupService<'_>;

    // P4+
    pub fn attachments(&self) -> AttachmentService<'_>;

    // P5+
    pub fn realtime(&self) -> RealtimeService<'_>;

    // P6+
    pub fn secure(&self) -> SecureService<'_>;
}
```

## 5. 错误类型

`ImError` 表达领域失败，不携带 CLI exit code：

```rust
pub enum ImError {
    InvalidInput { field: Option<String>, message: String },
    IdentityRequired,
    IdentityNotFound { selector: String },
    DefaultIdentityMissing,
    AuthRequired,
    SessionExpired,
    PermissionDenied,
    PeerNotFound,
    GroupNotFound,
    MessageNotFound,
    ContactNotFound,
    TransportUnavailable { detail: String },
    UnsupportedCapability { capability: String },
    LocalStateUnavailable { detail: String },
    PathUnavailable { path_kind: String, detail: String },
    CredentialFileUnreadable { path_kind: String, detail: String },
    Service { status_code: Option<u16>, code: Option<String>, message: String },
    Internal { message: String },
}
```

CLI 负责把 `ImError` 映射为 exit code、`error.code`、human hint、pretty/table/json 输出。

## 6. 路径边界

核心规则：

- `im-core` 可以读取/写入调用方显式传入的路径。
- `im-core` 不解析 `config.yaml`。
- `im-core` 不发现 workspace。
- `im-core` 不假定路径来自 CLI。
- 外部业务调用不直接传 `*Paths`。
- `core.client(selector)` 负责把身份选择解析成 `ImClient`，并把 actor、auth paths、secure paths 和 local state 绑定到 client 内部。

## 7. public/internal 边界

公开 SDK 优先暴露 `ImCore`、`IdentityRegistry`、`CoreBootstrap` 和 `ImClient`。对外身份对象使用 `IdentitySummary`，不暴露路径和运行时状态。

以下类型只能是 `pub(crate)`：

```rust
ActorContext
ClientIdentityRuntime
LoadedIdentity
IdentityRuntimePaths
AuthStatePaths in business call
SecureStatePaths in business call
SQLite connection
wire payload
RPC params
```
