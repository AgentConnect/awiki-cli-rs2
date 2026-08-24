# im-core Public API：公共接口总览

## 1. 设计原则

SDK public API 只表达业务意图：

- 选择哪个身份。
- 做什么 IM 操作。
- 返回什么领域结果。

SDK public API 不表达底层实现：

- 不传 `ActorContext`。
- 不在业务函数里传 `owner_did` 或 SQLite path。
- 不传 RPC method name、wire params、raw JSON payload。
- 不传 DID auth proof 参数。
- 不传 secure session、prekey、MLS path。
- 不返回 private key、auth file path、runtime path。

## 2. 阶段标记

本文接口使用以下标记：

```text
P1      第一阶段必须实现，目标是让 SDK 主链路跑起来
P2+     后续阶段实现，但提前明确接口形态
Email  独立 Email / Mail 迁移阶段，详见 Interface/08-email-interface.md
internal 只能在 im-core 内部出现
```

## 3. 顶层入口

```rust
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

pub struct ImClient {
    core: Arc<ImCoreInner>,
    identity: IdentitySummary,
    runtime: Arc<ClientIdentityRuntime>, // pub(crate)
}

pub struct ImCoreConfig {
    pub service_base_url: Url,
    pub did_domain: String,
    pub client_version_info: Option<ClientVersionInfo>,
    pub user_service_endpoint: Option<Url>,
    pub message_service_endpoint: Option<Url>,
    pub mail_service_endpoint: Option<Url>, // Email
    pub transport_policy: MessageTransportPolicy,
}

pub enum MessageTransportPolicy {
    Auto,
    HttpOnly,

    // P5+
    RealtimePreferred,
}

pub struct ImCorePaths {
    pub identities: IdentityRegistryPaths,
    pub local_state: LocalStatePaths,
    pub runtime: RuntimePaths,
}

pub struct ImCoreOpenOptions {
    pub identity_secret_storage_policy: IdentitySecretStoragePolicy,
    pub identity_secret_vault: Option<ImCoreSecretVaultOptions>,
    pub multi_device_device_revoke_enabled: bool, // default false
    pub multi_device_direct_e2ee_enabled: bool, // default false
    pub multi_device_group_e2ee_enabled: bool, // default false
    pub multi_device_handle_recovery_enabled: bool, // default false
    pub external_http_allow_insecure_loopback_for_testing: bool, // default false
}

pub enum IdentitySecretStoragePolicy {
    FileCompat,
    VaultPreferred,
    VaultRequired,
}

pub struct ImCoreSecretVaultOptions {
    // Host-provided no-prompt root key. It must be redacted and never persisted
    // in config, logs, diagnostics, or public DTOs.
    root_key: DeviceVaultRootKey,
    vault_dir: PathBuf,
    workspace_id: String,
    device_id: String,
}

pub struct ClientVersionInfo { /* validated product/release/version/build */ }
```

`ClientVersionInfo` 只接受 `awiki-me|awiki-cli|awiki-daemon`、四位 release、
header-safe version 和可选十进制 build。Core 对配置中同源的 AWiki User/Message/Mail
产品 HTTP 请求及 Message WebSocket 注入唯一
`X-AWiki-Client-Version: <product>/<release>/<version>[+<build>]`；公共 ANP、任意 DID
解析和对象存储请求不携带该头。User Service 产品端点统一使用 canonical
`/user-service/v1/...`，客户端不回退无版本 alias。

`ImCoreSecretVaultOptions.device_id` is the existing compatibility spelling for
the **local vault context device id**. Internally it is parsed as
`VaultContextDeviceId`; it is not the public ANP device endpoint and must never
be copied into a DID Document or message. New multi-device identity code uses a
separate `ProtocolDeviceId` for the opaque Manifest/P5/P6 identifier. New V1
protocol identifiers reject the legacy sentinel `default` and can be generated
with cryptographic randomness; neither type is derived from the other.

P1 API：

```rust
impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;
    pub fn new_with_options(
        config: ImCoreConfig,
        paths: ImCorePaths,
        options: ImCoreOpenOptions,
    ) -> ImResult<Self>;

    pub async fn open(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;
    pub async fn open_with_options(
        config: ImCoreConfig,
        paths: ImCorePaths,
        options: ImCoreOpenOptions,
    ) -> ImResult<Self>;

    pub fn identities(&self) -> IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;
    pub fn onboarding(&self) -> SkillOnboardingService<'_>;
    pub fn handle_recovery(&self) -> HandleRecoveryService<'_>;

    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn did(&self) -> &Did;
    pub fn handle(&self) -> Option<&Handle>;
    pub async fn active_sync_account_binding(
        &self,
    ) -> ImResult<ActiveSyncAccountBinding>;

    pub fn auth(&self) -> AuthService<'_>;
    pub fn messages(&self) -> MessageService<'_>;
}
```

### 3.1 Skill Agent Token onboarding

`SkillOnboardingService` 是 environment-level API，因为 claim 从没有 current identity
的空 workspace 开始。调用方只能构造 `SkillOnboardingToken` 并将其交给一次完整操作；
该类型不可序列化且 `Debug` 始终脱敏。DID 私钥、User Service wire DTO、JWT 和问候
消息发送都由 im-core 内部持有。

```rust
pub struct SkillClaimRequest {
    pub token: SkillOnboardingToken,
    pub service_base_url: String,
    pub expected_controller_handle: String,
    pub expected_agent_handle: String,
}

pub struct SkillResumeRequest {
    pub service_base_url: String,
    pub expected_controller_handle: String,
    pub expected_agent_handle: String,
}

impl SkillOnboardingService<'_> {
    pub async fn claim_async(&self, request: SkillClaimRequest)
        -> ImResult<SkillClaimResult>;
    pub async fn recover_legacy_claim_async(&self, request: SkillClaimRequest)
        -> ImResult<SkillClaimResult>;
    pub async fn resume_async(&self, request: SkillResumeRequest)
        -> ImResult<SkillClaimResult>;
    pub fn claim(&self, request: SkillClaimRequest) -> ImResult<SkillClaimResult>;
    pub fn resume(&self, request: SkillResumeRequest) -> ImResult<SkillClaimResult>;
    pub fn recover_legacy_claim(&self, request: SkillClaimRequest)
        -> ImResult<SkillClaimResult>;
}
```

claim 只接受与 SDK 配置完全同源的 HTTPS 服务和已初始化、无可用 identity 的
workspace。相同 journal 可恢复同一 DID；其他非空或无法识别状态均返回
`skill_onboarding_workspace_conflict`。成功结果只包含 Agent DID/Handle、Controller
Handle、确定性 greeting message ID、phase/status 和稳定错误码，不含 Token、JWT 或
私钥。问候尚未被 Message Service 接受时返回 `greeting_pending + retryable=true`，
后续通过无 Token 的 `resume` 继续使用同一 DID 和 message ID，不重新注册。
新建与 legacy-recovery 两条 Skill exchange 路径都会显式声明
`capabilities=["group_membership_v1"]`；这只是 Agent 实现能力声明，User Service 的独立
部署开关和最终 admission 仍决定能否加入群聊，Core 不在本地绕过服务端策略。

`resume` 只接受严格匹配 service origin、Controller Handle、Agent Handle 的 schema-v2
journal，并且只继续 `device_prekey_pending` 或 `controller_greeting_pending`。它先验证本地
唯一 identity、DID document digest、device manifest 与 Handle service，再复用已提交的
exact device、稳定 PreKey material 和 greeting idempotency key；不会调用 `verify_token`
或 `exchange_token`。`completed` journal 幂等返回，`identity_pending`、缺失、损坏、版本或
scope 不匹配、identity 冲突均 fail closed。

PreKey 的 Service 错误只向公开结果与 journal 保留经 JSON-RPC 权威 sanitizer 验证的
有界公共 code；数值 JSON-RPC code 使用固定的 `skill_onboarding_prekey.rpc.*` 编码，
未知 code 折叠。远端 message、data 与未分类 detail 不持久化也不向调用方暴露。

新 claim 使用 v2 Agent/device identity、Device Access 和 PreKey 发布阶段。workspace 中
存在 v1 journal/pending material 时，普通 claim 返回
`skill_onboarding_legacy_claim_recovery_required`，调用方必须显式调用
`recover_legacy_claim_async`，不得删除旧状态、申请第二个 Token 或生成第二个 DID。
恢复会精确重放原 v1 exchange、持久化旧 identity、执行 same-DID Legacy upgrade，再从
已提交的 vNext DID document 重算 journal hash 并继续 PreKey/greeting。v1 journal 对应的
pending secret/file 缺失或孤立时返回稳定
`blocked_requires_operator_reconciliation`，不把裸 I/O 错误伪装成可重试注册。

### 3.2 Core open 前的 local-state 升级与恢复

release/0710 schema 27 必须在 `ImCore` 打开前通过独立入口升级；普通 open
只返回 `local_state_upgrade_required`，不能绕过 backup：

```rust
pub fn inspect_local_state_upgrade(
    paths: &LocalStatePaths,
) -> ImResult<LocalStateUpgradeInspection>;

pub fn upgrade_local_state(
    paths: &LocalStatePaths,
) -> ImResult<LocalStateUpgradeResult>;

pub fn restore_local_state_backup(
    paths: &LocalStatePaths,
) -> ImResult<LocalStateRestoreResult>;
```

upgrade 只接受显式白名单中的真实 release/0710 schema fingerprint，使用完整
SQLite backup、shadow migration 和守恒校验后 cutover。restore 只接受已完成
cutover 的 journal，将当前 target 保留为 private safety copy 后恢复整个 schema 27
backup。公共结果只包含 schema、聚合计数、alias mapping 和 backup/safety-copy
availability，不返回 backup 路径、消息内容或凭证。

当前 target 为 schema 35。pre-open canonical runner 只拥有 schema 27；已经完成
canonical cutover 的 schema 28 到 35 必须返回 `not_required`，随后由普通 Core open
推进或校验。普通 open 严格识别 release/0714 schema 28-31 以及两条开发线曾产生的
v32-v34 合法形态，并在单一事务中收敛为 hydration projection、subject-scoped checkpoint、
可证明的旧 Direct WireIdentity 修复、v2 account/message sync、read recovery，以及
未解析消息与 remote-thread binding 的 durable association。schema 35 的 association
使 Persona replay 能原子写入 canonical message/binding，不能把暂定 DID conversation
写成 durable binding。
未知、残缺或混合得无法证明的同号形态必须 fail closed，不能被猜测性迁移或静默删除。

P2+ API：

```rust
impl ImClient {
    pub fn identity(&self) -> IdentityService<'_>;      // P2+
    pub fn directory(&self) -> DirectoryService<'_>;    // P2+
    pub fn groups(&self) -> GroupService<'_>;           // P3+
    pub fn attachments(&self) -> AttachmentService<'_>; // P4+
    pub fn realtime(&self) -> RealtimeService<'_>;      // P5+
    pub fn secure(&self) -> SecureService<'_>;           // P6+
    pub fn email(&self) -> EmailService<'_>;             // Email
}
```

`ClientIdentityRuntime`、`ActorContext`、`LoadedIdentity`、`IdentityRuntimePaths` 都是 `pub(crate)`。

Email / Mail 不属于 Phase 1 IM MVP，但独立 Email 阶段已定义并打开默认命令面。接口形态见 `docs/api/im-core-interface/08-email-interface.md`。CLI `mail.*` 默认通过 `client.email()` 执行，不回退到 legacy mail implementation；CLI 仍负责 dry-run、输出 envelope 和附件文件写入。

## 4. 错误类型

```rust
pub type ImResult<T> = Result<T, ImError>;

pub enum ImError {
    InvalidInput { field: Option<String>, message: String },
    IdentityRequired,
    IdentityNotFound { selector: String },
    DefaultIdentityMissing,
    AuthRequired,
    SessionExpired,
    PermissionDenied,
    PeerNotFound,
    GroupNotFound,
    MessageNotFound,
    ContactNotFound,
    TransportUnavailable { detail: String },
    UnsupportedCapability { capability: String },
    LocalStateUnavailable { detail: String },
    PathUnavailable { path_kind: String, detail: String },
    CredentialFileUnreadable { path_kind: String, detail: String },
    Service { status_code: Option<u16>, code: Option<String>, message: String },
    Internal { message: String },
}
```

CLI 负责把 `ImError` 映射成 exit code、human hint、pretty/json/table 输出。`ImError` 不包含 CLI exit code。
JSON-RPC 收到 HTTP 2xx 但响应体为零字节时返回固定、无响应内容 detail 的
`TransportUnavailable`；非空畸形 JSON 仍返回 `Serialization`。HTTP 状态、响应体或其片段
不得被拼入该空响应诊断。

## 5. 基础类型

```rust
pub struct IdentityId(String);
pub struct Did(String);
pub struct Handle(String);
pub struct PeerRef(String);
pub struct GroupRef(String);
pub struct MessageId(String);
pub struct ThreadId(String);
pub struct Cursor(String);

pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
    pub has_more: bool,
}

pub struct PageLimit(pub u32);
```

