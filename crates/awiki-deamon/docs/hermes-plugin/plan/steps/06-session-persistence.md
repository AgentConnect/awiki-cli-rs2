# Step 06: Hermes session 持久化与 resume/reset

主计划: [../plan.md](../plan.md)  
步骤编号: 06  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 未开始 |
| 完成时间 | 未完成 |
| 提交 | 未提交 |
| 审查证据 | 待记录 |
| 验证证据 | 待记录 |
| 下一步 | 等 Step 03/04 完成后，持久化 Hermes native session 映射 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：保存 Awiki message route context 到 Hermes native session 的映射，支持同一 conversation 复用 session，并提供基本 resume/reset/cleanup 能力。
- 系统可见结果：`(agent_did, controller_did, conversation_id, session_kind)` 可以稳定映射到 `hermes_session_id`；daemon 重启后可恢复映射或按策略重建。
- 非目标：不实现所有 runtime 的完整 session abstraction；不复制 Hermes transcript；不把 session event 作为 run final 主事实源。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/state/mod.rs` | 新增 `hermes_native_sessions`，可选 `runtime_session_mapping` | schema version 递增，migration tests。 |
| `crates/awiki-deamon/src/plugins/hermes/session.rs` | Session manager：get/create/resume/reset | Hermes 私有字段留在扩展表。 |
| `crates/awiki-deamon/src/plugins/hermes/runner.rs` | 使用 session manager 获取 session | Step 03 的内存 session 替换为持久映射。 |
| `crates/awiki-deamon/src/daemon_cli/mod.rs` 或 commands | 可增加诊断/重置命令 | 若范围过大，可留到 Step 07。 |
| `crates/awiki-deamon/tests/` | schema migration、unique route、resume/reset tests | 不依赖真实 Hermes。 |
| `crates/awiki-deamon/docs/hermes-plugin/` | 更新 session mapping 说明 | 中文。 |

## 4. 依赖

- 前置步骤：Step 03、Step 04。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 11、16、21.3 章。
- 环境前置条件：SQLite migration tests。

## 5. 设计

### 推荐模型

本步骤建议实现两层模型，但允许按复杂度降级：

```text
runtime_session_mapping
  -> hermes_native_sessions
```

如果实现者判断通用表会拉大范围，可以只建 `hermes_native_sessions`，但必须预留：

- `runtime_session_id`；
- `agent_did`；
- `runtime_profile_id`；
- `controller_did`；
- `conversation_id`；
- `session_kind`；
- `status`；
- route unique constraint。

### 表结构

通用表目标态：

```sql
CREATE TABLE runtime_session_mapping (
  runtime_session_id TEXT PRIMARY KEY,
  agent_did TEXT NOT NULL,
  runtime_profile_id TEXT NOT NULL,
  runtime_plugin_id TEXT NOT NULL,
  controller_did TEXT NOT NULL,
  conversation_id TEXT,
  session_kind TEXT NOT NULL,
  route_key TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  UNIQUE(agent_did, controller_did, conversation_id, session_kind)
);
```

Hermes 表：

```sql
CREATE TABLE hermes_native_sessions (
  id TEXT PRIMARY KEY,
  runtime_session_id TEXT NOT NULL,
  agent_did TEXT NOT NULL,
  runtime_profile_id TEXT NOT NULL,
  controller_did TEXT NOT NULL,
  conversation_id TEXT,
  hermes_profile TEXT NOT NULL,
  hermes_session_id TEXT NOT NULL,
  session_kind TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  UNIQUE(agent_did, controller_did, conversation_id, session_kind)
);
```

### route key

route key 应稳定、可 debug、不可包含 secret。建议：

```text
hermes:<agent_did>:<controller_did>:<conversation_id-or-no-conversation>:<session_kind>
```

如果担心 DID 长度或特殊字符，可以保存 sha256 route hash，同时保留 debug 字段。

### session kind

MVP 至少支持：

```text
conversation
```

可预留：

```text
ephemeral
debug
```

### reset/resume

- Resume：查到 active mapping 时使用原 `hermes_session_id`。
- Reset：将旧 mapping 标记 `reset` 或 `archived`，创建新 Hermes session。
- Cleanup：只清理 daemon mapping，不删除 Hermes transcript，除非后续明确实现 Hermes transcript cleanup。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 06 标记为 `in_progress`。
2. 决定本步骤实现两层表还是仅 Hermes 私有表；如偏离主计划推荐，先更新主计划变更日志。
3. 实现 migration：
   - schema version 递增；
   - `CREATE TABLE IF NOT EXISTS`；
   - 添加 unique index；
   - migration test 覆盖旧 DB 升级。
4. 实现 state CRUD：
   - `get_or_create_hermes_session`；
   - `load_hermes_session_by_route`；
   - `mark_hermes_session_status`；
   - `reset_hermes_session`。
5. 接入 Hermes runner：
   - Step 04 的 prompt submit 前先调用 session manager；
   - fake gateway 创建 session 时返回 deterministic id；
   - runner 使用已有 session 时不重复 `session.create`，除非 reset。
6. 增加 tests：
   - 同一 route 重复请求复用 session；
   - 不同 conversation 创建不同 session；
   - reset 后创建新 session 且旧 session 状态改变；
   - daemon 重开 connection 后仍能 load mapping；
   - unique constraint 防止重复 active session。
7. 如果新增 CLI 诊断命令，保持只读或 reset 明确参数；避免本步骤扩展太多 UX。
8. 运行验证，review，修复后提交。

## 7. 验收标准

- [ ] Hermes native session mapping 持久化到 SQLite。
- [ ] 同一 `(agent_did, controller_did, conversation_id, session_kind)` route 可复用 Hermes session。
- [ ] reset/resume 语义有测试覆盖。
- [ ] daemon 不复制 Hermes transcript，不把 observation 当 final。
- [ ] schema migration 兼容旧 DB。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 07。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| session focused | `cargo test -p awiki-deamon --locked hermes_session` | mapping/resume/reset tests 通过。 |
| state migration | `cargo test -p awiki-deamon --locked state` | schema 初始化和迁移测试通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| SQLite 手工检查 | 测试中查询 `sqlite_master` 和 unique index | 表和索引存在。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查数据模型、唯一约束、迁移、route key、reset 语义、并发重复创建风险。
- 数据安全 review：session 表不得存 prompt 原文、token secret、private key 或 JWT。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：session schema/manager/runner 接入/tests/docs。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: persist hermes native sessions`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 通用 `runtime_session_mapping` 影响其他 runtime 太大 | 记录受影响文件和测试 | 降级为仅 `hermes_native_sessions` 并预留 `runtime_session_id` | 当前步骤 | 更新主计划变更日志后继续 |
| reset 语义与真实 Hermes session API 不匹配 | 记录 Hermes API 行为 | daemon 标记 archived，真实 reset 延后 | reset 功能 | 保留 resume，reset 命令标记 unsupported |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 13. 风险、回滚与后续

- 风险：并发 route 可能创建重复 session；未来多 runtime 通用表迁移成本。
- 回滚/fallback：回滚后 Hermes 每次 run 可使用 ephemeral session，但无法 resume。
- 后续文档：若通用表落地，更新 daemon runtime host 架构文档中的 session mapping 章节。
