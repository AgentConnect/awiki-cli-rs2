# Hermes Runtime Plugin 落地总计划

状态：draft  
DOC：`crates/awiki-deamon/docs/hermes-plugin/plan/`  
Harness：`/home/ecs-user/awiki-space/awiki-harness`  
创建日期：2026-05-31  
当前分支：`feature/release-0526/hermes-plugin-cli-rs2`  
恢复位置：Step 07，状态 `review`，准备提交长驻 daemon 集成与诊断

## 1. 目标

- 目标：基于 [../hermes_runtime_plugin_design.md](../hermes_runtime_plugin_design.md)，把 Hermes native runtime 接入拆成可执行、可 review、可验证、可逐步提交的落地计划。
- 预期行为：daemon 可以创建 Hermes Runtime Agent，初始化 Hermes profile 和 Awiki Skills，通过 Hermes TUI Gateway 投递 controller 消息，接收 Hermes 经 daemon CLI wrapper/local RPC 回传的状态、最终回复和真实 ANP direct/direct-e2ee 外发消息，并持久化 Hermes native session。
- 完成标准：所有阶段实现、验证、代码 review 和聚焦提交完成；最终在 `../awiki-system-test` 执行完整系统测试，使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info` 域名，记录实际命令、通过/失败/跳过数量、失败或跳过原因以及关键环境配置。
- 非目标：不实现 Hermes 内部 Python plugin；不维护 `plugin.yaml`；不做 Hermes Platform Adapter for Awiki / ANP；不让 Hermes 持有 DID 私钥或直连 message-service；MVP 不做 approval、sandbox/container、handle.resolve、inbox.list、conversation.read、完整 task workflow 或 `task.result` 协议。

## 2. 上下文包

| 来源 | 作用 |
|---|---|
| `/home/ecs-user/awiki-space/awiki-harness/AGENTS.md` | Harness 控制面规则、权威来源和完成标准。 |
| `/home/ecs-user/awiki-space/awiki-harness/README.md` | 多仓库上下文读取顺序和维护原则。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/00-context-map.md` | 将任务路由到 Auth、Message Flow、Client Architecture、System Test 等领域。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/02-repo-map.md` | 确认 `awiki-cli-rs2`、message-service、user-service、awiki-system-test 的职责边界。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/03-cross-repo-architecture.md` | 明确 daemon 与 `im-core`、message-service v2、user-service、系统测试的跨服务关系。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/20-rules-index.md` | 路由到文档、架构、AI coding 和验证规则。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/30-tools-env.md` | Rust workspace、awiki-system-test、本地/远端验证命令入口。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/40-verification.md` | 验证等级：文档 L0，daemon 单仓 L1，跨服务行为 L2，local RPC/token/DID/E2EE 边界 L3。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/50-task-workflow.md` | 非平凡任务的请求、上下文、方案、验证记录格式。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/auth.node.md` | local RPC token、controller DID、registration token 属于身份安全面。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/message-flow.node.md` | direct/direct-e2ee、inbox、history、message-service v2 的行为边界。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/client-architecture.node.md` | daemon 与 awiki-cli 平行复用 `im-core`，不能绕过 SDK 重拼 message-service wire。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | 当前仓库拥有 `im-core`、awiki CLI 壳、Dart package 和 daemon crate。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/message-service.md` | message-service v2 是 direct/group/attachment/realtime 服务端权威。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/user-service.md` | user-service 拥有 DID、Handle、JWT、registration token。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/awiki-system-test.md` | 跨服务 E2E 和最终系统测试证据入口。 |
| `/home/ecs-user/awiki-space/awiki-harness/rules/documentation-principles.md` | 文档放置、权威层级、验证规则。 |
| `/home/ecs-user/awiki-space/awiki-harness/rules/architecture-principles.md` | 依赖方向、DID profile、E2EE 和变更影响规则。 |
| `/home/ecs-user/awiki-space/awiki-harness/rules/ai-coding-rules.md` | 小步、可 review、文档同步和验证报告要求。 |
| `/home/ecs-user/awiki-space/awiki-harness/rules/verification-policy.md` | L0-L3 验证、security gate 和系统测试要求。 |
| `AGENTS.md` | 本仓计划文档必须使用中文；最终系统测试必须在 `../awiki-system-test` remote 模式和 `awiki.info` 域名执行并记录细节。 |
| [../hermes_runtime_plugin_design.md](../hermes_runtime_plugin_design.md) | Hermes native runtime 目标设计和 MVP 范围。 |
| [../../awiki_agent_runtime_host_architecture.md](../../awiki_agent_runtime_host_architecture.md) | daemon 作为 ANP Agent Runtime Host 的通用架构。 |
| [../../cli-plugin/generic_cli_runtime_plugin_design.md](../../cli-plugin/generic_cli_runtime_plugin_design.md) | 当前 `RuntimePlugin` v1 / Generic CLI MVP 的对照参考。 |
| [../../openclaw-plugin/openclaw_runtime_plugin_design.md](../../openclaw-plugin/openclaw_runtime_plugin_design.md) | Native Runtime Plugin、session mapping 和 Skill 回调链路参考。 |
| [../../create/plan.md](../../create/plan.md) | daemon 初始化创建计划和已完成的本地 RPC、agent 管理、系统测试约束。 |
| `crates/awiki-deamon/src/runtime/mod.rs` | 当前 `RuntimePlugin` v1、`RuntimeTask`、`RuntimeRun` 和兼容 task 命名基线。 |
| `crates/awiki-deamon/src/runtime/host.rs` | controller 文本消息到 runtime run、token 签发和 callback 执行的当前入口。 |
| `crates/awiki-deamon/src/state/mod.rs` | SQLite schema、runtime token、agent/profile/workspace/run/task 状态权威。 |
| `crates/awiki-deamon/src/local_rpc/mod.rs` | `rpc.ping`、`task.status`、`task.finish`、`msg.send`、`artifact.created` 与 token 授权。 |
| `crates/awiki-deamon/src/outbox/mod.rs` | test outbox 与 `ImCoreAgentOutbox`，后续真实 `msg.send` 必须落到这里或相邻模块。 |
| `crates/awiki-deamon/src/commands/mod.rs` | `runtime.agent.create`、registration token 兑换、ready/failed 状态。 |
| `crates/awiki-deamon/src/foreground.rs` | 当前长驻 daemon inbox polling、UDS local RPC worker、test runtime 和 status/final payload 入口。 |
| `../awiki-system-test/AGENTS.md` | 系统测试报告必须写明通过、失败、跳过、命令和关键环境配置。 |
| `../awiki-system-test/README.md` | `AWIKI_SYSTEM_TEST_MODE`、`E2E_*` 远端/本地配置入口。 |

