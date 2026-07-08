# 切片 05：Messages 异步化

## 目标

将 direct/group message 的发送、读取、mark_read、conversations 和 local projection 转换为 async。

本切片必须复用现有 message DTO、wire builder 和 runtime 逻辑，不重写消息协议。

## 依赖

依赖切片：

```text
slice-02-async-http-transport.md
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/messages/service.rs
crates/im-core/src/messages/dto.rs
crates/im-core/src/internal/message_runtime/**
crates/im-core/src/internal/wire/direct.rs
crates/im-core/src/internal/wire/group.rs
crates/im-core/src/internal/wire/inbox.rs
crates/im-core/src/internal/wire/history.rs
crates/im-core/src/internal/transport.rs
```

## 设计要求

1. Public service methods async 化：

   ```rust
   pub async fn send(&self, request: SendMessageRequest) -> ImResult<SendMessageResult>;
   pub async fn inbox(&self, query: InboxQuery) -> ImResult<Page<Message>>;
   pub async fn inbox_with_metadata(&self, query: InboxQuery) -> ImResult<MessagePage>;
   pub async fn history(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<Page<Message>>;
   pub async fn history_with_metadata(&self, thread: ThreadRef, query: HistoryQuery) -> ImResult<MessagePage>;
   pub async fn mark_read(&self, ids: Vec<MessageId>) -> ImResult<MarkReadResult>;
   pub async fn conversations(&self, query: ConversationQuery) -> ImResult<Page<Conversation>>;
   ```

2. Service getter 保持同步：

   ```rust
   client.messages()
   ```

3. 保留 message DTO 和 JSON-RPC payload 语义。

4. Handle-to-DID resolution 改为 async directory lookup。

5. Local projection 改走 DB actor。

6. Secure direct/group E2EE 路径至少 async-compatible。

   如果完整 E2EE 改造在切片 09 完成，本切片要避免阻塞后续接入。

7. Cancellation 语义诚实。

   ```text
   cancellation 可以停止等待或停止未提交本地工作；
   已提交到服务端的 send 不能声称撤回。
   ```

8. Idempotency 不引起重复发送。

   如果现有协议支持 `client_message_id` / idempotency key，应保持或补齐测试。

## 执行步骤

1. 将 `MessageService` 方法改为 async。

2. 将 `internal/message_runtime/direct.rs`、`group.rs`、`read.rs`、`mark_read.rs`、`conversations.rs` 中触达 session/transport/DB 的方法改为 async。

3. 保留现有 validation 函数同步执行。

   例如：

   ```text
   validate_body
   validate_send_mode
   validate_attachment_security
   message_kind_from_result
   DTO conversion
   ```

4. 将 `CoreHttpTransport` 调用改为 `.await`。

5. 将 `persist_*` local projection 改为 actor command `.await`。

6. 对 handle target 统一通过 async directory service 解析。

7. 增加 fake transport tests。

   覆盖：

   ```text
   direct send payload
   group send payload
   inbox payload
   history payload
   mark_read payload
   401 retry remains in transport layer
   ```

8. 增加 cancellation/idempotency tests。

## 上层同步

如果 `MessageService` public methods 改为 async，必须同步所有直接调用者：

```text
crates/awiki-cli/src/m_core_cli_adapter/messages.rs
crates/awiki-cli/src/cli_shell/msg_handlers.rs
crates/im-core-dart/src/api/messages.rs
packages/awiki_im_core/lib/src/**
```

如果 CLI/Dart 计划留到切片 11/12，则本切片可以只让 `im-core` 通过，但必须明确记录 workspace 编译暂时失败原因。

## 测试

本切片必须运行：

```bash
cargo test -p im-core messages --locked
cargo test -p im-core direct_send --locked
cargo test -p im-core group_send --locked
cargo test -p im-core inbox --locked
cargo test -p im-core history --locked
cargo check -p im-core --locked
```

稳定性测试必须覆盖：

```text
- direct send payload golden
- group send payload golden
- handle target 通过 server resolve
- 拒绝 empty text
- 保留 unsupported attachment/security 行为
- 保留 inbox/history pagination
- local projection 持久化 owner_did 作用域行
- server submission 后 cancellation 不声称 recall
```

## 验收

```text
1. MessageService I/O methods 是 async。
2. wire payload 和 DTO 语义不变。
3. local projection 使用 DB actor。
4. handle resolution 使用 async directory path。
5. 上层调用者已同步或登记到切片 11/12。
```

## 完成报告

报告必须包含：

```text
- async 化的方法列表
- payload golden tests 列表
- local projection 迁移状态
- cancellation/idempotency 测试状态
- CLI/Dart 同步状态
- 已运行测试命令和结果
```
