# Plan：awiki-deamon 静默 CPU 事件驱动改造

状态：in_progress  
DOC：`awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md`  
Harness：`awiki-harness`  
创建时间：2026-06-28  
恢复指针：Step 01 已完成基线采样和验证；恢复时从 Step 02 / Step 03 / Step 04 的并行 Wave 前置检查开始，并读取本文件、当前 Step 文档、执行台账和 `git status --short --branch`。

## 1. 目标

- 任务目标：将 `awiki-deamon foreground` 从 250ms 高频主动扫描模型改造成“远端 WebSocket 事件 + 本地 Notify + due timer + 低频可靠兜底”的事件驱动模型，降低静默 CPU / I/O，并保持 direct/group runtime message、local RPC、outbox、heartbeat、reconnect 和系统测试行为不回归。
- 预期行为：没有远端消息、本地 runtime 回调、outbox 到期任务、heartbeat 到期任务或 shutdown 信号时，daemon 主要阻塞等待，不持续扫描所有 active agent、所有 group 和所有本地队列；有事件时只处理对应 agent / queue / thread / group。
- 非目标：不在本计划内重写 message-service 协议；不让 runtime backend 持有 DID 私钥或直连 message-service；不把 reliable checkpoint 暴露给 daemon；不把 `im-core` 改成 daemon 专用 SDK；不把 legacy `molt-message` WSS 作为长期主链路。
- 完成标准：完成所有 Step 计划、每步 Review / 验证 / 聚焦 commit、最终全局 Review、`awiki-system-test` remote `awiki.info` 完整系统测试证据、idle CPU/I/O 对比证据、文档同步和执行台账回填。

## 2. Context Pack