建议不要把所有业务字段都用裸 `String` 暴露。第一阶段内部仍可转换成字符串，但 SDK DTO 应逐步领域化。

## 6. identity

P1 API：

```rust
pub enum IdentitySelector {
    Default,
    Id(IdentityId),
    Did(Did),
    Handle(Handle),
    LocalAlias(String), // CLI credential name / local account alias
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

pub struct IdentityReadiness {
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<String>,
}

pub enum IdentityDeviceMode { Legacy, VNext }
pub enum IdentityDeviceRole { Member, Admin }
pub enum IdentityDeviceAuthorizationStatus { Active, Revoked }
pub enum AgentIdentityKind { Skill, Daemon, Runtime }
pub enum IdentityDeviceReadiness {
    Legacy,
    MemberReady,
    AdminAwaitingRoot,
    AdminReady,
    Blocked,
}

pub struct IdentityDeviceSummary {
    pub identity: IdentitySummary,
    pub mode: IdentityDeviceMode,
    pub protocol_device_id: Option<ProtocolDeviceId>,
    pub role: Option<IdentityDeviceRole>,
    pub signing_key_id: Option<String>,
    pub e2ee_key_id: Option<String>,
    pub readiness: IdentityDeviceReadiness,
    pub blocked_reason: Option<String>,
}

pub struct ActiveSyncAccountBinding {
    pub owner_identity_id: String,
    pub account_id: String,
    pub current_did: String,
    pub protocol_device_id: String,
    pub identity_generation: String,
    pub device_auth_generation: String,
}

// Secret-bearing trusted-host DTOs. Neither type implements Serde and both
// redact documents, private keys, and tokens from Debug.
pub struct VNextAgentBootstrapMaterial { /* typed Agent/device/key material */ }
pub struct HostBackedDeviceIdentityMaterial { /* exact account/device access */ }
pub trait HostBackedAuthTokenPersistence: Send + Sync {
    fn persist_auth_token(&self, token: &str) -> ImResult<()>;
}

impl ImCore {
    pub fn generate_vnext_agent_bootstrap(
        &self,
        kind: AgentIdentityKind,
        handle_local_part: &str,
    ) -> ImResult<VNextAgentBootstrapMaterial>;

    pub fn prepare_vnext_agent_legacy_upgrade(
        &self,
        kind: AgentIdentityKind,
        handle_local_part: &str,
        legacy_did_document: serde_json::Value,
        root_private_key_pem: String,
    ) -> ImResult<VNextAgentBootstrapMaterial>;

    pub fn client_with_device_identity_material(
        &self,
        material: HostBackedDeviceIdentityMaterial,
    ) -> ImResult<ImClient>;

    pub fn client_with_device_identity_material_and_auth_persistence(
        &self,
        material: HostBackedDeviceIdentityMaterial,
        persistence: Arc<dyn HostBackedAuthTokenPersistence>,
    ) -> ImResult<ImClient>;
}

pub struct IdentityRegistry<'a> {
    core: &'a ImCore,
}

impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;
    pub fn delete_local_identity(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<DeleteLocalIdentityResult>;
    pub fn delete_local_identity_data(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<DeleteLocalIdentityResult>;
    pub fn legacy_upgrade_status(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<LegacyUpgradeStatus>;
    pub async fn upgrade_legacy_identity_async(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<LegacyUpgradeStatus>;
    pub fn device_summary(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentityDeviceSummary>;
    pub fn custody_status(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentityCustodyStatus>;
    pub fn inspect_identity_custody_migration(
        &self,
    ) -> ImResult<IdentityCustodyMigrationReport>;
    pub fn migrate_identity_custody(
        &self,
    ) -> ImResult<IdentityCustodyMigrationReport>;
    pub async fn authorize_daemon_subkey_async(
        &self,
        selector: IdentitySelector,
        proposal: DaemonSubkeyPublicProposal,
    ) -> ImResult<DaemonSubkeyPublicPackage>;

    // Deprecated AWiki-vault compatibility views. Use custody_status for
    // identity custody. The migration compatibility name now performs the
    // real old-to-ANP Identity migration.
    pub fn vault_status(&self, selector: IdentitySelector) -> ImResult<IdentityVaultStatus>;
    pub fn migrate_identity_vault(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentityVaultMigrationReport>;
    pub fn verify_identity_vault(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<IdentityVaultVerificationReport>;

    pub fn register_handle(
        &self,
        request: RegisterHandleRequest,
    ) -> ImResult<IdentityRegistration>;
    pub async fn request_registration_otp_async(
        &self,
        request: RegisterHandleRequest,
    ) -> ImResult<RegistrationOtpChallenge>;

    pub fn plan_default_identity_change(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<DefaultIdentityChange>;
}

pub struct RegistrationOtpChallenge {
    pub retry_after_seconds: u32,
    pub retry_at: String, // RFC 3339 UTC
}

pub struct DeleteLocalIdentityResult {
    pub deleted: IdentitySummary,
    pub was_default: bool,
    pub next_default: Option<IdentitySummary>,
    pub warnings: Vec<String>,
}

pub enum LegacyUpgradeStatus {
    Idle,
    Running,
    RetryRequired { identity_id: String, code: String },
    Completed,
}
```

`IdentityCustodyStatus` is secret-free and independent from
`IdentitySecretStoragePolicy`. `backend` is one of `anp_identity`,
`legacy_file_compat`, or `legacy_vault`; `state` is one of `creating`, `active`,
`enrolling`, `revoked`, `legacy`, or `unavailable`. It may expose only opaque
store/identity IDs, readiness/root-capability/pending booleans, and closed
missing/warning lists. It never exposes private-key presence by KID, a JWT,
`SecretRef`, document checkpoint, root fingerprint, or provider root key.

`migrate_identity_vault` is retained only for source compatibility. For an
unmigrated identity it invokes the workspace copy → verify → atomic cutover →
cleanup migration; for an ANP-managed identity it returns `migrated=false` and
the explicit `already_migrated` warning. A blocked pre-cutover migration fails
without deleting legacy records.

`IdentityDeviceSummary` 是面向产品层的安全投影，只公开协议设备 ID、公开
key ID、角色和由本地密钥可用性与服务端授权共同计算出的 readiness。它不公开
Vault 引用、根私钥存在标志或 AWiki 域内的 `document_version`、
`document_hash`、`registry_version`、`auth_generation` checkpoint。

`IdentitySummary.device_id` 是兼容摘要字段，不是多设备密码运行时的授权来源。
P5/P6 需要精确设备端点时，Core 从持久化 identity index 的当前 active vNext
authorization 读取 `ProtocolDeviceId`；host 如需展示设备摘要，应调用
`IdentityRegistry::device_summary`，不得从缺失值推导 `default` 或 sibling 设备。

`ImClient::active_sync_account_binding()` 是消息同步唯一可用的账号绑定来源。
`account_id` 必须来自 identity index 的稳定 `user_id`，
`protocol_device_id` 必须来自当前 active vNext authorization；
`identity_generation` 来自注册结果或经过 full Handle/current DID 校验的权威 WNS
文档，缺失时异步补查并持久化。Legacy、hosted、缺失账号、缺失设备授权均
fail closed；网络不可用、权威数据不一致和本地绑定冲突保留各自 typed error，
不得回退到 `IdentitySummary.device_id`、DID、Vault context device id 或常量 `1`。
两个 generation 和后续 cursor 都以 canonical decimal string 暴露，以支持超过
`u64` 的值而不损失精度。

`generate_vnext_agent_bootstrap` 是 Skill/Daemon/Runtime 独立 Agent Account 的唯一
共享生成器，DID path 固定为
`agent/{skill|daemon|runtime}/{canonical-handle-local}`，并生成唯一随机 bootstrap
Device 和互相独立的 root/device-signing/device-E2EE keys。
`prepare_vnext_agent_legacy_upgrade` 只构造 same-DID 更新目标：旧 root 必须与旧文档
匹配，请求的 Agent kind 必须与 DID path 匹配，Handle service 必须唯一且属于同一
DID；函数保留 DID/root/Handle，不执行远端 `update_document`。

`HostBackedDeviceIdentityMaterial` 只供同进程受信 native host 在 SecretVault 解封后
立即构造。Core 会一起校验 canonical Manifest、唯一 matching device、key IDs 与
private/public binding、Agent 自己的 account、canonical generations、active/admin/ready
状态，以及 Device Access 的 DID/user/device/key/generation、精确 scopes、双 audience、
purpose 和时间结构。绑定完全匹配但已过期的 Token 仍可构造 client，并通过 auth status
暴露 `needs_refresh=true`；缺字段、主体或设备不匹配、错误 generation/audience/scope/role/
readiness 仍然 fail closed。第一版 root 是 mandatory；rootless member 需要未来单独 API。
校验成功后 Core 从正式 seed 派生 exact device 和六字段 binding；旧
`HostedIdentityMaterial` 仍然没有 binding。

原有 `client_with_device_identity_material` 保持兼容，只在当前 Core client 内更新刷新后的
Token。需要跨进程重启继续使用刷新结果的受信 host 应选择
`client_with_device_identity_material_and_auth_persistence`：Core 先对新 Token 做完整 exact
binding 校验，再调用 host persistence；回调必须原子替换同一 DID/device/key/generation 的
SecretVault Token，不得记录 Token，也不得顺带改写密钥、身份或同步状态。host 持久化失败时
本次刷新失败，Core 不把仅存在内存中的 Token 表述为已提交。

这里的本地 Device Access 校验只解码并核对 claims/时效，不验证 JWT 签名；该 API
因此只允许受信同进程 host 使用。Token 的密码学权威性来自 User/Message Service
验签，运行期 fencing 来自 Message Service 对 live Device Registry 的重检。Host
不得把本地构造成功表述为 JWT cryptographic verification。

Realtime 子协议严格性也从该 exact binding 自动派生。exact vNext 必须收到
`awiki.sync.event.v3` 或 `awiki.sync.changed.v2` 回显：Core 按该顺序同时 offer，服务端选择
v3 时可发送内联 `message.created`，也可对超限/不可内联事件发送 schema 2 hint；选择 v2 时
只能接收 schema 2。`NoSubProtocol` 或缺失回显直接返回 transport error，不能无子协议重连；
只有无 exact binding 的 Legacy/Hosted client 可走兼容 fallback。

V3 是可协商的兼容新增，不替换 v2，也不改变任何 public DTO。内联正文只提前提交既有
message/thread projection：WS 不写消费回执、不推进 Sync V2 cursor；跨 epoch、未知 Group 或
Direct Persona 未验证时只保留 dirty/gap hint 并调度 delta。可靠 delta 随后按 `event_id`
提交回执和 cursor，并跳过已由 WS 提交的正文。因此本次无需 public API、协议替换或本地数据
schema 版本提升。

普通消息 v2 同步对每个合法 binding 默认生效，不存在 public API 级账号/设备 allowlist 或
百分比灰度。host 只可通过全局配置做应急回滚；该配置不改变 cursor、replica 或 Event
Envelope 语义，也不影响独立的 Direct/Group E2EE rollout gate。

`delete_local_identity` 是纯本地、crash-safe 的身份退役事务。registry/default
pointer tombstone 是目录与 Vault 清理之前的权威状态；Core open 会恢复中断阶段，并
通过永久 identity-ID tombstone 清理由并发尾部任务晚写的 identity-scoped Vault
records。Host 的 realtime stop、runtime dispose 和任何网络 logout 都不属于该调用的
成功条件。消息投影的 stable account binding 会保留；仅当本地 registry 已无相关身份，且
唯一 binding 与 completed retirement marker 的 identity ID、DID、`protocol_device_id`
精确闭合时，后续同 Handle 注册才把它视为“无 live 本地凭证”并返回 ordinary
`join_required`。缺失、未完成、冲突或部分匹配仍失败关闭为
`handle_recovery.transition_missing`。

`delete_local_identity_data` 只用于用户明确确认的“退出并删除当前数据”。它先按 stable
identity ID 删除 Core SQLite 中该身份拥有的消息、会话、群、智能体投影、同步状态与密钥；
对仅有 `owner_did` 的旧表，同时清理 identity DID history 中的当前及历史 DID，随后复用
`delete_local_identity` 的 crash-safe 本地身份退役。其他本地身份和远端账号不受影响。
该 API 不修改 ANP 协议，也不引入数据库 schema migration。

Legacy upgrade 是 Core 内部可恢复事务；host 必须等待
`upgrade_legacy_identity_async` 的 typed status，不得用更短的通用 UI request timeout
包裹它。`RetryRequired.code` 只返回
`transport_unavailable|service_error|permission_denied|auth_required|local_state_unavailable|legacy_upgrade_failed`
之一，既不泄露响应正文，也不会把 native future 仍在执行误判为已取消。

