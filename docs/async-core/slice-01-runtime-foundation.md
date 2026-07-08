# 切片 01：运行时基础

## 目标

引入异步运行时基础，但不在本切片迁移所有业务 service。

本切片为后续 HTTP、SQLite actor、realtime、attachments、CLI 和 FRB 迁移提供共同基础：

```text
- Tokio runtime dependency
- operation context
- cancellation token
- timeout/deadline defaults
- concurrency limits
- worker 边界约定
```

## 依赖

依赖切片：

```text
slice-00-baseline-and-docs.md
```

## 修改范围

优先修改或新增：

```text
Cargo.toml
crates/im-core/Cargo.toml
crates/im-core/src/internal/runtime/**
crates/im-core/src/internal/mod.rs
crates/im-core/src/config.rs
crates/im-core/tests/**
```

不要在本切片批量修改：

```text
messages/groups/attachments/realtime service public methods
CLI handlers
FRB/Dart API
```

除非编译需要，不要改 public DTO。

## 设计要求

1. Tokio 是明确运行时。

   推荐 feature：

   ```text
   tokio = { features = ["rt", "rt-multi-thread", "macros", "time", "sync", "fs"] }
   tokio-util = { features = ["rt"] } 或等价 cancellation 支持
   ```

2. 新增依赖不得引入 OpenSSL/native-tls。

   如果后续 HTTP/WebSocket 使用 `reqwest` / `tokio-tungstenite`，必须选择 rustls TLS feature。

3. OperationContext 先作为内部能力，不强制改 public API。

   推荐形态：

   ```rust
   pub(crate) struct OperationContext {
       pub(crate) operation_id: OperationId,
       pub(crate) request_id: Option<String>,
       pub(crate) deadline: Option<Instant>,
       pub(crate) cancellation: CancellationToken,
       pub(crate) trace: TraceContext,
   }
   ```

   public API 可以在后续切片内部创建默认 context；只有确实需要外部 cancellation/progress 的 API 再引入 options。

4. Runtime limits 必须集中配置。

   最少包括：

   ```text
   network_limit
   attachment_limit
   db_queue_limit
   crypto_worker_limit
   ```

5. Timeout defaults 必须集中定义。

   最少包括：

   ```text
   connect timeout
   request timeout
   websocket idle timeout
   websocket ping timeout
   attachment transfer timeout
   db request timeout（如使用）
   ```

## 执行步骤

1. 在 workspace 中增加 Tokio 相关依赖，选择不会引入系统 TLS 的 feature。

2. 在 `im-core` 增加运行时基础模块。优先放在现有 `internal` 边界下，例如：

   ```text
   crates/im-core/src/internal/runtime/mod.rs
   crates/im-core/src/internal/runtime/context.rs
   crates/im-core/src/internal/runtime/limits.rs
   crates/im-core/src/internal/runtime/timeout.rs
   crates/im-core/src/internal/runtime/worker.rs
   ```

3. 实现 `OperationId`、`TraceContext` 和 default context builder。

4. 实现 `CancellationToken` 集成。

   可以直接使用 `tokio_util::sync::CancellationToken`，也可以封装成 crate 内部类型；封装必须能 clone、cancel、检查状态。

5. 实现 runtime limits，并为缺省配置提供稳定默认值。

6. 添加单元测试：

   ```text
   - operation id uniqueness
   - cancellation token clone/cancel behavior
   - timeout config default values
   - limits default values
   ```

7. 更新总文档或本切片文档中与实际模块名不一致的描述。

## 上层同步

本切片原则上不修改 CLI 或 Dart。

如果实际增加的 public config 字段影响 CLI config loading，则必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/core_config.rs
packages/awiki_im_core/lib/src/models/*
crates/im-core-dart/src/mapping/*
```

## 测试

本切片必须运行：

```bash
cargo test -p im-core runtime --locked
cargo check -p im-core --locked
cargo tree -p im-core --locked | rg -i "openssl|openssl-sys|native-tls"
```

`cargo check --workspace --locked` 建议运行。若因为后续未迁移 API 暂时失败，必须记录失败位置和后续切片。

## 验收

```text
1. im-core 可以编译或至少 runtime 相关模块测试通过。
2. Tokio runtime 基础可被后续切片引用。
3. 没有改动业务 service 行为。
4. 没有引入 OpenSSL/native-tls。
5. OperationContext 不强制扩散到所有 public API。
```

## 完成报告

报告必须包含：

```text
- 新增依赖及 feature 说明
- 新增 runtime 模块列表
- 已运行测试命令和结果
- cargo tree 依赖兼容性检查结果
- 是否有 workspace 暂时失败及原因
```
