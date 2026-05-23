# Phase 5R：Rust native WebSocket transport 与 Dart realtime 封装落地规划

**状态**：Draft / execution-ready plan  
**日期**：2026-05-23  
**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**目标范围**：`crates/im-core`、`crates/im-core-dart`、`packages/awiki_im_core`  
**明确不改**：本次不修改 `awiki-me` / `awake-me` App 仓库代码。

参考文档：

- [`../modules/11-realtime.md`](../modules/11-realtime.md)：`im-core::realtime` 模块边界。
- [`phase5-realtime-runner-migration-execution-plan.md`](phase5-realtime-runner-migration-execution-plan.md)：Phase 5 runner 迁入 im-core 的主计划。
- [`awiki_im_core_flutter_plan.md`](awiki_im_core_flutter_plan.md)：Flutter/Dart SDK scaffold 与平台打包计划。
- [`../../flutter-sdk/awiki-im-core-flutter-sdk.md`](../../flutter-sdk/awiki-im-core-flutter-sdk.md)：Flutter SDK 用户侧文档入口。

---

## 0. 总体结论

本计划采用“**Rust native WebSocket transport 接入 im-core，Dart 只消费高层 realtime event stream**”作为优先方案。

目标形态：

```text
Flutter App / awake-me / awiki-me
    |
    | 不知道 WebSocket；只监听 SDK event stream / connection state
    v
packages/awiki_im_core
    |
    | Dart API: client.events / client.connectionStates / client.realtime.start-stop-status
    v
crates/im-core-dart
    |
    | flutter_rust_bridge: RealtimeSession + StreamSink<DartImEvent>
    v
crates/im-core
    |
    | RealtimeService / RealtimeRunner / ImEvent projection
    v
internal native WebSocket transport
```

核心原则：

1. **不把 WebSocket 暴露给 App**：App 不拼 `/im/ws`，不依赖 `web_socket_channel`，不解析 raw JSON notification。
2. **尽量不改 im-core public API**：继续使用现有 `ImClient::realtime()`、`RealtimeService`、`RealtimeOptions`、`RealtimeHandle`、`ImEvent`。
3. **优先复用现有 Rust 传输实现经验**：从 `awiki-cli` 的同步 Rust WebSocket transport 迁移/抽取到 `im-core internal`，默认不新增第三方 WebSocket 依赖。
4. **Dart 层只做桥接和生命周期管理**：`im-core-dart` 不重新实现 ANP RPC 或 WebSocket 协议。
5. **本轮不接 awiki-me**：只交付通用 SDK 能力，未来 App 接入作为独立计划。

---

## 1. 当前基线

| 区域 | 当前状态 | 缺口 |
| --- | --- | --- |
| `crates/im-core/src/realtime/*` | 已有 `RealtimeService`、`RealtimeOptions`、`RealtimeHandle`、`ImEvent`、runner/control skeleton | 默认 transport 仍不可用；runner 默认不读取真实 WebSocket notification |
| `crates/im-core/src/internal/realtime/transport.rs` | 已有 endpoint 派生、bearer dial、401 refresh once、transport trait、fake/unavailable transport | 缺少真实 native WebSocket transport 实现 |
| `crates/im-core/src/internal/realtime/projection.rs` | 已能把 direct/group/local/host notification 投影成 `ImEvent` | 需要接入真实 notification source 并补充 live/fixture 覆盖 |
| `crates/awiki-cli/src/runtime/listener_ws_transport.rs` | 已有同步 Rust WebSocket dial/read/write/ping/pong/frame 处理 | 当前在 CLI runtime 内，带 CLI 错误/运行时语义，需要抽取到 im-core internal |
| `crates/im-core-dart/src/api/realtime.rs` | `status()` 可用，`capability()` 暴露当前限制 | `connect()` 明确 unsupported，未暴露 event stream/session |
| `packages/awiki_im_core` | 已有 Flutter/Dart package、native loader、platform build scripts、`RealtimeStatus/Capability` | 缺少 `RealtimeEvent` DTO、Dart stream、session dispose、native runner 生命周期 |

当前最重要的事实：`im-core` 的 **接口形状基本正确**，但真实 WebSocket 连接和 Dart stream bridge 尚未打通。因此本计划不重写接口，而是把实现补齐到接口后面。

---

## 2. 范围与非范围

### 2.1 本次范围