正式兼容基线是 AWiki Me `0.1.5+14` / im-core `d7c853a...`。升级保留同一 DID、
root key、Handle、local identity ID、Vault 历史和本地业务数据，但不会复制 Legacy
managed fields：Core 通过 ANP builder 重新生成唯一的 vNext managed fields，只保留合法的
authentication-only daemon 委托，并用真实 root key 算法生成新的
`assertionMethod` proof。Host 不得在 Dart 层读取或重写 DID 文档、私钥或 SQLite 来参与迁移。
pending 记录固定保存同一组 device ID/keys 和目标文档；重试时远端若已是该精确目标则直接收敛，
若明确仍是 Legacy 才允许保留原 device keys 刷新 root proof，任何其他 Manifest 或无法确认的远端
状态都失败关闭。

`request_registration_otp_async` 是 phone 注册的第一阶段，只接受 OTP 为空的
`VerificationInput::Phone`，并返回 User Service 给出的正数重试秒数和 RFC 3339 UTC
`retry_at`。它不把 token、pending checkpoint 或原始 service response 暴露给 host；第二阶段
仍用同一 `RegisterHandleRequest` 填入 OTP 调用 `register_handle_async`。

`register_handle` 是唯一完成注册的入口。新注册生成带 bootstrap Manifest 的 DID 和独立设备
keys，并通过同一个 `register` RPC 原子创建远端状态；无 Manifest 的旧客户端仍走 Legacy
兼容。Handle 已存在且已经是完整 Manifest 时返回 typed `join_required`，不创建第二个身份，
Core 将账号验证 token 和可选 Recovery transition 保存在短生命周期、进程内的 opaque
preparation 中；host 只能读取 preparation ID、typed mode、user-presence 要求、预期 DID 和
完整 Handle，并通过 `begin_prepared_registration_device_join` 进入 Device Join。若服务端确认该 Handle
仍是 Legacy 且本次 phone factor 与原绑定完全一致，`register` 可以作为窄范围兼容路径把它
原子恢复为新的 canonical vNext DID，同时保留原 `user_id`、Handle 和递增后的 binding
generation；这不是 Manifest Recovery，也不能替换已有 Manifest 身份。`registered` wire
响应中的 `message` 是必填但非权威的诊断文本，Core 不解析其文案来区分首次注册与 Legacy
恢复；远端提交只由 exact DID/Handle/domain/binding generation 和 exact-device access token
共同确认。新注册本地提交后立即尝试发布 exact-device P5 PreKey Bundle；启用 Group E2EE v2
时，还会为 bootstrap device 发布确定性、可重放的 P6 KeyPackage family，使该新身份可以被其他
用户加入加密群。远端和本地身份提交完成后，发布失败不再反向改写为注册失败，而是在
`registered` 结果中返回稳定的 `registration_prekey_publish_pending` 或
`registration_group_key_package_publish_pending` warning。注册 pending 在身份提交边界结束，
清理失败使用 `registration_pending_cleanup_required` warning。公共 DTO 不暴露
私钥、pending、内部 checkpoint 或 refresh token。`HandleRegistrationResult.account_id` 仅在
`registered` 结果中返回服务端 canonical `user_id`；`join_required` 不伪造账号 ID，也不暴露
account verification token 或 owner 选择。Core 在 prepared begin 时重新验证稳定 owner；
Recovery rebind 必须在远端 Join create 之前持久化 joined-device marker。进程重启后 preparation
失效，host 重新发起注册验证，不提供兼容恢复或独立 JSON continuation。已完成本地身份退役
但保留消息 binding 的同 Handle 仍通过 ordinary `join_required` 重新进入显式 Join，而不是
因该历史 binding 返回 `handle_recovery.transition_missing`。
注册写入发生传输不确定性时，Core 先用同一 pending DID 对账；所有 JSON-RPC HTTP 状态都
保留服务端 `code/data`。只有 User Service 返回精确
`error.data.awiki_code=did_auth.active_did_not_found`（或既有明确 not-found 契约）时，
Core 才判定远端未提交并重试同一份注册材料；不得解析错误文案或把所有未认证错误当成缺失。

### 5.1 Device Join host facade

Device Join is an AWiki-local control-plane API and has no host-local rollout
gate. The host facade provides new-device begin/poll/cancel plus
management-device Registry, local notification-driven request listing,
start-verification, reject, and approval operations through
`core.device_join()`. Management devices do not poll Join status and do not
have an admin-side cancel API.

`local_device_join_verification_progress(admin_identity, join_session_id)` 是
ResponseVerified/ApprovalPrepared 阶段的纯本地短期读取入口。它只从已验证的 admin session
与 Vault 读取 SAS，不发 RPC、不写 System Notification projection，也不推进 Join state。SAS
不进入 `DeviceJoinRequestNotice`、realtime event、CLI JSON 或 durable notice。

新设备侧 `poll_new_device_join` 在远端保持 `response_verified` 且本地仍为
`ResponsePrepared` 时，会从 restart-safe transcript 与 Vault pairing secret 重新派生同一
SAS。该恢复语义不持久化 SAS，也不把它放入远端状态；它保证首次响应提交后即使 host
重启或先执行了无 UI 的状态轮询，下一次前台调用仍可显示并比较 SAS。

`DeviceJoinAccountVerificationGrant` is a write-only input consumed by
`begin_new_device_join`; it is not serializable and its `Debug` output is
redacted. Approval is intentionally split: after the host confirms the
independently derived SAS, `prepare_device_join_approval` returns a short-lived
process-local handle; after real local user presence,
`confirm_device_join_approval` consumes it. Preparing another handle for the
same session/admin invalidates the previous unused handle; an in-flight
confirmation cannot be replaced.

`DeviceJoinSessionView`、progress 和 pending summaries 是安全投影，不包含
account/Join token、pairing secret/private key、root material、challenge/ciphertext
细节、`document_version`、`document_hash`、`registry_version` 或
`auth_generation`。

显式 Registry 读取是唯一例外：`DeviceJoinRegistrySnapshot.registry_version` 和
`DeviceRegistryAuthorizedDeviceSummary.auth_generation` 以 canonical decimal
string 暴露，用于 App 的 display-only account-state cache 做单调版本替换。它们不是
跨域 ANP 字段，也不能授权 Join、revoke 或 root transfer；这些安全动作必须继续通过
Core fresh Registry 校验。User Service 当前的权威 Registry 实现以 `u64` 维护这两个值，
但 Rust→Dart→Flutter 边界必须先转换为十进制 `String`，不能让 Dart/JavaScript 数值表示参与
传输或比较。Registry snapshot 仍不暴露 document version/hash。

```rust
pub struct DeviceJoinRegistrySnapshot {
    pub did: Did,
    pub registry_version: String,
    pub devices: Vec<DeviceRegistryAuthorizedDeviceSummary>,
}

pub struct DeviceRegistryAuthorizedDeviceSummary {
    pub protocol_device_id: ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DeviceJoinAuthorizationStatus,
    pub role: DeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
    pub auth_generation: String,
}
```

Host 观察到服务端 `consumed` 并不代表本地已经可用。Core 会验证最终 DID
Document/Manifest，使用候选设备 signing key 发起新的 DID-WBA `get_me` 请求，并从标准
`Authentication-Info` 或 `Authorization: Bearer` 响应头取得 access token。只有 exact
device principal、rootless vNext 身份和 checkpoint 原子落盘后，session 才变为
`Authorized`。V1 没有 `device_token_issue` 或设备 refresh token。最终 Document 中规范的
`JsonWebKey2020` OKP Ed25519/X25519 方法由 hosted device auth、Join 实时验证、
Vault restart-safe activation、Root 流程和 P5 发布共用同一个解析边界，不能由各调用方
或通用 verification-method decoder 另行解释。

### 5.2 Management-device root-key transfer

Root transfer is an identity-scoped `ImClient::root_key_transfer()` capability
with no rollout gate. The host first calls `prepare` with only the exact
recipient `ProtocolDeviceId`. Core verifies the current ready Admin, the active
Member/not-ready recipient, Manifest/Registry bindings, ANP Identity root
capability and P5 Session or PreKey readiness. It returns an opaque 60-second, single-use
authorization handle plus a secret-free recipient summary. The host then calls
`confirm_and_send` once with that handle and local user presence; Core, not the
host, generates the message ID. A false confirmation fails before ANP Identity
exports any root bytes. A true confirmation authorizes exactly that one handle,
recipient and Core-generated message ID.

The sender always emits the legacy `RootKeyEnvelopeV1`; it does not negotiate or
send the wrapped-root format. ANP Identity exposes the active managed root key
to native Core as a zeroizing PKCS#8 DER value only after confirmation, and
Core immediately constructs the secret JSON carried by a standard P5 v2 Init
or Cipher.
There is no private endpoint, delivery class, sidecar, empty-Init handshake,
imported ACK, public list/retry state, or host-supplied message ID. Core commits
the standard P5 pending state and its secret-free sender delivery ledger in the
same SQLite transaction, and an uncertain transport response is retried only
with the identical P5 bytes and message ID. Startup and an explicit later
prepare first recover any `pending_delivery` by resuming those durable bytes;
P5 acceptance and the sender `sent` fact commit atomically. Core never re-exports
the root or creates a replacement message during recovery, and it rejects
a new transfer while that recipient already has a pending or sent fact.

On the recipient, authenticated Mailbox delivery supplies the exact accepted
tuple and timestamp. Core validates the outer P5 binding, Registry/Manifest,
RootEnvelope, fingerprint and current checkpoint before importing the root as a
pending ANP Identity capability. It then sends the closed, double-proof
`device_root_import_complete` request. The exact canonical params, proof and
nonce are reused after response loss. A fresh DID-WBA `get_me` may return only
the original Member principal or the next-generation ready Admin principal:
Member retries the exact completion; Admin skips the business replay and does
one exact self Registry confirmation. Only after Registry confirmation does
Core promote the pending root and local identity projection atomically, then
persist the new Admin access token. Root envelopes are never projected
as ordinary messages or public DTOs. A realtime Root candidate is only a hint:
Core hydrates the exact authenticated Inbox row and never substitutes local
arrival time for the service-provided `accepted_at`. Startup recovery replays
`registry_confirmed` and `promoted` coordinators to repair local promotion and
projection cleanup crash windows.

### 5.3 Multi-device P5/P6 message rollout gates

`ImCoreOpenOptions.multi_device_direct_e2ee_enabled` 与
`multi_device_group_e2ee_enabled` 是彼此独立的 host-local rollout gate，均默认
`false`，不会序列化到 ANP、DID Document 或跨域请求。gate 关闭时保持原有消息路径；
开启 P5 gate 只会为本地 vNext 身份选择 exact-device P5 v2 Direct 产品路径，开启
P6 gate 只会选择 device-scoped P6 v2 Group 产品路径。

`ImCoreOpenOptions.multi_device_group_e2ee_enabled` is host-local configuration,
defaults to `false`, and is never serialized into ANP, DID Documents, or
cross-domain requests. When enabled, the redacted `secure().group()` status and
repair facade uses the device-scoped P6 v2 local state. The facade exposes only
readiness, repair state, and the `added_devices`, `removed_devices`, and
`remaining_devices` counts; it does not expose KeyPackages, Welcome or Commit
payloads, Leaf identifiers, MLS secrets, state paths, or raw SQLite rows.

Identity vault DTOs are redacted status/report surfaces. They report selected
backend, storage policy, vault availability, metadata verification, workspace /
device context, warnings, and plaintext compatibility retention, but they must
not expose root keys, private PEM, JWTs, bearer tokens, raw `SecretRef` JSON, or
ciphertext internals. `VaultRequired` is fail-closed for new secret persistence.

`verify_identity_vault` 的失败分支使用 `ImError::IdentityVault` 与稳定的
`IdentityVaultFailure`：`Unavailable`、`MetadataMissing`、`MetadataUnverified`、
`WorkspaceMismatch`、`DeviceMismatch`、`RecordOpenFailed` 和
`VerificationFailed`。Dart facade 将其映射为同义的稳定 snake-case error code；host
必须按 code 分支，不得解析 message。出于安全边界，错误 root key、密文损坏和 AEAD
authentication failure 均归一为 `RecordOpenFailed`。

P2+ API：

```rust
pub struct IdentityService<'a> {
    client: &'a ImClient,
}

impl IdentityService<'_> {
    pub fn profile(&self) -> ImResult<Profile>;
    pub fn update_profile(&self, patch: ProfilePatch) -> ImResult<Profile>;
    pub fn bind_contact(&self, request: BindContactRequest) -> ImResult<BindContactResult>;

    // 危险命令，晚于普通 IM 能力迁移。
    pub fn replace_did(&self, request: ReplaceDidRequest) -> ImResult<ReplaceDidResult>;
}
```

