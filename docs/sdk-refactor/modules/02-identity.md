# identity 模块接口设计

**阅读顺序**：02 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：身份与账号能力。

## 1. 目标

`identity` 包含当前 CLI 的大部分 `id.*` 业务能力，但不包含 CLI 的本地文件布局。它负责注册、恢复、绑定、解析、profile、DID 替换等业务顺序和验证规则。公开 SDK 应以 `IdentityRegistry` 和绑定身份的 `ImClient` 为入口，避免 App/CLI 直接传路径或 actor。

## 2. 主要职责

- `register_handle(input)`：注册 handle-backed identity。
- `recover_handle(input)`：恢复 handle。
- `bind_contact(phone/email/otp)`：绑定手机号或邮箱。
- `resolve_identity(handle_or_did)`：DID / handle 解析。
- `switch_identity(identity_ref)`：返回切换意图和所需目标 identity 信息；默认 identity 文件更新由 CLI 完成。
- `get_profile(subject)` / `update_profile(patch)`。
- `replace_did(request)`：生成并保存新 DID document 和私钥，并返回明确风险信息。
- `import_legacy_identity(plan)`：可作为迁移能力，但 CLI 负责选择 legacy 路径。

## 3. Phase A 路径需求

- `IdentityPaths`：DID document、key1 私钥、E2EE 私钥、auth/session 文件、identity 元数据文件。
- `IdentityOutputPaths`：注册、恢复、replace DID 时的 DID document、私钥、备份和临时文件目标路径。
- `IdentityPathSet`：由 CLI 或 App 枚举后传入的多个 identity path bundle。
- `ImCoreConfig`：user-service / did-auth 端点、DID domain、profile/security profile 默认值。

## 4. 多身份 SDK 模型

`im-core` 必须按多身份设计，以兼容当前 “一个 CLI 多个身份” 的使用方式，并支持未来 App 多账号、后台多账号监听和账号切换。

核心模型：

- `ImCore` 是环境级入口，不绑定单个身份。它持有 `ImCoreConfig`、`ImCorePaths` 和身份注册表访问能力。
- `IdentityRegistry` 负责列出、解析、加载和设置默认身份。
- `IdentitySelector` 表达调用方选择身份的方式，例如 default、identity id、DID、handle、CLI identity name。
- `ImClient` / `ImSession` 是绑定单个身份运行时的业务客户端；公开层只暴露 `IdentitySummary`、DID、handle 等身份摘要，不暴露 `ActorContext`、私钥路径、auth 路径或 secure 路径。
- `messages`、`groups`、`attachments`、`secure`、`realtime` 等业务能力对外应优先通过绑定身份的 `ImClient` 调用。

不推荐把“当前身份”作为 `im-core` 的全局隐式状态。`Default identity` 是 CLI 或 App 的 UX 选择，真正执行业务的身份必须在 `ImClient` 中显式绑定。

推荐调用形态：

```rust
let core = ImCore::new(config, paths)?;

let default_client = core.client(IdentitySelector::Default).await?;
let alice_client = core.client(IdentitySelector::Name("alice".into())).await?;
let bob_client = core.client(IdentitySelector::Did(bob_did)).await?;

alice_client.messages().send(request).await?;
bob_client.groups().list(GroupQuery::default()).await?;

alice_client.realtime()
    .run_until_shutdown(options, shutdown)
    .await?;
```

多身份下的隔离规则：

- auth/session 必须按身份隔离，`alice.auth()` 只读写 alice 的 auth paths，`bob.auth()` 只读写 bob 的 auth paths。
- secure / MLS 必须按身份隔离，`IdentityPaths.e2ee_private_path`、direct session、prekey、secure outbox、MLS state 都应绑定在内部 `IdentityRecord` / `ClientIdentityRuntime` 上，并由 `ImClient` 间接持有。
- 本地 SQLite 可以在 Phase A 使用共享数据库，但所有 message/group/contact/outbox 记录和查询必须按 `owner_did` 或 `owner_identity_id` 隔离。
- `ImClient` 应自动向本地状态查询注入 owner，避免 CLI handler 或 App 手动拼 owner 条件。
- `realtime` 多身份监听可以由调用方为多个 `ImClient` 分别启动 runner；第一版不要求提供 `run_many` convenience API。

Phase A 可以继续兼容当前 CLI 文件布局，但 `ImCorePaths` 应表达身份注册表入口，而不是只表达单个身份：