```text
crates/im-core
  - native WebSocket transport internal implementation
  - default_connect / DefaultRunnerTransport 接入真实 transport
  - runner read-loop、heartbeat、reconnect、control shutdown 打通
  - notification -> ImEvent 投影验证

crates/im-core-dart
  - Dart realtime DTO mapping
  - RealtimeSession opaque handle
  - event / connection state stream bridge
  - start/stop/status/capability facade

packages/awiki_im_core
  - Dart public realtime API
  - events / connectionStates stream
  - native-only support；web stub 保持 unsupported
  - Android / iOS / macOS 构建验证

docs / tests
  - plan、SDK 文档、contract tests、fixture tests、platform smoke tests
```

### 2.2 明确非范围

```text
不修改 awiki-me / awake-me 仓库。
不删除 awiki-me 现有 AwikiWsRealtimeGateway。
不迁移 App UI、Provider、Repository。
不把 raw WebSocket frame 暴露到 Dart public API。
不让 Dart package 公开 /im/ws、wss://、Authorization header 细节。
不把 systemd / launchd / Windows service manager 迁入 im-core。
不实现 Flutter Web native realtime；Web 继续走 stub/UnsupportedError。
不在本计划内处理 secure direct decrypt、group E2EE/MLS、attachment-specific realtime enrichment。
```

---

## 3. 目标 API 边界

### 3.1 Rust im-core public API

优先保持现有形态：

```rust
impl ImClient {
    pub fn realtime(&self) -> RealtimeService<'_>;
}

impl RealtimeService<'_> {
    pub fn status(&self) -> ImResult<RealtimeStatus>;
    pub fn connect(&self, options: RealtimeOptions) -> ImResult<RealtimeHandle>;
    pub fn run_until_shutdown(
        &self,
        options: RealtimeOptions,
        shutdown: ShutdownSignal,
    ) -> ImResult<RealtimeExit>;
}
```

允许的内部变化：

```text
DefaultRunnerTransport 从 unavailable stub 换成 native WebSocket notification source。
default_connect 使用 NativeWsRealtimeTransport。
RealtimeHandle 内部可以持有 worker/thread/socket control，但 public 字段语义不变。
RealtimeStatus 可以补充 warnings/last_error，但不暴露 raw socket 对象。
```

不建议新增 public provider trait。若必须抽象 socket/dialer，应放在 `internal::realtime` 或 `compat`，不进 prelude，不承诺 semver。

### 3.2 Dart SDK public API

建议目标形态：

```dart
class AwikiImClient {
  RealtimeApi get realtime;

  /// Normalized IM domain events. The caller does not know whether they came
  /// from WebSocket, polling, local cache, or future transport.
  Stream<ImEvent> get events;

  /// High-level connection state stream.
  Stream<RealtimeConnectionState> get connectionStates;
}

class RealtimeApi {
  Future<RealtimeCapability> capability();
  Future<RealtimeStatus> status();

  /// Starts realtime if supported by this platform/config.
  Future<RealtimeSession> start({RealtimeOptions options = const RealtimeOptions()});

  /// Idempotent stop for the active session.
  Future<void> stop();
}

class RealtimeSession {
  Future<void> stop();
  Future<void> dispose();
}
```

兼容策略：

```text
现有 connect() 可以保留为 start() 的 deprecated/alias wrapper，避免马上破坏已生成 API。
首版如果不想扩 Dart API 太多，也至少需要 start/stop + events stream + connectionStates stream。
```

### 3.3 App/Data 层未来接入契约

本次不改 App，但 SDK 需要为未来接入提供这个契约：

```text
App 只配置：serviceBaseUrl / didDomain / transportPolicy。
App 只监听：client.events / client.connectionStates。
App 只调用：client.realtime.start/stop/status 或由 SDK auto-start。
App 不导入：web_socket_channel。
App 不知道：/im/ws、wss://、Sec-WebSocket-Key、bearer header、ping/pong、reconnect。
```

未来 `awiki-me` 如果继续保留 `RealtimeGateway` domain port，可以新增一个 adapter：

```text
AwikiImCoreRealtimeGateway implements RealtimeGateway
    -> delegates to AwikiImClient.realtime + AwikiImClient.events
```

但该 adapter 属于未来 App 仓库修改，不进入本计划。

---

## 4. Transport policy 语义

