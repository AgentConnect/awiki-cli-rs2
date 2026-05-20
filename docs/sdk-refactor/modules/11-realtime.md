# realtime 模块接口设计

**阅读顺序**：11 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：实时连接、WebSocket 消息处理、notification 标准化和事件流。

## 1. 目标

`realtime` 必须是可嵌入的运行循环，而不是 CLI daemon 本身。同一套 realtime runner 必须同时支持 CLI 后台进程和 App 线程/task。

## 2. 主要职责

- `run(options, shutdown) -> RealtimeExit`。
- `connect(options) -> RealtimeHandle`。
- `consume(session) -> Stream<ImEvent>` 或同步等价接口。
- WebSocket response / notification 分类。
- pending request 路由。
- notification queue。
- ping / heartbeat / reconnect decision。
- direct/group/group-state/mail notification 的 IM 事件投影。
- host notification 领域事件生成。

## 3. 运行模型

- `im-core` 提供 `RealtimeSession` / `RealtimeRunner` 这类长期运行对象，负责 WebSocket 连接、重连、心跳、请求路由、notification 投影和事件输出。
- 调用方拥有进程、线程、task 和生命周期。`im-core` 可以返回一个 async future、事件 stream、channel handle 或同步等价接口，但不决定它跑在后台进程还是 App 线程里。
- CLI：`runtime listener start/install/service-run` 启动或安装后台进程；该进程内部构造 `ImCore` 和路径参数，然后调用 `im-core::realtime` 的运行循环。
- App：App 在自己的线程、tokio task、移动端 runtime task 或前后台生命周期里构造 `ImCore`，然后调用同一个运行循环，并把 `ImEvent` 投递到 UI/store/notification 层。

## 4. 接口草案

```rust
pub struct RealtimeOptions {
    pub reconnect: ReconnectPolicy,
    pub event_buffer: usize,
    pub subscriptions: Vec<RealtimeSubscription>,
}

pub struct RealtimeHandle {
    pub events: ImEventStream,
    pub control: RealtimeControl,
}

pub struct RealtimeService<'a> {
    client: &'a ImClient,
}

impl RealtimeService<'_> {
    pub async fn connect(
        &self,
        options: RealtimeOptions,
    ) -> ImResult<RealtimeHandle>;

    pub async fn run_until_shutdown(
        &self,
        options: RealtimeOptions,
        shutdown: ShutdownSignal,
    ) -> ImResult<RealtimeExit>;
}
```

实际落地时可以选择 stream、callback、channel 或 handle，但必须保持一个原则：`im-core` 提供可运行的 IM realtime engine，CLI/App 选择如何启动和停止它。`send_rpc`、raw `RealtimeSession` 和 WebSocket frame 处理属于内部 transport 层，不作为 SDK 主接口暴露。

## 5. 不负责

- systemd/launchd/Windows service install/start/stop。
- daemonize、pid file、日志文件轮转、进程保活。
- CLI daemon socket 文件路径。
- OpenClaw/Hermes route 配置。
- App 前后台生命周期、UI 线程调度、平台通知权限。

## 6. CLI 边界

CLI 的后台程序只是 realtime runner 的一个宿主：负责进程化、安装、启动、停止、日志和本机 socket；不重新实现 IM realtime 状态机。
