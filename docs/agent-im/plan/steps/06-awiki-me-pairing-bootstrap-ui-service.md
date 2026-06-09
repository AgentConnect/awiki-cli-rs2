# Step 06：awiki-me bootstrap UI 与 service

主 Plan：[../plan.md](../plan.md)  
Step index：06  
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
| Next action | 在 awiki-me 中接入 DID 创建时已有 daemon key 和一次性 bootstrap service |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`awiki-me` 在用户 DID 创建时本地生成 `user_did#daemon-key-1` private package，把 public registration 交给 user-service，并通过 message-service 普通消息发送把本地既有 private package 一次性发送给 Daemon，同时展示 bootstrap 与 message agent 状态。
- 用户 / 系统可见行为：用户安装 Daemon 后，APP 发送一次声明式 bootstrap system/control payload；Daemon 自动创建或复用消息处理智能体。APP 不反复发送 create runtime command。
- 非目标：不实现独立 APP ↔ Daemon pairing channel、本地 RPC、局域网通道或第二条传输链路；不实现 bootstrap 普通消息 body 加密；不在 APP 侧追加修改 DID Document；不支持 E2EE 明文/摘要/metadata 转发给 Agent。
- 完成标准：APP 能构造 `awiki.daemon.bootstrap.v1`，包含 `user_subkey_package`、APP capabilities 和 `desired_message_agent`；system/control payload 不显示成普通聊天；错误和重试幂等。

## 3. 设计方法

- 设计边界：APP 是用户授权、展示端和 daemon subkey private material 初始生成方；user-service 只登记 public verification method；Agent 创建由 Daemon ensure。APP 和 Daemon 之间只有 message-service 普通消息发送这一条通道。
- 核心决策：bootstrap 是一次性 desired state；APP 记录 `bootstrap_id` / `idempotency_key`，重试同一 desired state，不循环创建 runtime。
- 契约 / API / 数据流：APP 创建 DID 时生成 `DaemonSubkeyPrivatePackage`，向 user-service 提交 `DaemonDelegatedKeyPublicRegistration`；APP 在本地 session/identity service 保存 private package 或 key ref；通过现有 `sendPayload` / message-service 普通消息发送向 `daemon_agent_did` 发送 `awiki.daemon.bootstrap.v1`；MVP body 是明文 JSON，后续可改为加密文本或加密 JSON envelope；接收 Daemon 返回的 bootstrap/message agent status；后续 `message.sync`、`app.action`、`app.action.result` 都继续走同一普通消息发送路径。
- 兼容性：现有 AgentControlService 的 daemon install、status、create runtime 功能保留；新增 flow 只用于 APP message handler agent。
- 迁移策略：已有用户如果没有 daemon key，提示或触发 Step 01 的兼容补齐/rotate 流程。
- 风险控制：private key package 不进入 UI 文本、日志、普通 state dump；system/control payload 过滤规则优先于聊天渲染。

## 4. 实现方法