`AwikiImCoreConfig.transportPolicy` / `MessageTransportPolicy` 是上层唯一需要知道的 realtime 选择入口。

| Policy | SDK 行为 | App 是否知道 WebSocket |
| --- | --- | --- |
| `httpOnly` | 不启动 native realtime；`start()` 返回 disabled/unsupported 或 no-op status；普通 message/group API 走 HTTP | 否 |
| `auto` | 平台支持且有 auth session 时自动尝试 realtime；失败时保留 HTTP fallback 和 warning/status | 否 |
| `realtimePreferred` | 优先启动 realtime；失败时返回明确 `TransportUnavailable/AuthFailed`，由 App 决定是否继续 | 否 |

规则：

```text
1. policy 不等于 “useWebSocket bool”。它表达业务偏好，而不是传输实现承诺。
2. capability() 只告诉 Dart 当前平台/构建是否支持 realtime runner，不泄漏 socket 实现。
3. 错误类型用 SDK 错误：AuthRequired、AuthFailed、TransportUnavailable、UnsupportedCapability、InvalidInput、Internal。
```

---

## 5. 实施切片

### RWS-0：基线与 fixture 固化

目标：在接入真实 transport 前锁定现有接口和投影行为。

改动范围：

```text
crates/im-core/tests/realtime_api.rs
crates/im-core/tests/realtime_connect.rs
crates/im-core/tests/realtime_runner.rs
crates/im-core/tests/realtime_projection.rs
crates/im-core-dart/tests/facade_contract.rs
packages/awiki_im_core/test/*
```

执行步骤：

```text
1. 补齐当前 unsupported/capability/status 的 contract tests。
2. 为 direct/group/local/host notification 增加 JSON fixture。
3. 增加 “raw WebSocket frame 不进入 public DTO” 的断言。
4. 记录当前 default transport unavailable 行为，后续 RWS-2 修改该断言。
```

验收：

```bash
cargo test -p im-core realtime
cargo test -p im-core-dart facade_contract
flutter test packages/awiki_im_core
```

完成标准：

```text
接口基线清楚；后续把 unavailable 改成 native transport 时，测试变化是有意的。
```

---

### RWS-1：迁移 native WebSocket wire transport 到 im-core internal

目标：把 Rust native WebSocket dial/frame/read/write 能力放入 `im-core` internal，不改变 public API。

优先策略：从现有 CLI 实现迁移/抽取，而不是新增第三方 WebSocket crate。

源：

```text
crates/awiki-cli/src/runtime/listener_ws_transport.rs
crates/awiki-cli/src/runtime/listener_wsclient.rs
```

目标建议：

```text
crates/im-core/src/internal/realtime/ws_transport.rs
crates/im-core/src/internal/realtime/ws_frame.rs
crates/im-core/src/internal/realtime/ws_url.rs
crates/im-core/src/internal/realtime/transport.rs
```

执行步骤：

```text
1. 抽取 URL parse、ws/wss scheme、host/port/path/query 规则。
2. 抽取 HTTP Upgrade request/response 验证、Sec-WebSocket-Accept 计算。
3. 抽取 WebSocket frame read/write、masking、close/ping/pong/text handling。
4. 把 CLI anyhow/context 错误替换为 im-core internal error -> ImError mapping。
5. 保持 rustls/webpki-roots 方案；不要引入 tokio runtime 作为 public 或 required dependency。
6. awiki-cli 原文件暂不删除，可在后续 CLI cleanup 中改为复用 im-core compat。
```

依赖策略：

```text
默认不新增依赖。
如果必须引入 tokio-tungstenite/tungstenite/url 等新依赖，需要先写 ADR：
  - 为什么现有同步 transport 不能满足 Android/iOS/macOS。
  - 对 MSRV 1.78、binary size、TLS roots、mobile packaging 的影响。
  - 如何避免把 async runtime 泄漏进 im-core public API。
```

验收：

```bash
cargo test -p im-core realtime_frame
cargo test -p im-core realtime_connect
rg "anyhow|ParsedCommand|GlobalOptions|config::Resolved|awiki_cli" crates/im-core/src/internal/realtime crates/im-core/tests
```

完成标准：

```text
im-core internal 有 native ws wire 能力；不依赖 CLI 类型；public API 未变化。
```

---

### RWS-2：接入 default_connect 与 auth refresh handshake

