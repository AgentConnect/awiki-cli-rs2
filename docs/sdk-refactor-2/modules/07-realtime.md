# 07-realtime：Phase 5 可嵌入实时运行循环

## 1. 目标

`realtime` 不进入 Phase 1。Phase 5 再把 WebSocket 连接、notification 分类、事件投影和可嵌入 runner 下沉到 `im-core`。

CLI 的 daemon/service 管理永远留在 CLI。

## 2. Phase 5 public API

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

## 3. SDK 负责

```text
WebSocket connect
heartbeat / reconnect decision
notification classification
ImEvent 投影
message/group local projection trigger
```

## 4. CLI 负责

```text
runtime listener install/start/stop/restart/uninstall
foreground/service-run
systemd/launchd/Windows service
pid/log/socket
OpenClaw/Hermes host notification setup
```

## 5. internal only

不暴露：

```text
raw WebSocket frame
request id pending dispatch queue
send_rpc()
listener daemon socket
service manager
```

## 6. 完成判定

Phase 5 完成时，同一套 `im-core::realtime` runner 能被 CLI 后台进程和 App task/thread 复用。
