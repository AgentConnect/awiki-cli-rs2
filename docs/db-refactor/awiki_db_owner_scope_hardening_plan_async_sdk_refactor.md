
# AWiki 本地 SQLite 所有者作用域彻底重构方案

状态：方案草案
目标仓库：`awiki-cli-rs2`
目标分支上下文：面向 `feature/release-0526/db-refactor-in-async`。如果本地暂时无法读取该分支，先同步分支，再重新检查本文列出的文件。
核心原则：**AWiki 还没有上线，因此这次改动应在生产数据出现前彻底消除本地数据库所有者作用域问题。不要保留会制造线上数据债的弱兼容模型。**

---

## 1. 摘要

上一版短期加固方案假设当前本地数据库需要大体兼容旧的 `owner_did` 主键模型。现在这个原则不再适合。

由于产品还没有上线，正确目标应该是**在表结构层面修正模型**：

```text
owner_identity_id = 唯一稳定的所有者 / 分区键
owner_did         = 当前 DID 快照 / 兼容字段 / 展示字段
```

所有本地业务表都必须按 `owner_identity_id` 建键和查询，而不是按 `owner_did`。

`owner_did` 不允许参与主键、唯一键、应用可见的会话身份，也不允许承担所有者隔离语义。DID 替换或恢复不应要求把业务行从一个所有者分区移动到另一个所有者分区。它只应该更新身份记录、DID 历史记录，并在必要时刷新冗余的 `owner_did` 字段。

目标结果是：

- 活跃运行时查询不再依赖旧的 `owner_did` 回退条件。
- 应用和命令行都不手写所有者查询条件。
- 活跃业务表不再把 `owner_did` 作为主键组成部分。
- 本地状态始终通过 `OwnerScope { owner_identity_id, owner_did, device_id? }` 作用域访问。
- DID 恢复或替换不会制造重复联系人、重复群、重复消息或重复关系事件。
- 现有开发库和旧库在进入生产前被迁移、合并，或者明确备份后重建。

---

## 2. 为什么旧的渐进式方案不够

上一轮评审识别出的风险是真实存在的：

- `contacts`、`messages`、`groups`、`group_members`、`contact_handle_bindings` 的物理主键仍以 `owner_did` 为核心。
- 运行时代码正在转向 `owner_identity_id`。
- DID 替换或恢复可能造成旧 DID / 新 DID 两份行。
- `relationship_events(event_id)` 是全局唯一键。
- 私聊 `thread_id` 包含本地 owner DID，owner DID 改变后会拆分同一会话。
- 一些旧路径仍使用 `UPDATE OR IGNORE` 做 rebind。

渐进修复可以降低损害，但仍然会留下两个互相竞争的所有者模型：

```text
逻辑所有者 = owner_identity_id
物理所有者 = owner_did
```

当应用、命令行和监听器共享同一个本地状态库时，这个双模型会变得危险。既然 AWiki 还没有上线，现在应该直接移除这个分裂模型。

---

## 3. 目标

### 3.1 让 `owner_identity_id` 成为标准分区键

每个本地业务表都必须有 `owner_identity_id TEXT NOT NULL`。

主键和唯一索引必须包含 `owner_identity_id`，不能包含 `owner_did` 作为所有者分区键。

### 3.2 让 DID 替换不破坏业务数据

DID 替换或恢复不应该移动业务行的所有权。

一次 DID 变化应更新：

- 身份注册表或身份存储；
- 本地 DID 历史表；
- 可选的冗余 `owner_did` 快照字段；
- 仅当密钥材料确实变化时，才更新端到端加密材料。

它不能制造重复会话、重复联系人、重复群行或重复消息。

### 3.3 支持应用、命令行和监听器共用一套本地状态契约

`im-core` 拥有本地状态业务逻辑。命令行和应用不能构造裸 SQLite 所有者查询条件。

所有面向 SDK 的本地状态调用都必须通过 `im-core` 的接口以及内部 `LocalStateDb` / 存储辅助模块。

