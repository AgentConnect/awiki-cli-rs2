# 步骤 03：原子 SQL Key 切换

主计划：[../plan.md](../plan.md)  
步骤编号：03  
状态：完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/db-refactor-in-async` |
| 开始时间 | 2026-05-30T16:52:11Z |
| 完成时间 | 2026-05-30T18:06:59Z |
| 提交 | 实现提交 `69d8d61`：`im-core: switch local state keys to owner identity` |
| 审查证据 | 提交前审查已完成：确认 active v17 schema、messages、contacts、groups、conversations、mail、recover、replace-did 计数和 `e2ee_outbox` 均按 `owner_identity_id` 分区；修复 review 发现的 recover merge 曾把 `final_credential_name` 当作 `owner_identity_id` 的问题，改为用保存后的 `unique_id` 作为 `final_owner_identity_id`，`final_identity_name` 仅保留为 `credential_name` metadata；确认 DID-only wrapper fail closed；未新增 Secure public discovery 或 raw secure output。 |
| 验证证据 | `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked local_state` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked contact_store` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked e2ee_outbox` 通过；`CARGO_BUILD_JOBS=1 cargo test -p im-core --locked identity_recovery` 通过；`CARGO_BUILD_JOBS=1 cargo check -p im-core --locked` 通过；`cargo fmt --all --check` 通过；`git diff --check` 通过；SQL 搜索仅命中 legacy DDL、未进入 active v17 的 legacy helper 和测试夹具。默认并行 `cargo test -p im-core --locked local_state` 曾在链接阶段被系统 `SIGKILL`，随后用 `CARGO_BUILD_JOBS=1` 重跑通过，判定为资源问题而非测试失败。 |
| 下一步 | 步骤 04：稳定消息会话。 |

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

- [x] `SCHEMA_VERSION` 是 17。
- [x] 活跃业务表 primary keys 不包含 `owner_did`。
- [x] 活跃 runtime SQL 不再使用 `owner_identity_id = ? OR ... owner_did = ?`。
- [x] 活跃 runtime SQL 不再使用 `credential_name` 作为 owner 回退。
- [x] `relationship_events` 不再拥有全局 `event_id` primary key。
- [x] 活跃 secure outbox read/write 按 owner identity scoped，public errors/logs 不暴露 plaintext。
- [x] Runtime writes 拒绝空 owner identity。
- [x] 聚焦 local-state tests 通过。
- [x] 审查发现 已处理或明确记录。
- [x] 依赖步骤开始前已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked local_state` | Local-state schema/store tests 通过。 |
| 单元 | `cargo test -p im-core --locked contact_store` | Contact/event tests 通过。 |
| 单元 | `cargo test -p im-core --locked e2ee_outbox` | Outbox tests 通过。 |
| 搜索 | `rg "owner_identity_id = \\? OR|credential_name.*owner|ON CONFLICT\\(owner_did|ON CONFLICT\\(event_id\\)|PRIMARY KEY \\(owner_did" crates/im-core/src` | 除 migration/legacy tests 外，没有 active runtime 命中。 |

实际验证记录：

- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked local_state`：通过，52 个 lib filtered tests 和相关 integration filtered tests 通过。
- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked contact_store`：通过，5 个相关测试通过。
- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked e2ee_outbox`：通过，6 个相关测试通过，其中包含 `secure_outbox_lists_failed_entries_without_plaintext`。
- `CARGO_BUILD_JOBS=1 cargo test -p im-core --locked identity_recovery`：通过，5 个相关测试通过，覆盖 recover runtime 调用链。
- `CARGO_BUILD_JOBS=1 cargo check -p im-core --locked`：通过。
- `cargo fmt --all --check`：通过。
- `git diff --check`：通过。
- SQL 搜索残留分类：
  - `crates/im-core/src/internal/local_state/schema.rs` 的 `PRIMARY KEY (owner_did, ...)` 命中来自 v6/v11 legacy DDL 常量。
  - `crates/im-core/src/internal/local_state/schema.rs` 的 `ON CONFLICT(owner_did, handle, did)` 命中来自 legacy `backfill_contact_handle_bindings` helper；active v17 `create_schema()` 不再调用该 helper。
  - `crates/im-core/src/internal/identity_recover_local_state.rs` 的 `owner_did` 查询和 `e2ee_sessions` primary key 命中来自 recover 测试断言和测试内显式 legacy table fixture。
  - 未发现 active runtime 的 owner-DID fallback、`credential_name` owner fallback、旧 conflict target 或全局 `relationship_events.event_id` conflict target。

补充说明：一次未限制并行度的 `cargo test -p im-core --locked local_state` 在链接阶段被 `ld` 的 `SIGKILL` 中断；同一命令使用 `CARGO_BUILD_JOBS=1` 后完整通过，作为最终验证证据。

## 9. 审查流程

- 将 schema 和 SQL 变更作为一个契约整体 审查。
- 检查 active runtime 中没有 empty-owner 回退。
- 检查 secure outbox errors/logs 仍然脱敏。
- 检查测试覆盖两个 identity 使用相同 natural key 的隔离场景。

实际审查记录：

- 已审查 `schema.rs`：active schema 直接创建 v17 identity-owned tables，pre-v17 DB fail closed，active views 按 `owner_identity_id` 聚合和排序，legacy `e2ee_sessions` 不再由 active schema 创建。
- 已审查 active SQL：messages、contacts、contact handle bindings、relationship events、groups、group members、conversations、mail notification 和 `e2ee_outbox` 的读取、写入、mark/read、retry/drop/list 均使用 `owner_identity_id` predicate 或 conflict key。
- 已审查 actor/projection：group member replace 和 group leave 调用链传入当前 identity id，不再只依赖 owner DID。
- 已审查 recover/replace 边界：recover merge 使用保存后的 identity `unique_id` 写入 `owner_identity_id`，`final_identity_name` 只作为 `credential_name` metadata；replace-did 计数对缺失 legacy `e2ee_sessions` 表返回 0。
- 审查发现已修复：
  - recover merge 曾把 `final_credential_name` 当作 `owner_identity_id` 使用，已拆分为 `final_owner_identity_id` 和 `final_credential_name`。
  - 多个测试 fixture 在 v17 schema 中插入缺失 `owner_identity_id` 的行，已改为 identity-owned fixture 或 v17 no-op 断言。
  - `contact_store` 旧测试仍期待 owner-DID fallback，已改为验证 identity-only 查询和 DID snapshot 不参与 owner scope。
- 残余风险：步骤 08 仍需把 workspace upgrade/rebuild 正式接入 pre-v17 数据库；本步骤已让 active runtime 对 pre-v17 DB 返回明确 migration-required error。

## 10. 提交要求

- 建议提交信息：`im-core: switch local state keys to owner identity`

## 11. 风险、回滚和后续

- 风险：SQL 改动面广，可能破坏无关 runtime tests。
- 回滚/回退：回滚本步骤原子提交；必要时保留步骤 02 scaffold。
- 后续：步骤 08 负责 workspace migration/rebuild packaging。
