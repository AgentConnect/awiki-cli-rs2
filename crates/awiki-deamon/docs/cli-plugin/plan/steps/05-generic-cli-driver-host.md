# Step 05：Generic CLI driver registry 与真实 callback 主链路

主 Plan：[../plan.md](../plan.md)  
Step index：05  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 2026-06-01 17:58:49 +0800 |
| Completed | 2026-06-01 18:09:34 +0800 |
| Commit | 步骤提交：`daemon: route generic cli profiles through driver registry`，hash 以 `git log` 为准 |
| Review evidence | 自查发现并修复 2 项：`GenericCliInvocation` Debug 曾暴露 task text，已改为 redaction；payload 型 `runtime.task.submit` 曾仍 fallback 到 `UdsTestRuntimePlugin`，已让 `generic-cli` 路径加载 `cli_runtime_profile` 和 driver registry。确认 command driver 不注入 `AWIKI_DAEMON_TASK_TEXT`，真实 command driver callback 为空并通过 local RPC 发送 status/final。 |
| Verification evidence | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked`：13 passed；`cargo test -p awiki-deamon --locked foreground`：4 foreground tests passed；`cargo test -p awiki-deamon --test hermes_message --locked`：8 passed；`cargo test -p awiki-deamon --test hermes_gateway --locked`：6 passed + 1 ignored；`cargo test -p awiki-deamon --locked`：29 unit passed，integration 10/13/5/6+1 ignored/8/3/13/2 passed；`git diff --check` 通过；`rg -n "AWIKI_DAEMON_TASK_TEXT" crates/awiki-deamon/src crates/awiki-deamon/tests` 仅命中测试断言。 |
| Next action | 提交 Step 05 后启动 Step 06。 |

## 2. 目标

- 结果：daemon runtime host 可以在 `runtime_plugin_id=generic-cli` 时加载 CLI profile，并按 `driver_id` 选择 driver；真实 CLI run 的 status/final/msg.send 主链路只走 daemon CLI wrapper + local RPC。
- 用户 / 系统可见行为：controller 发给 generic-cli Runtime Agent DID 的消息，会进入该 agent 的 driver，而不是 fallback 到 `UdsTestRuntimePlugin` 或 legacy `runtime.cli.*`。
- 非目标：不实现 Codex driver 参数细节；不删除 test driver callbacks。
- 完成标准：test driver 仍可用；真实 command/generic driver 不再依赖 `RuntimeLaunchOutcome.callbacks` 模拟成功 final；foreground 可路由到 generic-cli plugin。

## 3. 设计方法

- 设计边界：`RuntimePluginRegistry` 看到 `generic-cli` plugin type；`DriverRegistry` 是 generic-cli 内部机制。
- 核心决策：`RuntimeLaunchOutcome.callbacks` 只保留给 test driver 或历史兼容 fallback；真实 driver 进程通过 local RPC 自己产生 side effect。
- 契约 / API / 数据流：
  1. `run_runtime_text_message_with_gateway` 加载 runtime profile。
  2. 如果 `runtime_plugin_id=runtime.hermes`，走 Hermes。
  3. 如果 `runtime_plugin_id=generic-cli`，加载 `cli_runtime_profile`。
  4. Driver registry 根据 `driver_id` 构造 driver。
  5. Plugin launch 注入 socket/token/env，启动 driver。
- 兼容性：`UdsTestRuntimePlugin` 仍用于 `test-runtime-uds` 和系统测试，直到 Step 08 替换/扩展。
- 迁移策略：legacy `runtime.cli.*` 如未迁移，loader 可临时 alias 到 generic-cli profile，但不得新写。
- 风险控制：真实 driver 不再注入 `AWIKI_DAEMON_TASK_TEXT`；用户消息只进入 prompt/stdin。

## 4. 实现方法

1. 将 `plugins/generic_cli/mod.rs` 拆成更清晰模块，例如 `driver.rs`、`command.rs`、`profile.rs`、`registry.rs`。
2. 新增 `GenericCliDriverRegistry`：
   - `test` / `test-runtime-uds` 用于测试。
   - `command` 用于手工 program。
   - `codex` 在 Step 06 接入。
3. 调整 `GenericCliInvocation`：
   - 保留 run/task/workspace/token/socket。
   - 移除真实 command driver 的 `callbacks` 依赖。
   - 不再包含完整 task text env 注入；prompt 内容由后续 Step 06 builder 处理。
4. 修改 `CommandGenericCliDriver`：
   - 注入 `AWIKI_DAEMON_RUN_ID`、`AWIKI_DAEMON_TASK_ID`、`AWIKI_DAEMON_RUNTIME_RPC_TOKEN`、`AWIKI_DAEMON_SOCKET`、`AWIKI_DAEMON_AGENT_DID`、`AWIKI_DAEMON_RUNTIME_PROFILE_ID`、`AWIKI_DAEMON_CLI_WRAPPER`。
   - 不注入 `AWIKI_DAEMON_TASK_TEXT`。
5. 修改 runtime host / foreground：
   - `runtime_plugin_id=generic-cli` 走 generic-cli plugin。
   - unknown/test runtime 仍走 `UdsTestRuntimePlugin`。
6. 增加 tests：
   - real command driver fake process 通过 UDS local RPC 回传 running/finish。
   - fake process 环境中没有 `AWIKI_DAEMON_TASK_TEXT`。
   - `RuntimeLaunchOutcome.callbacks` 对真实 command driver 为空或不作为主链路。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/mod.rs` | 拆分并接 driver registry | 可新增子模块 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/host.rs` | launch context / callback 兼容逻辑 | 确认真实 driver 不需要 callbacks |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/foreground.rs` | `generic-cli` foreground dispatch | Hermes 分支保持 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/cli_wrapper/mod.rs` | wrapper env 或 helper | 与 Hermes 共享 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/generic_cli_runtime_mvp.rs` | 更新 test driver / real command fake tests | 核心测试 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/local_rpc_security.rs` | UDS wrapper 回归 |  |

## 6. 依赖

- 前置步骤：Step 03、Step 04。
- 外部文档或决策：Hermes local RPC 实现和 generic-cli 设计文档回调章节。
- 环境前提：Unix UDS tests 在当前环境可运行；非 Unix 需要 gated。

## 7. 验收标准

- [x] `runtime_plugin_id=generic-cli` 在 foreground 中不再 fallback 到 `UdsTestRuntimePlugin`。
- [x] driver registry 可以按 `driver_id` 选择 driver，并对未知 driver 返回明确错误。
- [x] 真实 command driver 使用 wrapper/local RPC 产生 status/final，不依赖 `RuntimeLaunchOutcome.callbacks` 伪造主链路。
- [x] 真实 command driver env 不包含 `AWIKI_DAEMON_TASK_TEXT`。
- [x] Hermes runtime path 不受影响。
- [x] token/socket 不进入 prompt 文本、Debug 或日志。
- [x] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Generic CLI tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked` | driver registry/callback/env tests 通过。 |
| Foreground tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked foreground` | foreground routing tests 通过。 |
| Daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | 全 crate 通过。 |
| Env grep | `cd codex-plugin-cli-rs2 && rg -n "AWIKI_DAEMON_TASK_TEXT" crates/awiki-deamon/src crates/awiki-deamon/tests` | 生产真实 driver 不命中；如果测试/文档命中需说明。 |

实际验证证据：

- `cargo fmt --all --check`：通过。
- `cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked`：13 passed。
- `cargo test -p awiki-deamon --locked foreground`：4 foreground tests passed。
- `cargo test -p awiki-deamon --test hermes_message --locked`：8 passed。
- `cargo test -p awiki-deamon --test hermes_gateway --locked`：6 passed + 1 ignored。
- `cargo test -p awiki-deamon --locked`：29 unit passed；integration 10/13/5/6 passed + 1 ignored/8/3/13/2 passed；doc-tests 0 passed。
- `git diff --check`：通过。
- Env grep：`AWIKI_DAEMON_TASK_TEXT` 仅命中 `generic_cli_runtime_mvp` 中确认该 env 未注入的测试脚本。

## 9. Review 环节

- Review 时机：driver host 和 tests 完成后、commit 前。
- Review 重点：真实 callback 单链路、test fallback 边界、Hermes 不回退、token/env 泄漏面、driver registry 错误处理。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 已发现 2 项，均已处理 | 1. `GenericCliInvocation` Debug 曾包含 task text；2. payload 型 `runtime.task.submit` 的 generic-cli route 仍可能走 `UdsTestRuntimePlugin` fallback。 |
| 已修复问题 | 已修复 | Debug 改为 `<redacted-task-text>`；payload 和文本消息的 `generic-cli` foreground 路径都加载 `cli_runtime_profile` 并走 `GenericCliDriverRegistry`。 |
| 剩余风险 | 已记录 | Step 05 只注册 `command` 真实 driver；`codex` 返回未安装/未实现占位，Step 06 实现。`CommandGenericCliDriver` 仍把 token 放进子进程 env，这是本地 wrapper 当前契约，后续 Step 06/07 继续收敛。 |
| 新增或缺失测试 | 已新增 | command fake process 通过 UDS local RPC 发送 status/final；env 不含 `AWIKI_DAEMON_TASK_TEXT`；registry command profile；foreground generic-cli 不 fallback；Debug redaction；Hermes gateway/message 回归。 |
| 已更新或缺失文档 | 已更新 | 主 Plan 与本 Step 文档已记录状态、Review 结论和验证证据。 |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：generic-cli host/registry、foreground route、tests。
- Commit 信息建议：`daemon: route generic cli profiles through driver registry`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：去掉 callback 模拟可能让旧 tests 失效。缓解：保留 test driver callbacks，真实 driver 分支单独测试。
- 假设：Codex driver 在 Step 06 才注册；本步骤可注册 placeholder 并返回未安装。
- 回滚：回退本步骤后 generic-cli 仍只能走旧 MVP/test driver，不应继续 Step 06。
