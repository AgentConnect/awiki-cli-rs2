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
```

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

impl SkillOnboardingService<'_> {
    pub async fn claim_async(&self, request: SkillClaimRequest)
        -> ImResult<SkillClaimResult>;
    pub fn claim(&self, request: SkillClaimRequest) -> ImResult<SkillClaimResult>;
}
```

claim 只接受与 SDK 配置完全同源的 HTTPS 服务和已初始化、无可用 identity 的
workspace。相同 journal 可恢复同一 DID；其他非空或无法识别状态均返回
`skill_onboarding_workspace_conflict`。成功结果只包含 Agent DID/Handle、Controller
Handle、确定性 greeting message ID、phase/status 和稳定错误码，不含 Token、JWT 或
私钥。问候尚未被 Message Service 接受时返回 `greeting_pending + retryable=true`，
重试继续使用同一 DID 和 message ID，不重新注册。

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

pre-open canonical runner 只拥有 schema 27。已经完成 canonical cutover 的 schema
28 到当前版本必须返回 `not_required`，随后由普通 Core open 的原子 schema migration
推进到当前版本；pre-open detector 不得复制或抢占普通迁移的版本分派。

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

    pub fn plan_default_identity_change(
        &self,
        selector: IdentitySelector,
    ) -> ImResult<DefaultIdentityChange>;
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

`delete_local_identity` 是纯本地、crash-safe 的身份退役事务。registry/default
pointer tombstone 是目录与 Vault 清理之前的权威状态；Core open 会恢复中断阶段，并
通过永久 identity-ID tombstone 清理由并发尾部任务晚写的 identity-scoped Vault
records。Host 的 realtime stop、runtime dispose 和任何网络 logout 都不属于该调用的
成功条件。

Legacy upgrade 是 Core 内部可恢复事务；host 必须等待
`upgrade_legacy_identity_async` 的 typed status，不得用更短的通用 UI request timeout
包裹它。`RetryRequired.code` 只返回
`transport_unavailable|service_error|permission_denied|auth_required|local_state_unavailable|legacy_upgrade_failed`
之一，既不泄露响应正文，也不会把 native future 仍在执行误判为已取消。

`register_handle` 是唯一注册入口。新注册生成带 bootstrap Manifest 的 DID 和独立设备
keys，并通过同一个 `register` RPC 原子创建远端状态；无 Manifest 的旧客户端仍走 Legacy
兼容。Handle 已存在时返回 typed `join_required`，不创建第二个身份，host 使用其中的一次性
account verification grant 进入 Device Join。新注册本地提交后必须发布 exact-device P5
PreKey Bundle；失败保留同一 PendingRegistration 精确重试。公共 DTO 不暴露私钥、pending、
内部 checkpoint 或 refresh token。`HandleRegistrationResult.account_id` 仅在
`registered` 结果中返回服务端 canonical `user_id`；`join_required` 不伪造账号 ID。

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
Member/not-ready recipient, Manifest/Registry bindings, Root Vault metadata and
P5 Session or PreKey readiness. It returns an opaque 60-second, single-use
authorization handle plus a secret-free recipient summary. The host then calls
`confirm_and_send` once with that handle and local user presence; Core, not the
host, generates the message ID.

The RootKeyEnvelope is secret JSON carried by a standard P5 v2 Init or Cipher.
There is no private endpoint, delivery class, sidecar, empty-Init handshake,
imported ACK, public list/retry state, or host-supplied message ID. Core commits
the standard P5 pending state and its secret-free sender delivery ledger in the
same SQLite transaction, and an uncertain transport response is retried only
with the identical P5 bytes and message ID. Startup and an explicit later
prepare first recover any `pending_delivery` by resuming those durable bytes;
P5 acceptance and the sender `sent` fact commit atomically. Core never reopens
the Root Vault or creates a replacement message during recovery, and it rejects
a new transfer while that recipient already has a pending or sent fact.

On the recipient, authenticated Mailbox delivery supplies the exact accepted
tuple and timestamp. Core validates the outer P5 binding, Registry/Manifest,
RootEnvelope, fingerprint and current checkpoint before sealing the root as
`IdentityRootImportPending`. It then sends the closed, double-proof
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
`registry_confirmed` and `promoted` coordinators to repair both local promotion
and pending-Vault cleanup crash windows.

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

V1 不公开 Handle Recovery API。未来 Recovery 必须作为独立安全方案重新设计，不复用
Device Join 或 Legacy→Manifest 升级，也不能恢复性复制 Ratchet/MLS 私有状态。

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
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
    pub fn local_history(
        &self,
        thread: ThreadRef,
        query: LocalHistoryQuery,
    ) -> ImResult<Page<Message>>;
    pub fn sync_delta(&self, request: SyncDeltaRequest) -> ImResult<SyncDeltaResult>;
    pub fn sync_now(&self, request: MessageSyncRequest) -> ImResult<MessageSyncOutcome>;
    pub fn sync_thread_after(
        &self,
        request: SyncThreadAfterRequest,
    ) -> ImResult<SyncThreadAfterResult>;
}
```

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
}
```

