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

pub struct IdentityRegistry<'a> {
    core: &'a ImCore,
}

impl IdentityRegistry<'_> {
    pub fn list(&self) -> ImResult<Vec<IdentitySummary>>;
    pub fn default_identity(&self) -> ImResult<Option<IdentitySummary>>;
    pub fn resolve(&self, selector: IdentitySelector) -> ImResult<IdentitySummary>;
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
```

Identity vault DTOs are redacted status/report surfaces. They report selected
backend, storage policy, vault availability, metadata verification, workspace /
device context, warnings, and plaintext compatibility retention, but they must
not expose root keys, private PEM, JWTs, bearer tokens, raw `SecretRef` JSON, or
ciphertext internals. `VaultRequired` is fail-closed for new secret persistence.

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

- `conversations(ConversationQuery)` 返回本地 committed `conversation_summaries`
  projection 的 `Page<Conversation>`，`ConversationQuery` 包含 `limit`、`cursor`、
  `include_groups`、`include_direct` 和 `unread_only`。`PageLimit::new` 仍把单页最大值
  截到 100；调用方必须通过 `next_cursor` 循环翻页，不能假设传入 500/1000 会一次性返回完整列表。
  `next_cursor` 是 SDK 生成的不透明 keyset cursor，只能原样传回下一次
  `conversations` 调用。
- Conversation list 排序固定为 `last_message_at DESC, conversation_id DESC`。cursor 内部保存上一页最后一条
  `last_message_at` 与 `conversation_id` 排序键，比 offset 更能抵抗新增消息或排序变化。
  调用方不得解析、修改或复用到其他 API。
- `sync_delta` 是高层可靠同步入口，`since_event_seq` 从 `im-core` Rust/SQLite 内部
  checkpoint 注入，调用方不能传入或推进。
- `sync_conversation_after` 是 conversationId-first thread-local 补新 wrapper。新的 App/Dart
  消息显示主路径应使用 `ConversationReadRef.conversation_id`，旧 `sync_thread_after(ThreadRef)`
  只作为 CLI/legacy adapter 或低层调试入口。
- `local_conversation_timeline` 读取 `conversation_id` 对应的 committed SQLite projection，
  是 App local-first timeline 的事实源；远端 history/backfill 结果只有持久化到 projection
  后才能成为 UI 可见事实。
- `send_conversation_text` / `send_conversation_payload` 是 conversation-surface send 主路径。
  `im-core` 先写 durable pending projection，再按网络结果更新 `MessageMetadata.send_state` /
  retry plan 并发 committed patch；App 不应维护第二套 durable optimistic message truth。
- `mark_conversation_read` 是 conversationId-first read watermark API。local read-state 使用
  canonical `conversation_id` storage key，远端 `read_state.mark_read` 由 core resolver 转成
  direct / group service thread；旧服务端 fallback 到本地 unread ids +
  `inbox.mark_read(message_ids)` 或本地 group pending ack。`mark_thread_read(ThreadRef)` 与
  `mark_read(ids)` 仅保留 legacy/explicit message-id compatibility。
- `load_conversation_snapshot`、`clear_conversation_snapshot`、
  `watch_conversation_patches`、`repair_conversation_store` 和
  `watch_conversation_timeline_patches`、`repair_conversation_timeline_store` 是
  conversationId-first snapshot / patch runtime store API，当前仍挂在 message service namespace
  下；`watch_thread_patches(ThreadRef)` 和 `repair_thread_store(ThreadRef)` 是 compatibility
  wrapper；
  DTO 必须保持 core-only，不引用 `awiki-me` 的 `ConversationSummary`、`ChatMessage`
  或 presentation overlay 字段。
- Public API 不得暴露 `loadGlobalCheckpoint`、`storeGlobalCheckpoint`、SQLite helper、
  raw `sync.delta` wire params 或手动 checkpoint advance。
- Realtime sync hint 只作为只读事件元数据进入 event stream，用于调度 `sync_delta`，
  不推进 checkpoint。

`msg send --to`、`--group`、`--text-file`、`--file`、`--secure` 是 CLI 输入形态，不是 SDK 字段。CLI adapter 负责转换成 `MessageTarget`、`MessageBody`、`MessageSecurityPolicy`。

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
    pub version_id: Option<String>,
    pub ttl: Option<u64>,
}
```

`hydrate_display_profiles` 是本地 cache 读取 API，不会发起 WNS / User Service 远程请求。它用于联系人列表、会话列表、群成员列表等热路径水化展示资料；cache miss 时返回 `cache_hit = false`，调用方按 `display_name -> handle -> did` 的展示 fallback 处理。远程刷新仍应通过显式 `resolve_peer` / `public_profile` / 安全验证链路触发。

`relation_status(peer)` 是本地 contact projection 查询；`relationship_status(peer)` 是远端 DID relationship authoritative 查询，并合并本地 `is_contact` / `messaged` / `relationship` 投影。Relationship DTO 不暴露 user-service 内部 `from_user_id` / `to_user_id`。

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

Rust SDK 调用方创建群组时推荐使用 `GroupCreateRequest::new(name)`，再按需设置 `description`、`avatar_uri`、`discoverability` 等可选字段，避免后续新增可选字段时依赖完整 struct literal。群资料更新继续使用 `GroupProfilePatch::default()` 后按需填写字段；`avatar_uri` 对应 Group Host 权威的 `group_profile.avatar_uri`，`name` 仍只是 `group_profile.display_name` 的兼容输入。

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
    pub fn download(&self, request: DownloadAttachmentRequest) -> ImResult<DownloadedAttachment>;
}

pub struct AttachmentSendRequest {
    pub input: AttachmentInput,
    pub caption: Option<String>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub delivery: MessageDeliveryOptions,
    pub security: MessageSecurityMode,
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
