# Phase 5：realtime runner 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：`docs/sdk-refactor/implementation-playbook.md` 中的 `19. Phase 5：realtime runner`  
**目标**：把 IM realtime engine 迁入 `crates/im-core`，使 CLI 后台进程和 App 都能使用同一套 runner；同时保留 `awiki-cli` 对 systemd/launchd/Windows service、daemon socket、pid/log、OpenClaw/Hermes 的宿主职责。

---

## 1. 总体结论

Phase 5 不迁移 CLI daemon / service 管理。`im-core::realtime` 只负责可嵌入的 IM realtime runner：

```text
WebSocket connect
auth/session refresh for realtime
heartbeat / reconnect
request/response routing
notification classify
notification -> ImEvent projection
event stream / control handle
run_until_shutdown
```

CLI 继续负责：

```text
runtime listener install/start/stop/restart/uninstall
foreground/service-run process
systemd / launchd / Windows service
daemon socket
pid/log
OpenClaw / Hermes setup
host notification sink delivery
```

推荐迁移粒度：

```text
主策略：pure decision / classifier leaf-file 先迁
辅策略：2-5 个强相关 listener_* 文件组成一个切片
禁止：整体迁移 runtime 目录
禁止：迁移 service manager / platform integration
禁止：把 raw WebSocket frame 暴露给 SDK public API
```

Phase 5 core 可以先于 Phase 4 执行，但必须保持 attachment-agnostic：

```text
允许执行顺序：Phase 5 core -> Phase 4 attachments -> Phase 5' attachment enrichment follow-up

Phase 5 core 不调用 client.attachments()。
Phase 5 core 不依赖 attachments::AttachmentInput / AttachmentDestination / DownloadedAttachment。
Phase 5 core 不实现 attachment-specific notification enrichment。
遇到附件类 notification 时，只做 MessageReceived / Unsupported body / metadata content_type / UnknownNotification 级别投影。
Phase 4 完成后，再由独立 Phase 5' 计划回补附件 notification enrichment。
```

---

## 2. 与主方案的关系

`docs/sdk-refactor/modules/11-realtime.md` 已经定义：`realtime` 必须是可嵌入的运行循环，而不是 CLI daemon；CLI 后台进程只是 runner 的一个宿主，不重新实现 IM realtime 状态机。

Phase 5 执行计划把这个目标拆成 PR：

```text
PR 5A：Realtime DTO / service skeleton
PR 5B：WebSocket frame classifier / pending dispatch / notification queue
PR 5C：reconnect / heartbeat / session loop decisions
PR 5D：notification -> ImEvent projection
PR 5E：Realtime transport boundary and connect handshake
PR 5F：RealtimeHandle / runner / run_until_shutdown
PR 5G：CLI listener service-run 接入 SDK runner
PR 5H：local projection / host notification event bridge / compat cleanup
```

如果 Phase 5 在 Phase 4 前执行，PR 5D / 5H 必须使用附件无关投影规则；不要提前引入 AttachmentService 或附件 DTO 依赖。

---

## 3. 进入条件

开始 Phase 5 前，建议满足：

```text
1. P1/P3 message DTO 和 local projection 稳定。
2. Auth ensure/refresh 可由 im-core 或 stable adapter 完成。
3. Direct/group notification 可 normalize 成 Message / Group DTO。
4. CLI listener legacy path 仍可 fallback。
5. im-core 不依赖 awiki-cli。
6. runtime listener 相关 contract tests 当前基线清楚。
7. Phase 4 attachments 不是 Phase 5 core 的进入条件；Phase 5 core 先执行时必须保持 attachment-agnostic。
```

建议进入前检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

---

## 4. 目标目录和 API 形态

建议新增：

```text
crates/im-core/src/realtime/
  mod.rs
  dto.rs
  service.rs
  events.rs
  handle.rs
  runner.rs
  control.rs

crates/im-core/src/internal/realtime/
  mod.rs
  frame.rs
  dispatch.rs
  notification.rs
  projection.rs
  reconnect.rs
  heartbeat.rs
  session_loop.rs
  transport.rs
  shutdown.rs

crates/im-core/src/compat/
  realtime.rs
```

Public API：

