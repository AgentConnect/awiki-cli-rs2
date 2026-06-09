# Step 03：awiki-deamon bootstrap 与 user delegated identity state

主 Plan：[../plan.md](../plan.md)  
Step index：03  
状态：review

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | review |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T12:46:56Z |
| Completed | - |
| Commit | 待回填：`awiki-deamon: add app bootstrap state` |
| Review evidence | 2026-06-09 Review：检查 secret handling、control payload redaction、幂等冲突、状态恢复、schema dispatch、E2EE / main key 禁止、Step 04 边界。发现并修复：control payload / extra Debug redaction 不够硬；bootstrap replay 查重在 transaction 外存在并发写入窗口；schema version 升级后集成测试仍期望 15。剩余风险：MVP 仍按现有 daemon secret 存储方式保存 delegated subkey，明文 bootstrap body 加密和 secure key store 留到后续版本。 |
| Verification evidence | `cargo test -p awiki-deamon --locked -j1`：72 lib tests passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。补充 targeted：`cargo test -p awiki-deamon --locked -j1 app_bridge -- --nocapture`：8 passed；`cargo test -p awiki-deamon --locked -j1 delegated_identity -- --nocapture`：2 passed；`git diff --check`：通过。 |
| Next action | 创建 Step 03 聚焦 commit，并在主 Plan / Step 台账回填 commit hash |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`awiki-deamon` 能从 message-service 普通消息发送接收 APP 发送的一次性 `awiki.daemon.bootstrap.v1` system/control payload，MVP body 是明文 JSON，保存 user delegated identity，按 `bootstrap_id` / `idempotency_key` 幂等处理。
- 用户 / 系统可见行为：APP 只需 bootstrap 一次；Daemon 重启后能恢复 bootstrap state 和 delegated subkey profile；重复 bootstrap 不产生重复身份状态。
- 非目标：不实现独立 APP ↔ Daemon pairing channel、本地 RPC、局域网通道或第二条传输链路；不实现 bootstrap 普通消息 body 加密；不实现完整本地 secret 生命周期管理、OS keychain/KMS、密钥覆盖/撤销自动清理；不把用户主私钥或 E2EE private state 存入 Daemon；不创建 Message Agent，本步骤只完成 bootstrap state，为 Step 04 提供入口。
- 完成标准：Daemon 有明确 bootstrap schema、解析校验、secret 存储、幂等表和测试；日志/prompt/runtime temp/audit detail 不泄露 key material。

## 3. 设计方法

- 设计边界：APP 和 Daemon 之间只有一条通道：message-service 承载的普通消息发送。bootstrap 是这条通道上的 system/control payload，是声明式 desired state，不是命令式 create runtime；它只传递已存在且已登记的 `user_did#daemon-key-1` key package 和 APP 期望的 message agent 配置。
- 核心决策：MVP 明文传子私钥，且明确通过 message-service 普通消息明文 JSON payload 路由给 Daemon；message-service 只负责普通消息 payload 的路由/存储，不理解 private package 语义。该 payload 必须被 Daemon/APP schema router 识别为 system/control，不得渲染为普通聊天、不得进入 Hermes prompt。后续安全升级仍使用同一普通消息发送路径，只把消息 body 从明文 JSON 改为加密文本或加密 JSON envelope。
- 契约 / API / 数据流：APP 使用现有消息发送能力向 `daemon_agent_did` 发送普通消息 payload。MVP payload body 是明文 JSON，包含 `schema`、`bootstrap_id`、`idempotency_key`、`controller_did`、`app_instance_id`、`user_subkey_package`、`capability_policy`、`desired_message_agent`。其中 `user_subkey_package` 使用 Step 01 固定的 `DaemonSubkeyPrivatePackage`。Daemon 从普通消息 inbox/control dispatch 中识别该 schema，校验后写入 `user_delegated_identity` 和 `bootstrap_replay` 状态。
- 兼容性：未知 schema 或旧 `awiki.agent.command.v1` 不应显示为普通聊天；本步骤处理 Daemon 侧 schema dispatch、control payload 过滤和 unsupported 状态。
- 迁移策略：在 Daemon state schema 中增加新表或新记录类型；旧 state 无该记录时按 unpaired 处理。
- 风险控制：private key field 使用 redaction wrapper；所有 debug/audit 输出只记录 key ref、verification method hash 或尾号，不记录明文 key。MVP 对本地 secret 生命周期只做最小保护：复用现有 secret 存储方式、禁止日志/prompt/runtime temp/audit detail 泄露；自动 rotate、覆盖清理、OS keychain/KMS、文件权限 hardening 记录为后续版本。