```rust
pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: LocalStatePaths,
    pub runtime: RuntimePaths,
}

pub struct IdentityRegistryPaths {
    pub entries: Vec<IdentityPathBundle>,
    pub identity_root_dir: Option<PathBuf>,
    pub default_identity_path: Option<PathBuf>,
    pub registry_path: Option<PathBuf>,
}

pub struct IdentitySummary {
    pub id: IdentityId,
    pub did: Did,
    pub handle: Option<String>,
    pub name: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
}

pub(crate) struct IdentityRecord {
    pub summary: IdentitySummary,
    pub paths: IdentityRuntimePaths,
}

pub struct IdentityPathBundle {
    pub id: IdentityId,
    pub name: Option<String>,
    pub paths: IdentityRuntimePaths,
}

pub struct IdentityRuntimePaths {
    pub identity: IdentityPaths,
    pub auth: AuthStatePaths,
    pub secure: SecureStatePaths,
}
```

`IdentityPathBundle` 是 Phase A 构造 `ImCore` 时的路径参数，不是业务 API 的返回值。公开身份枚举仍返回 `IdentitySummary`。如果当前没有 registry 文件，`identity_root_dir` 可以作为显式枚举入口。关键限制不变：`im-core` 可以在调用方传入的目录下枚举身份，但不能自己发现 workspace，也不能读取 CLI config。

## 5. 接口草案

```rust
pub enum IdentitySelector {
    Default,
    Id(IdentityId),
    Did(Did),
    Handle(String),
    Name(String),
}

pub struct IdentityRegistry<'a> {
    core: &'a ImCore,
}

pub struct IdentityService<'a> {
    client: &'a ImClient,
}

impl ImCore {
    pub fn identities(&self) -> IdentityRegistry<'_>;

    pub async fn client(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<ImClient>;
}

impl IdentityRegistry<'_> {
    pub async fn register_handle(
        &self,
        request: RegisterHandleRequest,
    ) -> ImResult<IdentityRegistration>;

    pub async fn recover_handle(
        &self,
        request: RecoverHandleRequest,
    ) -> ImResult<RecoveredIdentity>;

    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;

    pub fn resolve(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentitySummary>;

    pub(crate) fn load(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<LoadedIdentity>;

    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;

    pub fn set_default_identity(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentitySummary>;
}

impl IdentityService<'_> {
    pub async fn bind_contact(
        &self,
        request: BindContactRequest,
    ) -> ImResult<BindContactResult>;

    pub async fn resolve_identity(
        &self,
        subject: IdentitySubject,
    ) -> ImResult<ResolvedIdentity>;

    pub async fn profile(&self) -> ImResult<Profile>;

    pub async fn update_profile(
        &self,
        patch: ProfilePatch,
    ) -> ImResult<Profile>;

    pub async fn replace_did(
        &self,
        request: ReplaceDidRequest,
    ) -> ImResult<ReplaceDidResult>;
}
```

`IdentityRegistry::list()` / `resolve()` / `default_identity()` 只返回 `IdentitySummary`。`IdentityRegistry::load()` 是 `pub(crate)` 内部装配函数，或由 `ImCore::client(selector)` 间接使用；只有这一步才读取 DID document、校验关键路径并构造内部 `ActorContext`。注册、恢复、默认身份选择属于环境级 identity registry 能力；绑定手机号/邮箱、更新 profile、replace DID 属于已绑定身份的 `client.identity()` 能力。

路径级函数如 `load_identity(paths)`、`summarize_identity(paths)`、`summarize_identities(identity_path_set)` 和需要 `IdentityOutputPaths` 的写入步骤属于 registry/client 构造和注册恢复流程的内部实现，不作为 App 面向主接口暴露。公开接口不能返回 `IdentityRecord` 或 `IdentityRuntimePaths`，否则调用方会拿到私钥、auth、secure 状态路径，破坏高层 SDK 边界。

## 6. CLI 边界

CLI 负责：

- identity 目录在哪里；
- 私钥文件怎么命名；
- 文件权限怎么设；
- 备份目录怎么创建；
- 默认 identity 文件路径如何传入；
- CLI 输出和危险命令确认 UX。

`identity` 不接收 `--identity`、`--profile-file`、`ParsedCommand`、`ExitError`。

CLI 的 `--identity alice` 应转换为 `IdentitySelector::Name("alice")`；没有 `--identity` 时转换为 `IdentitySelector::Default`。业务调用应使用 `core.client(selector)` 得到绑定身份的 `ImClient`，再调用 `client.messages()`、`client.groups()`、`client.secure()`、`client.realtime()`。

## 7. Phase B 可选演进

Phase B 可在 `IdentityPaths` 背后增加外部 credential 接管能力，但不作为 Phase A 前置条件。
