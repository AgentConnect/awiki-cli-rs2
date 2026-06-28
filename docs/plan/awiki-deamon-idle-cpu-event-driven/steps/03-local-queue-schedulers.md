# Step 03：本地 due queue scheduler

主 Plan：[../plan.md](../plan.md)  
Step index：03  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | TBD |
| Started | TBD |
| Completed | TBD |
| Commit | TBD |
| Review evidence | TBD |
| Verification evidence | TBD |
| Next action | 将本地队列从 250ms 固定扫描改为 Notify + next due timer + 启动恢复 + 低频 reconciliation。 |
| Assigned agent | agent-scheduler |
| Parallel group | A |
| Parallel safe | yes |
| Parallel with | Step 02；Step 04 默认只读时也可并行 |
| Conflict resources | queue scheduler 模块、queue state helper、`foreground.rs` 局部接入、enqueue notify hook |
| Baseline commit | TBD，必须来自 Step 01 完成后的 commit |
| Worktree / branch | TBD |
| Merge gate | Step 01 done；合并前确认与 Step 02 路径无冲突；Step 05 前必须完成。 |
| Verification gate | scheduler focused tests + `cargo test -p awiki-deamon --locked`。 |
| Gate status | pending |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`message_sync_outbox`、`runtime_final_outbox`、`cli_route_message_queue`、`runtime_retry_queue` 不再依赖 foreground 250ms 全量扫描，而是由入队 notify、最近 due 时间和低频 reconciliation 驱动。
- 用户 / 系统可见行为：CLI route、runtime final outbox、message sync outbox、busy retry 和失败重试仍按原有 due / retry 语义执行；到期任务不会明显延迟；shutdown 能停止 scheduler。
- 非目标：本步骤不接入远端 WebSocket 事件、不修改 message-service 协议、不改变 queue schema 语义、不重写 runtime dispatcher。
- 完成标准：四类队列都有明确唤醒来源、due timer、启动恢复和低频兜底；focused tests 覆盖即时入队、未来 due、due 到达、shutdown、漏 notify 恢复；Wave A group gate 通过。

## 3. 设计方法

- 设计边界：本步骤只处理本地持久队列，不处理远端 direct/group inbox。远端 inbox 事件化在 Step 05 / 06 完成。
- 核心决策：为本地队列建立统一 scheduler 抽象，内部使用 `tokio::sync::Notify` 和 `tokio::time::sleep_until(next_due)`；每次处理后重新查询最近 due。
- 契约 / API / 数据流：
  - 入队路径负责 notify 对应 scheduler。
  - scheduler 负责到期 drain/flush，不改变 queue item 状态机。
  - 低频 reconciliation 只用于恢复漏 notify、启动旧数据和异常路径，不回到 250ms。
- 兼容性：保留现有 due 字段、retry/backoff、processed message 幂等和 error handling；不改变 local RPC payload 或 runtime host contract。
- 迁移策略：无 schema 迁移；若缺少最近 due 查询 helper，新增 state 层 read helper，返回最小 due time 和 pending count。
- 风险控制：所有 scheduler 必须支持 graceful shutdown；测试使用可控时间或短 timeout，避免 flaky 长 sleep。

## 4. 实现方法

1. 梳理四类本地队列：
   - `flush_message_sync_outbox`
   - `flush_runtime_final_outbox`
   - `drain_cli_route_message_queue_once`
   - `drain_runtime_retry_queue_once`
2. 设计 scheduler 结构：
   - 可放在 `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground/queue_scheduler.rs`、`runtime_scheduler.rs` 或符合现有模块风格的位置。
   - 核心字段包含 state handle、notify handles、shutdown receiver / token、reconciliation interval、clock。
   - 每类 queue 可以是一个 task，也可以共享一个 coordinator select；优先选择最小可测试抽象。
3. 增加最近 due 查询：
   - 若 state 层已有 pending/due 查询，直接复用。
   - 若没有，新增只读 helper，例如返回 `Option<i64>` 最近到期时间和待处理数量。
   - helper 必须使用现有 state transaction / repository 风格，避免 ad hoc SQL 字符串散落。
4. 接入 notify：
   - local RPC 入队 CLI route 后 notify `cli_route_message_queue` scheduler。
   - runtime host 写 final outbox / retry queue 后 notify 对应 scheduler。
   - user delegated / message sync outbox 入队后 notify `message_sync_outbox` scheduler。
   - 如果某些入队路径分散，优先通过一个 queue facade 统一 notify，避免漏钩子。
