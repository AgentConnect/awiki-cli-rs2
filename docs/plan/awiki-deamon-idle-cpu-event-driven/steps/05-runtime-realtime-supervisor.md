# Step 05：多 WebSocket 统一事件 supervisor

主 Plan：[../plan.md](../plan.md)  
Step index：05  
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
| Next action | 为 active agent 建立 per-agent `RealtimeSession` task，将多个 WebSocket 事件 fan-in 到 daemon 统一事件队列并路由 runtime message。 |
| Assigned agent | agent-realtime |
| Parallel group | 串行 |
| Parallel safe | no |
| Parallel with | 无 |
| Conflict resources | `foreground.rs` 主控制流、新增 realtime supervisor、dispatcher、transport policy、`im-core` contract |
| Baseline commit | TBD，必须包含 Step 02 / 03 / 04 完成结果 |
| Worktree / branch | TBD |
| Merge gate | Step 02、Step 03、Step 04 done；Step 04 明确没有未解决 shared API blocker。 |
| Verification gate | realtime supervisor focused tests + `cargo test -p awiki-deamon --locked`；如触及 `im-core`，执行 shared SDK gate。 |
| Gate status | pending |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：daemon foreground 不再通过 250ms 全量扫描所有 active agent 的 direct/group inbox，而是为每个 active agent 启动 `im-core` realtime session，由单 reader task 读取 event stream，包装 source metadata 后发送到统一 `tokio::mpsc` channel，由 coordinator 串行处理路由。
- 用户 / 系统可见行为：direct runtime command、group runtime command、controller 到 agent 的消息仍能被 daemon 接收并触发 runtime；多 active agent 可以同时保持 WSS；shutdown / config change / identity change 后 session 生命周期正确。
- 非目标：不修改 message-service WSS 协议；不把 daemon source metadata 加入 `im-core::ImEvent`；不在 runtime backend 内直连 message-service；不在本步骤完整处理 gap / reconnect fallback，那属于 Step 06。
- 完成标准：有 `RuntimeRealtimeSupervisor` 或等价模块，支持 per-agent session lifecycle、event fan-in、status/error 观测、direct/group event 初步 dispatch、shutdown；tests 覆盖多 session fan-in、单 reader 约束、client 复用、事件路由和敏感信息保护。

## 3. 设计方法

- 设计边界：daemon 是多 agent runtime host，`im-core` 是单 client / 单 session SDK；多 WebSocket 编排属于 daemon。
- 核心决策：
  - 每个 active agent DID 对应一个长期 `ImClient` 和一个 `RealtimeSession`。
  - 每个 `RealtimeSession::subscribe()` 只由一个 daemon task 读取。
  - task 将 `ImEvent` 包装成 daemon 层事件：

```text
DaemonRealtimeEvent {
  source: RealtimeSource { agent_did, endpoint_kind, session_id, generation },
  event: im_core::realtime::ImEvent,
}
```

  - central coordinator 统一做 routing、sync hint 转调度、fallback 标记和 audit。
- 契约 / API / 数据流：
  - `ImEvent::MessageReceived` 中的 direct / group message 进入 existing dispatcher。
  - `ImEvent::GroupUpdated` 或 sync hint 不直接全量扫 group，而是记录 dirty / 调度 targeted sync。
  - source metadata 不穿透给 runtime backend，除非现有 audit 需要记录 agent DID。
- 兼容性：保留原 `process_runtime_inbox_message`、`route_message` 或等价 dispatcher 行为；先抽取可复用函数，再由 realtime event 调用。
- 迁移策略：首次可以保留低频 direct/group poll fallback 作为安全兜底，但主链路应是 WSS event；fallback 细化在 Step 06。
- 风险控制：所有 session task 必须有 backpressure、shutdown、错误日志节流和 token / key 脱敏；不能每个 event 重新创建 `ImClient`。

## 4. 实现方法

1. 确认 Step 04 contract：
   - 如果 Step 04 状态不是 done 或 shared API gate blocked，本步骤不得启动。
   - 根据 Step 04 结论决定 daemon `ImCoreConfig` transport policy 是否需要从 `HttpOnly` 调整为允许 realtime。
2. 抽取 dispatcher：
   - 从现有 foreground poll 路径中抽取 direct/group runtime message 处理函数。
   - 保持 processed message 幂等、sender/controller 校验、payload command 解析、runtime invocation 和 error handling。
   - 抽取后旧 poll fallback 和新 realtime event 都走同一 dispatcher。
3. 新增 realtime supervisor 模块：
   - 路径可为 `awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime_realtime.rs`、`foreground/runtime_realtime.rs` 或符合现有结构的模块。
   - 维护 active agent session map，key 至少包含 `agent_did`，value 包含 `ImClient`、session handle、reader task、generation、last status。
   - 根据 active agent 列表启动缺失 session，停止不再 active 或 identity/config hash 变化的 session。
