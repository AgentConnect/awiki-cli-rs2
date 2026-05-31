# 切片 09：E2EE / Secure Services 异步安全化

## 目标

让 direct/group E2EE、secure services 和 secure outbox 在 async runtime 下保持事务安全和磁盘优先语义。

本切片不改变 E2EE wire format，不改变密钥材料语义，不暴露 private material。

## 依赖

依赖切片：

```text
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
slice-05-messages-async.md
slice-07-groups-async.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/secure/**
crates/im-core/src/internal/secure_direct/**
crates/im-core/src/internal/group_e2ee/**
crates/im-core/src/internal/store/e2ee_outbox.rs
crates/im-core/src/internal/message_runtime/local_projection.rs
```

## 设计要求

1. 涉及 I/O 或 DB 的 SecureService methods async 化。

2. E2EE session store 使用 DB actor。

3. 每次 E2EE mutation 前从 DB actor 加载最新 session。

4. Send-side encrypted flow：

   ```text
   load latest session
   encrypt
   transactionally update session/outbox
   send over async transport
   update delivery state
   local projection if allowed
   ```

5. Receive-side encrypted flow：

   ```text
   load latest session
   decrypt
   update session
   persist plaintext/projection when allowed
   emit terminal e2ee_error when needed
   update peer failure/outbox state
   ```

6. Crypto-heavy operations 使用 dedicated worker 或 `spawn_blocking` 隔离。

7. 不跨 await 持锁。

8. 不记录 key/JWT/private material。

## 执行步骤

1. 盘点 direct secure 和 group E2EE 的 DB 访问点。

2. 将 session load/save/outbox operations 收口到 DB actor command。

3. 为需要事务一致性的操作增加 actor command：

   ```text
   load_session_for_mutation
   save_session_and_outbox
   mark_outbox_sent
   mark_outbox_failed
   retry_outbox
   drop_outbox
   persist_decrypted_projection
   ```

4. 将 secure send/read/realtime normalization path async-compatible。

5. 将 crypto-heavy block 放入 worker。

   要求：

   ```text
   - worker limit 受 runtime limits 控制
   - cancellation 停止等待，但不承诺回滚已提交服务端请求
   - private material 不进入 log/error
   ```

6. 增加 stale-session 防回归测试。

7. 增加 outbox retry/drop tests。

8. 增加日志/错误敏感信息测试或 grep 检查。

## 上层同步

如果 SecureService public methods 改为 async，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/*
crates/awiki-cli/src/cli_shell/group_e2ee_handlers.rs
crates/awiki-cli/src/cli_shell/msg_handlers.rs
crates/im-core-dart/src/api/secure.rs
packages/awiki_im_core/lib/src/**
```

如果 secure DTO 改变，必须同步 FRB/Dart DTO mapping。

## 测试

本切片必须运行：

```bash
cargo test -p im-core secure --locked
cargo test -p im-core e2ee --locked
cargo test -p im-core outbox --locked
cargo check -p im-core --locked
```

稳定性测试：

```text
- mutation 前重新加载 session
- unsupported version path
- peer error 更新 outbox
- retry/drop outbox operations
- decrypt failure emits terminal e2ee_error when required
- 日志/错误中没有 key/JWT/private material
```

## 验收

```text
1. E2EE session/outbox DB 操作经 DB actor。
2. session mutation 是磁盘优先。
3. 事务关键区段可以同时更新 session/outbox/projection。
4. crypto-heavy work 不阻塞 async runtime。
5. wire format 和 DTO 语义不变。
```

## 完成报告

报告必须包含：

```text
- E2EE DB actor command 列表
- transaction 边界说明
- crypto worker 使用情况
- stale-session/outbox 测试结果
- CLI/Dart 同步状态
```
