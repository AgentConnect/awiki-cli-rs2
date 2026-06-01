# Step 03：runtime.agent.create 写入 generic-cli profile

主 Plan：[../plan.md](../plan.md)  
Step index：03  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 2026-06-01 17:22:29 +0800 |
| Completed | 2026-06-01 17:26:45 +0800 |
| Commit | 步骤提交：`daemon: create cli agents through generic cli profiles` |
| Review evidence | 自查无阻塞发现；确认 create path 不再调用旧 `runtime_plugin_id()`，CLI family 新建写 `generic-cli` + `cli_runtime_profile.driver_id`，Hermes native 分支保持 `runtime.hermes`。 |
| Verification evidence | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --test agent_registration_management --locked`：10 passed；`cargo test -p awiki-deamon --test hermes_profile --locked`：3 passed；`cargo test -p awiki-deamon --locked`：27 unit passed，integration 10/6/5/6+1 ignored/8/3/8/2 passed；legacy grep 仅命中 legacy helper、legacy metadata/audit 测试和 migration/fixture。 |
| Next action | Step 04：Recipient policy、handle resolve 与 `msg.send` 审计 |

## 2. 目标

- 结果：`runtime.agent.create` 对 CLI family 新建 runtime agent 时，写入 `runtime_plugin_id=generic-cli`，并在 `cli_runtime_profile` 保存 `driver_id`、config 和 recipient policy。
- 用户 / 系统可见行为：创建 `runtime=codex` 的 Runtime Agent 后，消息按该 agent DID 路由，daemon 命中 agent 后用 `generic-cli` plugin type 分发，再由 `driver_id=codex` 选择 driver。
- 非目标：不启动 Codex run，不实现 workspace worktree，不改 message-service。
- 完成标准：agent 创建测试中 Codex/Claude/Gemini 都不再期望 `runtime.cli.*`，Hermes 仍初始化 Hermes profile。

## 3. 设计方法

- 设计边界：`runtime.agent.create` 是 daemon agent 管理命令，目标是创建 Runtime Agent DID 和本地 profile，不是消息执行。
- 核心决策：`runtime_profile_id` 可以继续由 runtime alias + handle 生成，也可以改用 resolved runtime + driver + handle；只要稳定、可迁移、测试覆盖即可。
- 契约 / API / 数据流：
  1. 解析 payload args。
  2. 用 Step 01 的 `RuntimeResolution` 得到 core `runtime_plugin_id` 和可选 `driver_id`。
  3. 写 `agent_identity`、`agent_definition`、`runtime_profile`。
  4. 如果 `runtime_plugin_id=generic-cli`，写 `cli_runtime_profile`。
  5. 如果 `runtime_plugin_id=runtime.hermes`，执行现有 Hermes profile 初始化。
- 兼容性：`runtime=test-runtime-uds` 可以继续作为 native/test runtime type，用于现有系统测试；不要强行归入 generic-cli。
- 迁移策略：创建路径只写新语义；旧数据由 Step 02 处理。
- 风险控制：status payload 和 audit 不能包含 registration token 原文。

## 4. 实现方法

1. 更新 `RuntimeAgentCreateArgs`：
   - `driver_id: Option<String>`
   - `driver_config: Option<Value>`
   - `recipient_policy: Option<Value>`
   - 可选 `workspace_mode`
2. 在 `create_runtime_agent` 中使用 `resolve_runtime`，禁止直接用旧 `runtime_plugin_id(runtime)` 推导 CLI family。
3. 对 `generic-cli`：
   - `RuntimeAgentProfile.runtime_plugin_id = "generic-cli"`
   - `CliRuntimeProfileRecord.driver_id = resolved.driver_id`
   - `recipient_policy_json` 使用 payload 或安全默认值。
4. 对 Hermes 保持现有 `initialize_hermes_profile` 分支。
5. 更新 ready status payload：可包含 `runtime_plugin_id=generic-cli` 和 `driver_id=codex`，但不要把 `generic-cli` 写成消息 routing key。
6. 更新 audit detail：记录 `runtime_alias`、`runtime_plugin_id`、`driver_id`、`defaulted_driver_id`、`legacy_runtime_plugin_id`。
7. 修改 `agent_registration_management` 测试预期，并新增 Codex/Gemini alias tests。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/commands/mod.rs` | create args、resolution、profile 写入、status payload/audit | 主要实现面 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/agent/mod.rs` | 如 Step 01 留有 helper，这里接入 | 禁止 CLI family 旧映射 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | 调用 Step 02 profile API | 只接线，不扩 schema |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/agent_registration_management.rs` | 更新 create tests | 重点覆盖 Codex alias |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/hermes_profile.rs` | 确认 Hermes create 仍通过 |  |

## 6. 依赖

- 前置步骤：Step 01、Step 02。
- 外部文档或决策：`generic_cli_runtime_plugin_design.md` 的 Agent create alias 规则。
- 环境前提：registration client mock 可用。

## 7. 验收标准

- [x] `runtime.agent.create(runtime=codex|codex-cli)` 写入 `runtime_plugin_id=generic-cli` 和 `driver_id=codex`。
- [x] `runtime.agent.create(runtime=generic-cli, driver_id=codex)` 可创建 Codex profile。
- [x] `runtime.agent.create(runtime=claude-code|gemini|gemini-cli)` 写入 `runtime_plugin_id=generic-cli` 和对应 `driver_id`。
- [x] `runtime.agent.create(runtime=hermes)` 仍写入 `runtime.hermes`，并初始化 Hermes profile。
- [x] status payload / audit 不包含 registration token 原文。
- [x] 新创建数据不再出现 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli`。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Agent create tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test agent_registration_management --locked` | daemon/runtime create tests 通过。 |
| Hermes profile tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test hermes_profile --locked` | Hermes create/profile 仍通过。 |
| Full daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | daemon crate 全部通过。 |
| Legacy grep | `cd codex-plugin-cli-rs2 && rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini" crates/awiki-deamon/src crates/awiki-deamon/tests` | 只允许 legacy migration/alias 测试和兼容说明命中。 |

## 9. Review 环节

- Review 时机：create path 和测试完成后、commit 前。
- Review 重点：DID 路由语义未被 `generic-cli` 取代、Hermes native path 未破坏、status/audit 字段不误导、不泄漏 token。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 无阻塞发现 | 旧 helper 仍存在，但 create path 已不再调用。 |
| 已修复问题 | 已清除旧 create 期望 | `agent_registration_management` 不再正向期望 `runtime.cli.*` 新写入。 |
| 剩余风险 | profile id 仍使用 runtime alias | 按本 Step 兼容策略允许；后续若要统一 profile id 规则需单独迁移。 |
| 新增或缺失测试 | 已新增 alias coverage | 覆盖 `codex`、`codex-cli`、`generic-cli` 默认 Codex、`claude-code`、`gemini`、`gemini-cli` 和 Hermes 保持不变。 |
| 已更新或缺失文档 | 已更新本 Step 台账 | 主 Plan 执行台账已同步。 |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：create path wiring、tests、必要 docs。
- Commit 信息建议：`daemon: create cli agents through generic cli profiles`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：现有 long-running E2E 使用 `test-runtime-uds`，如果把 unknown runtime 都改成 error 会破坏测试。缓解：保留 native/test runtime fallback。
- 假设：status payload 中暴露 `driver_id` 对 debug 可接受。
- 回滚：回退本步骤可恢复旧 create path；Step 02 schema 可保留未使用。
