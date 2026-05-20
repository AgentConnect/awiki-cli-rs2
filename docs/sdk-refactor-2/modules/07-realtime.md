# 模块设计：realtime（Phase 2）

## 1. 阶段定位

`realtime` 不进入第一阶段主体迁移。第一阶段只保留接口占位和边界定义。

Phase 2 再迁移：

- WebSocket connect。
- heartbeat/reconnect。
- notification classify。
- pending request routing。
- notification -> `ImEvent` 投影。
- 可嵌入 runner。

## 2. 对外接口预留

```rust
pub struct RealtimeService<'a> {
    client: &'a ImClient,
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

## 3. 领域事件

```rust
pub enum ImEvent {
    ConnectionStateChanged(ConnectionState),
    MessageReceived(MessageEvent),
    MessageUpdated(MessageEvent),
    GroupUpdated(GroupEvent),
    ContactUpdated(ContactEvent),
    RepairHint(RepairHint),
}
```

raw WebSocket frame 和 raw notification JSON 不作为默认事件暴露。

## 4. CLI 边界

CLI 永远保留：

- systemd/launchd/Windows service。
- daemonize。
- pid file。
- daemon socket。
- log file。
- service install/start/stop/restart/uninstall。
- OpenClaw/Hermes host notify 配置。

SDK 只提供可嵌入 runner。

## 5. 第一阶段处理

第一阶段 CLI runtime 命令继续使用旧实现，不阻塞 `im-core` 基础能力拆分。
