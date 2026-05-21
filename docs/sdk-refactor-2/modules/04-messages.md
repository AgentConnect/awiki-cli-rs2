# 04-messages：Phase 1 私聊与群聊文本 MVP

## 1. 目标

`messages` 是 Phase 1 的核心业务模块。第一阶段只做普通文本消息主链路：私聊文本、群聊文本，以及验证消息闭环所需的 inbox/history。

不在 Phase 1 中做附件、mark-read、conversation projection、secure direct、group E2EE。

## 2. Phase 1 public API

```rust
pub struct MessageService<'a> {
    client: &'a ImClient,
}

impl MessageService<'_> {
    pub fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
    pub fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
    pub fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
}
```

## 3. Phase 1 request DTO

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

pub enum ThreadRef {
    Direct(PeerRef),
    Group(GroupRef),
}

pub enum MessageBody {
    Text { text: String, kind: MessageKind },
}

pub enum MessageSecurityMode {
    DefaultPlain,
    Plain,
}
```

Phase 1 不要求公开 attachment body 或 secure mode。若为了未来兼容提前保留 enum variant，调用时必须返回 `UnsupportedCapability`。

## 4. 内部编排

`send()` 内部负责：

- 通过 `ImClient` 注入当前 identity/actor。
- `auth().ensure_session(AuthScope::Messaging | GroupMessaging)`。
- Direct target 的最小 handle/DID 解析。
- Group target 使用已有 `GroupRef`。
- 选择 HTTP/RPC transport。
- 构造 wire params。
- 处理 401 refresh retry。
- 返回领域化 `SendMessageResult`。
- 可选地写入最小本地 message record。

CLI/App 不应该传 actor、auth path、owner DID、RPC params 或 raw payload。

## 5. 后续阶段

Phase 3 补全：

```text
mark_read
conversations/thread projection
local cache merge
message status / retry
更多 inbox/history query
```

Phase 4 增加附件。Phase 6 增加 secure direct 和 group E2EE。

## 6. CLI 边界

CLI 负责：

- `--to`、`--group`、`--text`、`--text-file` 解析。
- dry-run 展示。
- 输出格式。
- `--identity` 转成 `IdentitySelector`。

CLI 不再直接调用 `message::send(&resolved, &manager, message::SendRequest { identity_name, ... })`。

## 7. 完成判定

- `msg send --to` 和 `msg send --group` 均走 `client.messages().send()`。
- `msg inbox/history` 走 `client.messages()`。
- `group messages` 可适配到 `client.messages().history(ThreadRef::Group)`。
- `build_*_rpc_params` 不作为 SDK public API 导出。
