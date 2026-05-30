# 步骤 03：原子 SQL Key 切换

主计划：[../plan.md](../plan.md)  
步骤编号：03  
状态：草案

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | |
| 完成时间 | |
| 提交 | |
| 审查证据 | |
| 验证证据 | |
| 下一步 | 激活 v17，并一次性重写所有活跃 local-state SQL conflict keys。 |

## 2. 目标

- 产出：活跃 SQLite schema 和活跃 storage SQL 全部使用 identity-owned keys。
- 用户/系统行为：新本地数据库是 v17，业务行必须提供 `owner_identity_id`。
- 非目标：最终文档、完整 workspace migration、public DTO cleanup、启用 Secure 公开发现。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `schema.rs` | 将 `SCHEMA_VERSION` bump 到 17 并激活 v17 DDL。 | 现有 v16 DB 需要 rebuild/swap。 |
| `messages.rs` | 使用 `ON CONFLICT(owner_identity_id, msg_id)` 和 strict owner identity。 | 如有需要暂时保留 `thread_id`。 |
| `contact_store/records.rs` | identity-owned upserts 和 lookups。 | relationship conflict target 在这里改。 |
| `groups.rs` | identity-owned groups/members。 | delete/replace 按 owner identity。 |
| `e2ee_outbox.rs` | 使用 `(owner_identity_id, outbox_id)` 和 strict scope predicate。 | 移除 runtime credential/DID 回退。 |
| `actor.rs` | Command handlers 调用 strict identity-owned functions。 | 某些 command signature 仍可带 `owner_did` 作为 metadata。 |
| 测试 | 更新所有触碰 SQLite 的既有 local-state tests。 | 必须全部通过。 |

## 4. 依赖

- 前置步骤：步骤 02。
- 外部文档或决策：legacy `e2ee_sessions` 处理方式已决定。

## 5. 核心设计

本步骤必须原子执行，因为活跃 schema 和 SQL conflict targets 必须一致。`SCHEMA_VERSION = 17` 后，任何运行时 SQL 继续使用 `ON CONFLICT(owner_did, ...)` 或 `ON CONFLICT(event_id)` 都会与新表结构不匹配或语义错误。

规则：

- Runtime writes 要求非空 `owner_identity_id`。
- Runtime reads 使用 `WHERE owner_identity_id = ?`。
- `owner_did` 可以作为 snapshot 字段更新，但不是 primary/unique owner key。
- `credential_name` 永远不能作为 active runtime owner 回退。
- 对现有 v16 DB，在步骤 08 接入 workspace upgrade 前，要么调用 rebuild helper，要么返回清晰的 migration-required error。
- Secure local-state runtime 在 key 切换期间保持同样的 redaction/public-surface posture：active query 不得用 `credential_name`、DID 或 global outbox id 代替 owner；public output 不得暴露 outbox plaintext 或 secure internals。

## 6. 实施指南

1. 将 `SCHEMA_VERSION` bump 到 17。
2. 在 `schema.rs` 中激活 v17 table definitions。
3. 替换 conflict targets：
   - messages: `(owner_identity_id, msg_id)`
   - contacts: `(owner_identity_id, did)`
   - bindings: `(owner_identity_id, handle, did)`
   - relationship events: `(owner_identity_id, event_id)`
   - groups: `(owner_identity_id, group_id)`
   - members: `(owner_identity_id, group_id, user_id)`
   - outbox: `(owner_identity_id, outbox_id)`
4. 将 active owner predicates 替换为 `owner_identity_id = ?`。
5. 在每个 active write 前增加 required-field checks。
6. 更新单元测试和 fixtures，确保都提供 `owner_identity_id`。
7. 保持 active write/read helper 返回的 secure errors 已脱敏。
8. schema 创建或 rebuild 后执行 invariant checks。

## 7. 验收标准

- [ ] `SCHEMA_VERSION` 是 17。
- [ ] 活跃业务表 primary keys 不包含 `owner_did`。
- [ ] 活跃 runtime SQL 不再使用 `owner_identity_id = ? OR ... owner_did = ?`。
- [ ] 活跃 runtime SQL 不再使用 `credential_name` 作为 owner 回退。
- [ ] `relationship_events` 不再拥有全局 `event_id` primary key。
- [ ] 活跃 secure outbox read/write 按 owner identity scoped，public errors/logs 不暴露 plaintext。
- [ ] Runtime writes 拒绝空 owner identity。
- [ ] 聚焦 local-state tests 通过。
- [ ] 审查发现 已处理或明确记录。
- [ ] 依赖步骤开始前已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked local_state` | Local-state schema/store tests 通过。 |
| 单元 | `cargo test -p im-core --locked contact_store` | Contact/event tests 通过。 |
| 单元 | `cargo test -p im-core --locked e2ee_outbox` | Outbox tests 通过。 |
| 搜索 | `rg "owner_identity_id = \\? OR|credential_name.*owner|ON CONFLICT\\(owner_did|ON CONFLICT\\(event_id\\)|PRIMARY KEY \\(owner_did" crates/im-core/src` | 除 migration/legacy tests 外，没有 active runtime 命中。 |

## 9. 审查流程

- 将 schema 和 SQL 变更作为一个契约整体 审查。
- 检查 active runtime 中没有 empty-owner 回退。
- 检查 secure outbox errors/logs 仍然脱敏。
- 检查测试覆盖两个 identity 使用相同 natural key 的隔离场景。

## 10. 提交要求

- 建议提交信息：`im-core: switch local state keys to owner identity`

## 11. 风险、回滚和后续

- 风险：SQL 改动面广，可能破坏无关 runtime tests。
- 回滚/回退：回滚本步骤原子提交；必要时保留步骤 02 scaffold。
- 后续：步骤 08 负责 workspace migration/rebuild packaging。
