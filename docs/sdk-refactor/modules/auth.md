# auth 模块接口设计

**所属 crate**：`crates/im-core`  
**模块职责**：登录、会话和认证。

## 1. 目标

`auth` 负责 DID auth、JWT/session 刷新、401 重试策略和会话状态转换。登录是 `im-core` 功能，因为 CLI 和 App 都需要。

## 2. 主要职责

- `login_with_did(actor)`。
- `refresh_session(actor)`。
- `ensure_session(actor, scope)`。
- `logout(actor)`。
- `build_did_auth_request(...)`。
- 处理服务端 token 返回、过期、401 重试策略。
- 返回 `SessionUpdate`，Phase A 可按显式 auth/session 路径保存，也可返回给调用方二次持久化。

## 3. Phase A 路径需求

- `IdentityPaths`：读取 DID document 和签名私钥。
- `AuthStatePaths`：读取/写入 JWT、refresh token、session metadata。
- `ImCoreConfig`：did-auth RPC / REST endpoint。

token 存储位置由 CLI 或 App 通过路径参数决定；`im-core` 不自行选择路径。

## 4. 接口草案

```rust
pub struct AuthService<'a> {
    core: &'a ImCore,
}

impl AuthService<'_> {
    pub async fn login_with_did(
        &self,
        actor: ActorContext,
        paths: &IdentityPaths,
        auth_paths: &AuthStatePaths,
    ) -> ImResult<SessionBundle>;

    pub async fn refresh_session(
        &self,
        actor: ActorContext,
        auth_paths: &AuthStatePaths,
    ) -> ImResult<SessionUpdate>;

    pub async fn ensure_session(
        &self,
        actor: ActorContext,
        scope: AuthScope,
        auth_paths: &AuthStatePaths,
    ) -> ImResult<SessionBundle>;

    pub fn build_did_auth_request(
        &self,
        actor: &ActorContext,
        paths: &IdentityPaths,
        challenge: DidAuthChallenge,
    ) -> ImResult<DidAuthRequest>;

    pub fn logout(
        &self,
        actor: ActorContext,
        auth_paths: &AuthStatePaths,
    ) -> ImResult<SessionUpdate>;
}
```

## 5. CLI 边界

CLI 负责：

- 当前 identity 路径解析；
- auth/session 文件路径解析；
- 文件权限和目录创建；
- `id refresh-token`、login 相关命令输出；
- 将 `AuthRequired`、`CredentialFileUnreadable` 等错误映射成 CLI hint。

`auth` 不负责 CLI 配置读取、workspace 查找或 exit code。
