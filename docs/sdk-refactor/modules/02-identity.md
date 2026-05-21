# identity 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P1 包含 registry / local identity resolve / Handle 注册；P2 补全 recover、profile、bind、replace DID。  
**职责**：身份与账号能力。

## 1. 目标

`identity` 负责多身份 registry、Handle 注册、后续恢复、绑定、解析、profile、DID 替换等业务顺序和验证规则。公开 SDK 以 `IdentityRegistry` 和绑定身份的 `ImClient` 为入口，避免 App/CLI 直接传路径或 actor。

## 2. 多身份模型

```rust
pub enum IdentitySelector {
    Default,
    Id(IdentityId),
    Did(Did),
    Handle(Handle),
    LocalAlias(String),
}

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

规则：

- `ImCore` 是环境级入口，不绑定身份。
- `IdentityRegistry` 负责 list/default/resolve/register。
- `ImClient` 是绑定单个身份后的业务入口。
- 不把“当前身份”作为 SDK 全局状态。
- CLI 的 `--identity alice` 转换为 `IdentitySelector::LocalAlias("alice")`。
- `IdentitySelector::Name` 不使用，避免和 display name 混淆。

## 3. P1 职责

- `list()`：列出本地身份摘要。
- `default_identity()`：读取默认身份摘要。
- `resolve(selector)`：解析 selector 到身份摘要。
- `register_handle(request)`：Handle 注册。
- `plan_default_identity_change(selector)`：返回默认身份切换计划。
- `load(selector)`：`pub(crate)`，供 `ImCore::client(selector)` 装配内部 runtime。

P1 不做完整 profile/directory/recover/replace DID。

## 4. P2+ 职责

- `recover_handle(request)`。
- `bind_contact(phone/email/otp)`。
- `profile()` / `update_profile(patch)`。
- `replace_did(request)`。
- `import_legacy_identity(plan)`，可作为迁移工具或 advanced feature。

## 5. 接口草案

```rust
pub struct IdentityRegistry<'a> {
    core: &'a ImCore,
}

impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;

    pub fn register_handle(
        &self,
        request: RegisterHandleRequest,
    ) -> ImResult<IdentityRegistration>;

    pub fn plan_default_identity_change(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<DefaultIdentityChange>;

    pub(crate) fn load(&self, selector: IdentitySelector) -> ImResult<LoadedIdentity>;
}
```

P2+：

```rust
impl IdentityRegistry<'_> {
    pub fn recover_handle(
        &self,
        request: RecoverHandleRequest,
    ) -> ImResult<RecoveredIdentity>;
}

pub struct IdentityService<'a> {
    client: &'a ImClient,
}

impl IdentityService<'_> {
    pub fn bind_contact(&self, request: BindContactRequest) -> ImResult<BindContactResult>;
    pub fn profile(&self) -> ImResult<Profile>;
    pub fn update_profile(&self, patch: ProfilePatch) -> ImResult<Profile>;
    pub fn replace_did(&self, request: ReplaceDidRequest) -> ImResult<ReplaceDidResult>;
}
```

## 6. 路径边界

`IdentityPathBundle` 是构造 `ImCore` 时的路径参数，不是业务 API 的返回值。公开身份枚举只返回 `IdentitySummary`。

P1 可以兼容当前 CLI 文件布局，但 `im-core` 不自行发现 workspace，也不读取 CLI config。

## 7. CLI 边界

CLI 负责：

- identity 目录在哪里；
- 私钥文件怎么命名；
- 文件权限怎么设；
- 备份目录怎么创建；
- 默认 identity 文件路径如何传入；
- CLI 输出和危险命令确认 UX。

`identity` 不接收 `--identity`、`--profile-file`、`ParsedCommand`、`ExitError`。
