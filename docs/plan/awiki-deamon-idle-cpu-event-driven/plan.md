# Plan：awiki-deamon 静默 CPU 与事件驱动优化

状态：draft  
DOC：`awiki-cli-rs2/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md`  
Harness：`awiki-harness`  
创建时间：2026-06-28  
恢复指针：执行开始前从 Step 01 开始；本文件是当前唯一规划入口，后续正式执行时可按本文件拆分 step 文档。

## 1. 目标

- 任务目标：沉淀 `awiki-deamon` 静默 CPU 占用偏高的代码和运行态分析，并给出后续把高频轮询改造成异步事件触发模型的可执行优化计划。
- 预期行为：后续优化完成后，daemon 在没有远端消息、没有本地 runtime 回调、没有 outbox 到期任务时应主要处于等待状态；有 WSS notification、本地 RPC、队列到期或 heartbeat 到期时才唤醒对应工作。
- 非目标：本文件不直接修改业务代码、不改变线上 daemon 发布通道、不在本阶段调整 message-service 或 user-service 协议。
- 完成标准：文档记录当前证据、轮询原因、事件化可行性、不可完全事件化的边界、分阶段实施方案、并行执行设计、验证标准和后续 Codex Goal 提示词。

## 2. awiki-plan 技能并行要求核对

已读取 `awiki-plan` 技能说明。当前技能已经明确要求所有 Plan 包含并行执行分析，并要求在步骤依赖、写入范围、契约和验证面互不冲突时尽量启动多个 Agent / Worker 并行处理。核心要求包括：

- 每个 Plan 必须包含并行执行章节，即使最终判断必须串行。
- 每个步骤都要标记 `parallel-safe`、可并行对象、互斥资源、写入范围和验证冲突。
- 当两个或更多步骤依赖独立、写集独立、契约边界清楚时，应计划启动多个 Codex agents 或 worker runs 并行工作。
- Coordinator 负责合并、Review、冲突处理、验证证据和执行台账。

结论：当前 `awiki-plan` 技能已经满足“创建规划时要求能够进行并行处理”的要求，本次文档按该要求设计并行 Wave。暂未修改技能文件本身。

## 3. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | 确认非平凡 AWiki 任务需要读 Harness、识别影响仓库、更新文档和报告验证。 |
| `awiki-harness/README.md` | 确认 Harness 是多仓库控制面，子仓库仍是实现权威来源。 |
| `awiki-harness/context/00-context-map.md` | 将任务路由到 Agent Runtime Host、Message Flow、Client Architecture、System Test。 |
| `awiki-harness/context/02-repo-map.md` | 确认 `awiki-cli-rs2/crates/awiki-deamon` 是终端 Agent Runtime Host，复用 `im-core`。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 确认 daemon、runtime、message-service、im-core 的边界和依赖方向。 |
| `awiki-harness/context/20-rules-index.md` | 定位文档、架构、AI 编码和验证规则。 |
| `awiki-harness/context/30-tools-env.md` | 记录 `awiki-cli-rs2`、`message-service`、`awiki-system-test` 常用验证入口。 |
| `awiki-harness/context/40-verification.md` | 确认本任务后续实现属于 L1 到 L3，最终需要系统测试证据。 |
| `awiki-harness/context/50-task-workflow.md` | 确认需要 context、analysis、solution plan、verification。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | 确认 daemon 是通用 ANP Agent Runtime Host，runtime 不直接持有 DID 私钥或直连 message-service。 |
| `awiki-harness/context/nodes/message-flow.node.md` | 确认消息流、WebSocket、reliable sync、`sync.delta` / `sync.thread_after` / realtime hint 边界。 |
| `awiki-harness/context/nodes/client-architecture.node.md` | 确认 realtime 对 App/SDK 暴露为高层事件流，可靠 checkpoint 只在 `im-core` Rust/SQLite 内部。 |

## 4. 当前运行态证据

运行态调查基于当前用户级 `awiki-deamon.service` 中的 `awiki-deamon foreground --state-root <daemon-state-root>` 进程。外部 daemon state root 不写入本计划的固定路径，后续执行时以实际环境为准。

| 证据 | 观测结果 | 解释 |
|---|---|---|
| 进程 CPU | PID `1275` 平均约 `6.9%`，主线程瞬时约 `4%` 到 `13%`。 | CPU 主要来自 foreground 主循环和少量 im-core local-state DB 线程。 |
| 线程状态 | 主线程曾处于 `D` 状态，系统 `%wa` 曾到 `4.8%` 到 `20%`。 | 说明除了计算，还有明显磁盘 I/O 等待。 |
| 5 秒 I/O 采样 | `syscr` 约 `1143/s`，`syscw` 约 `2482/s`，`wchar` 约 `4.06MB/s`，`write_bytes` 约 `5.64MB/s`。 | 静默时仍在做大量系统调用和写入，不是空闲等待。 |
| active agents | `agent-list` 当前有 8 个 active agent：7 个 runtime agent + 1 个 daemon agent。 | 每轮轮询工作按 agent 数量放大。 |
| 网络连接 | `ss -tpn` 显示 daemon 连接到 `awiki.info:443` 和本地代理端口。 | 静默时仍存在远端消息 / session / status 相关通信。 |
| 状态文件更新时间 | `<daemon-state-root>/identity/*/did.json`、`private.key`、`e2ee-agreement-private.pem`、`identity/registry.json`、`identity/default`、`im-core/local-state.sqlite-wal` 等文件持续更新。 | 代码存在每轮无条件重写 identity 文件的问题。 |
| 日志 | `journalctl --user -u awiki-deamon.service` 看到启动期多次 `daemon.runtime_inbox.session.failed`、heartbeat latest/control 失败。 | 失败路径不是当前持续 CPU 的唯一原因，但说明轮询路径会频繁触发远端 session / inbox / status 行为。 |
| DB 规模 | `agent_definition: 12`、`runtime_profile: 11`、`app_message_agent_binding: 1`、`message_sync_outbox: 24`、`runtime_final_outbox: 26`。 | 即使没有人工任务，daemon 仍有多个队列和 agent 状态需要调度。 |

## 5. 当前代码路径分析