目标：让 `RealtimeService::connect()` 从 unavailable stub 切换到 native transport。

改动范围：

```text
crates/im-core/src/internal/realtime/transport.rs
crates/im-core/src/realtime/service.rs
crates/im-core/tests/realtime_connect.rs
```

执行步骤：

```text
1. 保留 realtime_client_endpoints(service_base_url) 的 /im/ws 派生规则。
2. 用 NativeWsRealtimeTransport 替换 default_connect 的 UnavailableRealtimeTransport。
3. 保留 401 -> did-auth refresh once -> retry dial 规则。
4. 连接成功后返回 RealtimeHandle，并发送 Connecting/Connected state event。
5. 连接失败统一映射到 TransportUnavailable/AuthFailed/FatalError。
6. `httpOnly` policy 下不 dial，直接返回明确 disabled/unsupported 状态。
```

测试：

```bash
cargo test -p im-core realtime_connect
cargo test -p im-core realtime_api
```

Manual/live 可选，不作为默认 Codex gate：

```bash
# 需要有效 identity/auth/session 和远端环境时才执行
awiki-cli ... realtime smoke ...
```

完成标准：

```text
fake transport 测试继续通过；default_connect 不再是固定 unavailable；auth refresh 行为被单测覆盖。
```

---

### RWS-3：runner read-loop、heartbeat、reconnect 与 shutdown

目标：让 `run_until_shutdown()` 和 Dart session 能持续消费 WebSocket notification 并输出 `ImEvent`。

改动范围：

```text
crates/im-core/src/realtime/runner.rs
crates/im-core/src/realtime/control.rs
crates/im-core/src/realtime/handle.rs
crates/im-core/src/internal/realtime/heartbeat.rs
crates/im-core/src/internal/realtime/reconnect.rs
crates/im-core/src/internal/realtime/session_loop.rs
crates/im-core/src/internal/realtime/projection.rs
```

执行步骤：

```text
1. 定义 internal notification source：connect -> next_notification -> close/ping。
2. 把 native ws text message 解析为 JSON notification。
3. 用 projection.rs 输出 MessageReceived / GroupUpdated / LocalNotification / HostNotification / UnknownNotification。
4. 实现 event_buffer backpressure 策略：阻塞、drop、warning 必须明确。
5. 实现 heartbeat ping/pong timeout -> reconnect/exit decision。
6. 实现 RealtimeControl.close() 触发 socket shutdown 与 runner exit。
7. 确保 run_until_shutdown 不创建 OS daemon，不持有 CLI service 类型。
```

测试：

```bash
cargo test -p im-core realtime_runner
cargo test -p im-core realtime_projection
cargo test -p im-core realtime_loop
```

完成标准：

```text
fake/native-test transport 可跑 connect -> notification -> ImEvent -> shutdown。
shutdown 可重复调用且不泄漏线程。
reconnect policy 有 deterministic tests。
```

---

### RWS-4：im-core-dart RealtimeSession 与 stream bridge

目标：把 Rust runner 封装成 Dart 可消费的 stream/session，仍不让 Dart 知道 WebSocket。

改动范围：

```text
crates/im-core-dart/src/api/realtime.rs
crates/im-core-dart/src/dto/realtime.rs
crates/im-core-dart/src/dto/message.rs
crates/im-core-dart/src/dto/group.rs
crates/im-core-dart/src/mapping/from_core.rs
crates/im-core-dart/src/mapping/to_core.rs
crates/im-core-dart/tests/facade_contract.rs
```

建议新增 DTO：

```text
DartRealtimeOptions
DartRealtimeSession
DartRealtimeEvent / DartImEvent
DartConnectionStateChanged
DartMessageReceivedEvent
DartMessageUpdatedEvent
DartGroupUpdatedEvent
DartLocalNotificationEvent
DartHostNotificationEvent
DartUnknownNotificationEvent
```

建议 lifecycle：

```text
realtime_start(client, options, event_sink, state_sink) -> DartRealtimeSession
realtime_stop(session) -> ()
realtime_status(client) -> DartRealtimeStatus
realtime_capability(client) -> DartRealtimeCapability
```

线程/FFI 规则：

```text
1. 不在 Flutter UI isolate/blocking thread 上运行长期 loop。
2. DartRealtimeSession 持有 runner control、join handle 或等价 runtime handle。
3. dispose/stop 幂等；drop 时必须尝试关闭 native runner。
4. event sink 关闭时，native runner 应退出或进入 stopped 状态，不能无限重试。
5. 所有 Rust panic/内部错误映射成 DartImError。
```

