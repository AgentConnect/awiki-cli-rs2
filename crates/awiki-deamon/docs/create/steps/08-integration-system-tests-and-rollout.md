# 步骤 08：集成、系统测试与发布门禁

主计划：[../plan.md](../plan.md)
步骤编号：08
状态：草稿

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | 待开始 |
| 分支 | 集成分支 |
| 开始时间 | 待定 |
| 完成时间 | 待定 |
| 提交 | 待定 |
| 审查证据 | 待定 |
| 验证证据 | 待定 |
| 下一步 | 步骤 01 到 07 完成后运行跨仓集成。 |

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

## 2. 目标

- 结果：证明协议、SDK、服务端和 daemon 的端到端运行时宿主流程可运行，并准备发布门禁。
- 可见行为：controller 可以向 daemon/runtime agent 发送文本和 payload command；服务端能传输；daemon 执行 MVP runtime 并返回 status/final；安全和 audit 证据完整。
- 非目标：不新增超出步骤 01 到 07 的产品范围。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 说明 |
|---|---|---|
| `awiki-system-test/` | 增加 payload、token、daemon MVP 闭环 E2E。 | 跨服务权威验证。 |
| `awiki-harness/context/` | 只有架构路由或验证策略发生变化时更新。 | 避免无关变更。 |
| `crates/awiki-deamon/docs/create/plan.md` | 完成执行账本和证据记录。 | Goal 完成的来源。 |
| `crates/awiki-deamon/docs/` | 发布、安全、操作说明。 | 包含已知限制。 |
| 子仓库文档 | 修正集成中发现的文档漂移。 | 范围必须聚焦。 |

## 4. 依赖

- 前置步骤：步骤 01 到 07。
- 外部契约：所有前序步骤的实现和记录。
- 环境前提：本地 AWiki stack 能启动 user-service、message-service v2 和 daemon executable。

## 5. 核心设计

集成验证要覆盖纵向链路，而不只做分层单测：

1. `application/json + body.payload` 协议测试夹具。
2. SDK 发送 direct/group payload。
3. message-service 存储并投递 payload。
4. user-service 签发和兑换 registration token。
5. daemon 注册或加载 agent identity。
6. daemon 接收 controller 文本任务和 payload command。
7. 通用 CLI 运行时插件运行测试替身/无界面 task。
8. runtime 调 CLI 封装器本地 RPC。
9. daemon 发送 status/final message。
10. audit 记录 `token_id`、`run_id` 和 result，不记录原始 secret。

## 6. 实施指引

1. 确认 `awiki-system-test` 中 message-v2 本地启动命令。
2. 先增加聚焦测试夹具：
   - direct/group payload 往返校验。
   - registration token 成功路径和失败路径。
   - daemon 本地 RPC token 失败路径。
3. 增加使用测试 CLI runtime 的 daemon MVP E2E。
4. 记录启动要求、端口和环境变量。
5. 运行聚焦 suite 并收集日志。
6. 做安全审查：
   - token 原文不在日志中。
   - socket 权限正确。
   - audit 只记录 `token_id`。
   - method level enforcement 生效。
   - workspace mode warning 存在。
7. 更新文档和本计划执行账本。
8. 如果集成发现契约漂移，先更新本计划，再改变前序步骤范围。

## 7. 验收标准

- [ ] `application/json + body.payload` direct/group E2E 通过。
- [ ] SDK 能解析 service 返回的 payload history/incoming message。
- [ ] user-service registration token 成功路径和失败路径通过。
- [ ] daemon 本地 RPC token 安全测试通过。
- [ ] daemon MVP 测试 runtime 闭环通过。
- [ ] 安全审查证据已记录。
- [ ] 文档反映实际命令和已知限制。
- [ ] 执行账本记录每个步骤的提交和验证证据。
- [ ] 审查发现已修复或明确记录。
- [ ] 如本步骤产生文件变更，创建聚焦提交。

## 8. 验证

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 系统环境 | `cd ../awiki-system-test && <local message-v2 startup command>` | 必需服务健康。 |
| 系统测试 | `cd ../awiki-system-test && <focused daemon/payload suite>` | E2E 测试通过。 |
| 当前仓库测试 | `cargo test --workspace --locked` | SDK/daemon 测试通过。 |
| message-service 测试 | `cd ../message-service && cargo test --workspace --locked` | 服务测试通过。 |
| user-service 测试 | `cd ../user-service && uv run pytest tests -v` | token 测试通过。 |
| 文档检查 | `git diff --check -- crates/awiki-deamon/docs` 和子仓库 docs | 文档 diff 干净。 |
| 安全审查 | 手工检查清单记录到本步骤文档 | 没有未解决的严重发现。 |

## 9. 审查过程

集成完成后、最终提交前进行审查，重点检查跨仓行为、契约漂移、测试覆盖、发布文档、安全/隐私和残余风险。

| 审查项 | 结果 | 说明 |
|---|---|---|
| 发现 | 待定 | 待定 |
| 已修复 | 待定 | 待定 |
| 残余风险 | 待定 | 待定 |
| 测试缺口 | 待定 | 待定 |
| 文档缺口 | 待定 | 待定 |

## 10. 提交要求

- 提交时机：集成验证和审查完成后。
- 提交范围：system tests、文档和必要集成修复。
- 提交前记录：`git status` 和纳入文件。
- 提交后记录：commit hash 和提交后的 `git status`。
- 建议提交信息：`test: add daemon runtime host integration coverage`

## 11. 阻塞处理

| 阻塞 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|
| 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划记录 |
|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 |

## 13. 风险、回滚与后续

- 风险：本地测试环境可能尚不支持 daemon 进程生命周期；跨仓分支协调成本高。
- 回滚：功能保持配置禁用，只交付协议消费、SDK/service 支持，等 daemon E2E 稳定后再开启。
- 后续：MVP 发布后，分别创建 Claude Code driver、Hermes/OpenClaw 原生插件、workspace sandbox 加固和未来 proof/delegation 计划。
