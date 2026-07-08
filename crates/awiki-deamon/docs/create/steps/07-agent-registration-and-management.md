# 步骤 07：daemon agent 与 runtime agent 管理

主计划：[../plan.md](../plan.md)
步骤编号：07
状态：已完成

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 已完成 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 2026-05-31 04:35:30 CST |
| 完成时间 | 2026-05-31 07:55:35 CST |
| 提交 | `9ac7b4e` |
| 审查证据 | Review 已完成：daemon agent/runtime agent DID 生成、registration token 兑换、`controller_did` MVP 校验、payload command parser、ready/failed status outbox、daemon 管理命令、schema v4 迁移、CLI JSON 兼容输出、secret 脱敏和 awiki-cli 边界均已审查；发现项已修复，残余风险记录在第 9 节。 |
| 验证证据 | `CARGO_BUILD_JOBS=1 cargo fmt --all --check` 通过；`CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked` 通过；`CARGO_BUILD_JOBS=1 cargo test --workspace --locked` 通过；边界、旧字段、日志和 diff 检查通过。 |
| 下一步 | 开始步骤 08：集成、系统测试与发布门禁。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：daemon 能注册或恢复 Daemon Agent DID，能用 registration token 创建 Runtime Agent DID 记录，并暴露最小 daemon 管理命令。
- 可见行为：App/controller 可通过 `application/json + body.payload` command 创建 runtime agent；daemon 写入 `agent_definition`、`runtime_profile`、`workspace_binding`、`controller_did` 并回报 ready/status。
- 非目标：不实现复杂 installer、多 runtime 插件、多 controller proof、group workflow 或 Hermes/OpenClaw 原生插件。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `crates/awiki-deamon/src/agent/` | Daemon agent 和 runtime agent definition。 | 包含 `controller_did` 模型。 |
| `crates/awiki-deamon/src/commands/` | payload command 解析器和处理器。 | 使用 `application/json + body.payload`。 |
| `crates/awiki-deamon/src/registration/` | user-service registration token client。 | 使用步骤 06 API。 |
| `crates/awiki-deamon/src/daemon_cli/` | 最小管理命令。 | 与 awiki-cli 分离。 |
| `crates/awiki-deamon/src/state/` | agent/runtime/profile/workspace 持久化。 | 单个 `daemon.db`。 |
| `crates/im-core/src/identity/` | 如适合，增加 token-based agent registration client API。 | 避免 daemon 长期原始调用 user-service。 |
| user-service | 如果步骤 06 发现缺口，做小范围修正。 | 范围扩大需先更新计划。 |
| message-service | 如果步骤 05 发现互操作缺口，做小范围修正。 | 范围扩大需先更新计划。 |
| `crates/awiki-deamon/docs/` | daemon 命令设计和 agent lifecycle 文档。 | 必须澄清 awiki-cli 边界。 |

## 4. 依赖

- 前置步骤：步骤 01、02、03、04、05、06。
- 外部决策：registration token API、payload 契约、daemon 架构文档。
- 环境前提：可访问 user-service 和 message-service 测试端点。

## 5. 核心设计

管理流程：

```text
App/controller 获取 registration token
  -> daemon setup 兑换 daemon_registration_token
  -> daemon 创建或恢复 Daemon Agent DID
  -> controller 向 daemon agent 发送 application/json + body.payload runtime.agent.create command
  -> daemon 校验 sender_did == controller_did
  -> daemon 兑换 runtime_agent_registration_token
  -> daemon 写入 agent_definition/runtime_profile/workspace_binding
  -> daemon 安装或准备 runtime 插件
  -> daemon 发送 ready/status payload response
```

daemon CLI 命令应比 awiki-cli 小，聚焦管理和诊断：

- `daemon status`
- `daemon doctor`
- `agent list`
- `agent status`
- `runtime list`
- `runtime check`
- `runtime start`
- `runtime stop`

步骤 02 的 runtime Skill CLI 封装器是本地 RPC client，不应扩展成完整用户 CLI。

## 6. 实施指引

1. 定义 daemon agent setup command 和 config storage。
2. 实现 user-service registration token client 或 `im-core` wrapper。
3. 实现 agent definition 生命周期：
   - create。
   - list。
   - status。
   - disable/delete，如第一版纳入。
4. 实现 `runtime.agent.create` payload command parser 和校验。
5. 使用简单 MVP 规则校验 `controller_did`。
6. 实现 runtime profile/workspace binding 创建。
7. 实现 ready/status payload response。
8. 增加最小 daemon 管理命令。
9. 尽量使用 mocked user-service 和 message-service 增加测试。
10. 更新命令面和边界文档。

## 7. 验收标准

