# Step 01：基线观测与调度保护

主 Plan：[../plan.md](../plan.md)  
Step index：01  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/perf/cpu-youhua-jingmo-0628` |
| Started | 2026-06-28T12:18:30+08:00 |
| Completed | 2026-06-28T14:28:19+08:00 |
| Commit | `d0d01e9` |
| Review evidence | 本步骤未修改业务代码；Review 确认现有测试已覆盖 foreground owner guard、archive finalizer、queue drain、future due、retry defer、heartbeat 节流和 runtime inbox poll scope；无 `im-core` diff。 |
| Verification evidence | 60 秒 idle 采样完成；`cargo test -p awiki-deamon --locked -j1` 通过，471 passed / 0 failed / 3 ignored；首次无 `-j1` 运行因 linker `signal 9 [Killed]` 失败，记录为本机资源 / OOM 型失败。 |
| Next action | 启动 Step 02 / Step 03 / Step 04 前置检查；这些步骤可按主 Plan parallel-safe 规则并行或由 coordinator 串行推进。 |
| Assigned agent | coordinator |
| Parallel group | 串行 |
| Parallel safe | no |
| Parallel with | 无 |
| Conflict resources | `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs`、运行态 daemon 进程、idle 采样环境 |
| Baseline commit | `4b15c4d` |
| Worktree / branch | `feature/perf/cpu-youhua-jingmo-0628` |
| Merge gate | 通过；Step 02 / 03 / 04 可启动。 |
| Verification gate | 通过；见第 8 节和第 14 节执行证据。 |
| Gate status | pass |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：在不改变 daemon 行为语义的前提下，建立静默 CPU、I/O、日志、identity 文件 mtime、`im-core` local state 写入、active agent 数和 foreground 循环频率的可重复基线。
- 用户 / 系统可见行为：daemon 仍按现有 250ms 轮询方式处理 direct/group runtime message、user delegated inbox、本地 queue、heartbeat 和 shutdown；不会提前引入事件驱动逻辑。
- 非目标：本步骤不重构 queue scheduler、不接入 WebSocket realtime、不修改 `im-core` public API、不改变 message routing 或 heartbeat 语义。
- 完成标准：有一组可以在后续 Step 重复使用的采样命令、采样结果表和最小诊断字段；现有 daemon tests 通过或记录不可运行原因；主 Plan 执行台账记录基线 commit、采样环境和证据位置。

## 3. 设计方法

- 设计边界：只做观测、测试保护和极小诊断增强，不把后续架构变化提前混进本步骤。
- 核心决策：基线必须覆盖 CPU、I/O、mtime 和 foreground 工作项次数，因为当前问题不是单一 CPU hot loop，而是 250ms 轮询触发多类扫描和写入。
- 契约 / API / 数据流：不改变 public API、配置默认值、local RPC 协议、message payload、state schema 或 `im-core` 行为。
- 兼容性：如果新增日志或诊断字段，必须使用现有 daemon tracing / audit 风格，不能泄露 DID 私钥、JWT、runtime token、message plaintext 或 E2EE opaque 内部内容。
- 迁移策略：无迁移；如新增临时 helper 或测试工具，必须放在 daemon 内部测试 / docs 允许范围。
- 风险控制：本步骤的采样命令要记录运行前提，避免把本机其他进程、debug build、后台系统测试或热启动同步误判为 daemon idle 成本。

## 4. 实现方法

1. 固化基线采样流程：
   - 记录 daemon 启动方式、配置来源、active agent 数、状态目录、是否连接 `awiki.info`、是否开启 debug 日志。
   - 采样 60 秒 `awiki-deamon` 进程 CPU、RSS、线程数、文件 I/O、状态文件 mtime 和日志量。
   - 明确 idle 定义：无主动 CLI route、无 runtime 回调、无人工触发 direct/group message、无系统测试正在执行。
2. 阅读并标记现有 foreground 工作项：
   - `process_inbox_once`
   - `process_user_delegated_inbox_once`
   - `flush_message_sync_outbox`
   - `drain_cli_route_message_queue_once`
   - `drain_runtime_retry_queue_once`
   - `flush_runtime_final_outbox`
   - `HeartbeatScheduler::tick`
   - `tokio::time::sleep(Duration::from_millis(options.poll_interval_ms))`
3. 只在必要时新增轻量诊断：
   - 可以新增 `trace!` / `debug!` 级别计数或测试可见 counters，用于确认每轮执行次数和每个工作项是否被调用。
   - 默认日志级别下不能产生高频新日志，避免观测本身增加 CPU / I/O。
4. 补齐行为保护测试：
   - 如果已有 tests 能覆盖 foreground shutdown、archive finalizer、queue drain、heartbeat 节流，则记录现有测试入口。
   - 如果没有，新增最小 focused tests，证明调度保护不改变现有处理顺序、shutdown 退出和 finalizer 行为。
5. 形成基线证据表：
   - 将采样命令、环境、时间窗口、指标结果和异常项记录到主 Plan 执行台账或 Step Review 证据。
   - 后续 Step 02 / 03 / 05 / 07 复用同一套命令做对比。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` | 只允许新增必要诊断、测试 seam 或 focused test 入口；不重构主循环。 | 本步骤互斥资源，不能与 Step 03 / 05 / 06 并行修改同一段。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/*` tests | 允许新增或扩展 focused unit tests。 | 优先跟随现有 test module 风格。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填执行台账、基线证据、Review 结论。 | 后续实现执行时由 coordinator 更新。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/steps/01-baseline-observability.md` | 回填本 Step 状态、证据、commit。 | 本规划文档本身。 |

## 6. 依赖与并行约束

- 前置步骤：无。
- 可并行步骤：无。
- 不可并行步骤：Step 02 / 03 / 04 都依赖本步骤的 baseline commit；Step 03 / 05 / 06 与本步骤共享 foreground 主循环。
- 并行安全依据：本步骤需要独占当前运行态采样环境和 foreground 初始状态，否则基线不可比较。
- 互斥资源 / 冲突路径：`awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs`、daemon 本地状态目录、remote message traffic。
- 外部文档或决策：无需用户决策；如果无法启动 daemon 或无法访问采样环境，记录 blocker 和替代证据。
- 环境前提：能够从 `awiki-cli-rs2-cpu` 运行 daemon 或读取现有 foreground 进程；有权限读取 `/proc` 指标、状态目录 mtime 和日志。
- 合并前置条件：baseline 采样和测试保护已记录；未引入行为改变。
- 合并后验证门禁：`cargo test -p awiki-deamon --locked` 通过或记录原因；主 Plan 台账更新。

## 7. 验收标准

- [x] 已记录执行前 `git status --short --branch`、baseline commit、采样时间、daemon 启动方式和关键环境。
- [x] 已记录 active agent 数、foreground `poll_interval_ms`、状态目录、服务端环境和日志级别。
- [x] 已采样至少 60 秒 idle CPU / RSS / thread count / I/O / mtime / 日志量。
- [x] 已确认本步骤没有修改 `im-core` public API、message-service 协议、state schema 或 transport 默认语义。
- [x] 若新增诊断，默认日志级别下不会制造高频新写入；实际没有新增诊断代码。
- [x] 已运行 `cargo test -p awiki-deamon --locked -j1` 并通过；首次无 `-j1` 运行失败原因和风险已记录。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Worktree baseline | `cd awiki-cli-rs2-cpu && git status --short --branch && git rev-parse HEAD` | 开始前、commit 前 | 基线 commit `4b15c4d`；开始前工作区干净。 | Step gate |
| Daemon unit | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | commit 前 | 首次运行链接 `hermes_gateway` test 时 `ld terminated with signal 9 [Killed]`；判断为本机资源 / OOM 型失败。 | Step gate |
| Daemon unit retry | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked -j1` | commit 前 | 通过：471 passed / 0 failed / 3 ignored。 | Step gate |
| CPU 采样 | `ps -o pcpu,pmem,rss,nlwp,stat -p <daemon_pid>` 每 5 秒采样 12 次 | idle 窗口 | 60 秒 CPU 平均 6.50%，RSS 平均 9992KB，线程数平均 6。 | Baseline evidence |
| I/O 采样 | 读取 `/proc/<daemon_pid>/io` 前后差值 | idle 窗口 | `write_bytes=244801536`，约 4080025.60/s；`wchar=176642981`，约 2944049.68/s；`syscw=120735`，约 2012.25/s。 | Baseline evidence |
| mtime 采样 | 对 daemon state root 下 identity、registry、default、`im-core/local-state.sqlite*` 做 60 秒前后 `stat` 对比 | idle 窗口 | 31 个文件 mtime 变化，集中在 identity 和 `im-core` local-state 文件。 | Baseline evidence |
| 日志采样 | `journalctl --user -u awiki-deamon.service --since <采样开始>` | idle 窗口 | 采样窗口日志增量 1 行；采样前最近日志有多条 `daemon.runtime_inbox.session.failed`。 | Baseline evidence |
| Docs ledger | 检查主 Plan 执行台账和本 Step 状态已回填 | commit 前 | 台账含命令、结果、风险。 | Step gate |

如果某个命令不能运行，必须记录原因、影响和替代证据；不能把未运行命令写成通过。

## 9. Review 环节

- Review 时机：本步骤代码或诊断改动完成后、commit 前。
- Review 重点：是否保持现有行为、是否误改调度顺序、是否泄露敏感信息、采样方法是否可重复、测试是否能保护后续重构。
- Review 必须确认本步骤没有提前引入事件驱动实现或隐藏修改 `im-core`。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 首次 `cargo test -p awiki-deamon --locked` 因 linker `signal 9 [Killed]` 失败。 | 失败发生在链接 `hermes_gateway` test，属于本机资源 / OOM 型失败，不是测试断言或编译错误。 |
| 已修复问题 | 使用 `-j1` 降低并发后完整测试通过。 | `cargo test -p awiki-deamon --locked -j1` 通过。 |
| 剩余风险 | idle 采样来自当前用户级 daemon service，后续对比必须使用同一口径；日志窗口内远端流量不可完全静态。 | 已记录 active agent、queue 数、采样窗口和服务状态。 |
| 新增或缺失测试 | 未新增测试；现有测试已覆盖本步骤保护面。 | 关键测试包括 foreground owner guard、archive finalizer、queue due/future、retry defer、heartbeat scheduler、runtime inbox scope。 |
| 已更新或缺失文档 | 已更新主 Plan 和本 Step 执行证据。 | 长期 daemon docs 留到 Step 07 同步。 |
| 并行安全是否仍成立 | no | 本步骤为串行基线步骤。 |
| Agent 是否越界修改 | 否 | 只修改计划文档。 |
| 互斥资源是否被修改 | 否 | 未修改 `foreground.rs`。 |
| 合并风险 | 低 | Step 02 / 03 / 04 可基于本基线启动。 |
| Group gate 影响 | Step 02 / 03 / 04 依赖本步骤完成 | 本步骤 gate 已通过。 |

## 10. Commit 要求

- Commit 时机：采样证据、必要诊断、测试、Review 都完成后。
- Commit 范围：只包含本步骤的诊断 / 测试 / 文档台账改动，不包含 Step 02 之后的功能重构。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- Commit 后必须记录 Step 02 / 03 / 04 是否可以启动。
- 建议消息：`daemon: record idle scheduling baseline`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| 无法定位或启动 daemon 进程 | 启动命令失败、无 PID、日志缺失 | 检查 local-dev 文档、现有服务、测试环境 | 当前步骤 | 是 | 是 | 记录 blocker；可先完成源码级测试保护，但不能标记 baseline 完成。 |
| 无法获得稳定 idle 窗口 | 远端消息、系统测试、用户操作持续发生 | 延后采样、换空状态目录、记录流量来源 | 当前步骤 / final 对比 | 是 | 是 | 等待稳定窗口或记录不可比较风险。 |
| 观测命令权限不足 | `/proc` 或日志读取失败 | 使用可用替代命令 | 当前步骤 | 否 | 视替代证据而定 | 记录缺失指标和风险。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 01 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 执行证据

| 项 | 结果 |
|---|---|
| 基线 commit | `4b15c4d` |
| 运行进程 | 当前用户级 `awiki-deamon foreground --state-root <daemon_state_root> --ready-file <ready_file>` |
| 采样窗口 | 2026-06-28T12:20:51+08:00 到 2026-06-28T12:21:51+08:00，60 秒 |
| active agents / queues | Python `sqlite3` 只读查询：`agent_definition` 12 条，其中 active 8 条；`runtime_profile` 11 条；`message_sync_outbox` 24 条；`runtime_final_outbox` 26 条。 |
| CPU / RSS / threads | 12 次采样 CPU 均为 6.5%，平均 6.50%；RSS 平均 9992KB；线程平均 6。 |
| I/O | `rchar=160111338`，约 2668522.30/s；`wchar=176642981`，约 2944049.68/s；`syscr=53622`，约 893.70/s；`syscw=120735`，约 2012.25/s；`read_bytes=0`；`write_bytes=244801536`，约 4080025.60/s。 |
| mtime | 31 个 identity / `im-core` local-state 文件 mtime 变化。 |
| 日志 | 采样窗口内日志增量 1 行；采样前最近日志显示重复 `daemon.runtime_inbox.session.failed`。 |
| 测试 | `cargo test -p awiki-deamon --locked`：失败于 linker `signal 9 [Killed]`；`cargo test -p awiki-deamon --locked -j1`：通过，471 passed / 0 failed / 3 ignored。 |
| 代码范围 | 未修改业务代码，未修改 `im-core`，未修改 message-service 协议或 state schema。 |

## 14. 风险、回滚与后续文档

- 风险：采样环境不稳定会导致后续优化效果不可比较；新增诊断若默认输出过多会反向增加 I/O。
- 并行执行风险：本步骤不并行，避免 baseline 被其他改动污染。
- 合并冲突风险：低；仅当同时有人改 `foreground.rs` 诊断或 tests 时需要 coordinator 合并。
- Group gate 失败回退：不启动 Wave A / B，先修正基线或记录用户确认的替代基线。
- Agent 交接说明：后续 Step 必须引用本步骤记录的采样方法和 baseline commit，不要自行换口径比较。
- 回滚 / 回退：如诊断影响行为，回退诊断代码，保留文档中的已采样证据并注明来源 commit。
- 后续文档：最终 Step 07 将把有效采样方法沉淀到 daemon docs 或执行台账；如仅为一次性证据，可不更新长期文档但要说明。
