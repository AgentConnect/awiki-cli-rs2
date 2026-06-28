# Step 06：reliable sync、gap 与 fallback 协调

主 Plan：[../plan.md](../plan.md)  
Step index：06  
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
| Next action | 在 Step 05 realtime supervisor 上补齐 sync hint、gap、reconnect、snapshot_required 和低频 fallback 策略。 |
| Assigned agent | coordinator |
| Parallel group | 串行 |
| Parallel safe | no |
| Parallel with | 无 |
| Conflict resources | realtime supervisor、sync/fallback coordinator、foreground select、`im-core` sync contract |
| Baseline commit | TBD，必须包含 Step 05 完成结果 |
| Worktree / branch | TBD |
| Merge gate | Step 05 done；realtime fan-in 和 dispatcher 稳定。 |
| Verification gate | reconnect/gap/fallback focused tests + `cargo test -p awiki-deamon --locked`；必要时 `cargo test -p im-core --locked realtime sync`。 |
| Gate status | pending |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：daemon 对 WebSocket hint、断线、重连、gap、unknown notification、snapshot_required 和服务端不可用有可靠、低 CPU 的处理路径；远端消息可靠性依赖 `im-core` sync API，而不是 daemon 自己推进 checkpoint 或恢复 250ms 全量扫。
- 用户 / 系统可见行为：WSS 短暂断开后，daemon 能重连并通过 sync 补齐消息；消息不重复执行；WSS 不可用时降级为低频 fallback；snapshot_required 等高风险状态 fail-closed 并记录清晰错误。
- 非目标：不新增 message-service protocol，不让 daemon 写 sync checkpoint，不重写 `im-core` sync engine，不消除所有 polling，低频 reconciliation 仍作为可靠兜底。
- 完成标准：sync delta/thread-after 调度、reconnect backoff、dirty agent/thread/group 归并、fallback intervals、snapshot_required 策略和 observability 都有实现和 tests；最终不再使用 250ms 全量 direct/group 扫描。

## 3. 设计方法

- 设计边界：realtime 是通知通道，reliable data source 是 `im-core` sync；daemon 只调度 sync，不直接操作 checkpoint。
- 核心决策：
  - `RealtimeSyncHint`、disconnect、reconnect、gap 都转化为 dirty work item。
  - dirty work item 按 agent / thread / group 归并，避免 event storm。
  - 使用 `sync_delta_async` 补齐账号级变化，使用 `sync_thread_after_async` 或 `groups().messages_async` 做 targeted context。
  - 低频 fallback 只在 WSS 不可用、unknown event、missed notify 或 startup recovery 时触发。
- 契约 / API / 数据流：
  - checkpoint 只由 `im-core` 内部 sync transaction 推进。
  - daemon 的 processed message 幂等继续防重复 runtime execution。
  - fallback poll 不得恢复 250ms；默认可以是 30s / 60s / 5min 分层，最终值由实现和测试记录。
- 兼容性：保持 Step 05 dispatcher contract；不改变 runtime payload、local RPC 或 message-service WSS frame。
- 迁移策略：无 schema 迁移；如需要存储 per-agent last fallback time，优先内存态，必要持久化需更新 Plan。
- 风险控制：断线重连和 sync error 必须指数退避加 jitter；snapshot_required 不得盲目继续处理旧数据。

## 4. 实现方法

1. 建立 fallback coordinator：
   - 接收 Step 05 的 `DaemonRealtimeEvent`、session status、reader errors 和 unknown event。
   - 管理 dirty set：agent-level、thread-level、group-level、reason、first_seen、last_attempt、attempt_count。
   - 对 dirty set 使用 notify + due timer，避免每个 event 立即同步造成 storm。
2. 处理 sync hint：
   - 对 `RealtimeSyncHint` 触发 agent-level `sync_delta_async`。
   - 对 message event 中能定位 thread 的情况，优先 `sync_thread_after_async` 或直接 dispatcher。
   - 对 group event 缺上下文时，使用 `groups().messages_async` 获取少量上下文，不全 group 扫。
3. 处理 reconnect / gap：
   - session reconnect 成功后，调度一次 agent-level delta sync。
   - reader error / disconnect 进入 backoff，并标记 agent dirty。
   - gap / missed event / channel pressure 触发 delta sync，而不是丢弃后继续等待。