Manifest Handle Recovery V4.0 是 host-neutral、默认关闭的 Core 能力。Host 通过
`ImCoreOpenOptions.multi_device_handle_recovery_enabled` 显式开启后，使用
`ImCore::handle_recovery()` 的 typed 操作：`request_handle_recovery_otp`、
`prepare_handle_recovery`、`activate_handle_recovery`、`resume_handle_recovery`、
`issue_handle_recovery_attestation`、`handle_recovery_status`、`list_handle_recovery_operations`、
`discard_handle_recovery_pre_attempt`、`quarantine_handle_recovery_key_unavailable`、
`authorized_handle_recovery_receipt`、`activate_authorized_join` 和
`resume_authorized_join_activation`；metrics 另有只读快照。`status` 和 list 只读；
activate/resume 才能推进持久化状态机。
`issue_handle_recovery_attestation` 是 DSH Host-only 的短时对账边界：只有本机 operation 已为
`applied`、reset reference 来自同一 initiator operation，且 account、稳定 owner、完整 Handle、
previous/current DID、binding generation 与当前已安装 client 全部精确一致时，Core 才用 current
DID 的 authenticated RPC 获取不透明 token 与过期时间。token 由 zeroizing memory 承载，Debug
始终 redacted；Host 只可立即转交固定受众的 Model Proxy，不得持久化、记录日志、进入 Browser/
Agent schema 或模型上下文。joined-device reset、待续跑 operation、旧 client 与任何 epoch mismatch
全部 fail closed。
OTP、Recovery Grant、proof、私钥与 JWT 不进入公开进度 DTO 或 SQLite transition marker。
OTP request 接受规范化 full Handle、phone 与 optional identity：提供 selector 时必须与 Handle
精确闭合；省略时 Core 只按 Handle 查找本地身份，本机不存在则创建新的本地 owner，绝不回退到
default/current identity。Core 生成并返回 operation ID 与权威 owner ID；prepare、status、
activate 和 resume 都精确接受 operation ID，不按 identity scope 猜测，也不把 unknown 映射成
nullable success。
`HandleRecoveryProgress` 包含 secret-free impact 和 initiator reset projection；authorized Join
返回 `AuthorizedJoinActivationProgress { join, reset_reference }`，其中 joined-device reset 的
source ID 是精确普通 Join session ID，App 不需要也不得猜 epoch tuple。

V4.0 的公开进度阶段闭集是 `awaiting_factor`、`ready_to_commit`、
`remote_outcome_unknown`、`remote_committed`、`identity_transition_pending`、`applied` 和
`quarantined_key_unavailable`。公开 Recovery 错误码闭集是 `factor_retry_required`、
`result_absent`、`outcome_unknown`、`local_key_unavailable`、`local_transition_pending`、
`local_migration_unsupported` 和 `unknown_epoch`。V3 阶段名和 `handle_recovery_*` 兼容错误别名
均不存在。

本地已有目标时恢复保留稳定 `owner_identity_id` 和本地 alias；新机器则安装新的本地 owner，
`local_ordinary_data_will_migrate=false`，且不读取或覆盖其他本地身份。切换后用新设备签名刷新 JWT、发布新的 P5
PreKey，并只为 authoritative `required_security_profile=transport-protected` 的 Handle-backed
群创建 P4 rebind。缺失、未知、冲突、DID-only、group-e2ee 均 fail closed；Recovery 任务绝不
创建 P6/MLS 或 `awaiting_p6`。旧 Ratchet、PreKey/OPK、MLS 和 device-scoped checkpoint 被退役，
已有目标 owner 的普通业务历史仍保留；fresh owner 的新 sync replica 仍按 `tail_only` 启动，
不会自动取得 Recovery 前的 Direct 历史。让 fresh Recovery replica 获得旧 Direct snapshot
需要新增可审计的 sync bootstrap 恢复授权，V4.0 不通过本地猜测绕过该协议边界。
`identity_transition_pending` 在本地 epoch reset 前持久化，并按 initiator
operation ID 或 joined-device Join session ID 绑定来源。
远端 Commit 成功后，新身份 JWT 刷新或 P5 PreKey 发布遇到可续跑的 transport/auth/session/
service/serialization 失败时，Core 持久化稳定 `local_transition_pending`，并要求 Host 对同一
operation ID 调用 resume；不生成新恢复任务，也不回退远端 Commit。续跑收敛为 `applied`
后会清理该 operation 上的旧可重试错误投影。权限、Vault、本地不变式或持久化破坏仍保持原错误并
fail closed，不得被伪装成可重试连接故障。
Recovery Commit 收到 HTTP 2xx 零字节响应时按传输结果不确定处理：activate 返回稳定
`outcome_unknown`，pending phase 保持 `remote_outcome_unknown`；进程重开后的 resume 先用
Vault 中持久化的 bootstrap key 对 `handle_recovery_result_get_v4` 签名。已提交结果继续本地
transition；`result_absent` 才允许同一冻结 intent 重试 Commit，不生成新 operation ID 或身份材料。
pre-attempt discard 必须先在 SQLite operation index 中原子占有
`pre_commit && commit_attempted=false`，再幂等删除 Vault key。post-attempt 刷新 Grant 时如果
fresh binding 已变化，Core 会再次 Result Get；只有仍为 `result_absent` 才将旧 operation 标为
state-change loser，防止把刚刚完成的延迟 Commit 错判为 superseded。

V4.0 不增加 CLI command、Daemon task、Agent 恢复入口或 process-global identity。未来这些 host
可复用同一 typed service 和显式 `IdentitySelector`，不得绕过 Core 状态机。App 迁移旧
device-registry epoch 时只能采用 `IdentityRegistry::legacy_registry_epoch_adoption_authority`
返回的精确、marker-free、opaque authority；任意 Recovery marker phase 都会使该 authority
fail closed。V4.0 支持无目标本地身份的新机器 fresh install，但不实现已有 owner 的透明 N-k
本地历史认领；后者保留给后续 V4.1。

`plan_default_identity_change` 返回计划，CLI/App 负责是否写入 default identity 文件。若未来 SDK 需要直接写入，必须只写显式传入的 `default_identity_path`。

## 7. auth

P1 API：

```rust
pub struct AuthService<'a> {
    client: &'a ImClient,
}

pub enum AuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

impl AuthService<'_> {
    pub fn login(&self) -> ImResult<SessionBundle>;
    pub fn ensure_session(&self, scope: AuthScope) -> ImResult<SessionBundle>;
    pub fn refresh_session(&self) -> ImResult<SessionUpdate>;
    pub fn status(&self) -> ImResult<AuthStatus>;
}
```

P2+ API：

```rust
impl AuthService<'_> {
    pub fn logout(&self) -> ImResult<SessionUpdate>;
}
```

DID auth request、JWT 文件格式、session metadata path 都是内部实现。
`SessionBundle` / `SessionUpdate` may return bearer tokens for existing auth
flows, so callers must treat those values as sensitive and avoid logging or
persisting them. CLI/App 不应该直接从本地文件 / private state 读取 bearer
token，也不应该把返回的 token 再保存到外部 credential/session store，除非
Phase 7 明确引入该边界。

### 7.1 Host-owned external HTTP transport authentication

`external_http_auth()` allows a trusted host to authenticate one exact HTTP
request while retaining ownership of the network transport. Core chooses an
origin-scoped in-memory Bearer token when available; otherwise it signs the
method, target URI, authority and optional body digest with the current device
request-signing key. The host cannot choose the key, nonce, algorithm or auth
mode.

```rust
pub const EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

impl ImClient {
    pub fn external_http_auth(&self) -> ExternalHttpAuthService<'_>;
}

impl ExternalHttpHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>)
        -> ImResult<Self>;
    pub fn name(&self) -> &str;
    pub fn value(&self) -> &str;
}

impl ExternalHttpRequest {
    pub fn new(
        url: impl Into<String>,
        method: impl Into<String>,
        headers: Vec<ExternalHttpHeader>,
        body: Option<Vec<u8>>,
    ) -> ImResult<Self>;
}

impl ExternalHttpResponse {
    pub fn new(status_code: u16, headers: Vec<ExternalHttpHeader>)
        -> ImResult<Self>;
}

impl ExternalHttpAuthService<'_> {
    pub fn prepare(&self, request: ExternalHttpRequest)
        -> ImResult<ExternalHttpAuthAttempt>;
    pub async fn prepare_async(&self, request: ExternalHttpRequest)
        -> ImResult<ExternalHttpAuthAttempt>;
    pub fn handle_response(
        &self,
        attempt: ExternalHttpAuthAttempt,
        response: ExternalHttpResponse,
    ) -> ImResult<ExternalHttpAuthDecision>;
    pub async fn handle_response_async(
        &self,
        attempt: ExternalHttpAuthAttempt,
        response: ExternalHttpResponse,
    ) -> ImResult<ExternalHttpAuthDecision>;
    pub fn clear_cached_tokens(&self) -> ImResult<()>;
}

impl ExternalHttpAuthAttempt {
    pub fn header_patch(&self) -> &[ExternalHttpHeader];
    pub fn target_url(&self) -> &str;
    pub fn method(&self) -> &str;
    pub fn retry_count(&self) -> u8;
}

pub enum ExternalHttpAuthDecision {
    Complete,
    Retry(ExternalHttpAuthAttempt),
}
```

The request must be an absolute HTTPS URL without credentials or a fragment.
Literal loopback HTTP is accepted only when the host explicitly enables
`external_http_allow_insecure_loopback_for_testing`. Request methods are
canonical uppercase tokens. Duplicate header names and caller-supplied
`Authorization`, `Signature-Input`, `Signature` or `Content-Digest` fail before
network I/O. `body: Some(Vec::new())` means an explicitly empty body and is
still digest-bound; `None` means no body. Bodies above 4 MiB are rejected.

`ExternalHttpAuthAttempt` is opaque, single-use and not cloneable. Its header
patch is sensitive because it contains either an HTTP signature or Bearer
token. A `401` can return only one retry attempt; a response for that retry can
never request a third transport call. Only a `2xx` response
`Authentication-Info` field can update the process-local token cache. Response
`Authorization` is ignored. Cache keys bind the current owner identity, DID,
request-signing key and normalized origin; a stale Bearer `401` uses
fingerprint compare-and-clear so it cannot delete a concurrently replaced
token. The cache is not persisted and disappears when the client lifecycle is
released or explicitly cleared.

The fixed verifier `Accept-Signature` may advertise `content-digest` for every
challenge. Core treats that component as compatible even when the original
request has no body, while the actual GET/HEAD/no-body retry signature still
omits `Content-Digest`. A combined `WWW-Authenticate` value may contain other
schemes before or after DID-WBA; Core selects exactly one well-formed DID-WBA
challenge and ignores unrelated schemes. A recognized non-terminal Bearer
challenge compare-and-clears the matching stale token before a later
`Accept-Signature` incompatibility can decline the retry.

Core does not send the request or read its response body. The language/host
facade must send the canonical `attempt.target_url()` and `attempt.method()`,
apply the returned patch to the exact original headers and body bytes, keep
redirects manual, submit only response status and headers, and avoid logging
the patch. Production hosts must not expose this service through browser RPC,
model tools or an untrusted signing endpoint.

## 8. messages

P1 API：

```rust
pub struct MessageService<'a> {
    client: &'a ImClient,
}

pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<MessageId>,
    pub delivery: MessageDeliveryOptions,
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

pub enum MessageTarget {
    Direct(PeerRef),
    Group(GroupRef),
}

pub enum ThreadRef {
    Direct(PeerRef),
    Group(GroupRef),
    Thread(ThreadId), // P3+
}

pub enum MessageBody {
    Text {
        text: String,
        kind: MessageKind,
    },
    Payload {
        payload: serde_json::Value,
    },

    Attachment {
        input: AttachmentInput,
        caption: Option<String>,
        mention_payload: Option<serde_json::Value>,
        mime_type: Option<String>,
        filename: Option<String>,
    },
}

pub enum MessageKind {
    Text,
    Markdown,
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    E2eeRequired,
    SecureDirect,
    GroupE2ee,
}

pub struct MessageDeliveryOptions {
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

impl Default for MessageDeliveryOptions {
    fn default() -> Self {
        Self {
            idempotency_key: None,
            wait_for_final_acceptance: false,
        }
    }
}

impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
    pub fn send_conversation_text(
        &self,
        request: SendConversationTextRequest,
    ) -> ImResult<SendMessageResult>;
    pub fn send_conversation_payload(
        &self,
        request: SendConversationPayloadRequest,
    ) -> ImResult<SendMessageResult>;
    pub fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
    pub async fn local_inbox_projection_with_metadata_async(
        &self,
        query: InboxQuery,
    ) -> ImResult<MessagePage>;
    #[doc(hidden)]
    pub async fn hydrate_exact_device_secure_inbox_async(
        &self,
        limit: PageLimit,
    ) -> ImResult<Vec<String>>;
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
    pub fn conversation_history(
        &self,
        conversation: ConversationReadRef,
        query: HistoryQuery,
    ) -> ImResult<Page<Message>>;
    pub async fn conversation_history_async(
        &self,
        conversation: ConversationReadRef,
        query: HistoryQuery,
    ) -> ImResult<Page<Message>>;
    pub fn local_history(
        &self,
        thread: ThreadRef,
        query: LocalHistoryQuery,
    ) -> ImResult<Page<Message>>;
    pub fn sync_delta(&self, request: SyncDeltaRequest) -> ImResult<SyncDeltaResult>;
    pub fn sync_now(&self, request: MessageSyncRequest) -> ImResult<MessageSyncOutcome>;
    pub fn sync_diagnostics(&self) -> ImResult<MessageSyncDiagnostics>;
    pub fn sync_thread_after(
        &self,
        request: SyncThreadAfterRequest,
    ) -> ImResult<SyncThreadAfterResult>;
}
```