## 3. 影响分析

| Domain / repo / module | 影响 | 权威文档或代码 |
|---|---|---|
| `crates/awiki-deamon` runtime/plugin | 新增 `runtime.hermes` daemon-side native plugin；保留 `RuntimePlugin` v1 兼容，同时为 Hermes 引入 runner/session/streaming 语义。 | `src/runtime/mod.rs`, `src/runtime/host.rs`, `src/plugins/` |
| `crates/awiki-deamon` state | 新增 `hermes_profiles`、`hermes_native_sessions`，可选新增 `runtime_session_mapping`；schema 版本递增和迁移测试。 | `src/state/mod.rs` |
| `crates/awiki-deamon` local RPC | 继续支持 `task.status` / `task.finish` 兼容名；以 message/run 语义封装；补真实 `msg.send` 语义和 recipient scope 校验证据。 | `src/local_rpc/mod.rs`, `src/cli_wrapper/mod.rs`, `src/security/runtime_token.rs` |
| `crates/awiki-deamon` outbox / IM Core | `msg.send` 必须真正调用 `im-core` direct/direct-e2ee send，而不是状态 payload 模拟；status/final 仍回 controller。 | `src/outbox/mod.rs`, `src/im_core_adapter.rs` |
| `crates/awiki-deamon` commands | `runtime.agent.create` 对 `runtime: "hermes"` 后触发 Hermes profile/Skills 初始化和 smoke test。 | `src/commands/mod.rs`, `src/agent/mod.rs` |
| `crates/awiki-deamon` foreground | 长驻 daemon 按 `runtime_plugin_id == "runtime.hermes"` 路由到 Hermes plugin，而不是 `UdsTestRuntimePlugin`。 | `src/foreground.rs` |
| Hermes 本地 profile | 创建 Hermes profile home、SOUL.md、profile config、Awiki Skills、wrapper 路径和 smoke test 配置。 | [../hermes_runtime_plugin_design.md](../hermes_runtime_plugin_design.md) |
| `im-core` | 真实 direct/direct-e2ee 发送能力应复用现有 SDK；仅在必要时增加 daemon 侧适配，不把 wire 细节写进 daemon。 | `crates/im-core/src/messages/`, Harness client architecture |
| message-service v2 | 系统测试需证明 Hermes 经 daemon 发出的 direct/direct-e2ee 消息进入 message-service 并可被目标 DID 接收。 | message-service API docs, `../awiki-system-test/tests_v2/message_service/` |
| user-service | registration token 已作为前置能力；本计划不扩展 user-service，除非 Hermes 初始化发现 profile/agent metadata 需要新字段。 | user-service `SPEC.md`, registration token tests |
| `../awiki-system-test` | 新增或扩展 daemon/Hermes E2E，用 fake Hermes gateway 或可控 Hermes binary 验证 profile、消息执行、status/final、真实外发。最终执行完整 remote 系统测试。 | `../awiki-system-test/README.md`, `tests_v2/daemon/` |

