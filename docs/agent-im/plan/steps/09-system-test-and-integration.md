# Step 09：系统测试与集成收口

主 Plan：[../plan.md](../plan.md)  
Step index：09  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | 待执行者填写 |
| Started | - |
| Completed | - |
| Commit | - |
| Review evidence | - |
| Verification evidence | - |
| Next action | 完成跨仓库端到端验证、remote 系统测试、全局 Review 和台账收口 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：验证 APP bootstrap -> Daemon user delegated identity -> ensure message agent -> delegated inbox/send -> message sync/action -> APP 展示/执行 的完整链路。
- 用户 / 系统可见行为：真实或接近真实环境中，用户创建 DID 后可一次性启用消息处理 Agent；普通消息可被 Agent 处理；E2EE 消息不进入 Agent；系统 payload 不污染聊天。
- 非目标：不把失败的 remote 系统测试标记为通过；不在此步骤补做大规模功能开发，除非是集成修复且先更新 Plan。
- 完成标准：所有前置 Step done；全局 Review 完成；`awiki-system-test` remote `awiki.info` 模式执行并记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置；最终工作区状态记录。

## 3. 设计方法

- 设计边界：本步骤是集成收口和证据收集，不替代各步骤 unit/integration tests。
- 核心决策：系统测试必须使用 `AWIKI_SYSTEM_TEST_MODE=remote` 与 `awiki.info`；命令示例可调整，但实际命令必须记录。
- 契约 / API / 数据流：测试覆盖 user-service DID key、message-service delegated policy、awiki-me bootstrap、awiki-deamon ensure/sync/action。
- 兼容性：同时跑必要旧链路 smoke，确认普通 APP messaging、Daemon agent command、E2EE opaque boundary 不回归。
- 迁移策略：如涉及数据库迁移，先在测试环境执行迁移并记录版本。
- 风险控制：remote 环境不可用或依赖服务未部署时，记录 blocker 和替代证据，不伪造通过。

## 4. 实现方法

1. 阅读 `awiki-system-test` 的 README、脚本帮助和现有测试用例，确认 remote `awiki.info` 模式实际命令。
2. 如现有系统测试缺少覆盖，新增或扩展测试用例：
   - 用户 DID 创建默认包含 `#daemon-key-1`；
   - APP 侧生成 daemon subkey private package，user-service 只登记 public registration；
   - 同一 APP 重复提交相同 public key 幂等，提交不同 public key conflict；
   - APP 通过 message-service 普通消息发送 `awiki.daemon.bootstrap.v1` 明文 JSON system/control payload；
   - bootstrap private package 在 MVP 中经过普通消息发送路由到 Daemon，但不显示为普通聊天、不进入 Hermes prompt、日志或 audit detail；
   - Daemon 写入 user delegated identity；
   - Daemon `ensure_app_message_agent` 幂等；
   - message-service delegated send/inbox/history 只处理普通非 E2EE；
   - message-service MVP 校验 DID proof、DID Document `authentication`、key owner 一致性和普通非 E2EE scope；
   - 同 DID APP/Daemon 多连接 fanout；
   - E2EE opaque notification 到 Daemon 后 ignored；
   - 普通非 E2EE 消息进入 Hermes 前使用 untrusted content envelope，`message_event` 使用最小投影；
   - APP action 最小 allowlist 和 payload filter。
