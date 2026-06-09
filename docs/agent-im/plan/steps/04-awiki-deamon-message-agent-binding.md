# Step 04：awiki-deamon message agent binding

主 Plan：[../plan.md](../plan.md)  
Step index：04  
状态：review

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | review |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T13:28:24Z |
| Completed | - |
| Commit | - |
| Review evidence | 2026-06-09 Review：检查 active binding 唯一性、bootstrap replay、runtime token scope、secret/token 泄漏、Hermes 私钥边界、Step 05 非目标。发现并修复：重复 bootstrap 去掉 `runtime_registration_token` 时 payload hash 冲突；专用 Message Agent 沿用 Hermes 默认 runtime token recipient scope 过宽。 |
| Verification evidence | `cargo test -p awiki-deamon --locked -j1`：78 lib tests passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。定向：`cargo test -p awiki-deamon --locked -j1 app_bridge -- --nocapture`：11 passed；`cargo test -p awiki-deamon --locked -j1 daemon_bootstrap_replay_reuses_message_agent_without_runtime_token -- --nocapture`：1 passed；`cargo test -p awiki-deamon --locked -j1 app_message_agent_runtime_token_scope_is_limited_to_bound_user -- --nocapture`：1 passed；`git diff --check`：通过。 |
| Next action | 提交 Step 04 聚焦 commit，回填 commit hash 后标记 done |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：Daemon 在收到有效 bootstrap 后，幂等创建或复用专门处理 APP 普通消息的 Hermes Message Agent，并持久化 `app_message_agent_binding`。
- 用户 / 系统可见行为：APP 不需要反复发送 create runtime command；重复 bootstrap、APP 重启或 Daemon 重启都不会创建第二个 active message handler agent。
- 非目标：不让 Runtime/Hermes 直接持有用户子私钥；不实现普通消息 inbox poller；不实现完整 APP action allowlist。
- 完成标准：`ensure_app_message_agent` 可按 `ensure_once_key` 创建/复用 `role=app_message_handler`；binding 绑定 user DID、verification method、app_instance、pairing、runtime_agent_did、capability policy 和状态；重启恢复测试通过。

## 3. 设计方法

- 设计边界：Message Agent 是 APP 消息处理链路的一部分，与用户手动创建的通用 Runtime Agent 分开。
- 核心决策：以 `role=app_message_handler` 和 `app_message_agent_binding` 标识专用 Agent；Hermes 只拿 runtime token，不拿 delegated subkey private key。
- 契约 / API / 数据流：Step 03 产生 `paired_key_received` 后，Daemon 执行 `ensure_app_message_agent(desired_message_agent)`；创建或复用 Runtime Agent；写入 binding；状态进入 `message_agent_ready` 或 `message_agent_active`。现有 Runtime Agent 创建仍需要 user-service runtime registration token；MVP 将其作为 `desired_message_agent.runtime_registration_token` 可选字段放在同一条 `awiki.daemon.bootstrap.v1` 普通消息 JSON 中。已有 active binding 时不需要该 token；首次创建时必须提供，且不得持久化到 binding / audit detail。
- 兼容性：不破坏现有 Hermes runtime 创建流程；新增路径优先复用 runtime host 现有模块。
- 迁移策略：已有 Daemon state 无 binding 时，bootstrap 后创建；已有 binding 且 active 时复用。
- 风险控制：唯一约束应覆盖 user DID + app_instance_id + role + active status，防止重复 active Agent。

## 4. 实现方法

1. 阅读 `awiki-cli-rs2/crates/awiki-deamon/src/runtime/host.rs`、`runtime/mod.rs`、`plugins/hermes/*`、`agent/mod.rs` 和现有创建 Hermes runtime 的命令。
2. 新增 `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/message_agent.rs`，实现 `ensure_app_message_agent` 服务。
3. 定义 `AppMessageAgentBinding` 状态模型，字段至少包含：`binding_id`、`user_did`、`inbox_auth_verification_method`、`app_instance_id`、`pairing_id`、`runtime_agent_did`、`role`、`capability_policy_ref`、`status`、`created_at`、`updated_at`、`revoked_at`。
4. 接入 Step 03 bootstrap flow：收到 `desired_message_agent` 后执行 ensure；失败时保留可重试状态，不创建重复 Agent。
5. 复用现有 Hermes runtime 创建和 registration token 流程；首次创建时从 `desired_message_agent.runtime_registration_token` 读取 token，已存在 active binding 时不再消耗 token；如果缺少 runtime token scope，新增只允许 message handler 所需能力的 scope。
6. 实现 Daemon 重启恢复：读取 active binding，确认 runtime agent 存在；不存在时按策略恢复或标记 repair_needed。
7. 增加测试：首次 bootstrap 创建 Agent、重复 bootstrap 复用、冲突 desired state 拒绝或更新策略、重启恢复、Hermes 不持有 user subkey。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/message_agent.rs` | 新增 ensure 和 binding 服务 | 如 app_bridge 不存在则创建 |
| `awiki-cli-rs2/crates/awiki-deamon/src/runtime/host.rs` | 接入专用 message handler runtime 创建/复用 | 复用现有 runtime host |
| `awiki-cli-rs2/crates/awiki-deamon/src/plugins/hermes/mod.rs` | 标识 Hermes Message Agent profile | 不改变 Hermes 私钥边界 |
| `awiki-cli-rs2/crates/awiki-deamon/src/security/runtime_token.rs` | 增加 message handler token scope | Runtime 只能调用受限能力 |
| `awiki-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | 新增 `app_message_agent_binding` 状态 | 需要唯一性和恢复 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 启动时恢复 binding | Step 05 将继续接 poller |
| `awiki-cli-rs2/crates/awiki-deamon/tests/*` | 新增 ensure/binding/restart 测试 | 或模块内测试 |

