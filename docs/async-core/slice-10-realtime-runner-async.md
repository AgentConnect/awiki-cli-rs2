# 切片 10：Realtime Runner 异步化

## 目标

用 Tokio task/session/stream 替换当前 thread/mpsc realtime runner，并使用真正 async WebSocket transport。

本切片保留现有 realtime event DTO、notification projection、reconnect policy 和 heartbeat/status 互补语义。

## 依赖

依赖切片：

```text
slice-02-async-http-transport.md
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
slice-05-messages-async.md
slice-07-groups-async.md
slice-09-e2ee-secure-async.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/realtime/**
crates/im-core/src/internal/realtime/**
crates/im-core/src/internal/realtime/ws_transport.rs
crates/im-core/src/internal/realtime/projection.rs
crates/im-core/src/internal/realtime/reconnect.rs
crates/im-core/src/internal/realtime/session_loop.rs
```

当前阻塞点：

```text
std::thread::spawn
std::sync::mpsc
std::net::TcpStream
rustls::StreamOwned over std socket
blocking Read/Write
```

## 设计要求

1. Public API 目标：

   ```rust
   let session = client.realtime().start(options).await?;
   let mut events = session.subscribe();
   session.stop().await?;
   ```

2. `RealtimeSession` 最少提供：

   ```text
   event stream or broadcast subscription
   status watch
   stop().await
   join/exit result
   deterministic dispose behavior
   ```

3. WebSocket transport 必须真正 async。

   推荐：

   ```text
   tokio-tungstenite with rustls TLS
   ```

4. 使用 Tokio channels：

   ```text
   bounded mpsc for internal event flow
   broadcast or single-consumer stream for public events
   watch for status
   CancellationToken for shutdown
   ```

5. Realtime degraded 不破坏 HTTP request/response APIs。

6. Heartbeat/status API 保持可用。

7. Secure notification normalization 使用 async-compatible secure path。

8. Local projection 使用 DB actor。

## 执行步骤

1. 新增 async WebSocket transport。

   保留当前 handshake/header/auth 行为，或用成熟库实现等价行为。

2. 将 realtime runner loop 改为 async select loop。

   覆盖：

   ```text
   connect
   connected state emit
   read notification
   ping/pong heartbeat
   reconnect delay
   shutdown token
   event backpressure
   transport error mapping
   ```

3. 将 `RealtimeHandle` 替换或迁移为 `RealtimeSession`。

   如果短期保留旧 `RealtimeHandle`，必须标注 legacy 并在切片 13 清理。

4. 将 event channel 从 `std::sync::mpsc` 改为 Tokio channel。

5. 将 local projection 改为 DB actor command。

6. 将 direct/group secure notification normalization 接 async secure path。

7. 添加 fake realtime transport。

   覆盖：

   ```text
   notification sequence
   connect failure
   read timeout
   connection close
   ping timeout
   reconnect success/failure
   buffer full
   shutdown during connect/read
   ```

## 上层同步

如果 realtime public API 改为 `RealtimeSession`，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/realtime.rs
crates/awiki-cli/src/host_runtime/**
crates/im-core-dart/src/api/realtime.rs
crates/im-core-dart/src/dto/realtime.rs
packages/awiki_im_core/lib/src/**
```

CLI host runtime 里已有 listener/service 相关代码，修改时必须保持 CLI service lifecycle 语义。

## 测试

本切片必须运行：

```bash
cargo test -p im-core realtime --locked
cargo test -p im-core websocket --locked
cargo check -p im-core --locked
```

稳定性测试：

```text
- start 发出 connecting/connected
- shutdown 快速完成
- connection closed 根据 policy 退出或重连
- ping timeout 触发重连
- event buffer full 行为确定
- direct message notification 投影到 local state
- group update notification 投影到 local state
- degraded realtime 不破坏 HTTP request/response APIs
```

Grep：

```bash
rg "std::thread::spawn|std::sync::mpsc|std::net::TcpStream|StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/realtime crates/im-core/src/internal/realtime
```

## 验收

```text
1. Realtime 使用 Tokio task/session/stream。
2. WebSocket transport 是 async。
3. event/status/shutdown/reconnect 行为可测试。
4. local projection 使用 DB actor。
5. HTTP fallback 与 heartbeat/status 仍可用。
```

## 完成报告

报告必须包含：

```text
- RealtimeSession API 说明
- channel/backpressure 策略
- reconnect/ping/shutdown 测试结果
- legacy RealtimeHandle 是否保留
- CLI/Dart 同步状态
- grep 围栏结果
```