4. 处理 fallback poll：
   - WSS 正常：低频 health / reconciliation，可较长间隔。
   - WSS 失败但可重试：按 agent backoff + delta sync，间隔明显大于 250ms。
   - WSS 长期不可用：进入 degraded mode，低频 direct/group poll fallback，记录原因和次数。
5. 处理 snapshot_required / fatal sync：
   - 如果 `im-core` 返回需要 snapshot 或本地 checkpoint 不可信，daemon 不自行推进 checkpoint。
   - 标记 agent degraded / blocked，记录 audit，提示需要更高层恢复策略。
6. 保持幂等：
   - 所有 sync 后发现的 message 仍走 processed message 幂等检查。
   - runtime retry / final outbox 仍交给 Step 03 queue scheduler。
7. tests：
   - reconnect 后触发 delta sync。
   - gap hint 合并为一次 sync。
   - unknown event 触发 fallback，不 busy loop。
   - snapshot_required fail-closed。
   - duplicate message 不重复 runtime execution。
   - WSS down 时低频 fallback 执行且间隔可控。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime_realtime.rs` 或 Step 05 等价模块 | 增加 sync/gap/fallback coordinator、dirty set、backoff。 | 复用 Step 05 结构。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` | 将 fallback due timer 接入主 select / supervisor lifecycle。 | 串行修改。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs` | 复用 `ImClient` 调用 `sync_delta_async` / `sync_thread_after_async`；不高频创建 client。 | 如需新 helper，保持 daemon-private。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/state/*` | 复用 processed message 幂等；默认不新增 schema。 | 需要持久化 fallback 状态时先更新 Plan。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/messages/service.rs` | 默认只读；确认 sync error / snapshot_required 类型。 | 如改 shared API，回到 Step 04。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*` | 默认只读；可能只补 tests。 | 不改 `ImEvent` DTO。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填 Step 06 状态、验证证据、commit。 | Coordinator 更新。 |

## 6. 依赖与并行约束

- 前置步骤：Step 05 done。
- 可并行步骤：无。
- 不可并行步骤：Step 07 依赖本步骤；与任何 realtime supervisor / foreground fallback 改动互斥。
- 并行安全依据：不适用，本步骤串行。
- 互斥资源 / 冲突路径：realtime supervisor、sync coordinator、foreground select、shared sync contract。
- 外部文档或决策：如果发现 message-service 需要新 notification 类型或 `im-core` 需要 public sync API，先 blocked 并更新 Plan。
- 环境前提：能够运行 focused tests；可用 fake realtime events 和 fake sync client 模拟 gap/reconnect。
- 合并前置条件：fallback intervals、backoff、dirty set 合并、snapshot_required 策略和 tests 已 Review。
- 合并后验证门禁：`cargo test -p awiki-deamon --locked`；必要时 `cargo test -p im-core --locked realtime sync`。

## 7. 验收标准

- [ ] `RealtimeSyncHint` 不被当作 checkpoint，只调度 `sync_delta_async` / `sync_thread_after_async`。
- [ ] reconnect 成功后至少调度一次 agent-level reliable sync。
- [ ] gap / unknown / channel pressure 能进入 dirty set 和 fallback，而不是静默丢弃。
- [ ] fallback interval 明显大于 250ms，并带 backoff / jitter / reason 记录。
- [ ] group context 缺失时使用 targeted group/thread fetch，不全 group 扫。
- [ ] snapshot_required 或 checkpoint 不可信时 fail-closed，记录恢复需求，不继续盲目执行。
- [ ] duplicate message 不重复 runtime execution。
- [ ] 不修改 message-service protocol；不出现未经 Step 04 批准的 `im-core` public API 变更。
- [ ] 本步骤合并前的 Step gate 已通过，或已记录不能运行的具体原因和风险。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入 Step 07 之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Fallback focused tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked fallback` 或实际测试名 | commit 前 | reconnect、gap、unknown、snapshot_required tests 通过 | Step gate |
| Realtime focused tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked realtime` | commit 前 | Step 05 supervisor 行为不回归 | Step gate |
| Daemon unit | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | commit 前 | crate tests 通过或记录原因 | Step gate |
| Im-core sync focused | `cd awiki-cli-rs2-cpu && cargo test -p im-core --locked realtime sync` | 如触及 sync/realtime shared behavior | tests 通过或记录原因 | Contract gate |
| Duplicate execution check | focused test 或手动 fixture，同一 message 经 event + sync 出现两次 | Review 前 | processed message 幂等阻止重复 runtime invocation | Review gate |
| Idle 对比 | 复用 Step 01 采样，WSS 正常时无 250ms direct/group fallback | Step 06 后 | CPU/I/O/日志对比表 | Evidence |
| Degraded mode smoke | 人为断开 WSS 或 fake disconnect，观察 backoff/fallback 日志和恢复 | Review 前或 Step 07 前 | 不 busy loop，恢复后 sync 补齐 | Integration evidence |

