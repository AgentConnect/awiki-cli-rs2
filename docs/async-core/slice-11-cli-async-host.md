# 切片 11：CLI 异步宿主

## 目标

将 `awiki-cli` 迁移到 async host，并让 CLI adapter/handlers 调用 `im-core` async APIs。

从用户视角，CLI 仍是“命令运行、等待、打印、退出”的同步体验。CLI JSON output shape 和错误映射必须保持兼容。

## 依赖

依赖切片：

```text
slice-03-identity-bootstrap-auth.md
slice-05-messages-async.md
slice-06-directory-profile-content.md
slice-07-groups-async.md
slice-08-attachments-streaming.md
slice-09-e2ee-secure-async.md
slice-10-realtime-runner-async.md
```

可以分阶段同步；如果某些 im-core domain 尚未 async 化，不要提前改该 handler。

## 当前代码锚点

重点改造：

```text
crates/awiki-cli/src/main.rs
crates/awiki-cli/src/lib.rs
crates/awiki-cli/src/m_core_cli_adapter/**
crates/awiki-cli/src/cli_shell/**
crates/awiki-cli/src/host_runtime/**
```

## 设计要求

1. CLI entrypoint 使用 Tokio runtime：

   ```rust
   #[tokio::main]
   async fn main() {
       std::process::exit(awiki_cli::execute_async().await);
   }
   ```

2. 保留同步命令体验。

3. Parser/rendering 中不触达 I/O 的代码可以保持 sync。

4. `build_im_core` / `build_im_client` async 化。

5. 所有调用 `im-core` async method 的 handler 使用 `.await`。

6. 不跨 await 持有 CLI 内部锁或 borrowed mutable state。

7. 保持 CLI JSON output shape。

8. 保持 error mapping 和 hint 文案，除非异步切换明确需要更新。

## 执行步骤

1. 为 `awiki-cli` 增加 Tokio dependency 或复用 workspace dependency。

2. 新增 `execute_async().await`。

   可以短期保留 `execute()` wrapper，但不能在 async runtime 内用阻塞方式调用 async API。

3. 迁移 core/client build path：

   ```text
   config loading
   ImCore::open.await
   core.client.await
   bootstrap/status calls.await
   ```

4. 按 domain 迁移 handlers：

   ```text
   id/auth
   people/profile
   msg
   group
   secure/group-e2ee
   attachment
   realtime/runtime listener
   content/site/email
   ```

5. 更新 CLI tests 和 snapshots。

6. 确认 CLI docs/help 文案不因 async 迁移漂移。

## 上层同步

本切片本身就是上层同步切片。任何 `im-core` async signature 变更后，CLI 不应长期停留在编译失败状态。

如果某个 CLI command 依赖尚未迁移的 im-core domain，需要：

```text
- 在切片报告中列出 command
- 指向负责的后续切片
- 确认不会影响最终验收
```

## 测试

本切片必须运行：

```bash
cargo test -p awiki-cli --locked
cargo check -p awiki-cli --locked
cargo run -p awiki-cli -- --help
```

CLI 输出稳定性测试至少覆盖：

```text
id list
id current
auth status
msg send
msg inbox
msg history
group create/list/get/members/messages
secure status
```

手动或 fake env 冒烟：

```bash
awiki-cli id list
awiki-cli id current
awiki-cli auth status
awiki-cli msg inbox --limit 5
awiki-cli msg history --with <peer> --limit 5
awiki-cli msg send --to <peer> --text "hello"
awiki-cli group list --limit 5
awiki-cli realtime status
```

## 验收

```text
1. CLI entrypoint 是 async host。
2. CLI adapter/handlers 正确 await im-core async API。
3. CLI JSON output shape 保持兼容。
4. CLI error mapping 保持兼容。
5. cargo test -p awiki-cli 通过，或未迁移 domain 的失败被明确登记。
```

## 完成报告

报告必须包含：

```text
- execute/execute_async 兼容策略
- 已迁移 command domain 列表
- CLI output snapshot 更新说明
- 已运行测试命令和结果
- 尚未迁移的 CLI paths
```