Reliable sync 补充：

- `sync_now(MessageSyncRequest { reason, limit })` 是 v2 普通 Direct/Group 主链路。
  account、device、cursor 均由 Core 内部从 active binding 和 SQLite 获取，public outcome
  只暴露高层状态、计数、changed conversation IDs、已提交 incoming message 和诊断。
  `sync.bootstrap` 的 binding/Group baseline/cursor，以及每页 `sync.delta` 的 receipt、
  exact-hydrated projection/cursor 均在单个 SQLite 事务提交；必需 hydration、schema、
  canonical identity 或 route 解析失败时整页回滚，cursor 与 receipt 不变。
  `message.get_batch` 服务端使用 16 MiB hard response budget；Core 固定按请求顺序每 8 个
  event ID 分批，为 compact JSON 封装与转义保留余量。任一批次出现 unavailable 时整页不应用。
  bootstrap 的 `client_instance_id` 是 Core 为每个本地 owner 在同步 SQLite 首次生成并先持久化
  的随机不透明值：请求丢失/失败重试和重启复用，清库/新 DB 自动变化，不能由 owner/account/
  device 稳定标识派生。
- `sync_now` 在一次调用内闭合 compact recovery：
  delta（或 existing-device bootstrap）→ 只存在于 Rust 进程栈的不透明 token →
  snapshot 严格校验/原子 merge → post-anchor delta。成功只返回既有 `changed` / `idle`；
  snapshot 或 post-anchor delta 失败返回 `retryable_failure`，授权或 generation fence 返回
  `auth_revoked`。没有第二个 public recover API，raw token、cursor/anchor、cutoff、policy
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
  parse、协议验证或本地 ACK transaction 失败都必须把 claim 退回 `retryable`。
- Direct read state 可以暂时只引用服务端不透明 `conversation_ref`，即使 48 小时/500 条
  snapshot window 内没有建立 canonical conversation 的消息，也允许把该状态存入 owner-scoped
  durable backlog 并提交 snapshot/cursor。后续普通消息建立精确 remote-thread binding 时，
  Core 在同一事务 replay 并删除 backlog；不得根据 DID 猜测 canonical conversation。
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
  Rust/SQLite 内部 checkpoint 注入，调用方不能传入或推进；它与 v2 cursor 不得互转或共用。
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
  只作为 CLI/legacy adapter 或低层调试入口。Core 对服务返回页还要执行 ordinary-only
  防御过滤：E2EE、MLS、device-ciphertext 行即使被服务错误返回也不得进入本地普通消息投影。
  blocking/async 均调用 account-authorized `sync.thread_after`；private body 严格只有
  `thread_key`、`after_server_seq`、`limit`。Direct 的 `thread_key` 只能来自 owner-scoped
  durable canonical-conversation binding，缺失时 fail closed，禁止用 peer DID 猜测；Group
  使用 Group DID。该路径只提交本地 message facts/projection，不推进 account cursor。
- `local_conversation_timeline` 读取 `conversation_id` 对应的 committed SQLite projection，
  是 App local-first timeline 的事实源；远端 history/backfill 结果只有持久化到 projection
  后才能成为 UI 可见事实。
- `send_conversation_text` / `send_conversation_payload` 是 conversation-surface send 主路径。
  该边界只接受 verified Persona route 对应的 `dm:peer-scope:v1:*`，或存在 active local
  membership projection 的 `group:<Group DID>`；`dm:<DID>` / `legacy_unresolved` 会在 local
  echo 前 fail closed。target-first `send` 继续作为 CLI/daemon/legacy compatibility API。
  `im-core` 先写 durable pending projection，再按网络结果更新 `MessageMetadata.send_state` /
  retry plan 并发 committed patch；App 不应维护第二套 durable optimistic message truth。
