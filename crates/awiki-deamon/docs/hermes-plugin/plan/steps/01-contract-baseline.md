# Step 01: 契约与代码基线收敛

主计划: [../plan.md](../plan.md)  
步骤编号: 01  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | pending |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 未开始 |
| 完成时间 | 未完成 |
| 提交 | 未提交 |
| 审查证据 | 待记录 |
| 验证证据 | 待记录 |
| 下一步 | 对照 Hermes 设计文档和当前 daemon 代码冻结 MVP 契约 |

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

## 13. 风险、回滚与后续

- 风险：契约收敛不充分会导致后续步骤重复返工；过度测试内部命名会阻碍未来 rename。
- 回滚/fallback：回滚本步骤提交不会影响运行时代码；后续步骤不得开始。
- 后续文档：如果冻结契约改变设计文档结论，同步更新 [../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md)。
