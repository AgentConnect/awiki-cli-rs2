# Step 05：awiki-deamon delegated inbox sync

主 Plan：[../plan.md](../plan.md)  
Step index：05  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T14:16:19Z |
| Completed | 2026-06-09T15:33:31Z |
| Commit | `59ec9b2 awiki-deamon: sync user delegated inbox` |
| Review evidence | 2026-06-09 Review：检查 cursor/processed message、dispatch 失败、E2EE opaque、system/control payload、prompt 包装、message_event retention、runtime status/final outbox、delegated key material 和 DID shadow。发现并修复：失败 dispatch 后 `processed_message` 必须进入 retryable 且 cursor 不前移；message_event 必须在 dispatch 成功后写入；生产 dispatcher 不能只插入 runtime_task，必须实际运行 Runtime Host；failed prior runtime run 需要 retry run id；`private_key_multibase` 需要规范化为 im-core 可解析 PEM；delegated DID shadow 必须随当前 identity 刷新；runtime status/final 需要进入 `message_sync_outbox` 且不保存 final 明文。 |
| Verification evidence | `cargo check -p awiki-deamon --locked`：通过；`cargo test -p awiki-deamon --lib --locked -j1 user_delegated -- --nocapture`：11 passed；`cargo test -p awiki-deamon --lib --locked -j1 delegated_inbox_sync_state -- --nocapture`：1 passed；`cargo test -p awiki-deamon --locked -j1`：lib 88 passed；main 0 passed；integration tests passed：21 + 22 + 5 + 19 passed / 3 ignored + 15 + 3 + 21 + 2；doc tests 0 passed；0 failed。 |
| Next action | 启动 Step 06：awiki-me bootstrap UI 与 service |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：Daemon 使用 user delegated identity 拉取普通非 E2EE inbox/history，持久化 cursor 和 processed message，把消息投递给 active `app_personal_agent_binding`。
- 用户 / 系统可见行为：用户普通消息到达后，即使 APP 没打开，Daemon 也能同步给专用 Hermes Personal Agent 处理；E2EE opaque notification 不进入 Agent pipeline。
- 非目标：不拉取或处理 E2EE 明文、摘要、metadata projection、private state；不实现 APP action 全部能力；不修改 message-service 服务器逻辑，本步骤只消费 Step 07 契约。
- 完成标准：`process_user_delegated_inbox_once` 可使用 `InboxHistoryOptions` 拉取普通消息；durable `inbox_cursor` 与 `processed_message` 防重复/丢失；E2EE opaque 消息被丢弃或标记 `ignored_e2ee_opaque`。

## 3. 设计方法

- 设计边界：user delegated inbox sync 与现有 Runtime Agent own inbox 分开，避免把用户普通消息和 agent command inbox 混为一个 cursor。
- 核心决策：durable cursor + processed message 是上线前置；不能只用内存 HashSet。
- 契约 / API / 数据流：foreground tick 调用 `process_user_delegated_inbox_once`；Daemon 从 `user_delegated_identity` 与 `app_personal_agent_binding` 取 `inbox_owner_did` 和 `inbox_auth_verification_method`；通过 `im-core` 拉取普通消息；写入最小 `message_event` 投影；把消息以 untrusted content envelope 投递给 Hermes；通过 `message.sync` / `app.action.result` 同步给 APP。
- 兼容性：现有 agent inbox polling 不应被破坏；普通消息 default_plain 和系统 payload dispatch 分开。
- 迁移策略：新增 state table/record：`inbox_cursor`、`processed_message`、`message_event`、`message_sync_outbox`。
- 风险控制：处理消息前先判断类型和 E2EE 标记；E2EE opaque 不写入可处理明文事件，不进入 prompt。普通非 E2EE 消息进入 Agent 前也必须做最小投影和 prompt 注入隔离，不得把用户正文拼接成系统指令。

### 3.1 message_event 最小投影与 prompt 边界

MVP 的 `message_event` 不应默认保存完整 Hermes prompt 或任意扩展上下文。建议最小字段：