- [x] Daemon Agent DID setup 或 restore path 已实现。
- [x] Runtime Agent DID 创建使用 user-service registration token API。
- [x] `controller_did` 简单模型已强制执行并文档化。
- [x] `runtime.agent.create` 使用 `application/json + body.payload`。
- [x] command/status/result 路由使用 payload 字段，不使用专用 JSON 内容类型。
- [x] daemon 管理命令与 awiki-cli 命令分离。
- [x] agent/runtime/profile/workspace 记录持久化到 `daemon.db`。
- [x] daemon 通过 `im-core` 发送 ready/status response。
- [x] 测试覆盖成功和授权失败。
- [x] 审查发现已修复或明确记录。
- [x] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 格式通过。 |
| Rust 测试 | `cargo test --workspace --locked` | daemon 和 SDK 测试通过。 |
| 服务契约测试 | 如 user-service/message-service 有调整，运行对应聚焦测试。 | 没有契约回归。 |
| 冒烟验证 | mocked `runtime.agent.create` 流程 | 产生 agent 记录和 ready/status response。 |
| 边界检查 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部。 |
| 字段检查 | 搜索旧字段名和旧内容类型 | 没有旧字段或旧 command/status 内容类型。 |

实际执行：

- `CARGO_BUILD_JOBS=1 cargo fmt --all --check` 通过。
- `CARGO_BUILD_JOBS=1 cargo test -p awiki-deamon --locked` 通过。
- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked` 通过。
- `git diff --check -- Cargo.toml Cargo.lock crates/awiki-deamon` 通过。
- `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无结果。
- `rg -n "structured_json|application/vnd\\.awiki\\.agent-(command|status|result|task)\\+json" crates/awiki-deamon crates/im-core crates/im-core-dart` 无结果。
- `rg -n "println!|eprintln!|dbg!|log::|tracing::" crates/awiki-deamon/src` 仅命中 `src/main.rs` 的 CLI stdout/stderr 输出。
- `rg -n "registration_token|private_key|token_secret|auth_private_key|e2ee_.*private" crates/awiki-deamon/src crates/awiki-deamon/tests` 命中字段定义、数据库存储、测试用例和脱敏断言；未发现生产代码打印 token/private key。

## 9. 代码 Review

实现后、提交前进行审查，重点检查 registration token 消费、`controller_did` 假设、payload parsing、daemon CLI 边界、持久化、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 已处理 | secret-bearing request/identity 类型可 serde 序列化；同步 registration client 在已有 Tokio runtime 内直接创建 runtime 并 `block_on`；新增 `run_command_json` 初版改变了 `status` 输出形状；恢复已有 daemon handle 时未校验 requested `controller_did`；schema v4 对旧 `agent_definition` 行缺少回填；文档中仍出现旧结构化 JSON 字段字面量。 |
| 已修复 | 已完成 | 移除 secret-bearing 类型的 serde 派生；已有 Tokio runtime 内改用独立线程 current-thread runtime 执行 registration RPC；`run_command_json` 对 `status`、`agent-list`、`agent-status`、`runtime-list` 输出原始对象；恢复 daemon handle 时校验 controller mismatch；v4 migration 回填 handle、agent_kind、policy、runtime plugin 和本地 DB 路径；文档改为不出现旧字段名。 |
| 残余风险 | 已记录 | daemon 侧使用 mocked user-service/message-service 测试完成本地闭环；真实跨服务 registration token 兑换、真实 message-service 投递和系统级 E2E 留到步骤 08。`controller_did` 仍是 MVP 简单边界，高风险自动执行需要后续 approval 或 sandbox 策略。 |
| 测试缺口 | 可接受 | 已覆盖成功创建、非 controller 拒绝、registration token 失败不持久化 runtime agent、daemon 管理命令、schema v4、Debug 脱敏和 workspace 全量测试；缺口是跨仓真实服务 E2E，归入步骤 08。 |
| 文档缺口 | 已修复 | 更新 `docs/local-dev.md` 的 agent 管理、DB 状态、daemon CLI 和 `application/json + body.payload` 规则；本步骤文档和主计划台账记录提交、审查、验证和残余风险。 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：agent registration/management 实现、测试和文档。
- 提交前记录：`git status --short --branch` 显示分支 ahead 1，包含 `Cargo.lock`、`crates/awiki-deamon/Cargo.toml`、`docs/local-dev.md`、agent/commands/daemon_cli/registration/outbox/state/main/lib 和测试文件改动。
- 提交后记录：实现提交 `9ac7b4e`；提交后 `git status --short --branch` 显示分支 ahead 2，工作区无未提交代码改动。
- 提交信息：`daemon: add agent registration management`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：真实跨服务 registration token 兑换和 message-service 投递尚未在系统测试中闭环；daemon 命令面仍是最小管理命令；`controller_did` 对高风险 runtime 仍偏弱。
- 回滚：保留手工 runtime profile 创建作为 MVP 回退路径，禁用 payload management command，或回退实现提交 `9ac7b4e`。
- 后续：步骤 08 增加跨仓 E2E、真实服务集成验证、发布门禁和阶段 C Review。