### 3.4 优先重建表结构，而不是保留线上兼容债

对现有开发库或测试库：

- 先创建备份；
- 可行时执行确定性迁移和合并；
- 不可行时清晰失败，并给出重置或重建本地库的指引。

不要把无限期运行时回退逻辑带入生产。

### 3.5 保持端到端加密私有状态属于客户端

私聊端到端加密和群组 MLS 私有状态仍然属于本地客户端。服务端端到端加密状态仍是 opaque 透明转发。安全表必须按 `owner_identity_id` 建键；适用时还要带 `device_id`。

---

## 4. 非目标

本方案不要求：

- 迁移服务端多租户数据库；
- 改变 `user-service` 的身份真相源；
- 改变 `message-service` v2 的存储模型；
- 改变 ANP 协议；
- 支持两个本地 live identity 共用同一个 DID；
- 永久保留旧本地库行为；
- 向应用或命令行暴露裸 SQLite 访问。

因为系统还没有上线，本方案允许一次性开发库迁移或本地数据库重置路径。

---

## 5. 必须采用的所有者模型

引入或集中一个内部所有者作用域类型：

```rust
pub(crate) struct OwnerScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) device_id: Option<String>,
}
```

规则：

1. 所有活跃本地状态读写路径都必须提供 `owner_identity_id`。
2. `owner_did` 是当前身份的属性，不是所有者分区键。
3. `device_id` 用于绑定设备的加密状态，例如群组 MLS 或未来按设备隔离的私聊端到端加密材料。
4. 存储函数应接收 `OwnerScope` 或明确的身份作用域 DTO，不应接收分散的自由字符串 `owner_identity_id` / `owner_did`。
5. 运行时代码中，空的 `owner_identity_id` 是非法输入。
6. 旧 owner 回退查询只允许存在于一次性迁移或导入辅助模块中。

---

## 6. 目标表结构模型

使用新的 schema 版本，例如 `SCHEMA_VERSION = 17`，或当前分支之后的下一个可用版本。

### 6.1 `identity_did_history`

新增本地身份 DID 历史表：

```sql
CREATE TABLE IF NOT EXISTS identity_did_history (
    owner_identity_id TEXT NOT NULL,
    did               TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'current', -- current | previous | revoked
    first_seen_at     TEXT NOT NULL,
    last_seen_at      TEXT NOT NULL,
    metadata          TEXT,
    PRIMARY KEY(owner_identity_id, did)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_did_history_current
    ON identity_did_history(owner_identity_id)
    WHERE status = 'current';
```

可选索引：

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_did_history_live_did_unique
    ON identity_did_history(did)
    WHERE status = 'current';
```

如果身份注册表已经保证 live DID 唯一，数据库索引可以只作为诊断辅助。但这个不变量必须被测试覆盖。

### 6.2 `contacts`

把旧主键：

```sql
PRIMARY KEY(owner_did, did)
```

替换为：

```sql
owner_identity_id TEXT NOT NULL,
owner_did         TEXT NOT NULL DEFAULT '',
did               TEXT NOT NULL,
...
PRIMARY KEY(owner_identity_id, did)
```

建议索引：

```sql
CREATE INDEX IF NOT EXISTS idx_contacts_owner_last_seen
    ON contacts(owner_identity_id, last_seen_at DESC);

CREATE INDEX IF NOT EXISTS idx_contacts_owner_handle
    ON contacts(owner_identity_id, handle);

CREATE INDEX IF NOT EXISTS idx_contacts_owner_source_group
    ON contacts(owner_identity_id, source_group_id);
```

### 6.3 `contact_handle_bindings`

把旧主键：

```sql
PRIMARY KEY(owner_did, handle, did)
```

替换为：

```sql
PRIMARY KEY(owner_identity_id, handle, did)
```

当前 handle 唯一性：

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_current
    ON contact_handle_bindings(owner_identity_id, handle)
    WHERE is_current = 1;
```

`owner_did` 可以继续作为冗余字段保留，用于展示或旧数据导入诊断，但不能作为键。