```rust
pub struct RealtimeService<'a> {
    client: &'a ImClient,
}

impl RealtimeService<'_> {
    pub fn status(&self) -> ImResult<RealtimeStatus>;

    pub fn connect(
        &self,
        options: RealtimeOptions,
    ) -> ImResult<RealtimeHandle>;

    pub fn run_until_shutdown(
        &self,
        options: RealtimeOptions,
        shutdown: ShutdownSignal,
    ) -> ImResult<RealtimeExit>;
}
```

P5 可以使用 blocking-first / channel-first 模型，不强制 async runtime：

```rust
pub struct RealtimeHandle {
    pub events: RealtimeEventReceiver,
    pub control: RealtimeControl,
}
```

如果需要异步 runtime，必须作为后续 feature 或 internal implementation，不作为 P5 public API 前置。

---

## 5. 进程 / 线程运行模型

Phase 5 的运行模型必须显式区分“进程宿主”和“runner 执行线程”：

```text
1. im-core 不创建 OS daemon 进程，不 fork，不 daemonize。
2. CLI / App 决定在哪个进程里构造 ImCore / ImClient 并调用 realtime runner。
3. awiki-cli runtime listener run 在当前 foreground CLI 进程中运行 runner。
4. awiki-cli runtime listener service-run 在 service manager 启动的 service-run 进程中运行 runner。
5. awiki-cli runtime listener install/start/stop/restart/uninstall 只管理服务进程，不运行 runner。
```

`run_until_shutdown` 的 public contract：

```text
调用方在哪个线程调用 run_until_shutdown，runner 主循环就在哪个线程阻塞运行。
shutdown signal 触发后退出主循环并返回 RealtimeExit。
CLI foreground / service-run 默认使用这个模型。
```

`connect` 的 Phase 5 contract：

```text
connect 建立可控制的 RealtimeHandle，但不要求 im-core 创建独立 OS 进程。
Phase 5 默认不把 worker thread / async runtime 暴露为 public API。
若实现需要内部 worker thread，只能作为 RealtimeHandle 的 internal implementation detail。
调用方只依赖 events receiver 和 control handle，不依赖具体线程模型。
```

CLI 推荐落地：

```text
runtime listener run
  -> 当前 CLI 进程
  -> 当前主线程调用 run_until_shutdown
  -> Ctrl-C / shutdown signal 转成 SDK ShutdownSignal

runtime listener service-run
  -> service manager 启动的 service-run 进程
  -> service-run 主线程调用 run_until_shutdown
  -> SIGTERM / service stop 转成 SDK ShutdownSignal

runtime listener start/stop/install/uninstall
  -> 只管理 service-run 进程
  -> 不构造 realtime runner，不消费 ImEvent
```

---

## 6. Public DTO 建议

```rust
pub struct RealtimeOptions {
    pub reconnect: ReconnectPolicy,
    pub event_buffer: usize,
    pub subscriptions: Vec<RealtimeSubscription>,
}

pub enum RealtimeSubscription {
    Messages,
    Groups,
    Notifications,
    HostNotifications,
}

pub enum ReconnectPolicy {
    Disabled,
    Fixed { delay_ms: u64, max_attempts: Option<u32> },
    Exponential { base_delay_ms: u64, max_delay_ms: u64, max_attempts: Option<u32> },
}

pub enum ImEvent {
    ConnectionStateChanged(ConnectionStateChanged),
    MessageReceived(MessageReceivedEvent),
    MessageUpdated(MessageUpdatedEvent),
    GroupUpdated(GroupUpdatedEvent),
    LocalNotification(LocalNotificationEvent),
    HostNotification(HostNotificationEvent),
    UnknownNotification(UnknownNotificationEvent),
}

pub struct RealtimeExit {
    pub reason: RealtimeExitReason,
    pub reconnect_attempts: u32,
    pub warnings: Vec<String>,
}

pub enum RealtimeExitReason {
    ShutdownRequested,
    ConnectionClosed,
    AuthFailed,
    TransportUnavailable,
    FatalError,
}
```

`HostNotificationEvent` 可以作为 SDK 领域事件存在；真正 delivery sink 仍在 CLI。

不进入 public API：

```text
raw WebSocket frame
request_id dispatch map
pending queue internals
serde_json raw notification as primary event
socket path
pid/log path
system service state
OpenClaw/Hermes route config
```

