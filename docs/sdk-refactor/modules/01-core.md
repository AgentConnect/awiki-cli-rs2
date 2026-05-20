# core 模块接口设计

**阅读顺序**：01 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：基础类型、统一入口、错误和操作上下文。

## 1. 目标

`core` 是 `im-core` 的公共入口层。它不承载具体业务流程，但为 identity、auth、messages、groups、attachments、secure、realtime、local_state、discovery 提供统一配置、路径、身份绑定、错误和结果类型。

## 2. 主要职责

- `ImCore`：环境级总入口，不绑定单个身份。
- `ImClient`：绑定单个身份后的高层业务入口。
- `ImCoreConfig`：服务地址、DID domain、runtime mode 等不含本地路径的配置。
- `ImCorePaths`：Phase A 路径参数总入口。
- `IdentityRegistry`：多身份枚举、解析和 default 选择入口。
- `IdentitySummary`：对外展示的身份摘要，不包含私钥、auth、secure、本地状态路径。
- `ClientIdentityRuntime` / `ActorContext`：内部身份运行时，包含 actor、auth paths、secure paths 等底层状态，只在 `im-core` 内部使用。
- `CoreBootstrap`：路径校验、本地状态初始化/迁移等生命周期入口，避免 CLI 直接调用 store 层。
- `OperationContext`：trace id、operation id、deadline、幂等键。
- `ImError`：领域错误，不能携带 CLI exit code。
- `ImResult<T>`。
- 分页、cursor、时间戳、message id、group id、attachment id 等基础类型。

## 3. 不负责

- CLI flag。
- 配置文件路径。
- stdout/stderr。
- process exit code。
- workspace 自动发现。
- CLI `ParsedCommand` / `ExitError` / `GlobalOptions`。

## 4. 接口草案

```rust
pub struct ImCore {
    config: ImCoreConfig,
    paths: ImCorePaths,
}

pub struct ImClient {
    core: Arc<ImCoreInner>,
    identity: IdentitySummary,
    runtime: Arc<ClientIdentityRuntime>,
}

pub struct CoreBootstrap<'a> {
    core: &'a ImCore,
}

pub struct ImCoreConfig {
    pub service_base_url: Url,
    pub did_domain: String,
    pub runtime_mode: RuntimeMode,
    pub user_service_endpoint: Option<Url>,
    pub message_service_endpoint: Option<Url>,
    pub attachment_service_endpoint: Option<Url>,
}

pub struct IdentitySummary {
    pub id: IdentityId,
    pub did: Did,
    pub handle: Option<String>,
    pub name: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
}

pub(crate) struct ClientIdentityRuntime {
    actor: ActorContext,
    paths: IdentityRuntimePaths,
}

pub(crate) struct ActorContext {
    pub did: Did,
    pub handle: Option<String>,
    pub credential_name: Option<String>,
    pub device_id: Option<String>,
    pub session_id: Option<String>,
}

pub struct OperationContext {
    pub trace_id: Option<String>,
    pub operation_id: Option<String>,
    pub deadline: Option<SystemTime>,
    pub idempotency_key: Option<String>,
}

pub type ImResult<T> = Result<T, ImError>;

impl ImCore {
    pub fn identities(&self) -> IdentityRegistry<'_>;

    pub fn bootstrap(&self) -> CoreBootstrap<'_>;

    pub async fn client(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn did(&self) -> &Did;
    pub fn handle(&self) -> Option<&str>;
    pub fn identity(&self) -> IdentityService<'_>;
    pub fn auth(&self) -> AuthService<'_>;
    pub fn directory(&self) -> DirectoryService<'_>;
    pub fn messages(&self) -> MessageService<'_>;
    pub fn groups(&self) -> GroupService<'_>;
    pub fn attachments(&self) -> AttachmentService<'_>;
    pub fn secure(&self) -> SecureService<'_>;
    pub fn realtime(&self) -> RealtimeService<'_>;
}

impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

## 5. 错误类型

`ImError` 应表达领域失败：

- `IdentityRequired`
- `AuthRequired`
- `PermissionDenied`
- `PeerNotFound`
- `GroupNotFound`
- `MessageNotFound`
- `AttachmentNotFound`
- `TransportUnavailable`
- `UnsupportedCapability`
- `SecureSessionMissing`
- `SecureOutboxFailed`
- `PathUnavailable`
- `CredentialFileUnreadable`
- `LocalStateUnavailable`
- `InvalidInput`

CLI 负责把 `ImError` 映射为 exit code、`error.code`、human hint、pretty/table/json 输出。

## 6. 路径边界

`ImCorePaths` 的详细定义在 [整体架构文档](../architecture.md) 中。核心规则：

- `im-core` 可以读取/写入调用方显式传入的路径。
- `im-core` 不解析 `config.yaml`。
- `im-core` 不发现 workspace。
- `im-core` 不假定路径来自 CLI。
- 外部业务调用不直接传 `*Paths`。`core.client(selector)` 负责把身份选择解析成 `ImClient`，并把 actor、auth paths、secure paths 和 local state 绑定到 client 内部。
- CLI 需要做本地状态初始化/迁移时，调用 `core.bootstrap().initialize_local_state()` 或 `core.bootstrap().migrate_local_state()`，不直接调用 store/query/helper 级接口。

## 7. 接口层级

公开 SDK 优先暴露 `ImCore`、`IdentityRegistry`、`CoreBootstrap` 和 `ImClient`。对外身份对象使用 `IdentitySummary`，不暴露路径和运行时状态。底层类型如 `ActorContext`、`ClientIdentityRuntime`、`IdentityPaths`、`AuthStatePaths`、`SecureStatePaths`、`LocalStatePaths`、RPC params、wire payload、SQLite connection 应只作为 `pub(crate)` 内部实现类型或测试辅助使用，不作为业务 API 的主入口参数，也不提供 `ImClient::actor()` 这类逃逸口。

## 8. 依赖边界

`core` 可以被其他 `im-core` 模块依赖。它不能依赖 `awiki-cli`，也不能引入 CLI 类型。
