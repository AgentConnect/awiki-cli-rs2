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
}
```

P1 不把 `mark_read` 放进 `InboxQuery`。当前 CLI 若有 `--mark-read`，P1 adapter 可以继续走旧逻辑或返回 unsupported；完整 mark-read 放 Phase 3。

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
2. ensure_session。
3. 构造 internal RPC/wire params。
4. 拉取远端必要子集。
5. normalize 成 Page<Message>。
6. 不在 P1 强制做 conversation projection。
```
