# 步骤 02：v17 Schema 和重建脚手架

主计划：[../plan.md](../plan.md)  
步骤编号：02  
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
| 下一步 | 增加 inactive identity-owned schema 和 rebuild helpers。 |

## 2. 目标

- 产出：定义 v17 identity-owned schema 和确定性重建机制，但不把它激活为默认运行时 schema。
- 用户/系统行为：本步骤不改变活跃运行时 schema。
- 非目标：修改活跃 `SCHEMA_VERSION`、修改运行时 SQL、删除 legacy tables、改变 secure discovery 或 public secure output。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/im-core/src/internal/local_state/schema.rs` | 新增 v17 DDL constants 和 invariant SQL helpers。 | 本步骤不 bump 活跃 `SCHEMA_VERSION`。 |
| `crates/im-core/src/internal/local_state/` | 如有需要，增加 rebuild/migration helper module。 | 优先使用结构化 row readers 和 table-specific merge helpers。 |
| `crates/im-core/src/internal/identity_recover_local_state/*` | 可复用已有 merge semantics。 | 避免重复写 ad hoc string SQL。 |
| 测试 | in-memory schema creation 和 invariant checks。 | 测试不应要求修改运行时代码。 |

## 4. 依赖

- 前置步骤：步骤 01 owner model。
- 外部文档或决策：激活前决定 legacy `e2ee_sessions` 是删除还是重命名。

## 5. 核心设计

构建 v17 schema 创建函数，目标表主键如下：

- `contacts PRIMARY KEY(owner_identity_id, did)`
- `contact_handle_bindings PRIMARY KEY(owner_identity_id, handle, did)`
- `messages PRIMARY KEY(owner_identity_id, msg_id)`
- `groups PRIMARY KEY(owner_identity_id, group_id)`
- `group_members PRIMARY KEY(owner_identity_id, group_id, user_id)`
- `relationship_events PRIMARY KEY(owner_identity_id, event_id)`
- `e2ee_outbox PRIMARY KEY(owner_identity_id, outbox_id)`
- `identity_did_history PRIMARY KEY(owner_identity_id, did)`
- direct E2EE tables 保持 identity-owned，并保留 `revision`。

SQLite 主键变化需要表重建。脚手架应创建 `_new` 表，使用明确 ownership resolution 复制/合并数据，验证后 swap。该能力可在步骤 03/08 前保持 inactive。

Secure scaffold 要求：

- 如果保留 `e2ee_outbox.plaintext` 列，它仍是内部实现细节；schema diagnostics 只能以脱敏列名检查的形式提到。
- Legacy `e2ee_sessions` 如果保留，只能明确标记为 migration-only diagnostics，不得被 active runtime helpers 引用。
- Rebuild/invariant helpers 不得记录 message body、plaintext outbox payload、private key material、JWT、raw MLS notices、KeyPackage、Welcome/Commit/Proposal payloads 或 provider stdout/stderr。
- Backup/rebuild helper API 应为步骤 08 保留文件权限、backup lock/journal 行为留好接口。

## 6. 实施指南

1. 新增 v17 DDL constants 和 test-only entrypoint，例如 `create_schema_v17_for_test`。
2. 增加 invariant helper functions：
   - 活跃表没有 empty/null `owner_identity_id`；
   - 活跃表没有重复 identity-owned natural keys；
   - direct conversation id 不包含本地 owner DID；
   - `identity_did_history` 每个 identity 最多一个 `current` DID。
3. 增加 row ownership resolver API shape：
   - 优先使用已有非空 `owner_identity_id`；
   - 否则通过 identity registry/DID history 解析 `owner_did`；
   - 否则返回 unresolved row。
4. 增加 messages/contacts/groups/events/outbox 的 deterministic merge helper signatures。
5. 增加 redaction helper 或 logging convention，确保 migration diagnostics 默认不输出 row content。
6. 增加测试，在内存中创建 v17 schema 并检查 primary keys/indexes。

## 7. 验收标准

- [ ] v17 DDL 存在，并在测试中创建 identity-owned primary keys。
- [ ] Rebuild helper API 可以表达 resolved 和 unresolved rows。
- [ ] Invariant SQL helpers 有测试覆盖。
- [ ] Invariant/rebuild diagnostics 已脱敏，不打印 secure payloads 或 private material。
- [ ] 如果保留 legacy secure tables，代码注释/测试明确说明它们仅用于 migration。
- [ ] 活跃运行时 `SCHEMA_VERSION` 在步骤 03 前保持不变。
- [ ] 审查发现 已处理或明确记录。
- [ ] 步骤 03 开始前已创建本步骤聚焦提交。

## 8. 验证

| 检查 | 命令 / 方法 | 期望证据 |
|---|---|---|
| 单元 | `cargo test -p im-core --locked local_state_schema_v17` | v17 schema shape 测试通过。 |
| 单元 | `cargo test -p im-core --locked owner_invariant` | invariant helper 测试通过。 |
| 编译 | `cargo check -p im-core --locked` | 无编译错误。 |
| 搜索 | `rg "println!|eprintln!|tracing::|log::" crates/im-core/src/internal/local_state crates/im-core/src/internal/identity_recover_local_state` | 任何 migration diagnostics 都经过脱敏 审查。 |

## 9. 审查流程

- 检查 v17 schema 中没有活跃业务主键使用 `owner_did`。
- 检查任何 `owner_did` indexes 仅用于诊断/展示，不是唯一 owner 分区键。
- 检查 unresolved legacy rows 没有被静默分配。
- 检查 rebuild diagnostics 不暴露 plaintext、JWT、private keys、ratchet/MLS material、raw secure outbox rows 或 backup contents。

## 10. 提交要求

- 建议提交信息：`im-core: scaffold identity-owned local state schema`

## 11. 风险、回滚和后续

- 风险：inactive scaffold 如果步骤 03 改字段名，可能 drift。
- 回滚/回退：保持 scaffold tests 严格，激活前更新。