`conversation_history(_async)` 只接受 directory、conversation list 或
`ensure_conversation` 已确认的 canonical `conversation_id`。Direct peer-scope ID 由 Core
解析到当前 DID/Handle route，Group ID 解析到 group route；host 不得用展示字段重建
`ThreadRef`。该约束与 `send_conversation_*`、`mark_conversation_read` 相同。

P3+ API：

```rust
impl MessageService<'_> {
    pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
    pub fn mark_thread_read(
        &self,
        request: MarkThreadReadRequest,
    ) -> ImResult<MarkThreadReadResult>;
    pub fn mark_conversation_read(
        &self,
        request: MarkConversationReadRequest,
    ) -> ImResult<MarkThreadReadResult>;
    pub fn local_conversation_timeline(
        &self,
        conversation: ConversationReadRef,
        query: LocalHistoryQuery,
    ) -> ImResult<Page<Message>>;
    pub fn sync_conversation_after(
        &self,
        request: SyncConversationAfterRequest,
    ) -> ImResult<SyncThreadAfterResult>;
    pub fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
    pub fn load_conversation_snapshot(&self) -> ImResult<Option<ConversationListSnapshot>>;
    pub fn clear_conversation_snapshot(&self) -> ImResult<()>;
    pub fn watch_conversation_patches(&self) -> ImResult<ConversationPatchSession>;
    pub fn repair_conversation_store(&self) -> ImResult<ConversationStorePatch>;
    pub fn watch_thread_patches(
        &self,
        thread: ThreadRef,
        limit: Option<u32>,
    ) -> ImResult<ThreadMessagePatchSession>;
    pub fn repair_thread_store(
        &self,
        thread: ThreadRef,
        limit: Option<u32>,
    ) -> ImResult<ThreadMessageStorePatch>;
    pub fn watch_conversation_timeline_patches(
        &self,
        conversation: ConversationReadRef,
        limit: Option<u32>,
    ) -> ImResult<ThreadMessagePatchSession>;
    pub fn repair_conversation_timeline_store(
        &self,
        conversation: ConversationReadRef,
        limit: Option<u32>,
    ) -> ImResult<ThreadMessageStorePatch>;
    pub async fn local_hydrated_incoming_recovery_async(
        &self,
        query: IncomingMessageRecoveryQuery,
    ) -> ImResult<IncomingMessageRecoveryPage>;
}
```

Reliable sync 补充：

- `local_hydrated_incoming_recovery_async` 是 Daemon crash compensation 的 Rust-only
  本地读取边界。它只接受 `limit` 和 Core 签发的 opaque typed page token；owner、account、
  DID、device 和 generation 全部从 exact active binding 派生，Legacy/Hosted fail closed。
  page 只含 hydrated incoming typed `Message`，按稳定 oldest-first keyset 顺序返回
  `items/next_page_token/has_more`，单页 `limit` 为 `1..=1000`。Token 对外字段私有，绑定
  owner/account/device/generations，不能跨 identity 复用，也不是 Sync v2 cursor。Daemon
  必须设置每轮总扫描 hard cap，并在 `has_more` 时逐页推进；固定反复读取第一页会造成
  ledger 前缀饥饿。

- `hydrate_exact_device_secure_inbox_async` 是 Rust host 的迁移期 P5 Inbox 降级边界。
  Core 先按 active device authorization generation 完成 lane capability bootstrap；若服务端
  广播 `lanes.p5_device.v1`，该调用直接返回且不发送 `inbox.get` / `inbox.mark_read`，P5 改由
  `sync_now` 的同一条 `sync.delta` 处理。只有 capability 未广播时，才发送闭合
  `body.security_profile=direct-e2ee` 的本域 `inbox.get`，再次过滤非 P5 v2 candidate，并只 ACK
  已完成认证解密和 committed local projection 的 raw message ID。ACK 缺失、部分成功、无进展
  或达到 100 页上限均失败但不回滚已提交消息。该接口与服务端旧 Inbox RPC 均保留，不返回
  远端消息页、不接纳 delegated/Legacy 身份，也绝不能补齐普通消息。

- `sync_now(MessageSyncRequest { reason, limit })` 是 V2/V3 ordinary、P5 与 P6 的统一可靠主链路。
  account、device、cursor 均由 Core 内部从 active binding 和 SQLite 获取，public outcome
  只暴露高层状态、计数、changed conversation IDs、已提交 incoming message 和诊断。
  `sync.bootstrap` 除 ordinary binding/Group baseline/cursor 外，还可协商
  `lanes.p5_device.v1` / `lanes.p6_group.v1` 及其独立 cursor。已有 V2 本地状态只在升级或 device
  auth generation 改变后补一次 capability bootstrap；随后 ordinary/P5/P6 由同一条
  `sync.delta` 拉取，未协商 lane 时 request/response 保持旧 V2 形状。每页 ordinary receipt、
  exact-hydrated projection/cursor 均在单个 SQLite 事务提交；必需 hydration、schema、
  canonical identity 或 route 解析失败时整页回滚，cursor 与 receipt 不变。
  `message.get_batch` 服务端使用 16 MiB hard response budget；Core 固定按请求顺序每 8 个
  event ID 分批，为 compact JSON 封装与转义保留余量。任一批次出现 unavailable 时整页不应用。
  bootstrap 的 `client_instance_id` 是 Core 为每个本地 owner 在同步 SQLite 首次生成并先持久化
  的随机不透明值：请求丢失/失败重试和重启复用，清库/新 DB 自动变化，不能由 owner/account/
  device 稳定标识派生。
  P5 `committed_seq` 只有在既有 Direct E2EE 解密/ratchet/replay 状态与消息或 durable backlog
  均成功持久化后才推进；毒密文只停住 P5 lane，ordinary/P6 继续。P6 按
  `group_did + group_event_seq` 幂等，单群失败进入 per-group blocker 而聚合 cursor 继续推进。
  lane error 仅产生 lane warning/retry，不得升级为 `AuthRevoked`；ordinary lane 对 E2EE/MLS
  discriminator 的既有拒绝不变。
  `group.member_changed` / `group.profile_updated` 在同一事务中同时提交 Group 状态和一条已读的
  群系统时间线记录；记录 ID 固定为 `<group_did>:<group_event_seq>`，因此本地 mutation、
  realtime、v1 delta 和 v2 delta 乱序或重复到达都收敛为同一条记录。群系统记录不进入
  ordinary `committed_incoming_messages`。
- `sync_diagnostics()` 是只读、类型化且产品安全的诊断入口。结果只包含
  `last_success_at`、`mode`、`pending_mutation_count`、`dirty_domains`、
  `retry_state` 和可选 `next_retry_at`；不包含 raw cursor/epoch、完整 account/device ID、
  recovery token/anchor/hash、正文、payload 或 auth token。该调用不发网络请求，不推进
  checkpoint，也不触发 recovery。
- 每次成功 delta/snapshot commit 后，Core 可 best-effort 执行最多 256 条本地清理。receipt
  清理继续保留每 owner 最近至少 10,000 条并保护 recovery anchor；mutation/recovery
  terminal row 至少保留七天。pending/in-flight/retryable mutation 永不由该清理删除；
  清理失败不改变已经返回的同步成功语义。
- `sync_now` 在一次调用内闭合 compact recovery：
  delta（或 existing-device bootstrap）→ 只存在于 Rust 进程栈的不透明 token →
  snapshot 严格校验/原子 merge → post-anchor delta。成功只返回既有 `changed` / `idle`；
  snapshot 或 post-anchor delta 失败返回 `retryable_failure`，授权或 generation fence 返回
  `auth_revoked`；同步链路在认证刷新或服务调用中收到 HTTP 401/403，或 Core transport
  完成有界认证重试后仍收到 JSON-RPC `1401`，属于终止性 `auth_revoked`。在线 Registry
  校验返回 `anp.device_not_eligible` / `anp.device_state_changed` 时，Core 会先执行一次有界
  session 刷新、transport auth reload 和 active binding 重取，然后重试被拒的 delta 或
  read outbox；刷新失败或再次收到 Registry fence 才终止为 `auth_revoked`。没有第二个
  public recover API，raw token、
  cursor/anchor、cutoff、policy
  limit、snapshot 返回数量都不得进入 Rust public DTO、Dart/Flutter、CLI、App、SQLite 或日志。
  snapshot 合并当前 read/Group 状态和最近普通消息，但不得删除更早的本地消息；receipts、
  projections、cursor 和 recovery completion 在同一 SQLite 事务提交。Core 按 owner
  串行化 `sync_now`，并在事务内对 previous cursor、recovery-id hash、anchor 和 phase 做
  CAS；过期并发恢复不能回退 cursor。Snapshot response 必须是 closed schema，消息时间不得
  早于服务端 cutoff，event ID/seq 不得重复，read/Group timestamp 必须严格合法。
- `committed_incoming_messages` 只包含 post-snapshot/live delta 实际提交的 incoming messages；
  snapshot hydration 与 realtime hint 不进入该列表。
- 本地 read watermark 更新与 durable `read_state_mark_read` outbox enqueue 是同一 SQLite
  事务。尚未发送的条目按最大 watermark 合并；已 in-flight payload 不可修改，更高 watermark
  创建 successor；启动恢复把 stale in-flight 改为 retryable。服务响应或经过 account/device/
  auth-generation 验证的远端 read-state event 必须在同一事务内执行 MAX merge、更新 read
  projection 并 ack 已覆盖的 outbox。v2 retry 必须把同一个 outbox `operation_id` 放在
  ANP `meta.operation_id`；body 只保留 `user_did`、`thread { kind, thread_key }` 和 watermark
  字段，不发送 account/device/origin selector，也不重复发送 body `operation_id`。发送前
  必须 claim exact operation；同 aggregate 的 in-flight predecessor 会阻挡 successor。
  成功响应必须 closed-schema 回显相同 DID/thread，并满足
  `remote_acknowledged=true`、`pending_remote_ack=false`、`partial=false`、
  且 server watermark 不低于发送值，才可提交本地 ACK。`fallback_used` 只描述服务端
  采用的兼容路径，不削弱上述最终 ACK 条件。transport、
  parse、协议验证或本地 ACK transaction 失败都必须把 claim 退回 `retryable`，且 drain
  发生在最终 delta commit 之后，不能覆盖已提交的同步结果；损坏的本地 payload 进入
  `permanent_failure`。Registry fence 的 mutation 在上述单次 session/binding 刷新后立即
  重发。本行为调整不修改 `anp.read_state.local.v1` wire schema 或 public DTO，不升级协议版本。
- Direct read state 可以暂时只引用服务端不透明 `conversation_ref`，即使 48 小时/500 条
  snapshot window 内没有建立 canonical conversation 的消息，也允许把该状态存入 owner-scoped
  durable backlog 并提交 snapshot/cursor。后续普通消息建立精确 remote-thread binding 时，
  Core 必须先用 verified Persona canonicalize 消息，再把该 canonical conversation ID 与
  remote thread binding 在同一事务提交并 replay/删除 backlog；不得把 hydration 阶段的
  `dm:<DID>` 暂定值写成 durable binding，也不得根据 DID 猜测 canonical conversation。
- `message.read_state_updated` 必须显式携带 `thread_kind`；Core 不根据 thread key 猜测。
  只有 read state 的 delta/snapshot 也必须在事务提交后产生对应 conversation/thread patch。
- `ensure_conversation(ConversationReadRef)` 幂等提交空会话存在性。Direct 必须已有
  owner-scoped canonical route，Group 必须已有 active membership；校验失败不写入。
- `conversations(ConversationQuery)` 从 committed `conversation_registry` 左连接
  `conversation_summaries` 返回 `Page<Conversation>`，`ConversationQuery` 包含 `limit`、`cursor`、
  `include_groups`、`include_direct` 和 `unread_only`。`PageLimit::new` 仍把单页最大值
  截到 100；调用方必须通过 `next_cursor` 循环翻页，不能假设传入 500/1000 会一次性返回完整列表。
  `next_cursor` 是 SDK 生成的不透明 keyset cursor，只能原样传回下一次
  `conversations` 调用。
- Conversation list 排序固定为 `activity_at DESC, conversation_id DESC`。`activity_at`
  独立于 `last_message_at`，因此无消息会话也有稳定排序键。cursor 内部保存上一页最后一条
  `activity_at` 与 `conversation_id` 排序键，比 offset 更能抵抗新增消息或排序变化。
  调用方不得解析、修改或复用到其他 API。
- `sync_delta` 保留为 v1 compatibility 高层入口，`since_event_seq` 从 `im-core`
  Rust/SQLite 内部 checkpoint 注入，调用方不能传入或推进。该 checkpoint 按稳定
  `owner_identity_id` 与服务端 `sync_subject_id` 双重分区；当前 `sync_subject_id` 是
  canonical DID，DID recovery 后不能继承旧 DID 的 event sequence。v1 checkpoint 与
  v2 cursor 相互隔离，不得互转或共用。
