# 04. Message Interface

本文最早来自 P1 message interface 设计，但本文件位于当前 API 文档目录下，必须以当前实现为准。历史 P1 限定只作为背景；当前消息显示链路的主合同是 canonical `ConversationIdentity.conversation_id` / `ConversationReadRef`，旧 `ThreadRef` 入口只保留为 CLI、legacy caller 和低层诊断 adapter。

## 1. Service

`messages/service.rs`：

```rust
pub struct MessageService<'a> {
    pub(crate) client: &'a crate::core::ImClient,
}

impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> crate::ImResult<SendMessageResult>;

    pub fn send_conversation_text(
        &self,
        request: SendConversationTextRequest,
    ) -> crate::ImResult<SendMessageResult>;

    pub fn send_conversation_payload(
        &self,
        request: SendConversationPayloadRequest,
    ) -> crate::ImResult<SendMessageResult>;

    pub fn inbox(&self, query: InboxQuery) -> crate::ImResult<crate::ids::Page<Message>>;

    pub fn history(
        &self,
        thread: ThreadRef,
        query: HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<Message>>;

    pub fn local_history(
        &self,
        thread: ThreadRef,
        query: LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<Message>>;

    pub fn mark_read(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<MarkReadResult>;

    pub fn mark_thread_read(
        &self,
        request: MarkThreadReadRequest,
    ) -> crate::ImResult<MarkThreadReadResult>;

    pub fn mark_conversation_read(
        &self,
        request: MarkConversationReadRequest,
    ) -> crate::ImResult<MarkThreadReadResult>;

    pub fn sync_delta(&self, request: SyncDeltaRequest) -> crate::ImResult<SyncDeltaResult>;

    pub fn sync_thread_after(
        &self,
        request: SyncThreadAfterRequest,
    ) -> crate::ImResult<SyncThreadAfterResult>;

    pub fn sync_conversation_after(
        &self,
        request: SyncConversationAfterRequest,
    ) -> crate::ImResult<SyncThreadAfterResult>;

    pub fn local_conversation_timeline(
        &self,
        conversation: ConversationReadRef,
        query: LocalHistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<Message>>;

    pub fn load_conversation_snapshot(
        &self,
    ) -> crate::ImResult<Option<ConversationListSnapshot>>;

    pub fn clear_conversation_snapshot(&self) -> crate::ImResult<()>;

    pub fn watch_conversation_patches(&self) -> crate::ImResult<ConversationPatchSession>;

    pub fn repair_conversation_store(&self) -> crate::ImResult<ConversationStorePatch>;

    pub fn watch_thread_patches(
        &self,
        thread: ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<ThreadMessagePatchSession>;

    pub fn repair_thread_store(
        &self,
        thread: ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<ThreadMessageStorePatch>;

    pub fn watch_conversation_timeline_patches(
        &self,
        conversation: ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<ThreadMessagePatchSession>;

    pub fn repair_conversation_timeline_store(
        &self,
        conversation: ConversationReadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<ThreadMessageStorePatch>;
}
```

当前实现已经包含 conversation projection、conversation identity、local-first timeline、
reliable sync、conversation read watermark、text/payload conversation send、attachment 和
secure/E2EE 相关能力。不要再把“P1 只做普通文本”当成当前 contract。

## 2. Send Request