```text
message_event(
  event_id,
  owner_did,
  conversation_id,
  message_id,
  message_kind,
  sender_did,
  received_at,
  plain_text_ref_or_excerpt,
  content_hash,
  schema,
  processing_status,
  retention_class
)
```

要求：

- `plain_text_ref_or_excerpt` 只保存处理所需的普通非 E2EE 内容引用或短摘录；是否保存全文必须由实现显式决定并记录 retention class。
- 不保存 E2EE plaintext、metadata projection、private state 或 Hermes 完整 prompt。
- 投递给 Hermes 的内容必须包装为 untrusted content，例如包含 `content_role=user_message_untrusted`、`source_message_id`、`allowed_actions`，避免用户消息正文被当作系统指令。
- system/control schema 与用户正文必须分离 dispatch，不能直接拼接到同一 prompt 指令段。
- retention 默认跟随本地消息缓存策略；如果没有现成策略，MVP 默认只保留处理状态、hash、短摘录或引用，后续再补完整保留策略。

## 4. 实现方法

1. 阅读 `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、`runtime_inbox.rs`、`inbox/mod.rs`、`outbox/mod.rs` 和 `im_core_adapter.rs`。
2. 定义 user delegated inbox state：`inbox_cursor(owner_did, inbox_scope, cursor, updated_at)` 和 `processed_message(owner_did, message_id, schema, processed_at, status)`。
3. 实现 `process_user_delegated_inbox_once(binding_id)`：构造 `InboxHistoryOptions`，调用 Step 02 API 拉取普通非 E2EE消息。
4. 实现 message dispatch：default plain 消息进入 Hermes Personal Agent；`awiki.message.sync.v1`、`awiki.app.action.v1` 等 system/control payload 进入对应 handler；E2EE opaque notification 直接 ignore/drop。
5. 实现 ordinary message projection：构造 untrusted content envelope，写入最小 `message_event`，避免保存完整 prompt 或把用户正文当系统指令。
6. 实现 crash recovery：先记录处理状态或使用事务，确保重复拉取不会重复投递；失败进入 retry/outbox。
7. 增加状态和 final/action 回传路径：Agent processing status、final summary、draft/action request 通过 `message_sync_outbox` 或现有 outbox 同步给 APP。
8. 增加测试：cursor 持久化、processed message 去重、历史重放、E2EE opaque ignore、绑定 Agent 投递、untrusted content envelope、message_event 最小投影、Daemon 重启恢复。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 接入 user delegated inbox poller | 与现有 runtime inbox poller 并存 |
| `awiki-cli-rs2/crates/awiki-deamon/src/runtime_inbox.rs` | 复用或分离处理函数 | 注意不要混用 cursor |
| `awiki-cli-rs2/crates/awiki-deamon/src/inbox/mod.rs` | 新增 user delegated inbox 模块 | 处理普通消息和 control payload |
| `awiki-cli-rs2/crates/awiki-deamon/src/outbox/mod.rs` | message sync/action result outbox | 视现有 outbox 结构调整 |
| `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | 使用 `InboxHistoryOptions` 拉取 | 依赖 Step 02 |
| `awiki-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | 新增 cursor、processed message、message_event | durable 状态前置 |
| `awiki-cli-rs2/crates/awiki-deamon/tests/*` | 新增 sync/cursor/idempotency 测试 | 或模块内测试 |

## 6. 依赖

- 前置步骤：Step 02 optional inbox API；Step 04 active personal agent binding；Step 07 message-service delegated inbox 契约。可先用 mock adapter 开发，但最终验收依赖 Step 07。
- 外部文档或决策：`agent_im_core_design.md` 第 3.2、5.1、5.3、5.4；`agent_delegated_identity_message_proof_plan.md` 第 3.4、5.8、8。
- 环境前提：能运行 awiki-deamon unit tests；如 message-service 尚未实现，必须用 mock 明确隔离。

## 7. 验收标准

- [x] `process_user_delegated_inbox_once` 使用 `inbox_owner_did` 和 `inbox_auth_verification_method` 拉取普通消息。
- [x] `inbox_cursor` 和 `processed_message` 持久化，并能覆盖重启、重试、历史重放。
- [x] 普通非 E2EE 消息投递给 active `app_personal_agent_binding`。
- [x] 普通非 E2EE 消息投递给 Hermes 前被包装为 untrusted content envelope，不把用户正文当系统指令。
- [x] `message_event` 使用最小投影，不保存 E2EE 内容或 Hermes 完整 prompt；全文保留策略如需启用必须记录 retention class。
- [x] E2EE opaque notification 不解密、不转发给 Hermes、不写入可处理明文事件。
- [x] Agent status/final 能通过 `message_sync_outbox` 进入 APP 可消费路径；action/result schema 和 APP 消费仍由 Step 08 完成。
- [x] 现有 Agent own inbox polling 不回归。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | cursor、processed message、E2EE ignore、dispatch、untrusted envelope、message_event 最小投影测试通过。 |
| Mock integration | 使用 mock im-core/message-service 响应 | delegated inbox 只处理普通消息，E2EE opaque ignored。 |
| Regression | 运行现有 runtime inbox 相关测试 | agent command/status 旧链路不受影响。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：重复处理、漏消息、事务边界、cursor 语义、E2EE boundary、prompt 注入入口、message_event retention、与 Step 08 schema dispatch 一致性。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 7 项 | dispatch 失败若保持 `processing` 会导致后续跳过；message_event 在 dispatch 前写入会伪造已处理；生产 dispatcher 只入库不运行 Runtime Host；失败 runtime run 会被相同 run id 复用；bootstrap `private_key_multibase` 不能直接作为 im-core PEM key ref；DID shadow 文件不刷新会在 key 替换后过期；runtime status/final 只有 audit，没有进入 APP 可消费 outbox。 |
| 已修复问题 | 已修复 | 增加 `failed_retryable` 与 retry-aware cursor 语义；成功 dispatch 后再写 `message_event`；生产 dispatcher 调用 `run_existing_runtime_task_with_config`；失败 run 使用 `_retry_N` run id；新增 delegated private key PEM normalization 和 0600 key 文件权限；DID shadow 每次按当前 identity 重写；status/final 入 `message_sync_outbox`，final 只保存 hash/has_text。 |
| 剩余风险 | 已记录 | Step 07 message-service delegated send/inbox/history server policy 尚未实现，本步骤用 im-core adapter 和 mock contract 验证 daemon 本地逻辑；Step 08 才实现 APP action/result schema 与 APP 侧消费/展示；MVP delegated private key 仍沿用当前 daemon 本地 secret 存储方式，secure key store 留到后续。 |
| 新增或缺失测试 | 已补充 | 新增 delegated inbox plain dispatch、cursor/replay、retryable failure、retry run id、E2EE ignore、system payload filter、private key normalization、runtime status/final outbox、state roundtrip/dedup 测试。 |
| 已更新或缺失文档 | 已更新 | 本 Step 文档与主 Plan 台账回填 Review、验证、剩余风险和 commit 证据。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 delegated inbox sync、state、dispatch/outbox 和直接测试。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`awiki-deamon: sync user delegated inbox`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| message-service delegated inbox API 尚未完成 | Step 07 仍为 pending；本步骤使用 mock adapter 和 im-core optional API 验证 daemon 本地逻辑 | 已实现 daemon 本地 adapter、state 和 mock tests；端到端服务端策略留给 Step 07/09 | 当前步骤 / Step 09 | Step 05 可标记 daemon 本地逻辑 done；不把跨服务端到端验收提前标记完成 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 05 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：错误处理 E2EE opaque notification 可能让 Agent 获得不应有的信息；message-service delegated inbox server policy 未落地前无法完成真实跨服务端到端验收。
- 回滚 / 回退：停用 user delegated inbox poller，保留 existing runtime inbox；清理可疑 message_event、message_sync_outbox 和 delegated key cache。
- 后续文档：Step 07 更新 message-service API/architecture；Step 08 更新 APP action/schema 消费；Step 09 汇总 remote system test 证据。