如果无法做 live disconnect，必须用 deterministic fake tests 覆盖，并把 live 验证留到 Step 07。

## 9. Review 环节

- Review 时机：sync/fallback 实现和 tests 完成后、commit 前。
- Review 重点：checkpoint 边界、sync 调用条件、dirty set 合并、backoff/jitter、fallback 间隔、snapshot_required、幂等、安全日志、shared API 守门。
- Review 必须检查没有把 fallback 设计退化为 250ms 全量扫描；如果保留低频 poll，必须有明确间隔、触发条件和停止条件。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | TBD | TBD |
| 已修复问题 | TBD | TBD |
| 剩余风险 | TBD | TBD |
| 新增或缺失测试 | TBD | TBD |
| 已更新或缺失文档 | TBD | final 更新 daemon docs。 |
| 并行安全是否仍成立 | no | 本步骤串行。 |
| Agent 是否越界修改 | TBD | TBD |
| 互斥资源是否被修改 | TBD | realtime supervisor / foreground 为授权范围。 |
| 合并风险 | TBD | 进入 Step 07 前必须稳定。 |
| Group gate 影响 | 无 | Final gate 依赖本步骤。 |

## 10. Commit 要求

- Commit 时机：fallback focused tests、daemon tests、Review 都完成后。
- Commit 范围：只包含 Step 06 的 sync/gap/fallback coordinator、tests、必要 docs 台账。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- Commit 后必须记录 Step 07 可以启动的前置条件。
- 如果 commit 修改了 `im-core`、message-service 协议或 state schema，必须先更新主 Plan 并完成对应 Review。
- 建议消息：`daemon: coordinate realtime sync fallback`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| sync API 无法表达需要的 delta/thread-after | 编译或运行证据、缺少参数或错误类型 | 使用现有 `sync_delta_async`、`sync_thread_after_async`、group messages | Step 06 / Step 04 | 是 | 是 | 暂停，回到 Step 04 做共享 API 评审。 |
| snapshot_required 恢复策略不明确 | `im-core` 返回需要 snapshot，但无安全恢复 API | fail-closed、记录 audit、低频 fallback 不推进 checkpoint | 当前步骤 / final | 否 | 是 | 标记风险，询问用户是否扩展任务。 |
| fallback 仍造成高 CPU | idle 采样显示高频循环或日志 storm | backoff、jitter、间隔上调、dirty set 合并 | Step 06 / Step 07 | 否 | 是 | 修复后重跑 idle 采样。 |
| duplicate execution | tests 显示 event + sync 双路重复 runtime invocation | processed message 幂等、dedupe key 修正 | Step 06 | 否 | 是 | 修复并新增 regression test。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 06 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：fallback 过慢会延迟消息处理，过快会重新制造 CPU/I/O；sync 错误处理不当会漏消息或重复执行。
- 并行执行风险：本步骤串行，避免与 supervisor 主控制流冲突。
- 合并冲突风险：中到高；会触及 Step 05 新增模块和 foreground select。
- Group gate 失败回退：保留 realtime event 主链路，临时关闭复杂 fallback 或降级到低频 poll；记录可靠性风险。
- Agent 交接说明：Step 07 需要重点验证 WSS 正常、WSS 断线、remote system test 和 idle 采样。
- 回滚 / 回退：可按 fallback coordinator feature flag / config 回退到低频 reconciliation；不得恢复 250ms 全量扫作为最终状态。
- 后续文档：Step 07 更新 daemon docs 的 WSS degraded mode、fallback interval、troubleshooting 和 checkpoint 边界说明。
