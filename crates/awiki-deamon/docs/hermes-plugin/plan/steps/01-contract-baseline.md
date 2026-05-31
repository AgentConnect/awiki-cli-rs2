# Step 01: 契约与代码基线收敛

主计划: [../plan.md](../plan.md)  
步骤编号: 01  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-05-31 23:02:52 +0800 |
| 完成时间 | 2026-05-31 23:17:21 +0800 |
| 提交 | 实现提交 `e4c46fc4cd6fcf7e44f793404640abc21c0cade4`；账本收尾提交待回填 |
| 审查证据 | 2026-05-31 23:13:02 +0800 完成提交前 review：契约文档与设计一致，未实现 Hermes Python plugin、profile、TUI Gateway、session 或真实外发；生产代码仅新增 `plugins::hermes` 常量；发现 focused 测试函数名未全部匹配 `hermes` 过滤器，已改名修复。 |
| 验证证据 | 启动前 `git status --porcelain=v1 -b` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes` 通过，5 个 hermes contract tests；`cargo test -p awiki-deamon --locked` 通过，39 个测试；`git diff --check -- crates/awiki-deamon/docs/hermes-plugin crates/awiki-deamon/tests crates/awiki-deamon/src/plugins` 通过；禁止项搜索仅命中文档非目标、测试断言和既有 `WorkspaceMode::Sandbox`。 |
| 下一步 | 启动 Step 02 Hermes profile 与 Awiki Skills 安装 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：在动核心实现前，把 Hermes MVP 的产品语义、兼容命名、代码缺口和测试策略收敛成可执行契约。
- 系统可见结果：仓库内存在 Hermes MVP contract 文档、代码基线审计记录或 contract tests，后续步骤可以直接引用，不再反复争论是否要做 Hermes plugin、approval、sandbox 或 product task protocol。
- 非目标：不实现 TUI Gateway，不创建 Hermes profile，不改 `msg.send` 真实发送，不做 schema migration。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| [../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) | 如实现前发现设计与当前代码不一致，补充“执行契约冻结”小节 | 保持中文，避免重写整篇设计。 |
| `crates/awiki-deamon/docs/hermes-plugin/plan/` | 更新本计划和账本 | 每个后续步骤引用本阶段输出。 |
| `crates/awiki-deamon/src/runtime/mod.rs` | 只在必要时新增 Hermes 兼容类型或注释，不做大重构 | `RuntimeTask` / `task.status` / `task.finish` 暂保留兼容名。 |
| `crates/awiki-deamon/src/plugins/mod.rs` | 可新增 `hermes` 模块占位和最小常量 | 不实现 runner。 |
| `crates/awiki-deamon/tests/` | 增加 contract tests 或 fixture，锁定非目标和兼容行为 | 可放 `hermes_contracts.rs`。 |

## 4. 依赖

- 前置步骤：无。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md)、[../../../awiki_agent_runtime_host_architecture.md](../../../awiki_agent_runtime_host_architecture.md)、[../../../create/plan.md](../../../create/plan.md)。
- 环境前置条件：Rust toolchain、当前 workspace 可运行 `cargo test -p awiki-deamon --locked`。

## 5. 设计

本阶段只冻结契约，不把后续实现塞进一个大提交。

核心决策必须被明确记录：

- Hermes MVP 是消息驱动：controller DID 发送 text/plain 到 Runtime Agent DID，daemon 校验后投递 Hermes，Hermes 回消息。
- 当前代码中的 `RuntimeTask`、`runtime_task`、`task.status`、`task.finish` 是历史兼容命名，不代表对 App 暴露完整 product task workflow。
- 不新增 `task.result`、不新增 `application/vnd.awiki...` content type；结构化 JSON 仍为 `application/json + body.payload`。
- 不安装 Hermes Python plugin，不写 `plugin.yaml`，不维护 Hermes 内部 awiki tool handler。
- Awiki Skills 只提供行为说明和 wrapper 调用约定，不是安全边界。
- local RPC 的可信上下文只能来自 run token 和 daemon 内部状态，不信任请求体自报身份。
- profile 初始化不得写入长期可用的 `msg.send` / `task.finish` token。
- `msg.send` 的目标契约是真实 ANP direct/direct-e2ee send，不是向 controller 发送状态 payload。

建议新增一个轻量 contract 文件或测试模块，例如：

```text
crates/awiki-deamon/tests/hermes_contracts.rs
```

测试可以先覆盖这些稳定点：

- `runtime_plugin_id("hermes") == "runtime.hermes"`。
- `task.status`、`task.finish` 仍能通过 `RpcMethod::parse`，后续 `message.status` / `message.finish` 尚未承诺。
- `msg.send` recipient scope 仍通过 token 控制。
- 非 controller 文本消息仍不能进入 runtime run。
- Hermes plan 中禁止的 `plugin.yaml` 路径不作为初始化产物。

如果执行者认为 contract tests 会过度绑定当前实现，可以改为新增 `docs/hermes-plugin/implementation-contract.md`，但必须保留可验证检查。

## 6. 细节与流程

1. 执行 `git status --short --branch`，确认是否有用户未提交改动；不得覆盖无关改动。
2. 读取本步骤文档、主计划、Hermes 设计文档、当前 `runtime/mod.rs`、`local_rpc/mod.rs`、`commands/mod.rs`、`foreground.rs`。
3. 对照设计文档列出当前已有能力和缺口，至少覆盖：
   - Runtime Agent 注册；
   - controller DID 校验；
   - local RPC token；
   - `task.finish` failed final 缺失；
   - `msg.send` 当前是否只走 `RuntimeOutbox::send_message`；
   - `UdsTestRuntimePlugin` 仍是测试 runtime；
   - session 表缺失；
   - Hermes profile/Skills/TUI Gateway 缺失。
4. 如需新增文档，放在 `crates/awiki-deamon/docs/hermes-plugin/` 下，正文中文。
5. 如需新增测试，保持 focused，不引入真实 Hermes binary 依赖。
6. 更新 [../plan.md](../plan.md) 的执行账本：状态、开始时间、验证证据、review 证据。
7. 进入代码 review，检查 contract 是否与设计一致、是否误引入后续实现。
8. 修复 review 发现后，创建一个聚焦提交。

## 7. 验收标准

- [ ] Hermes MVP 的非目标和安全边界在文档或 contract tests 中可被后续步骤引用。
- [ ] 当前兼容命名策略明确：`task.status` / `task.finish` 保留，但 Skill/Prompt 使用 message/run 语义。
- [ ] 未新增 Hermes Python plugin、`plugin.yaml`、approval、sandbox 或 product task result 协议。
- [ ] `msg.send` 真实发送缺口被明确列为 Step 05 的实现门禁。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 02。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。若只改 docs，可记录不适用，但 Rust 测试变更必须运行。 |
| daemon focused | `cargo test -p awiki-deamon --locked hermes` | 新增 Hermes contract tests 通过；如果没有测试名包含 `hermes`，记录实际测试名。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon/docs/hermes-plugin crates/awiki-deamon/tests` | 通过。 |
| 禁止项搜索 | `rg -n "plugin.yaml|plugins/awiki-runtime|approval|sandbox" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/hermes-plugin` | 只允许命中文档中的非目标说明；生产代码不得安装 Hermes plugin 或启用 approval/sandbox。 |

