# 步骤 06：通用 CLI 运行时插件 MVP

主计划：[../plan.md](../plan.md)
步骤编号：06
状态：草稿

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待开始 |
| 分支 | `feature/release-0526/awiki-deamon` |
| 开始时间 | 待定 |
| 完成时间 | 待定 |
| 提交 | 待定 |
| 审查证据 | 待定 |
| 验证证据 | 待定 |
| 下一步 | 本地 RPC 完成后，实现最小通用 CLI 运行时插件。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：daemon 能把手工配置的 runtime agent 文本任务路由到无界面 CLI runtime，并通过 CLI 封装器和本地 RPC 收到状态和最终输出。
- 可见行为：controller 文本消息创建 runtime run；runtime 执行一次；Skill/封装器上报 status 和 final；daemon 通过 `im-core` 发送状态/结果并记录 run/audit。
- 非目标：不实现完整 Claude Code/Codex/Gemini driver；不实现 workspace 强隔离；不引入 RuntimeEvent 作为第二条权威状态通道。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `crates/awiki-deamon/src/runtime/` | RuntimeTask、RuntimeRun、plugin trait 和 runner。 | daemon 自有抽象。 |
| `crates/awiki-deamon/src/plugins/generic_cli/` | 最小无界面 CLI runtime 插件。 | 先用测试替身 driver。 |
| `crates/awiki-deamon/src/workspace/` | workspace binding 和 mode validation。 | 记录 mode，不夸大安全性。 |
| `crates/awiki-deamon/src/state/` | runtime run、session mapping、audit 持久化。 | 单个 `daemon.db`。 |
| `crates/awiki-deamon/src/inbox/` | controller 文本任务分类。 | `controller_did` 简单 MVP 模型。 |
| `crates/awiki-deamon/docs/` | runtime 插件和 workspace 文档。 | 包含 workspace mode 安全边界表。 |
| `crates/awiki-deamon/tests/` | 使用测试 CLI 的 runtime 插件测试。 | 不依赖真实外部 CLI。 |

## 4. 依赖

- 前置步骤：步骤 04 和步骤 05。
- 外部文档：架构文档中的 RuntimeTask、workspace-bound 模型和 callback chain。
- 环境前提：测试中可在临时目录生成测试 CLI binary/script。

## 5. 核心设计

MVP 流程：

```text
controller 文本消息
  -> daemon 校验 sender_did == controller_did
  -> 创建 RuntimeTask
  -> 通用 CLI 插件创建 RuntimeRun
  -> daemon 签发 runtime_rpc_token
  -> 插件启动无界面 CLI，并注入封装器环境/config
  -> runtime 调封装器 task.status/task.finish/msg.send
  -> daemon 校验 token，通过 im-core 发消息
  -> daemon 记录 run/audit
```

workspace mode：

| 模式 | 第一版行为 |
|---|---|
| `shared-root` | 可用于个人低风险/读任务；不是安全边界。 |
| `worktree-per-task` | 可为每次 run 创建 git worktree，用于代码变更隔离；不防系统凭据读取。 |
| `container / sandbox` | 后续或可选模式，才可作为安全边界。 |

RuntimeEvent 在本步骤只做观察/日志。权威状态和结果通道是 Skill / daemon CLI 封装器 / 本地 RPC。

## 6. 实施指引

1. 定义 daemon 自有 `RuntimeTask` 和 `RuntimeRun` DTO。
2. 定义最小 `RuntimePlugin` trait：
   - check/install status。
   - prepare run。
   - launch run。
   - observe completion。
3. 先用测试替身 driver 实现通用 CLI 插件。
4. 增加手工 runtime agent profile config loader。
5. 使用静态 `controller_did` 实现 controller 文本任务路由。
6. 每个 run 生成 `runtime_rpc_token`，并注入封装器环境。
7. 持久化 run status 和 audit。
8. 增加测试 CLI 调封装器或 mock RPC client 的测试。
9. 增加 MVP 手工配置文档和已知限制。

## 7. 验收标准

- [ ] 能加载手工 runtime agent config。
- [ ] controller 文本任务能创建 RuntimeTask 和 RuntimeRun。
- [ ] 通用 CLI 插件能启动测试替身/无界面 runtime。
- [ ] runtime callback 使用步骤 05 的本地 RPC 和 token 校验。
- [ ] daemon 通过 `im-core` 或 testable adapter 发送 status/final message。
- [ ] run 和 audit 记录被持久化。
- [ ] RuntimeEvent 不作为第二条状态权威来源。
- [ ] workspace mode 限制已文档化。
- [ ] 审查发现已修复或明确记录。
- [ ] 完成验证和审查后，为本步骤创建聚焦提交。

## 8. 验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 格式通过。 |
| daemon 测试 | `cargo test -p awiki-deamon --locked` 或 workspace 等价命令 | runtime 插件测试通过。 |
| workspace 测试 | `cargo test --workspace --locked` | 无 workspace 回归。 |
| 测试 runtime 冒烟验证 | 使用 temp state 和测试 CLI 跑冒烟验证 | 产生 status/final callback 记录。 |
| 状态源检查 | `rg -n "RuntimeEvent" crates/awiki-deamon/src` | 如存在，仅用于观察/日志。 |
| 文档 | `git diff --check -- crates/awiki-deamon/docs` | 文档 diff 干净。 |

## 9. 审查过程

实现后、提交前进行审查，重点检查 task routing、`controller_did` 假设、runtime launch 隔离、token 注入、状态通道、持久化、测试和文档。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待定 | 待定 |
| 已修复 | 待定 | 待定 |
| 残余风险 | 待定 | 待定 |
| 测试缺口 | 待定 | 待定 |
| 文档缺口 | 待定 | 待定 |

## 10. 提交要求

- 提交时机：实现、验证和审查完成后。
- 提交范围：通用 CLI 运行时插件 MVP、测试和文档。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`daemon: add generic cli runtime mvp`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：测试 CLI 冒烟验证可能掩盖真实 runtime 差异；workspace mode 可能被误解为隔离。
- 回滚：插件默认禁用，只允许手工 profile 开启。
- 后续：步骤 07 增加产品化 daemon/runtime agent 管理。