3. 执行所有相关仓库的最终验证命令，并把结果回填主 Plan 第 7、11、17 节和本 Step 执行状态。
4. 执行全局 Review：检查所有 changed files、public schema、文档、测试、迁移、安全/隐私边界、错误命名和未提交变更。
5. 如果系统测试或全局 Review 需要小修，先更新 Plan 变更记录，再做修复、验证、Review、commit。
6. 最终记录每个仓库 `git status --short --branch`，确保没有遗漏未提交完成工作。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-system-test` | 新增或运行端到端测试 | 按仓库 README 和实际脚本 |
| `awiki-cli-rs2/docs/agent-im/plan/plan.md` | 回填执行台账、最终 Review、验证证据 | 文档更新属于本步骤 |
| `awiki-cli-rs2/docs/agent-im/plan/steps/*.md` | 回填各 Step 状态或最终证据 | 如实现过程中已回填则本步骤核对 |
| `user-service` | 最终 status 和必要 smoke | 不做新功能 |
| `message-service` | 最终 status、迁移和 smoke | 不做新功能 |
| `awiki-cli-rs2` | 最终 status、cargo tests | 不做新功能 |
| `awiki-me` | 最终 status、Flutter tests | 不做新功能 |

## 6. 依赖

- 前置步骤：Step 01-08 已完成、Review、验证并提交。
- 外部文档或决策：`awiki-cli-rs2/AGENTS.md` 对系统测试的强制要求；`awiki-harness/context/40-verification.md` 与 `50-task-workflow.md`。
- 环境前提：`awiki-system-test` 可访问 remote `awiki.info`；必要服务已部署或可由测试脚本配置。

## 7. 验收标准

- [ ] Step 01-08 的执行状态均为 `done`，commit hash 和验证证据完整。
- [ ] 所有相关仓库最终 unit/integration 验证完成，无法运行的命令记录原因和替代证据。
- [ ] remote `awiki.info` 系统测试执行并记录实际命令。
- [ ] 系统测试记录通过/失败/跳过数量、失败或跳过原因、关键环境配置。
- [ ] 全局 Review 没有未处理 P0/P1 问题；剩余风险已记录。
- [ ] 错误命名检查通过，不出现新增旧候选 naming family，所有新增收件箱授权命名保持 `inbox_*`。
- [ ] 最终 `git status --short --branch` 已记录。
- [ ] 如果本步骤修改测试或文档，已经创建聚焦最终集成 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| user-service | `cd user-service && uv run pytest tests/app/did -v` | 记录通过/失败/跳过数量。 |
| awiki-cli-rs2 | `cd awiki-cli-rs2 && cargo test -p im-core --locked && cargo test -p awiki-deamon --locked` | 记录通过/失败/跳过数量或 Rust test summary。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | analyze clean，tests 通过。 |
| message-service | `cd message-service && cargo test --workspace` | workspace tests 通过；如运行 clippy 也记录。 |
| system remote | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote uv run python manage_local_test_env.py run-tests --domain awiki.info` | 实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。 |
| naming | `PATTERN="$(printf '%s|%s|%s|%s|%s|%s' 'message_''owner|message_''auth' 'Message''Access' 'Scoped''Message' 'mailbox_''owner' 'Scoped''Mailbox' 'Scoped''MailboxToken')" && rg -n "$PATTERN" awiki-cli-rs2/docs/agent-im/plan awiki-cli-rs2/docs/agent-im/*.md` | 不出现错误命名残留；若检查命令本身被命中，需调整为脚本变量形式。 |
| final status | 对每个相关仓库运行 `git status --short --branch` | 没有遗漏未提交完成工作；允许用户既有无关变更但必须记录。 |

如果 `awiki-system-test` 当前命令参数不同，执行者必须先查看仓库 README 或脚本帮助，使用实际命令，并在主 Plan 和本 Step 中记录差异。

## 9. Review 环节

- Review 时机：系统测试前做一次 readiness Review；系统测试后做最终全局 Review；最终 commit 前再核对工作区。
- Review 重点：跨仓库契约一致性、E2EE boundary、子私钥安全、runtime token scope、APP action allowlist、schema 可见性、system-test 证据、Plan 台账完整性、未提交变更。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待填写 | - |
| 已修复问题 | 待填写 | - |
| 剩余风险 | 待填写 | - |
| 新增或缺失测试 | 待填写 | - |
| 已更新或缺失文档 | 待填写 | - |

## 10. Commit 要求

- Commit 时机：本步骤新增/修改系统测试、计划台账或收口文档并验证后。
- Commit 范围：只包含系统测试、最终文档证据和必要集成修复；大功能修复必须回到对应 Step 或更新 Plan。
- Commit 前状态：记录每个相关仓库的 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`agent-im: add system integration coverage`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| remote `awiki.info` 环境不可用 | 待填写实际错误 | 重试、检查脚本帮助、确认环境变量、运行局部替代测试 | 整体上线验收 | 标记 blocked，不伪造通过，等待环境恢复或用户决策 |
| 系统测试脚本缺少目标用例 | 待填写 | 新增 focused E2E 用例或记录 manual evidence | 当前步骤 | 修改测试后 Review/commit |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 09 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：系统测试环境不稳定导致无法证明端到端完成。
- 回滚 / 回退：保留各仓库 unit/integration 证据，但主 Plan 不标记最终完成；等待 remote 环境恢复后重跑。
- 后续文档：最终更新主 Plan 第 17 节，必要时在相关仓库 changelog/PR notes 记录 rollout、迁移和已知限制。