5. 调整 foreground 主循环：
   - 将 250ms 每轮 queue drain 改为启动 queue scheduler task。
   - foreground 保留 supervisor 生命周期管理和 shutdown join。
   - 临时保留低频 reconciliation timer，间隔建议从几十秒起，具体值写入代码常量和测试说明。
6. 增加 focused tests：
   - notify 后立即 drain，不等 reconciliation。
   - 未来 due item 不提前处理，due 到达后处理。
   - missed notify 场景由 startup scan 或低频 reconciliation 恢复。
   - shutdown 能退出 task，不泄露任务。
   - 多 queue 同时到期时不会互相饥饿。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` | 局部替换本地 queue drain 调度入口，启动 / 停止 scheduler。 | 与 Step 05 / 06 互斥；Wave A 内需避免与 Step 02 冲突。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground/queue_scheduler.rs` 或等价新模块 | 新增 Notify + due timer scheduler。 | 具体路径按现有模块组织决定，需在 Plan 台账记录。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/state/*` | 可能新增最近 due 查询 helper 或 pending count helper。 | 不做 schema 迁移，优先只读 helper。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/local_rpc/*` | 可能在 queue 入队成功后触发 notify。 | 不改变 local RPC 协议。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime/host.rs` | 可能在 final outbox / retry queue 入队后触发 notify。 | 不改变 runtime backend contract。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/inbox/user_delegated.rs` | 可能在 message sync outbox 入队后触发 notify。 | 不改变 user delegated inbox 语义。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填 Step 03 状态、验证证据、commit。 | Coordinator 合并时更新。 |

## 6. 依赖与并行约束

- 前置步骤：Step 01 done。
- 可并行步骤：Step 02；Step 04 默认只读或仅做兼容性调查时也可并行。
- 不可并行步骤：Step 05 / 06，因为它们会重构 foreground supervisor 和远端事件调度，依赖本地 queue scheduler 稳定。
- 并行安全依据：本步骤不修改 identity sync helper 和 `im-core` public API；与 Step 02 写入范围分离。
- 互斥资源 / 冲突路径：`foreground.rs` 局部调度段、queue state helper、runtime/local_rpc enqueue hook。
- 外部文档或决策：不需要用户确认；如果发现需要 queue schema migration，必须更新主 Plan 并暂停 Wave A。
- 环境前提：能够运行 daemon tests；若测试需要 fake clock，优先使用当前项目已有时间抽象或 `tokio::time::pause`。
- 合并前置条件：focused scheduler tests 通过；所有入队路径 notify 已 Review；低频 reconciliation 间隔和原因已记录。
- 合并后验证门禁：Wave A 合并后运行 `cargo test -p awiki-deamon --locked`。

## 7. 验收标准

- [ ] 四类本地队列都有明确 notify 入口和 due timer 驱动。
- [ ] 未来 due item 不提前执行，到期后不依赖 250ms 轮询即可执行。
- [ ] daemon 启动时能发现已有 pending/due item。
- [ ] 漏 notify 时能通过低频 reconciliation 恢复，且间隔不是 250ms 级别。
- [ ] scheduler shutdown 可控，不留下悬挂 task。
- [ ] 不改变 queue item 状态机、retry/backoff、processed 幂等、local RPC payload 或 runtime backend contract。
- [ ] 如果本步骤标记为 parallel-safe，已确认没有修改 identity sync 或 `im-core` 互斥资源。
- [ ] 如果本步骤属于并行组，已记录 Agent、基线 commit、分支 / worktree 和合并门禁状态。
- [ ] 本步骤合并前的 Step gate 已通过，或已记录不能运行的具体原因和风险。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入 Step 05 之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Scheduler focused tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked queue_scheduler` 或实际测试名 | commit 前 | notify、due、shutdown、reconciliation tests 通过 | Step gate |
| Queue state tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked queue` | commit 前 | 最近 due helper 和状态机 tests 通过 | Step gate |
| Daemon unit | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | commit 前 | crate tests 通过或记录原因 | Step gate |
| 入队 hook Review | 人工检查 `local_rpc`、`runtime/host.rs`、`user_delegated.rs` 等入队路径 | Review 前 | 每个成功入队路径都有 notify 或由 startup/reconciliation 覆盖 | Review gate |
| Idle 对比 | 复用 Step 01 采样，观察本地 queue drain 不再 250ms 每轮运行 | Step 03 后或 Wave A 后 | CPU/I/O/日志对比表 | Evidence |
| Parallel scope check | `cd awiki-cli-rs2-cpu && git diff --name-only` | commit 前 | 不包含 Step 02 授权路径或 `im-core` public API | Group gate |
| Group Verification | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | Wave A 合并后 | Step 02 + Step 03 组合后通过 | Group gate |

如果 focused test 名称不同，执行者必须在台账记录实际命令；不能只记录计划命令。

## 9. Review 环节

- Review 时机：scheduler 实现和 tests 完成后、commit 前；Wave A 合并后由 coordinator 做组合 Review。
- Review 重点：所有入队路径是否 notify、最近 due 查询是否正确、timer reset 是否处理新更早 due、shutdown 是否可靠、reconciliation 是否低频且有明确目的、是否保留原状态机。
- Review 必须检查不存在 busy loop：空队列、未来 due、连续错误、shutdown 后都不能快速循环占 CPU。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | TBD | TBD |
| 已修复问题 | TBD | TBD |
| 剩余风险 | TBD | TBD |
| 新增或缺失测试 | TBD | TBD |
| 已更新或缺失文档 | TBD | 通常 final 更新 daemon docs。 |
| 并行安全是否仍成立 | TBD | 不应修改 identity sync 或 `im-core`。 |
| Agent 是否越界修改 | TBD | TBD |
| 互斥资源是否被修改 | TBD | `foreground.rs` 修改需限定在本地 queue scheduler 接入。 |
| 合并风险 | TBD | 与 Step 05 前后衔接风险较高，需记录接口。 |
| Group gate 影响 | Wave A | 合并后跑 daemon tests 和 idle 对比。 |

## 10. Commit 要求

- Commit 时机：focused scheduler tests、daemon tests、Review 都完成后。
- Commit 范围：只包含本地 queue scheduler、必要 state helper、入队 notify hook、相关 tests、执行台账回填。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 并行步骤的 commit 必须基于 Step 01 的基线 commit，或说明 rebase / merge 过程。
- Commit 后必须记录是否 `ready_for_group_merge`。
- 如果 commit 修改了原计划未授权路径，必须先更新主 Plan 的 parallel-safe 判定和变更记录。
- 建议消息：`daemon: schedule local queues by notify and due time`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| 无法枚举所有入队路径 | Review 发现 queue 写入分散且无统一 facade | `rg` 查找 queue enqueue / insert / retry 写入 | 当前步骤 | 是 | 是 | 先新增最小 facade 或记录未覆盖路径并保留 reconciliation，更新 Plan。 |
| 缺少最近 due 查询且新增 helper 需要 schema 改动 | state tests 或 SQL 限制 | 查找现有索引和 due 字段；尝试只读排序查询 | 当前步骤 / schema | 是 | 是 | 暂停并更新主 Plan；schema 改动需独立验证。 |
| scheduler test flaky | CI / 本地 timeout 不稳定 | 使用 fake clock、缩短受控 timeout、避免 wall clock race | 当前步骤 | 否 | 是 | 修复测试后再提交。 |
| queue 漏唤醒造成系统行为回归 | focused test 或手动验证失败 | startup recovery、reconciliation、补 notify hook | Step 03 / Step 07 | 是 | 是 | 修复或临时保留低频 drain，记录风险。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 03 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：漏 notify 或 timer reset 错误会造成 queue 延迟处理；错误处理 busy loop 会抵消 CPU 优化。
- 并行执行风险：本步骤可能碰 `foreground.rs`，必须与 Step 05 / 06 串行；Wave A 内不要越界修改 Step 02。
- 合并冲突风险：中等；`foreground.rs` 后续会被 Step 05 大幅集成，需要写清 scheduler API。
- Group gate 失败回退：恢复对应 queue 的低频 reconciliation 或旧 drain 入口，但避免恢复 250ms 全量扫作为长期方案。
- Agent 交接说明：Step 05 远端 realtime supervisor 启动时，应复用本步骤的本地 queue scheduler 生命周期和 shutdown 机制。
- 回滚 / 回退：可按 queue 类型逐个回退到低频 timer；若某队列风险高，可暂时只事件化其他队列并更新 Plan。
- 后续文档：Step 07 更新 daemon docs 中本地队列调度模型、reconciliation 作用和 troubleshooting。
