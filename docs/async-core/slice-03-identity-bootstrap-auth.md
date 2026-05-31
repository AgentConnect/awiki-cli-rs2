# 切片 03：Identity、Bootstrap 和 Auth 异步化

## 目标

将 identity/bootstrap/auth 的 I/O 入口点转换为 async，并为后续 service async 化提供稳定的 client/session 基础。

本切片保持 credential path、identity registry、auth state 和 DID WBA proof 语义不变。

## 依赖

依赖切片：

```text
slice-01-runtime-foundation.md
slice-02-async-http-transport.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/core/mod.rs
crates/im-core/src/core/bootstrap.rs
crates/im-core/src/core/client.rs
crates/im-core/src/identity/**
crates/im-core/src/auth/**
crates/im-core/src/internal/auth/session.rs
crates/im-core/src/internal/identity_*.rs
crates/im-core/src/internal/identity_store.rs
```

## 设计要求

1. `ImCore::open(config, paths).await` 成为 async 初始化入口。

   可以暂时保留 `ImCore::new` 作为兼容 facade，但必须明确：

   ```text
   - 是否仅用于 tests/legacy
   - 是否会在切片 13 移除
   - 是否不会触发阻塞 I/O
   ```

2. `ImCore::client(selector).await` 加载 identity runtime。

3. Bootstrap 中触达 filesystem 或 SQLite actor 的方法 async 化：

   ```text
   validate_paths
   initialize_local_state
   migrate_local_state
   ```

4. Identity registry 中触达文件或网络的方法 async 化：

   ```text
   list
   default_identity
   resolve
   register_handle
   recover_handle
   recover_handle_plan
   plan_default_identity_change
   ```

5. Auth service 和 session provider async 化：

   ```text
   login
   ensure_session
   refresh_session
   status
   ```

6. 不记录 private key、JWT、auth proof private material。

## 执行步骤

1. 为 `ImCore` 增加 async open/client 入口。

   public API 目标：

   ```rust
   let core = ImCore::open(config, paths).await?;
   let client = core.client(IdentitySelector::Default).await?;
   ```

2. 把 `CoreBootstrap` 的文件系统和 DB 操作改为 async。

   SQLite 初始化如果切片 04 尚未完成，可以短期通过明确的 blocking boundary 隔离；切片 04 完成后必须改走 DB actor。

3. 把 identity registry 的文件 I/O 改为 `tokio::fs` 或运行时 worker。

   保留：

   ```text
   registry JSON format
   legacy registry parsing
   default identity path
   credential directory layout
   readiness semantics
   ```

4. 把 identity registration/recovery 中的 network calls 改走 async transport。

5. 把 `FileSessionProvider` 改为 async session provider。

   注意：

   ```text
   - snapshot 不跨 await 持锁
   - auth state parse 错误映射不变
   - refresh_session 使用 async transport
   - JWT persist 行为不变
   ```

6. 更新 identity/auth/bootstrap 相关 tests。

## 上层同步

如果本切片改变 `ImCore::new` / `client` / `bootstrap` / `auth` 的 public signature，则必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/core.rs
crates/awiki-cli/src/m_core_cli_adapter/auth.rs
crates/awiki-cli/src/m_core_cli_adapter/identity.rs
crates/im-core-dart/src/api/core.rs
crates/im-core-dart/src/api/auth.rs
crates/im-core-dart/src/api/identity.rs
packages/awiki_im_core/lib/src/**
```

如果 CLI/Dart 暂未迁移，必须记录为后续切片 11/12 的已知编译失败，不要留下半同步状态。

## 测试

本切片必须运行：

```bash
cargo test -p im-core identity --locked
cargo test -p im-core auth --locked
cargo test -p im-core bootstrap --locked
cargo check -p im-core --locked
```

建议额外运行：

```bash
cargo test -p im-core registry --locked
cargo test -p im-core recovery --locked
```

## 验收

```text
1. ImCore::open 和 ImCore::client async 入口可用。
2. identity/auth/bootstrap 触达 I/O 的路径 async 化。
3. credential path 和 registry compatibility 不变。
4. auth/JWT/private material 不出现在日志或错误 detail 中。
5. 如果上层签名受影响，CLI/Dart 同步或明确登记到后续切片。
```

## 完成报告

报告必须包含：

```text
- open/client/new 的最终兼容策略
- session provider async 化范围
- credential/registry 兼容性测试结果
- 已运行测试命令和结果
- CLI/Dart 是否受影响
```
