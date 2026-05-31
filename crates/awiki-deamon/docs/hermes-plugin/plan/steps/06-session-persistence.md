# Step 06: Hermes session 持久化与 resume/reset

主计划: [../plan.md](../plan.md)  
步骤编号: 06  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-06-01 00:40:24 +0800 |
| 完成时间 | 2026-06-01 00:53:38 +0800 |
| 提交 | 实现提交 `9c5bff865682d3c77e97355484aacbe8d0f64823` |
| 审查证据 | 2026-06-01 00:51:13 +0800 完成提交前 review：确认 session 表只保存 route/session metadata，不保存 prompt 原文、runtime token、private key 或 JWT；确认 active route 唯一约束、resume/reset 和 schema migration；残余风险为首次同 route 并发创建缺少事务重试。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_session` 通过，2 个 focused tests；`cargo test -p awiki-deamon --locked state` 通过，12 个匹配测试；`cargo test -p awiki-deamon --locked hermes_message` 通过，6 个匹配测试；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，6 个匹配测试、1 个 ignored real smoke 被过滤；`cargo test -p awiki-deamon --locked` 通过，56 个测试、1 ignored；`git diff --check -- crates/awiki-deamon` 通过；边界/secret 搜索结果已记录在执行记录。 |
| 下一步 | 启动 Step 07 长驻 daemon 集成与诊断 |

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

- [x] Hermes native session mapping 持久化到 SQLite。
- [x] 同一 `(agent_did, controller_did, conversation_id, session_kind)` route 可复用 Hermes session。
- [x] reset/resume 语义有测试覆盖。
- [x] daemon 不复制 Hermes transcript，不把 observation 当 final。
- [x] schema migration 兼容旧 DB。
- [x] 审查发现 已修复或明确记录。
- [x] 本步骤创建一个聚焦提交后才进入 Step 07。

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
| 发现 | 若按推荐一次性新增通用 `runtime_session_mapping`，会把 Step 06 扩展到其他 runtime；首次同 route 并发创建时当前实现是查库、创建、插入，未做事务级 get-or-create 重试。 | 本步骤按计划允许降级为 Hermes 私有表；并发首次创建风险由 active route unique index fail-closed，后续长驻并发化时可补事务或 retry。 |
| 已修复 | 选择仅实现 `hermes_native_sessions` 并保留 `runtime_session_id`；新增 active route partial unique index；runner 有 state 时优先复用 active session，reset 后创建 replacement；fake gateway 记录 create_session 次数，测试覆盖同 route 复用、不同 conversation 分离、daemon restart 后复用和 reset。 | 未新增通用表，主计划变更日志已记录范围决策。 |
| 残余风险 | 首次同 route 并发创建仍可能有一个写入因 unique index 失败，而不是自动重试加载 winner；真实 Hermes reset API 未接入，只做 daemon mapping reset。 | Step 07/后续如果 foreground 并发执行同 conversation，需要补事务/retry；真实 Hermes transcript cleanup 不在 MVP。 |
| 测试新增或缺失 | 新增 `hermes_session_mapping_reuses_session_for_same_conversation_after_restart`、`hermes_session_mapping_reset_archives_old_session_and_creates_replacement`、`hermes_native_session_roundtrips_and_resets_active_route`；更新 schema version 相关测试。 | 没有真实 Hermes native session resume smoke，仍由 fake gateway 锁定 daemon 行为。 |
| 文档更新或缺失 | 主计划和本步骤记录已同步私有表决策、验证证据和残余风险。 | 未更新通用 runtime host 架构文档，因为通用表未落地。 |

## 14. Step 06 执行记录

### 已实现

- `DaemonState` schema version 升到 7，新增 `hermes_native_sessions` 表，字段包含 `runtime_session_id`、`agent_did`、`runtime_profile_id`、`controller_did`、`conversation_id`、`route_key`、`hermes_profile`、`hermes_session_id`、`session_kind`、`status`、时间戳。
- 新增 `idx_hermes_native_sessions_active_route` partial unique index，仅约束 `status = 'active'` 的 route；reset 后旧记录可保留为 `reset`，新 active session 可创建。
- 新增 `HermesSessionRoute` 和 `HermesNativeSessionRecord`，route key 形如 `hermes:<agent_did>:<controller_did>:<conversation-or-no-conversation>:<session_kind>`，不包含 token 或 prompt 原文。
- 新增 state CRUD：`store_hermes_native_session`、`load_active_hermes_session_by_route`、`mark_hermes_session_status`、`reset_active_hermes_session_by_route`。
- `HermesRuntimePlugin::with_state` 在有 state 时优先加载 active session；没有 active mapping 时调用 gateway `create_session` 并持久化；`HermesRuntimePlugin::new` 保持原无 state 行为，降低 Step 07 之前的接入风险。
- 新增 `reset_hermes_session_by_route` helper；fake gateway 记录 `created_sessions`，同 route 首次创建保持原确定性 session id，后续创建追加序号供 reset 测试区分。
- 更新 schema version 相关测试到 7，并覆盖 v6 旧库初始化迁移到 v7。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 通用 `runtime_session_mapping` 暂不落地；首次并发同 route 创建缺少事务重试；真实 Hermes reset API 未接入。 | 已写入主计划变更日志和残余风险。 |
| 已修复 | 私有表保留 `runtime_session_id`，active route unique index，runner stateful resume/reset，fake gateway deterministic session instrumentation，focused tests 覆盖 restart/不同 conversation/reset。 | 满足 Step 06 MVP 验收。 |
| 残余风险 | 并发首次创建可能 fail-closed；reset 只归档 daemon mapping，不删除 Hermes transcript。 | Step 07/后续并发化和真实 Hermes adapter 收敛时处理。 |
| 测试新增或缺失 | 新增 3 个 session/state tests，更新 schema version tests。 | 未跑真实 Hermes smoke。 |
| 文档更新或缺失 | 主计划和本步骤已记录执行状态、范围变更和 review 证据。 | Harness 文档无需更新。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes_session` | 通过：2 个 focused tests。 |
| `cargo test -p awiki-deamon --locked state` | 通过：12 个匹配测试。 |
| `cargo test -p awiki-deamon --locked hermes_message` | 通过：6 个匹配测试。 |
| `cargo test -p awiki-deamon --locked hermes_gateway` | 通过：6 个匹配测试，1 个 ignored real smoke 被过滤。 |
| `cargo test -p awiki-deamon --locked` | 通过：56 个测试，1 ignored，doc tests 0 个。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |
| `rg -n "crates/awiki-cli\|awiki_cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过：无命中。 |
| `rg -n "rtok_\|runtime_rpc_token.*println\|auth_private_key\|jwt_token\|prompt\|task_text" crates/awiki-deamon/src/state/mod.rs crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests/hermes_message.rs` | 通过但有预期命中：prompt wrapper 生产代码和测试、测试假 token/脱敏断言、既有 agent auth/private key 状态字段、runtime task 文本字段；`hermes_native_sessions` 未新增 prompt/token/private key/JWT 字段。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 11]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/06-session-persistence.md
 M crates/awiki-deamon/src/plugins/hermes/gateway.rs
 M crates/awiki-deamon/src/plugins/hermes/mod.rs
 M crates/awiki-deamon/src/plugins/hermes/runner.rs
 M crates/awiki-deamon/src/state/mod.rs
 M crates/awiki-deamon/tests/hermes_message.rs
 M crates/awiki-deamon/tests/hermes_profile.rs
 M crates/awiki-deamon/tests/state_bootstrap.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/06-session-persistence.md`
  - `crates/awiki-deamon/src/plugins/hermes/gateway.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/runner.rs`
  - `crates/awiki-deamon/src/state/mod.rs`
  - `crates/awiki-deamon/tests/hermes_message.rs`
  - `crates/awiki-deamon/tests/hermes_profile.rs`
  - `crates/awiki-deamon/tests/state_bootstrap.rs`

### 提交后状态

- 实现提交：`9c5bff865682d3c77e97355484aacbe8d0f64823`
- 实现提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 12]
```

- 遗留未提交变更：无。

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
| 2026-06-01 | 本步骤先实现 Hermes 私有 `hermes_native_sessions`，保留 `runtime_session_id` 和 route 字段；通用 `runtime_session_mapping` 延后 | 控制跨 runtime 影响面，同时满足 Hermes resume/reset 验收 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 13. 风险、回滚与后续

- 风险：并发 route 可能创建重复 session；未来多 runtime 通用表迁移成本。
- 回滚/fallback：回滚后 Hermes 每次 run 可使用 ephemeral session，但无法 resume。
- 后续文档：若通用表落地，更新 daemon runtime host 架构文档中的 session mapping 章节。
