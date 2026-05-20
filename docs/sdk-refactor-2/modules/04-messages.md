# 模块设计：messages

## 1. 职责

`messages` 是第一阶段核心模块，负责普通私聊和群聊文本消息：

- direct text send。
- group text send。
- inbox。
- direct/group history。
- mark-read。
- conversation/thread projection。
- 远端结果与本地状态合并。

第一阶段不处理 E2EE，不处理附件完整 upload/download。

## 2. 对外接口

```rust
impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
    pub fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
    pub fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
    pub fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
}
```

## 3. SendMessageRequest

```rust
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

pub enum MessageBody {
    Text { text: String, kind: MessageKind },
    Attachment { input: AttachmentInput, caption: Option<String>, mime_type: Option<String> }, // Phase 2
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
    SecureDirect, // Phase 3，第一阶段 UnsupportedCapability
    GroupE2ee,    // Phase 3，第一阶段 UnsupportedCapability
}
```

## 4. 不暴露内容

以下都必须是 internal：

```rust
build_direct_send_rpc_params
build_group_send_rpc_params
build_inbox_rpc_params
build_history_rpc_params
send_direct_http
send_direct_http_with_fallback_refresh
persist_inbox_messages(owner, paths, raw)
MessageRecord store row
```

## 5. conversation projection

第一阶段必须提供：

```rust
client.messages().conversations(query)
```

原因：App 和 CLI 都需要 conversation/thread 列表。如果 SDK 不提供，App 会继续自己解析 inbox、计算 thread id、合并本地 cache，导致业务规则重复。

## 6. CLI adapter

CLI 负责转换：

```text
--to       -> MessageTarget::Direct
--group    -> MessageTarget::Group
--text     -> MessageBody::Text
--text-file -> CLI 先读取文件再传 text
--secure on -> MessageSecurityMode::SecureDirect，但第一阶段返回 UnsupportedCapability
```

## 7. 第一阶段验收

- `msg send --to` 走 SDK。
- `msg send --group` 走 SDK。
- `msg inbox/history/mark-read` 走 SDK。
- 普通消息本地落库和 conversation projection 正常。
- secure mode 在第一阶段明确报 `UnsupportedCapability`，不静默降级。
