# Step 04：M-Core realtime endpoint 与共享接口守门

主 Plan：[../plan.md](../plan.md)  
Step index：04  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/perf/cpu-youhua-jingmo-0628` |
| Started | 2026-06-28T16:49:00+08:00 |
| Completed | 2026-06-28T16:54:56+08:00 |
| Commit | `6e750ff` |
| Review evidence | 当前 `im-core` public API 足够首版 daemon 多 WebSocket fan-in：daemon 使用 per-agent `RealtimeSession`，每个 session 只有一个 reader，source metadata 放在 daemon wrapper；不修改 `ImEvent`、DTO、feature gate 或 transport 默认语义。 |
| Verification evidence | `git diff -- crates/im-core` 无输出；`cargo test -p im-core --locked realtime -j1` 通过；`cargo test -p im-core --locked sync -j1` 通过；原计划单命令 `cargo test -p im-core --locked realtime sync -j1` 已确认是非法 Cargo 命令形状并修正。 |
| Next action | Step 05 可以启动：在 daemon 层实现 per-agent realtime supervisor 和统一事件 fan-in，不修改 `im-core` public API。 |
| Assigned agent | agent-sdk-contract |
| Parallel group | B |
| Parallel safe | yes |
| Parallel with | Step 02 / Step 03；仅当本步骤保持只读或小范围向后兼容改动时成立 |
| Conflict resources | `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*`、`awiki-cli-rs2-cpu/crates/im-core/src/messages/*`、public API / DTO / transport policy |
| Baseline commit | `db30f77` |
| Worktree / branch | 当前主工作区 / `feature/perf/cpu-youhua-jingmo-0628` |
| Merge gate | Step 05 前必须完成；若需要修改 public API，暂停并等待兼容性评审和用户确认。 |
| Verification gate | 默认 shared SDK diff check；如改 `im-core`，运行 `cargo test -p im-core --locked realtime -j1`、`cargo test -p im-core --locked sync -j1` 和共享调用方回归。 |
| Gate status | pass |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：给出可执行结论：首版 daemon 事件驱动是否可以完全复用现有 `im-core` API；如果不能，列出最小、向后兼容、需用户确认的 API / endpoint 修改项。
- 用户 / 系统可见行为：默认不改变 `awiki-cli`、`im-core-dart`、App 或其他 `im-core` 调用方行为；daemon 后续只在已有 API 能力上构建多 WebSocket fan-in。
- 非目标：不把 daemon source metadata 加进 `im-core::ImEvent`；不让 `im-core` 承担 daemon 多 session supervisor；不改变 message-service 协议；不暴露 checkpoint 给 daemon 外部。
- 完成标准：形成明确 contract 结论、endpoint 选择结论、共享接口 diff 证据；如无改动，Step 05 可继续；如必须改，主 Plan 已更新、用户已确认、回归范围已扩大。

## 3. 设计方法

- 设计边界：`im-core` 是共享 SDK，daemon 是其一个调用方；daemon 特有的多 WebSocket source、agent session lifecycle、routing/fallback 应放在 daemon 层。
- 核心决策：
  - 首选复用 `RealtimeService::start_async`、`RealtimeSession::subscribe`、`status_updates`、`stop`、`join`。
  - 首选复用 `messages().sync_delta_async`、`messages().sync_thread_after_async` 和 `groups().messages_async`。
  - `RealtimeSession::subscribe()` 是单 session reader，不要求 `im-core` 内置 multi-session multiplexer。
- 契约 / API / 数据流：
  - `im-core::ImEvent` 保持 SDK 级事件，不带 daemon-only `agent_did`、`session_id`、`endpoint_kind`。
  - daemon 通过 wrapper `DaemonRealtimeEvent { source, event }` 承载 source metadata。
  - realtime hint 只触发 sync，不能当作可靠 checkpoint。
- 兼容性：任何 public API、DTO、feature gate、transport 默认语义变化都必须先 blocked，完成兼容性评审和用户确认。
- 迁移策略：默认无迁移；若仅 endpoint 选择内部逻辑需要修正，应保持向后兼容，例如优先使用现有配置中 message-service endpoint，缺省回退 `service_base_url`。
- 风险控制：不能让解决 daemon endpoint 问题变成修改所有 SDK 调用方默认行为；如线上 endpoint 差异无法确认，先通过 daemon 配置或 adapter 处理。

## 4. 实现方法

1. 复核现有 `im-core` API：
   - `awiki-cli-rs2-cpu/crates/im-core/src/realtime/session.rs`
   - `awiki-cli-rs2-cpu/crates/im-core/src/realtime/service.rs`
   - `awiki-cli-rs2-cpu/crates/im-core/src/realtime/events.rs`
   - `awiki-cli-rs2-cpu/crates/im-core/src/messages/service.rs`
   - `awiki-cli-rs2-cpu/crates/im-core/src/groups/service.rs`
2. 输出能力矩阵：
   - per-agent session 启动：是否满足。
   - session event stream：是否满足单 reader。
   - session status / shutdown：是否满足。
   - direct/group message event：是否足够定位。
   - sync hint / gap：是否足够触发 reliable sync。
   - thread-after / delta / group messages：是否足够补齐上下文。
3. 验证 endpoint 选择：
   - 检查 realtime endpoint 是否由 `service_base_url` 推导 `/im/ws`。
   - 检查 daemon config 是否存在 `message_service_base_url` / `message_service_endpoint` 与 `service_base_url` 差异。
   - 如果部署要求不同 endpoint，优先设计向后兼容内部选择：`message_service_endpoint.unwrap_or(service_base_url)`，不改变调用方 public contract。
4. 决策树：
   - 如果现有 API 满足：本步骤只更新 Plan 台账和 Step 证据，不改代码或只补充测试。
   - 如果需要内部 endpoint 选择修正且不破坏 public API：更新主 Plan，限定 `im-core` internal change 和 tests。
   - 如果需要新增 / 修改 public API：标记 blocked，写明为什么 daemon adapter 不能解决，等待用户确认后再改。
5. 验证共享接口：
   - 检查 `crates/im-core` diff。
   - 如无 diff，记录“未修改共享 SDK”。
   - 如有 diff，运行 `im-core` focused tests 和共享调用方回归，并更新 docs。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/session.rs` | 默认只读；必要时增加 tests 或内部兼容修正。 | 禁止无评审 public API 变更。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/service.rs` | 检查 endpoint 选择和 `HttpOnly` 拒绝逻辑；必要时做向后兼容内部调整。 | 任何行为变化都需记录。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/events.rs` | 默认只读；不加入 daemon source metadata。 | source metadata 属于 daemon wrapper。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/messages/service.rs` | 默认只读；确认 `sync_delta_async` / `sync_thread_after_async` 能力。 | 不暴露 checkpoint。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/groups/service.rs` | 默认只读；确认 `groups().messages_async` 用于 group context。 | 不做全 group 扫描方案。 |
| `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` | 只有 public API 或 endpoint 语义实际变化时更新。 | 默认无需修改。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填 Step 04 结论、证据、是否允许 Step 05 启动。 | 必须更新执行台账。 |

## 6. 依赖与并行约束

- 前置步骤：Step 01 done。
- 可并行步骤：Step 02 / Step 03，前提是本步骤保持只读或只在 `im-core` 内做独立兼容 tests。
- 不可并行步骤：Step 05 必须等待本步骤结论；如果 API 变更 blocked，Step 05 不得继续依赖未确认契约。
- 并行安全依据：默认只读调查，不与 daemon storage / queue 写入冲突。
- 互斥资源 / 冲突路径：`crates/im-core/src/realtime/*` public API、`ImEvent` DTO、transport policy 默认行为、shared docs。
- 外部文档或决策：修改 public API / DTO / transport 语义前必须向用户说明影响并等待确认。
- 环境前提：能够运行 `im-core` focused tests；若 `im-core-dart` tests 在环境中不可用，需记录原因和替代检查。
- 合并前置条件：Step 04 结论已写入主 Plan；如有代码改动，tests 和 docs 同步完成。
- 合并后验证门禁：Step 05 开始前确认 shared SDK gate 是 pass 或明确 blocked。

## 7. 验收标准

- [x] 明确回答“现在 API 是否满足当前需要”：满足；证据见第 14 节能力矩阵和第 15 节验证证据。
- [x] 明确回答“如果要修改，要修改哪些 API”：首版不修改 M-Code / `im-core` public API；如后续证明 endpoint 分离部署阻塞 Step 05/06，优先做向后兼容内部 endpoint 选择或 daemon 配置策略。
- [x] 明确回答“多个 WebSocket 的异步事件如何处理”：daemon 层 per-agent session task fan-in 到统一 channel，不改 `ImEvent`。
- [x] 已确认 `RealtimeSession::subscribe()` 单 reader 约束，并在 daemon 设计中只由一个 task 读取每个 session。
- [x] 已确认 realtime hint 不推进 checkpoint，可靠同步仍调用 `sync_delta_async` / `sync_thread_after_async`。
- [x] 已检查 `service_base_url` 与 message-service endpoint 选择风险，并记录结论。
- [x] 如果本步骤标记为 parallel-safe，已确认没有修改 Step 02 / 03 互斥资源或超出授权路径。
- [x] 如果本步骤属于并行组，已记录 Agent、基线 commit、分支 / worktree 和合并门禁状态。
- [x] 本步骤合并前的 Step gate 已通过，或已记录不能运行的具体原因和风险。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入 Step 05 之前已经创建聚焦 commit：`6e750ff`。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Shared SDK diff check | `cd awiki-cli-rs2-cpu && git diff -- crates/im-core` | Review 前、commit 前 | 无未授权 diff；或 diff 已获批准并记录 | Step gate |
| Im-core focused tests | `cd awiki-cli-rs2-cpu && cargo test -p im-core --locked realtime -j1` 和 `cd awiki-cli-rs2-cpu && cargo test -p im-core --locked sync -j1` | 如修改 `im-core`、补 tests 或执行 Step 04 contract gate | tests 通过或记录原因 | Step gate |
| Daemon compile compatibility | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | 如修改 `im-core` 或 daemon config | daemon 仍能编译和测试 | Step gate |
| Shared caller regression | `cd awiki-cli-rs2-cpu && cargo test -p awiki-cli --locked && cargo test -p im-core-dart --locked` | 仅当修改 `im-core` public API / DTO / transport 语义 | 共享调用方通过或记录环境失败原因 | Shared SDK gate |
| Docs contract check | 检查 `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` 是否需要更新 | Review 前 | 若无需更新，记录理由；若需要，完成更新 | Docs gate |
| Plan gate | 回填主 Plan Step 04 结论和 Step 05 前置条件 | commit 前 | Step 05 可启动 / blocked 状态明确 | Integration gate |

如果必须改 public API 但用户尚未确认，本步骤状态保持 `blocked`，不得将后续实现建立在未确认接口上。

## 9. Review 环节

- Review 时机：能力矩阵和 endpoint 判断完成后；如有代码改动，代码实现和 tests 完成后、commit 前。
- Review 重点：共享 SDK 兼容性、是否有更小的 daemon 内 adapter 方案、是否误把 daemon metadata 塞进 `im-core`、endpoint 选择是否向后兼容、docs 是否同步。
- Review 必须由 coordinator 以 contract review 视角执行，优先找影响 `awiki-cli`、`im-core-dart`、App 和 message-service 协议的风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 无 public API 缺口；发现 endpoint 选择风险和验证命令形状错误 | realtime WSS 当前从 `service_base_url` 推导 `/im/ws`；daemon 当前 config 仍设为 `HttpOnly`，Step 05 需要 daemon 配置层启用 realtime。 |
| 已修复问题 | 修正文档中的验证命令 | 将非法的 `cargo test -p im-core --locked realtime sync` 拆成 `realtime` 与 `sync` 两条 focused test。 |
| 剩余风险 | endpoint 分离部署风险 | 如果 `service_base_url` 与 `message_service_base_url` 不同，后续优先做向后兼容内部 endpoint 选择或 daemon config 策略，不直接新增 public API。 |
| 新增或缺失测试 | 未新增测试，已运行 focused tests | 本步骤未修改代码；`realtime` 与 `sync` focused tests 通过。 |
| 已更新或缺失文档 | 已更新主 Plan 和本 Step 文档；未更新 `im-core` API 文档 | 未修改 `im-core` public API 或 endpoint 语义，因此 `docs/api/im-core-interface/04-message-interface.md` 暂不需要更新；Step 07 再做最终文档同步检查。 |
| 并行安全是否仍成立 | 是 | 没有修改 Step 02 / 03 互斥资源；没有 shared API 改动。 |
| Agent 是否越界修改 | 否 | 本步骤只回填计划文档和验证证据。 |
| 互斥资源是否被修改 | 否 | `git diff -- crates/im-core` 无输出。 |
| 合并风险 | 低 | Step 05 可在 daemon 层实现 supervisor；若后续发现 endpoint 阻塞，再按 Plan 变更控制处理。 |
| Group gate 影响 | Wave B | Step 04 结论是 Step 05 gate。 |

## 10. Commit 要求

- Commit 时机：contract 结论、必要 tests、Review 都完成后。
- Commit 范围：默认只包含 Plan 台账 / docs 结论；若批准代码改动，只包含 `im-core` endpoint / tests / docs 相关最小变更。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 并行步骤的 commit 必须基于 Step 01 的基线 commit 或说明 rebase / merge 过程。
- Commit 后必须记录是否 `ready_for_group_merge` 和是否允许 Step 05。
- 如果 commit 修改了 public API 或原计划未授权路径，必须先更新主 Plan 的 parallel-safe 判定和变更记录。
- 建议消息：`im-core: document realtime contract for daemon` 或 `im-core: choose realtime endpoint compatibly`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| daemon 必须新增 `im-core` public API 才能实现多 WSS fan-in | 能力矩阵显示现有 session / subscribe / sync API 不足 | daemon wrapper、per-session task、adapter、internal helper | 共享 SDK / Step 05 | 是 | 是 | 暂停；向用户列出 API 修改项、兼容性影响和验证范围，等待确认。 |
| realtime endpoint 无法连接目标 message-service | endpoint 推导与部署配置不一致、WSS 连接失败 | 检查 daemon config、`message_service_base_url`、service discovery | Step 04 / Step 05 | 是 | 是 | 优先内部向后兼容 endpoint 选择；若需 public config，用户确认。 |
| `im-core-dart` 或 `awiki-cli` 回归不可运行 | 命令失败或环境缺失 | 记录环境、尝试 focused compile/test | Shared SDK gate | 是 | 是 | 不把 shared API 改动标记完成，除非用户接受记录的风险。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 04 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |
| 2026-06-28 | 修正 Step 04 focused test 命令并记录 contract 结论 | Cargo 不接受 `realtime sync` 双过滤参数；复核后确认首版无需改 `im-core` public API | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：轻率修改 `im-core` 会影响 CLI、Dart bridge、App 和外部调用方；endpoint 选择错误会让 daemon realtime 永远连接错误服务。
- 并行执行风险：默认只读时低；一旦要改 `im-core`，必须暂停并重新评估 Step 02 / 03 / 05 的并行和依赖关系。
- 合并冲突风险：低到中；`im-core` realtime 可能同时被其他任务修改。
- Group gate 失败回退：取消共享 SDK 改动，改用 daemon 内 adapter 或保留低频 fallback；Step 05 不依赖未确认 API。
- Agent 交接说明：Step 05 执行者必须读取本步骤结论，不得自行新增 `im-core` public API 或修改 `ImEvent` DTO。
- 回滚 / 回退：如 endpoint 内部修正有回归，恢复旧 endpoint 推导，并在 daemon 配置层或 fallback 层处理。
- 后续文档：如未改 public API，Step 07 记录已检查 `im-core` docs 且无需更新；如改 endpoint 或 API，更新 `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` 和 Harness 相关架构摘要。

## 14. 能力矩阵与 contract 结论

| 问题 | 当前 API 能力 | Step 04 结论 |
|---|---|---|
| per-agent session 启动 | `RealtimeService::start_async(RealtimeOptions) -> RealtimeSession` 已存在，并在 `HttpOnly` policy 下 fail closed。 | 满足；Step 05 需要 daemon config 启用非 `HttpOnly` realtime policy。 |
| session event stream | `RealtimeSession::subscribe()` 返回 `RealtimeEventStream = tokio::sync::mpsc::Receiver<ImEvent>`，并且只能调用一次。 | 满足；daemon 每个 session 只放一个 reader task，再 fan-in 到统一 daemon channel。 |
| session 状态与 shutdown | `status_updates()` 返回 watch receiver；`stop()` 请求 shutdown；`join()` 等待 `RealtimeExit`；`Drop` 会 request shutdown。 | 满足 daemon supervisor 生命周期管理。 |
| SDK 事件 DTO | `ImEvent` 覆盖 connection、message、group、local notification、host notification 和 unknown notification。 | 满足；不把 daemon-only `agent_did`、`session_id`、`endpoint_kind` 塞进 `ImEvent`。 |
| sync hint | `RealtimeSyncHint` 包含 `event_id`、`event_seq`、`event_type`、`sync_dirty`、`gap_detected`。 | 只作为调度 hint；不能当作可靠 checkpoint。 |
| reliable sync | `messages().sync_delta_async(...)`、`messages().sync_thread_after_async(...)` 已存在。 | 满足 dirty/gap 后可靠补齐。 |
| group context | `groups().messages_async(...)` 已存在。 | 满足首版按 group/thread 拉取少量上下文，不要求全 group 扫描。 |
| 多 WebSocket 统一读取 | `im-core` 没有内置 multi-session multiplexer。 | 不需要改 API；daemon 层建立 per-agent task fan-in 机制。 |
| endpoint 选择 | realtime WSS 当前从 `sdk_config().service_base_url` 推导 `/im/ws`；通用 HTTP transport 会优先使用 `message_service_endpoint.unwrap_or(service_base_url)`。 | 记录为 Step 05/06 配置风险；除非实际阻塞，不在 Step 04 修改 public API。 |

最终结论：现有 M-Code / `im-core` API 满足当前首版 daemon 事件驱动需要，不需要修改 public API。Step 05 的实现边界是在 `awiki-deamon` 层新增 supervisor、source wrapper、fan-in channel、路由和 lifecycle 管理。

## 15. 验证证据

| 检查项 | 结果 |
|---|---|
| `git status --short --branch` | 分支为 `feature/perf/cpu-youhua-jingmo-0628`；本步骤只包含计划文档改动。 |
| `git diff -- crates/im-core` | 无输出，确认未修改 shared SDK。 |
| 原计划命令形状 | `cargo test -p im-core --locked realtime sync -j1` 会被 Cargo 视为非法多过滤参数，已在主 Plan 和本 Step 中修正。 |
| `cargo test -p im-core --locked realtime -j1` | 通过；lib 23 passed / 0 failed；`realtime_api` 6 passed、`realtime_connect` 5 passed、`realtime_frame` 9 passed、`realtime_loop` 16 passed、`realtime_projection` 16 passed。 |
| `cargo test -p im-core --locked sync -j1` | 通过；lib 62 passed / 0 failed，并覆盖 sync delta、sync thread-after、realtime sync hint、secure direct async receive/send 等相关过滤测试。 |
| shared caller regression | 未运行；因为没有 `crates/im-core` 改动，没有触发 shared SDK regression 门禁。 |
| 文档检查 | `docs/api/im-core-interface/04-message-interface.md` 已复核同步 / checkpoint 语义；因为未改 public API 或 endpoint 语义，本步骤不更新该文档。 |