| 模块 / 文件 | 当前行为 | 关键证据 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | `ForegroundOptions::new` 默认 `poll_interval_ms = 250`，foreground 主循环每 250ms 执行一次。 | `ForegroundOptions::new`、`run_foreground` 主循环、最后 `tokio::time::sleep(...)`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 每轮调用 `process_inbox_once`、两次 `flush_message_sync_outbox`、`drain_cli_route_message_queue_once`、`drain_runtime_retry_queue_once`、`flush_runtime_final_outbox`、`heartbeat.tick`。 | `run_foreground` 主循环。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | `process_inbox_once` 每轮读取所有 agent，逐个创建 client、ensure session，并轮询 direct 和 group。 | `state.list_agent_definitions()`、`client_for_agent_identity`、`ensure_agent_messaging_session`、`runtime_agent_inbox_poll_scopes()`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | direct inbox 通过 `messages().inbox_with_metadata_async(InboxQuery { scope: DirectOnly, limit: 20, ... })` 拉取。 | `process_agent_direct_inbox_once`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | group inbox 先 `groups().list_async(limit: 50)`，再对每个 active group 调 `groups().messages_async(limit: GROUP_CONTEXT_FETCH_LIMIT)`。 | `process_agent_group_inbox_once`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | `client_for_agent_identity` 每次创建 client 前都会调用 `sync_agent_identity_to_im_core`。 | `client_for_agent_identity`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | `sync_agent_identity_to_im_core` 无条件写 `did.json`、`private.key`、`e2ee-agreement-private.pem`、`auth.json`、`registry.json`、`default`。 | `sync_agent_identity_to_im_core`。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/agent_status.rs` | heartbeat 自身有节流：idle 5 分钟、active 30 秒、latest status 10 秒、release status 5 分钟。 | `IDLE_HEARTBEAT_MS`、`ACTIVE_HEARTBEAT_MS`、`LATEST_STATUS_CHECK_MS`。 |

结论：当前静默 CPU 不是“后台任务真的在跑”，而是 foreground 以 250ms 高频主动扫描所有 active agent、队列和状态，并且扫描过程中包含可避免的文件写入和远端请求。

## 6. 为什么当前是轮询模型

当前轮询模型不是没有理由，主要是工程上简单且覆盖面广：

- direct inbox、group inbox、outbox retry、runtime retry、final outbox、heartbeat、archive finalizer 可以用一个循环统一兜底。
- 对 HTTP-only、legacy message-service、message-service v2、WSS 不可用、断线重连、daemon 重启恢复等场景更容易保证“最终会再扫一次”。
- 旧消息监听能力、`im-core` realtime、可靠同步和 daemon runtime inbox 是分阶段演进的，早期用轮询能减少跨仓库协议依赖。
- 消息通知不能作为可靠 checkpoint。即使收到 WSS notification，也需要 `sync.delta` 或历史接口补齐丢失事件；轮询天然提供粗粒度补偿。

问题在于当前轮询把所有工作都绑到同一个 250ms 间隔，并且没有把“高频响应”和“低频兜底”分开，导致静默时也承担了活跃期成本。

## 7. 全异步 WSS 方案可行性判断

用户提出的目标是：异步监听所有 WSS，有新消息才行动，没有消息就等待。

结论：**direct/group runtime inbox 主链路可以朝这个方向做，但不能把全部工作变成“只有 WSS 才唤醒”。正确目标应是“WSS 事件触发 + 本地事件触发 + 到期 timer + 低频可靠兜底”，而不是完全删除所有定时任务。**

### 7.1 已具备的基础

| 能力 | 当前证据 | 对 daemon 的意义 |
|---|---|---|
| `im-core` realtime API | `awiki-cli-rs2/crates/im-core/src/realtime/service.rs` 提供 `start_async(options) -> RealtimeSession`。 | daemon 可以为每个 active agent 建立 async realtime session。 |
| realtime 事件流 | `awiki-cli-rs2/crates/im-core/src/realtime/session.rs` 提供 `RealtimeSession::subscribe()` 返回 `RealtimeEventStream`。 | foreground 可以 `select!` 等待 WSS 事件，而不是固定每 250ms 扫 inbox。 |
| 本地投影 | `awiki-cli-rs2/crates/im-core/src/realtime/runner.rs` 能把 `MessageReceived` / `GroupUpdated` 投影到 local-state。 | daemon 可复用 SDK 的 projection，减少自拼 WSS frame。 |
| message-service `/im/ws` | `message-service/crates/im-app/src/router.rs` 注册 `ws_path`，握手后按 authenticated DID 建 session。 | 服务端已有按 DID 建立 WSS session 的模型。 |
| session notify | `message-service/crates/im-runtime/src/session_registry.rs` 按 `agent_did` 投递 notification 给所有匹配 session。 | direct/group 新消息可按 agent DID 唤醒对应 daemon realtime task。 |
| reliable sync | `message-service/docs/api/ANP-client-server-api-sync.md` 定义 `sync.delta`、`sync.thread_after` 和 WebSocket 顶层 `sync` hint。 | WSS notification 应触发可靠同步或 targeted thread 补齐，而不是直接推进 checkpoint。 |

### 7.1.1 补充调查结论

| 结论 | 证据 | 对方案的影响 |
|---|---|---|
| daemon 当前不能直接启动 `im-core` realtime | `awiki-cli-rs2/crates/awiki-deamon/src/config.rs` 构造 `ImCoreConfig` 时当前使用 `MessageTransportPolicy::HttpOnly`；`awiki-cli-rs2/crates/im-core/src/realtime/service.rs` 在 `HttpOnly` 下返回 unsupported 或 `TransportUnavailable`。 | Step 04 必须先增加 daemon transport policy 配置或 `Auto` / `RealtimePreferred` 模式，并明确 WSS endpoint 来源。 |
| `RealtimeSession::subscribe()` 是本地事件流，不是服务端订阅协议 | `awiki-cli-rs2/crates/im-core/src/realtime/session.rs` 只返回本地 `mpsc::Receiver<ImEvent>`；当前没有 wire-level `subscribe` / `unsubscribe`。 | 不能把 `RealtimeOptions.subscriptions` 理解为服务端过滤契约；短期按 `/im/ws` 连接默认接收 authenticated DID 的通知处理。 |
| message-service v2 的 direct/group WSS 唤醒基础够用 | `message-service/crates/im-app/src/router.rs` 在 WS 握手后按 authenticated DID 注册 session；`message-service/crates/im-direct/src/service.rs`、`message-service/crates/im-group/src/handlers.rs` 会在 direct/group mutation 后 notify 对应 DID。 | runtime agent 自身 DID 的 direct/group 事件驱动可以先落地。 |
| legacy `molt-message` WSS 不适合作为长期 daemon 事件源 | legacy `/message/ws` 按 `user_id` 管理连接，推 `new_message`，缺少 DID/agent 精确订阅、账号级 `sync_events` 和 `sync.delta`。 | legacy 只作为兼容 fallback；长期优化应以 message-service v2 + `im-core` realtime 为主。 |
| 当前 local RPC worker 仍有 10ms 级轮询细节 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground/lifecycle_support.rs` 使用 nonblocking UDS accept + sleep。 | Step 03 可顺手改为 blocking/tokio UDS accept 或保留但降低频率；不要把它误认为完全无轮询。 |