不能运行的命令必须记录原因、替代验证和残余风险。

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查正确性、契约稳定性、后续步骤可执行性、测试是否过度绑定、文档是否中文、是否扩大 scope。
- 安全 review 重点：不要把 prompt/Skill 当安全边界；不要允许 profile 长期持有可写 token。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

## 10. 提交要求

- 提交时机：本步骤实现、验证、review 修复完成后。
- 提交范围：只包含 Hermes 契约收敛文档、contract tests、极小 module 占位。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：如有未提交变更，说明是否用户改动或后续步骤依赖。
- 建议提交信息：`daemon: freeze hermes runtime contract baseline`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 当前设计文档与代码基线矛盾，无法安全冻结契约 | 记录冲突段落和代码位置 | 对照 create plan、runtime host docs、相关测试 | 当前步骤 | 先更新主计划变更日志，再决定修正文档还是调整步骤 |
| 现有测试全量失败且与本步骤无关 | 记录失败测试和错误 | 运行 focused tests、查看最近状态 | 验证证据 | 可提交但必须记录残余风险和不相关判断 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |
| 2026-05-31 | 执行中补充 Git hash 回填策略 | Git 提交无法在同一个提交中记录自身最终 hash，主计划已允许同一步账本收尾提交 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 14. Step 01 执行记录

### 已实现

