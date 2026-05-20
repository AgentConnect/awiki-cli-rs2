# identity 模块接口设计

**所属 crate**：`crates/im-core`  
**模块职责**：身份与账号能力。

## 1. 目标

`identity` 包含当前 CLI 的大部分 `id.*` 业务能力，但不包含 CLI 的本地文件布局。它负责注册、恢复、绑定、解析、profile、DID 替换等业务顺序和验证规则。

## 2. 主要职责

- `register_handle(actor/input)`：注册 handle-backed identity。
- `recover_handle(input)`：恢复 handle。
- `bind_contact(actor, phone/email/otp)`：绑定手机号或邮箱。
- `resolve_identity(handle_or_did)`：DID / handle 解析。
- `load_identity(paths)`：从显式 DID document、签名私钥、E2EE 私钥、auth/session 路径加载当前身份上下文。
- `summarize_identity(paths)`：读取显式路径并返回 status/profile/security 摘要。
- `summarize_identities(identity_path_set)`：对 CLI 或 App 已经枚举出的 identity 路径集合做业务摘要；不自行扫描工作区。
- `switch_identity(identity_ref)`：返回切换意图和所需目标 identity 信息；默认 identity 文件更新由 CLI 完成。
- `get_profile(subject)` / `update_profile(actor, patch)`。
- `replace_did(actor, request, output_paths)`：使用调用方传入的输出路径保存新 DID document 和私钥，并返回明确风险信息。
- `import_legacy_identity(plan)`：可作为迁移能力，但 CLI 负责选择 legacy 路径。

## 3. Phase A 路径需求

- `IdentityPaths`：DID document、key1 私钥、E2EE 私钥、auth/session 文件、identity 元数据文件。
- `IdentityOutputPaths`：注册、恢复、replace DID 时的 DID document、私钥、备份和临时文件目标路径。
- `IdentityPathSet`：由 CLI 或 App 枚举后传入的多个 identity path bundle。
- `ImCoreConfig`：user-service / did-auth 端点、DID domain、profile/security profile 默认值。

## 4. 接口草案

```rust
pub struct IdentityService<'a> {
    core: &'a ImCore,
}

impl IdentityService<'_> {
    pub async fn register_handle(
        &self,
        request: RegisterHandleRequest,
        output_paths: IdentityOutputPaths,
    ) -> ImResult<IdentityRegistration>;

    pub async fn recover_handle(
        &self,
        request: RecoverHandleRequest,
        output_paths: IdentityOutputPaths,
    ) -> ImResult<RecoveredIdentity>;

    pub async fn bind_contact(
        &self,
        actor: ActorContext,
        request: BindContactRequest,
    ) -> ImResult<BindContactResult>;

    pub async fn resolve_identity(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<ResolvedIdentity>;

    pub fn load_identity(
        &self,
        paths: &IdentityPaths,
    ) -> ImResult<ActorContext>;

    pub fn summarize_identity(
        &self,
        paths: &IdentityPaths,
    ) -> ImResult<IdentitySummary>;

    pub async fn get_profile(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<Profile>;

    pub async fn update_profile(
        &self,
        actor: ActorContext,
        patch: ProfilePatch,
    ) -> ImResult<Profile>;

    pub async fn replace_did(
        &self,
        actor: ActorContext,
        request: ReplaceDidRequest,
        output_paths: IdentityOutputPaths,
    ) -> ImResult<ReplaceDidResult>;
}
```

## 5. CLI 边界

CLI 负责：

- identity 目录在哪里；
- 私钥文件怎么命名；
- 文件权限怎么设；
- 备份目录怎么创建；
- 默认 identity 文件如何更新；
- CLI 输出和危险命令确认 UX。

`identity` 不接收 `--identity`、`--profile-file`、`ParsedCommand`、`ExitError`。

## 6. Phase B 可选演进

Phase B 可在 `IdentityPaths` 背后增加外部 credential 接管能力，但不作为 Phase A 前置条件。
