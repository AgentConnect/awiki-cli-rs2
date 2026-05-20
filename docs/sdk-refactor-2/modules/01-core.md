# 模块设计：core

## 1. 职责

`core` 是 `im-core` 的公共入口层，负责：

- SDK 初始化。
- 配置和路径总入口。
- 多身份 registry 入口。
- 绑定身份的 `ImClient` 构造。
- bootstrap 生命周期。
- 统一错误类型。
- 基础分页、ID、cursor 类型。

`core` 不直接实现业务 RPC，但其他模块都依赖它。

## 2. 对外接口

```rust
pub struct ImCore;
pub struct ImClient;
pub struct ImCoreConfig;
pub struct ImCorePaths;
pub struct CoreBootstrap<'a>;
pub enum ImError;
pub type ImResult<T> = Result<T, ImError>;
```

```rust
impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;
    pub fn identities(&self) -> IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;
    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn auth(&self) -> AuthService<'_>;
    pub fn identity(&self) -> IdentityService<'_>;
    pub fn directory(&self) -> DirectoryService<'_>;
    pub fn messages(&self) -> MessageService<'_>;
    pub fn groups(&self) -> GroupService<'_>;
}
```

## 3. 内部类型

以下类型只能是 `pub(crate)`：

```rust
ClientIdentityRuntime
ActorContext
LoadedIdentity
IdentityRuntimePaths
AuthStatePaths
LocalOwnerContext
```

不提供 `client.actor()`、`client.paths()`、`client.auth_paths()` 这类逃逸口。

## 4. 路径原则

`ImCorePaths` 只在构造和 bootstrap 阶段作为路径入口。业务 API 不再接收 path 参数。

```rust
client.messages().send(request)
```

而不是：

```rust
send(actor, auth_paths, local_state_paths, request)
```

## 5. 第一阶段实现建议

第一阶段可以把 `ImCore` 做成薄 façade，内部临时调用旧 CLI 模块，但 public API 必须先按目标形态稳定下来。等 handler 都切到 façade 后，再逐步把旧模块移动到 `crates/im-core`。
