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
}
```

P1 不提供：

```rust
mark_read
conversations
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
```

P1 不把 `mark_read` 放进 `InboxQuery`。当前 CLI 若有 `--mark-read`，P1 adapter 可以继续走旧逻辑或返回 unsupported；完整 mark-read 放 Phase 3。

### 5.1 Delegated Inbox / History Optional 扩展

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