- 对发送者 owner 投影的普通 P3 Direct 元数据事件，`sync_delta` 按 `message_id + server_seq`
  检查具体消息；缺失时先通过权威 Handle 目录解析 Persona，再按 Direct peer 批量从该页最早缺失
  sequence 前一位调用 `direct.get_history` 补齐；同一个权威 history page 的 peer scope 只解析
  一次，禁止逐事件或逐消息 N+1 拉取。会话中存在更晚消息不代表该消息已存在；只有该 peer 在本页
  要求的全部指定消息均已提交后才推进 checkpoint。历史页使用稳定 conversationId 展示时，
  Core 仍以 `direct + peer DID` 保存普通 Direct 的不可变 wire identity，不能把展示 ID 写成
  `thread` wire identity。该流程不创建 P5 session，也不改变原消息的安全级别；补齐失败时
  fail closed，保留原 checkpoint。
- `sync_conversation_after` 是 conversationId-first thread-local 补新 wrapper。新的 App/Dart
  消息显示主路径应使用 `ConversationReadRef.conversation_id`，旧 `sync_thread_after(ThreadRef)`
  只作为 CLI/legacy adapter 或低层调试入口。blocking/async 均调用 account-authorized、
  plain-only 的 `sync.thread_after`；private body 严格只有 `thread_key`、
  `after_server_seq`、`limit`。Direct 的 `thread_key` 只能来自 owner-scoped durable
  canonical-conversation binding，缺失时 fail closed，禁止用 peer DID 猜测；Group 使用
  Group DID。Core 对服务返回页执行 ordinary-only 防御过滤，E2EE、MLS、device-ciphertext
  行即使被错误返回也不得进入本地普通消息投影。该路径只提交本地 message facts/projection，
  不推进 account cursor。
- `sync.delta` 的 metadata-only message discovery 可以先更新会话活动和未读数，但不会作为
  完整 Message 暴露给 local timeline。`discovered` / `hydrated` / migration-only
  `legacy_probe` 都是 Core 私有持久化状态，不作为每条 Message 的 public DTO 字段暴露，host
  不得自行保存或推断。
- `after_server_seq` 是补新提示。若 Core 存在更早的 durable hydration gap，blocking/async
  路径都会使用 `min(requested_after, earliest_gap - 1)`；未显式传值时同样从最早 gap 前开始，
  无 gap 才使用本地最大 sequence。结果中的过滤和 `next_after_server_seq` 均以这个 effective
  cursor 为准，因此结果可能包含不大于调用方原始提示、但尚未 hydrated 的缺口消息。
- `local_conversation_timeline` 读取 `conversation_id` 对应的 committed SQLite projection，
  是 App local-first timeline 的事实源，并且只返回 Core 已确认完整的 hydrated 消息；远端 history/backfill 结果只有持久化到 projection
  后才能成为 UI 可见事实。
- `send_conversation_text` / `send_conversation_payload` 是 conversation-surface send 主路径。
  该边界只接受 verified Persona route 对应的 `dm:peer-scope:v1:*`，或存在 active local
  membership projection 的 `group:<Group DID>`；`dm:<DID>` / `legacy_unresolved` 会在 local
  echo 前 fail closed。target-first `send` 继续作为 CLI/daemon/legacy compatibility API。
  `im-core` 先写 durable pending projection，再按网络结果更新 `MessageMetadata.send_state` /
  retry plan 并发 committed patch；App 不应维护第二套 durable optimistic message truth。
- conversation-surface Direct 正常发送只读取 owner-scoped 本地 route，不预先请求 Directory
  或公开 WNS。只有远端明确返回 `anp.invalid_target_binding`（兼容旧 `1406` 或
  `data.json_rpc_code = 1406`）且 `reason = stale_did` 时，Core 才执行一次 Direct route
  恢复并最多重发一次；其他 service/application error、Group 发送和第二次失败均不恢复。
  同域 Handle 必须由本地域 Directory 与公开 WNS 对 Handle、域名、current DID 和
  `binding_generation` 双重校验，跨域 Handle 只使用公开 WNS。并发恢复按
  `owner_identity_id + conversation_id` 合并，且新绑定必须保持 Persona / canonical
  conversation 不变、generation 单调前进并拒绝旧 DID。
- Direct stale-route 重发保持同一 message ID、operation/idempotency ID、正文、security mode
  和 canonical conversation。`direct_peer_routes.current_did` 是可替换路由；任何已经落盘的
  wire receiver DID 都是不可变消息事实。text/payload 在首次网络发送前写入 local echo，因此
  保留失败 route；attachment 若在远端接受后才首次建立消息行，则记录 accepted route。后续同一
  logical message 调用复用已有 wire snapshot 做本地冲突校验，但网络发送使用当前 route，不能把
  DID rotation 误判为 `message_wire_identity_conflict`。
- `attachments().send_conversation` / Dart `client.attachments.sendConversation(...)` 是
  conversation-surface attachment send 主路径。AWiki Me 已选中会话的附件发送和重试必须传
  `ConversationReadRef.conversation_id`，不能用 target DID、handle、display thread id 或
  memory pending 决定发送归属。旧 `attachments().send(target, ...)` 只保留给 CLI、daemon、
  legacy caller 和尚未持有 canonical conversation 的兼容入口。Direct stale-route 恢复覆盖
  plain / secure、blocking / async conversation attachment；object create/upload/commit 不重复，
  只使用同一 message/operation ID 向新 route 重发已经准备好的 Manifest。
- `mark_conversation_read` 是 conversationId-first read watermark API。local read-state 使用
  canonical `conversation_id` storage key，远端 `read_state.mark_read` 由 core resolver 转成
  direct / group service thread；旧服务端 fallback 到本地 unread ids +
  `inbox.mark_read(message_ids)` 或本地 group pending ack。`mark_thread_read(ThreadRef)` 与
  `mark_read(ids)` 仅保留 legacy/explicit message-id compatibility。返回的
  `MarkThreadReadResult.effective_watermark` 是本地已提交水位；`pending_remote_ack=true`
  只表示远端回执尚待收敛，不回滚本地 read-state。调用方必须保留这个结构化结果，不能把
  `remote_acknowledged=false` 直接等同于本地失败。
- `load_conversation_snapshot`、`clear_conversation_snapshot`、
  `watch_conversation_patches`、`repair_conversation_store` 和
  `watch_conversation_timeline_patches`、`repair_conversation_timeline_store` 是
  conversationId-first snapshot / patch runtime store API，当前仍挂在 message service namespace
  下；`watch_thread_patches(ThreadRef)` 和 `repair_thread_store(ThreadRef)` 是 compatibility
  wrapper；
  snapshot/patch DTO 的 optional `title` 只表示 committed Group profile display name；
  DTO 必须保持 core-only，不引用 `awiki-me` 的 `ConversationSummary`、`ChatMessage`
  或 presentation overlay 字段。
- Dart patch stream 的 `cancel()` / 对应 stop API 是一个完成屏障：即使 stream 正在 idle
  `next_patch()`，也必须唤醒并等待 Rust 后台 worker 退出后返回。raw session 移交后台任务后，
  stop 能力仍由 Dart bridge session 保留；conversation、conversation timeline 与 legacy
  thread patch 不得出现“attach 后 stop 失效”的 ownership 状态。
- conversation/timeline runtime store 只对 material change 递增版本并发 patch。
  committed invalidation 重新投影后若 items 与当前 store 完全相同，则不发事件；
  单项变化发 `upsert/remove`，多项真实变化才回退 `reset`。显式 repair、lag/overflow
  仍按 repair contract 返回 patch。
- Public API 不得暴露 `loadGlobalCheckpoint`、`storeGlobalCheckpoint`、SQLite helper、
  raw `sync.delta` wire params 或手动 checkpoint advance。
- Realtime sync hint 只作为只读事件元数据进入 event stream，用于调度 `sync_delta`，
  不推进 checkpoint。
- Realtime incoming message 尚无 thread-local `server_sequence` 时，Core committed local
  projection 的时间排序键使用接收侧时间，不使用发送方 `sent_at`；可靠同步补齐 sequence 后，
  App timeline consumer 在两条消息都具备 sequence 时以 sequence 为权威顺序，时间只作为缺失
  sequence 时的兼容排序键。
- P5 v2 Direct 发送仍返回一个逻辑 `SendMessageResult`。Core 从目标 DID Document 内嵌的
  `deviceManifest` 解析 exact device，为每个目标设备以及发送者自己的其他有效设备分别发送
  一次标准 `direct.send`；跨域 wire 不新增 `deliveries[]` 批量封装。自有设备副本解密后按
  outgoing/own-sync 逻辑消息投影，而不是显示为一条控制消息。
- P5 的设备级 accepted 状态由 Core 的本地 ledger 聚合：全部接受映射为 `Accepted`，至少一台
  接受映射为带 warning 的部分成功 `DeliveryState::Sent`，零接受映射为 `Failed`。使用相同逻辑
  message ID 和 idempotency scope 重试时，只继续 pending/failed 设备投递，不重复发送已
  accepted 的设备。
- P6 v2 Group 发送先读取标准 P4 group state，再把一条业务消息恰好加密为一个 MLS
  Application Ciphertext，并只提交该密文一次；不会按群内设备拆成多份完整消息密文。群附件
  对象只加密、上传一次，附件 Manifest 随这一份 MLS Application 消息交付。
- Inbox/History、可靠 sync、realtime 与 delegated 投影必须在 legacy renderer 之前识别 P5/P6
  v2 candidate。只有成功认证并解密的业务 plaintext 可以转成普通 `Message`/`ImEvent`；
  own-sync 只投影为 outgoing 业务消息，握手、notice、其他 control、replay、畸形或 gate-disabled
  candidate 均不得原样暴露 wire/cipher/control JSON，也不得回退到 legacy 明文渲染。
- gate 开启且未协商 P6 lane 时，blocking/async legacy read 与 realtime 收到的标准 P6
  `group.e2ee.notice` 会在 Core 内部进入 device-scoped SDK MLS 状态机；协商
  `lanes.p6_group.v1` 后，legacy Inbox 捎带消费停用，notice 只由 P6 lane 可靠消费。成功、幂等
  replay 或拒绝都不会产生 public `Message` / `ImEvent`，也不暴露 wire control JSON。

`msg send --to`、`--group`、`--text-file`、`--file`、`--secure` 是 CLI 输入形态，不是 SDK 字段。CLI adapter 负责转换成 `MessageTarget`、`MessageBody`、`MessageSecurityPolicy`。

当 `msg send --payload` 成功时，`im-core` 返回的 `MessageBodyView::Payload`
必须被 CLI 作为正常的结构化消息结果渲染，JSON 输出中的
`data.message.type` 为 `application/json`；不得将 payload 成功回执误判为仅允许
text body 的内部错误。

## 9. directory：P2+

```rust
pub struct DirectoryService<'a> {
    client: &'a ImClient,
}

pub enum IdentitySubject {
    Did(Did),
    Handle(Handle),
    Any(String),
}

impl DirectoryService<'_> {
    pub fn resolve_peer(&self, peer: PeerRef) -> ImResult<DirectoryResolution>;
    pub fn lookup_handle(&self, handle: Handle) -> ImResult<HandleLookupResult>;
    pub fn public_profile(&self, subject: IdentitySubject) -> ImResult<PublicProfile>;
    pub fn save_contact(&self, request: SaveContactRequest) -> ImResult<Contact>;
    pub fn contacts(&self, query: ContactListQuery) -> ImResult<Page<Contact>>;
    pub fn hydrate_display_profiles(
        &self,
        request: DisplayProfileBatchRequest,
    ) -> ImResult<Vec<DisplayProfile>>;
    pub fn relation_status(&self, peer: PeerRef) -> ImResult<RelationStatus>;
    pub fn follow(&self, request: FollowRequest) -> ImResult<FollowResult>;
    pub fn unfollow(&self, request: UnfollowRequest) -> ImResult<UnfollowResult>;
    pub fn relationship_status(&self, peer: PeerRef) -> ImResult<RelationshipStatus>;
    pub fn followers(&self, query: RelationshipListQuery) -> ImResult<Page<RelationshipListItem>>;
    pub fn following(&self, query: RelationshipListQuery) -> ImResult<Page<RelationshipListItem>>;
}
```

`DirectoryResolution.conversation_id` 由 im-core directory resolver 生成：Handle lookup
具有稳定 user scope 时返回 `dm:peer-scope:v1:*`，无法取得 peer scope 时才回退到 legacy
`dm:<DID>`。成功解析稳定 user scope 时，im-core 同时在 owner-scoped local state 中记录
canonical conversation 到 current DID 的内部 route；因此空会话的首条 text/payload/attachment
发送也直接使用 canonical ID。App/CLI 不得复制 hash 算法、拼 `dm:<DID>` write alias，或在
收到首条消息后才纠正会话 ID。route 缺失或完整性校验失败必须 fail closed。

