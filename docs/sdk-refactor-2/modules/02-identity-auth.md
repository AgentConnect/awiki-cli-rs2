# 模块设计：identity-auth

## 1. 职责

`identity` 和 `auth` 是第一阶段基础能力，负责：

- 多身份枚举、默认身份、selector 解析。
- handle 注册和恢复。
- profile 读取和更新。
- DID auth / session 刷新 / logout。
- 绑定身份后自动持有身份运行时。

## 2. IdentitySelector

```rust
pub enum IdentitySelector {
    Default,
    Id(IdentityId),
    Did(Did),
    Handle(Handle),
    LocalAlias(String),
}
```

不用 `Name(String)`，避免和用户 display name 混淆。CLI 的 `--identity alice` 转成 `LocalAlias("alice")`。

## 3. IdentitySummary

```rust
pub struct IdentitySummary {
    pub id: IdentityId,
    pub did: Did,
    pub handle: Option<Handle>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub readiness: IdentityReadiness,
}
```

`IdentitySummary` 不含私钥、JWT、DID document path、auth path、secure path。

## 4. Registry API

```rust
impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;
    pub fn register_handle(&self, request: RegisterHandleRequest) -> ImResult<IdentityRegistration>;
    pub fn recover_handle(&self, request: RecoverHandleRequest) -> ImResult<RecoveredIdentity>;
    pub fn plan_default_identity_change(&self, selector: IdentitySelector) -> ImResult<DefaultIdentityChange>;
}
```

`load(selector)` 可以存在，但必须是 `pub(crate)`，供 `ImCore::client(selector)` 内部使用。

## 5. Auth API

```rust
impl AuthService<'_> {
    pub fn login(&self) -> ImResult<SessionBundle>;
    pub fn ensure_session(&self, scope: AuthScope) -> ImResult<SessionBundle>;
    pub fn refresh_session(&self) -> ImResult<SessionUpdate>;
    pub fn logout(&self) -> ImResult<SessionUpdate>;
    pub fn status(&self) -> ImResult<AuthStatus>;
}
```

auth path 和 token persistence 是 SDK 内部职责，但路径由 `ImCorePaths` 显式传入。

## 6. CLI 边界

CLI 负责：

- identity alias 来源。
- identity root/default 文件路径。
- 私钥文件权限。
- OTP 输入和危险命令确认。
- 默认身份文件的实际写入。
- 输出。

SDK 负责：

- 注册/恢复业务流程。
- session refresh。
- profile API。
- 身份 readiness 计算。

## 7. 第一阶段不做

- replace DID 可后移，或只做 façade 保持旧逻辑。
- import legacy 可留 CLI。
- credential vault/provider 不做。
