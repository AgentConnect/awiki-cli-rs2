# 切片 04：LocalStateDbActor

## 目标

将 SQLite 访问收口到专用 actor，避免 async service/runtime 直接阻塞 Tokio runtime。

本切片不是重写 local state。必须复用现有 `internal/local_state` 的 schema、migration、SQL 和 projection 语义。

## 依赖

依赖切片：

```text
slice-01-runtime-foundation.md
```

可并行参考：

```text
slice-03-identity-bootstrap-auth.md
```

## 当前代码锚点

重点复用和改造：

```text
crates/im-core/src/internal/local_state/**
crates/im-core/src/internal/message_runtime/local_projection.rs
crates/im-core/src/internal/group_runtime/projection.rs
crates/im-core/src/internal/contact_store/**
crates/im-core/src/internal/secure_direct/sqlite_store.rs
crates/im-core/src/internal/group_e2ee/storage.rs
```

## 设计要求

1. Actor 拥有唯一 writer connection。

   ```text
   LocalStateDbActor
     - owns rusqlite::Connection
     - runs on dedicated blocking thread or spawn_blocking loop
     - serializes writes
     - exposes cloneable async handle
   ```

2. 保留 SQLite 配置：

   ```text
   WAL
   foreign_keys=ON
   busy_timeout
   bundled sqlite
   schema version
   migrations
   ```

3. Command 必须是 typed command，不使用任意 SQL 字符串穿透 service layer。

4. Actor 内部复用现有函数。

   例如：

   ```text
   schema::ensure_schema
   schema::current_schema_version
   messages::upsert_message / upsert_messages
   groups::upsert_group / upsert_group_member
   conversations::list_conversations
   contacts projection helpers
   e2ee session/outbox helpers
   ```

5. Service/runtime 不直接打开 connection。

   禁止在这些路径直接使用：

   ```text
   rusqlite::Connection::open
   internal::local_state::open_writable
   internal::contact_store::open_writable
   ```

## 执行步骤

1. 新增 actor 和 handle。

   推荐位置：

   ```text
   crates/im-core/src/internal/local_state/actor.rs
   crates/im-core/src/internal/local_state/command.rs
   crates/im-core/src/internal/local_state/handle.rs
   ```

2. 在 `ImCoreInner` 或等价 runtime state 中挂载 `LocalStateStore` handle。

   需要考虑：

   ```text
   - ImCore clone 行为
   - actor shutdown
   - tests 使用 temp sqlite path
   - sqlite feature 关闭时的 unsupported/noop behavior
   ```

3. 把 schema init/migration 命令接入 actor。

4. 把 message local projection 命令接入 actor。

   覆盖：

   ```text
   persist outgoing direct
   persist outgoing group
   persist incoming messages
   inbox/history projection
   conversations
   mark_read local state
   attachment message projection
   ```

5. 把 group/contact/e2ee/outbox 需要的命令补齐。

6. 为 actor 添加 shutdown 行为。

   要求：

   ```text
   - normal shutdown drains accepted commands when appropriate
   - disposed/closed actor returns deterministic error
   - no task leak in tests
   ```

7. 添加 concurrency tests。

   覆盖：

   ```text
   - concurrent write commands serialize
   - owner_identity_id / owner_did isolation
   - actor closes cleanly
   - database locked 不因多 writer 触发
   ```

## 上层同步

本切片原则上只影响 `im-core` 内部。

如果 local state status/migration public result 改变，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/core.rs
crates/im-core-dart/src/dto/*
packages/awiki_im_core/lib/src/models/*
```

## 测试

本切片必须运行：

```bash
cargo test -p im-core local_state --locked
cargo test -p im-core db_actor --locked
cargo check -p im-core --locked
```

硬性 grep：

```bash
rg "rusqlite::Connection|open_writable|Connection::open" crates/im-core/src/messages crates/im-core/src/groups crates/im-core/src/directory crates/im-core/src/realtime crates/im-core/src/attachments crates/im-core/src/secure
```

预期：

```text
service layer 没有直接 SQLite 访问。
```

允许位置：

```text
crates/im-core/src/internal/local_state/**
tests/**
```

如果 `internal/contact_store` 或 legacy compat 仍有临时直接连接，必须登记到切片 13 清理。

## 验收

```text
1. LocalStateDbActor/handle 可用。
2. actor 内部复用现有 SQL/schema/projection 函数。
3. service/runtime 不直接打开 SQLite。
4. 并发写入通过 actor 串行化。
5. schema version、owner isolation、migration 语义不变。
```

## 完成报告

报告必须包含：

```text
- actor 模块和 handle 接入点
- 已迁移到 actor 的 command 列表
- 尚未迁移的 SQLite 直接访问及后续切片
- local_state/db_actor 测试结果
- grep 围栏结果
```