---

## 7. 通用边界规则

`im-core` 不能直接使用：

```text
ParsedCommand
ExitError
GlobalOptions
config::Resolved
runtime::ListenerConfig
runtime service manager types
systemd / launchd / Windows service types
OpenClaw / Hermes config types
CLI daemon socket path
CLI output envelope
```

允许的迁移方式：

```text
1. awiki-cli wrapper 把 CLI runtime config 转成 RealtimeOptions。
2. im-core internal 使用自己的 frame/event/runner DTO。
3. im-core compat 暂时为 awiki-cli listener service-run 暴露迁移期函数。
4. platform service management 永远留在 awiki-cli。
```

---

## 8. Compat 与 internal trait 规则

Phase 5 可能需要 internal trait：

```text
RealtimeTransport
RealtimeAuthProvider
RealtimeEventSink
RealtimeLocalProjector
RealtimeClock
SleepProvider
```

这些必须是：

```text
internal-only
compat-only
不是 Phase 7 public provider trait
不进入 prelude
不承诺 semver
```

如果 `awiki-cli` 需要调用 `im_core::compat::realtime`：

```text
1. compat API 不进入 prelude。
2. compat API 使用 #[doc(hidden)]。
3. compat API 不作为 SDK semver 稳定 API。
4. 发布独立 crate 前应放入 non-default feature 或清理。
```

---

## 9. 测试分层规则

### 9.1 Required：Codex Goal / 单 PR 必跑

```text
cargo test -p im-core realtime
rg import fence
```

若 PR 涉及当前已有 runtime listener integration tests，使用明确的 test target：

```text
cargo test -p awiki-cli --test runtime_listener_wsclient_contract
cargo test -p awiki-cli --test runtime_listener_session_loop_contract
```

如果某个 target 在当前仓库不存在，文档或 PR prompt 必须标注为“待新增”，不要当作当前已有测试执行。

### 9.2 Optional integration：合并前或本地补跑

```text
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
cargo test -p awiki-cli --test host_runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
```

### 9.3 Manual / live / system：不由默认 Codex Goal 执行

```text
runtime listener install/start/stop/restart/uninstall
runtime listener service-run with real service
real WebSocket connection to message service
systemd / launchd / Windows service tests
OpenClaw / Hermes delivery
real host notification sink
```

只有当某个 PR 明确声明进入系统验证时，才运行 Manual / live / system 测试。

---

## 10. PR 5A：Realtime DTO / service skeleton

### 10.1 目标

建立 realtime public API 形态，不连接真实 WebSocket。

### 10.2 改动范围

```text
crates/im-core/src/realtime/mod.rs
crates/im-core/src/realtime/dto.rs
crates/im-core/src/realtime/service.rs
crates/im-core/src/realtime/events.rs
crates/im-core/src/realtime/handle.rs
crates/im-core/src/core/client.rs
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
crates/im-core/tests/realtime_api.rs
```

### 10.3 执行步骤

```text
1. 新增 RealtimeService。
2. 在 ImClient 上新增 realtime()。
3. 新增 RealtimeOptions / RealtimeHandle / ImEvent / RealtimeExit DTO。
4. status/connect/run_until_shutdown 先返回 UnsupportedCapability 或 stub。
5. 不改 awiki-cli runtime。
6. 增加 public API shape 和 boundary tests。
```

### 10.4 Required 验收

```bash
cargo test -p im-core realtime
rg "ParsedCommand|ExitError|config::Resolved|runtime::ListenerConfig|awiki_cli" crates/im-core/src crates/im-core/tests
```

### 10.5 完成标准

```text
1. RealtimeService API 可编译。
2. 不连接真实 WebSocket。
3. 不引入 CLI runtime 类型。
4. CLI 行为零变化。
```

---

## 11. PR 5B：WebSocket frame classifier / pending dispatch / notification queue

### 11.1 目标

迁移 raw frame 分类、request_id 提取、pending response 路由和 notification queue 纯逻辑。

### 11.2 源和目标

源：

```text
crates/awiki-cli/src/runtime/listener_wsclient.rs
crates/awiki-cli/src/runtime/listener_json_helpers.rs
```

