# Step 07：Workspace instance 与 CLI run metadata

主 Plan：[../plan.md](../plan.md)  
Step index：07  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 2026-06-01 18:36:02 +0800 |
| Completed | 2026-06-01 18:56:04 +0800 |
| Commit | 步骤提交：`daemon: track cli run workspace metadata`，hash 以 `git log` 为准 |
| Review evidence | 自查发现并修复 4 项：workspace instance 准备应只作用于 `generic-cli`，避免影响 Hermes/native；`runtime_temp_dir` 必须从 `DaemonConfig` 显式传入，不能从 socket path 推断；无 workspace instance 的 metadata path 需尽量 canonicalize；worktree 测试需要静默 git 输出并检查命令成功。确认 `worktree-per-task` 仍只作为变更隔离，不作为安全边界。 |
| Verification evidence | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --test state_bootstrap --locked`：2 passed；`cargo test -p awiki-deamon --locked codex`：7 passed；`cargo test -p awiki-deamon --test generic_cli_runtime_mvp --locked`：21 passed；`cargo test -p awiki-deamon --locked`：unit 29 passed，integration 10/21/5/6+1 ignored/8/3/13/2 passed；`git diff --check` 通过；secret/sandbox grep 仅命中测试断言、token 类型实现、Hermes fake token、redaction 词表和 `env_remove`；path grep 无本机绝对路径命中；legacy grep 仅命中兼容 alias/migration 和计划说明。 |
| Next action | Step 08：系统测试、文档收口与全局 Review |

## 2. 目标

- 结果：Codex run 记录 workspace instance、route/session metadata、driver command/output paths，并支持 `shared-root` 和 `worktree-per-task` 的明确行为。
- 用户 / 系统可见行为：写任务可以在 daemon-managed worktree 中运行；run 可追溯触发 conversation/controller、输出文件和 fallback final 来源。
- 非目标：不实现完整 container sandbox，不实现长期 native session mapping，不实现 Claude/Gemini native session。
- 完成标准：path containment、git worktree、run metadata、fallback final、重复 final 防护都有测试。

## 3. 设计方法

- 设计边界：workspace isolation 是 daemon/generic-cli 层责任；Codex driver 只接收准备好的 `workspace_instance_path`。
- 核心决策：`worktree-per-task` 是变更隔离，不是安全边界；硬安全边界只能来自 container/sandbox。
- 契约 / API / 数据流：
  1. runtime host 读取 profile workspace binding。
  2. workspace preparer 根据 mode 生成 instance。
  3. generic-cli/Codex driver 使用 instance path。
  4. run metadata 写 `cli_driver_run`。
  5. process 结束后检查 local RPC final；如果没有 final，按 final output / exit code 生成 fallback 并标记来源。
- 兼容性：`shared-root` 保持原有低风险模式；`worktree-per-task` 只在 git repo 可用时创建。
- 迁移策略：已有 profile 未配置 workspace mode 时继续要求 workspace fields 成组出现；新 create 可允许 payload 指定 mode。
- 风险控制：worktree path 必须位于 daemon state root 的 runtime temp/cache 下，禁止路径穿越和覆盖用户目录。

## 4. 实现方法

1. 新增 `WorkspaceInstance` 和 preparer：
   - `workspace_instance_path`
   - `workspace_mode`
   - `is_security_boundary`
   - `cleanup_policy`
   - `base_ref` / `branch_name` / `worktree_path`
2. `shared-root`：直接使用 `workspace_root`，但 audit 记录“不是安全边界”。
3. `worktree-per-task`：
   - 确认 `workspace_root` 是 git repo。
   - 在 `runtime/tmp/worktrees/<workspace_id>/<run_id>` 下创建 worktree。
   - 路径 containment 校验必须以 canonical path 为准。
   - 成功/失败默认保留，后续 cleanup job 再处理。
4. 新增或补齐 `cli_driver_run`：
   - `run_id`
   - `agent_did`
   - `runtime_profile_id`
   - `driver_id`
   - `controller_did`
   - `conversation_id`
   - `route_key`
   - `workspace_instance_path`
   - `command_json`
   - `output_log_path`
   - `final_output_path`
   - `native_session_id`
   - `synthetic_session_id`
   - `status`
5. 实现 route key：
   - `cli:<agent_did>:<controller_did>:<conversation_id-or-no-conversation>:message-run`
6. 实现 fallback final：
   - 优先 local RPC `task.finish`。
   - 若进程成功退出但未 final，读取 `final_output_path` 并发送 fallback final，audit 标记 `fallback_source=codex_output_last_message`。
   - 若进程失败且未 final，发送 failed status 或保持 failed，不伪造 success final。
7. 实现重复 final 防护：
   - 可以通过 run metadata/outbox result 表记录 final sent。
   - 第二次 `task.finish` 不重复发送 controller final，返回幂等结果或明确重复。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/workspace/*` | workspace instance preparer | 如目录不存在则新增 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs` | `cli_driver_run` API、final sent state | 可接 Step 02 schema |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/host.rs` | 运行前准备 workspace，运行后 fallback final |  |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/*` | driver command/output metadata |  |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/generic_cli_runtime_mvp.rs` | workspace/fallback final tests |  |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/*` | 可新增 workspace instance tests |  |

## 6. 依赖

- 前置步骤：Step 06。
- 外部文档或决策：generic-cli 设计文档 Workspace 模型与 Session 策略。
- 环境前提：测试环境有 git；若无 git，worktree tests 需要 gated 或用临时 repo 检查跳过原因。

## 7. 验收标准

- [x] `shared-root` run 记录 workspace instance path 和“非安全边界” audit/metadata。
- [x] `worktree-per-task` 在 daemon state root 下创建唯一 worktree path。
- [x] 非 git workspace 明确失败或只读回退，不静默污染原 workspace。
- [x] path containment 防止 worktree path 逃逸 state root。
- [x] `cli_driver_run` 保存 route/session/output/workspace metadata。
- [x] Codex process 成功但未 final 时，fallback final 只发送一次并标记来源。
- [x] Codex process 失败时不伪造 success final。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Workspace tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked workspace` | workspace mode/path/worktree tests 通过。 |
| Generic/Codex tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked codex` | fallback final/run metadata tests 通过。 |
| Daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | daemon crate 全部通过。 |
| Path grep | 检查生产代码、测试和计划文档中是否出现本机绝对路径、workspace 目录名前缀或 IDE URI。 | 不应引入本机绝对路径；已有历史文档命中需记录并避免复制。 |

## 9. Review 环节

- Review 时机：workspace/run metadata/fallback final 完成后、commit 前。
- Review 重点：路径安全、worktree 清理策略、fallback final 幂等、route/session 与 Hermes 模型方向一致、非安全边界文档准确。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 无阻塞发现 | Step 07 可独立交付；remote/system-test 和文档最终收口属于 Step 08。 |
| 已修复问题 | 已修复 workspace 作用域、runtime temp 来源、metadata path 和测试输出 | workspace preparer 只挂到 `generic-cli`；`RuntimeLaunchContext` 显式携带 `runtime_temp_dir`；`cli_driver_run` 尽量保存 canonical workspace path；git 测试命令静默并检查 success。 |
| 剩余风险 | worktree 成功/失败后默认保留，cleanup job 未实现 | 当前记录 `cleanup_policy=preserve`；container/sandbox 未实现，只保留明确错误。 |
| 新增或缺失测试 | 已新增 workspace/run metadata/fallback tests | 覆盖 shared-root metadata、worktree-per-task runtime tmp containment、fallback final、失败不伪造 final、schema v10。 |
| 已更新或缺失文档 | 已更新本 Step 台账和主 Plan 台账 | 设计文档未在本步骤修改；最终文档收口留到 Step 08。 |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：workspace preparer、run metadata、fallback final/idempotency、tests、必要 docs。
- Commit 信息建议：`daemon: track cli run workspace metadata`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：git worktree 创建失败会阻塞写任务。缓解：明确失败原因；只读 shared-root 可作为受控回退。
- 假设：run metadata 表属于 `generic-cli` plugin 内部，但可由 daemon state API 管理。
- 回滚：回退本步骤后 Codex driver 只能 shared-root 或无 metadata 运行，不应发布写任务默认能力。