Authority subject 按域边界区分：同域 AWiki Directory lookup 继续使用域内 `user_id`；跨域
Direct 与 target-first attachment 只从目标域公开 WNS 文档读取并验证 ANP-04 的 `handle`、
`did`、`status`、`binding_generation`，以规范化且永久保留的完整 Handle 作为 authority
subject。公开文档中即使出现 `user_id` / `subject_id` 也必须忽略；generation 必须是无固定位宽
限制的 canonical positive decimal string。相同 local-part 位于不同 domain 时始终属于不同
scope。stale-DID 重试也必须重新走该权威路由，不能使用错误响应携带的私有 subject ID。

旧预发布实现若曾使用公开 WNS 中的私有 ID 生成 scope，Core 不猜测该 ID 与完整 Handle 的
唯一对应关系，也不自动创建合并 alias；当前没有足够可信输入完成无歧义迁移。新解析统一使用
上述 scope，历史数据迁移需未来提供独立、可验证的迁移证据后再实现。

`HandleLookupResult` 和 `DirectoryResolution.profile` 可以承载 WNS Handle Resolution Document 中的 DID Subject Profile 投影。SDK 优先接受合法的 `profile`：

- `profile.subject_did` 必须等于外层 `did`；
- `profile.handle` 如存在必须等于外层 `handle`；
- 合法 profile 会映射为 `Profile.display_name`、`avatar_uri`、`profile_uri`、`description`、`subject_type`、`version_id`、`ttl`；
- 不合法 profile 会被忽略并写入 `warnings`，然后回退旧 `get_public_profile` 路径；
- `profile` 只用于展示，不用于 routing、authentication、authorization、service discovery、E2EE session binding 或安全策略选择。

`Profile` 标准字段：

```rust
pub struct Profile {
    pub subject: Did,
    pub handle: Option<Handle>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub bio: Option<String>,          // legacy compatibility
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,   // legacy compatibility
    pub profile_uri: Option<String>,
    pub subject_type: Option<String>,
    pub agent_kind: Option<String>,
    pub agent_capabilities: Vec<String>,
    pub updated_at: Option<String>,
    pub profile_version: Option<String>,
    pub version_id: Option<String>,
    pub ttl: Option<u64>,
}
```

`agent_kind` 与 `agent_capabilities` 是 User Service inventory 的公开安全投影，供客户端
做类型展示和交互预判；能力数组只保留前 16 个去重非空字符串。它们不是授权证据，群成员
准入必须以 Message/User Service 的最终结果为准，也不得从 DID、Handle 或展示文案补造能力。

`profile_version` 是 User Service 账号 Profile 域提交后的 canonical non-negative decimal
string，允许 `"0"` 且不得转换为固定位宽整数。它只在相应私有 Profile RPC 返回该版本时存在。
`version_id` 仍是 WNS DID Subject Profile 的 `versionId` 展示元数据；二者来源和语义独立，
旧响应只有 `versionId` 时 `profile_version` 保持 `None`。

`hydrate_display_profiles` 是本地 cache 读取 API，不会发起 WNS / User Service 远程请求。它用于联系人列表、会话列表、群成员列表等热路径水化展示资料；cache miss 时返回 `cache_hit = false`，过期 Persona Profile 返回 `is_stale = true`，仅由旧 contact `name/nick_name` 补出的兼容值额外返回 `legacy_fallback = true`。调用方可以先稳定显示旧值，再合并一次显式远端刷新，并始终按 `display_name -> handle -> did` fallback。Persona Profile 一旦存在，即使其 `display_name` 为空也不得用 contact 旧名称补回；权威响应清空名称时必须回退 Handle，而不是永久保留旧值。远程刷新仍应通过显式 `resolve_peer` / `public_profile` / 安全验证链路触发。`public_profile` 成功后，如果目标 DID 已绑定到 verified Persona，Core 会持久更新该 Persona 的可变展示字段，使后续 `hydrate_display_profiles` 在 Core/client 重建后仍返回最新值；该投影保留 verified Handle，且不会为 contact-only 目标创建 Persona、route 或 canonical conversation。

当前身份的账号级 Profile 快照由 User Service Account State 持有。非 Core 调用方取得该权威快照后，只能通过 identity registry 的 owner-scoped、幂等 display projection API 更新本机 `IdentitySummary.display_name`；不得直接改写 registry 文件或按 credential alias 猜测目标身份。该投影只影响本机展示 cache，不改变 DID、Handle、认证、路由、设备绑定或 SessionEpoch。

远程 Directory Handle lookup、Profile resolve 和 public-profile 查询都是幂等读取；遇到
`TransportUnavailable` 时，Core 仅以完全相同的 endpoint、method 和 params 重放一次，
不会重放服务错误，也不会把该策略扩展到没有幂等身份的写操作。

`relation_status(peer)` 是本地 contact projection 查询；`relationship_status(peer)` 是远端 DID relationship authoritative 查询，并合并本地 `is_contact` / `messaged` / `relationship` 投影。Relationship DTO 不暴露 user-service 内部 `from_user_id` / `to_user_id`。 Flutter facade 的 `client.directory.relationStatus(peer)` 明确桥接后者，并完整保留五个方向/阻塞布尔值；其中 `relationship` 仍只表示调用方的 outbound local projection，不能替代 combined state。

P1 的 `messages().send(Direct)` 可以内部做最小目标解析，但不需要对外暴露完整 `DirectoryService`。

## 10. groups：P3+

P1 的群聊只要求：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group_ref),
    body: MessageBody::Text { ... },
    security: MessageSecurityPolicy::Default,
    ..
})
```

daemon 初始化步骤 04 之后，普通结构化 JSON 也走同一个 messages API：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer_ref),
    body: MessageBody::Payload {
        payload: serde_json::json!({
            "schema": "awiki.agent.command.v1",
            "command": "runtime.agent.create",
        }),
    },
    security: MessageSecurityPolicy::DefaultPlain,
    ..
})
```

SDK 内部必须把该 body 发送为 `meta.content_type = "application/json"` 和 `body.payload`。`im-core` 不解释 payload 内部的 command/status/result 语义，也不新增 daemon 业务专用 content type。

Daemon 的 `runtime.agent.create` 成功语义包含消息就绪门禁：新 Runtime Agent 必须先用其
exact-device client 完成一次 Sync V2 bootstrap/delta 提交并持久化 reconcile completion，
之后才能发送既有 `ready` command status 和欢迎消息。门禁失败沿用既有 status schema，
额外返回 `phase="message_readiness"`；相同 `client_request_id` 的重试复用已创建 Agent，
不得再次 exchange，也不得回填创建前历史。这样旧客户端仍消费原有 ready/failed 结构，
而 ready 之后发送的第一条消息具有稳定的 tail-only 接收基线。

完整 group service 在 P3 下沉：

```rust
pub struct GroupService<'a> {
    client: &'a ImClient,
}

pub struct CreateGroupRequest {
    pub profile: GroupProfileDraft,
    pub policy: GroupPolicyDraft,
}

pub struct GroupProfileDraft {
    pub display_name: String,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
}

pub struct GroupPolicyDraft {
    pub discoverability: Option<GroupDiscoverability>,
    pub admission: Option<GroupAdmissionMode>,
    pub max_members: Option<u32>,
    pub attachments_allowed: Option<bool>,
}

impl GroupService<'_> {
    pub fn create(&self, request: CreateGroupRequest) -> ImResult<GroupSnapshot>;
    pub fn get(&self, group: GroupRef) -> ImResult<GroupSnapshot>;
    pub fn list(&self, query: GroupQuery) -> ImResult<Page<GroupSummary>>;
    pub fn join(&self, group: GroupRef, request: JoinGroupRequest) -> ImResult<GroupMembershipChange>;
    pub fn leave(&self, group: GroupRef, request: LeaveGroupRequest) -> ImResult<GroupMembershipChange>;
    pub fn add_member(&self, group: GroupRef, member: PeerRef, options: AddMemberOptions) -> ImResult<GroupMembershipChange>;
    pub fn remove_member(&self, group: GroupRef, member: PeerRef, options: RemoveMemberOptions) -> ImResult<GroupMembershipChange>;
    pub fn update_profile(&self, group: GroupRef, patch: GroupProfilePatch) -> ImResult<GroupSnapshot>;
    pub fn update_policy(&self, group: GroupRef, patch: GroupPolicyPatch) -> ImResult<GroupSnapshot>;
    pub fn members(&self, group: GroupRef, query: MemberQuery) -> ImResult<Page<GroupMember>>;
    pub fn messages(&self, group: GroupRef, query: HistoryQuery) -> ImResult<Page<Message>>;

    // Convenience。内部等价于 messages().send(MessageTarget::Group(...))。
    pub fn send_text(&self, group: GroupRef, text: String) -> ImResult<SendMessageResult>;
}
```

普通群消息统一走 `client.messages().send(MessageTarget::Group)`。`groups().send_text()` 只是便利封装，不重复实现业务逻辑。

Step 4 对当前 `GroupReadResult` 做兼容增量，不另建一套结果 hierarchy：

```rust
pub struct GroupListRequest {
    pub limit: PageLimit,
    pub cursor: Option<Cursor>,
}

pub struct GroupMembersRequest {
    pub group: GroupRef,
    pub limit: PageLimit,
    pub cursor: Option<Cursor>,
}

pub struct GroupReadResult {
    // 原有 group/groups/members/messages/total/source/warnings 保持
    pub next_cursor: Option<Cursor>,
    pub has_more: bool,
    pub page_group: Option<GroupRef>,
    pub group_state_version: Option<String>,
}
```

`cursor` 是 opaque service cursor，只能原样传回同一方法和 scope。`page_group` 与
`group_state_version` 只投影 `group.list_members` Host response；version 跨 Rust/Dart 保持
canonical positive decimal string，不转换成整数。`messages: Page<Message>` 只承载 message
history 的分页信息，不能承载 group/member page metadata。普通 list API 一次只返回一页，
不会隐式抓取全部成员。CLI 的 `page_group` 以及兼容 `group` 字段都来自该 Host response；
response 缺失该 binding 或与请求 Group 冲突时返回
`group.local_inventory_incomplete`，不得用请求参数补写。

MLS roster、P6 notice 和安全成员判断使用内部 bounded complete collector：固定单页 100，
最多 10 页、1000 个 parsed item，并以群的权威 `max_members`（缺省 500、产品上限 500）
继续约束 active members。collector 校验 raw/typed count、Group DID、version、cursor progress、
显式 `status=active`、Message wire 中可解析且唯一的 `agent_did`、total 与最终页；
version/cursor 的首尾空白不会被规范化接受。stale 最多重启三次，完整收齐前不修改 MLS。
稳定错误闭集为
`group.local_cursor_invalid`、`group.local_cursor_stale`、
`group.local_inventory_incomplete`、`group.local_inventory_too_large`。

Rust SDK 调用方创建群组时推荐使用 `GroupCreateRequest::new(name)`，再按需设置 `description`、`avatar_uri`、`discoverability` 等可选字段，避免后续新增可选字段时依赖完整 struct literal。群资料更新继续使用 `GroupProfilePatch::default()` 后按需填写字段；`avatar_uri` 对应 Group Host 权威的 `group_profile.avatar_uri`，`name` 仍只是 `group_profile.display_name` 的兼容输入。Node facade 的 `createGroup()` 与 `joinGroup()` 不接受调用方传入成员 Handle；它们从当前 identity-bound `ImClient` 读取完整 Handle 并分别写入 `creator_handle` / `member_handle`。无 Handle 的明确 DID-only identity 仍保持字段缺失，最终 Group Host 对存在的 Handle 执行 fresh resolve 与 DID 精确匹配。

Handle recovery 后，host 通过现有 high-level `resume_rebind_recovery_async(limit)` 恢复 durable P4/P6 任务。该调用会先从完整 Handle 的 provider-domain HTTPS `/.well-known/handle/{local-part}` 读取公开 WNS 文档，再补建历史缺失的 P4 job；普通 `handle.lookup` RPC 不含权威 generation，不能替代该文档。只有以下条件全部满足时才补建：公开状态为 `active`、返回的完整 Handle 精确一致、WNS DID 等于当前签名 DID、`did:wba` domain 与 Handle provider 一致、`binding_generation` 是 canonical positive decimal string，旧成员 DID 来自当前 owner 的 previous DID history，或来自同一 state root 下已完成且 Handle/current DID/generation 精确一致的 Recovery receipt。fresh owner 没有旧 roster 时，只接受 reliable sync 写入、subject 为当前 DID 的 active Group projection 作为候选；最终仍由 Group Host 对旧 Handle/DID/generation 和 transport-only policy 做权威校验。缺字段、numeric/非 canonical generation、DID/domain mismatch、跨域同名 local-part 都 fail closed；不得推算 generation。

该 Group recovery 权威读取与跨域 Direct 使用同一公共 WNS 绑定边界，只消费
`handle` / `did` / `status` / `binding_generation`；公共响应中的域内 `user_id` /
`subject_id` 不参与群成员换绑、Persona 或 scope 判断。