## 6. 依赖

- 前置步骤：Step 03 完成 bootstrap state；Step 01 key registry 契约用于字段命名；Step 02 optional API 为后续 runtime 能力服务。
- 外部文档或决策：`agent_im_core_design.md` 第 3.1.3、5.1、5.2、5.3；`agent_delegated_identity_message_proof_plan.md` 第 5.5、10.2、11.1。
- 环境前提：现有 Hermes runtime 创建测试可运行。

## 7. 验收标准

- [x] `ensure_app_message_agent` 首次调用能创建 `role=app_message_handler` 的 Runtime Agent。
- [x] 重复 `bootstrap_id` / `idempotency_key` 不创建第二个 active Agent。
- [x] `app_message_agent_binding` 持久化 user DID、verification method、app_instance、pairing、runtime_agent_did、capability policy。
- [x] Daemon 重启后恢复 binding，不重新创建 Agent。
- [x] Hermes / Runtime 不直接持有 user delegated subkey private key。
- [x] runtime token scope 只允许 message handler 所需能力。
- [x] `desired_message_agent.runtime_registration_token` 仅首次创建使用，不进入 binding、audit detail、Hermes prompt 或 runtime temp。
- [x] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | ensure/binding/restart/runtime token 测试通过。 |
| Idempotency | 重放同一 bootstrap fixture | active binding 数量仍为 1。 |
| Security | 检查 Hermes runtime config 和 prompt 输入 | 不包含 private key material。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：active binding 唯一性、runtime 生命周期、secret 边界、runtime token scope、失败重试、重启恢复。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 2 项 | 1. 重复 bootstrap 已有 active binding 时允许省略 `runtime_registration_token`，但 Step 03 payload hash 会把该 token 纳入冲突判断；2. 专用 Message Agent 运行任务时若沿用 Hermes 默认 recipient scope，会允许 active handle lookup / any group，超过绑定用户 DID 范围。 |
| 已修复问题 | 已修复 | bootstrap payload hash 只忽略一次性 runtime registration token 字段，不放松其他 payload 冲突；runtime host 在发现 active `app_message_agent_binding` 后使用绑定 `user_did` 和 `default_plain` 生成 runtime token scope。 |
| 剩余风险 | 已记录 | Step 04 不实现 user delegated inbox poller；如果 Runtime Agent profile 缺失，重复 bootstrap 会失败并保留可重试状态，Step 05/后续 repair 流程再扩展。MVP 仍按 Step 03 决策保存 delegated subkey 明文。 |
| 新增或缺失测试 | 已新增 | 覆盖首次 bootstrap 创建、重复 bootstrap/reopen 后复用且不消耗 token、active binding 持久化恢复、runtime token scope 限制、registration token 不进入 binding/audit/Hermes prompt/runtime temp 相关存储字段。 |
| 已更新或缺失文档 | 已更新 | 主 Plan 与本 Step 已记录 `desired_message_agent.runtime_registration_token` 契约、Review 发现和验证证据。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 message agent ensure、binding、runtime token 和直接测试。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`awiki-deamon: ensure app message agent`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 现有 runtime 创建流程只能命令式调用 | 待填写 | 提取 idempotent service 层，保留命令作为 wrapper | 当前步骤 / Step 06 | 先重构最小服务边界并记录 Review |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 04 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | 明确 `desired_message_agent.runtime_registration_token` 可选字段 | Runtime Agent 创建必须经 user-service runtime registration token；仍通过普通消息 bootstrap JSON 传递，不新增第二通道 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：专用 Message Agent 与用户手动 runtime 混用，导致权限扩大。
- 回滚 / 回退：停用 binding，撤销 runtime token，保留手动 runtime 不受影响。
- 后续文档：实现后更新 Daemon runtime host 文档，明确 `role=app_message_handler` 生命周期。