### 6.4 `messages`

把旧主键：

```sql
PRIMARY KEY(msg_id, owner_did)
```

替换为：

```sql
PRIMARY KEY(owner_identity_id, msg_id)
```

建议核心形状：

```sql
CREATE TABLE IF NOT EXISTS messages (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    msg_id            TEXT NOT NULL,
    conversation_id   TEXT NOT NULL,
    thread_id         TEXT NOT NULL, -- 可选别名；迁移后应等于 conversation_id
    direction         INTEGER NOT NULL DEFAULT 0,
    sender_did        TEXT,
    receiver_did      TEXT,
    group_id          TEXT,
    group_did         TEXT,
    content_type      TEXT DEFAULT 'text',
    content           TEXT,
    title             TEXT,
    server_seq        INTEGER,
    sent_at           TEXT,
    stored_at         TEXT NOT NULL,
    is_e2ee           INTEGER DEFAULT 0,
    is_read           INTEGER DEFAULT 0,
    sender_name       TEXT,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(owner_identity_id, msg_id)
);
```

索引：

```sql
CREATE INDEX IF NOT EXISTS idx_messages_owner_conversation_time
    ON messages(owner_identity_id, conversation_id, COALESCE(sent_at, stored_at));

CREATE INDEX IF NOT EXISTS idx_messages_owner_conversation_seq
    ON messages(owner_identity_id, conversation_id, server_seq);

CREATE INDEX IF NOT EXISTS idx_messages_owner_sender
    ON messages(owner_identity_id, sender_did);

CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_owner_group_seq
    ON messages(owner_identity_id, group_id, server_seq)
    WHERE group_id IS NOT NULL AND TRIM(group_id) <> '' AND server_seq IS NOT NULL;
```

### 6.5 稳定会话身份

私聊会话 ID 不能包含本地 owner DID。

使用：

```text
私聊 conversation_id = "dm:" + peer_did
群聊 conversation_id = "group:" + group_id_or_group_did
邮件 conversation_id = "mail:" + mailbox_or_source
```

因为 `owner_identity_id` 已经是分区键，所以在 `conversation_id` 中包含本地 owner DID 没有必要，而且有害。

如果为了接口兼容继续保留 `thread_id`，它应该被设置为和 `conversation_id` 相同的稳定值。

### 6.6 `groups`

把旧主键：

```sql
PRIMARY KEY(owner_did, group_id)
```

替换为：

```sql
PRIMARY KEY(owner_identity_id, group_id)
```

### 6.7 `group_members`

把旧主键：

```sql
PRIMARY KEY(owner_did, group_id, user_id)
```

替换为：

```sql
PRIMARY KEY(owner_identity_id, group_id, user_id)
```

所有 `replace_group_members`、`mark_group_left`、群快照操作都必须使用 `owner_identity_id`。

### 6.8 `relationship_events`

把旧的：

```sql
event_id TEXT PRIMARY KEY
```

替换为：

```sql
PRIMARY KEY(owner_identity_id, event_id)
```

这会消除跨 owner 的事件 ID 冲突。

如果事件来自远端导入或测试 fixture，并带有固定 `event_id`，它只需要在当前 owner 作用域内唯一。

### 6.9 `e2ee_outbox`

当前 `e2ee_outbox` 使用全局 `outbox_id` 主键。替换为：

```sql
PRIMARY KEY(owner_identity_id, outbox_id)
```

建议索引：

```sql
CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_status
    ON e2ee_outbox(owner_identity_id, local_status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_msg
    ON e2ee_outbox(owner_identity_id, sent_msg_id);
```

### 6.10 旧 `e2ee_sessions`

不要把旧 `e2ee_sessions` 保留在生产运行时表结构中。

可选处理方式：

1. 从新的干净表结构中移除该表。
2. 只在 `legacy_*` 名称下保留，用于导入或迁移诊断。
3. 如果继续保留，所有运行时路径都必须忽略它。

私聊端到端加密运行时应使用按 `owner_identity_id` 建键的 `direct_e2ee_*` 表。