- 新增 [../../implementation-contract.md](../../implementation-contract.md)，冻结 Hermes MVP 消息驱动语义、兼容 RPC 命名、安全边界、当前代码缺口和后续步骤引用要求。
- 新增 `crates/awiki-deamon/src/plugins/hermes/mod.rs`，只提供 `HERMES_RUNTIME_NAME` 和 `HERMES_RUNTIME_PLUGIN_ID` 常量，不实现 runner/profile/TUI Gateway。
- 新增 `crates/awiki-deamon/tests/hermes_contracts.rs`，覆盖 runtime plugin id、兼容 RPC 名称、recipient scope、controller 文本路由和非目标文档断言。

### 当前代码基线审计

| 项 | 证据 | 结论 |
|---|---|---|
| Runtime Agent 注册 | `commands::handle_agent_payload_message`、`agent::runtime_plugin_id("hermes")` | 已有创建入口，Step 02 可接 profile 初始化。 |
| controller DID 校验 | `commands` 校验 daemon command sender，`inbox::route_controller_text_task` 校验 runtime text sender | 后续 Hermes 路由必须复用或等价实现。 |
| local RPC token | `RuntimeTokenScope`、`DaemonState::authorize_runtime_rpc`、`local_rpc::execute_runtime_rpc_request` | 授权上下文来自 token，不能信任请求体自报字段。 |
| `task.finish` failed final | `local_rpc::apply_runtime_rpc_side_effects` 固定把 `task.finish` 落为 `finished` | 后续缺口，本步骤不修。 |
| `msg.send` | `RuntimeOutbox::send_message` 抽象存在，foreground `ControllerRuntimeOutbox::send_message` 仍发 status payload | 明确为 Step 05 门禁，不能算真实外发。 |
| `UdsTestRuntimePlugin` | foreground 文本与 `runtime.task.submit` 当前都构造测试 runtime | Step 07 才切换 `runtime.hermes` 路由。 |
| session/profile/TUI Gateway | 当前无 `hermes_profiles`、`hermes_native_sessions` 和 Hermes runner | 分别留给 Step 02、03、06。 |

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | focused 命令 `cargo test -p awiki-deamon --locked hermes` 最初只匹配到 1 个测试，覆盖不足。 | 测试函数名未全部带 `hermes`。 |
| 已修复 | 将新增 contract 测试函数统一改为 `hermes_*`，重跑 focused 命令后 5 个测试全部执行并通过。 | 已验证。 |
| 残余风险 | `msg.send` 真实 direct/direct-e2ee 外发仍未实现；`task.finish` failed final 仍未实现。 | 已作为 Step 05 和后续缺口记录，不属于 Step 01 范围。 |
| 测试新增或缺失 | 新增 `hermes_contracts.rs`。 | 不依赖真实 Hermes binary。 |
| 文档更新或缺失 | 新增 `implementation-contract.md`，回填主计划计划变更日志。 | 文档中文优先。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes` | 通过：5 个 `hermes_*` contract tests。 |
| `cargo test -p awiki-deamon --locked` | 通过：39 个测试，doc tests 0 个。 |
| `git diff --check -- crates/awiki-deamon/docs/hermes-plugin crates/awiki-deamon/tests crates/awiki-deamon/src/plugins` | 通过。 |
| `rg -n "plugin.yaml\|plugins/awiki-runtime\|approval\|sandbox" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/hermes-plugin` | 通过但有预期命中：仅文档非目标说明、测试断言和既有 `WorkspaceMode::Sandbox`；生产代码无 Hermes Python plugin 安装逻辑。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md
 M crates/awiki-deamon/src/plugins/mod.rs
?? crates/awiki-deamon/docs/hermes-plugin/implementation-contract.md
?? crates/awiki-deamon/src/plugins/hermes/
?? crates/awiki-deamon/tests/hermes_contracts.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/implementation-contract.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md`
  - `crates/awiki-deamon/src/plugins/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/tests/hermes_contracts.rs`

### 提交后状态

- 实现提交：`e4c46fc4cd6fcf7e44f793404640abc21c0cade4`
- 实现提交纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/implementation-contract.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md`
  - `crates/awiki-deamon/src/plugins/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/tests/hermes_contracts.rs`
- 实现提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 1]
```

- 遗留未提交变更：无。
- 账本收尾提交：待回填。

## 13. 风险、回滚与后续

- 风险：契约收敛不充分会导致后续步骤重复返工；过度测试内部命名会阻碍未来 rename。
- 回滚/fallback：回滚本步骤提交不会影响运行时代码；后续步骤不得开始。
- 后续文档：如果冻结契约改变设计文档结论，同步更新 [../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md)。