1. 阅读 `awiki-me/lib/src/application/agent/agent_control_service.dart`、`agent_control_projection.dart`、`data/im_core/*`、`domain/entities/agent/*`、`presentation/agents/*`。
2. 扩展 identity adapter 或 session model，使 APP 在 DID 创建时本地生成并保存 daemon subkey private package，同时只把 public registration 交给 user-service；如果当前 session 未保存，需要实现安全的本地传递/短期持有策略。
3. 定义 Dart model：`DaemonBootstrapEnvelope`、`UserSubkeyPackage`、`DesiredMessageAgent`、`AppCapabilityPolicy`；字段与 Step 03 schema 对齐。
4. 在 `AgentControlService` 或新增 daemon bootstrap service 中实现 `ensureMessageAgentBootstrap`：生成稳定 `idempotency_key`，通过普通消息发送 `sendPayload` 发送 payload，处理 success/conflict/retry。
5. UI 只展示 bootstrap status、message agent status、授权范围摘要和错误恢复入口；不展示 key material。
6. 更新 `ChatMessage` / payload filter：`awiki.daemon.bootstrap.v1`、`awiki.message.sync.v1`、`awiki.app.action.v1`、`awiki.app.action.result.v1` 不显示为普通聊天内容。
7. 增加 tests：bootstrap payload 构造、idempotency、control payload hidden、状态 reducer、缺少 daemon key 的错误提示。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-me/lib/src/application/agent/agent_control_service.dart` | 增加 bootstrap/session service | 复用现有 `sendPayload` 普通消息发送 |
| `awiki-me/lib/src/application/ports/identity_core_port.dart` | 暴露 APP 本地 daemon key package 或 key ref | 需避免泄露 |
| `awiki-me/lib/src/data/im_core/awiki_im_core_identity_adapter.dart` | DID 创建时生成 daemon subkey private package，并提交 public registration | 依赖 Step 01/02 binding |
| `awiki-me/lib/src/domain/entities/agent/agent_control_payloads.dart` | 新增 bootstrap/status/action schema model | 与 Daemon schema 对齐 |
| `awiki-me/lib/src/domain/entities/session_identity.dart` | 记录 daemon key ref / bootstrap state | private key 存储需谨慎 |
| `awiki-me/lib/src/domain/entities/chat_message.dart` | 过滤 system/control payload | 防止普通聊天污染 |
| `awiki-me/lib/src/presentation/agents/*` | 展示 bootstrap/message agent 状态 | 不做营销式新页面 |
| `awiki-me/test/*` | 新增 service/provider/model tests | 以现有测试结构为准 |

## 6. 依赖

- 前置步骤：Step 01 public registration/private package schema；Step 03 Daemon bootstrap schema；Step 04 message agent status 契约。
- 外部文档或决策：`agent_im_core_design.md` 第 3.1、5.2、5.7；`agent_delegated_identity_message_proof_plan.md` 第 5.3、5.5。
- 环境前提：Flutter SDK 可运行 analyze/test；如果 im-core binding 未完成，可用接口 mock，但最终需补真实 adapter。

## 7. 验收标准

- [ ] APP 不在 bootstrap 时修改 DID Document；APP 创建 DID 时本地生成 `#daemon-key-1` private package，并向 user-service 提交 public registration。
- [ ] APP 能通过 message-service 普通消息发送构造并发送一次性 `awiki.daemon.bootstrap.v1`，包含 `desired_message_agent`。
- [ ] `bootstrap_id` / `idempotency_key` 稳定，重试不变成重复 create runtime。
- [ ] APP 能展示 bootstrap 和 message agent ready/active/failed 状态。
- [ ] system/control payload 不显示成普通聊天；bootstrap private package 不进入普通聊天 UI、Hermes prompt、错误文案或日志。
- [ ] private key package 不进入 UI、日志、普通错误消息。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Analyze | `cd awiki-me && flutter analyze` | 无新增分析错误。 |
| Unit/widget | `cd awiki-me && flutter test` | bootstrap service、payload filter、provider/UI tests 通过。 |
| Manual check | 运行 APP bootstrap flow 或 mock service | 只通过普通消息发送 bootstrap desired state，不发送重复 create runtime command。 |
| Security | 搜索 key material 字段是否进入 UI/log | 未发现泄露路径。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：bootstrap 幂等、是否只走普通消息发送、私钥不进 UI/log、payload filter、状态展示、错误恢复、与 Daemon schema 对齐、旧 Agent 管理功能不回归。
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
- Commit 范围：只包含 awiki-me bootstrap/service/UI/filter 和直接测试。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`awiki-me: bootstrap app message agent`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| identity adapter 尚不能拿到 daemon key package | 待填写 | 使用 Step 01 mock/fixture；等待 binding 更新 | 当前步骤 / Step 09 | 不合并真实 flow 前必须补 adapter 验证 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 06 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：system/control payload 被当普通消息展示，泄露 bootstrap 或 action 内容；MVP 明文 private package 经普通消息发送路由/存储。
- 回滚 / 回退：关闭 bootstrap UI 入口，保留 daemon install/status；过滤所有未知 `awiki.*` system payload。
- 后续文档：更新 awiki-me Agent 管理文档或用户流程说明，记录 MVP 明文 bootstrap 安全债和后续同一普通消息发送路径上的 body 加密升级。