### 6.11 私聊端到端加密

`direct_e2ee_sessions` 已经采用正确 owner 模型：

```sql
PRIMARY KEY(owner_identity_id, peer_did)
UNIQUE(owner_identity_id, session_id)
```

如果当前分支已有 `revision` / CAS 语义，继续保留。

### 6.12 群组 MLS

群组 MLS 应继续按以下作用域隔离：

```text
owner_identity_id + device_id
```

`owner_did` 只保留为当前身份快照或元数据。

---

## 7. 上线前迁移策略

因为 AWiki 还没有上线，应优先采用严格的表结构重置或迁移门禁。

### 7.1 方案 A：开发库破坏性重建

如果现有本地库只是开发或测试状态：

1. 备份旧库到 `upgrade/backups/<timestamp>/awiki-cli.db`。
2. 用新表结构创建新数据库。
3. 可选导入安全的身份作用域行。
4. 记录清晰警告，说明旧本地缓存已重建。
5. 不保留旧运行时回退查询条件。

这是最简单、最安全、最干净的生产前路径。

### 7.2 方案 B：确定性重建迁移

如果需要保留开发数据：

1. 创建采用 identity-owned 表结构的 `_new` 表。
2. 通过身份注册表和 `identity_did_history` 把旧 `owner_did` 解析为 `owner_identity_id`。
3. 对已经有 `owner_identity_id` 的行，优先信任 identity id，而不是 DID。
4. 对没有 identity id 的行：
   - 如果恰好只有一个 live identity 映射到该 owner_did，则映射它；
   - 否则标记为未归属数据，并跳过或放入迁移隔离表。
5. 确定性合并重复数据。
6. 删除旧表并把 `_new` 表改名为正式表。
7. 运行不变量检查。
8. 设置 `PRAGMA user_version` 为新版本。

### 7.3 必须采用的合并优先级

使用确定性的合并规则：

- 保留最新的 `last_seen_at`、`updated_at`、`stored_at` 或 `remote_updated_at`。
- 非空展示字段优先于空字段。
- 如果任意重复消息已读，则保留 `is_read = 1`。
- 如果任意重复消息标记为加密，则保留端到端加密标记。
- 保留最高 `server_seq`。
- 元数据是 JSON 对象时尽量合并；否则最新非空值胜出。
- 联系人数据不能用空投影覆盖用户备注或关系状态。
- 群数据不能用陈旧或空状态把活跃成员状态降级，除非有明确离群事件。
- 关系事件只在相同 `owner_identity_id + event_id` 内合并。

### 7.4 迁移后的硬性不变量

迁移后以下查询结果都必须为 0：

```sql
SELECT COUNT(*) FROM contacts WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = '';
SELECT COUNT(*) FROM messages WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = '';
SELECT COUNT(*) FROM groups WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = '';
SELECT COUNT(*) FROM group_members WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = '';
SELECT COUNT(*) FROM relationship_events WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = '';
```

生产运行时不应再出现以下查询模式：

```sql
owner_identity_id = ? OR legacy owner_did = ?
```

该模式只能存在于迁移或导入代码中。

---

## 8. 重构后的运行时行为

### 8.1 普通写入

所有写入都按 identity 归属。

示例：

```sql
INSERT INTO messages (...)
ON CONFLICT(owner_identity_id, msg_id) DO UPDATE ...
```

```sql
INSERT INTO contacts (...)
ON CONFLICT(owner_identity_id, did) DO UPDATE ...
```

```sql
INSERT INTO groups (...)
ON CONFLICT(owner_identity_id, group_id) DO UPDATE ...
```

### 8.2 普通读取

所有读取都按 identity 作用域过滤：

```sql
WHERE owner_identity_id = ? AND ...
```

活跃读取不能依赖 DID 回退。

### 8.3 DID 恢复或替换

DID 恢复现在表示：