### 7.2 可以事件化的工作

| 当前轮询项 | 事件化方案 | 是否可完全取消轮询 |
|---|---|---|
| runtime direct inbox | 每个 active runtime/daemon agent 建立 `im-core` realtime session，收到 `MessageReceived` 后进入 `route_message` 或调度 `sync.delta(reason = realtime_gap)` / `sync.thread_after`。 | 可以取消 250ms direct inbox 扫描，但保留启动、重连、低频 reconciliation。 |
| runtime group inbox | 收到 group message / group update 后只针对对应 group/thread 补齐上下文，不再每轮 `list groups + list messages`。 | 可以取消高频 group 全量扫描，但需要群成员列表缓存、group update 处理和低频兜底。 |
| local RPC callbacks | `start_runtime_rpc_worker` 已经基于 UDS accept 事件工作；RPC side effect 后应直接 notify outbox/queue scheduler。 | 可事件化，不需要 250ms 扫。 |
| message_sync_outbox | 写入 outbox 时发送 `tokio::Notify`；失败重试按 `next_attempt_at_ms` 用 `sleep_until` 唤醒。 | 可以取消固定扫，保留到期 timer 和启动恢复。 |
| runtime_final_outbox | runtime 结束写入 pending 时立即 notify flush；失败按 `next_attempt_at_ms` timer。 | 可以取消固定扫，保留到期 timer 和启动恢复。 |
| cli_route_message_queue | enqueue/claim 后 notify；future due item 用最早 `next_attempt_at_ms` timer。 | 可以取消固定扫，保留 due timer。 |
| runtime_retry_queue | pending retry 按 `next_attempt_at_ms` timer；新增 retry notify。 | 可以取消固定扫，保留 due timer。 |
| identity sync | 改为启动时同步、identity/token 变更时同步，或内容变化才写。 | 可以取消每次 client 创建时的无条件写。 |

### 7.3 不能只靠 WSS 的工作

| 工作 | 不能纯事件化的原因 | 建议 |
|---|---|---|
| reliable checkpoint | `RealtimeSyncHint` 只用于 duplicate/gap/dirty 判断和调度 `sync.delta`，不得直接推进 checkpoint。 | WSS 触发 `sync_delta`，启动 / reconnect / gap / 周期性 reconciliation 也触发 `sync_delta`。 |
| offline missed messages | daemon 断线或服务端 notification 丢失时，WSS 不会补发所有历史。 | reconnect 后必须跑一次 reliable sync 或 targeted inbox/history reconciliation。 |
| heartbeat / latest status | heartbeat 是状态报告，不由消息事件决定。 | 保留 timer，但按现有 10s/30s/5m 节流，不进入 250ms 主循环。 |
| archive finalizer / stale recovery | 这类任务依赖本地状态时间和启动恢复，不一定有外部事件。 | 启动时执行一次，之后低频 timer 或状态写入 notify。 |
| WSS auth/session refresh | WSS 长连接需要 auth session、reconnect、backoff、admission 失败处理。 | 建立 per-agent realtime supervisor，失败后指数退避。 |
| service multi-instance notification | message-service 当前 `SessionRegistry` 是进程内 session registry；多实例部署若消息写入进程与 WS 连接进程不同，单纯进程内 notify 可能漏推。 | 单实例可先落地；多实例需要 Redis Pub/Sub、PostgreSQL LISTEN/NOTIFY 或 NATS 等跨实例 notification bus，并保留 `sync.delta` 兜底。 |

### 7.4 推荐目标模型

```text
foreground supervisor
  -> agent registry watcher / reload timer
  -> per-agent realtime task pool
       -> im-core RealtimeSession per active agent DID
       -> MessageReceived / GroupUpdated / sync hint
       -> runtime message dispatcher
  -> local RPC worker
       -> runtime progress/final/msg.send side effects
       -> notify outbox schedulers
  -> queue schedulers
       -> message_sync_outbox due timer
       -> runtime_final_outbox due timer
       -> cli_route_message_queue due timer
       -> runtime_retry_queue due timer
  -> timed jobs
       -> heartbeat timer
       -> release/latest status timer
       -> low-frequency inbox reconciliation
       -> stale recovery timer
```

静默状态下主任务应大多阻塞在 `tokio::select!`：WSS event、local notify、timer due、shutdown signal、agent registry change。没有事件时不做 DB scan、不写 identity 文件、不发 HTTP/WSS RPC。

## 8. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| Agent Runtime Host / `awiki-cli-rs2` | 重构 foreground 调度模型、agent realtime sessions、runtime message dispatch、outbox scheduler。 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、`awiki-cli-rs2/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` |
| IM SDK / `im-core` | 复用 realtime session、sync delta、thread after、local projection；可能补 public API 或 daemon-friendly adapter。 | `awiki-cli-rs2/crates/im-core/src/realtime/*`、`awiki-cli-rs2/docs/api/im-core-interface/04-message-interface.md` |
| Message Flow / `message-service` | direct/group WSS 已有基础；realtime 通知需要与 reliable sync 组合使用。 | `message-service/crates/im-app/src/router.rs`、`message-service/docs/api/ANP-client-server-api-sync.md` |
| 本地状态 / SQLite | 减少无条件文件写和高频 DB 查询；新增 scheduler state 或 wakeup notify 可能需要状态字段。 | `awiki-cli-rs2/crates/awiki-deamon/src/state/*` |
| Auth / DID | per-agent WSS 需要 DID WBA session、session refresh 和 admission 失败处理。 | `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` |
| System Test | daemon 静默 CPU、WSS 唤醒、断线重连、offline 补齐、outbox retry、remote `awiki.info` 完整系统测试。 | `awiki-system-test` |

