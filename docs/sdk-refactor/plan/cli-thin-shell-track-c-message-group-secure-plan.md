# Track C: Message, Group, Attachment, and Secure Legacy Cleanup

**并行分支**：`cutover/thin-shell-track-c-message-group-secure`  
**依赖**：可直接开始；完全删除共享 legacy module 需等 Final。  
**目标**：让 `msg.*`、`group.*`、`msg.attachment.*`、`msg.secure.*`、`group.secure.*` 默认路径只通过 `im-core` public API；旧 `awiki-cli/src/message` 只保留临时测试兼容，最终由 Final 删除。

---

## 1. 范围

本 track 处理：

```text
msg.send
msg.inbox
msg.history
msg.mark-read
msg.attachment.download
group.create/get/join/add/remove/leave/update/list/members/messages
msg.secure.status/repair
group.secure.status/repair
group.e2ee deprecated aliases and internal commands
message/group/attachment/secure adapter cleanup
old awiki-cli message tests migration
```

主要文件：

```text
crates/awiki-cli/src/app/msg_handlers.rs
crates/awiki-cli/src/app/group_handlers.rs
crates/awiki-cli/src/app/group_e2ee_handlers.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
crates/awiki-cli/src/im_core_adapter/groups.rs
crates/awiki-cli/src/im_core_adapter/message_result.rs
crates/awiki-cli/src/message/*
crates/awiki-cli/tests/*msg*
crates/awiki-cli/tests/*message*
crates/awiki-cli/tests/*group*
crates/awiki-cli/tests/*secure*
crates/im-core/src/messages/*
crates/im-core/src/groups/*
crates/im-core/src/attachments/*
crates/im-core/src/secure/*
crates/im-core/tests/*message*
crates/im-core/tests/*group*
crates/im-core/tests/*attachment*
crates/im-core/tests/*secure*
```

不处理：

```text
runtime listener event projection
store module deletion
identity/auth build boundary
mail/page/site cleanup
Cargo dependency deletion
```

---

## 2. 边界目标

CLI adapter 允许：

```text
ParsedCommand -> SendMessageRequest / InboxQuery / HistoryQuery / Group DTO
input file read for --text-file / --file
output file path validation and permission after download
dry-run plan
render old-compatible CLI envelope
ImError -> ExitError mapping
```

CLI adapter 禁止：

```text
crate::message::* business fallback
old SendRequest / InboxRequest / HistoryRequest DTO bridge
manual target DID resolve when im-core service can resolve
manual auth retry / JWT refresh
direct local_state projection
im_core::compat wire builders as default execution path
secure prekey / ciphertext / KeyPackage / MLS provider internals in default API
```

---

## 3. 执行步骤

### C1. msg.send / attachment 只走 im-core

确认：

```text
text send -> client.messages().send(...)
attachment send -> client.attachments().send(...)
attachment download -> client.attachments().download(...)
secure send -> client.messages().send(... security=E2eeRequired ...)
```

删除或迁移：

```text
message/service.rs direct/group send path
message/attachment.rs and attachment_service.rs default path
message/wire.rs proof/wire builders as CLI dependencies
```

短期允许这些文件存在，但 app/adapter/runtime 默认路径不得引用它们。

### C2. msg.inbox/history/mark-read/conversations 收敛

默认路径：

```text
client.messages().inbox_with_metadata(...)
client.messages().history_with_metadata(...)
client.messages().mark_read(...)
client.messages().conversations(...)
```

不支持的旧 flags：

```text
返回 field-level unsupported
不回退旧 inbox/history implementation
```

如果 CLI 输出还需要旧 shape，在 adapter render 层重组，不在 CLI 重新做本地 query/projection。

### C3. group lifecycle 全走 client.groups()

默认路径：

```text
client.groups().create(...)
client.groups().get(...)
client.groups().join(...)
client.groups().add_member(...)
client.groups().remove_member(...)
client.groups().leave(...)
client.groups().update(...)
client.groups().list(...)
client.groups().members(...)
client.groups().messages(...)
```

禁止：

```text
message/group_service.rs business fallback
message/group_wire.rs raw group wire patch as default path
group.code.* default surface
```

### C4. secure direct / group secure 命令面收窄

默认保留高层：

```text
msg send --secure required
msg secure status
msg secure repair
group secure status
group secure repair
```

默认隐藏或 unsupported：

```text
msg.secure.failed / retry / drop
msg.secure.outbox.*
group.e2ee.publish-key-package
group.e2ee.pending
group.e2ee.update-key
group.e2ee.rejoin
group.e2ee.recover-member
group.e2ee.process-leave-request
```

`group.e2ee.status` 和 `group.e2ee.repair` 可以作为 deprecated alias 指向 `group secure status/repair`，但不应再展示 provider binary、MLS data dir、KeyPackage 等底层计划。

### C5. 旧 message tests 迁移

分类：

```text
wire / payload contract -> crates/im-core/tests
CLI output contract -> crates/awiki-cli/tests, but call CLI high-level command
legacy-only behavior -> delete or move behind diagnostic/migration feature
```

删除前检查：

```bash
rg "awiki_cli::message|crate::message|use awiki_cli::message" crates/awiki-cli/tests crates/im-core/tests
```

### C6. 留给 Final 的删除准备

本 track 可以删除已孤立的 message leaf files，但不要强行删除共享 module root 如果会冲突。最终删除留给 Final：

```text
crates/awiki-cli/src/message/*
crates/awiki-cli/src/lib.rs pub mod message
Cargo dependencies only used by old message
```

---

## 4. 验证

最小验证：

```bash
cargo test -p im-core
cargo check -p awiki-cli
rg "crate::message::|use crate::message\\b|awiki_cli::message|im_core::compat" \
  crates/awiki-cli/src/app \
  crates/awiki-cli/src/im_core_adapter \
  crates/awiki-cli/tests
```

推荐测试：

```bash
cargo test -p im-core --test message_contract
cargo test -p im-core --test group_contract
cargo test -p im-core --test attachment_contract
cargo test -p im-core --test secure_contract
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
```

如果 test target 当前不存在，按现有命名运行对应测试，不把不存在 target 当 blocker。

---

## 5. 完成定义

本 track 完成后：

```text
1. msg/group/attachment/secure 默认 handler 不再调用 crate::message。
2. im_core_adapter/messages.rs 和 groups.rs 不再做 legacy request bridge、manual auth retry、local projection。
3. 旧 awiki-cli message tests 已迁移或删除。
4. group.e2ee 低层命令不进入默认命令面。
5. Final 可以删除 awiki-cli/src/message 及相关依赖。
```

