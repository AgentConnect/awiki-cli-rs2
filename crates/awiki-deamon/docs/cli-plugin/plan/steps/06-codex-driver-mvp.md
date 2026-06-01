# Step 06：Codex driver MVP

主 Plan：[../plan.md](../plan.md)  
Step index：06  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 2026-06-01 18:11:07 +0800 |
| Completed | 2026-06-01 18:33:17 +0800 |
| Commit | 步骤提交：`daemon: add codex generic cli driver`，hash 以 `git log` 为准 |
| Review evidence | 自查发现并修复 4 项：`driver_config_json.sandbox` 应优先于 profile 默认 sandbox；Codex/command 子进程需要显式移除继承环境中的 `AWIKI_DAEMON_TASK_TEXT`；Codex prompt 需要携带 `message_id` 和 `conversation_id`；本机真实 Codex 安装会影响 foreground 路由测试，已改用 missing binary 固化未安装分支。 |
| Verification evidence | 已确认 `codex exec --help` 支持 stdin `-`、`--cd`、`--sandbox read-only/workspace-write/danger-full-access`、`--json`、`--output-last-message`、`--model`、`--profile`、`--ignore-user-config`、`--ignore-rules`、`--ephemeral`，且危险 bypass flag 存在但未使用；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked codex`：6 passed；`cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked`：19 passed；`cargo test -p awiki-deamon --locked`：unit 29 passed，integration 10/19/5/6+1 ignored/8/3/13/2 passed；`git diff --check` 通过；secret/sandbox grep 仅命中测试断言、token 类型实现、Hermes fake token、redaction 词表和 `env_remove`。 |
| Next action | Step 07：Workspace instance 与 CLI run metadata |

## 2. 目标

- 结果：在 `generic-cli` 插件内部新增 `CodexDriver`，能以 `codex exec` headless non-interactive 模式启动本地进程，并通过 wrapper/local RPC 形成 status/final/msg.send 主链路。
- 用户 / 系统可见行为：`driver_id=codex` 的 profile 可以被检查安装状态、构造 command、通过 stdin 收 prompt，并在 fake binary 测试中完成 run。
- 非目标：不实现 Claude/Gemini driver，不默认真实运行本机 Codex，不启用危险 sandbox。
- 完成标准：fake `codex` binary tests 覆盖 command args、stdin prompt、env 注入、output-last-message、JSONL/stdout/stderr、token redaction 和 exit fallback。

## 3. 设计方法

- 设计边界：Codex driver 是 `generic-cli` 内部 driver；daemon core 仍负责 DID/controller/token/outbox/audit。
- 核心决策：prompt 通过 stdin；token/socket 通过 env 或受控 credential 文件；不向真实 Codex run 注入 `AWIKI_DAEMON_TASK_TEXT`。
- 契约 / API / 数据流：
  1. `CodexDriver::check_install_status` 查找 `codex` binary 和版本。
  2. `PromptEnvelopeBuilder` 生成 Awiki runtime context、controller/message、callback rules 和 safety。
  3. `CodexCommandBuilder` 生成 `codex exec --cd <workspace> --sandbox <mode> --json --output-last-message <path> -`。
  4. driver 启动进程，stdin 写 prompt envelope。
  5. Codex 内部通过 wrapper 产生 local RPC status/final/msg.send。
  6. driver 收集 stdout/stderr/JSONL/final output metadata，并返回 exit code。
- 兼容性：如果真实 Codex 未安装，install status 返回未安装；测试不依赖真实安装。
- 迁移策略：只注册 `driver_id=codex`，不新增 `runtime.cli.codex`。
- 风险控制：MVP 只允许 `read-only`、`workspace-write`，显式拒绝 `danger-full-access` 和 `--dangerously-bypass-approvals-and-sandbox`；外部 sandbox 审计接入延后到后续步骤。

## 4. 实现方法

1. 新增 `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/codex.rs`。
2. 定义 `CodexDriverConfig`：
   - `binary_path`
   - `profile`
   - `model`
   - `sandbox`
   - `ignore_user_config`
   - `ignore_rules`
   - `ephemeral`
   - output/log 目录策略
3. 实现 prompt envelope builder，内容包括：
   - `agent_did`
   - `agent_handle`
   - `runtime_plugin_id=generic-cli`
   - `driver_id=codex`
   - `workspace_id` / `workspace_mode` / `workspace_instance_path`
   - `controller_verified=true`
   - `message_id` / `run_id` / `conversation_id`
   - user message
   - wrapper callback rules
   - safety rules
4. 实现 command builder，校验 Codex sandbox 参数只使用 `read-only`、`workspace-write`，显式禁止默认 `danger-full-access`。
5. 实现 fake binary 测试：
   - fake binary 读取 stdin 并写入 captured prompt 文件。
   - fake binary 检查 env 中有 token/socket/run id，但 prompt 中没有 token。
   - fake binary 可模拟 success、non-zero、missing final。
6. 实现输出文件：
   - `output_log_path`
   - `final_output_path`
   - stdout/stderr capture
   - JSONL event file，作为 observation，不作为授权事实源。
7. 注册 Codex driver 到 Step 05 driver registry。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/codex.rs` | 新增 Codex driver | 主要实现 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/mod.rs` | 注册 `codex`，扩展 `GenericCliInvocation` 消息上下文 | 来自 Step 05 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/foreground.rs` | 固化 generic-cli foreground 未安装分支测试 | 避免本机真实 Codex 安装影响测试预期 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/generic_cli_runtime_mvp.rs` | 可扩展或拆新 Codex test 文件 | fake binary tests |
| `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md` | 如实现偏离设计，需要同步 | 文档只更新事实 |

## 6. 依赖

- 前置步骤：Step 05。
- 外部文档或决策：Codex CLI 参数需要实现前确认当前版本；如需浏览，只引用官方 OpenAI 文档。
- 环境前提：测试可以创建临时 fake binary 并调整 `PATH` 或 driver `binary_path`。

## 7. 验收标准

- [x] `CodexDriver::driver_id()` 或等价 registry key 为 `codex`。
- [x] check install status 能处理 binary 存在/不存在。
- [x] command builder 生成 `codex exec`、`--cd`、`--sandbox`、`--output-last-message`、stdin `-`。
- [x] prompt 通过 stdin 传入，不通过 argv。
- [x] env 注入 local RPC 必需字段，但不注入 `AWIKI_DAEMON_TASK_TEXT`。
- [x] prompt、stdout/stderr、JSONL、final output、Debug 都不包含 runtime token。
- [x] 非零 exit 不伪造 success final。
- [x] 默认不使用 `danger-full-access` 或 bypass sandbox flags。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Codex focused tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked codex` | Codex driver fake binary tests 通过。 |
| Generic CLI tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked` | generic-cli regression 通过。 |
| Daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | daemon crate 全部通过。 |
| Secret/sandbox grep | `cd codex-plugin-cli-rs2 && rg -n "AWIKI_DAEMON_TASK_TEXT|rtok_|runtime_rpc_token.*println|danger-full-access|dangerously-bypass" crates/awiki-deamon/src crates/awiki-deamon/tests` | 生产路径无泄漏；危险 flags 不作为默认。 |

## 9. Review 环节

- Review 时机：Codex driver 和 fake tests 完成后、commit 前。
- Review 重点：Codex 参数正确性、stdin/prompt 安全、token 泄漏面、sandbox 默认、fallback final 是否诚实、driver 不越过 daemon local RPC。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 无阻塞发现 | Step 06 可独立交付；路径收敛、metadata 持久化和 fallback final 审计仍属于 Step 07 范围。 |
| 已修复问题 | 已修复 sandbox 优先级、env 继承、prompt 上下文和测试稳定性 | `driver_config_json.sandbox` 覆盖 profile 默认值；Codex/command 子进程都 `env_remove("AWIKI_DAEMON_TASK_TEXT")`；Codex prompt 包含 `message_id` 和 `conversation_id`；foreground 测试使用 missing binary。 |
| 剩余风险 | 输出路径收敛、run metadata、成功退出但未 `task.finish` 的 fallback final/audit 延后 Step 07 | 当前 Codex driver 按进程 exit code 返回 run status，不把 `RuntimeLaunchOutcome.callbacks` 当真实主链路；fake binary 覆盖契约，未执行真实 Codex smoke。 |
| 新增或缺失测试 | 已新增 Codex fake binary tests | 覆盖 command args、install status、sandbox 拒绝和优先级、stdin prompt/env、本地 RPC status/final、token redaction、非零 exit 不伪造 final。 |
| 已更新或缺失文档 | 已更新本 Step 台账和主 Plan 台账 | 未修改设计文档；实现仍符合 Step 06 范围。 |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：Codex driver、prompt/command builder、fake tests、必要 docs。
- Commit 信息建议：`daemon: add codex generic cli driver`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：Codex CLI 参数变化。缓解：实现前确认当前 `codex exec --help` 或官方文档；fake tests 固化本地 contract。
- 假设：MVP 可通过 fake binary 完成自动化验证，真实 Codex smoke 作为可选或手工证据。
- 回滚：回退本步骤后 `driver_id=codex` 可以保持 profile 但 install/run 返回 unsupported，不能继续发布 Codex agent。