目标：

```text
crates/im-core/src/internal/realtime/frame.rs
crates/im-core/src/internal/realtime/dispatch.rs
crates/im-core/src/internal/realtime/notification.rs
crates/im-core/src/compat/realtime.rs
```

### 11.3 迁移范围

可迁移：

```text
request_id_from_value
int64_from_value
IncomingWsMessage classification
ListenerWsPendingDispatch equivalent
notification queue capacity rules
dropped notification / routed response decisions
build_ws_rpc_request pure builder, if not tied to CLI
```

暂不迁移：

```text
actual WebSocket dial
bearer token refresh side effects
listener status file
local daemon socket
service supervisor
host notification delivery
```

### 11.4 Required 验收

```bash
cargo test -p im-core realtime_frame
cargo test -p awiki-cli --test runtime_listener_wsclient_contract
```

### 11.5 Optional integration

```bash
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
```

### 11.6 完成标准

```text
1. frame classification 和 dispatch 逻辑由 im-core 覆盖测试。
2. awiki-cli 原 listener_wsclient 可 wrapper 或继续兼容。
3. raw frame 不进入 public API。
```

---

## 12. PR 5C：reconnect / heartbeat / session loop decisions

### 12.1 目标

迁移 session loop、backoff、heartbeat 这类纯 decision 逻辑。

### 12.2 源和目标

源：

```text
crates/awiki-cli/src/runtime/listener_session_loop.rs
crates/awiki-cli/src/runtime/listener_supervisor_shutdown.rs
crates/awiki-cli/src/runtime/listener_shutdown_signal.rs
```

目标：

```text
crates/im-core/src/internal/realtime/reconnect.rs
crates/im-core/src/internal/realtime/heartbeat.rs
crates/im-core/src/internal/realtime/session_loop.rs
crates/im-core/src/internal/realtime/shutdown.rs
```

### 12.3 迁移范围

可迁移：

```text
base/max reconnect delay
backoff reset/increment
connect failure retry decision
consume finished decision
shutdown signal decision
heartbeat interval decision
```

暂不迁移：

```text
process supervisor
service status files
child process management
platform-specific service manager
log file rotation
```

### 12.4 Required 验收

```bash
cargo test -p im-core realtime_loop
cargo test -p awiki-cli --test runtime_listener_session_loop_contract
```

如果 `runtime_listener_session_loop_contract` 尚不存在，先新增明确 target，或只跑对应 im-core unit tests，不使用模糊 Cargo filter。

### 12.5 Optional integration

```bash
cargo test -p awiki-cli --test host_runtime_listener_supervisor_shutdown_contract
```

### 12.6 完成标准

```text
1. reconnect/backoff 行为与 legacy 测试一致。
2. no system service code entered im-core。
3. decision logic 可被 RealtimeRunner 复用。
```

---

## 13. PR 5D：notification -> ImEvent projection

### 13.1 目标

把 message/group notification 标准化成 SDK `ImEvent`，但不做 secure decrypt，不投递 host sink。

### 13.2 源和目标

源：

```text
crates/awiki-cli/src/runtime/listener_notification_handler.rs
crates/awiki-cli/src/runtime/listener_notification_consume.rs
crates/awiki-cli/src/runtime/listener_message_records.rs
crates/awiki-cli/src/runtime/listener_local_notifications.rs
crates/awiki-cli/src/runtime/host_notify.rs
```

目标：

```text
crates/im-core/src/internal/realtime/projection.rs
crates/im-core/src/realtime/events.rs
crates/im-core/src/compat/realtime.rs
```

### 13.3 范围

支持：

```text
direct message notification -> MessageReceived
group message notification -> MessageReceived / GroupUpdated
group state notification -> GroupUpdated
local notification normalized event
host notification domain event
unknown notification -> UnknownNotification
attachment-like notification -> generic MessageReceived / Unsupported body / metadata content_type / UnknownNotification
```

暂不支持：

```text
secure direct decrypt
group E2EE MLS event processing
host notification delivery to OpenClaw/Hermes
platform notification permissions
attachment-specific realtime enrichment
```

### 13.4 Required 验收

```bash
cargo test -p im-core realtime_projection
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test group_contract
```

### 13.5 Optional integration

