# 计划：AWiki Daemon 运行时宿主初始化创建

状态：执行中（步骤 06 已完成，等待阶段 B Review）
文档目录：`crates/awiki-deamon/docs/create/`
创建日期：2026-05-30
当前分支：`feature/release-0526/awiki-deamon`
恢复位置：步骤 06 已完成，下一次执行阶段 B Review

## 1. 目标

- 目标：基于 [awiki_agent_runtime_host_architecture.md](../awiki_agent_runtime_host_architecture.md) 和当前代码结构，形成一份后续 Codex Goal 可以直接执行的 daemon 工程初始化创建计划。
- 预期结果：先在 `crates/awiki-deamon` 搭起 daemon 工程和本地 MVP 闭环，再补齐 SDK、服务端、注册和系统测试能力。
- 完成标准：所有步骤完成后，daemon、SDK 和服务端均有实现、测试、文档、代码 review 记录和聚焦提交；系统测试覆盖 `application/json + body.payload`、registration token、`runtime_rpc_token` 和 daemon MVP 闭环。
- 非目标：本计划不实现代码；不再规划协议仓库改动；不设计 AVIC / AMP proof；不把 `shared-root` 或 `worktree-per-task` 宣称为强安全边界；不一次性接入 Hermes、OpenClaw、Claude Code、Codex、Gemini CLI 的完整能力；不展开 App UI 细节。

## 2. 已完成的前置条件

协议仓库中的 JSON payload 相关改动已经完成，本计划不再把协议仓库作为一个待执行阶段。后续实现直接消费协议仓库现状。

| 前置项 | 已确认内容 | 参考位置 |
|---|---|---|
| 协议正文 | 普通结构化 JSON 使用 `application/json`，JSON 对象放在 `body.payload` 或安全层的内部 `payload`；不再为了 command、status、task、result 等业务语义定义专用内容类型。 | `/home/ecs-user/awiki-space/anp/AgentNetworkProtocol/message/01-core-binding.md` |
| Rust SDK 说明 | 协议 SDK 将 JSON 对象视为不透明的应用层 payload；command、status、task、result 等业务含义由 ANP SDK 上层调用方定义。 | `/home/ecs-user/awiki-space/anp/anp/rust/README.md` |
| 架构文档 | daemon 架构文档已同步为 `application/json + body.payload`，业务语义由 payload 内部字段识别。 | [awiki_agent_runtime_host_architecture.md](../awiki_agent_runtime_host_architecture.md) |

执行本计划时必须遵守：

- daemon 工程实现主目录固定为 `crates/awiki-deamon`；除根 `Cargo.toml` workspace member、明确列出的 SDK/service 跨仓步骤和系统测试外，daemon 进程、配置、DB、本地 RPC、runtime 插件、agent 管理和 daemon CLI 均应在该目录下实现。
- 不新增 command/status/result 专用 JSON 内容类型。
- 不新增旧版结构化 JSON 字段名，也不引入 `body.payload` 之外的同义字段。
- 结构化 JSON 的消息字段固定为 `body.payload`。
- `payload.schema`、`payload.command`、`payload.state`、`payload.result` 等字段属于上层业务 schema，不属于 message-service 的执行语义。
- message-service 只负责传输、存储和投递 payload，不解释 daemon 命令含义。

## 3. 上下文