## 9. 假设与开放问题

### 假设

- message-service v2 `/im/ws` 在 `awiki.info` 环境可用，并且 DID WBA auth / admission 能覆盖 daemon runtime agent DID。
- `im-core` realtime `start_async` 可以在 daemon async runtime 中长期运行多个 agent session。
- direct/group notification 能携带足够 metadata 让 daemon 定位 message/thread；正文和 E2EE opaque 补齐仍通过 `sync.thread_after` 或现有 history/inbox path。

### 开放问题

- message-service v2 当前线上是否已为所有 agent DID 写入完整 `sync_events`，以及 direct/group notification 的顶层 `sync` hint 是否覆盖 daemon runtime inbox 需要的所有消息类型。
- daemon realtime endpoint 应从 `service_base_url` 还是 `message_service_base_url` 推导；当前 `im-core` realtime 以 SDK config 的 service base 推导 `/im/ws`，daemon 需要明确配置来源。
- `RealtimeOptions.subscriptions` 是否需要升级为 wire-level subscribe/filter 契约，还是维持当前“连接即隐式订阅 authenticated DID 通知”的模型。
- `awiki-deamon` 当前生产配置是否仍可能指向 legacy endpoint；如果存在 legacy-only 环境，需要保留 HTTP poll fallback。
- 多 active agent 建立多条 WSS 长连接时，服务端和本机资源目标上限是多少。

## 10. 总体设计方法

- 设计边界：daemon 仍复用 `im-core`，不直接拼 WSS frame、不让 runtime backend 直连 message-service、不把 checkpoint 暴露给 daemon 外部。
- 关键决策：将一个 250ms 全量主循环拆成 per-agent WSS、local Notify、due timer、low-frequency reconciliation 四类触发源。
- 兼容性策略：保留 `--poll-interval-ms` 和 HTTP poll fallback；WSS 不可用时进入低频退避轮询，而不是恢复 250ms 全量扫。
- 数据策略：identity 文件改为内容感知写入，队列按 `next_attempt_at_ms` 计算 timer，processed message 继续使用现有幂等表防重复执行。
- 协议策略：direct/group 先复用现有 message-service WSS + reliable sync，本计划不新增 message-service 协议。
- 风险控制：每一步都保留启动恢复、断线重连、low-frequency reconciliation 和系统测试证据。

## 11. 任务拆分

| Step | 标题 | 依赖 | 并行组 | Parallel-safe | 建议 Agent | 可并行对象 | 互斥资源 / 冲突路径 | 产出 | Commit gate | 合并 / 验证门禁 | 状态 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 | 基线观测与低风险止血 | 无 | 串行 | 否 | coordinator | 无 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、service 配置测试面 | CPU/I/O 基线、可配置 poll interval、观测日志 | 必须 | `cargo test -p awiki-deamon --locked` | pending |
| 02 | 消除无条件 identity 文件写 | Step 01 | A | 是 | agent-storage | Step 03 | `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | 内容感知写入或一次性同步 | 必须 | storage unit + daemon unit | pending |
| 03 | 本地队列调度器改为 Notify + due timer | Step 01 | A | 是 | agent-scheduler | Step 02 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` 局部调度段、queue state 方法 | outbox/retry/final/route queue 不再 250ms 扫 | 必须 | scheduler unit + retry tests | pending |
| 04 | runtime direct/group WSS realtime session | Step 02, Step 03 | 串行 | 否 | agent-realtime | 无 | `awiki-cli-rs2/crates/awiki-deamon/src/config.rs`、`awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、新增 realtime supervisor 模块 | per-agent WSS 唤醒 direct/group runtime message routing | 必须 | daemon realtime unit + im-core tests | pending |
| 05 | reliable sync / gap / fallback 协调 | Step 04 | 串行 | 否 | coordinator | 无 | daemon realtime supervisor、`im-core` sync API 调用面 | WSS hint 触发 `sync.delta` / `sync.thread_after`，重连兜底 | 必须 | WSS gap/reconnect tests | pending |
| 06 | 最终集成、文档同步、系统测试 | Step 02-05 | 串行 | 否 | coordinator | 无 | 全部已改模块、docs、Harness 相关摘要 | remote `awiki.info` 完整系统测试与最终 Review | 必须 | `awiki-system-test` remote full gate | pending |

## 12. 并行执行与多智能体分工

- 并行策略：先用 Step 01 建立基线和保护性配置；之后把“文件写入优化”和“本地队列调度器”拆为 Wave A 并行；再由 coordinator 串行推进 runtime WSS、reliable sync 集成和全局验证。
- 最大并行度：2。建议 Wave A 同时启动 2 个 worker。
- Coordinator：主执行者负责合并、Review、计划状态、执行台账、最终系统测试和文档同步。
- 串行原因：Step 04 和 Step 05 都会触碰 foreground/realtime 调度主路径，不能并行改同一控制流；Step 06 必须等待所有实现完成。

### Agent 分工

| Agent / Worker | 负责 Step | 责任边界 | 可修改路径 | 禁止修改路径 / 资源 | 交付物 | Review 责任 |
|---|---|---|---|---|---|---|
| coordinator | Step 01、05、06 | 基线、主调度集成、最终验证 | `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs`、最终 docs | 不覆盖并行 worker 未合并成果 | commit + 验证证据 | 全局 Review |
| agent-storage | Step 02 | identity 文件写入优化 | `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs`、对应 tests | foreground 主循环重构 | focused commit | coordinator review |
| agent-scheduler | Step 03 | local queue scheduler、Notify、due timer | 新增 scheduler 模块、queue drain 调用点、相关 state tests | realtime WSS 逻辑、identity sync | focused commit | coordinator review |
| agent-realtime | Step 04 | per-agent realtime supervisor 和 direct/group message routing | daemon realtime 模块、foreground 集成、realtime tests | queue scheduler、identity sync | focused commit | coordinator review |

### 并行组

| Wave / 并行组 | 可并行 Step | 可并行原因 | 共享依赖 | 写入范围 | 依赖屏障 | 合并顺序 | Group gate / 验证责任 |
|---|---|---|---|---|---|---|---|
| A | Step 02, Step 03 | 一个优化文件写入，一个优化本地队列 timer，契约相对独立。 | Step 01 基线 | storage 文件 vs scheduler/queue 文件 | 合并前确认没有同时改同一 foreground 段 | Step 02 -> Step 03 -> group daemon tests | `cargo test -p awiki-deamon --locked`，并记录 idle I/O 对比 |

### 互斥资源

| 资源 / 路径 / 契约 | 互斥原因 | 受影响 Step | 规则 |
|---|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` 主循环 | 调度控制流重构容易冲突 | Step 01、03、04、05 | 同一时间只能一个 worker 修改同一段，合并前 coordinator review。 |
| remote `awiki.info` 系统测试环境 | 同一环境验证会互相影响 | Step 06 | 最终 gate 串行执行并记录环境配置。 |