```bash
cargo test -p awiki-cli --test host_runtime_listener_notification_consume_contract
cargo test -p awiki-cli --test host_runtime_notify_contract
```

### 13.6 完成标准

```text
1. notification 可 normalize 成 ImEvent。
2. ImEvent 不暴露 raw payload 作为主业务字段。
3. OpenClaw/Hermes 仍由 CLI 投递。
4. secure notification 未被误处理。
5. Phase 5 core 不依赖 attachments module。
```

---

## 14. PR 5E：Realtime transport boundary and connect handshake

### 14.1 目标

建立 realtime connect 的 internal transport 边界，支持 bearer token refresh / connect handshake，但不接 CLI service-run。

### 14.2 源和目标

源：

```text
crates/awiki-cli/src/runtime/listener_wsclient.rs
crates/awiki-cli/src/runtime/listener_ws_transport.rs
crates/awiki-cli/src/runtime/listener_connect_session.rs
crates/awiki-cli/src/runtime/listener_session_bootstrap.rs
```

目标：

```text
crates/im-core/src/internal/realtime/transport.rs
crates/im-core/src/internal/realtime/session_loop.rs
crates/im-core/src/realtime/service.rs
```

### 14.3 范围

支持：

```text
derive websocket endpoint from ImCoreConfig / discovery
ensure realtime bearer session
connect handshake
401 refresh once
transport unavailable mapping
initial connection state event
```

暂不支持：

```text
system service start/stop
daemon socket
foreground process loop
host notification delivery
runtime mode config write
```

### 14.4 Required 验收

```bash
cargo test -p im-core realtime_connect
cargo test -p awiki-cli --test runtime_listener_wsclient_contract
```

### 14.5 Manual / live / system

```bash
awiki-cli runtime listener run
```

### 14.6 完成标准

```text
1. RealtimeService::connect 能通过 fake transport 单元测试。
2. connect/auth refresh 行为与 legacy decision 一致。
3. 不触发真实 WebSocket 默认测试。
```

---

## 15. PR 5F：RealtimeHandle / runner / run_until_shutdown

### 15.1 目标

实现可嵌入 runner，返回 event stream / control handle，并支持 shutdown。

### 15.2 范围

支持：

```text
blocking channel-based event receiver
control close/shutdown
run_until_shutdown
connect -> consume loop
reconnect policy
event buffer
runner exit reason
```

暂不支持：

```text
async public API
background process spawn
platform service lifecycle
OpenClaw/Hermes delivery
secure event processing
```

### 15.3 目标文件

```text
crates/im-core/src/realtime/handle.rs
crates/im-core/src/realtime/runner.rs
crates/im-core/src/realtime/control.rs
crates/im-core/src/internal/realtime/session_loop.rs
```

### 15.4 Required 验收

```bash
cargo test -p im-core realtime_runner
```

### 15.5 Optional integration

```bash
cargo test -p awiki-cli --test host_runtime_listener_bridge_connection_contract
```

### 15.6 完成标准

```text
1. runner 可用 fake transport 跑 connect -> notification -> event -> shutdown。
2. shutdown 不依赖 CLI process signal 类型。
3. reconnect policy 可单元测试。
4. public API 仍 blocking-first / channel-first。
```

---

## 16. PR 5G：CLI listener service-run 接入 SDK runner

### 16.1 目标

让 CLI listener 的 service-run / foreground run 可以调用 `im-core::realtime` runner，同时 service install/start/stop 仍留在 CLI。

### 16.2 范围

支持：

```text
runtime listener run
runtime listener service-run
foreground process uses SDK runner
status refresh remains CLI
local daemon socket remains CLI
host notification sink remains CLI
```

不迁移：

```text
runtime listener install/start/stop/restart/uninstall
systemd/launchd/Windows service manager
pid/log file management
OpenClaw/Hermes setup
```

### 16.3 awiki-cli 侧改动

```text
crates/awiki-cli/src/runtime/listener_foreground.rs
crates/awiki-cli/src/runtime/listener_supervisor_run.rs
crates/awiki-cli/src/runtime/listener_session_bootstrap.rs
crates/awiki-cli/src/runtime/listener_session_loop.rs
crates/awiki-cli/src/im_core_adapter/realtime.rs
```

