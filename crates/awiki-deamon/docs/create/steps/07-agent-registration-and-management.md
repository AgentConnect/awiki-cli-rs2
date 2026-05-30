# 步骤 07：daemon agent 与 runtime agent 管理

主计划：[../plan.md](../plan.md)
步骤编号：07
状态：草稿

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待开始 |
| 分支 | 当前仓库、user-service、message-service 相关集成分支 |
| 开始时间 | 待定 |
| 完成时间 | 待定 |
| 提交 | 待定 |
| 审查证据 | 待定 |
| 验证证据 | 待定 |
| 下一步 | MVP 闭环完成后，产品化 daemon/runtime agent 注册和管理。 |

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

- [ ] Daemon Agent DID setup 或 restore path 已实现。
- [ ] Runtime Agent DID 创建使用 user-service registration token API。
- [ ] `controller_did` 简单模型已强制执行并文档化。
- [ ] `runtime.agent.create` 使用 `application/json + body.payload`。
- [ ] command/status/result 路由使用 payload 字段，不使用专用 JSON 内容类型。
- [ ] daemon 管理命令与 awiki-cli 命令分离。
- [ ] agent/runtime/profile/workspace 记录持久化到 `daemon.db`。
- [ ] daemon 通过 `im-core` 发送 ready/status response。
- [ ] 测试覆盖成功和授权失败。
- [ ] 审查发现已修复或明确记录。
- [ ] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 代码验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 格式通过。 |
| Rust 测试 | `cargo test --workspace --locked` | daemon 和 SDK 测试通过。 |
| 服务契约测试 | 如 user-service/message-service 有调整，运行对应聚焦测试。 | 没有契约回归。 |
| 冒烟验证 | mocked `runtime.agent.create` 流程 | 产生 agent 记录和 ready/status response。 |
| 边界检查 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部。 |
| 字段检查 | 搜索旧字段名和旧内容类型 | 没有旧字段或旧 command/status 内容类型。 |

## 9. 代码 Review

实现后、提交前进行审查，重点检查 registration token 消费、`controller_did` 假设、payload parsing、daemon CLI 边界、持久化、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待定 | 待定 |
| 已修复 | 待定 | 待定 |
| 残余风险 | 待定 | 待定 |
| 测试缺口 | 待定 | 待定 |
| 文档缺口 | 待定 | 待定 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：agent registration/management 实现、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`daemon: add agent registration management`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：user-service token API 未就绪；daemon 命令面过宽；`controller_did` 对高风险 runtime 仍偏弱。
- 回滚：保留手工 runtime profile 创建作为 MVP 回退路径，禁用 payload management command。
- 后续：步骤 08 增加跨仓 E2E 和发布门禁。