并行执行约束：

- 每个 Agent / Worker 只修改自己拥有的文件、模块或验证表面，不回退或覆盖其他 Agent 的修改。
- Agent / Worker 必须回报变更路径、命令、测试结果、阻塞、剩余风险和未触碰的外部所有权文件。
- 需要越界修改、改变并行组、改变合并顺序或发现互斥资源冲突时，先更新本 Plan 变更记录并重新评估 parallel-safe。
- Coordinator 必须在合并后检查组合 diff、冲突、Review 结论、步骤验证证据和整体验证证据。

## 13. 内嵌 Step 计划

### Step 01：基线观测与低风险止血

#### 目标

- 建立可重复的 idle CPU / I/O / 网络 / 日志观测方法。
- 确认 `--poll-interval-ms` 在 service 或 foreground 入口中可作为短期止血参数。
- 不改变消息语义，只降低后续优化风险。

#### 设计方法

- 先记录现状，再改配置或增加观测，不把调度模型一次性重写。
- 观测项覆盖 CPU、线程、procfs I/O、状态目录 mtime、WSS/HTTP 连接、audit/log。

#### 实现方法

1. 增加或整理 focused idle benchmark 脚本 / 文档，记录 60 秒平均 CPU、写入速率、active agent 数。
2. 若需要代码变更，优先让 foreground summary 或 debug log 输出 loop iteration、per-work item count、poll interval。
3. 验证 `--poll-interval-ms 2000/5000` 对 CPU 的影响，作为临时缓解方案。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 可选增加观测字段或日志 | 不改变调度语义。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/main.rs` | 核对 `--poll-interval-ms` 行为 | 当前已支持参数。 |
| `awiki-cli-rs2/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填实测基线 | 本文件。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| daemon unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | commit 前 | 测试通过或记录失败原因 | Step gate |
| idle baseline | `ps`、`top -H`、procfs I/O 采样、`journalctl --user -u awiki-deamon.service` | 改前和改后 | CPU/I/O 对比表 | Evidence |

#### Review 环节

- 检查是否只增加观测或低风险配置，不改变消息处理语义。
- 检查观测命令是否记录环境、active agent 数和时间窗口。
- 检查是否遗漏失败日志、网络连接和状态目录写入证据。

### Step 02：消除无条件 identity 文件写

#### 目标

- 消除 `client_for_agent_identity` 每轮无条件写文件造成的静默 I/O。
- 保持 identity/token 变更后仍能正确同步到 `im-core` identity registry。

#### 设计方法

- 使用内容感知写入：目标文件不存在或内容不同才写。
- identity registry 更新也应比较序列化后内容，避免相同 JSON 重写。
- 若缓存 identity sync 状态，缓存必须以 DID、token hash、did document hash、private key hash、e2ee key hash 为 key，并在进程重启后安全重建。

#### 实现方法

1. 新增 `write_if_changed(path, bytes, mode?)` helper，支持私钥权限设置。
2. 修改 `sync_agent_identity_to_im_core`，只在内容变化时写 `did.json`、`private.key`、`e2ee-agreement-private.pem`、`auth.json`、`registry.json`、`default`。
3. 增加 tests 覆盖“重复 sync 不更新 mtime / 不增加写入动作”和“内容变化会更新”。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/im_core_adapter.rs` | 内容感知 identity sync | 重点 I/O 降低点。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/*/tests.rs` | 增加 focused tests | 具体位置按现有测试结构选择。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| daemon unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | commit 前 | 测试通过 | Step gate |
| idle I/O | 重复运行 identity sync 相关 foreground 路径，比较 mtime / 写入计数 | commit 前 | 相同内容不重写 | Step gate |

#### Review 环节

- 检查私钥权限没有被内容感知写入绕过。
- 检查 token 缺失时 `auth.json` 兼容逻辑不变。
- 检查 registry 顺序稳定，避免序列化顺序导致误写。

### Step 03：本地队列调度器改为 Notify + due timer

#### 目标

- 将 `message_sync_outbox`、`runtime_final_outbox`、`cli_route_message_queue`、`runtime_retry_queue` 从 250ms 固定扫描改成“新增时 notify + 最早 due timer + 启动恢复”。

#### 设计方法

- 为每类队列建立 scheduler task：启动时查一次 due，之后等待 `Notify`、`sleep_until(next_due)`、shutdown。
- retry/backoff 仍使用现有 `next_attempt_at_ms` 字段，不改变表结构优先。
- 所有 enqueue/upsert 成功后通知对应 scheduler，避免等待下一次长 timer。

#### 实现方法

