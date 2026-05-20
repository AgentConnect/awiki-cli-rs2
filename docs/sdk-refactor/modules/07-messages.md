# messages 模块接口设计

**阅读顺序**：07 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：消息发送、收件箱、历史、已读、本地投影。

## 1. 目标

`messages` 覆盖当前 `msg.*` 的业务能力。请求模型必须表达业务意图，而不是 CLI flag。

## 2. 主要职责

- `send(SendMessageRequest) -> SendMessageResult`。
- direct text / group text 统一发送。
- `send_secure_direct(...)` 入口可在这里路由到 `secure`。
- `inbox(InboxQuery) -> MessagePage`。
- `history(ThreadRef, HistoryQuery) -> MessagePage`。
- `mark_read(MessageIds)`。
- message content type 规范化。
- inbox/history 远端结果与本地 cache 合并。
- 收到 direct/group notification 后投影成本地 message record。

公开接口挂在 `ImClient` 上，自动使用 client 绑定的 actor、auth session 和 local state owner。调用方不应传 `ActorContext`、`LocalStatePaths` 或 owner DID。

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
    client: &'a ImClient,
}

impl MessageService<'_> {
    pub async fn send(
        &self,
        request: SendMessageRequest,
    ) -> ImResult<SendMessageResult>;

    pub async fn inbox(
        &self,
        query: InboxQuery,
    ) -> ImResult<MessagePage>;

    pub async fn history(
        &self,
        thread: ThreadRef,
        query: HistoryQuery,
    ) -> ImResult<MessagePage>;

    pub async fn mark_read(
        &self,
        ids: Vec<MessageId>,
    ) -> ImResult<MarkReadResult>;
}
```

内部 helper 允许保留 notification 投影和本地状态合并接口，但不作为 SDK 主入口：

```rust
impl MessageServiceInternal {
    pub(crate) fn project_notification(
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
- 把 `--identity` 转为 `IdentitySelector` 并构造 `ImClient`。

消息业务规则、远端结果与本地 cache 合并、已读状态更新归 `messages`。
