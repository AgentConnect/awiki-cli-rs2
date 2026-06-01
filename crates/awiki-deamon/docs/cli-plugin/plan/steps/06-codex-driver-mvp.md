# Step 06：Codex driver MVP

主 Plan：[../plan.md](../plan.md)  
Step index：06  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 待记录 |
| Completed | 待记录 |
| Commit | 待记录 |
| Review evidence | 待记录 |
| Verification evidence | 待记录 |
| Next action | 实现 Codex driver、prompt envelope 和 fake binary tests |

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
- 风险控制：`danger-full-access`、`--dangerously-bypass-approvals-and-sandbox` 默认禁用；只有外部 sandbox 明确启用时可作为显式配置，并进入 audit。

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
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/prompt.rs` | prompt envelope builder | 可与未来 Claude/Gemini 复用 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/registry.rs` | 注册 `codex` | 来自 Step 05 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/generic_cli_runtime_mvp.rs` | 可扩展或拆新 Codex test 文件 | fake binary tests |
| `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md` | 如实现偏离设计，需要同步 | 文档只更新事实 |

## 6. 依赖

- 前置步骤：Step 05。
- 外部文档或决策：Codex CLI 参数需要实现前确认当前版本；如需浏览，只引用官方 OpenAI 文档。
- 环境前提：测试可以创建临时 fake binary 并调整 `PATH` 或 driver `binary_path`。

## 7. 验收标准

- [ ] `CodexDriver::driver_id()` 或等价 registry key 为 `codex`。
- [ ] check install status 能处理 binary 存在/不存在。
- [ ] command builder 生成 `codex exec`、`--cd`、`--sandbox`、`--output-last-message`、stdin `-`。
- [ ] prompt 通过 stdin 传入，不通过 argv。
- [ ] env 注入 local RPC 必需字段，但不注入 `AWIKI_DAEMON_TASK_TEXT`。
- [ ] prompt、stdout/stderr、JSONL、final output、Debug 都不包含 runtime token。
- [ ] 非零 exit 不伪造 success final。
- [ ] 默认不使用 `danger-full-access` 或 bypass sandbox flags。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

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
| 发现问题 | 待记录 |  |
| 已修复问题 | 待记录 |  |
| 剩余风险 | 待记录 |  |
| 新增或缺失测试 | 待记录 |  |
| 已更新或缺失文档 | 待记录 |  |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：Codex driver、prompt/command builder、fake tests、必要 docs。
- Commit 信息建议：`daemon: add codex generic cli driver`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：Codex CLI 参数变化。缓解：实现前确认当前 `codex exec --help` 或官方文档；fake tests 固化本地 contract。
- 假设：MVP 可通过 fake binary 完成自动化验证，真实 Codex smoke 作为可选或手工证据。
- 回滚：回退本步骤后 `driver_id=codex` 可以保持 profile 但 install/run 返回 unsupported，不能继续发布 Codex agent。