1. 抽象 `DueQueueScheduler` 或按队列分别实现小 scheduler，避免一次性大抽象。
2. 在 local RPC side effect、runtime finish、message sync outbox enqueue、retry scheduling 后触发 notify。
3. 启动时执行 stale recovery，再启动 scheduler。
4. 删除 foreground 主循环里每 250ms 对这些队列的固定 drain，保留显式 flush 函数供测试和启动恢复调用。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 移除固定队列 drain，接入 scheduler | 与 Step 04 互斥。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/local_rpc/*` | side effect 后 notify queue scheduler | local RPC 已是事件源。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/runtime/host.rs` | runtime final outbox enqueue 后 notify | 保持 final outbox 幂等。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| daemon unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | commit 前 | 现有 queue/retry tests 通过 | Step gate |
| scheduler tests | focused tests：未来 due 不立即跑、notify 后立即跑、retry due 到期跑 | commit 前 | 新增测试通过 | Step gate |

#### Review 环节

- 检查没有忙等或短周期 sleep。
- 检查 scheduler 退出、daemon shutdown、stale recovery 和 failed terminal 语义。
- 检查并行安全：不得覆盖 Step 02 对 identity sync 的写入优化。

### Step 04：runtime direct/group WSS realtime session

#### 目标

- 为 active runtime/daemon agent 建立 per-agent `im-core` realtime session，用 WSS notification 唤醒 direct/group runtime message routing。
- direct/group 不再依赖每 250ms 全量 inbox/group 扫描。
- 解除当前 daemon `HttpOnly` 配置对 realtime runner 的阻断，明确 realtime endpoint 和 fallback 策略。

#### 设计方法

- 新增 daemon realtime supervisor，按 active agent definition 管理 task 生命周期。
- daemon `ImCoreConfig` 需要支持非 `HttpOnly` 的 transport policy，优先设计为 `Auto` / `RealtimePreferred`，WSS 不可用时自动降级到低频 poll fallback。
- 明确 `service_base_url` / `message_service_base_url` 与 `/im/ws` 的推导关系，避免 runtime agent 连接到错误服务。
- 每个 active agent 的 realtime task 必须在启动时调用一次 `im_core.client_for_agent_identity` 创建 `ImClient`，并在 task 生命周期内复用这个 `ImClient` 处理 realtime session、message dispatch、sync/fallback。
- 不允许在 task 的 event loop、reconnect loop、每个 WSS event 或 fallback tick 内重复调用 `client_for_agent_identity`；只有 agent identity/token/hash 变化、agent inactive/delete、关键配置变化或 task 重建时才允许重新创建 `ImClient`。
- 事件流收到 `MessageReceived` 或 `GroupUpdated` 后只处理相关 message/thread/group，不再扫全部 agent 和全部 group。
- 保留 low-frequency reconciliation：启动、reconnect、gap、定期执行一次轻量 sync/poll。

#### 实现方法

1. 新增 `runtime_realtime` 模块，封装 per-agent session start/stop/restart/backoff。
2. 将 `process_runtime_inbox_message` 或 `route_message` 提取为可被 realtime event 调用的 dispatcher。
3. 为 `runtime_realtime` 设计 per-agent task state，至少包含 `agent_did`、`identity_hash`、`token_hash`、`ImClient`、realtime session handle、backoff/reconnect 状态和 shutdown handle。
4. supervisor 监听或周期性 reconciliation active agent registry：新增 active agent 时启动 task；identity/token/hash 变化时停止旧 task 并重建 `ImClient` 与 realtime session；agent inactive/delete 时停止 task 并释放 `ImClient`。
5. direct event：从 event message 构造现有 `Message` 路径，走 controller check、processed message 幂等和 runtime routing。
6. group event：只对 event 所属 group/thread 拉必要上下文，避免 `groups().list_async` + 全 group messages。
7. WSS 不可用或 transport policy `HttpOnly` 时进入低频 fallback poll，并记录 audit/diagnostics；fallback 也必须复用 task 持有的 `ImClient`，不得退回 250ms 全量创建 client。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/config.rs` | 增加或调整 transport policy 配置 | 当前 `HttpOnly` 会阻止 realtime。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 接入 realtime supervisor，移除 runtime inbox 高频轮询 | 关键控制流。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/runtime_inbox.rs` | 可复用 message dispatch 类型或 helper | 若需要拆分。 |
| `awiki-cli-rs2/crates/im-core/src/realtime/*` | 只在 public API 不足时补 adapter | 优先复用现有 API。 |
| `awiki-cli-rs2/crates/awiki-deamon/src/agent_status.rs` | 增加 realtime session diagnostics | 可选。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| daemon unit | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | commit 前 | 测试通过 | Step gate |
| im-core unit | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | commit 前 | realtime 相关测试通过 | Step gate |
| client lifecycle tests | focused tests 或诊断日志：同一 agent 在 task 生命周期内只创建一次 `ImClient`，identity/token 变化后才重建 | commit 前 | `client_for_agent_identity` 调用次数符合 task 启动 / 重建次数，不随 WSS event 或 fallback tick 增长 | Step gate |
| integration smoke | 本地或 remote 测试：发送 direct/group command 给 runtime agent | group gate | 收到 WSS 后执行，无 250ms 全量轮询 | Group gate |

#### Review 环节

- 检查 runtime backend 仍不持有 DID 私钥、不直连 message-service。
- 检查 controller DID 授权、processed message 幂等、group agent 跳过逻辑保持。
- 检查 WSS reconnect/backoff 不会形成重连风暴。
- 检查 `ImClient` 生命周期与 per-agent task 生命周期绑定，event loop 和 fallback loop 没有重复创建 client。

### Step 05：reliable sync / gap / fallback 协调

#### 目标

- 将 WSS notification 与 `sync.delta` / `sync.thread_after` 正确组合，确保低延迟和可靠性同时满足。
- 明确启动、reconnect、gap、periodic reconciliation 的行为。

#### 设计方法

- 收到 realtime `sync` hint 时，只做 gap/dirty 判断和调度；不得推进 checkpoint。
- `sync.delta` 的 checkpoint 由 `im-core` 内部 SQLite transaction 处理，daemon 不暴露或手动写 checkpoint。
- 当 WSS event 已带可执行 message 时可先低延迟处理，但仍需要幂等和后续 sync/reconciliation 保证一致。

#### 实现方法

1. 在 realtime supervisor 中识别 `RealtimeSyncHint`。
2. gap 或 reconnect 后调用 `messages().sync_delta(reason = "realtime_gap" / "reconnect")`。
3. 对 dirty thread 调 `sync_thread_after` 或现有 history path 补齐正文和 group context。
4. 设定低频 reconciliation 周期，例如 5 到 15 分钟，可配置且带抖动。
5. WSS 长期失败时进入退避 poll fallback，例如 30 秒、60 秒、5 分钟，而不是 250ms。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 接入 sync/fallback coordinator | Step 04 之后。 |
| `awiki-cli-rs2/crates/im-core/src/messages/*` | 如需 daemon-friendly async sync API，补充最小 API | 优先复用现有 `sync_delta`。 |
| `awiki-cli-rs2/docs/api/im-core-interface/04-message-interface.md` | 若 API 行为变化，更新文档 | checkpoint 边界必须保持。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| im-core sync tests | `cd awiki-cli-rs2 && cargo test -p im-core --locked sync` | commit 前 | sync 相关测试通过 | Step gate |
| daemon reconnect tests | focused tests：WSS disconnect -> reconnect -> sync delta | commit 前 | 不丢消息，不重复执行 | Step gate |
| message-service tests | `cd message-service && cargo test --workspace` | 仅当实现过程中实际修改 message-service 契约 | 通过或记录失败 | L2 gate |

#### Review 环节

- 检查没有把 realtime hint 当作可靠 checkpoint。
- 检查 snapshot_required / retention gap fail-closed。
- 检查 fallback poll 有 backoff 和上限，不会回到 250ms 全量扫。

### Step 06：最终集成、文档同步、系统测试

#### 目标

- 合并所有步骤，执行全局 Review 和 remote `awiki.info` 系统测试，证明 idle CPU/I/O 降低且消息链路不回归。

#### 设计方法

- 最终集成只做兼容修复、文档同步和验证，不再引入大范围新设计。
- 记录 idle 观测前后对比、WSS 唤醒证据、fallback/reconnect 证据、system test 结果。

#### 实现方法

1. 执行组合 diff Review，检查并行 worker 是否越界修改。
2. 更新 `awiki-cli-rs2` daemon docs 和必要 Harness docs。
3. 运行 repo tests 和完整 remote 系统测试；只有实际发生跨仓库协议改动时才运行对应服务端测试。
4. 记录命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。

#### 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/docs/*` | 更新 daemon runtime host / local-dev 说明 | 行为变化需要文档同步。 |
| `awiki-harness/context/*` | 如跨服务契约变化，更新摘要 | 只写摘要和链接。 |
| `awiki-system-test` | 执行 remote 系统测试 | 不在本计划中预设具体测试文件。 |

#### 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| daemon tests | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | final 前 | 通过 | Final gate |
| workspace tests | `cd awiki-cli-rs2 && cargo test --workspace --locked` | final 前 | 通过或记录不可运行原因 | Final gate |
| remote system test | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote AWIKI_BASE_URL=https://awiki.info uv run python manage_local_test_env.py run-tests` | final | 记录通过/失败/跳过数量、原因和环境 | Final system gate |
| idle measurement | 优化后相同 agent 数、相同时长采样 CPU/I/O | final | 静默 CPU 和写入速率显著下降 | Final evidence |

#### Review 环节

- 检查 direct/group runtime message、runtime final、local RPC、status heartbeat 全链路。
- 检查安全：DID 私钥、runtime_rpc_token、E2EE 明文、checkpoint 边界不泄露。
- 检查文档和 Harness 是否同步。
- 检查所有 Step commit 都已完成，最终工作区无意外未提交变更。

## 14. 验收标准

- [ ] 静默 CPU：在 active agent 数相同、无人工任务的 60 秒窗口内，`awiki-deamon` 平均 CPU 明显低于当前约 `6% - 7%`，目标优先设为低于 `1% - 2%`。
- [ ] 静默 I/O：相同 60 秒窗口内，不再出现 identity 文件持续重写；`write_bytes` 速率相较当前约 `5.64MB/s` 明显下降。
- [ ] direct runtime message：远端 direct command 通过 WSS 或 reliable sync 唤醒 daemon 并执行，仍保留 controller DID 校验和 processed message 幂等。
- [ ] group runtime message：群消息不再每轮全量扫所有 group；只对相关 group/thread 补齐上下文。
- [ ] `ImClient` 生命周期：同一个 active agent 的 realtime task 在生命周期内复用同一个 `ImClient`；`client_for_agent_identity` 不随 250ms tick、WSS event 或 fallback tick 重复调用，只在 task 启动、identity/token/hash 变化、agent 状态变化或 task 重建时调用。
- [ ] WSS 断线重连：断线、重连、gap、snapshot_required 都有明确行为和测试。
- [ ] outbox/retry/final queue：新增项能及时触发，未来 due 项按 timer 触发，失败 retry 不忙等。
- [ ] heartbeat：仍按原有节流发送，不被消息事件化重构破坏。
- [ ] final remote 系统测试在 `awiki-system-test` 下使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名执行，并记录实际结果。

## 15. Review 策略

- 每步骤 Review：优先检查 correctness、回归、消息幂等、授权、失败路径、测试覆盖和 idle 观测证据。
- 并行组 Review：检查路径所有权、是否越界修改、是否产生共享契约冲突、是否需要重新评估 `parallel-safe`。
- 合并后 Review：重点看 foreground supervisor、scheduler、realtime task、shutdown/restart/backoff 是否组合正确。
- 契约 / 安全 / 隐私 Review：检查 DID WBA session、runtime_rpc_token、E2EE opaque、sync checkpoint 边界。
- 文档 Review：检查 `awiki-cli-rs2` daemon docs、message-service API docs、Harness 摘要是否与实现一致。

## 16. 验证策略

| 层级 | 适用 Step / 并行组 | 命令 / 检查 | 运行时机 | 预期证据 | 门禁结果 |
|---|---|---|---|---|---|
| Step Unit | Step 01-06 | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked` | 每步 commit 前 | 通过或记录原因 | pending |
| SDK Unit | Step 04-05 | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | realtime/sync 改动后 | 通过 | pending |
| Cross Repo | Step 06 | `cd message-service && cargo test --workspace` | 仅当实现过程中实际修改 message-service 后 | 通过或记录跳过原因 | pending |
| Idle Evidence | Step 01、02、03、06 | 60 秒 CPU/I/O/mtime 采样 | 改前、关键改动后、final | 对比表 | pending |
| Final System | Step 06 | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote AWIKI_BASE_URL=https://awiki.info uv run python manage_local_test_env.py run-tests` | 所有步骤完成后 | 通过/失败/跳过数量、原因、环境 | pending |
| Docs | 全部 | Markdown 路径存在检查、Harness docs 检查 | final 前 | 无失效链接或记录原因 | pending |

## 17. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | Agent / Owner | 并行组 | 分支 / worktree | 基线 commit | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 合并状态 | 门禁状态 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 | pending | coordinator | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 建立基线 |
| 02 | pending | agent-storage | A | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 01 |
| 03 | pending | agent-scheduler | A | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 01 |
| 04 | pending | agent-realtime | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Wave A |
| 05 | pending | coordinator | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 04 |
| 06 | pending | coordinator | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 02-05 |

## 18. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、执行台账、当前 `git status --short --branch`。
- 默认同一时间只执行一个步骤；只有任务拆分表和“并行执行与多智能体分工”同时标记为 parallel-safe 的步骤，才启动多个 Agent / Worker 并行处理对应 Wave。
- 并行执行时，Coordinator 必须分配清晰的文件 / 模块 / 验证所有权，要求每个 Agent / Worker 不回退或覆盖他人修改，并在合并前收集变更路径、命令、测试结果、阻塞和剩余风险。
- 每个步骤依次执行或在 parallel-safe Wave 内并行执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 并行 Wave 结束后，Coordinator 必须执行组合 diff Review、冲突检查、必要的集成验证和执行台账回填。
- 改变范围、顺序、验收标准、公开契约、数据模型或验证策略前，先更新本 Plan。

## 19. Codex Goal 提示词

```text
请以 awiki-cli-rs2/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md 为唯一规划入口，按文档执行完整优化。

开始前先读取该 Plan 的执行台账、并行执行与多智能体分工、Step 计划、验证策略、Blocked 处理和 Plan 变更记录，并运行 git status --short --branch。

请从第一个状态不是 done 的步骤开始。默认一次只执行一个步骤；只有主 Plan 明确标记为 parallel-safe 的 Wave A，才尽量启动多个 Agent / Worker，并为每个 Agent / Worker 分配清晰的文件、模块或验证所有权。并行执行前确认隔离方式、路径范围、互斥资源、合并顺序和 group gate。

每步都要按对应 Step 计划实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填执行台账。并行 Wave 完成后，由 Coordinator 做组合 diff Review、冲突检查、必要的集成验证和证据归档。

核心注意点：不要让 runtime backend 持有 DID 私钥或直连 message-service；每个 active agent 的 realtime task 必须长期持有并复用同一个 ImClient，不能在 event loop/fallback tick 中重复创建；不要把 realtime sync hint 当作可靠 checkpoint；本计划不新增 message-service 协议；最终集成必须在 awiki-system-test 使用 AWIKI_SYSTEM_TEST_MODE=remote 和 awiki.info 域名执行完整系统测试并记录结果。
```

## 20. Blocked 处理

| Blocker | Step | Agent | 并行组 | 证据 | 已尝试方案 | 影响范围 | 是否暂停同组 | 下一步决策 |
|---|---|---|---|---|---|---|---|---|
| message-service WSS 不支持目标 agent DID | Step 04 | agent-realtime | 串行 | WSS auth/admission 失败日志 | 检查 DID WBA session、admission、transport policy | Step 04 / Step 05 | 是 | 启用低频 poll fallback 并记录协议缺口 |
| remote system test 环境不可用 | Step 06 | coordinator | 串行 | `awiki-system-test` 命令失败 | 重试、记录环境、检查 awiki.info | 整体计划 | 是 | 向用户报告并保留未通过风险 |

只有依赖允许且风险已记录时，才继续另一个 pending 步骤。如果 blocker 影响共享契约、共享路径、合并顺序或 group gate，必须暂停同组相关步骤的合并。只有没有安全假设、回退方案或独立下一步时，才询问用户。

## 21. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-28 | 创建初版 Plan | 沉淀 idle CPU 调查和事件驱动优化方向 | 全部 | 是 |

## 22. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| WSS notification 丢失导致命令不执行 | `sync.delta` / `sync.thread_after` / low-frequency reconciliation | 恢复低频 poll fallback，不恢复 250ms 全量扫。 |
| 多 agent WSS 连接造成服务端压力 | session supervisor 限流、指数退避、连接数指标 | 配置回退到低频 poll 或限制 active agent realtime。 |
| per-agent task 中重复创建 `ImClient`，导致轮询成本残留 | lifecycle tests、诊断计数、Review 检查 `client_for_agent_identity` 调用点 | 回退到显式 per-agent client cache，按 identity/token/hash 失效。 |
| identity 内容感知写入漏更新 | hash/mtime tests、token/did change tests | 回退到启动时强制 sync + 内容感知运行时 sync。 |
| queue scheduler 漏唤醒 | 启动恢复、due timer、notify tests | 临时增加低频 queue reconciliation。 |
| heartbeat/status 回归 | 保留现有节流常量和 dedicated timer tests | 回退 heartbeat timer 改动。 |

## 23. 最终全局 Review 与整体验证

- 触发条件：所有步骤完成、Review、验证并提交后执行。
- Review 范围：`awiki-cli-rs2` daemon foreground/realtime/scheduler/storage、`im-core` realtime/sync API、Harness 和子仓库 docs、执行台账、系统测试证据。
- 重点关注：静默 CPU/I/O 是否下降、direct/group runtime command 是否仍执行、per-agent `ImClient` 是否按 task 生命周期复用、WSS 断线重连和 gap 是否可靠、checkpoint 边界、安全/隐私、并行组合并冲突。
- 并行执行审计：确认 Wave A 每个 worker 只修改授权路径；所有越界变更均已更新 Plan；group verification gate 已通过。
- 整体验证命令 / 检查：见第 16 节。
- Review 发现：TBD。
- 已修复问题：TBD。
- 剩余风险：TBD。
- 最终证据：TBD。
- 最终 `git status`：TBD。
- 如果本阶段修改文件：记录 Review、验证和最终集成 commit。