| 来源 | 作用 |
|---|---|
| `/home/ecs-user/awiki-space/awiki-harness/AGENTS.md` | 定义 Harness 控制面和跨仓库工作规则。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/02-repo-map.md` | 明确 `anp`、user-service、message-service、awiki-cli-rs2、awiki-system-test 的职责边界。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/03-cross-repo-architecture.md` | 明确 `im-core` 是端侧共享 IM SDK，awiki-cli 是 CLI 壳，message-service v2 是新消息服务方向。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/40-verification.md` | 本任务涉及身份、安全和跨服务变更，后续实现需要兼容性、安全审查和 E2E 证据。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/auth.node.md` | Token、DID、注册、验证属于身份安全面，必须按服务契约落地。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/message-flow.node.md` | direct/group/realtime/history/local projection 等链路需要端到端保持消息 body 语义。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/nodes/client-architecture.node.md` | daemon 与 awiki-cli 平行，二者都复用 `im-core`，daemon 不应依赖 awiki-cli 命令系统。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/user-service.md` | user-service 拥有 DID、Handle、JWT 和 registration token 的服务端真相。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/message-service.md` | message-service v2 拥有 direct/group/attachment 服务端协议实现。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/awiki-cli-rs2.md` | 当前仓库拥有 `im-core`、`im-core-dart`、`awiki-cli`，并将新增 daemon crate。 |
| `/home/ecs-user/awiki-space/awiki-harness/context/repo-profiles/awiki-system-test.md` | 后续跨服务闭环必须落到系统测试。 |

## 4. 本地观察

| 来源 | 当前观察 |
|---|---|
| [awiki_agent_runtime_host_architecture.md](../awiki_agent_runtime_host_architecture.md) | daemon 是 ANP Agent 运行时宿主；`controller_did` MVP 保持简单模型；结构化 JSON 使用 `application/json + body.payload`；本地 RPC 使用短期 `runtime_rpc_token`；daemon 与 awiki-cli 平行复用 `im-core`。 |
| `Cargo.toml` | 当前 workspace 只有 `crates/im-core`、`crates/im-core-dart`、`crates/awiki-cli`、`xtask`；`crates/awiki-deamon` 还不是 crate，也不是 workspace 成员。 |
| `crates/awiki-deamon/` | 当前只有文档，没有 Rust 代码骨架。 |
| `docs/sdk-refactor/architecture.md` | `im-core` 是高层 SDK，公开接口不应暴露线协议 payload、actor、SQLite path、RPC 原始参数。 |
| `docs/sdk-refactor/modules/07-messages.md` | 当前 messages 设计以 direct/group text 为主，payload 还未成为一等 body。 |
| `crates/im-core/src/messages/dto.rs` | `MessageBody` 目前只有 `Text` 和 `Attachment`；`MessageBodyView` 只有 `Text` 和 `Unsupported`，缺少 payload view。 |
| `crates/im-core/src/internal/wire/direct.rs` | direct 线协议辅助模块目前构造 `body.text`，需要新增 `body.payload` 构造路径。 |
| `crates/im-core/src/internal/message_runtime/direct.rs` | direct sender 目前强制 text body，payload 发送要新增或泛化 sender。 |
| `crates/im-core/src/internal/message_runtime/group.rs` | group sender 目前强制 text body，group payload 要同步处理。 |
| `crates/im-core-dart/src/dto/message.rs` | Dart DTO 只有 text request/body view，需要同步 payload DTO 和映射。 |
| `crates/awiki-cli/src/host_runtime/` | 可作为 daemon runtime listener、host notify、service 管理经验参考，但不能作为 daemon 内部依赖。 |
| `message-service/docs/api/ANP-client-server-api-direct.md` | direct 客户端 API 使用 `params.meta/auth/body/client`，服务端需要兼容新的 payload body 语义。 |
| `message-service/docs/api/ANP-client-server-api-group.md` | group API 使用同样边界，group payload 要保持 group receipt、history、incoming 通知一致。 |
| `user-service/SPEC.md` | user-service 拥有 DID、Handle、JWT 和 token 相关基础能力，registration token 方案应落在此服务契约中。 |

## 5. 影响范围

| 范围 | 影响 | 权威来源 |
|---|---|---|
| `crates/im-core` | 公开 DTO、send/read/history/inbox/realtime projection、线协议辅助模块、本地状态需要支持 payload body，同时保留 unsupported/原始兼容路径。 | `crates/im-core/src/messages/`, `crates/im-core/src/internal/message_runtime/`, `crates/im-core/src/realtime/` |
| `crates/im-core-dart` | Dart/Flutter DTO、映射、API 需要暴露 payload send/view 能力。 | `crates/im-core-dart/src/dto/message.rs`, `crates/im-core-dart/src/mapping/`, `crates/im-core-dart/src/api/messages.rs` |
| message-service | direct/group send、history、inbox、realtime incoming、存储、API 文档需要识别并保留 `application/json + body.payload`。 | `message-service/docs/api/`, `message-service/crates/*` |
| user-service | 增加 daemon/runtime agent registration token 的签发、验证、兑换、过期、撤销和审计契约。 | `user-service/SPEC.md`, `user-service/docs/api/`, `user-service/src/user_service/` |
| `crates/awiki-deamon` | 新增 daemon crate、进程骨架、配置、DB、本地 RPC、runtime 插件、daemon CLI 封装器、agent/runtime 管理。 | `crates/awiki-deamon/` |
| `crates/awiki-cli` | 现有 CLI 可增加少量 payload 测试入口，但不成为 daemon 命令系统；host runtime 代码只作为参考。 | `crates/awiki-cli/src/m_core_cli_adapter/`, `crates/awiki-cli/src/host_runtime/` |
| awiki-system-test | 增加跨服务 E2E：payload direct/group、registration token、daemon MVP 闭环、本地 RPC 安全冒烟验证。 | `awiki-system-test/README.md`, `awiki-system-test/tests/` |

## 6. 假设与开放问题

### 假设

- 目录名暂时沿用用户指定的 `crates/awiki-deamon`，本计划不重命名为 `awiki-daemon`。
- `controller_did` 在 MVP 中继续作为简单自动执行权限边界，不引入 AVIC / AMP proof。
- 协议仓库的 JSON payload 相关改动已完成；本计划只消费该协议，不再规划协议仓库改动。
- 结构化 JSON 固定使用 `meta.content_type = application/json` 和 `body.payload`。
- message-service v2 是新能力的主要服务端目标；legacy Python message service 只做兼容评估，不作为首要实现目标。
- daemon 与 awiki-cli 是平行入口，二者都复用 `im-core`；daemon 可以参考 CLI 实现，但不依赖 awiki-cli 命令。
- 首个 daemon 版本使用一个 `daemon.db`，不同 agent/插件通过字段和表隔离。
- 首个 runtime 状态主链路只使用 Skill / daemon CLI 封装器 / 本地 RPC；RuntimeEvent 只做观测和日志，不作为第二个权威状态源。
- Workspace 模式先只记录和约束：`shared-root` 不是硬隔离，`worktree-per-task` 只做代码变更隔离，只有 `container / sandbox` 才能作为安全边界。

### 开放问题

- user-service registration token 的具体路由命名、表结构和幂等 key 需要在步骤 06 中冻结。
- App 或 Mac 端如何请求 daemon/runtime registration token，需要另行对齐产品入口。
- payload 是否需要第一版就支持 direct-e2ee 和 group-e2ee 的明文 inner body，还是先支持 transport-protected direct/group，再补 E2EE 兼容策略。
- daemon CLI 封装器最终二进制和命令命名是否使用 `awiki-daemon`，本计划先按架构文档命名，执行时可通过计划变更调整。
- daemon 本地 MVP 的消息出口第一版使用 testable adapter / mock transport，还是直接接真实 `im-core` 文本发送。当前约束：步骤 03 可以用 testable adapter 跑本地闭环，真实 payload command 闭环等步骤 04 到步骤 06 完成后再产品化。

## 7. 阶段策略

| 阶段 | 覆盖步骤 | 原因 | 阶段完成门禁 |
|---|---|---|---|
| 阶段 A：daemon 工程骨架与本地 MVP | 01, 02, 03 | 先把 `crates/awiki-deamon` 工程搭起来，跑通本地 runtime 闭环，避免执行入口先跑到服务端。 | 阶段代码 review、`cargo test -p awiki-deamon --locked` 或 workspace 等价测试、边界检查、阶段提交记录。 |
| 阶段 B：SDK 与服务端承接 | 04, 05, 06 | 协议已完成，再让 `im-core`、message-service、user-service 具备 daemon 产品化需要的消息和身份能力。 | 阶段代码 review、SDK/service/token 聚焦测试、契约检查、跨仓提交记录。 |
| 阶段 C：agent 管理与集成 | 07, 08 | 补 daemon agent/runtime agent 注册与命令管理，再做跨仓系统测试、文档同步和安全复核。 | 阶段代码 review、E2E 或替代验证、安全 review、发布门禁记录。 |

关键顺序判断：

- 协议仓库已经完成，执行时直接参考协议和 SDK 现状。
- daemon 主线先落到 `crates/awiki-deamon`；步骤 01 到步骤 03 不应被 SDK/service/token 工作阻塞。
- `im-core` 公开 DTO 是产品化 payload 闭环的前提，但 daemon 本地 MVP 可以先用 testable adapter / mock transport 跑通。
- user-service 的 registration token 与 daemon 内部 `runtime_rpc_token` 分开落地：前者是服务端身份注册授权，后者是本机 runtime 调 daemon 的短期授权。
- 本地 RPC 安全是 daemon MVP 必须门禁，不能作为后续优化。
- 每个阶段完成后必须做阶段级代码 review；阶段 review 未通过时，不能进入下一阶段。

## 8. 任务拆分

| 步骤 | 标题 | 依赖 | 输出 | 步骤文档 | 提交门禁 | 状态 |
|---|---|---|---|---|---|---|
| 01 | daemon MVP crate 与进程骨架 | 无 | `crates/awiki-deamon` crate、配置、状态根目录、`daemon.db`、`im-core` 初始化 | [steps/01-daemon-mvp-crate-and-process-skeleton.md](steps/01-daemon-mvp-crate-and-process-skeleton.md) | 必须 | 已完成 |
| 02 | 本地 RPC 安全与 CLI 封装器 | 01 | UDS 本地 RPC、`runtime_rpc_token`、方法分级、封装器命令 | [steps/02-local-rpc-security-and-cli-wrapper.md](steps/02-local-rpc-security-and-cli-wrapper.md) | 必须 | 已完成 |
| 03 | 通用 CLI 运行时插件 MVP | 01, 02 | 手工 runtime agent 配置、无界面 CLI driver、Skill callback 闭环 | [steps/03-generic-cli-runtime-plugin-mvp.md](steps/03-generic-cli-runtime-plugin-mvp.md) | 必须 | 已完成 |
| 04 | `im-core` payload 接口 | 协议仓库已完成 | `im-core`、Dart DTO、local projection、realtime 支持 payload | [steps/04-sdk-im-core-payload-interface.md](steps/04-sdk-im-core-payload-interface.md) | 必须 | 已完成 |
| 05 | message-service payload 支持 | 04，协议仓库已完成 | direct/group payload send、存储、history、realtime 支持 | [steps/05-message-service-payload-support.md](steps/05-message-service-payload-support.md) | 必须 | 已完成 |
| 06 | user-service registration token API | 无强依赖；阶段 B 内可并行 | daemon/runtime registration token 契约与实现 | [steps/06-user-service-registration-token-api.md](steps/06-user-service-registration-token-api.md) | 必须 | 已完成 |
| 07 | daemon agent 与 runtime agent 管理 | 01-06 | daemon DID 注册、runtime agent create/status、daemon 命令设计 | [steps/07-agent-registration-and-management.md](steps/07-agent-registration-and-management.md) | 必须 | 待开始 |
| 08 | 集成、系统测试与发布门禁 | 01-07 | 跨仓 E2E、安全审查、文档和发布检查清单 | [steps/08-integration-system-tests-and-rollout.md](steps/08-integration-system-tests-and-rollout.md) | 如有文件变更则必须 | 待开始 |

## 9. 执行账本

状态值：`待开始`、`进行中`、`审查中`、`阻塞`、`已提交`、`已完成`。

| 步骤 | 状态 | 分支 | 开始时间 | 完成时间 | 提交 | 审查证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | 已完成 | `feature/release-0526/awiki-deamon` | 2026-05-30 23:35:16 CST | 2026-05-31 00:33:19 CST | `e8f4fd1` | Review 已完成：crate 边界、路径处理、SQLite schema、config defaults、错误处理、测试和文档已审查；发现 state layout 目录未创建、manifest 有未使用依赖，均已修复。提交后 `git status --short --branch` 显示分支 ahead 1，工作区无未提交代码改动。 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；`cargo run -p awiki-deamon -- init-state --state-root <tmp>` 通过；源码边界 `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无结果；`git diff --check -- Cargo.toml Cargo.lock crates/awiki-deamon` 通过；`cargo test -p im-core --locked` 通过；`cargo test -p awiki-cli --locked` 通过；`cargo test --workspace --locked` 在 `im-core-dart` 链接动态库时被系统 SIGKILL。 | 开始步骤 02 |
| 02 | 已完成 | `feature/release-0526/awiki-deamon` | 2026-05-31 00:38:20 CST | 2026-05-31 01:08:54 CST | `395c815` | Review 已完成：token scope、hash 存储、过期/撤销/一次性使用、method/recipient 授权、UDS 权限、Linux `SO_PEERCRED`、macOS `getpeereid`、请求体身份字段不参与授权、audit 不记录 token 原文、CLI wrapper 边界和测试覆盖已审查；发现 macOS peer credential 分支缺失，已补齐。提交后 `git status --short --branch` 显示分支 ahead 3，仅剩计划台账更新待提交。 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；`git diff --check -- Cargo.toml Cargo.lock crates/awiki-deamon` 通过；源码边界 `rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无结果；secret 搜索确认生产代码无 token 原文日志，audit 测试确认只记录 `token_id`。 | 开始步骤 03 |
| 03 | 已完成 | `feature/release-0526/awiki-deamon` | 2026-05-31 01:14:03 CST | 2026-05-31 02:00:31 CST | `bc2458b` | Review 已完成：controller_did 文本路由、runtime profile/workspace 成组校验、runtime run 生命周期、local RPC token 回传、outbox test adapter、audit、RuntimeEvent 非权威通道、Debug token 脱敏、daemon/awiki-cli 边界和测试覆盖已审查；发现 schema migration 跳版本、run 状态更新未检查不存在的 run、runtime launch 失败时 run 可能停在 pending、非零退出会产生 final callback、callback Debug 可能泄露 token，均已修复。提交后 `git status --short --branch` 显示分支 ahead 1，仅剩计划台账更新待提交。 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；`cargo test --workspace --locked` 通过；`git diff --check -- Cargo.toml Cargo.lock crates/awiki-deamon` 通过；`git diff --check -- crates/awiki-deamon/docs` 通过；`rg -n "awiki_cli|awiki-cli|crates/awiki-cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` 无结果；`rg -n "RuntimeEvent" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/local-dev.md` 仅命中文档中“非权威通道”说明；secret 搜索确认生产代码无 token 原文日志，新增 Debug 脱敏测试通过。 | 执行阶段 A Review |
| 阶段 A Review | 已完成 | `feature/release-0526/awiki-deamon` | 2026-05-31 02:02:50 CST | 2026-05-31 02:03:46 CST | 无新增代码提交；本次为阶段 Review 台账更新 | 阶段 A Review 通过：步骤 01-03 的 daemon crate、配置、状态目录、`daemon.db` schema v3、本地 RPC token/UDS 安全、Generic CLI runtime MVP、workspace mode 文档、测试和提交记录已复核；未发现阻止进入阶段 B 的问题。 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；步骤 03 后 `cargo test --workspace --locked` 通过；daemon 源码/测试 awiki-cli 边界搜索无结果；旧字段和旧 content type 搜索无结果；`rg -n "RuntimeEvent" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/local-dev.md` 仅命中文档中“非权威通道”说明。 | 开始步骤 04 |
| 04 | 已完成 | `feature/release-0526/awiki-deamon` | 2026-05-31 02:09:13 CST | 2026-05-31 02:56:44 CST | `defc907` | Review 已完成：公开 DTO、direct/group 线协议、local projection、history/inbox/realtime payload 解析、Dart bridge、生成文件同步、CLI 兼容展示分支和文档已审查；发现 SDK 文档行尾空格、旧字段字面量验证会命中测试和历史记录、普通 workspace 测试首次在 `im-core-dart` 链接阶段被系统 SIGKILL，均已处理或用低并发重跑验证。实现提交后 `git status --short --branch` 显示分支 ahead 1，仅剩本台账更新待提交。 | `cargo fmt --all --check` 通过；`cargo test -p im-core --locked` 通过；`cargo test -p im-core-dart --locked` 通过；`cargo test -p awiki-deamon --locked` 通过；`scripts/flutter/codegen-check.sh` 通过；`CARGO_BUILD_JOBS=1 cargo test --workspace --locked` 通过；`git diff --check -- crates/awiki-cli crates/awiki-deamon/docs/create crates/im-core crates/im-core-dart docs/sdk-refactor packages/awiki_im_core` 通过；旧字段和旧 content type 搜索无结果；daemon 源码/测试 awiki-cli 边界搜索无结果。 | 开始步骤 05 |
| 05 | 已完成 | `feature/release-0526/daemon-payload-message-service` | 2026-05-31 03:02:41 CST | 2026-05-31 03:24:37 CST | `30eecf4` | Review 已完成：message-service direct 路径已有严格 `application/json + body.payload` 校验和存储/投影能力，本步骤补充 direct payload inbox/history/notification 回归测试；group 路径已审查并补齐 content-type/body 绑定、JSON object payload 校验、attachment/binary/history projection 和 incoming notification payload 保留；存储层已确认使用完整 `payload_json` / `wire_payload_json` 保存 `meta/body`，无需 migration；服务端不解释 daemon command/status/result schema，仅在测试和文档示例中作为不透明 payload 字段出现；旧字段和旧 content type 搜索无结果。提交后 message-service `git status --short --branch` 显示工作区干净。 | `cargo fmt --all --check` 通过；`cargo test -p im-group --locked group_incoming_notification_preserves_json_payload_body` 通过；`cargo test -p im-group --locked group_send` 通过；`cargo test -p im-group --locked group_list_messages_projects_payload_attachment_and_binary_content` 通过；`cargo test -p im-direct --locked json_payload` 通过；`cargo test -p im-direct --locked direct_send_rejects_non_object_json_payload` 通过；`cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`git diff --check -- docs/api crates` 通过；旧字段和旧 command/status/result 专用 content type 搜索无结果。 | 开始步骤 06 |
| 06 | 已完成 | `feature/release-0526/daemon-registration-token-user-service` | 2026-05-31 03:33:21 CST | 2026-05-31 04:22:36 CST | `4087e51`（user-service） | Review 已完成：API 契约、token hash 存储、一次性兑换、过期/撤销、scope mismatch、audit 隐私、DID/User 原子创建、文档一致性和测试覆盖已审查；发现 `one_time=false` 暴露了首版不支持的复用语义、过期 token 可被撤销为 revoked、测试文件名会加重仓库既有 pytest 顶层模块名冲突，均已修复。提交后 user-service `git status --short --branch` 显示工作区干净，分支已推送到 origin。 | `uv run ruff format src/user_service/app/agent_registration src/user_service/storage/sqlmodel/models/agent_registration.py tests/app/agent_registration` 通过；`uv run ruff check src/user_service/app/agent_registration src/user_service/app/app.py src/user_service/app/container.py src/user_service/storage/interfaces.py src/user_service/storage/types.py src/user_service/storage/sqlmodel/storage.py src/user_service/storage/sqlmodel/models/__init__.py src/user_service/storage/sqlmodel/models/agent_registration.py tests/app/agent_registration tests/conftest.py` 通过；`uv run python -m py_compile src/user_service/app/agent_registration/*.py src/user_service/storage/sqlmodel/models/agent_registration.py` 通过；`uv run python -m pytest tests/app/agent_registration -v` 通过，9 passed；`git diff --check -- SPEC.md docs src tests` 通过；secret/audit 搜索确认生产代码无 registration token 原文日志，audit 上下文只写 `token_id` 和 scope 元数据。`uv run python -m pytest tests -v` 仍因仓库既有同名测试文件收集冲突失败，新增 Step 06 测试改名后冲突从 5 个降到 3 个且不再来自 agent_registration；`uv run python -m pytest tests --import-mode=importlib -q` 结果为 635 passed、10 skipped、10 failed，失败集中在既有 DID profile / DID relationship 缺少 `did_auth_service` 注入和 Telegram bot-bound ticket，不在 Step 06 改动路径。 | 执行阶段 B Review |
| 阶段 B Review | 待开始 | 相关仓库分支 | 待定 | 待定 | 待定 | 待定 | 待定 | 步骤 04-06 完成后执行 |
| 07 | 待开始 | 当前仓库和服务端相关分支 | 待定 | 待定 | 待定 | 待定 | 待定 | 等待阶段 B Review |
| 08 | 待开始 | 集成分支 | 待定 | 待定 | 待定 | 待定 | 待定 | 等待步骤 01-07 |
| 阶段 C Review | 待开始 | 集成分支 | 待定 | 待定 | 待定 | 待定 | 待定 | 步骤 07-08 完成后执行 |

## 10. Codex Goal 执行协议

- 本计划是后续执行进度的唯一来源。
- 开始或恢复执行前，必须读取本计划、当前步骤文档、执行账本、架构文档和当前 `git status`。
- 除非本计划明确标记步骤可并行，否则一次只执行一个步骤。
- 恢复执行时，从第一个状态不是 `已完成` 的步骤继续。
- 每个步骤都按顺序执行：标记 `进行中`、实现、验证、审查、修复审查发现、提交、记录证据、标记 `已完成`。
- daemon 工程实现主目录固定为 `crates/awiki-deamon`；除根 `Cargo.toml` workspace member、明确列出的 SDK/service 跨仓步骤和系统测试外，不得在其他目录实现 daemon 逻辑。
- 不能带着上一依赖步骤的未提交完成工作进入下一步骤。
- 每步提交前记录 `git status` 和纳入文件；提交后记录 commit hash 和提交后的 `git status`。
- 如果需要改变范围、顺序、验收标准、公开契约、数据模型、安全假设或验证策略，必须先更新本计划。
- 本地 RPC 授权不能信任请求体中的 `agent_did` 等字段；可信上下文必须来自 daemon 根据 token 的反查。
- 结构化 JSON 只能使用 `application/json + body.payload`；不能新增 command/status/result 专用 JSON 内容类型。
- 阶段完成后必须执行阶段级代码 review，记录阶段审查证据和验证证据；阶段 review 未通过时，不得进入下一阶段。

## 11. 审查策略

- 每步实现后、提交前都必须做审查，优先检查正确性、回归风险、公开契约、数据安全、测试和文档漂移。
- 步骤 01 到步骤 03 重点审查 daemon 是否只在 `crates/awiki-deamon` 实现、是否没有依赖 awiki-cli 内部模块、本地 MVP 是否有可运行验证。
- 步骤 04 和步骤 05 重点审查 `application/json + body.payload` 在 SDK 与服务端之间是否一致。
- 步骤 02、步骤 05 和步骤 06 需要安全审查，重点是 token 存储、日志脱敏、审计字段、过期、撤销、UDS 权限和可信上下文。
- 步骤 08 做集成审查，确认 direct/group payload、registration token、daemon 本地 RPC 和 MVP runtime 闭环没有契约漂移。
- 阶段 A Review：检查 daemon crate 边界、DB/配置、本地 RPC、runtime 插件、测试和提交记录。
- 阶段 B Review：检查 SDK/service/token 契约、跨仓兼容性、安全设计、迁移和提交记录。
- 阶段 C Review：检查 agent 管理、系统测试、发布文档、安全/隐私和残余风险。

阶段 Review 记录模板：

| 阶段 | 状态 | 代码 Review 结论 | 发现 | 已修复 | 残余风险 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|
| 阶段 A | 已完成 | 通过。步骤 01-03 已形成可运行 daemon MVP：crate 和进程骨架、本地 RPC 安全、Generic CLI runtime 本地闭环均落在 `crates/awiki-deamon`，没有依赖 awiki-cli 内部模块。 | 未发现阻止进入阶段 B 的问题。 | 不适用；步骤内发现已在对应步骤修复并记录。 | 真实 payload 发送、真实 runtime CLI、长驻 listener、registration token 和跨服务 E2E 留到步骤 04-08。 | `cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked` 通过；步骤 03 后 `cargo test --workspace --locked` 通过；daemon 源码/测试 awiki-cli 边界搜索无结果；旧字段和旧 content type 搜索无结果。 | 进入步骤 04。 |
| 阶段 B | 待定 | 待定 | 待定 | 待定 | 待定 | 待定 | 待定 |
| 阶段 C | 待定 | 待定 | 待定 | 待定 | 待定 | 待定 | 待定 |

## 12. 验证策略

| 等级 | 命令或检查 | 预期证据 |
|---|---|---|
| 文档 | `git diff --check -- <changed-docs>` | 没有空白或补丁格式错误。 |
| 文档 | 搜索计划和架构文档中的旧字段名和旧 command/status 内容类型 | 没有旧字段名或旧 command/status 内容类型。 |
| daemon 主目录 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部模块。 |
| daemon 边界 | `rg -n "awiki-deamon" Cargo.toml crates/awiki-deamon` | daemon crate、源码、测试和文档均落在预期目录。 |
| 当前仓库 Rust | `cargo test --workspace --locked` | `im-core`、`im-core-dart`、`awiki-cli`、daemon crate 测试通过。 |
| 当前仓库格式 | `cargo fmt --all --check` | 格式检查通过。 |
| Dart/Flutter 桥接 | 仓库已有 codegen/check 脚本，或针对性 compile/test | DTO/API 桥接同步。 |
| message-service | `cd ../message-service && cargo test --workspace --locked` | direct/group payload API、存储、history、realtime 测试通过。 |
| user-service | `cd ../user-service && uv run pytest tests -v` | registration token API、过期、撤销、审计测试通过。 |
| 系统测试 | `cd ../awiki-system-test && <focused message-v2 / daemon E2E command>` | 跨服务 payload 和 daemon MVP 闭环通过。 |
| 安全审查 | 手工审查并记录到步骤文档 | token 原文不落日志，audit 只记录 `token_id`，UDS 和 peer credential 检查已验证。 |
| 阶段 Review | 阶段完成后手工代码 review 并记录到执行账本 | 阶段发现、修复、残余风险和验证证据完整。 |

命令可根据子仓库实际工具调整。不能运行的命令必须记录原因、替代验证和残余风险。

## 13. 文档更新

- Harness 文档：只有跨仓路由、验证策略、架构边界发生变化时才更新。
- message-service 文档：更新 direct/group API、存储/realtime 行为和错误码。
- user-service 文档：更新 `SPEC.md`、`docs/api/` 和数据库文档中的 registration token API 与表结构。
- 当前仓库文档：更新 SDK refactor 文档、daemon 架构文档和本计划执行账本。
- daemon 文档：维护 daemon CLI 命令设计、本地 RPC 安全、DB schema 和 runtime 插件文档。
- system-test 文档：记录新增 daemon/payload/token 测试套件和本地环境要求。

## 14. 提交计划

- 每个完成、验证、审查过的步骤必须有一个聚焦提交。
- 提交前记录 `git status` 和纳入文件。
- 提交后记录 commit hash 和工作区状态。
- 跨仓工作可能需要每个受影响仓库各一个聚焦提交；所有 hash 都要记录。
- 只有步骤 08 产生文件变更时才需要最终集成提交。
- 不能把所有步骤变更堆到最后一个大提交。

## 15. 阻塞处理

| 阻塞 | 步骤 | 证据 | 已尝试方案 | 影响范围 | 下一决策 |
|---|---|---|---|---|---|
| 待定 | 待定 | 待定 | 待定 | 当前步骤 / 整体计划 | 待定 |

- 阻塞必须同时记录在本计划和当前步骤文档中。
- 只有依赖允许且风险已记录时，才能继续另一个待执行步骤。
- 只有没有安全假设、替代路径或独立下一步时，才向用户请求决策。
- 如果服务端仓库缺失或有用户未提交变更，不能回退用户工作；只做安全读取或明确记录状态后做范围内修改。

## 16. 计划变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要审查 |
|---|---|---|---|---|
| 2026-05-30 | 初始计划创建到 `crates/awiki-deamon/docs/create/`。 | 用户要求创建 daemon 工程初始化计划。 | 全部 | 是 |
| 2026-05-30 | 根据架构文档更新，统一为 `application/json + body.payload`。 | 架构文档已取消 command/status/result 专用 JSON 内容类型。 | 01, 02, 07, 08 | 是 |
| 2026-05-30 | 删除协议仓库待执行阶段，将协议仓库修改作为已完成前置条件。 | 用户反馈协议仓库代码已经更新。 | 全部重新编号 | 是 |
| 2026-05-30 | 计划正文改为中文，保留必要代码标识、路径和命令名。 | 用户要求以后所有计划全部使用中文。 | 全部 | 是 |
| 2026-05-30 | 调整阶段顺序：daemon 工程骨架和本地 MVP 先于 SDK/service/token 产品化能力；补充阶段级代码 review 门禁。 | Review 发现原计划执行入口会先跑到 SDK/service，偏离 `crates/awiki-deamon` 初始化主线；用户要求每个阶段完成后做代码 review。 | 全部 | 是 |

## 17. 风险与回滚

| 风险 | 缓解 | 回滚或替代 |
|---|---|---|
| SDK 或服务端实现与协议仓库的 payload 契约漂移。 | 步骤 04 和步骤 05 增加契约测试，执行时参考协议仓库当前代码。 | 停止依赖步骤，先修正 SDK/service 契约。 |
| daemon 依赖 awiki-cli 内部模块。 | 固定依赖方向：daemon 和 awiki-cli 都依赖 `im-core`，daemon 只能参考 CLI 实现模式。 | 移除依赖，把可复用逻辑移到 `im-core` 或 daemon 自有模块。 |
| `controller_did` 对高风险自动执行偏弱。 | 文档明确为 MVP 取舍；高风险任务需要人工审批或 container/sandbox。 | 高风险 runtime profile 默认禁用自动写操作。 |
| 本地 RPC token 泄露。 | 步骤 02 要求 token 不写日志、audit 只记录 `token_id`、增加脱敏测试和安全审查。 | 撤销 token，轮换 daemon 本地 secret，修复日志与测试后再启用 runtime。 |
| workspace 安全被过度声明。 | `shared-root` 和 `worktree-per-task` 明确不是硬隔离；只有 container/sandbox 可作为安全边界。 | 高风险自动写代码只允许 container/sandbox。 |
| user-service token API 阻塞 daemon 注册。 | 步骤 01 可先支持手工本地 identity 配置；步骤 07 等待步骤 06 后再产品化注册。 | 暂时保留手工配置 MVP。 |
| 跨仓验证成本高。 | 每步使用聚焦单测/契约测试，步骤 08 做完整 E2E。 | 记录未运行测试和残余风险；没有 E2E 证据不能标记发布完成。 |
