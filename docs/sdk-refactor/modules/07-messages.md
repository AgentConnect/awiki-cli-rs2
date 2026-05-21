# messages 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P1 direct/group text send + 必要 inbox/history；P3 补全 mark-read、conversation projection、本地状态合并。  
**职责**：消息发送、收件箱、历史、已读、本地投影。

## 1. 目标

`messages` 覆盖当前 `msg.*` 的业务能力。请求模型必须表达业务意图，而不是 CLI flag。

## 2. P1 职责

- `send(SendMessageRequest) -> SendMessageResult`。
- direct text 发送。
- group text 发送，面向已有 `GroupRef`。
- `inbox(InboxQuery) -> Page<Message>` 的必要子集。
- `history(ThreadRef::Direct | ThreadRef::Group, HistoryQuery) -> Page<Message>` 的必要子集。
- auth ensure + 401 refresh retry。
- DID/handle 的最小 target resolve。
- RPC params 构造作为内部 helper。
- `SecureDirect` / `GroupE2ee` / `Attachment` 返回 `UnsupportedCapability`。

P1 不要求完整 conversation projection、mark-read、本地 cache merge。

## 3. P3 职责

- `mark_read(MessageIds)`。
- `conversations(ConversationQuery)`。
- message content type 规范化。
- inbox/history 远端结果与本地 cache 合并。
- 收到 direct/group notification 后投影成本地 message record。
- 失败重试和消息状态。

## 4. DTO 草案

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

    // P4+
    Attachment {
        input: AttachmentInput,
        caption: Option<String>,
        mime_type: Option<String>,
    },
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,

    // P6+
    SecureDirect,
    GroupE2ee,
}

pub enum ThreadRef {
    Direct(PeerRef),
    Group(GroupRef),
    Thread(ThreadId), // P3+
}
```

不应把 `--to`、`--group`、`--text-file`、`--file` 作为 core API 字段。那些是 CLI 输入形态，CLI 解析后转换成 `MessageTarget` 和 `MessageBody`。

## 5. 接口草案

```rust
pub struct MessageService<'a> {
    client: &'a ImClient,
}

impl MessageService<'_> {
    pub fn send(
        &self,
        request: SendMessageRequest,
    ) -> ImResult<SendMessageResult>;

    pub fn inbox(
        &self,
        query: InboxQuery,
    ) -> ImResult<Page<Message>>;

    pub fn history(
        &self,
        thread: ThreadRef,
        query: HistoryQuery,
    ) -> ImResult<Page<Message>>;

    // P3+
    pub fn mark_read(
        &self,
        ids: Vec<MessageId>,
    ) -> ImResult<MarkReadResult>;

    // P3+
    pub fn conversations(
        &self,
        query: ConversationQuery,
    ) -> ImResult<Page<Conversation>>;
}
```

## 6. 内部 helper

可以保留内部 notification 投影和本地状态合并接口，但不作为 SDK 主入口：

```rust
impl MessageServiceInternal {
    pub(crate) fn project_notification(
        &self,
        owner: LocalOwnerContext,
        event: MessageNotification,
        local_state: &LocalStatePaths,
    ) -> ImResult<Vec<MessageRecord>>;
}
```

## 7. CLI 边界

CLI 负责：

- `--to`、`--group`、`--text-file`、`--file` 参数解析；
- dry-run 呈现；
- table/pretty/json 输出；
- 把 `--identity` 转为 `IdentitySelector` 并构造 `ImClient`。

消息业务规则、远端结果与本地 cache 合并、已读状态更新归 `messages`。