## 4. 假设与开放问题

### 假设

- 当前仓库目录名沿用 `crates/awiki-deamon`，本计划不做拼写重命名。
- Hermes MVP 使用消息驱动，不新增产品层 task；代码中的 `RuntimeTask`、`runtime_task`、`task.status`、`task.finish` 先作为兼容实现名存在。
- `runtime.agent.create`、registration token、agent DID、本地 RPC token、UDS、本地 audit 已可作为前置基础。
- Hermes TUI Gateway 真实命令、stdio JSON-RPC frame 和 event schema 在实现时可能需要以本机 Hermes 安装为准；计划要求先以 trait/adapter 隔离，并用 fake gateway 做 deterministic 测试。
- 如果真实 Hermes binary 不存在，单元/集成测试必须使用 fake gateway；真实 Hermes smoke test 通过显式环境变量启用并记录 skip 原因。
- profile token 若后续出现，只能用于低风险 health/ping，不授权 `msg.send`、`task.finish` 或 future `message.finish`。
- `msg.send` recipient scope 继续由 `runtime_rpc_tokens.allowed_recipients_json` 限制；Step 05 已收敛为保守策略：controller text 触发的 Hermes run token 默认 `allowed_recipients = Some(controller_did)`，不默认开放任意 recipient；更宽协作目标需要后续显式 policy/config。
- E2EE 明文和密钥状态仍由客户端 SDK/`im-core` 持有；daemon 不接触私聊/群聊明文密钥。

### 开放问题

- Hermes TUI Gateway 的实际启动命令、`session.create`、`prompt.submit`、streaming event schema、session resume/reset API 需要实现阶段确认。
- Hermes profile 中 SOUL.md、config 文件名和目录结构是否有版本差异，需要 Step 02 通过 installation checker 固化。
- `task.finish` 幂等、failed final、`message.status` / `message.finish` 新方法是否进入本轮实现：本计划默认不阻塞 MVP，作为 Step 08 之后的后续增强。
- direct-e2ee 真实发送是否需要额外 prekey/session 初始化：Step 05 先支持 direct plain 与已具备 direct-e2ee 的 SDK 路径，若 E2EE 环境不可用必须记录替代验证和风险。
- remote `awiki.info` 完整系统测试是否受注册限额、外部服务限流或真实 Hermes 安装缺失影响：最终报告必须明确失败/跳过原因，不能把 skip 记为通过。

## 5. 阶段划分

| 阶段 | 目标 | 依赖 | 完成门禁 |
|---|---|---|---|
| 阶段 1 | 契约与基线收敛 | 无 | Hermes MVP 契约、代码缺口、测试夹具和兼容命名策略冻结；review 后提交。 |
| 阶段 2 | Hermes profile + Awiki Skills | 阶段 1 | profile/Skills 安装、无副作用 smoke test、schema 或配置文档完成；review 后提交。 |
| 阶段 3 | TUI Gateway runner | 阶段 2 | fake gateway 下 `session.create`、`prompt.submit`、stream observation 可测；真实 Hermes smoke 可选；review 后提交。 |
| 阶段 4 | Controller 消息执行链 | 阶段 3 | controller text/plain 到 Hermes prompt wrapper、run token、status/final 回调闭环；review 后提交。 |
| 阶段 5 | 真实外发消息 | 阶段 4 | `msg.send` 真实 direct/direct-e2ee send，recipient scope 和 audit 验证；review 后提交。 |
| 阶段 6 | Session 持久化 | 阶段 3, 4 | `hermes_native_sessions` 和可选 `runtime_session_mapping` 支持 get/create、resume/reset；review 后提交。 |
| 阶段 7 | 长驻 daemon 集成与诊断 | 阶段 2-6 | foreground 按 runtime.hermes 路由，诊断命令和日志/audit 可用；review 后提交。 |
| 阶段 8 | 整体验证与系统测试 | 阶段 1-7 | repo、本地集成、focused 系统测试、完整 remote `awiki.info` 系统测试和发布门禁记录完成；如有变更则 review 后提交。 |

