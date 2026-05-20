# core 模块接口设计

**阅读顺序**：01 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：基础类型、统一入口、错误和操作上下文。

## 1. 目标

`core` 是 `im-core` 的公共入口层。它不承载具体业务流程，但为 identity、auth、messages、groups、attachments、secure、realtime、local_state、discovery 提供统一配置、路径、actor、错误和结果类型。

## 2. 主要职责

- `ImCore` / `ImClient` 总入口。
- `ImCoreConfig`：服务地址、DID domain、runtime mode 等不含本地路径的配置。
- `ImCorePaths`：Phase A 路径参数总入口。
- `ActorContext`：当前 DID、handle、credential name、device id、session id。
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

pub struct ImCoreConfig {
    pub service_base_url: Url,
    pub did_domain: String,
    pub runtime_mode: RuntimeMode,
    pub user_service_endpoint: Option<Url>,
    pub message_service_endpoint: Option<Url>,
    pub attachment_service_endpoint: Option<Url>,
}

pub struct ActorContext {
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

## 7. 依赖边界

`core` 可以被其他 `im-core` 模块依赖。它不能依赖 `awiki-cli`，也不能引入 CLI 类型。
