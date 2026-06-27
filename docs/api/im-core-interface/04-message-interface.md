# 04. Message Interface

P1 messages 只做普通文本：私聊文本、面向已有 `GroupRef` 的群聊文本、必要 inbox/history。

## 1. Service

`messages/service.rs`：

```rust
pub struct MessageService<'a> {
    pub(crate) client: &'a crate::core::ImClient,
}

impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> crate::ImResult<SendMessageResult>;

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

    pub fn sync_delta(&self, request: SyncDeltaRequest) -> crate::ImResult<SyncDeltaResult>;

    pub fn sync_thread_after(
        &self,
        request: SyncThreadAfterRequest,
    ) -> crate::ImResult<SyncThreadAfterResult>;
}
```

P1 原始范围不提供完整 mark-read / conversation projection；当前实现已经在后续阶段追加
`mark_read`、`mark_thread_read` 和 `conversations` 等能力。本文保留 P1 原始说明，
并在下文补充当前 mark-read 契约。当前仍不属于 P1 基线的能力：

```rust
attachments
secure direct
group E2EE
group lifecycle
```

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

    // Reserved for Phase 4. P1 returns UnsupportedCapability.
    Attachment {
        input: AttachmentInput,
        caption: Option<String>,
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
pub enum MessageSecurityPolicy {
    Default,
    Plaintext,

    // Reserved for Phase 6. P1 returns UnsupportedCapability.
    E2eeRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryOptions {
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

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

P1 `AttachmentInput` 只作为 reserved enum 形态存在。不要在 P1 实现 upload/download。

### 2.1 Delegated Signing Optional 扩展

当前 Agent IM MVP 在不改变 ANP `origin_proof` 结构的前提下，为普通 direct/default plain 发送增加 `delegated_signing` optional 参数。调用方不传该字段时，SDK 继续使用当前 identity/session 默认 sender 与默认 authentication key，老 Rust/Dart 调用行为不变。

`delegated_signing` 的字段语义：

| 字段 | 语义 |
|---|---|
| `logical_sender_did` | 消息业务发送者 DID，MVP 为用户 DID，例如 `did:wba:...:user:e1_xxx`。SDK 会把它写入 `meta.sender_did`。 |
| `signing_verification_method` | 用于签 `auth.origin_proof` 的 verification method，MVP 为 `user_did#daemon-key-1`。 |
| `signing_key_ref` | SDK 本地可解析的子私钥引用，例如 `file:/.../daemon-key-1.pem` 或 `local:daemon-key-1`。 |
| `actor_agent_did` | 可选审计字段，标识实际发起能力调用的 daemon/runtime agent；不改变 ANP proof 结构。 |

SDK 本地校验：

1. `logical_sender_did` 必须与 `signing_verification_method` 的 DID owner 一致。
2. `signing_verification_method` 必须能在对应 DID Document 的 `authentication` 或兼容 verification method 中找到。
3. `signing_key_ref` 必须能在本地解析到私钥。
4. Delegated send 只允许 direct 普通非 E2EE 消息：`DefaultPlain` / `Plain`。
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

CLI、Dart 和其他 facade 可以继续使用 `attachments().send(target, AttachmentSendRequest { security, ... })` 作为便捷入口；当 `security` 为 secure required 时，该入口内部复用上述 message high-level 路径，不拼 P7/P5/P6 wire。

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
    Unsupported { content_type: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    pub operation_id: Option<String>,
    pub delivery_state: Option<String>,
    pub server_sequence: Option<i64>,
    pub content_type: Option<String>,
    pub attributes: Vec<MessageMetadataAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadataAttribute {
    pub key: String,
    pub value: String,
}
```

`MessageMetadata` 只承载业务可解释的补充字段。不要把完整 wire payload 塞进 `metadata`。

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
    pub max_message_ids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkThreadReadResult {
    pub updated_count: u32,
    pub message_ids: Vec<crate::ids::MessageId>,
    pub local_candidate_count: u32,
    pub local_updated_count: u32,
    pub remote_updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub warnings: Vec<String>,
}
```

### 5.0 Local History 当前补充

`history(thread, query)` 保持远端 history + 本地 projection/reconcile 语义。性能敏感的首屏读取应使用 `local_history(thread, query)`：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalHistoryQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}
```

`local_history` 行为：

1. 只读取本地 `messages` projection，不访问 `direct.get_history`、`group.list_messages`、`inbox.get` 或目录 RPC。
2. 按 `owner_identity_id` 和 `ThreadRef` 解析出的 conversation ids 查询，返回最近消息，顺序为 newest-first。
3. `next_cursor` 是 SDK 生成的不透明 `local-history:v1:*` cursor，只能传回 `local_history` 翻页。
4. direct、group 和 raw thread ref 使用与 thread mark-read 一致的 owner-scoped conversation-id 归一化。
5. App 首屏应先显示 `local_history`，再后台调用 `history` 做远端 reconcile。

P1 不把 `mark_read` 放进 `InboxQuery`。当前实现把 mark-read 作为
`MessageService` 的显式方法，避免 inbox/history 查询和 read ack 语义耦合。

### 5.1 Mark Read / Thread Mark Read 当前补充

`mark_read(ids)` 保留按消息 id 确认已读的兼容语义。`mark_thread_read(request)`
用于会话打开后的性能敏感路径，行为是：

1. SDK 先在本地 `messages` projection 中按 `owner_identity_id` 和 `ThreadRef`
   查询 unread incoming `msg_id`，默认最多 500 条；`max_message_ids` 可调但硬上限
   仍为 500。
2. 该查询只读本地状态，不调用 `inbox()`、`history()`、`direct.get_history`、
   `group.list_messages` 或其他历史分页 RPC。
3. 查询到的本地候选 ids 会先调用本地 mark-read，把 `messages.is_read` 更新为
   `1`；direct 消息再通过 `inbox.mark_read` 做远端 best-effort ack。
4. group 和本地 mail notification 当前只做本地 mark-read；后续如果服务端提供
   thread-level read watermark，可以隐藏在同一个 public method 后面。
5. 远端 ack 失败不回滚本地已读，返回 `partial = true` 并在 `warnings` 中带失败原因；
   空 unread 会返回 `updated_count = 0` 且不访问远端。
6. `message_ids` 是本地查询到并尝试处理的候选 ids；`local_candidate_count`、
   `local_updated_count`、`remote_updated_count` 和 `remote_acknowledged` 用于上层记录
   best-effort 状态。

Step 04 引入 `conversation_summaries` 后，thread mark-read 还需要同步维护 summary
unread 字段；本方法的 public 契约不需要因此变化。

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
   projection，并在 apply 成功后写入 `next_event_seq` checkpoint。
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

checkpoint 边界：

- `sync_state`、checkpoint load/store、`since_event_seq` 注入、`next_event_seq` 推进只在
  `im-core` Rust/SQLite 内部。
- Public Rust、Dart、Flutter、App API 不暴露 `loadGlobalCheckpoint`、
  `storeGlobalCheckpoint`、手动 `since_event_seq` 或手动 checkpoint advance。
- Realtime `RealtimeSyncHint` 只用于 duplicate/gap/dirty 判断和调度 `sync_delta`；即使
  realtime projection 成功，也不得推进 checkpoint。

### 5.3 Delegated Inbox / History Optional 扩展

`InboxHistoryOptions` 是 Step 02 新增的 optional 参数，供 Daemon 使用 `user_did#daemon-key-1` 证明自己有权读取用户普通 inbox/history。调用方不传 `inbox_history_options` 时，SDK 继续走当前 identity/session 默认 inbox/history 读取逻辑，老调用行为不变。

MVP DID proof 主路径：

| 字段 | 语义 |
|---|---|
| `inbox_owner_did` | 被读取 inbox/history 的用户 DID。 |
| `inbox_auth_verification_method` | 用于签读取 proof 的用户 DID authentication key，MVP 为 `user_did#daemon-key-1`。 |
| `inbox_auth_key_ref` | SDK 本地可解析的子私钥引用。 |
| `inbox_auth` | 后续 token 路径预留。MVP 中传 `ScopedInboxToken` 会返回明确 unsupported，不影响 DID proof 主路径。 |

SDK 本地校验：

1. `inbox_owner_did` 必须与 `inbox_auth_verification_method` 的 DID owner 一致。
2. `inbox_auth_verification_method` 必须在 DID Document `authentication` 中有效。
3. `inbox_auth_key_ref` 必须能在本地解析到私钥。
4. Delegated inbox/history 只投影普通非 E2EE 消息。SDK 会过滤 direct/group E2EE opaque 消息，不返回 E2EE 明文、metadata projection 或 private state。
5. `ScopedInboxToken` 为后续优化，MVP 不作为主路径。

## 6. P1 Behavior

`send()` 行为：

```text
1. 校验 body。
   - Text 不能为空。
   - Attachment -> UnsupportedCapability("attachments")。
2. 校验 security。
   - Default / Plaintext 继续。
   - E2eeRequired -> UnsupportedCapability("e2ee")。
3. 通过 ImClient runtime 注入身份、auth、owner。
4. Direct target：做最小 PeerRef resolve。
5. Group target：使用已有 GroupRef，不做 group lifecycle。
6. ensure_session(AuthScope::Messaging 或 GroupMessaging)。
7. 构造 internal RPC/wire params。
8. 发送；session expired 时 refresh 后 retry once。
9. 远端结果 normalize 成 Message。
10. 必要时写入最小本地状态或兼容旧存储。
```

`inbox()` / `history()` 行为：

```text
1. 注入身份和 owner。
2. 按查询范围 ensure_session：direct 使用 Messaging，group 使用 GroupMessaging，All 需要两者。
3. DirectOnly 通过 inbox.get 拉取 direct inbox，并过滤掉任何异常混入的 group 消息。
4. GroupOnly 先 group.list 获取当前身份所在群，再按群调用 group.list_messages，合并成 group inbox 视图。
5. All 合并 DirectOnly 与 GroupOnly 的结果，按 message id 去重并应用 limit。
6. normalize 成 Page<Message>。
7. 不在 P1 强制做 conversation projection。
```

约束：`InboxHistoryOptions` 目前只支持 direct/delegated inbox；`GroupOnly` 如果传入 delegated inbox options，应返回明确 unsupported；`All` 带 delegated options 时保持兼容，只读取 direct/delegated inbox，不进入 group 子路径，避免 daemon 误把用户 delegated inbox proof 用于群消息读取。

## 7. Dart / Flutter Binding

`packages/awiki_im_core` 公开 API 与 Rust DTO 保持同名 optional 参数：

```dart
const SendTextRequest(
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
```

兼容性要求：

- `SendTextRequest` / `SendPayloadRequest` 的 `delegatedSigning` 默认 `null`。
- `MessageApi.inbox` / `MessageApi.history` 的 `inboxHistoryOptions` 默认 `null`。
- 老 Dart 调用不需要补参数；FRB 生成 DTO/API 只增加 nullable 字段和 nullable 函数参数。