## 4. 实现方法

1. 阅读 `awiki-cli-rs2/crates/awiki-deamon/src/commands/mod.rs`、`foreground.rs`、`inbox/mod.rs`、`runtime_inbox.rs`、`im_core_adapter.rs` 和 `state/mod.rs`，确认现有 Agent command / 普通消息 JSON payload dispatch 入口和 state 存储方式。
2. 新增或扩展 `app_bridge` 模块；如果当前没有目录，可创建 `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs`、`message_control.rs`、`secret_store.rs`。
3. 定义 `DaemonBootstrapEnvelope`、`UserSubkeyPackage`、`DesiredMessageAgent`、`CapabilityPolicy` DTO；schema 固定为 `awiki.daemon.bootstrap.v1`。
4. 实现校验：禁止 user main key；`verification_method` 必须属于 `user_did`；MVP key fragment 必须是 `#daemon-key-1`；`allowed_usage_hint` 不得包含 E2EE private state。
5. 实现 secret store：MVP 可复用现有 daemon secret 存储方式，但必须集中封装 redaction 和日志保护，并在文档/注释中记录后续 secure key store。不得新增 APP ↔ Daemon 本地 RPC、局域网或独立传输通道入口；bootstrap 入口只来自普通消息 payload schema dispatch。
6. 实现幂等：`bootstrap_id` 与 `idempotency_key` 重复时返回已处理状态；payload 冲突时返回 deterministic conflict。
7. 增加状态机：`unpaired -> paired_key_received -> message_agent_ensuring -> message_agent_ready -> message_agent_active`，本步骤至少能进入 `paired_key_received`。
8. 增加测试：ordinary JSON message dispatch 到 bootstrap handler、valid bootstrap、重复 bootstrap、冲突 bootstrap、错误 key owner、main key 拒绝、E2EE private state 拒绝、日志 redaction、control payload 不进入普通聊天/Prompt。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/commands/mod.rs` | 接入 bootstrap system/control schema dispatch | 不新增 APP ↔ Daemon 本地 RPC |
| `awiki-cli-rs2/crates/awiki-deamon/src/inbox/mod.rs` | 从普通消息 payload 分发 `awiki.daemon.bootstrap.v1` | 唯一 APP ↔ Daemon 通道 |
| `awiki-cli-rs2/crates/awiki-deamon/src/runtime_inbox.rs` | 复用或扩展普通消息 payload 读取 | 注意与 Runtime Agent own inbox 分离 |
| `awiki-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | 新增 state schema/table | user delegated identity、bootstrap replay |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs` | 新增 bootstrap 逻辑 | 如目录不存在则新增 |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/message_control.rs` | 普通消息 control payload dispatch | 统一处理 system/control schema |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/secret_store.rs` | 封装 key package 存储 | MVP 复用现有方式，后续强化 |
| `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | 准备 delegated identity 给 Step 05 使用 | 不在本步骤拉取消息 |
| `awiki-cli-rs2/crates/awiki-deamon/tests/*` | 新增 bootstrap/state 测试 | 若无 tests 目录，可用模块内测试 |

## 6. 依赖

- 前置步骤：Step 01 的 key package 契约；Step 02 的 optional 参数契约可作为后续使用。
- 外部文档或决策：`agent_im_core_design.md` 第 3.1.2、3.1.3、5.1；`agent_delegated_identity_message_proof_plan.md` 第 5.3、5.5。
- 环境前提：能运行 `cargo test -p awiki-deamon --locked`。

## 7. 验收标准