`messages/dto.rs`：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityPolicy,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub delivery: MessageDeliveryOptions,
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegatedSigningOptions {
    pub logical_sender_did: Option<String>,
    pub signing_verification_method: Option<String>,
    pub signing_key_ref: Option<String>,
    pub actor_agent_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTarget {
    Direct(crate::ids::PeerRef),
    Group(crate::ids::GroupRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBody {
    Text { text: String, kind: MessageKind },
    Payload { payload: serde_json::Value },
    Attachment {
        input: AttachmentInput,
        caption: Option<String>,
        mention_payload: Option<serde_json::Value>,
        mime_type: Option<String>,
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageKind {
    Text,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    E2eeRequired,
    SecureDirect,
    GroupE2ee,
}

pub type MessageSecurityPolicy = MessageSecurityMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryOptions {
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

成功发送的 `SendMessageResult.message.metadata.attributes` 会保留服务响应中的
`final_acceptance`（字符串布尔值），供 CLI 等薄适配层在 `delivery_state=accepted`
时仍能准确报告最终接受状态；适配层不得仅从压缩后的 `DeliveryState` 反推该字段。

impl Default for MessageDeliveryOptions {
    fn default() -> Self {
        Self { idempotency_key: None, wait_for_final_acceptance: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentInput {
    LocalFile(String),
    Bytes { filename: Option<String>, mime_type: Option<String>, bytes_len: usize },
}
```

`SendMessageRequest` 是通用 target-first send 入口。Conversation UI 已经知道 canonical
conversation 时，优先使用 `SendConversationTextRequest` / `SendConversationPayloadRequest`：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadRef {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConversationTextRequest {
    pub conversation: ConversationReadRef,
    pub text: String,
    pub markdown: bool,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    pub delegated_signing: Option<DelegatedSigningOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConversationPayloadRequest {
    pub conversation: ConversationReadRef,
    pub payload: serde_json::Value,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
    pub delegated_signing: Option<DelegatedSigningOptions>,
}
```

Conversation send is local-echo first for plain text/payload. `im-core` resolves
`conversation_id` to the storage route, persists a pending local projection row,
emits committed patches after the local transaction succeeds, then updates the
same durable row with accepted/sent/failed state. AWiki Me renders
`MessageMetadata.send_state` / retry fields from SDK DTOs; it must not create a
second durable optimistic message source.

Direct conversation send keeps a stable peer scope when the conversation is bound
to a Handle/user identity. The normal send path uses the resolved DID already
stored for the conversation and does not perform a Handle lookup before every
message. If message-service rejects the send with JSON-RPC `1406` and
`error.data.reason = "stale_did"`, `im-core` treats that as an authoritative
target-rotation signal from user-service: it reads `current_did` /
`full_handle` / `user_id` from `error.data`, fills missing data with one Handle
lookup when a Handle is available, updates the retry target, and retries the
network send once. Other `1406` reasons and all non-`stale_did` errors are not
retargeted automatically and are persisted as failed local send state.

### 2.1 Delegated Signing Optional 扩展

当前 Agent IM MVP 在不改变 ANP `origin_proof` 结构的前提下，为普通 direct/default plain 发送增加 `delegated_signing` optional 参数。调用方不传该字段时，SDK 继续使用当前 identity/session 默认 sender 与默认 authentication key，老 Rust/Dart 调用行为不变。

`delegated_signing` 的字段语义：

| 字段 | 语义 |
|---|---|
| `logical_sender_did` | 消息业务发送者 DID，MVP 为用户 DID，例如 `did:wba:...:user:e1_xxx`。SDK 会把它写入 `meta.sender_did`。 |
| `signing_verification_method` | 用于签 `auth.origin_proof` 的 verification method，MVP 为 `user_did#daemon-key-1`。 |
| `signing_key_ref` | SDK 本地可解析的子私钥引用。新路径应使用 `vault:<secret-ref>`；`file:/.../daemon-key-1.pem`、`local:daemon-key-1` 和裸路径只作为兼容入口。 |
| `actor_agent_did` | 可选审计字段，标识实际发起能力调用的 daemon/runtime agent；不改变 ANP proof 结构。 |

SDK 本地校验：

1. `logical_sender_did` 必须与 `signing_verification_method` 的 DID owner 一致。
2. `signing_verification_method` 必须能在对应 DID Document 的 `authentication` 或兼容 verification method 中找到。
3. `signing_key_ref` 必须能在本地解析到私钥。`vault:` 失败时不会回退到文件路径，避免 scheme 混淆；`file:` / `local:` 仍按 legacy 兼容读取。
4. Delegated send 当前只允许 direct 普通非 E2EE 消息：`DefaultPlain` / `Plain`。
5. Delegated send 对 group、attachment、`E2eeRequired`、`SecureDirect`、`GroupE2ee` 返回 `UnsupportedCapability`，防止 Agent IM MVP 绕过 E2EE 边界。

### 2.2 ANP P9 mention payload 扩展

SDK 支持 ANP-P9 mention-bearing group payload 的最小互操作结构。P9 不新增
JSON-RPC 方法、外层 `meta.profile`、专用 content type、payload `protocol` /
`schema` 标记、mention sender、mention proof 或服务端 selector 展开。

普通 group base 发送继续走 `MessageBody::Payload`，wire content type 为
`application/json`，payload 形态为：

```json
{
  "text": "@agents please summarize this discussion.",
  "mentions": [
    {
      "id": "men_1",
      "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
      "target": {"kind": "group_selector", "selector": "agents"},
      "mention_role": "addressee"
    }
  ]
}
```

Group E2EE 发送同样使用 `MessageBody::Payload`，但 payload 只进入加密前
inner `GroupApplicationPlaintext.payload`，inner
`application_content_type = application/json`。外层 `meta.content_type` 仍是
group cipher content type，mention target 不复制到外层 metadata。

`im-core` 在 `messages` 模块暴露 P9 DTO / validator：

```rust
use im_core::messages::{
    parse_message_mention_payload, validate_message_mention_payload,
    MessageMentionPayload,
};
```

validator 规则：

- `mentions[*].id` 必须非空且在消息内唯一。
- `range.unit` 必须是 `unicode_code_point`；`start < end` 且 `end` 不超过
  `text` 的 Unicode code point 长度。
- `target.kind` 只允许 `human`、`agent`、`group_selector`；selector 只允许
  `all`、`agents`、`humans`。
- `group_selector` 不允许携带 `did`；`human` / `agent` 不允许携带
  `selector`，且 `did` 必须是 DID。
- mention 对象不得包含 `sender`、`sender_did`、`from`、`actor_did`、
  `auth`、`origin_proof`、`proof` 或 `signature`。
- `display_name` 仅是展示快照，不能用于身份、路由、认证或授权。

验证 gate：

```bash
cargo test -p im-core --locked mention
cargo test -p awiki-deamon --locked mention
```

`awiki-deamon` 只在终端侧解析收到的群 P9 payload。命中 runtime agent DID、
`@agents` 或 `@all` 后，daemon 会把 `mention_context` 注入 RuntimeTask；
`@humans`、`human` target、纯文本 `@AgentName`、invalid range 和 E2EE
opaque 都不能触发 runtime agent。该 prompt context 明确 mention 只是
attention signal，不是授权；controller/runtime policy 和 allowed actions 仍然
是执行边界。

## 3. Send Result

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageResult {
    pub message: Message,
    pub delivery: DeliveryState,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryState {
    Accepted,
    Sent,
    StoredLocally,
    Failed { reason: String },
}
```

P1 对远端返回的不稳定字段应 normalize 到 `MessageMetadata` 或 `warnings`，不要在默认 public API 中返回 raw `serde_json::Value`。

如果测试、诊断或 CLI debug 确实需要 raw response，可以放到：

```text
internal-test-helpers feature
diagnostics-only API
debug adapter
```

不能作为默认 `Message` 字段。

### 当前 E2EE 附件更新

当前实现已支持 `MessageBody::Attachment + MessageSecurityMode::E2eeRequired` 作为 canonical 高层入口：

```rust
client.messages().send(SendMessageRequest {
    target,
    body: MessageBody::Attachment {
        input,
        caption,
        mime_type,
        filename,
    },
    security: MessageSecurityMode::E2eeRequired,
    client_message_id,
    delivery,
})
```

direct 目标映射到 `direct-e2ee`，group 目标映射到 `group-e2ee`。附件对象在 SDK 内部先做 `object-e2ee` 加密，完整 manifest 只进入 E2EE 内层 plaintext；public `SendMessageResult.metadata.attributes["attachment_manifest"]` 和 `AttachmentSendResult.manifest` 只保存 redacted manifest，不包含 `object_key_b64u` 或 `nonce_b64u`。

Conversation UI 附件发送应使用 `attachments().send_conversation(SendConversationAttachmentRequest { conversation, ... })` 或 Dart `client.attachments.sendConversation(...)`。该入口接收 `ConversationReadRef.conversation_id`，由 SDK resolver 映射到 direct / group storage route，再复用同一 attachment upload runtime 和 local projection。plain/default 附件路径在 projection 写入失败时返回错误，避免 App 用 presentation fallback 补 list/detail correctness。

CLI、Dart legacy caller 和其他还没有 canonical conversation 的 facade 可以继续使用 `attachments().send(target, AttachmentSendRequest { security, ... })` 作为兼容入口；当 `security` 为 secure required 时，该入口内部复用上述 message high-level 路径，不拼 P7/P5/P6 wire。AWiki Me conversation UI 不应再通过 target DID、handle、display thread id 或 legacy thread API 发送/重试附件。

## 4. Message DTO

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: crate::ids::MessageId,
    pub thread: ThreadRef,
    pub direction: MessageDirection,
    pub sender: crate::ids::PeerRef,
    pub receiver: Option<crate::ids::PeerRef>,
    pub group: Option<crate::ids::GroupRef>,
    pub body: MessageBodyView,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub metadata: MessageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDirection {
    Outgoing,
    Incoming,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBodyView {
    Text { text: String, kind: MessageKind },
    Payload { payload: serde_json::Value },
    Unsupported { content_type: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    pub operation_id: Option<String>,
    pub delivery_state: Option<String>,
    pub send_state: Option<MessageSendState>,
    pub retry_plan: Option<MessageRetryPlan>,
    pub server_sequence: Option<i64>,
    pub content_type: Option<String>,
    pub conversation_identity: Option<ConversationIdentity>,
    pub attributes: Vec<MessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadataAttribute {
    pub key: String,
    pub value: String,
}
```

Flutter 展示 DTO 额外把 canonical identity 提升为顶层强字段：

```text
conversationId          // required；来自 metadata.conversation_identity
senderPeerPersonaId?    // verified Persona 已解析时提供
senderDidSnapshot       // 消息发生时的不可变 sender DID
```

`threadKind/threadId` 仅保留作 wire/legacy 兼容，不得再作为 App 会话主键。可靠入站
Direct 在 Persona 解析后把 `sender_peer_persona_id` 写入本地 projection metadata；若发送者
Persona 尚不可解析，`senderPeerPersonaId` 保持空而 `senderDidSnapshot` 仍必须保留。新 App
展示路径遇到空 `conversationId` 必须 fail closed，不得退回构造 `dm:<DID>`。

`MessageMetadata` 只承载业务可解释的补充字段。不要把完整 wire payload 塞进 `metadata`。
`conversation_identity.conversation_id` 是 list/detail/read/send/timeline repair 的跨层 routing
key；`send_state` 和 `retry_plan` 是 pending/accepted/sent/failed 展示事实，不能由 App memory
pending rows 替代。

## 5. Inbox / History Query

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadRef {
    Direct(crate::ids::PeerRef),
    Group(crate::ids::GroupRef),

    // Reserved for Phase 3.
    Thread(crate::ids::ThreadId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxQuery {
    pub scope: InboxScope,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
    pub unread_only: bool,
    pub inbox_history_options: Option<InboxHistoryOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxScope {
    All,
    DirectOnly,
    GroupOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
    pub inbox_history_options: Option<InboxHistoryOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxHistoryOptions {
    pub inbox_owner_did: Option<String>,
    pub inbox_auth_verification_method: Option<String>,
    pub inbox_auth_key_ref: Option<String>,
    pub inbox_auth: Option<InboxAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxAuth {
    ScopedInboxToken { token: ScopedInboxToken },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedInboxToken {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<crate::ids::MessageId>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadRequest {
    pub thread: ThreadRef,
    pub watermark: Option<ReadWatermark>,
    pub fallback_max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWatermark {
    pub last_read_message_id: Option<crate::ids::MessageId>,
    pub last_read_thread_seq: Option<String>,
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadResult {
    pub updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub fallback_used: bool,
    pub pending_remote_ack: bool,
    pub effective_watermark: Option<ReadWatermark>,
    pub legacy_message_ids: Vec<crate::ids::MessageId>,
    pub warnings: Vec<String>,
}
```

### 5.0 Local History 当前补充

Schema 28 的 conversation list/snapshot contract 直接携带：

```text
conversation_id          // required canonical key
peer_persona_id?         // resolved Direct required
canonical_group_did?     // resolved Group required
resolution_state         // resolved | legacy_unresolved | blocked_conflict
```

新 App 主路径必须使用这些字段，不能再执行 `conversation_id ?? thread_id`。Core 在
`resolved` 行缺少对应 Persona/Group DID 时返回 typed projection error；显式
`ensure_conversation` 只接受绑定到 verified Persona 的 Direct route 和以权威 Group DID
为 key 的 active Group membership。`legacy_unresolved` 仍可用于历史列表/诊断，但不能通过
ensure/send 边界伪装成 resolved canonical conversation。

Group member DTO 同时分离 `membership_id`、`peer_persona_id?`、`credential_did` 和
Handle；Handle binding generation/DID 轮换只更新属性，不改变 membership identity。

`history(thread, query)` 保持远端 history + 本地 projection/reconcile 语义。AWiki Me 首屏读取应使用 `local_conversation_timeline(conversation, query)`；`local_history(thread, query)` 是兼容入口：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalHistoryQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}
```

`local_conversation_timeline` / `local_history` 行为：

1. 只读取本地 `messages` projection，不访问 `direct.get_history`、`group.list_messages`、`inbox.get` 或目录 RPC。
2. 主路径按 `owner_identity_id` 和 `ConversationReadRef.conversation_id` 查询，返回最近消息，顺序为 newest-first；legacy `local_history(ThreadRef)` 先解析到 owner-scoped conversation ids。
3. `next_cursor` 是 SDK 生成的不透明 `local-history:v1:*` cursor，只能传回 local timeline/history 翻页。
4. direct、group 和 raw thread ref 使用与 conversation mark-read 一致的 owner-scoped conversation-id 归一化。
5. App 首屏应先显示 `local_conversation_timeline`，再后台调用 `sync_conversation_after` 或 repair/load core projection；远端 history/backfill 返回的 messages 只有持久化后才能成为 UI 事实。

P1 不把 `mark_read` 放进 `InboxQuery`。当前实现把 mark-read 作为
`MessageService` 的显式方法，避免 inbox/history 查询和 read ack 语义耦合。

### 5.1 Mark Read / Thread Mark Read 当前补充

`mark_read(ids)` 保留按消息 id 确认已读的 legacy compatibility 语义。`mark_thread_read(request)`
是 `ThreadRef` 兼容入口；AWiki Me、Dart SDK 和新的消息显示主路径应优先使用
`mark_conversation_read(request)`，让 `ConversationReadRef.conversation_id` 成为跨 list、
timeline、read ack 的唯一 routing key。

`mark_thread_read(request)` / `mark_conversation_read(request)` 都是 watermark-first API：

1. `mark_conversation_read` 接收 `ConversationReadRef { conversation_id }`，内部拆成两条
   routing：local projection / `thread_read_state` 使用 raw `ThreadRef::Thread(conversation_id)`
   作为 storage key；远端 `read_state.mark_read` 必须先由 core resolver 解析成
   `ThreadRef::Direct(peer_did)` 或 `ThreadRef::Group(group_did)`，不得把 raw
   `conversation_id` 作为 `kind: "thread"` 发给 message-service。
2. `mark_thread_read` 使用现有 `ThreadRef::Direct` / `ThreadRef::Group`，只作为 CLI/legacy
   migration adapter 或低层 compatibility surface。
3. `request.watermark` 可选。调用方不传时，SDK 从本地 committed
   `messages` projection / MessageStore thread window 计算当前 thread 可见的最高已读水位。
4. `ReadWatermark.last_read_thread_seq` 是 thread-local sequence：direct 使用 direct
   message `server_seq`；group 使用 group thread view 中的 `server_seq`，该值可由
   `group_event_seq` 投影而来。两者都不是账号级 reliable sync `event_seq`。
5. `ReadWatermark.last_read_message_id` 只是诊断、幂等和 mismatch 检查辅助，不是排序事实来源。
6. `ReadWatermark.read_at` 是客户端已读动作时间，用于审计/展示，不参与授权或 checkpoint。
7. SDK 优先调用服务端 `read_state.mark_read`。wire contract 以
   `message-service/docs/api/ANP-client-server-api-read-state.md` 为准，只允许 direct / group
   service thread。peer-scope direct conversation 的 current DID 必须由 core 优先从 directory
   解析时写入的 owner-scoped `direct_peer_routes` 获取；旧 message metadata / participants 只作
   compatibility fallback，不能由 AWiki Me 拼 alias。该 route projection 允许尚无 message row
   的空会话完成首条 canonical send/read/sync，并在 DID rotation 后保持 conversation ID 不变。
8. 旧服务端、endpoint unsupported 或 transport unavailable 时，SDK fallback 到当前
   本地 unread ids 查询；direct 尝试 `inbox.mark_read(message_ids)`，group 在旧服务端下只能
   保持 local-read / pending-remote-ack 语义。
9. `fallback_max_message_ids` 只限制 legacy message-id fallback 的候选数量，默认和硬上限仍为 500；
   watermark path 不受该 message id 数量限制。
10. 远端 ack 失败不回滚本地已读，返回 `partial = true`、`pending_remote_ack = true`
   或在 `warnings` 中带失败原因；空 unread 会返回 `updated_count = 0`。
11. `legacy_message_ids` 只作为 fallback diagnostics；App 不应依赖它作为 thread
    mark-read 的核心结果。

`conversation_summaries` 是 rebuildable projection；当前热路径对普通 message upsert、
bounded mark-read、`mark_conversation_read` 和 legacy `mark_thread_read` 使用增量维护，只有无法安全增量判断的场景才回退 rebuild。conversation mark-read 同步维护 summary unread 字段；public 契约不因此变化。

### 5.2 Reliable Sync API

Reliable sync 是 `im-core` 内部拥有 checkpoint 的高层能力。服务端 wire contract 以
`message-service/docs/api/ANP-client-server-api-sync.md` 为准；本节定义 Rust public
interface 对 App/CLI/facade 的暴露形态。

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SyncDeltaRequest {
    pub limit: Option<u32>,
    pub device_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDeltaResult {
    pub events_applied: u32,
    pub pages_fetched: u32,
    pub last_applied_event_seq: Option<String>,
    pub has_more: bool,
    pub snapshot_required: bool,
    pub retention_floor_event_seq: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncThreadAfterRequest {
    pub thread: ThreadRef,
    pub after_server_seq: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadRef {
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConversationAfterRequest {
    pub conversation: ConversationReadRef,
    pub after_server_seq: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncThreadAfterResult {
    pub messages: Vec<Message>,
    pub next_after_server_seq: Option<String>,
    pub has_more: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeSyncHint {
    pub event_id: String,
    pub event_seq: String,
    pub event_type: String,
}
```

`sync_delta(request)` 行为：

1. 从本地 SQLite `sync_state` 读取当前 owner 的 checkpoint。
2. 调用服务端 `sync.delta`，wire request 中的 `since_event_seq` 只能由 Rust runtime
   注入，public API 调用方不能传入。
3. `limit`、`device_id`、`reason` 只作为分页和诊断输入；`reason` 是字符串，不是封闭
   enum，便于 App 记录 `startup`、`app_resumed`、`reconnect`、`realtime_gap` 等来源。
4. 在同一个本地 SQLite transaction 中 apply 所有事件、更新 conversation/message
   projection。Direct 的 peer DID 尚未绑定 verified Persona 时，不创建 `dm:<DID>`：先把
   保留原 WireIdentity 的消息写入 owner-scoped `inbound_resolution_backlog`，成功后才能写入
   `next_event_seq` checkpoint。后续权威 Handle projection 会幂等重放对应 backlog；冲突保持
   blocked/conflict-visible，不猜测合并。
5. 返回 `events_applied`、`pages_fetched` 和 `last_applied_event_seq` 作为诊断和 UI 状态；
   `last_applied_event_seq` 不是 public checkpoint setter。
6. 当服务端返回 `snapshot_required=true` 时 fail-closed：不推进 checkpoint、不清空本地
   projection，返回 `snapshot_required=true` 和诊断字段。
7. `has_more=true` 时可由 runtime 或上层 coordinator 继续调度下一页，但每页仍必须走
   apply + checkpoint transaction。

`sync_thread_after(request)` 行为：

1. 使用 `ThreadRef` 和本地 thread max `server_seq` 或调用方传入的 `after_server_seq`
   生成服务端 `sync.thread_after`。
2. 返回并应用 `server_seq > after_server_seq` 的升序消息。
3. 不读取或写入账号级 checkpoint。
4. 不得直接返回 `history_async` 的本地合并 page；必要时使用 raw remote history path 并
   严格过滤。

`sync_conversation_after(request)` 是新的 conversationId-first wrapper。它使用
`ConversationReadRef.conversation_id` 解析 storage thread/ref，再调用同一 thread-local
`sync.thread_after` contract。AWiki Me 的 realtime dirty/gap、打开会话补新和 backfill
主路径应走该 API；旧 `sync_thread_after(ThreadRef)` 只保留给 CLI/legacy adapter 或低层调试。

`local_conversation_timeline(conversation, query)` 是 local-first timeline 主路径。它只读取
`conversation_id` 对应的 committed SQLite projection，不调用远端 history。App 可以用它做
首屏和 repair 后读取；远端 history/backfill 返回的 messages 只有在 core 持久化后，才能通过
该 timeline 或 patch stream 成为 UI 事实。

checkpoint 边界：

- `sync_state`、checkpoint load/store、`since_event_seq` 注入、`next_event_seq` 推进只在
  `im-core` Rust/SQLite 内部。
- Public Rust、Dart、Flutter、App API 不暴露 `loadGlobalCheckpoint`、
  `storeGlobalCheckpoint`、手动 `since_event_seq` 或手动 checkpoint advance。
- Realtime `RealtimeSyncHint` 只用于 duplicate/gap/dirty 判断和调度 `sync_delta`；即使
  realtime projection 成功，也不得推进 checkpoint。

### 5.3 Conversation / Thread Snapshot And Patch API

当前实现把 conversation snapshot、conversation patch stream 和 thread message patch stream
短期保留在 `messages` namespace 下：Rust 为 `client.messages()`，Dart/Flutter 为
`client.messages.*`。尚未引入独立 `client.conversations` namespace；文档、示例和 App
代码不得同时描述两套当前 API。

`conversations(ConversationQuery)` / Dart `conversations(limit: ..., cursor: ...)`
读取当前 owner 的 SQLite committed `conversation_summaries` projection，返回
`Page<Conversation>` / `ConversationPage`。API 设计要求：

- `ConversationQuery` 字段为 `limit`、`cursor`、`include_groups`、`include_direct`、
  `unread_only`。
- `limit` 单页最大值由 `PageLimit::new` 保护为 100；500/1000 会话必须循环
  `next_cursor` 翻页，直到 `has_more=false` 或 `next_cursor=null`。
- 排序为 `last_message_at DESC, conversation_id DESC`，cursor 内部保存上一页最后一条
  `last_message_at` 和 `conversation_id`，属于不透明 keyset cursor，不是 offset。
- 只有第一页、未过滤的 full conversation query 可以刷新 redb snapshot；带 cursor 的后续页、
  unread-only 或 direct/group 过滤查询不得覆盖冷启动 snapshot。
- App/Dart 层必须暴露分页参数和分页返回值，旧的 list API 只能作为第一页兼容包装。

`load_conversation_snapshot()` / Dart `loadConversationSnapshot()` 从 Rust `im-core`
redb snapshot cache 读取非权威、可丢弃的 core-only conversation snapshot。调用方不传
owner、schema、checkpoint 或 raw storage 参数；owner 来自当前 `ImClient` identity。
snapshot 只用于冷启动 first paint，随后仍必须用 SQLite local projection、sync 和 patch
校正，不代表可靠同步已追平。

`clear_conversation_snapshot()` / Dart `clearConversationSnapshot()` 只清除当前 owner 的
非权威 redb snapshot cache，不清除 SQLite committed projection、runtime store、read state
或 reliable checkpoint。它用于 logout、owner switch 或 corruption recovery 场景。

`watch_conversation_patches()` / Dart `watchConversationPatches()` 返回 versioned
`ConversationStorePatch` stream。patch kind 当前包括 `reset`、`upsert`、`remove`、
`reorder`、`repairRequired`。subscriber lag、stream drop、session switch 或 version gap
必须走 `repair_conversation_store()` / Dart `repairConversationStore()`，repair 返回的
patch version 是后续 patch 连续性判断的基线。Conversation Store 的唯一 key 是
`conversation_id`；`remove` / `reorder` 只携带 `conversation_id`，不得用
`(thread_kind, thread_id)` 或 alias/DID 作为 Store identity。snapshot format v2 同样要求
每项有 canonical `conversation_id`，旧的可丢弃 redb snapshot 会直接失效并从 SQLite 重建。

`watch_thread_patches(thread, limit)` / Dart `watchThreadPatches(thread, limit: ...)`
返回当前 thread 的 versioned `ThreadMessageStorePatch` stream。patch kind 当前包括
`reset`、`upsert`、`remove`、`repairRequired`。它只消费 committed local projection；
`sync_thread_after` persistence 失败、remote history best-effort page 或 realtime hint
不得直接生成 authoritative thread patch。
realtime incoming 消息如果成功写入 SQLite local projection，则按同一规则触发
conversation patch 和对应 thread patch；如果 projection 不存在或写入失败，不得发
authoritative patch。

`watch_conversation_timeline_patches(conversation, limit)` 和
`repair_conversation_timeline_store(conversation, limit)` 是 conversationId-first timeline
patch / repair wrapper，返回同一个 `ThreadMessageStorePatch` DTO。新的 App timeline 主路径
应使用这些 API；旧 `watch_thread_patches(ThreadRef)` / `repair_thread_store(ThreadRef)` 是
compatibility adapter，不应继续作为 AWiki Me 消息归属判断的主来源。

Conversation snapshot、conversation store patch 和 thread message patch DTO 都必须保持
core-only，不包含 `awiki-me` presentation overlay 字段或 App domain DTO。

### 5.4 Delegated Inbox / History Optional 扩展

`InboxHistoryOptions` 是 Step 02 新增的 optional 参数，供 Daemon 使用 `user_did#daemon-key-1` 证明自己有权读取用户普通 inbox/history。调用方不传 `inbox_history_options` 时，SDK 继续走当前 identity/session 默认 inbox/history 读取逻辑，老调用行为不变。

MVP DID proof 主路径：

| 字段 | 语义 |
|---|---|
| `inbox_owner_did` | 被读取 inbox/history 的用户 DID。 |
| `inbox_auth_verification_method` | 用于签读取 proof 的用户 DID authentication key，MVP 为 `user_did#daemon-key-1`。 |
| `inbox_auth_key_ref` | SDK 本地可解析的子私钥引用。新路径应使用 `vault:<secret-ref>`；`file:`、`local:` 和裸路径只作为兼容入口。 |
| `inbox_auth` | 后续 token 路径预留。MVP 中传 `ScopedInboxToken` 会返回明确 unsupported，不影响 DID proof 主路径。 |

SDK 本地校验：

1. `inbox_owner_did` 必须与 `inbox_auth_verification_method` 的 DID owner 一致。
2. `inbox_auth_verification_method` 必须在 DID Document `authentication` 中有效。
3. `inbox_auth_key_ref` 必须能在本地解析到私钥。`vault:` 失败时不会回退到 legacy 文件引用。
4. Delegated inbox/history 只投影普通非 E2EE 消息。SDK 会过滤 direct/group E2EE opaque 消息，不返回 E2EE 明文、metadata projection 或 private state。
5. `ScopedInboxToken` 为后续优化，MVP 不作为主路径。

## 6. Current Behavior Summary

`send()` 行为：

```text
1. 校验 body。
   - Text 不能为空。
   - Payload 必须是合法 JSON value。
   - Attachment 通过 attachment/object-E2EE 路径处理；conversation UI 附件发送使用 `attachments.send_conversation`，不通过 generic `messages.send_conversation_*` 传 `MessageBody::Attachment`。
2. 校验 security。
   - `DefaultPlain` / `Plain` 走普通发送。
   - `E2eeRequired` / `SecureDirect` / `GroupE2ee` 走 secure/E2EE 能力或 fail closed。
3. 通过 ImClient runtime 注入身份、auth、owner。
4. Direct target：做最小 PeerRef resolve。
5. Group target：使用已有 GroupRef，不做 group lifecycle。
6. ensure_session(AuthScope::Messaging 或 GroupMessaging)。
7. 构造 internal RPC/wire params。
8. 发送；session expired 时 refresh 后 retry once。
9. 远端结果 normalize 成 Message。
10. 必要时写入 local projection；conversation-surface text/payload send 先写 pending durable row，再根据网络结果更新 send state。
```

`inbox()` / `history()` 行为：

```text
1. 注入身份和 owner。
2. 按查询范围 ensure_session：direct 使用 Messaging，group 使用 GroupMessaging，All 需要两者。
3. DirectOnly 通过 inbox.get 拉取 direct inbox，并过滤掉任何异常混入的 group 消息。
4. GroupOnly 先 group.list 获取当前身份所在群，再按群调用 group.list_messages，合并成 group inbox 视图。
5. All 合并 DirectOnly 与 GroupOnly 的结果，按 message id 去重并应用 limit。
6. normalize 成 Page<Message>。
7. 当前实现会维护 conversation projection；历史 P1“不强制 projection”的限制不再代表当前 contract。
```

约束：`InboxHistoryOptions` 目前只支持 direct/delegated inbox；`GroupOnly` 如果传入 delegated inbox options，应返回明确 unsupported；`All` 带 delegated options 时保持兼容，只读取 direct/delegated inbox，不进入 group 子路径，避免 daemon 误把用户 delegated inbox proof 用于群消息读取。

## 7. Dart / Flutter Binding

`packages/awiki_im_core` 公开 API 与 Rust DTO 保持同名 optional 参数：

```dart
await client.messages.sendConversationText(
  const SendConversationTextRequest(
    conversation: ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    text: 'hello',
  ),
);

await client.messages.sendText(
  const SendTextRequest(
    target: MessageTarget.direct('did:example:bob'),
    text: 'hello',
  ),
);

const delegated = SendTextRequest(
  target: MessageTarget.direct('did:example:bob'),
  text: 'hello',
  delegatedSigning: DelegatedSigningOptions(
    logicalSenderDid: 'did:wba:...:user:e1_xxx',
    signingVerificationMethod: 'did:wba:...:user:e1_xxx#daemon-key-1',
    signingKeyRef: 'local:daemon-key-1',
  ),
);

await client.messages.inbox(
  limit: 20,
  inboxHistoryOptions: const InboxHistoryOptions(
    inboxOwnerDid: 'did:wba:...:user:e1_xxx',
    inboxAuthVerificationMethod: 'did:wba:...:user:e1_xxx#daemon-key-1',
    inboxAuthKeyRef: 'local:daemon-key-1',
  ),
);

await client.messages.localConversationTimeline(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  limit: 50,
);

await client.messages.markConversationRead(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
);

await client.attachments.sendConversation(
  SendConversationAttachmentRequest(
    conversation: const ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    input: const AttachmentInput.bytes(
      filename: 'note.txt',
      mimeType: 'text/plain',
      bytes: [104, 101, 108, 108, 111],
    ),
    caption: 'hello',
    clientMessageId: 'msg-app-attachment-001',
    idempotencyKey: 'op-msg-app-attachment-001',
  ),
);
```

兼容性要求：

- `SendTextRequest` / `SendPayloadRequest` 的 `delegatedSigning` 默认 `null`。
- `SendConversationTextRequest` / `SendConversationPayloadRequest` 是 AWiki Me conversation UI send 主路径；target-first send 保留给 CLI、legacy 和没有 canonical conversation 的调用面。
- `SendConversationAttachmentRequest` / `AttachmentApi.sendConversation` 是 AWiki Me conversation UI 附件 send/retry 主路径；`AttachmentApi.send` 保留给 CLI、legacy 和没有 canonical conversation 的调用面。
- `MessageApi.inbox` / `MessageApi.history` 的 `inboxHistoryOptions` 默认 `null`。
- 老 Dart 调用不需要补参数；FRB 生成 DTO/API 只增加 nullable 字段和 nullable 函数参数。
