# SDK Refactor：公共接口总览

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
```

P1 API：

```rust
impl ImCore {
    pub fn new(config: ImCoreConfig, paths: ImCorePaths) -> ImResult<Self>;

    pub fn identities(&self) -> IdentityRegistry<'_>;
    pub fn bootstrap(&self) -> CoreBootstrap<'_>;

    pub fn client(&self, selector: IdentitySelector) -> ImResult<ImClient>;
}

impl ImClient {
    pub fn current_identity(&self) -> &IdentitySummary;
    pub fn did(&self) -> &Did;
    pub fn handle(&self) -> Option<&Handle>;

    pub fn auth(&self) -> AuthService<'_>;
    pub fn messages(&self) -> MessageService<'_>;
}
```

P2+ API：

```rust
impl ImClient {
    pub fn identity(&self) -> IdentityService<'_>;      // P2+
    pub fn directory(&self) -> DirectoryService<'_>;    // P2+
    pub fn groups(&self) -> GroupService<'_>;           // P3+
    pub fn attachments(&self) -> AttachmentService<'_>; // P4+
    pub fn realtime(&self) -> RealtimeService<'_>;      // P5+
    pub fn secure(&self) -> SecureDiagnosticsService<'_>; // P6+
}
```

`ClientIdentityRuntime`、`ActorContext`、`LoadedIdentity`、`IdentityRuntimePaths` 都是 `pub(crate)`。

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
}
```

P2+ API：

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
    pub fn profile(&self) -> ImResult<Profile>;
    pub fn update_profile(&self, patch: ProfilePatch) -> ImResult<Profile>;
    pub fn bind_contact(&self, request: BindContactRequest) -> ImResult<BindContactResult>;

    // 危险命令，晚于普通 IM 能力迁移。
    pub fn replace_did(&self, request: ReplaceDidRequest) -> ImResult<ReplaceDidResult>;
}
```

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

DID auth request、JWT 文件格式、session metadata path 都是内部实现。CLI/App 不应该直接保存或读取 bearer token，除非 Phase 7 明确引入外部 credential/session store。

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

    // P4+
    Attachment {
        input: AttachmentInput,
        caption: Option<String>,
        mime_type: Option<String>,
    },
}

pub enum MessageKind {
    Text,
    Markdown,
    System,
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,

    // P6+ reserved. P1 返回 UnsupportedCapability。
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
    pub fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
}
```

P3+ API：

```rust
impl MessageService<'_> {
    pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
    pub fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
}
```

`msg send --to`、`--group`、`--text-file`、`--file`、`--secure` 是 CLI 输入形态，不是 SDK 字段。CLI adapter 负责转换成 `MessageTarget`、`MessageBody`、`MessageSecurityMode`。

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
    pub fn resolve_peer(&self, subject: IdentitySubject) -> ImResult<PeerProfile>;
    pub fn lookup_handle(&self, handle: Handle) -> ImResult<ResolvedIdentity>;
    pub fn save_contact(&self, request: SaveContactRequest) -> ImResult<Contact>;
    pub fn contacts(&self, query: ContactQuery) -> ImResult<Page<Contact>>;
    pub fn relation_status(&self, peer: PeerRef) -> ImResult<RelationStatus>;
}
```

P1 的 `messages().send(Direct)` 可以内部做最小目标解析，但不需要对外暴露完整 `DirectoryService`。

## 10. groups：P3+

P1 的群聊只要求：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group_ref),
    body: MessageBody::Text { ... },
    security: MessageSecurityMode::DefaultPlain,
    ..
})
```

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
    pub fn send(&self, target: MessageTarget, request: AttachmentSendRequest) -> ImResult<SendMessageResult>;
    pub fn download(&self, request: DownloadAttachmentRequest) -> ImResult<DownloadedAttachment>;
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

P1 不实现完整 upload/download。若调用 `MessageBody::Attachment`，返回 `UnsupportedCapability`。

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

P1 不实现 secure public flow。`MessageSecurityMode::SecureDirect` / `GroupE2ee` 返回 `UnsupportedCapability`。

P6 增加：

```rust
client.secure().direct_status(peer)
client.secure().repair_direct_session(peer)
client.secure().list_failed_outbox()
client.secure().retry_outbox(id)
client.secure().group_status(group)
client.secure().repair_group_state(group)
```

KeyPackage、prekey、MLS provider、ciphertext processing 不进入默认 public API。