- [x] Daemon 能从 message-service 普通消息 payload 解析 `awiki.daemon.bootstrap.v1` 并保存 user delegated identity。
- [x] `user_subkey_package` 与 Step 01 `DaemonSubkeyPrivatePackage` schema 对齐。
- [x] Daemon 不接受用户主私钥或 E2EE private state。
- [x] Daemon 不新增本地 RPC、局域网或独立传输通道 bootstrap 入口；APP ↔ Daemon bootstrap 只走普通消息发送。
- [x] `awiki.daemon.bootstrap.v1` 被识别为 system/control payload，不显示为普通聊天，不进入 Hermes prompt。
- [x] `bootstrap_id` / `idempotency_key` 重放幂等；冲突 payload 明确拒绝。
- [x] secret store 不把 private key 写入日志、prompt、runtime temp、普通 audit detail。
- [x] Daemon 重启后能恢复 `paired_key_received` 状态。
- [x] 本步骤不创建重复 Runtime Agent；Message Agent 创建留给 Step 04。
- [x] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | bootstrap/state/secret/idempotency 测试通过。 |
| Redaction | 搜索测试输出和 debug 格式，确认 private key 不出现 | 只输出 key ref 或 redacted。 |
| Schema | 使用示例 JSON fixture 解析，并模拟普通消息 payload 分发 | 与设计文档 `awiki.daemon.bootstrap.v1` 对齐，能从 ordinary JSON message 进入 handler。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：secret handling、幂等冲突处理、状态恢复、schema 兼容、错误日志、E2EE 禁止、与 Step 04 的 ensure 入口。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 3 项 | control payload / extra Debug redaction 不够硬；bootstrap replay 查重在 transaction 外存在并发写入窗口；schema version 升级后 `hermes_profile` / `state_bootstrap` 集成测试仍期望 15。 |
| 已修复问题 | 3 项 | Debug 改为 redacted control payload，并拒绝 extra 中通用 `private_key` 字段；`store_bootstrap_state` 改用 immediate transaction 内查重和写入；测试预期更新为 schema version 16。 |
| 剩余风险 | 已记录 | MVP 仍按现有 daemon identity private key 存储方式保存 delegated subkey；普通消息明文 bootstrap body 加密、OS keychain/KMS、密钥覆盖/撤销自动清理留到后续版本。 |
| 新增或缺失测试 | 已新增 | app_bridge 单测覆盖 schema、owner、fragment、E2EE scope、main key、extra private key redaction；state 单测覆盖 roundtrip、replay idempotency、conflict、restart restore；foreground 单测覆盖 system/control dispatch 和 ignored audit 不污染。 |
| 已更新或缺失文档 | 已更新 | 本 Step 和主 Plan 台账记录 Review、验证和 commit 范围；两篇设计文档边界清理已在独立 docs commit `745a3d9` 完成。 |

### Commit 执行记录

Commit 前状态：

```text
## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 7]
 M crates/awiki-deamon/src/foreground.rs
 M crates/awiki-deamon/src/lib.rs
 M crates/awiki-deamon/src/outbox/mod.rs
 M crates/awiki-deamon/src/runtime_inbox.rs
 M crates/awiki-deamon/src/state/mod.rs
 M crates/awiki-deamon/tests/hermes_profile.rs
 M crates/awiki-deamon/tests/state_bootstrap.rs
 M docs/agent-im/plan/plan.md
 M docs/agent-im/plan/steps/03-awiki-deamon-bootstrap-state.md
?? crates/awiki-deamon/src/app_bridge/
```

纳入文件：

```text
crates/awiki-deamon/src/app_bridge/mod.rs
crates/awiki-deamon/src/app_bridge/bootstrap.rs
crates/awiki-deamon/src/app_bridge/message_control.rs
crates/awiki-deamon/src/app_bridge/secret_store.rs
crates/awiki-deamon/src/lib.rs
crates/awiki-deamon/src/foreground.rs
crates/awiki-deamon/src/outbox/mod.rs
crates/awiki-deamon/src/runtime_inbox.rs
crates/awiki-deamon/src/state/mod.rs
crates/awiki-deamon/tests/hermes_profile.rs
crates/awiki-deamon/tests/state_bootstrap.rs
docs/agent-im/plan/plan.md
docs/agent-im/plan/steps/03-awiki-deamon-bootstrap-state.md
```

Commit 后证据：待回填 commit hash 和 commit 后 `git status --short --branch`。

遗留未提交变更：预期只剩 commit hash 回填台账；若出现其它变更，提交前必须重新检查原因。

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 awiki-deamon bootstrap/state/secret store 与直接测试文档。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`awiki-deamon: add app bootstrap state`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 现有 state 存储不支持 schema 迁移 | 待填写 | 评估新增 namespaced record 或 migration | 当前步骤 / Step 04-05 | 先更新 Plan，避免临时内存状态 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 03 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：MVP 明文 bootstrap private package 通过普通消息发送传输，是高风险路径；message-service 可能按普通消息 payload 路由/存储该 payload。
- 回滚 / 回退：删除 user delegated identity state，撤销 user-service daemon key，停用 APP bootstrap；后续在同一普通消息发送路径上把 private package 升级为加密文本或加密 JSON envelope。
- 后续文档：实现后更新 Daemon local dev 或 Agent IM 文档，记录 bootstrap schema 和 secret handling 约束。