1. 保存新的身份材料。
2. 更新身份注册表。
3. 把旧 DID 写入 `identity_did_history(status='previous')`。
4. 把新 DID 写入 `identity_did_history(status='current')`。
5. 如有展示或调试需要，更新冗余 `owner_did` 值。
6. 不重新给联系人、消息、群重新建所有权键。
7. 只有密钥材料变化时，才清理或轮换端到端加密状态。

由于业务行按 `owner_identity_id` 建键，所以不会产生重复业务行。

### 8.4 联系人 upsert

如果联系人按 `owner_identity_id` 建键，0 行更新问题会自然消失。

但仍应为手写更新增加 affected-row 检查。任何预期更新一行却更新 0 行的操作，都必须报错，或显式走插入逻辑。

### 8.5 关系事件

事件 ID 按 owner 作用域唯一。另一个 identity 下相同 `event_id` 不构成冲突。

运行时 upsert 目标：

```sql
ON CONFLICT(owner_identity_id, event_id) DO UPDATE ...
```

### 8.6 群成员替换

`replace_group_members` 必须按以下条件删除：

```sql
DELETE FROM group_members
WHERE owner_identity_id = ? AND group_id = ?
```

绝不能按 `owner_did` 删除。

---

## 9. 应用、命令行和监听器的并发模型

异步分支已经引入本地状态 actor 模型。保留并增强它。

### 9.1 进程内规则

在同一个 `ImCore` 进程内：

- 所有本地状态操作都使用 `LocalStateDb`；
- `LocalStateDb` 在 actor 线程中拥有一个 SQLite 连接；
- 所有异步调用方通过 actor 命令队列串行化。

### 9.2 跨进程规则

应用和命令行仍可能是不同进程。SQLite WAL 和 `busy_timeout` 是必要条件，但不足以保护身份变更类操作。

采用两级协调：

1. **普通本地状态写入**
   - 使用事务；
   - 使用 WAL；
   - 使用 `busy_timeout`；
   - 多步骤写入批次可在适当位置使用 `BEGIN IMMEDIATE`。

2. **工作区结构性操作**
   - 表结构迁移；
   - 身份 recover local-finalize；
   - DID 替换；
   - 身份注册表重写；
   - 本地数据库重建；
   - 破坏性导入或重置。

这些操作必须获取工作区操作锁，例如：

```text
~/.awiki-cli/upgrade/upgrade.lock
```

或者专用锁：

```text
~/.awiki-cli/data/local_state.lock
```

持锁期间，应用和监听器应暂停本地写入，或在完成后重新连接并重新加载状态。

### 9.3 应用和命令行禁止裸访问本地库

命令行 handler 和应用 SDK facade 不能执行裸 SQL。它们必须调用 `im-core`。

---

## 10. 身份注册表不变量

在加载、保存和 bootstrap 阶段增加注册表校验：

1. `identity_id` 唯一。
2. `local_alias` 存在时唯一。
3. live identity 的 `did` 唯一。
4. live identity 的 `handle` 存在时唯一。
5. 默认身份引用一个存在的 live identity。
6. 所有身份都有非空 `id` 和 `did`。

如果出现重复 DID，必须 fail closed 并给出诊断指引。不能让数据库按 DID 把两个 logical identity 合并。

---

## 11. 移除旧运行时回退

完成表结构迁移后：

- 移除活跃本地状态读取中的 owner fallback 条件。
- 移除运行时代码中把 `credential_name` 隐式当 identity id 的逻辑。
- `credential_name` 如需保留，只能作为兼容或调试字段。
- 删除或隔离带 `UPDATE OR IGNORE` 的旧 `rebind_local_identity_state`。
- 如果旧 rebind 代码仍用于 Go/Python 旧数据迁移，它必须调用相同的确定性 identity-owned 迁移代码，或标记为测试/迁移专用。

当前 `UPDATE OR IGNORE` 模型不允许进入生产。

---

## 12. 建议实施切片

### 12.1 第一组：schema vNext identity-owned 表

可能影响文件：

