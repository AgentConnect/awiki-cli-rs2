# 02-identity-auth：Phase 1 身份、鉴权与 Handle 注册

## 1. 目标

Phase 1 的 identity/auth 目标是让 SDK 能绑定身份、完成 DID auth/session、注册 handle，并支撑私聊/群聊消息发送。

不在 Phase 1 中实现完整 profile、recover、replace DID、directory contacts。

## 2. 多身份模型

```rust
pub enum IdentitySelector {
    Default,
    Id(IdentityId),
    Did(Did),
    Handle(Handle),
    LocalAlias(String),
}
```

`LocalAlias` 表达 CLI credential name / 本地账号别名。不要使用 `Name(String)`，避免和 display name 或 handle 混淆。

## 3. Phase 1 IdentityRegistry

```rust
impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;
    pub fn register_handle(&self, request: RegisterHandleRequest) -> ImResult<IdentityRegistration>;
    pub fn plan_default_identity_change(&self, selector: IdentitySelector) -> ImResult<DefaultIdentityChange>;
}
```

`IdentityRegistry::load()` 是内部方法，用于 `core.client(selector)` 组装 `ImClient`，不返回给 App/CLI。

## 4. Phase 1 AuthService

```rust
impl AuthService<'_> {
    pub fn login(&self) -> ImResult<SessionBundle>;
    pub fn ensure_session(&self, scope: AuthScope) -> ImResult<SessionBundle>;
    pub fn refresh_session(&self) -> ImResult<SessionUpdate>;
    pub fn status(&self) -> ImResult<AuthStatus>;
}
```

Phase 1 重点是 `AuthScope::Messaging` / `GroupMessaging`。`UserProfile` scope 可以等 Phase 2 profile/directory 迁移时完善。

## 5. CLI 边界

CLI 保留：

- `--identity` 解析并转换成 `IdentitySelector::LocalAlias` 或 `Default`。
- OTP 输入。
- identity alias 选择。
- DID document / key / auth 文件路径。
- 文件权限和备份。
- default identity 文件写入。
- 输出和错误 hint。

`im-core` 负责：

- 身份摘要。
- 身份 readiness。
- 加载显式路径中的 DID document 和 signing key。
- DID auth/login/refresh。
- 按身份隔离 auth/session。
- register handle 的业务请求和领域结果。

## 6. 后续阶段

Phase 2 再迁移：

```text
bind phone/email
recover handle
profile get/update
id resolve / directory resolve
replace DID
legacy import/rebind 相关高级能力
```

## 7. 完成判定

- `id register` 走 `core.identities().register_handle()`。
- `id refresh-token` 走 `client.auth().refresh_session()`。
- `msg send` 可以通过 `client.auth().ensure_session(AuthScope::Messaging)` 自动保证 session。
- public API 不暴露 private key、auth path、DID document path。