### 16.4 Required 验收

```bash
cargo test -p im-core realtime_runner
cargo test -p awiki-cli --test host_runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
```

### 16.5 Manual / live / system

```bash
awiki-cli runtime listener run
awiki-cli runtime listener service-run
```

### 16.6 完成标准

```text
1. CLI listener host process 调 SDK runner。
2. service install/start/stop 未迁入 im-core。
3. feature flag / fallback 可回退 legacy loop。
```

---

## 17. PR 5H：local projection / host notification event bridge / compat cleanup

### 17.1 目标

把 SDK `ImEvent` 接回 local projection 和 CLI host notification delivery，清理已稳定 compat。

### 17.2 范围

支持：

```text
ImEvent -> local message/group projection
ImEvent -> CLI host notification delivery adapter
UnknownNotification logging
event warnings propagation
compat wrapper cleanup
```

暂不支持：

```text
OpenClaw/Hermes 配置迁移
platform notification permissions
secure event decrypt
attachment-specific realtime handling
attachment notification enrichment
```

### 17.3 Required 验收

```bash
cargo test -p im-core realtime_projection
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test store_groups_contract
```

### 17.4 Optional integration

```bash
cargo test -p awiki-cli --test host_runtime_notify_contract
```

### 17.5 完成标准

```text
1. ImEvent 可被 CLI adapter 投递到现有 host notification sink。
2. local projection 不重复写入消息。
3. compat API 中不再需要的 wrapper 已清理或标注待清理。
4. 附件类事件只做 generic projection，不做 attachment-specific enrichment。
```

---

## 18. 错误映射规则

`im-core` 返回：

```text
AuthRequired
SessionExpired
TransportUnavailable
UnsupportedCapability(realtime)
Service
InvalidInput
Internal
```

`awiki-cli` wrapper 映射：

```text
AuthRequired / SessionExpired -> listener auth hint
TransportUnavailable -> runtime listener network hint
UnsupportedCapability -> phase unsupported hint
Service -> existing listener error envelope
```

规则：

```text
1. im-core 不返回 ExitError。
2. im-core 不知道 runtime CLI command 名。
3. im-core 不生成 systemd/launchd/Windows help 文案。
```

---

## 19. 回滚策略

```text
1. im-core realtime new implementation 先落地。
2. awiki-cli listener wrapper 再切过去。
3. feature flag / adapter fallback 保留一个阶段。
4. 出问题时只回滚 listener wrapper 调用点。
5. im-core new runner 可暂时保留但不走默认路径。
```

涉及长期运行进程的回滚规则：

```text
1. 不改变 service install/start/stop 文件。
2. 不改变 daemon socket 协议。
3. 不改变 pid/log 路径。
4. fallback legacy runner 可以继续读取原 listener config。
```

---

## 20. 明确不做事项

Phase 5 不做：

```text
1. 不迁 systemd/launchd/Windows service manager。
2. 不迁 runtime listener install/start/stop/restart/uninstall。
3. 不迁 daemon socket。
4. 不迁 pid/log file lifecycle。
5. 不迁 OpenClaw/Hermes setup。
6. 不迁 host notification sink delivery implementation。
7. 不迁 secure direct decrypt。
8. 不迁 group E2EE/MLS event processing。
9. 不把 raw WebSocket frame 暴露为 public API。
10. 不强制引入 async runtime 作为 public API。
11. 不迁 attachment-specific realtime enrichment；该工作进入 Phase 5' follow-up。
```

---

## 21. 方案核心

Phase 5 的核心是：

```text
先迁纯 WebSocket frame / dispatch / reconnect decision，
再迁 notification -> ImEvent projection，
再实现可嵌入 runner，
最后让 CLI listener service-run 作为 runner 宿主。
```

这样可以把 IM realtime 状态机收敛到 `im-core`，同时保留 CLI 对系统服务、进程、日志、daemon socket 和 host integration 的控制。

如果执行顺序采用 `5 -> 4 -> 5'`，Phase 5 只交付 attachment-agnostic realtime runner；Phase 4 完成附件 canonical DTO 和 send/download 后，再由 `phase5-attachment-enrichment-follow-up-plan.md` 回补附件通知 enrichment。