- `attachments().send_conversation` / Dart `client.attachments.sendConversation(...)` 是
  conversation-surface attachment send 主路径。AWiki Me 已选中会话的附件发送和重试必须传
  `ConversationReadRef.conversation_id`，不能用 target DID、handle、display thread id 或
  memory pending 决定发送归属。旧 `attachments().send(target, ...)` 只保留给 CLI、daemon、
  legacy caller 和尚未持有 canonical conversation 的兼容入口。
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
- gate 开启时，blocking/async read 与 realtime 收到的标准 P6 `group.e2ee.notice` 会在 Core 内部
  进入 device-scoped SDK MLS 状态机；成功、幂等 replay 或拒绝都不会产生 public `Message` /
  `ImEvent`，也不新增 public DTO 或跨域字段。

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
    pub updated_at: Option<String>,
    pub profile_version: Option<String>,
    pub version_id: Option<String>,
    pub ttl: Option<u64>,
}
```

`profile_version` 是 User Service 账号 Profile 域提交后的 canonical non-negative decimal
string，允许 `"0"` 且不得转换为固定位宽整数。它只在相应私有 Profile RPC 返回该版本时存在。
`version_id` 仍是 WNS DID Subject Profile 的 `versionId` 展示元数据；二者来源和语义独立，
旧响应只有 `versionId` 时 `profile_version` 保持 `None`。

`hydrate_display_profiles` 是本地 cache 读取 API，不会发起 WNS / User Service 远程请求。它用于联系人列表、会话列表、群成员列表等热路径水化展示资料；cache miss 时返回 `cache_hit = false`，调用方按 `display_name -> handle -> did` 的展示 fallback 处理。远程刷新仍应通过显式 `resolve_peer` / `public_profile` / 安全验证链路触发。

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

Rust SDK 调用方创建群组时推荐使用 `GroupCreateRequest::new(name)`，再按需设置 `description`、`avatar_uri`、`discoverability` 等可选字段，避免后续新增可选字段时依赖完整 struct literal。群资料更新继续使用 `GroupProfilePatch::default()` 后按需填写字段；`avatar_uri` 对应 Group Host 权威的 `group_profile.avatar_uri`，`name` 仍只是 `group_profile.display_name` 的兼容输入。

Handle recovery 后，host 通过现有 high-level `resume_rebind_recovery_async(limit)` 恢复 durable P4/P6 任务。该调用会先从完整 Handle 的 provider-domain HTTPS `/.well-known/handle/{local-part}` 读取公开 WNS 文档，再补建历史缺失的 P4 job；普通 `handle.lookup` RPC 不含权威 generation，不能替代该文档。只有以下条件全部满足时才补建：公开状态为 `active`、返回的完整 Handle 精确一致、WNS DID 等于当前签名 DID、`did:wba` domain 与 Handle provider 一致、`binding_generation` 是 canonical positive decimal string 且严格大于本地成员 generation、旧成员 DID 精确属于当前 `IdentityId` 的 previous DID history。缺字段、numeric/非 canonical generation、DID/domain mismatch、跨域同名 local-part 都 fail closed；不得推算 generation。

该 Group recovery 权威读取与跨域 Direct 使用同一公共 WNS 绑定边界，只消费
`handle` / `did` / `status` / `binding_generation`；公共响应中的域内 `user_id` /
`subject_id` 不参与群成员换绑、Persona 或 scope 判断。

补建后仍由新 DID 的 origin proof 调用 `group.rebind_member`，Group Host 负责再次校验 WNS continuity 和幂等性。SDK 不直接修改服务端 roster；transport-protected 群在 P4 接受后完成，Group E2EE 群继续遵循既有 P4 `group_state_ref` → P6 Add(new DID) → Remove(old DID) durable 顺序。群安全分类必须保留 Group Host `group.get` / `group.list` 返回的 `required_security_profile` 或等价 `group_policy.message_security_profile`；只有明确的 `transport-protected` 才能跳过 P6，缺失、畸形或冲突值一律按未知 fail closed。若旧客户端已把 transport 群误留在 `awaiting_p6`，high-level resume 会先刷新权威群快照，再仅完成本地 P4 outbox，不重复发送 P4，也不改服务端成员表。App/CLI 只调用 high-level resume 并消费脱敏 summary，不拼 raw RPC 或 SQL。

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
}

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
```

`AttachmentSendRequest.security = DefaultPlain | Plain` 保持 `transport-protected + encryption_info.mode=none`。
`E2eeRequired | SecureDirect | GroupE2ee` 复用 `messages().send(MessageBody::Attachment, security)` 的高层 secure attachment 路径：对象上传前本地加密，返回的 `AttachmentSendResult.manifest` 是 redacted manifest。

`SendConversationAttachmentRequest` 复用同一上传 runtime，但入口是 `ConversationReadRef`。
SDK resolver 先把 canonical `conversation_id` 映射到 direct / group storage route，再写入
durable projection 并 emit committed patch。plain/default 附件路径在 projection 失败时返回错误；
App 不再需要 presentation fallback 来补 conversation list/detail correctness。

target-first 调用方在首次消息尚未建立 canonical conversation 时，可以使用
`send_with_client_message_id(_async)` 显式传入逻辑消息 ID，并通过
`AttachmentSendRequest.delivery.idempotency_key` 传入幂等键。该入口与普通 `send`
使用相同上传和发送 runtime，不要求预先存在 conversation registry 记录。

默认 public API 不暴露 `object_key_b64u`、`nonce_b64u`、download ticket、raw ciphertext、secure session state 或 MLS provider path。

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
