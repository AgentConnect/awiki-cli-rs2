# 切片 02：异步 HTTP 和 Transport

## 目标

原地替换 `im-core` 的阻塞 HTTP 边界，并把内部 transport 迁移到 async-first。

本切片不重写消息、群组、身份或附件业务流程；它只替换 I/O 边界，并保留现有 routing、auth challenge retry、JWT capture/persist 和 JSON-RPC payload 语义。

## 依赖

依赖切片：

```text
slice-01-runtime-foundation.md
```

## 当前代码锚点

重点改造现有模块：

```text
crates/im-core/src/internal/http.rs
crates/im-core/src/internal/transport.rs
crates/im-core/src/internal/json_rpc.rs
crates/im-core/src/internal/wire/**
crates/im-core/src/internal/*_runtime*.rs
```

现有阻塞点：

```text
std::net::TcpStream
rustls::StreamOwned over std socket
std::io::Read / Write over socket
std::fs::read for CA bundle
```

## 设计要求

1. HTTP client 必须真正 async。

   推荐：

   ```text
   reqwest with rustls-tls
   或 hyper + hyper-rustls
   ```

   不允许：

   ```text
   native-tls
   openssl
   blocking reqwest client
   spawn_blocking 包住现有 std::net HTTP client 作为生产实现
   ```

2. 保留现有 endpoint routing。

   必须覆盖：

   ```text
   user service endpoint
   message service endpoint
   mail service endpoint
   ANP service endpoint
   content/search endpoint exceptions（如当前代码存在）
   absolute URL passthrough
   ```

3. 保留 auth 行为。

   必须覆盖：

   ```text
   DID WBA signing
   JWT preload
   Authorization header
   401 challenge retry
   token capture
   token persist
   clear stale token
   ```

4. Transport trait 只在内部 async 化。

   推荐方式：

   ```rust
   pub(crate) trait AsyncRpcTransport {
       async fn rpc(&self, endpoint: &str, method: &str, params: Value) -> ImResult<Value>;
   }
   ```

   如果需要在稳定 Rust 中避免 async trait object 问题，可以使用：

   ```text
   - async_trait 宏，仅 pub(crate)
   - concrete transport structs + inherent async methods
   - generic methods returning impl Future
   ```

   公共 API 不暴露这些 trait。

5. 保留 JSON-RPC wire builder。

   不要为了 async 改 JSON payload shape。

## 执行步骤

1. 为 `im-core` 增加 async HTTP 依赖，使用 rustls TLS feature。

2. 将 `internal/http.rs` 原地改造成 async client。

   建议保留当前 `HttpRequest` / `HttpResponse` 语义，先把 `execute` 改为：

   ```rust
   pub(crate) async fn execute(&self, request: HttpRequest) -> ImResult<HttpResponse>
   ```

3. 将 CA bundle 读取改为 async 文件 I/O，或在 client 初始化阶段使用隔离的 blocking worker。

4. 将 `internal/transport.rs` 中的核心 request 执行路径 async 化。

   保留当前类型职责：

   ```text
   CoreHttpTransport
   CorePlainTransport
   AuthenticatedRpcTransport
   RpcTransport
   RestTransport
   AuthenticatedRestTransport
   AttachmentObjectTransport
   RawJsonTransport
   ```

   可以短期同时保留 legacy sync trait，但必须标注迁移用途，并在切片 13 删除或限制为 test-only。

5. 增加 deterministic fake async transports。

   最少包括：

   ```text
   FakeAsyncRpcTransport
   FakeAsyncAuthenticatedRpcTransport
   FakeAsyncRestTransport
   FakeAsyncAttachmentObjectTransport
   ```

6. 为 JSON-RPC payload 增加 golden tests。

   覆盖：

   ```text
   direct send
   group send
   inbox
   history
   mark_read
   auth refresh
   relationship/profile lookup（如现有）
   ```

7. 保留错误映射。

   HTTP status >= 400、JSON-RPC error、serialization error 和 transport unavailable 的错误 code/field 不应漂移。

## 上层同步

本切片如果只改内部 transport，不要求 CLI/Dart 同步。

如果 public service method 被改为 async，则必须改到对应业务切片，不要混在本切片。

## 测试

本切片必须运行：

```bash
cargo test -p im-core transport --locked
cargo test -p im-core json_rpc --locked
cargo check -p im-core --locked
cargo tree -p im-core --locked | rg -i "openssl|openssl-sys|native-tls"
```

稳定性检查：

```bash
rg "std::net::TcpStream|StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
```

允许例外只应位于 legacy sync 兼容模块或测试中，并必须记录。

## 验收

```text
1. 生产 HTTP transport 不再使用阻塞 socket I/O。
2. endpoint routing 与 auth/JWT 行为保持不变。
3. JSON-RPC payload golden tests 通过。
4. 新增依赖没有引入 OpenSSL/native-tls。
5. 未迁移业务 service 的失败已记录到后续切片。
```

## 完成报告

报告必须包含：

```text
- HTTP client 选择和 TLS feature
- sync transport 是否还有临时保留
- 已覆盖的 payload golden cases
- 已运行测试命令和结果
- 阻塞 I/O grep 结果和例外说明
```
