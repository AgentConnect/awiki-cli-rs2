# Plan：Generic CLI Runtime Plugin 落地计划

状态：draft  
DOC：`codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan`  
Harness：`awiki-harness`  
创建时间：2026-06-01  
恢复指针：执行开始前从 Step 01 开始；恢复时读取本文件、当前 Step 文档、执行台账和 `git status`。

## 1. 目标

- 任务目标：根据 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md`，把 `generic-cli` 设计拆成可执行、可 review、可验证、可逐步提交的落地计划。
- 预期行为：后续执行计划后，Codex CLI 作为 `generic-cli` runtime plugin type 内部的 `driver_id=codex` 落地；Codex/Claude Code/Gemini 不再生成新的持久化 runtime plugin type；消息入口仍按 DID 路由 Runtime Agent。
- 非目标：本计划阶段不修改应用代码；不实现 Claude Code / Gemini CLI driver；不引入 MCP；不把 `worktree-per-task` 宣传为安全边界；不让具体 runtime 持有 DID 私钥或直连 message-service。
- 完成标准：所有步骤都有独立 Step 文档、执行状态字段、路径、验收标准、验证方式、Review 环节和 commit gate；最终保留全局 Review 与远端系统测试验收要求。

## 2. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/AGENTS.md` | 确认 Harness 是多仓库控制面，子仓库仍是实现权威来源。 |
| `awiki-harness/README.md` | 确认读取顺序、文档维护原则和子仓库边界。 |
| `awiki-harness/context/00-context-map.md` | 将任务路由到 Agent Runtime Host、Auth、Message Flow、System Test 领域。 |
| `awiki-harness/context/02-repo-map.md` | 确认当前实现属于 `awiki-cli-rs2` 角色；本 checkout 实际路径为 `codex-plugin-cli-rs2`。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 确认 runtime backend 不持 DID 私钥、不直连 message-service，回调经 daemon local RPC。 |
| `awiki-harness/context/20-rules-index.md` | 确认适用 Documentation、Architecture、Verification 规则。 |
| `awiki-harness/context/30-tools-env.md` | 确认 `cargo test -p awiki-deamon --locked`、workspace cargo 测试和系统测试入口。 |
| `awiki-harness/context/40-verification.md` | 确认本任务最终属于 L2/L3：daemon runtime、auth token、消息发送和系统测试都需要证据。 |
| `awiki-harness/context/50-task-workflow.md` | 确认计划、执行台账、Review 和验证记录要求。 |
| `awiki-harness/context/nodes/agent-runtime-host.node.md` | 确认 `generic-cli` 是 Runtime Plugin Layer 的 CLI family，Codex/Claude/Gemini 是内部 driver/profile。 |
| `awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | 确认 `awiki-deamon` 是 runtime host，不能依赖 `awiki-cli` 内部模块。 |
| `awiki-harness/context/repo-profiles/awiki-system-test.md` | 确认跨服务验收由 `awiki-system-test` 负责。 |
| `awiki-harness/rules/documentation-principles.md` | 确认子仓库 docs 是实现权威，计划文档路径应清晰且链接有效。 |
| `awiki-harness/rules/architecture-principles.md` | 确认跨仓库、身份、消息和 runtime 边界原则。 |
| `awiki-harness/rules/verification-policy.md` | 确认最终报告必须记录命令、结果、失败/跳过、未运行和剩余风险。 |

## 3. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| daemon runtime agent 创建 | `runtime.agent.create` 需要从单字符串 plugin type 改为结构化 resolution，并保存 CLI `driver_id`。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/commands/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/agent/mod.rs` |
| daemon state schema | 需要新增 `cli_runtime_profile`、CLI run metadata、可能的 outbound message 结果表，并迁移 legacy `runtime.cli.*`。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs` |
| runtime host | 需要按 `runtime_plugin_id=generic-cli` 加载 CLI profile，再选择 driver；消息路由仍按 `agent_did`。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/host.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/foreground.rs` |
| generic-cli plugin | 需要从 test/command driver 演进为 driver registry、prompt envelope、Codex driver、真实 callback 主链路。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/mod.rs` |
| Hermes 对照实现 | 复用 Hermes 的 daemon wrapper + local RPC 边界，不复制 Hermes native session 为 CLI 首版抽象。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/hermes/*` |
| local RPC / token / outbox | `msg.send` 需要支持授权的非 controller DID/handle、handle resolve、send result/audit；token scope 不能固定 controller。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/local_rpc/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/security/runtime_token.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/outbox/mod.rs` |
| workspace 安全边界 | 需要实现或明确 `shared-root`、`worktree-per-task`、container/sandbox 的行为与边界。 | `codex-plugin-cli-rs2/crates/awiki-deamon/src/workspace/*`、`codex-plugin-cli-rs2/crates/awiki-deamon/docs/local-dev.md` |
| 系统测试 | 需要新增或扩展 daemon acceptance，覆盖 generic-cli alias、Codex fake driver、local RPC、msg.send 和远端系统测试证据。 | `awiki-system-test/tests_v2/daemon/*` |
| 文档 | 需要同步 `generic_cli_runtime_plugin_design.md`、`local-dev.md`、必要时 Harness node/profile。 | `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md`、`codex-plugin-cli-rs2/crates/awiki-deamon/docs/local-dev.md` |

## 4. 假设与开放问题

### 假设

- 本 checkout 的工作区相对路径为 `codex-plugin-cli-rs2`，它承担 Harness 中 `awiki-cli-rs2` 仓库角色。
- 首个可交付 driver 是 Codex；真实 Codex binary 未安装时，单元和集成测试使用 fake `codex` binary 覆盖 command/env/stdin/output 行为。
- `runtime_plugin_id` 字段名在代码中暂不强制重命名，但语义按 runtime plugin type / discriminator 使用。
- 旧数据中的 `runtime.cli.codex`、`runtime.cli.claude-code`、`runtime.cli.gemini-cli` 必须迁移或读取 alias；新写入不得再产生这些值。
- `msg.send` 的真实消息出口是 daemon -> IM Core SDK -> message-service；Memory outbox 只用于测试证据。
- 最终系统测试必须在 `awiki-system-test` 下以 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名执行并记录通过、失败、跳过数量和原因。

### 开放问题

- handle resolve 的稳定入口应优先复用 `im-core` 还是 user-service handle API，需要在 Step 04 实现前确认当前 SDK 暴露面。
- Codex CLI 当前安装版本和 `codex exec` 参数兼容性需要实现时用 `codex --help` 或官方文档再确认；测试不能依赖本机真实安装。
- Codex final fallback 和 `task.finish` 幂等的持久化表名可以在 Step 04/06 实现时细化，但必须保持 run/audit 可追溯。
- remote 系统测试可能受远端配额、OTP、服务状态影响；失败或跳过不能静默忽略，必须记录具体原因。

## 5. 总体设计方法

- 设计边界：daemon core 负责 DID/controller/run/token/outbox/audit；`generic-cli` 负责 CLI family driver 选择和本地进程适配；具体 driver 不直接发送远端消息。
- 关键决策：`generic-cli` 是 runtime plugin type，不是 plugin id、消息 routing key 或对象实例 ID；Codex/Claude/Gemini 通过 `driver_id` 区分。
- 兼容性策略：新写入统一 `runtime_plugin_id=generic-cli`；历史 `runtime.cli.*` 通过迁移或 alias 读成 `generic-cli + driver_id`，并在 audit/detail 中记录 legacy 来源。
- 数据策略：新增 CLI plugin 内部 profile/run 表，避免把 driver-specific 配置塞进 `runtime_profile`；run metadata 记录 route/session/output/workspace 信息。
- 消息策略：消息入口始终按 DID 找 `agent_definition`；`msg.send` 对外发送按 resolved DID，授权同时检查原始 handle 和 resolved DID。
- 安全策略：token 不进 prompt、stdout/stderr、JSONL、final output、transcript 或 Debug；Codex 用户消息通过 stdin prompt envelope 传入，不使用 `AWIKI_DAEMON_TASK_TEXT`。
- 验证策略：每步先本仓 focused cargo 测试；最终做 workspace cargo、daemon acceptance、远端 `awiki-system-test` 完整系统测试。

## 6. 任务拆分

| Step | 标题 | 依赖 | 产出 | 小 Plan 文档 | Commit gate | 状态 |
|---|---|---|---|---|---|---|
| 01 | Runtime resolution 与创建契约 | 无 | `runtime=codex|claude-code|gemini` 解析为 `generic-cli + driver_id` 的契约和测试 | [steps/01-runtime-resolution-contract.md](steps/01-runtime-resolution-contract.md) | 必须 | pending |
| 02 | CLI profile 存储与 legacy 迁移 | 01 | `cli_runtime_profile`、recipient policy、legacy `runtime.cli.*` 迁移/alias | [steps/02-cli-profile-storage-migration.md](steps/02-cli-profile-storage-migration.md) | 必须 | pending |
| 03 | runtime.agent.create 写入 generic-cli profile | 01, 02 | 创建路径保存 `generic-cli` plugin type 和 `driver_id`，Hermes 保持 native type | [steps/03-runtime-create-wiring.md](steps/03-runtime-create-wiring.md) | 必须 | pending |
| 04 | Recipient policy、handle resolve 与 `msg.send` 审计 | 02, 03 | 非 controller 授权收件人、handle resolve、send result/audit、final 幂等基础 | [steps/04-recipient-policy-local-rpc.md](steps/04-recipient-policy-local-rpc.md) | 必须 | pending |
| 05 | Generic CLI driver registry 与真实 callback 主链路 | 03, 04 | runtime host 按 profile 选 driver，真实 CLI 不再依赖 callback 模拟 | [steps/05-generic-cli-driver-host.md](steps/05-generic-cli-driver-host.md) | 必须 | pending |
| 06 | Codex driver MVP | 05 | `CodexDriver`、prompt envelope、stdin、env 注入、fake binary 测试 | [steps/06-codex-driver-mvp.md](steps/06-codex-driver-mvp.md) | 必须 | pending |
| 07 | Workspace instance 与 CLI run metadata | 06 | `shared-root` / `worktree-per-task`、output paths、route/session metadata、fallback final | [steps/07-workspace-run-metadata.md](steps/07-workspace-run-metadata.md) | 必须 | pending |
| 08 | 系统测试、文档收口与全局 Review | 01-07 | daemon acceptance、remote `awiki.info` 系统测试证据、文档同步和最终 Review | [steps/08-system-test-and-docs.md](steps/08-system-test-and-docs.md) | 必须，如有文件变更 | pending |

## 7. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | 分支 | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `feature/release-0526/codex-plugin-cli-rs2` | 2026-06-01 16:59:10 +0800 | 2026-06-01 17:08:31 +0800 | 基线提交：`946d756`；步骤提交：`daemon: resolve cli runtimes to generic cli driver` | 自查无阻塞发现；确认 CLI alias 解析契约与 legacy helper 边界清晰，Hermes native runtime 不产生 `driver_id` | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --test hermes_contracts --locked`：5 passed；`cargo test -p awiki-deamon --test agent_registration_management --locked`：8 passed；`cargo test -p awiki-deamon agent::tests::resolve_runtime --locked`：4 passed；legacy grep 仅命中 legacy helper、legacy 兼容测试和新 legacy metadata 测试 | 启动 Step 02 |
| 02 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 01 |
| 03 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 02 |
| 04 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 03 |
| 05 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 04 |
| 06 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 05 |
| 07 | pending | `feature/release-0526/codex-plugin-cli-rs2` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 06 |
| 08 | pending | `feature/release-0526/codex-plugin-cli-rs2`、可能涉及 `awiki-system-test` | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 等 Step 07 |
| 最终全局 Review | pending | 同上 | 待记录 | 待记录 | 如有文件变更则记录 | 待记录 | 待记录 | 所有步骤 done 后执行 |

## 8. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、当前 Step 文档、执行台账和当前 `git status --short --branch`。
- 同一时间只执行一个步骤，除非本 Plan 明确标记多个步骤彼此独立且可以并行；当前所有步骤按依赖顺序串行。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤依次执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 上一个依赖步骤的完成工作未提交前，不要开始下一个依赖步骤。
- 改变范围、顺序、验收标准、公开契约、数据模型或验证策略前，先更新本 Plan 和对应 Step 文档，并在 Plan 变更记录中登记。

## 8.1 Codex Goal 提示词

```text
请以 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan/plan.md` 为唯一规划入口，按文档实现完整 Generic CLI Runtime Plugin / Codex driver 落地。

开始前先读取：
- `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan/plan.md`
- 当前第一个未 done 的 Step 文档，例如 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan/steps/01-runtime-resolution-contract.md`
- 主 Plan 的执行台账、Codex Goal 执行协议、验证策略、Blocked 处理和 Plan 变更记录
- 当前 `git status --short --branch`

请从第一个状态不是 `done` 的步骤开始，一次只执行一个步骤。每步都要按对应小 Plan 实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填主 Plan 执行台账和 Step 执行状态。需要改变范围、顺序、验收标准、公开契约、数据模型或验证策略时，先更新 Plan 变更记录。

核心注意点：`generic-cli` 是 runtime plugin type，不是 plugin id 或消息 routing key；消息路由始终按 Runtime Agent DID；Codex/Claude/Gemini 只能作为 `generic-cli` 内部 `driver_id`，新数据不得写 `runtime.cli.*`；真实 Codex 回调只走 daemon CLI wrapper + local RPC，不能依赖 `RuntimeLaunchOutcome.callbacks` 模拟主链路；`msg.send` 必须按 profile/run recipient policy 支持授权 DID/handle，并记录 handle resolve、send result 和 audit；真实 Codex run 不得注入 `AWIKI_DAEMON_TASK_TEXT`，token 不得进入 prompt、日志、stdout/stderr、JSONL 或 final output。

所有步骤完成后，执行最终全局 Review 和整体验证；最终系统测试必须在 `../awiki-system-test` 下用 `AWIKI_SYSTEM_TEST_MODE=remote`、`E2E_DID_DOMAIN=awiki.info` 运行并记录实际命令、通过/失败/跳过数量、原因、关键环境配置和最终工作区状态。
```

## 9. 小 Plan 摘要

### Step 01：Runtime resolution 与创建契约

- 小 Plan：[steps/01-runtime-resolution-contract.md](steps/01-runtime-resolution-contract.md)
- 目标：建立 `RuntimeResolution` 契约，明确 CLI alias 写入 `generic-cli + driver_id`。
- 设计方法：先做纯解析和契约测试，不触碰真实注册和 profile 写入。
- 实现方法：扩展或替换 `runtime_plugin_id(runtime)`，让 `commands` 可读取 `args.driver_id`、`driver_config` 和 `recipient_policy`。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/agent/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/commands/mod.rs`、相关测试。
- 验证方式：focused cargo test + 搜索确认新写入契约不再承诺 `runtime.cli.*`。
- Review 环节：重点看 `generic-cli` type 语义、Hermes native type 兼容、错误提示和默认 `driver_id`。
- Commit 要求：完成后一个聚焦 commit。
- 风险：只改解析不改存储时不能单独改变用户可见行为，需在 Step 03 才接入创建路径。

### Step 02：CLI profile 存储与 legacy 迁移

- 小 Plan：[steps/02-cli-profile-storage-migration.md](steps/02-cli-profile-storage-migration.md)
- 目标：新增 CLI plugin 内部 profile、policy 和 run metadata 的存储基础。
- 设计方法：schema migration 与 typed state API 先落地，创建路径后续接入。
- 实现方法：新增 `cli_runtime_profile`、legacy migration/alias、profile CRUD 和测试。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/tests/*`。
- 验证方式：state bootstrap、migration、legacy alias tests。
- Review 环节：重点看 schema 版本、旧库迁移、安全字段和 policy 默认值。
- Commit 要求：完成后一个聚焦 commit。
- 风险：迁移不可破坏 Hermes 与现有 daemon agent。

### Step 03：runtime.agent.create 写入 generic-cli profile

- 小 Plan：[steps/03-runtime-create-wiring.md](steps/03-runtime-create-wiring.md)
- 目标：让当前用户创建 Codex/Claude/Gemini runtime agent 时持久化 `runtime_plugin_id=generic-cli`，并保存对应 `driver_id`。
- 设计方法：把 Step 01/02 接到 `create_runtime_agent`，同时保留 Hermes 初始化路径。
- 实现方法：更新 `RuntimeAgentCreateArgs`、profile id 生成、status payload、audit 和测试预期。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/commands/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/tests/agent_registration_management.rs`。
- 验证方式：创建 alias tests、Hermes create tests、状态 payload 和 audit 不泄漏 token。
- Review 环节：重点看消息按 DID 路由、`generic-cli` 不作为外部 routing key、legacy `runtime.cli.*` 不再新写。
- Commit 要求：完成后一个聚焦 commit。
- 风险：现有系统测试仍用 `test-runtime-uds`，需要兼容 native/test runtime 创建。

### Step 04：Recipient policy、handle resolve 与 `msg.send` 审计

- 小 Plan：[steps/04-recipient-policy-local-rpc.md](steps/04-recipient-policy-local-rpc.md)
- 目标：让 `msg.send` 可以发送给授权的非 controller DID/handle，并记录授权和发送证据。
- 设计方法：policy 存 profile，token scope 由 profile/run policy 生成；handle resolve 先于最终授权和发送。
- 实现方法：扩展 recipient policy、local RPC 授权、outbox send result、audit 和 final 幂等基础。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/local_rpc/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/security/runtime_token.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/outbox/mod.rs`。
- 验证方式：local RPC security tests、Memory outbox tests、ImCore outbox contract tests。
- Review 环节：重点看授权来源、handle/DID 双重检查、token 不泄漏、发送失败不可伪造成成功。
- Commit 要求：完成后一个聚焦 commit。
- 风险：如果当前 SDK 缺少 handle resolver，需要先记录 blocker 或补最小 resolver adapter。

### Step 05：Generic CLI driver registry 与真实 callback 主链路

- 小 Plan：[steps/05-generic-cli-driver-host.md](steps/05-generic-cli-driver-host.md)
- 目标：让 `generic-cli` 在 daemon foreground / runtime host 中按 `driver_id` 调用真实 driver；真实 CLI run 不再依赖 `RuntimeLaunchOutcome.callbacks` 模拟 status/final。
- 设计方法：保留 test driver callbacks 作为测试兼容；真实 `CommandGenericCliDriver` / Codex driver 只走 wrapper + local RPC。
- 实现方法：新增 driver registry / launcher context、注入 socket/token env，删除真实 run 的 `AWIKI_DAEMON_TASK_TEXT`。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/*`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/foreground.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/host.rs`。
- 验证方式：generic-cli focused tests、token/env redaction tests、foreground route tests。
- Review 环节：重点看真实 callback 单链路、Hermes 逻辑未回退、RuntimeLaunchOutcome 兼容边界。
- Commit 要求：完成后一个聚焦 commit。
- 风险：如果 foreground 缺少真实 generic-cli process worker，需要先用 fake UDS driver 建立垂直闭环。

### Step 06：Codex driver MVP

- 小 Plan：[steps/06-codex-driver-mvp.md](steps/06-codex-driver-mvp.md)
- 目标：实现 `CodexDriver`，以 fake binary 测试覆盖 command、stdin prompt、env、输出和 fallback 行为。
- 设计方法：Codex driver 作为 `generic-cli` 内部 driver，不新增 runtime plugin type。
- 实现方法：实现 installation check、prompt envelope、`codex exec` command builder、stdin、`--output-last-message`、JSONL/stdout/stderr 路径。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/codex.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/tests/*`。
- 验证方式：fake `codex` binary integration、secret grep、command arg assertions。
- Review 环节：重点看不使用 `AWIKI_DAEMON_TASK_TEXT`、不默认 `danger-full-access`、不把 token 放 prompt。
- Commit 要求：完成后一个聚焦 commit。
- 风险：Codex CLI 参数可能随版本变更，实现前需确认当前 `codex exec --help` 或官方文档。

### Step 07：Workspace instance 与 CLI run metadata

- 小 Plan：[steps/07-workspace-run-metadata.md](steps/07-workspace-run-metadata.md)
- 目标：为 Codex run 增加 workspace instance、output 文件、route/session metadata 和 fallback final 证据。
- 设计方法：先支持 `shared-root` 和 `worktree-per-task`，container/sandbox 只保留明确接入点。
- 实现方法：新增 workspace preparer、路径校验、run metadata 写入、fallback final 和重复 final 防护。
- 路径：`codex-plugin-cli-rs2/crates/awiki-deamon/src/workspace/*`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/state/mod.rs`、`codex-plugin-cli-rs2/crates/awiki-deamon/src/plugins/generic_cli/*`。
- 验证方式：git worktree fake repo tests、path containment tests、metadata tests、fallback final tests。
- Review 环节：重点看 worktree 不作为安全边界、路径不逃逸 state root、失败保留证据。
- Commit 要求：完成后一个聚焦 commit。
- 风险：非 git workspace 的策略必须显式失败或只读回退，不能静默污染原 workspace。

### Step 08：系统测试、文档收口与全局 Review

- 小 Plan：[steps/08-system-test-and-docs.md](steps/08-system-test-and-docs.md)
- 目标：补齐 daemon acceptance、远端系统测试、文档同步和最终全局 Review。
- 设计方法：系统测试先 focused daemon，再执行 remote full suite；失败/跳过必须按原因记录。
- 实现方法：更新 `awiki-system-test/tests_v2/daemon/*`、`tests_v2/daemon/CLAUDE.md`、daemon docs 和 release validation 记录。
- 路径：`awiki-system-test/tests_v2/daemon/*`、`codex-plugin-cli-rs2/crates/awiki-deamon/docs/*`。
- 验证方式：`cargo test --workspace --locked`、daemon contract wrapper、remote `awiki.info` 完整系统测试。
- Review 环节：全局 Review 覆盖跨步骤契约、测试、文档、安全和工作树状态。
- Commit 要求：如修改系统测试或文档，按仓库创建聚焦 commit。
- 风险：remote 环境不可用时必须记录跳过/失败详情，不得把未验证项写成通过。

## 10. Review 策略

- 每步骤 Review：实现完成后、commit 前进行；优先查正确性、回归、公开契约、数据安全、安全/隐私、测试覆盖、文档漂移和兼容性。
- 全局 Review：Step 08 后执行，范围包括所有变更仓库、runtime create 契约、schema migration、local RPC、Codex driver、system-test、文档和执行台账。
- 契约 / 安全 / 隐私 Review：重点查 DID/controller 授权、runtime token、registration token、DID 私钥、JWT、prompt/stdout/stderr/JSONL/final output 泄漏面。
- 文档 Review：确认 `generic-cli` 仍是 runtime plugin type，不被写成 plugin id 或消息 routing key；确认系统测试证据使用真实日期、命令和结果。

## 11. 验证策略

| 层级 | 命令 / 检查 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | Rust 格式通过。 |
| Unit / crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | daemon crate 所有测试通过。 |
| Workspace | `cd codex-plugin-cli-rs2 && cargo test --workspace --locked` | workspace cargo 测试通过；如失败，记录失败 crate 和原因。 |
| Focused security grep | `cd codex-plugin-cli-rs2 && rg -n "AWIKI_DAEMON_TASK_TEXT|rtok_|runtime_rpc_token.*println|danger-full-access|dangerously-bypass" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 只允许预期测试/文档命中；生产代码不得泄漏 token 或默认危险 sandbox。 |
| Legacy plugin type grep | `cd codex-plugin-cli-rs2 && rg -n "runtime\\.cli\\.codex|runtime\\.cli\\.claude|runtime\\.cli\\.gemini" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs` | 只允许 legacy migration/alias 测试和兼容说明命中；新写入路径不得命中。 |
| Daemon acceptance | 从 `codex-plugin-cli-rs2` 执行：`cd ../awiki-system-test && AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync python -m pytest tests_v2/daemon/test_awiki_daemon_rust_contracts.py -q -rs` | daemon Rust contract wrapper 通过；跳过需记录原因。 |
| Remote full system test | 从 `codex-plugin-cli-rs2` 执行：进入 `../awiki-system-test`，环境包含 `AWIKI_SYSTEM_TEST_MODE=remote`、`E2E_DID_DOMAIN=awiki.info`、`AWIKI_DAEMON_RUST_REPO=../codex-plugin-cli-rs2`、`CARGO_BUILD_JOBS=1`，命令使用 `uv run --no-sync awiki-system-test`。 | 必须记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。 |
| Docs | `cd codex-plugin-cli-rs2 && find crates/awiki-deamon/docs/cli-plugin/plan -type f -name '*.md' -print`，并检查 Markdown 相对链接 | 所有 plan/step 文档存在，链接和路径不使用本机绝对路径。 |

## 12. 文档更新

- Harness 文档：如果实现改变 Agent Runtime Host 跨仓库摘要或 system-test 入口，检查 `awiki-harness/context/nodes/agent-runtime-host.node.md`、`awiki-harness/context/repo-profiles/awiki-cli-rs2.md` 和 `awiki-harness/context/repo-profiles/awiki-system-test.md` 是否需要同步。
- 子仓库文档：更新 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/generic_cli_runtime_plugin_design.md`、`codex-plugin-cli-rs2/crates/awiki-deamon/docs/local-dev.md`、必要时新增 release validation。
- 系统测试文档：如果修改 `awiki-system-test/tests_v2/daemon`，同步 `awiki-system-test/tests_v2/daemon/CLAUDE.md`。
- 本次生成的任务文档：主计划与 Step 文档位于 `codex-plugin-cli-rs2/crates/awiki-deamon/docs/cli-plugin/plan`。

## 13. Commit 计划

- 每个完成、验证、Review 通过的步骤创建一个聚焦 commit。
- Commit 前记录 `git status --short --branch` 和纳入文件。
- Commit 后记录 commit hash 和工作区状态。
- 如果 Step 08 修改 `awiki-system-test`，在该仓库创建单独聚焦 commit，并在主台账记录对应 hash。
- 只有最终集成确实修改文件时才创建最终集成 commit。
- 不要把所有步骤的修改积累到一个最终大 commit。

## 14. Blocked 处理

| Blocker | Step | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|---|
| Codex CLI 参数与当前版本不兼容 | 06 | `codex exec --help` 或 fake binary contract 差异 | 调整 command builder 或将真实 Codex smoke 标为可选 | 当前步骤 | 更新 Step 06，并记录兼容策略 |
| handle resolver 没有稳定 SDK/API | 04 | `im-core` / user-service 搜索结果 | 先支持 DID allowlist，补最小 handle resolver adapter 或标记 blocker | 当前步骤 | 若用户要求 handle MVP 必须完成，则阻塞 Step 04 |
| remote `awiki.info` 环境不可用或配额耗尽 | 08 | pytest failure/skip summary、HTTP 状态、日志 | 重跑 focused suite，记录服务状态和跳过原因 | 最终验收 | 不得标记 remote full passed；记录剩余风险 |
| schema migration 破坏旧数据 | 02/03 | migration test 或手工旧库 fixture 失败 | 增加兼容 migration 或回退到 read alias | 当前步骤 | 修复后重新 Review |

- 只有依赖允许且风险已记录时，才继续另一个 pending 步骤。
- 只有没有安全假设、回退方案或独立下一步时，才询问用户。

## 15. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-01 | 创建 Generic CLI Runtime Plugin 落地计划 | 用户要求根据设计文档生成执行计划 | 全部 | 是 |

## 16. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| CLI alias 新旧语义混用导致新建 agent 写入 `runtime.cli.*` | Step 01/03 增加解析契约和创建路径测试，grep 新写入路径 | 回退 create wiring commit；保留 storage migration 不启用 |
| `msg.send` 过度放开收件人 | 默认 deny，profile/run allowlist 必填或只允许 controller；handle resolve 后再授权 | 回退 Step 04，恢复 controller-only token scope |
| token 或用户消息泄漏到 env/prompt/log | 禁止 `AWIKI_DAEMON_TASK_TEXT`，prompt 不含 token，Debug redaction 和 grep gate | 回退 Codex driver env 注入，禁用真实 driver |
| Codex fake test 通过但真实 binary 行为不同 | fake binary 覆盖本地契约，真实 smoke 作为可选证据，最终系统测试记录真实结果 | 保留 generic-cli driver，禁用 Codex profile status |
| worktree 创建污染用户 repo | 路径必须在 daemon state root 下；非 git repo 显式失败或只读回退 | 清理 state-root worktree；回退 workspace preparer |
| remote 系统测试不稳定 | 记录具体失败/跳过原因，不把未验证写成通过 | 保留本地/focused 证据，等待环境恢复后重跑 |

## 17. 最终全局 Review 与整体验证

- 触发条件：所有步骤完成、Review、验证并提交后执行。
- Review 范围：`codex-plugin-cli-rs2`、如有修改则包括 `awiki-system-test`；覆盖 runtime create 契约、state migration、local RPC、outbox、Codex driver、workspace metadata、文档和执行台账。
- 重点关注：跨步骤一致性、回归风险、兼容性、安全/隐私、文档漂移、未提交变更、每个步骤 Review 发现是否已解决或记录。
- 整体验证命令 / 检查：至少执行 `cargo fmt --all --check`、`cargo test -p awiki-deamon --locked`、`cargo test --workspace --locked`、daemon acceptance wrapper、remote `AWIKI_SYSTEM_TEST_MODE=remote` 完整系统测试。
- Review 发现：待执行后记录。
- 已修复问题：待执行后记录。
- 剩余风险：待执行后记录。
- 最终证据：待执行后记录实际命令、通过/失败/跳过数量、失败或跳过原因和关键环境配置。
- 最终 `git status`：待执行后记录。
- 如果本阶段修改文件：记录 Review、验证和最终集成 commit。
