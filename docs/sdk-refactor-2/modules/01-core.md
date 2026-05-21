# 01-core：Phase 1 SDK 核心入口

## 1. 目标

`core` 是 `im-core` 的公共入口层。Phase 1 只要求它支撑 SDK 跑起来：配置、显式路径、多身份 registry、绑定身份、错误、bootstrap 和基础 DTO。

## 2. Phase 1 public API

```rust
pub struct ImCore;
pub struct ImClient;
pub struct CoreBootstrap<'a>;

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
}
```

Phase 2+ 再增加 `identity()`、`directory()`；Phase 3+ 增加 `groups()`；Phase 5+ 增加 `realtime()`；Phase 6+ 增加 `secure()`。

## 3. ImCoreConfig

```rust
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
    RealtimePreferred, // Phase 5
}
```

CLI 的 `runtime_mode` 不直接进入 SDK。CLI adapter 负责把本机配置转换成 `MessageTransportPolicy`。

## 4. ImCorePaths

```rust
pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: Option<LocalStatePaths>,
    pub runtime: Option<RuntimePaths>,
}
```

Phase 1 可以只使用身份、auth 和必要 local state 路径。业务函数不接收 `*Paths`。

## 5. 内部类型

以下类型只能是 `pub(crate)`：

```rust
ActorContext
ClientIdentityRuntime
LoadedIdentity
IdentityRuntimePaths
AuthStatePaths
LocalStateConnection
```

`ImClient` 可以内部持有这些运行时对象，但不能让 App/CLI 拿到。

## 6. Bootstrap

```rust
impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

Phase 1 只要求 bootstrap 能验证显式路径并为消息主链路准备必要本地状态。完整 projection/cache merge 后移。

## 7. 错误边界

`ImError` 表达领域失败，不包含 CLI exit code。CLI 负责映射为 `ExitError`、hint 和输出格式。

## 8. 完成判定

- `im-core` 可独立编译。
- `ImCore::new(config, paths)` 不需要 CLI `Resolved`。
- `core.client(selector)` 不需要 CLI `Manager`。
- `im-core` 中不存在 CLI 类型引用。