## 6. 任务拆分

| 步骤 | 标题 | 依赖 | 输出 | 步骤文档 | 提交门禁 | 状态 |
|---|---|---|---|---|---|---|
| 01 | 契约与代码基线收敛 | 无 | Hermes MVP 契约、现有代码差距、测试夹具策略和执行范围冻结 | [steps/01-contract-baseline.md](steps/01-contract-baseline.md) | 必须 | done |
| 02 | Hermes profile 与 Awiki Skills 安装 | 01 | `hermes_profiles`、profile home、SOUL.md、Awiki Skills、无副作用 smoke test | [steps/02-profile-skills.md](steps/02-profile-skills.md) | 必须 | done |
| 03 | TUI Gateway runner 与 plugin 骨架 | 02 | `runtime.hermes` plugin、TUI Gateway adapter、fake gateway 测试、stream observation | [steps/03-tui-gateway-runner.md](steps/03-tui-gateway-runner.md) | 必须 | done |
| 04 | Controller 消息执行链与 Prompt Wrapper | 03 | controller text/plain 到 Hermes，run token，prompt wrapper，status/final 回调 | [steps/04-message-execution.md](steps/04-message-execution.md) | 必须 | done |
| 05 | 真实 `msg.send` 外发消息 | 04 | `msg.send` 真实 ANP direct/direct-e2ee send，recipient scope，audit 和测试 | [steps/05-real-msg-send.md](steps/05-real-msg-send.md) | 必须 | done |
| 06 | Hermes session 持久化与 resume/reset | 03, 04 | `hermes_native_sessions`、可选通用 session mapping、resume/reset/cleanup | [steps/06-session-persistence.md](steps/06-session-persistence.md) | 必须 | done |
| 07 | 长驻 daemon 集成与诊断 | 02-06 | foreground Hermes 路由、runner 生命周期、诊断命令、observability | [steps/07-foreground-integration.md](steps/07-foreground-integration.md) | 必须 | review |
| 08 | 整体验证、系统测试与发布门禁 | 01-07 | repo 验证、system-test、remote `awiki.info` 完整系统测试、发布记录 | [steps/08-integration-verification.md](steps/08-integration-verification.md) | 如有文件变更则必须 | pending |

## 7. 执行账本

