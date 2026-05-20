# messages 模块接口设计

**所属 crate**：`crates/im-core`  
**模块职责**：消息发送、收件箱、历史、已读、本地投影。

## 1. 目标

`messages` 覆盖当前 `msg.*` 的业务能力。请求模型必须表达业务意图，而不是 CLI flag。

## 2. 主要职责

- `send(actor, SendMessageRequest) -> SendMessageResult`。
- direct text / group text 统一发送。
- `send_secure_direct(...)` 入口可在这里路由到 `secure`。
- `inbox(actor, InboxQuery) -> MessagePage`。
- `history(actor, ThreadRef, HistoryQuery) -> MessagePage`。
- `mark_read(actor, MessageIds)`。
- message content type 规范化。
- inbox/history 远端结果与本地 cache 合并。
- 收到 direct/group notification 后投影成本地 message record。

## 3. DTO 草案

```rust
pub struct SendMessageRequest {
    pub target: MessageTarget,
    pub body: MessageBody,
    pub security: MessageSecurityMode,
    pub client_message_id: Option<String>,
}

pub enum MessageTarget {
    Direct(PeerRef),
    Group(GroupRef),
}

pub enum MessageBody {
    Text { text: String, kind: MessageKind },
    AttachmentManifest { manifest: AttachmentManifest, caption: String },
}

pub enum ThreadRef {
    Direct(PeerRef),
    Group(GroupRef),
}
```

不应把 `--to`、`--group`、`--text-file`、`--file` 作为 core API 字段。那些是 CLI 输入形态，CLI 解析后转换成 `MessageTarget` 和 `MessageBody`。

## 4. 接口草案

```rust
pub struct MessageService<'a> {
    core: &'a ImCore,
}

impl MessageService<'_> {
    pub async fn send(
        &self,
        actor: ActorContext,
        request: SendMessageRequest,
    ) -> ImResult<SendMessageResult>;

    pub async fn inbox(
        &self,
        actor: ActorContext,
        query: InboxQuery,
    ) -> ImResult<MessagePage>;

    pub async fn history(
        &self,
        actor: ActorContext,
        thread: ThreadRef,
        query: HistoryQuery,
    ) -> ImResult<MessagePage>;

    pub async fn mark_read(
        &self,
        actor: ActorContext,
        ids: Vec<MessageId>,
    ) -> ImResult<MarkReadResult>;

    pub fn project_notification(
        &self,
        owner: Did,
        event: MessageNotification,
        local_state: &LocalStatePaths,
    ) -> ImResult<Vec<MessageRecord>>;
}
```

## 5. CLI 边界

CLI 负责：

- `--to`、`--group`、`--text-file`、`--file` 参数解析；
- dry-run 呈现；
- table/pretty/json 输出；
- 当前 identity 和 SQLite 路径传入。

消息业务规则、远端结果与本地 cache 合并、已读状态更新归 `messages`。
