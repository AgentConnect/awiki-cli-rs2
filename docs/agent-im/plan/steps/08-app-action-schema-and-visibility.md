# Step 08：APP action schema 与可见性

主 Plan：[../plan.md](../plan.md)  
Step index：08  
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
| Next action | 收敛 `message.sync`、APP capability、action request/result schema 和最小 allowlist |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：实现 MVP 最小 APP action allowlist、action request/result schema、message sync schema 和可见性过滤，避免新 JSON schema 被 Daemon 忽略或被 APP 当普通聊天显示。
- 用户 / 系统可见行为：Hermes Message Agent 可以总结普通消息、生成草稿、读取联系人、在受控确认下更新联系人显示名/备注；APP 能展示 action 状态和结果，不把系统 payload 混进普通聊天。
- 非目标：不开放 `message.send`、E2EE forward、删除/导出/身份密钥变更；不实现完整自动化能力配置、撤销、审计面板；不实现 Phase 4 高级自动化。
- 完成标准：`awiki.app.capabilities.v1`、`awiki.app.action.v1`、`awiki.app.action.result.v1`、`awiki.message.sync.v1` 在 Daemon 与 APP 两侧可解析、过滤、测试；最小 allowlist 生效。

## 3. 设计方法

- 设计边界：Agent 可以很强，但 MVP 必须可控；本步骤只做必要协议和最小能力，不做完整策略产品。
- 核心决策：能力 allowlist 固定为 `message.summarize_plain`、`message.create_draft`、`contact.read`、`contact.update_display_name`、`contact.update_note`。联系人写操作需要 APP 侧确认或明确策略授权。
- 契约 / API / 数据流：APP 发布 capability registry；Daemon/Hermes 只能看到 runtime token scope 允许的能力；Agent 发 `awiki.app.action.v1`；APP 执行或要求确认后返回 `awiki.app.action.result.v1`；message processing 状态用 `awiki.message.sync.v1`。
- 兼容性：未知 `awiki.*` system schema 默认隐藏并进入 system dispatch 或 unsupported 状态，不显示为普通聊天。
- 迁移策略：现有 `awiki.agent.command.v1` / `awiki.agent.status.v1` 保留；新 schema 通过 adapter 兼容。
- 风险控制：高风险动作 deny by default；E2EE 内容相关 action 在 MVP 中不存在；action payload 不得含 private key 或 E2EE private state。

## 4. 实现方法

1. 在 Daemon 侧定义 schema DTO 和 dispatch：`awiki.app.capabilities.v1`、`awiki.app.action.v1`、`awiki.app.action.result.v1`、`awiki.message.sync.v1`。
2. 在 runtime token 或 capability policy 中加入最小 allowlist，超出 allowlist 的 action request 直接拒绝并记录 result。
3. 在 APP 侧定义对应 Dart model 和 reducer，支持 action 状态：requested、requires_confirmation、accepted、rejected、succeeded、failed。
4. 实现联系人相关 action 的确认策略：`contact.read` 可自动；`contact.update_display_name` 和 `contact.update_note` 需要用户确认或策略授权。
5. 实现 message summary/draft 的展示和存储边界：只针对普通非 E2EE消息；结果作为 summary/draft，不自动发送。
6. 加强 `ChatMessage` 或 message adapter 过滤：system/control payload 不进入普通聊天渲染；未知 `awiki.*` schema 默认隐藏或显示系统状态。
7. 增加测试：allowlist 拒绝、联系人确认、普通消息 summary/draft、E2EE action 不存在或拒绝、unknown schema hidden、result 回传。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/security/runtime_token.rs` | action scope 与 allowlist | Runtime 能力边界 |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/action.rs` | 新增 action schema / dispatch | 如目录不存在则新增 |
| `awiki-cli-rs2/crates/awiki-deamon/src/inbox/mod.rs` | system/control payload dispatch | 与 Step 05 对齐 |
| `awiki-cli-rs2/crates/awiki-deamon/src/outbox/mod.rs` | action result / message sync 回传 | 视现有 outbox |
| `awiki-me/lib/src/domain/entities/agent/agent_control_payloads.dart` | 新增 capability/action/result model | 与 Daemon schema 对齐 |
| `awiki-me/lib/src/domain/entities/chat_message.dart` | 隐藏 system/control payload | 防止普通聊天污染 |
| `awiki-me/lib/src/application/messaging_service.dart` | action result / message sync 处理 | 视当前架构 |
| `awiki-me/lib/src/presentation/chat/*` | 展示 summary/draft/action status | 保持工作型 UI，避免说明文案堆叠 |
| `awiki-cli-rs2/packages/awiki_im_core/lib/src/generated/*` | 如公共 DTO 需要同步 | 仅在 binding 变更时 |