状态值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| 步骤 | 状态 | 分支 | 开始时间 | 完成时间 | 提交 | 审查证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-05-31 23:02:52 +0800 | 2026-05-31 23:17:21 +0800 | 实现提交 `e4c46fc4cd6fcf7e44f793404640abc21c0cade4`；账本收尾提交 `770a150896d5890093867346ab942aecd29f699f` | 2026-05-31 23:13:02 +0800 完成提交前 review：未发现生产实现越界；已修复 focused 测试过滤问题；残余风险为 `msg.send` 真实外发仍待 Step 05 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes` 通过，5 个 hermes contract tests；`cargo test -p awiki-deamon --locked` 通过，39 个测试；`git diff --check -- crates/awiki-deamon/docs/hermes-plugin crates/awiki-deamon/tests crates/awiki-deamon/src/plugins` 通过；禁止项搜索仅命中文档非目标、测试断言和既有 `WorkspaceMode::Sandbox`；提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 2]`，无未提交变更 | Step 02 已启动 |
| 02 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-05-31 23:19:11 +0800 | 2026-05-31 23:32:43 +0800 | 实现提交 `f8a0ae9b9994ebb7ebbee2aef48364a9d5fc6261`；账本收尾提交 `56b4c7ea50f498777bbe42d34e0f3a706f7f1f8f` | 2026-05-31 23:31:00 +0800 完成提交前 review：修复 profile 敏感字段名泄露风险、wrapper 配置夸大真实能力和 schema version 测试漂移；残余风险为真实 Hermes profile layout 仍待 Step 03 smoke 校验 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_profile` 通过，3 个测试；`cargo test -p awiki-deamon --locked` 通过，42 个测试；secret 搜索仅命中测试断言和安全说明；禁止 plugin 搜索仅命中测试断言；`git diff --check -- crates/awiki-deamon` 通过；提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 4]`，无未提交变更 | Step 03 已启动 |
| 03 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-05-31 23:34:15 +0800 | 2026-05-31 23:42:25 +0800 | 实现提交 `fa4668a206404e61beda09db1d5675548ff30731`；账本收尾提交 `d9eea0b9b57fb694ed7cd05fd6b309437626efad` | 2026-05-31 23:41:03 +0800 完成提交前 review：修复 launch context 与 Hermes profile binding 校验缺口；确认 `message.complete` 仅 observation、不产生 final callback；真实 stdio adapter 未虚假实现未知协议 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，5 个默认测试、1 个 ignored；`cargo test -p awiki-deamon --locked` 通过，47 个测试、1 ignored；`cargo test -p awiki-deamon --locked hermes_real_smoke -- --ignored --nocapture` 通过并记录 `AWIKI_HERMES_BIN is not set`；secret/debug 搜索仅命中测试 fixture/脱敏断言和安全说明；`git diff --check -- crates/awiki-deamon` 通过；提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 6]`，无未提交变更 | Step 04 已启动 |
| 04 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-05-31 23:43:53 +0800 | 2026-05-31 23:59:07 +0800 | 实现提交 `9d3c57cd020cd72fcec4814852266449ffb145bf`；账本收尾提交 `c097098d74f76e3412b422b00da82e07649b0aaa` | 2026-05-31 23:56:13 +0800 完成提交前 review：确认 controller 校验仍在 host/inbox；prompt wrapper 不含 token/private key/JWT；fake gateway callback token 会被 daemon run token 替换；`message.complete` 仍只作 observation；补充修复 launch context 中 task/run/profile 不一致校验；长驻 foreground 按 runtime_plugin_id 路由明确留 Step 07 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_message` 通过，4 个测试；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，6 个匹配测试、1 个 ignored real smoke 被过滤；`cargo test -p awiki-deamon --test local_rpc_security --locked` 通过，6 个测试；`cargo test -p awiki-deamon --locked` 通过，52 个测试、1 ignored；secret/plugin 搜索仅命中预期测试、文档和生产 token 替换点；`git diff --check -- crates/awiki-deamon` 通过；实现提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 7]`，无未提交变更 | 启动 Step 05 |
| 05 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-06-01 00:02:56 +0800 | 2026-06-01 00:38:29 +0800 | 实现提交 `9ee0ac805f897d132a4b4127eed56bf8b4c68ed4` | 2026-06-01 00:34:49 +0800 完成提交前 review：确认 `msg.send` 不再包装为 status payload，而是经 `ImCoreAgentOutbox::send_text` 调用 `im-core` direct send；确认 `to`/`recipient`、非空 text 和 `security` 校验；确认 controller text run token 默认只允许发给 controller DID；确认 direct-e2ee 只映射到 `MessageSecurityMode::SecureDirect`，daemon 不处理 E2EE key；发现并规避 foreground 诊断把 direct send 计入 status 消息计数的风险；真实网络 smoke 未运行，留 Step 08 remote 系统测试。 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked runtime_message_send_params_validate_and_map_security` 通过，1 个测试；`cargo test -p awiki-deamon --locked msg_send` 通过，3 个匹配测试；`cargo test -p awiki-deamon --locked hermes_message` 通过，6 个测试；`cargo test -p awiki-deamon --locked hermes_profile` 通过，3 个测试；`cargo test -p awiki-deamon --locked` 通过，54 个测试、1 ignored；`cargo test --workspace --locked` 通过；`git diff --check -- crates/awiki-deamon` 通过；边界搜索 `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无命中；secret/plugin 搜索仅命中预期测试、字段名、脱敏实现和文档非目标说明；实现提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 10]`，无未提交变更 | 启动 Step 06 |
| 06 | done | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-06-01 00:40:24 +0800 | 2026-06-01 00:53:38 +0800 | 实现提交 `9c5bff865682d3c77e97355484aacbe8d0f64823` | 2026-06-01 00:51:13 +0800 完成提交前 review：确认 Step 06 仅落 Hermes 私有 `hermes_native_sessions`，未新增通用 `runtime_session_mapping`；schema v7、active route partial unique index、state CRUD、runner session 复用和 reset helper 已检查；发现并记录首次同 route 并发创建仍依赖唯一约束报错而非事务重试，留 Step 07/后续长驻并发化处理；确认 session 表不存 prompt 原文、token、private key 或 JWT。 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_session` 通过，2 个 focused tests；`cargo test -p awiki-deamon --locked state` 通过，12 个匹配测试；`cargo test -p awiki-deamon --locked hermes_message` 通过，6 个匹配测试；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，6 个匹配测试、1 个 ignored real smoke 被过滤；`cargo test -p awiki-deamon --locked` 通过，56 个测试、1 ignored；`git diff --check -- crates/awiki-deamon` 通过；awiki-cli 边界搜索无命中；session/security 搜索仅命中预期 prompt wrapper、测试脱敏断言、既有 agent auth/private key 状态存储和 runtime task 文本字段；实现提交后 `git status --short --branch`：`## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 12]`，无未提交变更 | 启动 Step 07 |
| 07 | review | `feature/release-0526/hermes-plugin-cli-rs2` | 2026-06-01 00:55:12 +0800 | 未完成 | 未提交 | 2026-06-01 01:10:41 +0800 完成提交前 review：确认 foreground 按 `runtime_plugin_id` 路由到 Hermes；非 controller text 在 gateway 前被拒绝；`agent-status` Hermes 诊断不输出 token/JWT/private key/prompt，并修复 `last_error` 可能透传敏感 audit detail 的风险；残余风险为真实 `StdioHermesGateway` 的 `session.create`/`prompt.submit` 仍未接线 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_foreground` 通过，2 个 focused tests；`cargo test -p awiki-deamon --locked hermes_status` 通过，1 个 focused test；`cargo test -p awiki-deamon --locked` 通过，58 个测试、1 ignored；`cargo test --workspace --locked` 通过；`git diff --check -- crates/awiki-deamon` 通过；awiki-cli 边界搜索无命中；secret 搜索仅命中预期测试、既有密钥/JWT 字段、诊断敏感标记列表和 prompt wrapper 相关代码 | 提交 Step 07 实现并回填 hash |
| 08 | pending | `feature/release-0526/hermes-plugin-cli-rs2` | 未开始 | 未完成 | 未提交 | 待记录 | 待记录 | 等待 Step 01-07 完成 |

## 8. Codex Goal 执行协议

- 本计划是后续执行进度的唯一来源。
- 开始或恢复执行前，必须读取本计划、当前步骤文档、执行账本、[../hermes_runtime_plugin_design.md](../hermes_runtime_plugin_design.md) 和当前 `git status`。
- 除非本计划明确标记步骤可并行，否则一次只执行一个步骤。
- 恢复执行时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤都按顺序执行：标记 `in_progress`、实现、验证、代码 review、修复 review 发现、提交、记录证据、标记 `done`。
- 每次提交代码后必须进入代码 review 环节，review 发现必须修复或在步骤文档中明确记录残余风险，不能跳过 review 直接进入下一步。
- 不能带着上一依赖步骤的未提交完成工作进入下一步骤。
- 每步提交前记录 `git status` 和纳入文件；提交后记录 commit hash 和提交后的 `git status`。
- Git 提交对象无法在同一个提交中记录自己的最终 hash；执行时允许在当前步骤实现提交之后，追加一个同一步账本收尾提交，用于记录实现提交 hash、提交后状态和 `done` 状态。进入下一步骤前工作树必须干净，且账本收尾提交不得夹带下一步骤实现。
- 如果需要改变范围、顺序、验收标准、公开契约、数据模型、安全假设或验证策略，必须先更新本计划并写入变更日志。
- 本地 RPC 授权不能信任请求体中的 `agent_did`、`run_id`、`message_id`、`runtime_profile_id`、`controller_did`；可信上下文必须来自 daemon 根据 token 和内部状态反查。
- Hermes 不能持有 DID 私钥，不能直连 message-service，不能直接调用 `im-core`；所有远端通信必须经 daemon。
- `application/json + body.payload` 是结构化 JSON 承载方式；本计划不新增 `application/vnd.awiki...` 类专用 content type。

## 9. 审查策略

- 每步实现后、提交前都必须做代码 review，优先检查正确性、回归风险、公开契约、数据安全、测试和文档漂移。
- Step 01 重点 review 契约是否和设计文档一致，是否误把 task 概念扩大为产品协议。
- Step 02 重点 review profile/Skills 安装是否无长期写权限 token、无 Hermes plugin、无真实外发副作用。
- Step 03 重点 review TUI Gateway adapter 是否隔离真实 Hermes 协议差异，fake gateway 是否可确定性测试 streaming event。
- Step 04 重点 review controller 校验、prompt wrapper、安全边界和 final 主事实源是否正确。
- Step 05 重点 review `msg.send` 是否真实发送 direct/direct-e2ee，是否仍受 run token method/recipient scope 和 audit 约束。
- Step 06 重点 review schema migration、session 唯一约束、resume/reset 和数据回滚。
- Step 07 重点 review 长驻 daemon 生命周期、foreground routing、runner shutdown、日志脱敏和诊断输出。
- Step 08 重点 review 系统测试证据、失败/跳过原因、远端 `awiki.info` 配置和发布残余风险。
- L3 安全面必须显式 review：controller DID、runtime token、recipient scope、DID 私钥隔离、E2EE 明文边界、Hermes profile 中不得持久化可写 run token。

## 10. 验证策略

| 等级 | 命令或检查 | 预期证据 |
|---|---|---|
| 文档 L0 | `git diff --check -- crates/awiki-deamon/docs/hermes-plugin/plan` | 无 trailing whitespace 或补丁格式问题。 |
| 文档 L0 | `find crates/awiki-deamon/docs/hermes-plugin/plan -type f -maxdepth 3 -print` | `plan.md` 和所有 `steps/*.md` 存在。 |
| 文档 L0 | `rg -n "\\[steps/|Main plan" crates/awiki-deamon/docs/hermes-plugin/plan` | 主计划和步骤文档双向链接可人工核对。 |
| Rust 格式 | `cargo fmt --all --check` | 格式检查通过。 |
| 当前仓库 Rust | `cargo test -p awiki-deamon --locked` | daemon 单仓测试通过。 |
| 当前仓库 Rust | `cargo test --workspace --locked` | `im-core`、`im-core-dart`、`awiki-cli`、daemon crate 测试通过；若受资源限制失败需记录失败位置和替代 focused 验证。 |
| 边界搜索 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部模块。 |
| Hermes plugin 搜索 | `rg -n "plugin.yaml|Awiki Hermes Plugin|plugins/awiki-runtime" crates/awiki-deamon/src crates/awiki-deamon/docs/hermes-plugin` | 生产实现不安装 Hermes Python plugin；设计文档中的“删除/不做”说明可保留。 |
| local RPC 安全 | focused tests for `runtime_rpc_tokens`, `msg.send`, recipient scope, UDS permission | token 原文不进日志/audit，请求体 spoof 字段不参与授权，越权 recipient 被拒绝。 |
| Hermes fake gateway | focused tests for `runtime.hermes` fake gateway | profile/Skills、session.create、prompt.submit、event observation、status/final callback 可确定性通过。 |
| 真实 Hermes smoke | `AWIKI_HERMES_BIN=<path> cargo test -p awiki-deamon --locked hermes_real_smoke -- --ignored` | 有 Hermes binary 时通过；没有时记录 skip 原因。 |
| 系统 focused | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info uv run awiki-system-test tests_v2/daemon` | daemon/Hermes focused E2E 通过或记录失败/跳过原因。 |
| 最终完整系统测试 | `cd ../awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info E2E_USER_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_URL=https://awiki.info E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info uv run awiki-system-test` | 必须记录实际命令、通过/失败/跳过数量、失败或跳过原因、关键环境配置。 |

## 11. 文档更新

- 当前计划文档：执行时持续更新本 `plan.md`、对应 step 文档、执行账本和变更日志。
- 当前仓库 Hermes docs：如果实现改变设计边界，更新 [../hermes_runtime_plugin_design.md](../hermes_runtime_plugin_design.md)。
- 当前仓库 daemon docs：若新增命令、配置、DB schema、诊断或 local RPC method，更新 `crates/awiki-deamon/docs/` 下对应文档。
- Harness docs：只有跨仓路由、验证策略、架构边界发生变化时才更新。
- awiki-system-test docs：新增或修改 Hermes 系统测试时，按 `../awiki-system-test/AGENTS.md` 和 README 更新测试说明。

## 12. 提交计划

- 每个完成、验证、审查过的步骤必须有一个聚焦提交。
- 提交信息建议格式：`daemon: <step outcome>` 或 `test: <step outcome>`；跨仓提交要在账本中记录每个仓库 hash。
- 提交前记录 `git status` 和纳入文件。
- 提交后记录 commit hash 和工作区状态。
- Step 08 只有产生文件变更时才需要最终集成提交；验证记录本身如写入文档则必须提交。
- 不能把所有步骤变更堆到最后一个大提交。
- 如步骤文档需要记录刚创建的实现提交 hash，允许创建同一步账本收尾提交；该提交只包含主计划、步骤文档或执行记录的 hash/status 回填，不视为下一步骤实现提交。

## 13. 阻塞处理

| 阻塞项 | 步骤 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|---|
| Hermes TUI Gateway 真实协议不可用或未安装 | 03 | 待记录 `hermes --version`、启动命令、stderr、文档链接 | 先使用 fake gateway；真实 smoke 标记 skipped | 当前步骤真实 smoke；不阻塞 fake gateway 和 daemon 代码 | 若要发布真实 Hermes 支持，必须补真实 binary 验证 |
| remote `awiki.info` 服务不可用、限流或注册额度不足 | 08 | 待记录 HTTP status、pytest skip/fail summary | 重跑 focused suite；确认本地或 fake 替代 | 最终发布风险 | 不能记为通过；报告失败/跳过并请求环境修复 |
| `msg.send` direct-e2ee 前置 prekey/session 不满足 | 05 | 待记录 SDK error、目标 DID 状态 | 先验证 direct plain，补 E2EE setup 或跳过原因 | E2EE 外发验收受影响 | direct-e2ee 对外启用前必须补 L3 验证 |
| schema migration 破坏旧 daemon.db | 02/06 | 待记录旧库 fixture、迁移错误 | 加 migration test 和 rollback note | 当前仓库阻塞 | 修复迁移后才能提交 |

- 只有依赖允许且风险已记录时，才可继续另一个 pending 步骤。
- 无安全假设、无替代验证、无独立下一步时，才询问用户。
- 阻塞解除后必须在对应 step 文档记录解决方式。

## 14. 计划变更日志

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要审查 |
|---|---|---|---|---|
| 2026-05-31 | 创建 Hermes Runtime Plugin 落地总计划和 8 个阶段文档 | 用户要求把 Hermes 设计拆成可执行小任务并放入指定 plan 目录 | 01-08 | 是 |
| 2026-05-31 | 明确每步提交 hash 的回填策略：步骤实现提交后可追加同一步账本收尾提交，专门记录实现提交 hash、提交后状态和 `done` 状态；进入下一步骤前必须保持工作树干净 | Git 提交无法在同一个提交中记录自己的最终 hash，需要避免自引用账本不可能完成，同时保持逐步提交和可审计证据 | 01-08 | 是 |
| 2026-06-01 | 明确 Hermes controller text 触发的 run token 默认 `msg.send` recipient scope 为 controller DID only，不再把 `allowed_recipients = None` 作为默认开放策略；更宽 recipient 需要后续显式 policy/config | Step 05 实现阶段收敛安全假设，避免 Hermes prompt 或本地 RPC token 默认具备任意 DID 外发能力 | 05、08 | 是 |
| 2026-06-01 | Step 06 先实现 Hermes 私有 `hermes_native_sessions`，保留 `runtime_session_id`、route key 和唯一 route 约束；暂不新增通用 `runtime_session_mapping` 表 | 通用 runtime session abstraction 会影响其他 runtime，超出本步骤最小可验证范围；Hermes 私有表已能满足 resume/reset，并为后续通用映射预留字段 | 06、07、08 | 是 |

## 15. 风险与回滚

| 风险 | 缓解 | 回滚 / fallback |
|---|---|---|
| Hermes 真实 TUI Gateway 协议变动 | 以 `HermesGateway` trait 隔离；fake gateway 保障 daemon 行为；真实 smoke 独立启用 | 回退到 fake-only 支持，不发布真实 Hermes ready 状态 |
| `msg.send` 被误实现成 status payload | Step 05 明确验收 direct/direct-e2ee 真实发送；review outbox 和 system-test | 保留旧 status/final，禁用 Hermes `send-message` Skill 能力 |
| local RPC token 泄露或 profile 中持久化可写 token | profile 初始化只写 wrapper/socket/profile binding；run token 每次消息前签发；secret 搜索和 review | 撤销 token，删除 profile 中错误配置，补 migration/cleanup |
| task 兼容命名扩大成产品协议 | Step 01/04 明确 message/run 语义；文档和 prompt wrapper 不新增 `task.result` | 保留兼容 alias，推迟 rename，不扩大外部协议 |
| session mapping 设计过早泛化 | Step 06 允许先 Hermes 私有表，并预留 `runtime_session_id` | 只落 `hermes_native_sessions`，通用表延后 |
| remote 系统测试不可控 | 必须记录真实失败/跳过；本地/fake 测试只作为替代证据 | 不发布 remote-ready 结论，保留环境阻塞 |