### 2.1 Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | 确认非平凡 AWiki 任务需要读 Harness、识别影响仓库、更新文档和报告验证。 |
| `awiki-harness/README.md` | 确认 Harness 是多仓库控制面，子仓库仍是实现权威来源。 |
| `awiki-harness/harness-control-plane-plan.md` | 确认需求应先进入 context / analysis / plan，再实施，并维护验证证据。 |
| `awiki-harness/context/00-context-map.md` | 将任务路由到 Agent Runtime Host、Message Flow、Client Architecture、System Test。 |
| `awiki-harness/context/02-repo-map.md` | 确认 `awiki-cli-rs2-cpu/crates/awiki-deamon` 是终端 Agent Runtime Host，复用 `im-core`。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 确认 daemon、runtime、message-service、im-core、awiki-system-test 的边界和依赖方向。 |
| `awiki-harness/context/20-rules-index.md` | 定位架构、AI 编码和验证规则。 |
| `awiki-harness/context/30-tools-env.md` | 记录 `awiki-cli-rs2-cpu`、`message-service`、`awiki-system-test` 常用验证入口。 |
| `awiki-harness/context/40-verification.md` | 确认本任务实现属于 L1 到 L3，最终需要系统测试证据。 |
| `awiki-harness/context/50-task-workflow.md` | 确认需要 context、analysis、solution plan、verification。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | 确认 daemon 是通用 ANP Agent Runtime Host，runtime 不直接持有 DID 私钥或直连 message-service。 |
| `awiki-harness/context/nodes/message-flow.node.md` | 确认 WebSocket notification 不能推进 checkpoint，可靠同步必须经 `sync.delta` / `sync.thread_after`。 |
| `awiki-harness/context/nodes/client-architecture.node.md` | 确认 realtime 对 App / SDK 暴露为高层事件流，checkpoint 只属于 `im-core` Rust/SQLite。 |
| `awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | 确认 `im-core`、`awiki-cli`、`awiki-deamon`、`im-core-dart` 边界和验证入口。 |
| `awiki-harness/context/repo-profiles/message-service.md` | 确认 v2 WSS、sync、thread-after 和系统测试路径。 |
| `awiki-harness/rules/architecture-principles.md` | 确认 public API、身份、消息、安全边界变更需要兼容性 Review。 |
| `awiki-harness/rules/verification-policy.md` | 确认最终报告必须记录命令、结果、未运行项和剩余风险。 |

### 2.2 子仓库与源码上下文

| 来源 | 作用 |
|---|---|
| `awiki-cli-rs2-cpu/AGENTS.md` | 确认本仓库规划文档必须中文；最终系统测试必须在 `awiki-system-test` remote `awiki.info` 模式执行并记录证据。 |
| `awiki-cli-rs2-cpu/README.md` | 确认当前 Rust CLI / SDK / daemon 仓库布局和基本命令。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/local-dev.md` | 确认 daemon 与 CLI 平行、复用 `im-core`，状态目录、local RPC、安全模型和验证入口。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` | 确认 daemon runtime host、Runtime Agent DID、controller DID、runtime plugin、local RPC 和 payload command 边界。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` | 当前 250ms 主循环、runtime inbox poll、queue drain、heartbeat、routing 主入口。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs` | 当前 `client_for_agent_identity` 每次创建 client 前无条件同步 identity 文件。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/config.rs` | 当前 daemon `ImCoreConfig` 固定 `MessageTransportPolicy::HttpOnly`，阻止 realtime runner。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/state/*` | queue due 字段、状态迁移、processed message 幂等和 local state API。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime/host.rs` | runtime final outbox、retry queue、generic-cli busy retry 和 flush 函数。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/inbox/user_delegated.rs` | message sync outbox、user delegated inbox 和 flush 逻辑。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*` | 现有 `RealtimeSession`、`RealtimeEventStream`、`RealtimeSyncHint`、projection、stop/join/status API。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/messages/service.rs` | 现有 `sync_delta_async`、`sync_thread_after_async` 和 message history / inbox API。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/groups/service.rs` | 现有 `groups().messages_async` 可用于 group 事件上下文补齐。 |

### 2.3 当前运行态证据

| 证据 | 观测结果 | 解释 |
|---|---|---|
| 进程 CPU | 既有调查记录显示 `awiki-deamon` 静默平均约 `6% - 7%`。 | CPU 主要来自 foreground 主循环、HTTP/WSS/session 行为和 local-state DB 线程。 |
| I/O 采样 | 既有调查记录显示 `write_bytes` 约 `5.64MB/s`，`wchar` 约 `4.06MB/s`。 | 静默时仍在做大量写入，不是空闲等待。 |
| active agents | 既有调查记录显示 active agent 数约 8 个。 | 高频扫描成本按 agent 数量放大。 |
| 状态文件 mtime | identity 文件、registry、default、`im-core/local-state.sqlite-wal` 持续更新。 | `sync_agent_identity_to_im_core` 无条件写文件是静默 I/O 主要来源之一。 |
| foreground 代码 | `ForegroundOptions::new` 默认 `poll_interval_ms = 250`，主循环每轮处理 inbox、outbox、queue、heartbeat 后 sleep。 | 所有工作绑定到同一个短间隔。 |

## 3. 当前代码状态与关键判断

### 3.1 250ms 主循环问题

`awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` 当前每轮执行：

- `process_inbox_once`：读取所有 agent，逐个创建 `ImClient`、ensure session、poll direct / group。
- `process_user_delegated_inbox_once`：处理 user delegated inbox。
- `flush_message_sync_outbox`：发送 message sync outbox。
- `drain_cli_route_message_queue_once`：处理 CLI route queue。
- `drain_runtime_retry_queue_once`：处理 runtime retry queue。
- `flush_runtime_final_outbox`：发送 runtime final outbox。
- `HeartbeatScheduler::tick`：按内部节流发送 heartbeat / latest / release status。
- `tokio::time::sleep(Duration::from_millis(options.poll_interval_ms))`。

结论：静默 CPU 不是单个功能忙，而是多个本应事件化或按 due timer 唤醒的工作被绑到 250ms 全量循环。

### 3.2 M-Core API 能力判断

当前 `im-core` 基本满足第一版事件驱动改造：

- `RealtimeService::start_async(RealtimeOptions) -> RealtimeSession` 可以启动单个 agent DID 的 realtime session。
- `RealtimeSession::subscribe() -> RealtimeEventStream` 可以拿到单 session 的 `tokio::mpsc::Receiver<ImEvent>`。
- `RealtimeSession::status_updates()`、`stop()`、`join()` 可以支持生命周期管理。
- `ImEvent::MessageReceived` 已包含 `im_core::messages::Message`，direct / group incoming 都可以投影为 message event。
- `RealtimeSyncHint` 可以作为 dirty / gap 调度信号，但不能推进 checkpoint。
- `messages().sync_delta_async(...)` 和 `messages().sync_thread_after_async(...)` 已存在。
- `groups().messages_async(...)` 可按 group 拉少量上下文，替代全 group 扫描。

第一版不需要新增或破坏 `im-core` public API。唯一需要明确的潜在缺口是 realtime endpoint 选择：当前 `im-core` realtime endpoint 以 `service_base_url` 推导 `/im/ws`；如果部署中 `message_service_base_url` 与 `service_base_url` 不同，可能需要独立兼容性评审后修改 `im-core` 内部 endpoint 选择或新增向后兼容 helper。

### 3.3 多 WebSocket 统一事件判断

现有 API 没有内置“多 WebSocket multiplexer”，但上层 daemon 可以建立：

```text
per-agent RealtimeSession task
  -> read RealtimeEventStream
  -> wrap DaemonRealtimeEvent { source, event }
  -> central tokio::mpsc channel
  -> RuntimeRealtimeSupervisor coordinator
  -> route / sync / fallback / audit
```

`RealtimeSession::subscribe()` 只能 attach 一个 reader，所以 fan-in / fan-out 应在 daemon 层完成，而不是让多个业务模块直接读同一个 session。source metadata（`agent_did`、`session_id`、`endpoint_kind`）属于 daemon 语义，不应塞进 `im-core::ImEvent`。

## 4. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| Agent Runtime Host / `awiki-cli-rs2-cpu` | 重构 foreground 调度、queue scheduler、per-agent realtime supervisor、runtime message dispatcher。 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs`、`awiki-cli-rs2-cpu/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` |
| IM SDK / `im-core` | 默认只读复用 realtime、sync、thread-after、projection；如必须调整 endpoint 选择，先独立评审。 | `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*`、`awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` |
| Message Flow / `message-service` | 不改协议；依赖 v2 `/im/ws` direct/group notification 与 `sync.delta` / `sync.thread_after`。 | `message-service/docs/api/ANP-client-server-api-sync.md`、`message-service/docs/api/` |
| 本地状态 / SQLite | 复用现有 due 字段和 processed message 幂等；如需最近 due 查询 helper，优先只在 daemon state 层新增读方法。 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/state/*` |
| Auth / DID / Secret | per-agent WSS 使用 agent DID token；identity 私钥仍只在 daemon 本地；日志/audit 不泄露 token 或私钥。 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs`、`awiki-cli-rs2-cpu/crates/awiki-deamon/docs/local-dev.md` |
| Local RPC / Runtime Plugin | RPC side effect 后 notify queue scheduler；runtime backend 仍只能通过 daemon local RPC 回传。 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/local_rpc/*`、`awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime/*` |
| System Test | 最终需要 remote `awiki.info` 完整系统测试和 idle CPU/I/O 证据。 | `awiki-system-test` |

## 5. 假设与开放问题

### 假设

- `message-service` v2 `/im/ws` 在 `awiki.info` 环境可用，并且 DID WBA auth / admission 能覆盖 daemon runtime agent DID。
- `im-core` realtime `start_async` 可以在 daemon Tokio runtime 中长期运行多个 agent session。
- direct/group notification 能携带足够 metadata 让 daemon 定位 message/thread；正文和 E2EE opaque 补齐仍通过 `sync.thread_after`、`groups().messages_async` 或现有 history path。
- 当前任务允许在 `awiki-deamon` 内新增模块、tests 和文档；不要求一次性删除所有低频兜底。

### 开放问题

- `awiki.info` 上 `service_base_url` 与 `message_service_base_url` 是否总是相同；如果不同，`im-core` realtime endpoint 是否应优先使用 `message_service_endpoint`。
- message-service v2 当前线上是否为所有 agent DID 写入完整 `sync_events`，direct/group notification 的 `sync` hint 是否覆盖 daemon runtime inbox 需要的全部消息类型。
- 多 active agent 建多条 WSS 长连接时，服务端和本机资源目标上限是多少；是否需要首版 session 数上限配置。
- user delegated inbox 是否在本次纳入 realtime 事件化，还是保留低频 reconciliation；本计划首版保守处理为低频兜底，不把它绑定到 250ms。

## 6. 总体设计方法

- 设计边界：daemon 仍复用 `im-core`，不直接拼 WSS frame、不让 runtime backend 直连 message-service、不把 checkpoint 暴露给 daemon 外部。
- 共享底层约束：`im-core` 是 `awiki-cli`、`awiki-deamon`、`im-core-dart` 共享 SDK；本计划默认不破坏或顺手新增 public API / DTO / feature gate / transport 默认语义。必须修改共享接口时，先进入 Step 04 的独立兼容性评审。
- 多 WebSocket 设计：每个 agent DID 使用自己的 `ImClient` 和 `RealtimeSession`；daemon 层通过 `DaemonRealtimeEvent { source, event }` fan-in 到统一 channel，由 coordinator 串行处理路由、sync 和 fallback 决策。
- 队列设计：message sync outbox、runtime final outbox、cli route queue、runtime retry queue 使用 `Notify + next_attempt_at_ms sleep_until + 启动恢复 + 低频 reconciliation`，不再靠 250ms 固定扫。
- identity 设计：`sync_agent_identity_to_im_core` 改为内容感知写入；per-agent realtime task 生命周期内复用一个 `ImClient`，避免 event loop / fallback tick 重复创建 client。
- reliable sync 设计：realtime hint 只调度 `sync_delta_async` / `sync_thread_after_async`；checkpoint 由 `im-core` 内部事务推进；daemon 只看结果和错误。
- fallback 设计：WSS 不可用、auth 失败、gap、reconnect、snapshot_required、未知 notification 时进入受控低频 fallback；不回到 250ms 全量扫。
- 观测设计：每步都记录 idle CPU/I/O、active agent 数、session 数、queue due、WSS 连接状态、fallback 原因和关键 audit。

## 7. 任务拆分

| Step | 标题 | 依赖 | 并行组 | Parallel-safe | 建议 Agent | 可并行对象 | 互斥资源 / 冲突路径 | 产出 | 小 Plan 文档 | Commit gate | 合并 / 验证门禁 | 状态 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 | 基线观测与调度保护 | 无 | 串行 | 否 | coordinator | 无 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs`、运行态服务 | 可重复 idle CPU/I/O 采样、诊断字段、当前行为保护测试 | [steps/01-baseline-observability.md](steps/01-baseline-observability.md) | 必须 | `cargo test -p awiki-deamon --locked` | done |
| 02 | 内容感知 identity sync | Step 01 | A | 是 | agent-storage | Step 03 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs` | 相同 identity/token 不重写文件，mtime/write 降低 | [steps/02-identity-sync-write-if-changed.md](steps/02-identity-sync-write-if-changed.md) | 必须 | storage tests + daemon unit | pending |
| 03 | 本地 due queue scheduler | Step 01 | A | 是 | agent-scheduler | Step 02 | queue scheduler 模块、queue state helper、局部 foreground 接入 | 四类本地队列从固定扫描改为 Notify + due timer | [steps/03-local-queue-schedulers.md](steps/03-local-queue-schedulers.md) | 必须 | scheduler tests + group daemon tests | pending |
| 04 | M-Core realtime endpoint 与共享接口守门 | Step 01 | B | 是 | agent-sdk-contract | Step 02 / 03 | `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*` 只读优先；如改需独立评审 | 明确是否需要改 `im-core` endpoint 选择；默认不改 public API | [steps/04-m-core-realtime-contract.md](steps/04-m-core-realtime-contract.md) | 必须 | shared SDK contract gate | pending |
| 05 | 多 WebSocket 统一事件 supervisor | Step 02, Step 03, Step 04 | 串行 | 否 | agent-realtime | 无 | `foreground.rs` 主控制流、新增 realtime supervisor、dispatcher | per-agent WSS session、统一事件队列、direct/group 事件路由 | [steps/05-runtime-realtime-supervisor.md](steps/05-runtime-realtime-supervisor.md) | 必须 | realtime unit + daemon unit | pending |
| 06 | reliable sync、gap 与 fallback 协调 | Step 05 | 串行 | 否 | coordinator | 无 | realtime supervisor、sync/fallback coordinator | WSS hint 触发 sync delta/thread-after，断线重连和低频兜底 | [steps/06-sync-gap-fallback.md](steps/06-sync-gap-fallback.md) | 必须 | reconnect/gap tests | pending |
| 07 | 最终集成、文档同步、remote 系统测试 | Step 02-06 | 串行 | 否 | coordinator | 无 | 全部已改模块、docs、Harness 摘要、`awiki-system-test` 环境 | 全局 Review、idle 对比、remote `awiki.info` 完整系统测试 | [steps/07-final-integration-system-test.md](steps/07-final-integration-system-test.md) | 必须 | final full gate | pending |

## 8. 并行执行与多智能体分工

- 并行策略：Step 01 建立基线后，Step 02、Step 03、Step 04 可以并行推进；Step 05 / 06 / 07 因为会整合 foreground 主控制流和事件 supervisor，必须串行。
- 最大并行度：3。保守执行时可只并行 Step 02 和 Step 03，把 Step 04 由 coordinator 串行完成。
- Coordinator：负责主 Plan / 执行台账 / Plan 变更记录 / 合并顺序 / Review / 验证证据 / 最终系统测试。
- 串行原因：Step 05 和 Step 06 都会修改 realtime supervisor、dispatcher、foreground loop 和 fallback 语义；并行会造成控制流冲突和验证证据不清。

### Agent 分工

| Agent / Worker | 负责 Step | 责任边界 | 可修改路径 | 禁止修改路径 / 资源 | 交付物 | Review 责任 |
|---|---|---|---|---|---|---|
| coordinator | Step 01、06、07 | 基线、sync/fallback 集成、最终验证、台账和文档同步 | `awiki-cli-rs2-cpu/docs/plan/...`、`awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` 集成段、最终 docs | 不覆盖并行 worker 未合并成果 | focused commit + 验证证据 | 全局 Review |
| agent-storage | Step 02 | identity 文件内容感知写入 | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs`、相关 tests | foreground 主循环、realtime supervisor、queue scheduler | focused commit + mtime/write 证据 | coordinator review |
| agent-scheduler | Step 03 | due queue scheduler 与 notify 接入 | 新增 scheduler 模块、queue state helper、局部 enqueue notify tests | identity sync、realtime session、im-core | focused commit + scheduler tests | coordinator review |
| agent-sdk-contract | Step 04 | M-Core endpoint / public API 兼容性判断 | 默认只读；如批准修改，仅限 `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*` 与对应 tests/docs | daemon runtime routing、message-service 协议 | 兼容性结论或独立 API commit | coordinator review + shared SDK review |
| agent-realtime | Step 05 | per-agent realtime task、统一事件队列、dispatcher | 新增 `runtime_realtime` 模块、dispatcher helpers、focused tests | queue scheduler 内部、identity write helper、sync checkpoint | focused commit + realtime tests | coordinator review |

### 并行组

| Wave / 并行组 | 可并行 Step | 可并行原因 | 共享依赖 | 写入范围 | 依赖屏障 | 合并顺序 | Group gate / 验证责任 |
|---|---|---|---|---|---|---|---|
| A | Step 02, Step 03 | storage 写入优化与 due queue scheduler 路径基本独立。 | Step 01 | `im_core_adapter.rs` vs scheduler/state/queue 局部 | 合并前确认 Step 03 没有改 identity helper，Step 02 没有改 foreground 控制流 | Step 02 -> Step 03 -> `cargo test -p awiki-deamon --locked` | coordinator 记录 idle I/O 对比和 queue tests |
| B | Step 04 与 Step 02 / 03 | Step 04 默认只读或小范围 `im-core` endpoint contract；不依赖 storage / queue 实现。 | Step 01 | 默认无代码写入；如需写入 `im-core`，必须暂停并更新 Plan | 若 Step 04 决定改 shared API，则暂停 Step 05 并完成 shared SDK regression | Step 04 结论必须早于 Step 05 | shared SDK contract gate |

### 互斥资源

| 资源 / 路径 / 契约 | 互斥原因 | 受影响 Step | 规则 |
|---|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` 主循环 | 调度控制流重构容易冲突 | Step 01、03、05、06 | 同一时间只能一个 worker 修改同一段；合并前 coordinator Review。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/realtime/*` public API | 共享 SDK 影响 `awiki-cli` / `im-core-dart` / App | Step 04、05、06 | 默认只读；必须改时暂停相关 Step，完成兼容性评审和回归。 |
| reliable checkpoint 语义 | 不能让 daemon 手写或推进 checkpoint | Step 05、06 | 只调用 `sync_delta_async` / `sync_thread_after_async`；不得暴露 checkpoint。 |
| remote `awiki.info` 系统测试环境 | 同一环境验证会互相影响 | Step 07 | 最终 gate 串行执行并记录环境配置。 |

并行执行约束：

- 每个 Agent / Worker 只修改自己拥有的文件、模块或验证表面，不回退或覆盖其他 Agent 的修改。
- Agent / Worker 必须回报变更路径、命令、测试结果、阻塞、剩余风险和未触碰的外部所有权文件。
- 需要越界修改、改变并行组、改变合并顺序或发现互斥资源冲突时，先更新 Plan 变更记录并重新评估 parallel-safe。
- Coordinator 必须在合并后检查组合 diff、冲突、Review 结论、步骤验证证据和整体验证证据。
- Group A / B 的 gate 通过前，不得启动依赖它们的 Step 05。

## 9. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | Agent / Owner | 并行组 | 分支 / worktree | 基线 commit | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 合并状态 | 门禁状态 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 | done | coordinator | 串行 | `feature/perf/cpu-youhua-jingmo-0628` | `4b15c4d` | 2026-06-28T12:18:30+08:00 | 2026-06-28T14:28:19+08:00 | 本 Step 提交：`daemon: record idle scheduling baseline` | 已确认本步骤未修改业务代码，现有 tests 覆盖 foreground owner guard、archive finalizer、queue drain、future due、retry defer、heartbeat 节流和 runtime inbox poll scope；无 `im-core` diff。 | 60 秒 idle 采样：CPU 平均 6.50%，RSS 平均 9992KB，线程 6；`write_bytes` 增量 244801536，约 4080025.60/s；31 个 identity / `im-core` 文件 mtime 变化；`cargo test -p awiki-deamon --locked -j1` 通过，471 passed / 0 failed / 3 ignored。 | merged | pass | 启动 Step 02 / Step 03 / Step 04 前置检查 |
| 02 | pending | agent-storage | A | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 01 |
| 03 | pending | agent-scheduler | A | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 01 |
| 04 | pending | agent-sdk-contract | B | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 01 |
| 05 | pending | agent-realtime | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 02-04 |
| 06 | pending | coordinator | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 05 |
| 07 | pending | coordinator | 串行 | TBD | TBD | TBD | TBD | TBD | TBD | TBD | not_started | pending | 等 Step 02-06 |

## 10. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、当前第一个未 done 的 Step 文档、执行台账和当前 `git status --short --branch`。
- 默认同一时间只执行一个步骤；只有任务拆分表和“并行执行与多智能体分工”同时标记为 parallel-safe 的步骤，才启动多个 Agent / Worker 并行处理对应 Wave。
- 并行执行时，Coordinator 必须分配清晰的文件 / 模块 / 验证所有权，要求每个 Agent / Worker 不回退或覆盖他人修改，并在合并前收集变更路径、命令、测试结果、阻塞和剩余风险。
- 并行步骤必须使用分支、worktree、子智能体隔离工作区或等价隔离机制；如果当前环境只能串行应用并行结果，必须记录实际隔离方式和合并顺序。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤依次执行或在 parallel-safe Wave 内并行执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 并行 Wave 结束后，Coordinator 必须执行组合 diff Review、冲突检查、必要的集成验证和执行台账回填。
- 上一个依赖步骤的完成工作未提交前，不要开始下一个依赖步骤。
- 改变范围、顺序、验收标准、公开契约、数据模型、parallel-safe 状态或验证策略前，先更新本 Plan 和对应 Step 文档。

## 10.1 Codex Goal 提示词

```text
请以 awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md 为唯一规划入口，按文档执行完整实现。

开始前先读取：
- awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md
- 当前第一个未 done 的 Step 文档
- 主 Plan 的执行台账、Codex Goal 执行协议、验证策略、Blocked 处理和 Plan 变更记录
- 当前 git status --short --branch

请从第一个状态不是 done 的步骤开始。默认一次只执行一个步骤；只有主 Plan 的任务拆分表和“并行执行与多智能体分工”明确标记为 parallel-safe 的 Step 02 / Step 03 / Step 04，才按并行组尽量启动多个 Agent / Worker，并为每个 Agent / Worker 分配清晰的文件、模块或验证所有权。并行执行前确认隔离方式、路径范围、互斥资源、合并顺序和 group gate。

每步都要按对应小 Plan 实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填主 Plan 执行台账和 Step 执行状态。并行 Wave 完成后，由 Coordinator 做组合 diff Review、冲突检查、必要的集成验证和证据归档。需要改变范围、顺序、验收标准、公开契约、数据模型、parallel-safe 标记或验证策略时，先更新 Plan 变更记录。

核心注意点：不要让 runtime backend 持有 DID 私钥或直连 message-service；每个 active agent 的 realtime task 必须长期持有并复用同一个 ImClient；多 WebSocket 事件统一在 daemon 层 fan-in，不把 daemon source metadata 塞进 im-core ImEvent；不要把 realtime sync hint 当作可靠 checkpoint；本计划默认不新增 message-service 协议、不破坏 im-core public API；最终必须在 awiki-system-test 使用 AWIKI_SYSTEM_TEST_MODE=remote 和 awiki.info 域名执行完整系统测试并记录结果。
```

## 11. 小 Plan 摘要

| Step | 小 Plan | 目标 | Parallel-safe | 验证重点 |
|---|---|---|---|---|
| 01 | [steps/01-baseline-observability.md](steps/01-baseline-observability.md) | 建立可重复 idle CPU/I/O/日志基线和调度保护。 | 否 | baseline 证据、daemon unit。 |
| 02 | [steps/02-identity-sync-write-if-changed.md](steps/02-identity-sync-write-if-changed.md) | 消除相同 identity/token 的无条件文件写。 | 是 | mtime/write-if-changed tests。 |
| 03 | [steps/03-local-queue-schedulers.md](steps/03-local-queue-schedulers.md) | 本地队列改为 Notify + due timer。 | 是 | scheduler due/notify/shutdown tests。 |
| 04 | [steps/04-m-core-realtime-contract.md](steps/04-m-core-realtime-contract.md) | 明确 M-Core endpoint/API 是否需要改，默认守住共享接口。 | 是 | shared SDK contract gate。 |
| 05 | [steps/05-runtime-realtime-supervisor.md](steps/05-runtime-realtime-supervisor.md) | 建 per-agent realtime task 和统一事件 fan-in。 | 否 | direct/group event routing、client lifecycle。 |
| 06 | [steps/06-sync-gap-fallback.md](steps/06-sync-gap-fallback.md) | 可靠 sync、gap、reconnect、fallback 协调。 | 否 | sync delta/thread-after、reconnect/gap tests。 |
| 07 | [steps/07-final-integration-system-test.md](steps/07-final-integration-system-test.md) | 全局 Review、文档同步、remote system test。 | 否 | workspace tests、remote awiki.info full gate。 |

## 12. Review 策略

- 每步骤 Review：优先检查 correctness、回归、消息幂等、授权、失败路径、测试覆盖、observability 和 idle 证据。
- 并行组 Review：检查路径所有权、是否越界修改、是否产生共享契约冲突、是否需要重新评估 `parallel-safe`。
- 合并后 Review：重点看 foreground supervisor、queue scheduler、realtime task、unified event channel、shutdown/restart/backoff 是否组合正确。
- 契约 / 安全 / 隐私 Review：检查 DID WBA session、runtime_rpc_token、DID 私钥、JWT、E2EE opaque、sync checkpoint 边界、`im-core` public API 兼容性。
- 文档 Review：检查 `awiki-cli-rs2-cpu` daemon docs、`im-core` docs、message-service API docs、Harness 摘要是否与实现一致；若不需要更新，记录检查结果和理由。

## 13. 验证策略

| 层级 | 适用 Step / 并行组 | 命令 / 检查 | 运行时机 | 预期证据 | 门禁结果 |
|---|---|---|---|---|---|
| Step Unit | Step 01-07 | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | 每步 commit 前 | 通过或记录原因 | pending |
| Storage Focus | Step 02 | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked im_core_adapter` 或 focused identity sync tests | Step 02 commit 前 | 相同内容不重写 | pending |
| Queue Focus | Step 03 | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked queue` 和新增 scheduler tests | Step 03 commit 前 | due/notify/shutdown 通过 | pending |
| SDK Unit | Step 04-06 | `cd awiki-cli-rs2-cpu && cargo test -p im-core --locked realtime sync` | realtime/sync 改动后 | 通过或记录原因 | pending |
| Shared SDK Contract | Step 04-06 | 检查 `awiki-cli-rs2-cpu/crates/im-core/src` diff；若无改动，记录“未改共享 SDK”；若有改动，必须先完成兼容性评审 | Step 04 / 05 / 06 Review 前 | 没有未授权共享接口改动 | pending |
| Shared SDK Regression | Step 04-06 | `cd awiki-cli-rs2-cpu && cargo test -p awiki-cli --locked && cargo test -p im-core-dart --locked` | 仅当独立评审批准修改 `crates/im-core` 后 | 共享调用方通过或记录环境失败原因 | pending |
| Group Integration | Wave A / B | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | 并行组合并后 | 通过或记录原因 | pending |
| Idle Evidence | Step 01、02、03、05、07 | 60 秒 CPU/I/O/mtime 采样，记录 active agent 数和环境 | 改前、关键改动后、final | 对比表 | pending |
| Cross Repo | Step 07 | `cd message-service && cargo test --workspace` | 仅当实际修改 message-service 后 | 通过或记录跳过原因 | pending |
| Final Workspace | Step 07 | `cd awiki-cli-rs2-cpu && cargo test --workspace --locked` | final 前 | 通过或记录不可运行原因 | pending |
| Final System | Step 07 | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote AWIKI_BASE_URL=https://awiki.info uv run python manage_local_test_env.py run-tests` | 所有步骤完成后 | 通过/失败/跳过数量、原因、环境 | pending |
| Docs | 全部 | Markdown 路径存在检查；必要时 `cd awiki-harness && python scripts/validate-docs.py && python scripts/check-drift.py` | final 前 | 无失效链接或记录原因 | pending |

## 14. 文档更新

- 主计划与小计划：`awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` 和 `steps/*.md`。
- 子仓库文档：最终实现后更新 `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/local-dev.md` 和 `awiki-cli-rs2-cpu/crates/awiki-deamon/docs/awiki_agent_runtime_host_architecture.md` 中 foreground 调度、realtime supervisor、fallback 和验证说明。
- `im-core` 文档：只有实际修改 `im-core` public API 或 endpoint 语义时，更新 `awiki-cli-rs2-cpu/docs/api/im-core-interface/04-message-interface.md` 或相关 public API 文档；否则 final 记录已检查且无需更新。
- Harness 文档：若跨服务契约、运行拓扑或验证入口变化，更新 `awiki-harness/context/03-cross-repo-architecture.md`、相关 node card 或 repo profile；如果只是 daemon 内部实现优化，final 记录 Harness 摘要仍准确。

## 15. Commit 计划

- 每个完成、验证、Review 通过的步骤创建一个聚焦 commit。
- Commit 前记录 `git status --short --branch` 和纳入文件。
- Commit 后记录 commit hash 和工作区状态。
- 并行步骤仍保持“一步一个聚焦 commit”；不得把多个 Agent / Worker 的完成工作合并成一个大 commit，除非对应 Step 文档记录不能独立提交的具体原因和最小安全范围。
- Group A 合并顺序为 Step 02 -> Step 03 -> Group gate；Step 04 结论必须早于 Step 05。
- 只有最终集成确实修改文件时才创建最终集成 commit。

## 16. Blocked 处理

| Blocker | Step | Agent | 并行组 | 证据 | 已尝试方案 | 影响范围 | 是否暂停同组 | 下一步决策 |
|---|---|---|---|---|---|---|---|---|
| message-service WSS 不支持目标 agent DID | Step 05 / 06 | agent-realtime / coordinator | 串行 | WSS auth/admission 失败日志 | 检查 DID WBA session、admission、transport policy | Step 05 / 06 | 是 | 启用低频 poll fallback，记录协议缺口，不恢复 250ms 全量扫 |
| 现有 `im-core` public API 无法满足 daemon 事件驱动需求 | Step 04 / 05 / 06 | agent-sdk-contract / coordinator | B / 串行 | 编译错误、缺失能力调查、daemon 内 adapter 无法覆盖的证据 | 优先尝试 daemon 内 adapter、复用现有 `start_async` / `subscribe` / `sync_delta_async` / `sync_thread_after_async` | 共享 SDK 契约，影响 `awiki-cli` / `im-core-dart` | 是 | 暂停当前 Step，更新 Plan，做独立兼容性评审并等待用户确认 |
| 多 WebSocket session 造成资源压力 | Step 05 / 06 | agent-realtime | 串行 | session 数、连接失败、服务端限流或本机资源指标 | 限制 active realtime session 数、增加 backoff、降级低频 fallback | Step 05 / 06 / Final | 是 | 增加配置或保守默认，并记录容量风险 |
| queue scheduler 漏唤醒 | Step 03 / 07 | agent-scheduler | A | focused test 或系统测试中 pending 未处理 | 启动恢复、低频 reconciliation、补 notify hook | Step 03 / 07 | 是 | 修复 scheduler 或临时保留低频 queue reconciliation |
| remote system test 环境不可用 | Step 07 | coordinator | 串行 | `awiki-system-test` 命令失败 | 重试、记录环境、检查 awiki.info | 整体计划 | 是 | 向用户报告并保留未通过风险 |

只有依赖允许且风险已记录时，才继续另一个 pending 步骤。如果 blocker 影响共享契约、共享路径、合并顺序或 group gate，必须暂停同组相关步骤的合并。只有没有安全假设、回退方案或独立下一步时，才询问用户。

## 17. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-28 | 创建初版 Plan | 沉淀 idle CPU 调查和事件驱动优化方向 | 全部 | 是 |
| 2026-06-28 | 升级为主 Plan + 小 Plan 文档，增加多 WebSocket 统一事件 supervisor、M-Core API 守门和 7 步执行设计 | 用户要求基于现有文档设计全面、详细、可落地的修改方案 | 全部 | 是 |

## 18. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| WSS notification 丢失导致命令不执行 | `sync_delta_async` / `sync_thread_after_async` / low-frequency reconciliation / processed message 幂等 | 恢复低频 poll fallback，不恢复 250ms 全量扫。 |
| 多 agent WSS 连接造成服务端压力 | session supervisor 限流、指数退避、连接数指标、可配置上限 | 配置回退到低频 poll 或限制 active agent realtime。 |
| per-agent task 中重复创建 `ImClient`，导致轮询成本残留 | lifecycle tests、诊断计数、Review 检查 `client_for_agent_identity` 调用点 | 回退到显式 per-agent client cache，按 identity/token/hash 失效。 |
| identity 内容感知写入漏更新 | hash/mtime tests、token/did/private key/e2ee key 变化 tests | 回退到启动时强制 sync + 内容感知运行时 sync。 |
| queue scheduler 漏唤醒 | 启动恢复、due timer、notify tests、低频 reconciliation | 临时保留对应 queue 的低频 reconciliation。 |
| heartbeat/status 回归 | 保留现有节流常量和 dedicated timer tests | 回退 heartbeat timer 改动。 |
| 共享 `im-core` 接口变更影响 `awiki-cli` / `im-core-dart` | 默认不改 public API；Shared SDK Contract gate 检查 diff；必须改时先 blocked 并做独立兼容性评审 | 回退共享接口改动，改为 daemon 内 adapter 或低频 fallback。 |
| foreground shutdown / archive finalizer 回归 | Step 01 / 07 保留 archive finalizer 和 shutdown tests | 回退 finalizer 集成段，保留启动时检查和低频 timer。 |

## 19. 最终全局 Review 与整体验证

- 触发条件：所有步骤完成、Review、验证并提交后执行。
- Review 范围：`awiki-cli-rs2-cpu` daemon foreground/realtime/scheduler/storage、`im-core` realtime/sync 只读复用或批准变更、Harness 和子仓库 docs、执行台账、系统测试证据。
- 重点关注：静默 CPU/I/O 是否下降、direct/group runtime command 是否仍执行、per-agent `ImClient` 是否按 task 生命周期复用、多 WebSocket 事件是否统一 fan-in、WSS 断线重连和 gap 是否可靠、checkpoint 边界、安全/隐私、并行组合并冲突。
- 共享 SDK 审计：确认没有未经独立评审的 `im-core` public API / DTO / feature gate / transport 默认语义变更；若 `crates/im-core` 有任何 diff，必须记录批准依据、兼容性验证和 `awiki-cli` / `im-core-dart` 回归证据。
- 并行执行审计：确认 Wave A / B 每个 worker 只修改授权路径；所有越界变更均已更新 Plan；group verification gate 已通过。
- 整体验证命令 / 检查：见第 13 节。
- Review 发现：TBD。
- 已修复问题：TBD。
- 剩余风险：TBD。
- 最终证据：TBD。
- 最终 `git status`：TBD。
- 如果本阶段修改文件：记录 Review、验证和最终集成 commit。

## 20. Step 01 执行证据

本节记录 Step 01 的基线证据，后续 Step 02 / 03 / 05 / 07 应按同一口径复测。为避免把本机路径固化进计划文档，运行态 state root、ready file 和临时采样目录均以占位符记录；原始命令在执行终端中使用当前用户级 daemon service 的实际参数。

| 项 | 证据 |
|---|---|
| 基线 commit | `4b15c4d` |
| 分支 | `feature/perf/cpu-youhua-jingmo-0628` |
| 运行进程 | `awiki-deamon foreground --state-root <daemon_state_root> --ready-file <ready_file>` |
| 采样窗口 | 2026-06-28T12:20:51+08:00 到 2026-06-28T12:21:51+08:00，60 秒 |
| active agents / queues | 只读 SQLite 查询：`agent_definition` 12 条，其中 active 8 条；`runtime_profile` 11 条；`message_sync_outbox` 24 条；`runtime_final_outbox` 26 条。环境缺少 `sqlite3` CLI，改用 Python `sqlite3` 只读查询。 |
| CPU / RSS / 线程 | 12 次 5 秒间隔 `ps` 采样，CPU 样本均为 6.5%，平均 6.50%；RSS 平均 9992KB，最小 8192KB，最大 12252KB；线程数平均 6。 |
| I/O | `/proc/<daemon_pid>/io` 60 秒差值：`rchar=160111338`，约 2668522.30/s；`wchar=176642981`，约 2944049.68/s；`syscr=53622`，约 893.70/s；`syscw=120735`，约 2012.25/s；`read_bytes=0`；`write_bytes=244801536`，约 4080025.60/s。 |
| mtime | 31 个被采样文件 mtime 变化，集中在 `identity/<agent>/did.json`、`private.key`、`e2ee-agreement-private.pem`、`identity/registry.json`、`identity/default`、`im-core/local-state.sqlite`、`im-core/local-state.sqlite-shm`、`im-core/local-state.sqlite-wal`。 |
| 日志 | 采样窗口内 `journalctl --user -u awiki-deamon.service` 增量 1 行；采样前最近日志可见多条 `daemon.runtime_inbox.session.failed`，原因是 agent DID WBA session refresh 失败。 |
| 测试 | 首次 `cargo test -p awiki-deamon --locked` 在并发链接 `hermes_gateway` test 时失败：`ld terminated with signal 9 [Killed]`，判断为本机资源 / OOM 型失败；重跑 `cargo test -p awiki-deamon --locked -j1` 通过，471 passed / 0 failed / 3 ignored。 |
| 代码改动 | 本步骤未修改业务代码；只回填 Plan / Step 执行证据。 |