- `crates/im-core/src/internal/local_state/schema.rs`
- `crates/im-core/src/internal/local_state/messages.rs`
- `crates/im-core/src/internal/local_state/groups.rs`
- `crates/im-core/src/internal/contact_store/records.rs`
- `crates/im-core/src/internal/store/e2ee_outbox.rs`
- `crates/im-core/src/internal/local_state/conversations.rs`

任务：

1. 增加 identity-owned 主键的新表结构版本。
2. 增加 `identity_did_history`。
3. 增加 `conversation_id`，或把 `thread_id` 规范为不含 owner DID 的稳定值。
4. 移除按 `owner_did` 分组的活跃运行时视图。
5. 更新索引。

### 12.2 第二组：严格迁移或重建路径

可能影响文件：

- `crates/im-core/src/internal/local_state/schema.rs`
- `crates/im-core/src/internal/identity_recover_local_state.rs`
- `crates/awiki-cli/src/workspace_upgrade/legacy_sqlite/*`
- `crates/awiki-cli/src/workspace_upgrade/migration_*`

任务：

1. 备份旧数据库。
2. 重建或确定性迁移到新版本。
3. 移除把 `UPDATE OR IGNORE` rebind 当成功路径的逻辑。
4. 在迁移后增加不变量检查。

### 12.3 第三组：OwnerScope 接口清理

任务：

1. 增加内部 `OwnerScope`。
2. 将 `OwnerScope` 贯穿本地状态 actor 命令。
3. 尽量移除接收自由 owner 字符串的存储函数。
4. 确保空 `owner_identity_id` 是非法输入。

### 12.4 第四组：消息和会话稳定性

任务：

1. 生成不包含 owner DID 的会话 ID。
2. 更新本地投影，写入稳定 `conversation_id`。
3. 更新会话列表和线程视图，按 `owner_identity_id + conversation_id` 分组。
4. 增加测试，确保 DID 替换不会拆分同一私聊会话。

### 12.5 第五组：联系人和关系事件

任务：

1. 按 `(owner_identity_id, did)` upsert 联系人。
2. 按 `(owner_identity_id, handle, did)` upsert handle 绑定。
3. 按 `(owner_identity_id, event_id)` upsert 关系事件。
4. 增加 0 行更新保护测试。

### 12.6 第六组：群组

任务：

1. 按 `(owner_identity_id, group_id)` upsert 群。
2. 按 `(owner_identity_id, group_id)` 替换群成员。
3. 增加测试，确保同一 identity 下旧 / 新 owner DID 的同一群最终只保留一份。

### 12.7 第七组：身份注册表校验

任务：

1. 增加 live DID 唯一性校验。
2. 增加重复 DID fail-closed 测试。
3. 确保 recover / replace 会更新 `identity_did_history`。

### 12.8 第八组：诊断 / doctor

增加数据库 doctor 命令或内部诊断辅助，检查：

- 活跃表中没有空 `owner_identity_id`；
- 活跃表不使用 `owner_did` 主键；
- 身份注册表没有重复 live DID；
- 没有重复 `(owner_identity_id, msg_id)`；
- 没有重复 `(owner_identity_id, did)` 联系人；
- 没有重复 `(owner_identity_id, group_id)`；
- 没有重复 `(owner_identity_id, event_id)`；
- 会话 ID 不包含本地 owner DID；
- 旧 `e2ee_sessions` 不被运行时使用。

---

## 13. 测试计划

### 13.1 单元测试

为每个表增加 SQLite 内存测试：

- schema 能创建全部 identity-owned 主键；
- `owner_identity_id` 为空或 null 时插入失败；
- 同一 identity 下同一 DID 联系人只 upsert 一行；
- 同一 identity 下同一 msg_id 消息只 upsert 一行；
- 不同 identity 下相同 event_id 不冲突；
- 群成员替换只影响同一 `owner_identity_id`。

### 13.2 迁移测试

构造旧表结构 fixture 数据库，包含：

- 只有旧 owner DID 的行；
- old/new owner DID 混合行；
- 重复消息；
- 重复联系人；
- 重复群；
- 跨 identity 的关系事件 ID 冲突；
- handle 绑定冲突；
- 包含 owner DID 的旧私聊 thread ID。

