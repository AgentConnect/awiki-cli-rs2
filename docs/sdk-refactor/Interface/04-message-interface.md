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
    pub security: MessageSecurityMode,
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

    // Reserved for Phase 6. P1 returns UnsupportedCapability.
    SecureDirect,
    GroupE2ee,
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

P1 对远端返回不稳定字段可以保留在 `raw`：

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageRaw {
    pub value: serde_json::Value,
}
```

但 `SendMessageResult` 主字段必须是领域字段，不能只返回 `serde_json::Value`。

## 4. Message DTO

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub raw: Option<serde_json::Value>,
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
```

P1 允许 `raw` 用来兼容当前远端响应，但 CLI/App 不应依赖 raw 作为主业务字段。

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
   - DefaultPlain / Plain 继续。
   - SecureDirect -> UnsupportedCapability("secure-direct")。
   - GroupE2ee -> UnsupportedCapability("group-e2ee")。
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

## 7. CLI Mapping

| CLI 输入 | SDK DTO |
| --- | --- |
| `msg send --to bob --text hello` | `SendMessageRequest { target: Direct(PeerRef::parse("bob")), body: Text { ... } }` |
| `msg send --group did:... --text hello` | `SendMessageRequest { target: Group(GroupRef::parse(...)), body: Text { ... } }` |
| `--text-file path` | CLI 读取文件后传 `MessageBody::Text`。SDK 不接收 text file path。 |
| `--secure` | P1 映射为 `SecureDirect` 后 SDK 返回 `UnsupportedCapability`，或 CLI 直接提示 unsupported。 |
| `--file` | P1 不进入 `send()`；Phase 4 再支持。 |

## 8. Forbidden Public API

P1 `im-core::messages` 不允许公开：

```rust
build_direct_send_rpc_params
build_group_send_rpc_params
build_inbox_rpc_params
build_history_rpc_params
send_direct_http_with_fallback_refresh
store_message
owner_did
LocalStatePaths
ActorContext
message::types::SendRequest
```