测试：

```bash
cargo test -p im-core-dart realtime
cargo test -p im-core-dart facade_contract
scripts/flutter/codegen-check.sh
```

完成标准：

```text
Dart facade capability 从 runner_exposed=false 变为 native 平台 true。
connect/start 不再固定 UnsupportedCapability。
事件 DTO 与 im-core ImEvent 语义一致。
```

---

### RWS-5：Flutter package public API 与平台构建

目标：让 `packages/awiki_im_core` 提供通用 realtime API，Android/iOS/macOS 可编译。

改动范围：

```text
packages/awiki_im_core/lib/awiki_im_core.dart
packages/awiki_im_core/lib/src/awiki_im_core_base.dart
packages/awiki_im_core/lib/src/awiki_im_core_native.dart
packages/awiki_im_core/lib/src/awiki_im_core_web_stub.dart
packages/awiki_im_core/lib/src/models/realtime.dart
packages/awiki_im_core/test/*
scripts/flutter/*
docs/flutter-sdk/awiki-im-core-flutter-sdk.md
```

执行步骤：

```text
1. 新增 Dart RealtimeOptions / RealtimeSession / ImEvent model。
2. `AwikiImClient` 暴露 `events` 与 `connectionStates` broadcast stream。
3. `RealtimeApi.start()` 建立 native session 并把 native stream 转发到 client stream。
4. `RealtimeApi.stop()` 幂等关闭 native session。
5. web stub 保持 UnsupportedError，但 public API 可被 analyze。
6. README/SDK 文档说明：transport 是实现细节，App 使用 stream + transportPolicy。
```

平台验证：

```bash
flutter analyze packages/awiki_im_core
flutter test packages/awiki_im_core
scripts/flutter/build-android.sh
scripts/flutter/build-apple.sh
```

如需分开验证：

```bash
scripts/flutter/build-host.sh
scripts/flutter/build-android.sh
scripts/flutter/build-apple.sh --ios
scripts/flutter/build-apple.sh --macos
```

完成标准：

```text
Android/iOS/macOS native library 均可构建。
Dart analyze/test 通过。
Web stub 可 analyze，不声明支持 native realtime。
大型二进制产物继续被 .gitignore 忽略。
```

---

### RWS-6：CLI 复用与去重（可选后续，不阻塞 Dart SDK）

目标：避免 CLI 和 im-core 长期保留两套 WebSocket transport。

本切片不作为 Dart SDK 打通的前置条件。若执行，应遵守 Phase 5 主计划：CLI 只是 runner 宿主，不把 service manager 迁入 im-core。

可能改动：

```text
crates/awiki-cli/src/runtime/listener_ws_transport.rs
crates/awiki-cli/src/runtime/listener_supervisor_run.rs
crates/awiki-cli/src/runtime/listener_im_event_adapter.rs
crates/awiki-cli/src/im_core_adapter/realtime.rs
```

验收：

```bash
cargo test -p im-core realtime_runner
cargo test -p awiki-cli --test runtime_listener_bridge_connection_contract
cargo test -p awiki-cli --test runtime_listener_bridge_dispatch_contract
```

完成标准：

```text
CLI foreground/service-run 可以作为 im-core runner 宿主。
service install/start/stop、pid/log、daemon socket、OpenClaw/Hermes delivery 仍留 CLI。
```

---

## 6. 测试与验证矩阵

### 6.1 Rust unit/contract

```bash
cargo test -p im-core realtime
cargo test -p im-core-dart realtime
cargo test -p im-core-dart facade_contract
```