## 6. 依赖

- 前置步骤：Step 04 message agent binding；Step 05 message sync/outbox 入口；Step 06 APP bootstrap 与 payload filter 基础。
- 外部文档或决策：`agent_im_core_design.md` 第 1.2.5、2.2.3、4、5.7；用户确认 MVP 最小能力清单。
- 环境前提：awiki-deamon Rust tests 与 awiki-me Flutter tests 可运行。

## 7. 验收标准

- [ ] Daemon/APP 都能解析 `awiki.app.capabilities.v1`、`awiki.app.action.v1`、`awiki.app.action.result.v1`、`awiki.message.sync.v1`。
- [ ] MVP allowlist 只包含 5 个能力：`message.summarize_plain`、`message.create_draft`、`contact.read`、`contact.update_display_name`、`contact.update_note`。
- [ ] `message.send`、E2EE forward、删除/导出/身份密钥变更默认拒绝。
- [ ] 联系人写操作有 APP 侧确认或策略授权路径。
- [ ] system/control payload 和未知 `awiki.*` schema 不显示为普通聊天。
- [ ] action result 能回传给 Daemon/Hermes，并可同步到 APP 状态。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit；如跨仓拆 commit，台账必须说明。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Daemon unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | allowlist、dispatch、result/outbox 测试通过。 |
| APP tests | `cd awiki-me && flutter test` | payload filter、action reducer、UI state 测试通过。 |
| APP analyze | `cd awiki-me && flutter analyze` | 无新增分析错误。 |
| Naming | `PATTERN="$(printf '%s|%s|%s|%s|%s|%s' 'message_''owner|message_''auth' 'Message''Access' 'Scoped''Message' 'mailbox_''owner' 'Scoped''Mailbox' 'Scoped''MailboxToken')" && rg -n "$PATTERN" awiki-cli-rs2 awiki-me` | 不引入错误新增命名；历史无关残留需说明。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：权限绕过、普通聊天污染、未知 schema 处理、联系人写确认、E2EE 禁止、runtime token scope、APP/Daemon schema 一致性。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待填写 | - |
| 已修复问题 | 待填写 | - |
| 剩余风险 | 待填写 | - |
| 新增或缺失测试 | 待填写 | - |
| 已更新或缺失文档 | 待填写 | - |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：Daemon 和 APP 的 schema/action/filter 变化可以按仓库拆 commit；如果必须保持契约同步，台账记录跨仓 commit 顺序。
- Commit 前状态：记录每个相关仓库的 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`agent-im: add app action schemas`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| APP 联系人模型缺少备注更新入口 | 待填写 | 先实现 `contact.read` 和 `message.create_draft`，将写操作标记 unsupported | 当前步骤 | 更新 Plan 并记录 MVP 能力缩减 Review |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 08 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：Agent action 权限过宽导致用户数据被静默修改。
- 回滚 / 回退：关闭 action allowlist，仅保留 message sync 展示；APP 对未知 action 返回 rejected。
- 后续文档：实现后更新 Agent IM schema 文档，记录 MVP action allowlist 和 Phase 4 后续原则。