4. 建立 fan-in channel：
   - 每个 session reader task 调用 `subscribe()` 一次。
   - reader task 将事件发送到 central `mpsc`，channel 容量有明确常量。
   - channel 满时采用可解释策略：等待、节流日志、标记 fallback；不能静默丢 message event。
5. 接入 foreground lifecycle：
   - foreground 启动本地 queue scheduler 后启动 realtime supervisor。
   - 主 select 等待 realtime event、本地 queue signal、heartbeat due、fallback tick、shutdown。
   - 保留 archive finalizer 和 shutdown 顺序。
6. direct/group routing：
   - direct message event：按现有 runtime inbox message 处理路径处理。
   - group message event：如果 event 已含 message 且足够执行，直接 route；如果缺上下文，调度 targeted group context fetch，不全 group 扫描。
   - unknown event：记录 debug/audit 并交给 Step 06 fallback coordinator。
7. tests：
   - fake session source 发送两个 agent 的 events，central coordinator 按 source 区分。
   - 单 session 只有一个 reader。
   - session 生命周期中 `ImClient` 不按 event 重建。
   - shutdown 停止 session / reader task。
   - direct/group message 都走同一 dispatcher 并保持幂等。

## 5. 路径

本节所有路径都相对 AWiki workspace 根目录。

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/foreground.rs` | 集成 realtime supervisor，替代 direct/group 250ms 主扫描为事件入口。 | 本步骤高冲突主文件，串行修改。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/runtime_realtime.rs` 或等价路径 | 新增 per-agent session supervisor、fan-in event 类型和 tests。 | 具体路径由实现时按模块风格确定。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/config.rs` | 可能调整 daemon `ImCoreConfig` transport policy，使 realtime 可启动。 | 不能破坏 CLI 或 `im-core` 默认语义。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/im_core_adapter.rs` | 只允许复用 Step 02 client/identity helper；不重新引入高频写。 | 如需大改，更新 Plan。 |
| `awiki-cli-rs2-cpu/crates/awiki-deamon/src/state/*` | 复用 processed message 幂等；可能读取 active agent / session metadata。 | 不做 schema 迁移，除非更新 Plan。 |
| `awiki-cli-rs2-cpu/crates/im-core/src/*` | 默认不修改。 | 如必须修改，回到 Step 04 blocked protocol。 |
| `awiki-cli-rs2-cpu/docs/plan/awiki-deamon-idle-cpu-event-driven/plan.md` | 回填 Step 05 状态、验证证据、commit。 | Coordinator 更新。 |

## 6. 依赖与并行约束

- 前置步骤：Step 02、Step 03、Step 04 done；Wave A / B gate 通过。
- 可并行步骤：无。
- 不可并行步骤：Step 06 / 07 依赖本步骤；本步骤与任何 foreground 主控制流改动互斥。
- 并行安全依据：不适用，本步骤串行。
- 互斥资源 / 冲突路径：`foreground.rs`、realtime supervisor、dispatcher、daemon transport policy、shared `im-core` contract。
- 外部文档或决策：若发现必须修改 `im-core` public API 或 message-service 协议，立即 blocked，回到 Step 04 或新建 Plan 变更。
- 环境前提：message-service WSS 可用或 tests 可用 fake realtime source；daemon tests 可运行。
- 合并前置条件：direct/group event routing tests 通过；session lifecycle 和 shutdown Review 通过；无未授权 shared SDK diff。
- 合并后验证门禁：`cargo test -p awiki-deamon --locked`；如触及 `im-core`，shared SDK gate。

## 7. 验收标准

- [ ] 每个 active agent 只有一个长期 realtime session task 和一个长期 `ImClient`，不按 event 重建 client。
- [ ] 每个 `RealtimeSession::subscribe()` 只有一个 reader task；多业务模块不直接读同一 session。
- [ ] 多 WebSocket events 通过 daemon wrapper fan-in 到统一 channel，source metadata 不进入 `im-core::ImEvent`。
- [ ] direct message event 和 group message event 都能进入现有 runtime dispatcher 或等价抽取函数。
- [ ] group event 不触发全 group 扫描；缺上下文时只调度 targeted fetch / sync。
- [ ] shutdown 能停止 session、reader task 和 coordinator，不泄露 task。
- [ ] 错误日志、audit 和 debug 信息不泄露 token、private key、JWT、E2EE plaintext。
- [ ] 未修改 message-service 协议；未出现未经 Step 04 批准的 `im-core` public API 变更。
- [ ] 本步骤合并前的 Step gate 已通过，或已记录不能运行的具体原因和风险。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入 Step 06 之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 运行时机 | 预期证据 | 门禁类型 |
|---|---|---|---|---|
| Realtime supervisor focused tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked realtime` 或实际测试名 | commit 前 | 多 session fan-in、source、shutdown tests 通过 | Step gate |
| Dispatcher tests | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked runtime` 或 focused message routing tests | commit 前 | direct/group runtime message 路由不回归 | Step gate |
| Daemon unit | `cd awiki-cli-rs2-cpu && cargo test -p awiki-deamon --locked` | commit 前 | crate tests 通过或记录原因 | Step gate |
| Shared SDK diff check | `cd awiki-cli-rs2-cpu && git diff -- crates/im-core` | Review 前 | 无未授权 shared SDK diff | Contract gate |
| Client lifecycle Review | 人工检查 session task 中 `client_for_agent_identity` 调用位置和生命周期 | Review 前 | 不按 event 创建 client；identity sync 不回到高频写 | Review gate |
| Idle 对比 | 复用 Step 01 采样，检查 direct/group inbox 不再 250ms 全量扫描 | Step 05 后 | CPU / I/O / 日志对比 | Evidence |
| Optional live smoke | 在可控环境向一个 runtime agent 发 direct/group test message，观察 realtime event route | commit 前或 Step 07 前 | message 触发 runtime，不需要 250ms poll | Integration evidence |