### 6.2 Import/boundary fence

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|runtime::ListenerConfig|awiki_cli" crates/im-core/src crates/im-core/tests
rg "web_socket_channel|/im/ws|wss://|ws://|Sec-WebSocket" packages/awiki_im_core/lib
```

期望：

```text
im-core 不依赖 CLI 类型。
Dart public package 不暴露 raw WebSocket details；若内部文档或错误消息出现，需要确认不是 public API。
```

### 6.3 Flutter/Dart

```bash
flutter analyze packages/awiki_im_core
flutter test packages/awiki_im_core
scripts/flutter/codegen-check.sh
```

### 6.4 Platform build

```bash
scripts/flutter/build-host.sh
scripts/flutter/build-android.sh
scripts/flutter/build-apple.sh
```

最低验收平台：

```text
Android: arm64-v8a, x86_64
macOS: aarch64-apple-darwin, x86_64-apple-darwin
iOS: aarch64-apple-ios, aarch64-apple-ios-sim
```

### 6.5 Live/system（手动，不作为默认 gate）

仅在有有效远端环境、identity、auth session 时执行：

```text
1. 连接 awiki.info / awiki.ai 目标环境。
2. start realtime session。
3. 发送 direct/group 测试消息。
4. 验证 Dart event stream 收到 MessageReceived / GroupUpdated。
5. 断网/401/session refresh 场景验证 reconnect 和 auth refresh。
```

---

## 7. 回滚策略

```text
1. RWS-1/RWS-2 先保留 UnavailableRealtimeTransport 或 feature flag fallback，一个切片后再删除。
2. Dart package 初期可让 `start()` 根据 capability 返回 UnsupportedCapability，避免误声明支持。
3. `MessageTransportPolicy.httpOnly` 永远可作为绕开 realtime 的 fallback。
4. Native runner 接入失败时，普通 messages/groups HTTP API 不应受影响。
5. App 仓库未修改，因此可只回滚 SDK dependency/branch，不需要回滚 App 代码。
```

建议 feature/开关：

```text
im-core feature: realtime
Dart capability: runnerExposed/connectSupported
Runtime policy: httpOnly/auto/realtimePreferred
```

---

## 8. 风险与处理

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| 移动端长连接生命周期 | iOS/Android 后台可能暂停 socket | SDK 只提供 start/stop/status；App 生命周期接入放未来 awiki-me 计划 |
| blocking runner 阻塞 UI | Flutter 卡顿 | FRB worker/native thread 执行；Dart 只收 stream |
| TLS/root cert 差异 | Android/iOS/macOS 连接失败 | 复用 rustls/webpki-roots；必要时支持 explicit CA bundle/internal config |
| event backpressure | UI 未消费导致内存/卡死 | `event_buffer` 策略明确；满队列 warning/drop/block 单测覆盖 |
| auth refresh race | 401 后重复刷新或 token 丢失 | 保持 single refresh once 规则；session path 读写测试覆盖 |
| 两套 WS 实现并存 | CLI 与 SDK 行为漂移 | RWS-6 后续收敛；在收敛前用 shared fixtures/contract tests |
| 新依赖扩大 binary | Flutter 包变大、MSRV 风险 | 默认不新增依赖；新增必须 ADR |
| Web 平台误用 | Flutter Web 运行时报错 | conditional import stub + capability false + UnsupportedError |

---

## 9. 完成定义

本计划完成时应满足：

```text
1. im-core 默认 native realtime transport 可建立 WebSocket 连接。
2. RealtimeService::connect / run_until_shutdown 能输出高层 ImEvent。
3. im-core-dart 暴露 RealtimeSession 和 event/state stream bridge。
4. packages/awiki_im_core 暴露 Dart event stream，且不泄漏 WebSocket 细节。
5. Android、iOS、macOS native builds 通过。
6. cargo test / flutter analyze / flutter test / codegen-check 通过。
7. awiki-me / awake-me 仓库没有被修改。
8. 文档清楚说明 App 未来只接 SDK event stream，不接 raw WebSocket。
```

---

## 10. 推荐执行顺序

```text
RWS-0  基线与 fixtures
RWS-1  native WebSocket wire transport 迁入 im-core internal
RWS-2  default_connect/auth handshake 接入 native transport
RWS-3  runner read-loop/heartbeat/reconnect/shutdown
RWS-4  im-core-dart RealtimeSession + stream bridge
RWS-5  Flutter package public API + platform builds
RWS-6  CLI 复用/去重（可选后续）
```

首个可交付里程碑：

```text
RWS-1 + RWS-2 + RWS-3：Rust im-core native realtime 可用。
```

Dart SDK 可交付里程碑：

```text
RWS-4 + RWS-5：Flutter native App 可通过 awiki_im_core 监听 realtime event stream。
```

App 集成里程碑（未来，不在本计划）：

```text
在 awiki-me / awake-me 中用 AwikiImClient.events 替换显式 AwikiWsRealtimeGateway。
```