补建后仍由新 DID 的 origin proof 调用 `group.rebind_member`，Group Host 负责再次校验 WNS continuity 和幂等性。Manifest Handle Recovery V4.0 只为权威策略明确为 `transport-protected`、且权威完整 roster 精确显示旧 DID/旧 generation 的 Handle-backed member 创建修复任务；发送 P4 前必须重新读取 `group.get + group.get_info` 与版本一致的分页 roster。DID-only、Group E2EE、缺失、畸形或冲突状态一律 fail closed，并计入不支持影响项；Recovery operation ID 即使遇到缓存漂移也绝不进入 P6。身份 Recovery 在 receipt 落盘后即为 `applied`，Group 修复的 pending/blocked 只属于 Group journal，不得把 Recovery 改回 blocked。App 只调用 high-level resume 并消费脱敏 summary，不拼 raw RPC 或 SQL；Node facade 的 summary 保留 `group_did`、`layer`、`phase`、`blocked` 和发送暂停群列表，但不得把 warnings 或底层错误详情转发给 Browser；CLI/Daemon 的 Recovery 产品入口留待后续版本。

P4 被 Group Host 接受后，high-level resume 会先把本地稳定 Handle member 投影原子推进到返回请求对应的 `new_member_did` 与 generation，再把 durable P4 job 标记为 `complete` 或 `awaiting_p6`。若该本地投影未能完成，job 保持重试状态；下一次恢复仍使用相同稳定 `operation_id`。因此连续 Handle recovery 的下一代任务必须以前一代已接受并已投影的 DID 为 `previous_member_did`，不能重新从最早历史 DID 建链。

## 11. local state / bootstrap

P1 API：

```rust
pub struct CoreBootstrap<'a> {
    core: &'a ImCore,
}

impl CoreBootstrap<'_> {
    pub fn validate_paths(&self) -> ImResult<PathValidationReport>;
    pub fn initialize_local_state(&self) -> ImResult<LocalStateStatus>;
    pub fn migrate_local_state(&self) -> ImResult<MigrationReport>;
}
```

不暴露：

```rust
open_sqlite_connection()
store_message(owner, paths, record)
query_inbox(owner, paths, query)
execute_sql()
```

Debug SQL 属于 CLI `debug.db.*`，不属于 SDK default API。

## 12. attachments：P4+

```rust
pub struct AttachmentService<'a> {
    client: &'a ImClient,
}

impl AttachmentService<'_> {
    pub fn send(&self, target: MessageTarget, request: AttachmentSendRequest) -> ImResult<AttachmentSendResult>;
    pub fn send_with_client_message_id(
        &self,
        target: MessageTarget,
        request: AttachmentSendRequest,
        client_message_id: MessageId,
    ) -> ImResult<AttachmentSendResult>;
    pub async fn send_with_client_message_id_async(
        &self,
        target: MessageTarget,
        request: AttachmentSendRequest,
        client_message_id: MessageId,
    ) -> ImResult<AttachmentSendResult>;
    pub fn send_conversation(
        &self,
        request: SendConversationAttachmentRequest,
    ) -> ImResult<AttachmentSendResult>;
    pub async fn send_conversation_async(
        &self,
        request: SendConversationAttachmentRequest,
    ) -> ImResult<AttachmentSendResult>;
    pub fn download(&self, request: DownloadAttachmentRequest) -> ImResult<DownloadedAttachment>;
    pub async fn download_async(&self, request: DownloadAttachmentRequest) -> ImResult<DownloadedAttachment>;
    pub fn download_conversation(
        &self,
        request: DownloadConversationAttachmentRequest,
    ) -> ImResult<DownloadedAttachment>;
    pub async fn download_conversation_async(
        &self,
        request: DownloadConversationAttachmentRequest,
    ) -> ImResult<DownloadedAttachment>;
}

pub fn cancel_download(destination: impl AsRef<Path>) -> bool;

pub struct AttachmentSendRequest {
    pub input: AttachmentInput,
    pub caption: Option<String>,
    pub mention_payload: Option<serde_json::Value>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub delivery: MessageDeliveryOptions,
    pub security: MessageSecurityMode,
}

pub struct SendConversationAttachmentRequest {
    pub conversation: ConversationReadRef,
    pub input: AttachmentInput,
    pub caption: Option<String>,
    pub mention_payload: Option<serde_json::Value>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

pub struct UploadedAttachment {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub object_encryption_mode: String,
    pub plaintext_size_bytes: Option<u64>,
}

pub enum AttachmentInput {
    LocalFile(PathBuf),
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}

pub enum AttachmentDestination {
    LocalFile(PathBuf),
    Memory,
}

pub struct DownloadConversationAttachmentRequest {
    pub conversation: ConversationReadRef,
    pub message_id: MessageId,
    pub attachment_id: Option<String>,
    pub destination: AttachmentDestination,
    pub overwrite: bool,
}
```

`AttachmentSendRequest.security = DefaultPlain | Plain` 保持 `transport-protected + encryption_info.mode=none`。
`E2eeRequired | SecureDirect | GroupE2ee` 复用 `messages().send(MessageBody::Attachment, security)` 的高层 secure attachment 路径：对象上传前本地加密，返回的 `AttachmentSendResult.manifest` 是 redacted manifest。

`SendConversationAttachmentRequest` 复用同一上传 runtime，但入口是 `ConversationReadRef`。
SDK resolver 先把 canonical `conversation_id` 映射到 direct / group storage route，再写入
durable projection 并 emit committed patch。plain/default 附件路径在 projection 失败时返回错误；
App 不再需要 presentation fallback 来补 conversation list/detail correctness。

`DownloadConversationAttachmentRequest` 对下载应用同一 canonical route 规则，Node/Flutter 等
已经持有 conversation ID 的 host 不再从 peer DID 或 Group DID 猜测 `ThreadRef`。公开
`parse_attachment_manifest` 只返回 `AttachmentManifest` 的 redacted descriptor、caption 和
digest；不会返回 object key、nonce、ticket 或密文运行时状态。

target-first 调用方在首次消息尚未建立 canonical conversation 时，可以使用
`send_with_client_message_id(_async)` 显式传入逻辑消息 ID，并通过
`AttachmentSendRequest.delivery.idempotency_key` 传入幂等键。该入口与普通 `send`
使用相同上传和发送 runtime，不要求预先存在 conversation registry 记录。

默认 public API 不暴露 `object_key_b64u`、`nonce_b64u`、download ticket、raw ciphertext、secure session state 或 MLS provider path。

`AttachmentDestination::LocalFile` 是 App 和大文件调用方的权威下载模式。Core 使用与目标文件
同目录的固定 `<destination>.awiki-part` 暂存文件：每次失败或进程退出后保留已完成字节，下一次
调用重新申请短期 download ticket，并用单段 `Range: bytes=N-` 从已有长度继续。服务端忽略
Range 并返回完整对象时，Core 清空暂存文件后完整重下，不拼接两个对象版本。下载只有在声明
size 和 SHA-256 digest 均通过后才原子替换目标文件；E2EE 先完整校验密文对象，再解密并发布
明文目标文件。普通 RPC 的 30 秒总超时不适用于对象传输；对象下载使用每段无进度超时和最多
4 次有界重试。

Windows 原子发布会把绝对 drive/UNC 路径转换为 `\\?\` / `\\?\UNC\` 扩展长度形式后再
调用 `MoveFileExW`。因此 storage scope、群消息 ID 和 `.awiki-part` 组合超过传统
`MAX_PATH` 时仍可发布，不依赖用户机器另行开启 `LongPathsEnabled`。

`cancel_download(destination)` 只取消同一进程内该精确目标路径的活动下载，返回是否找到活动
传输。取消错误码为 `attachment_transfer_cancelled`，不会触发自动重试，也不会删除
`.awiki-part`；调用方再次使用相同目标路径即可继续。相同路径启动新下载时，Core 会先取消旧
传输，禁止两个 writer 并发追加同一暂存文件。其他稳定传输错误为
`attachment_transfer_network | attachment_transfer_stalled |
attachment_transfer_incomplete | attachment_transfer_range_rejected`。

## 13. realtime：P5+

```rust
pub struct RealtimeService<'a> {
    client: &'a ImClient,
}

impl RealtimeService<'_> {
    pub fn status(&self) -> ImResult<RealtimeStatus>;
    pub fn connect(&self, options: RealtimeOptions) -> ImResult<RealtimeHandle>;
    pub fn run_until_shutdown(&self, options: RealtimeOptions, shutdown: ShutdownSignal) -> ImResult<RealtimeExit>;
}
```

`RealtimeSyncHint` 新增只读 `dirty_lanes: BTreeSet<SyncLane>`，其中 `SyncLane` 是
`Ordinary | P5Device | P6Group`。它只描述需要可靠 reconciliation 的逻辑 lane；与
`event_seq`、`sync_dirty`、`gap_detected` 一样不授予 checkpoint authority。Rust Core 暴露该
字段；现有 Dart hint 仍只投影 domain/reason/dirty/gap，不把 cursor 或 lane checkpoint 暴露给
App。

协商 `awiki.sync.event.v3` 后，schema 3 可闭合内联 ordinary `message.created`、
`p5.delivery.created` 和 `p6.delivery.created`。三类事件均幂等应用，且**内联永不推进 ordinary、
P5 或 P6 checkpoint**；`sync.delta` 是唯一 SoT。超过 32 KiB 或无法内联时降级为 schema-2
hint；可选 `dirty_lanes` 仅用于已经协商 v3 的连接，纯 v2 连接继续收到原三字段 hint。该改动
直接扩展未发布的 v3，不新增 `awiki.sync.event.v4`。

CLI daemon/service 仍在 `awiki-cli`。SDK 只提供 runner，不安装服务、不写 pid、不管理 systemd/launchd/Windows service。

## 14. secure：P6+

P1 不实现 secure public flow。`MessageSecurityPolicy::E2eeRequired` 返回 `UnsupportedCapability`。

P6 增加：

```rust
client.secure().direct(peer).status()
client.secure().direct(peer).prepare()
client.secure().direct(peer).repair()
client.secure().outbox().list_failed()
client.secure().outbox().retry(id)
client.secure().outbox().drop(id)
client.secure().group(group).status()
client.secure().group(group).prepare()
client.secure().group(group).repair()
```

KeyPackage、prekey、MLS provider、ciphertext processing、direct session id、ratchet counter、raw attachment manifest 不进入默认 public API。

`secure().group(group).status()` 会先读取 Host-authoritative
`group.get.e2ee_maintenance`。`device_revocation_pending` gate 存在时：

- active owner 且本机持有 active controller state：`NeedsRepair`；
- 当前 identity 不是 active owner：`WaitingForMembershipUpdate`；
- 本机没有 controller state：`MissingLocalState`。

三种状态都固定 `can_send_secure=false`。该 status 调用不抓完整 roster、不 resolve member
Manifest、不生成 Commit，也不写 MLS WAL；Host projection 读取失败或字段畸形时 fail closed
为 unavailable，不能因本地 tree 看似 ready 而返回 `Ready`。低敏 projection 只接受
`reason` 与 `send_paused` 两个字段；出现 target、count 或其他额外字段同样按畸形处理。

永久设备撤销的成功结果仍只有 DID、target device ID 和 `Revoked`。异常边界额外提供闭集
`DeviceRevokeOutcomeCategory::{CancelledBeforeSubmit, RejectedBeforeCommit, OutcomeUnknown}`；
调用方不得靠错误 message 文本判断是否可以重试。成功只表示 User Registry/DID Document 与
本地 Identity state 已收敛，不表示所有 Group MLS 已完成。

## 15. system_notifications：V1 control-plane projection

```rust
client.system_notifications().list(query).await
client.system_notifications().get(event_id).await
client.system_notifications().watch(query).await
```

该 API 只读取 Core 已完成 P3 Origin Proof、目标 DID、closed payload、Join Request Proof、
durable dedupe 和单调 revision reducer 后的本地投影。公开
`SystemNotificationSnapshot` 只包含事件/session 标识、通知 kind/state/revision、时间和 terminal
标志；不暴露原始 P3 envelope、Origin Proof、Join Request、Challenge Response、token、SAS、
私钥或其他 Join secret。

系统通知的 P3 `meta.sender_did` 是独立、可解析且 E1 绑定的 System Notification Agent DID；
目标 DID 的 `ANPMessageService.serviceDid` 只用于锚定 Home Service 域，不能作为 Business
Origin。该信任校验是 Core 内部固定策略，不增加自定义 P3 profile 或公开配置字段。

System Notification 不进入 `Message`、conversation/history/search、unread/read watermark 或
attachment projection。`watch()` 只在 durable reducer commit 后发送
`SystemNotificationChange`；订阅 lag 返回 `RepairRequired`，由调用方重新 `list()`。
Realtime committed dispatch 同时发送 `ImEvent::SystemNotificationChanged`；其中可选
`sync: RealtimeSyncHint` 只用于调度可靠同步，不是 checkpoint，不能据此推进本地游标。
设备定向由 Message Service 的投递元数据和已认证的 exact-device Inbox scope 完成，不是 P3
协议字段；Core 不接受在 P3 `meta` 中增加 `device_id`、`recipient_device_id` 等自定义设备
目标字段，标准 P3 `target` 仍然只绑定目标 agent DID。