如 live WSS 环境不可用，必须用 fake source tests 覆盖 supervisor 逻辑，并在 Step 06 / 07 继续补系统证据。

## 9. Review 环节

- Review 时机：supervisor、dispatcher、tests 完成后、commit 前。
- Review 重点：并发生命周期、单 reader、backpressure、shutdown、client 复用、message 幂等、group routing、不泄露 secret、shared API 守门。
- Review 必须对照 Step 04 结论，确认没有擅自新增 `im-core` API 或改变 `ImEvent` DTO。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | TBD | TBD |
| 已修复问题 | TBD | TBD |
| 剩余风险 | TBD | TBD |
| 新增或缺失测试 | TBD | TBD |
| 已更新或缺失文档 | TBD | final 更新 daemon docs。 |
| 并行安全是否仍成立 | no | 本步骤串行。 |
| Agent 是否越界修改 | TBD | TBD |
| 互斥资源是否被修改 | TBD | `foreground.rs` / supervisor 为授权范围。 |
| 合并风险 | TBD | 进入 Step 06 前必须稳定。 |
| Group gate 影响 | 无 | Step 06 串行依赖。 |

## 10. Commit 要求

- Commit 时机：realtime supervisor tests、daemon tests、Review 都完成后。
- Commit 范围：只包含 Step 05 的 realtime supervisor、dispatcher 抽取、transport policy 接入、相关 tests 和台账。
- Commit 前状态：记录 `git status --short --branch`。
- 纳入文件：记录本步骤 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status --short --branch`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- Commit 后必须记录 Step 06 可以启动的前置条件。
- 如果 commit 修改了 `im-core`、message-service 协议或原计划未授权路径，必须先更新主 Plan 并完成对应 Review。
- 建议消息：`daemon: fan in realtime events per agent`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 是否影响并行组 | 是否影响合并门禁 | 下一步决策 |
|---|---|---|---|---|---|---|
| `im-core` realtime 无法在 daemon config 中启动 | `HttpOnly` 拒绝、endpoint 缺失、编译或 runtime 错误 | 检查 Step 04 结论、transport policy、config mapping | Step 05 / Step 04 | 是 | 是 | 暂停；回到 Step 04 评审 endpoint / API 变更。 |
| WSS event 缺少执行 runtime message 所需上下文 | fake/live event 无法定位 thread/group/message | 使用 `sync_thread_after_async`、`groups().messages_async`、existing history path | Step 05 / 06 | 否 | 是 | 在本步骤记录 targeted fetch 接口，细化由 Step 06 实现。 |
| 多 session task 资源过高 | session 数、CPU、FD、service limit 证据 | session cap、backoff、lazy start、低频 fallback | Step 05 / 06 / 07 | 否 | 是 | 更新 Plan，加入配置或保守默认。 |
| Dispatcher 抽取导致旧 poll fallback 回归 | tests 失败或 live smoke 失败 | 保留旧路径测试，逐步抽取公共函数 | Step 05 | 否 | 是 | 修复后重跑 tests。 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-28 | 创建 Step 05 小 Plan | 主 Plan 拆分要求 | `../plan.md#17-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：session 生命周期 bug 会导致收不到消息或泄露 task；fan-in channel backpressure 错误会丢事件；错误抽取 dispatcher 会回归 runtime command。
- 并行执行风险：本步骤串行，避免 foreground 主控制流冲突。
- 合并冲突风险：高；`foreground.rs`、dispatcher、config 可能被多个后续步骤触及。
- Group gate 失败回退：保留低频 direct/group poll fallback，禁用 realtime 主链路，但不要恢复 250ms 全量扫描为最终方案。
- Agent 交接说明：Step 06 接手时应复用本步骤 source wrapper、session map、status/error 事件和 dispatcher，不重复实现 supervisor。
- 回滚 / 回退：可通过配置或 code path 回退到低频 poll fallback；如 `im-core` transport policy 引起问题，恢复 Step 04 批准前状态。
- 后续文档：Step 07 更新 daemon runtime host architecture，说明 per-agent WSS、fan-in event queue、source metadata 和 dispatcher 边界。