验证迁移：

- 每个 identity-owned natural key 只产生一行；
- 删除或排除旧 owner 行；
- 把私聊会话 ID 改写为稳定值；
- 不留下空 owner identity 行；
- 记录 DID 历史。

### 13.3 DID 恢复 / 替换测试

测试：

1. 同一 identity 从旧 DID 切到新 DID：
   - 消息不重复；
   - 联系人不重复；
   - 群不重复；
   - 会话 ID 稳定；
   - DID 历史更新。

2. 两个 live identity 共用同一 DID：
   - 在本地状态写入前，身份注册表校验失败。

### 13.4 异步 actor 测试

测试：

- 多个并发本地状态写入通过 `LocalStateDb` 执行；
- 同一个 `ImCore` 内并发首次打开共享同一个 actor；
- actor 写入串行化保持表不变量；
- actor 命令不能接受空 owner identity。

### 13.5 命令行 / 应用边界测试

测试：

- 命令行通过 `im-core` 路径访问，不执行裸 SQL；
- Dart/Flutter facade 获得 identity-scoped 结果；
- 面向应用的 API 不要求调用方手动提供 `owner_did` 做本地 SQL 过滤。

### 13.6 系统测试

最低仓库检查：

```bash
cd awiki-cli-rs2
cargo test --workspace --locked
bash scripts/sdk-refactor/final-cutover-check.sh
scripts/flutter/codegen-check.sh
```

本地环境可用时，执行聚焦系统测试：

```bash
make local-test-did
make local-test-listener
make local-test-message-v2
```

如果修改了本地安全状态或 MLS 路径，应加入私聊 / 群组端到端加密聚焦测试。

---

## 14. 发布门禁

发布前必须满足：

1. 新 workspace 会创建 vNext 数据库。
2. 旧开发库会迁移，或被明确备份后重建。
3. 运行时不再使用 `owner_did` 作为所有者分区回退条件。
4. 没有主键使用 `owner_did`。
5. DID 恢复 / 替换不会移动业务行所有权。
6. 重复 live DID fail closed。
7. 应用和命令行都使用 `im-core` 本地状态接口。
8. 私聊端到端加密仍使用 owner identity 作用域的状态。
9. 群组 MLS 仍按 owner identity + device 隔离。
10. 验证报告包含命令、结果、失败项、未运行检查和剩余风险。

---

## 15. 验收标准

Codex 实现只有同时满足以下条件，才算完成：

- `contacts`、`messages`、`groups`、`group_members`、`contact_handle_bindings`、`relationship_events`、`e2ee_outbox` 都按 `owner_identity_id` 建键。
- `owner_did` 不再是任何活跃业务表主键的组成部分。
- `relationship_events` 按 owner 唯一，而不是全局唯一。
- 私聊会话稳定键不再包含本地 owner DID。
- DID 恢复 / 替换不会制造重复业务行。
- 运行时本地状态读取不使用旧 owner fallback。
- 身份注册表拒绝重复 live DID。
- 本地状态 actor 继续作为异步读写边界。
- 迁移或重建路径会备份开发数据，并强制执行 vNext 不变量。
- `awiki-cli-rs2` 文档已更新，明确本地状态 owner 模型。

---

## 16. 建议文档更新

更新或新增：

- `awiki-cli-rs2/docs/architecture/local-state-owner-scope.md`
- `awiki-cli-rs2/docs/architecture/local-state-upgrade.md`
- 如果存在模块文档，更新 `awiki-cli-rs2/docs/sdk-refactor/modules/local-state.md`
- 只有跨仓库摘要需要新增本地状态说明时，才更新 `awiki-harness/context/nodes/storage.node.md`
- 只有验证入口或文档入口变化时，才更新 `awiki-harness/context/repo-profiles/awiki-cli-rs2.md`

应沉淀的长期规则是：

```text
本地状态的 owner identity 是 owner_identity_id。
owner_did 是可变身份元数据，不能作为 owner 分区键。
```
