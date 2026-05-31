# Step 04: Controller 消息执行链与 Prompt Wrapper

主计划: [../plan.md](../plan.md)  
步骤编号: 04  
状态：review

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | review |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-05-31 23:43:53 +0800 |
| 完成时间 | 未完成 |
| 提交 | 未提交 |
| 审查证据 | 2026-05-31 23:56:13 +0800 完成提交前 review：controller 校验、prompt wrapper、安全边界、fake callback token 替换和 final 主事实源均已检查；发现并修复 Hermes plugin 直接 launch 时 task/run/profile 不一致校验缺口。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_message` 通过，4 个测试；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，6 个匹配测试、1 个 ignored real smoke 被过滤；`cargo test -p awiki-deamon --test local_rpc_security --locked` 通过，6 个测试；`cargo test -p awiki-deamon --locked` 通过，52 个测试、1 ignored；secret/plugin 搜索仅命中预期测试、文档和生产 token 替换点；`git diff --check -- crates/awiki-deamon` 通过。 |
| 下一步 | 提交 Step 04 实现提交 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：让来自 `controller_did` 的 text/plain 消息进入 Hermes TUI Gateway，并让 Hermes 通过 daemon CLI wrapper/local RPC 回传状态和最终回复。
- 系统可见结果：daemon 收到发给 `runtime.hermes` agent 的 controller 消息后，创建 run、签发 run token、构造 prompt wrapper、提交 Hermes；fake Hermes 可调用 `task.status` 和 `task.finish`，controller 收到 progress/final。
- 非目标：不实现真实外发 `msg.send`；不做 failed final；不做幂等 final；不改产品层为 task。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/runtime/host.rs` | 适配 Hermes plugin launch path 或新增 message-driven host helper | 保留 generic-cli 路径。 |
| `crates/awiki-deamon/src/plugins/hermes/` | prompt builder、launch context、fake gateway callback behavior | 不把 token 写入持久 profile。 |
| `crates/awiki-deamon/src/inbox/mod.rs` | 可新增 message 语义 alias 或 helper | `RuntimeTask` 兼容名可保留。 |
| `crates/awiki-deamon/src/foreground.rs` | 将 text/plain route 按 `runtime_plugin_id` 选择 Hermes plugin，而不是固定 `UdsTestRuntimePlugin` | 完整长驻集成在 Step 07。 |
| `crates/awiki-deamon/src/local_rpc/mod.rs` | 如需补充 final/status 参数验证 | 不新增 failed final。 |
| `crates/awiki-deamon/tests/` | Hermes message execution tests | fake Hermes 触发 local RPC callbacks。 |

## 4. 依赖

- 前置步骤：Step 03。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 10、12-15、20 章。
- 环境前置条件：fake gateway tests；UDS local RPC tests 在 Unix 上运行。

## 5. 设计

### 路由条件

可执行消息必须满足：

```text
message target DID == Runtime Agent DID
agent_definition.runtime_plugin_id == "runtime.hermes"
sender_did == agent_definition.controller_did
content_type == text/plain
message text 非空
```

非 controller 消息继续 inbox only，不自动执行。

### run 与 token

沿用当前状态：

```text
pending -> running -> finished / failed
```

每次消息投递前签发 run token，允许方法：

```text
rpc.ping
task.status
task.finish
msg.send
artifact.created
```

本步骤可以允许 `msg.send` 出现在 token scope 中，但 fake Hermes 不测试真实外发；真实语义在 Step 05 门禁。

### prompt wrapper

Prompt wrapper 必须由 daemon 构造，包含：

- Agent：handle、agent DID、runtime、profile；
- Controller：controller DID、sender DID、`controller_verified: true`；
- Message：message id、run id、conversation id、publisher DID、content type；
- Allowed actions：report-status、finish-message、send-message；
- 禁止事项：不得直连 message-service、不得声称未成功发送的消息已发送；
- User message：原始用户文本。

Prompt wrapper 不是安全机制；安全仍由 run token、method scope、recipient scope、daemon audit 实现。

### final 主事实源

MVP 规则：

- Hermes streaming `message.complete` 只作为 observation。
- local RPC `task.finish` 是 authoritative successful final。
- failed 暂用 `task.status state=failed`，不调用 `task.finish`。
- Skill 要求最多调用一次 final；幂等 final 留后续增强。

### fake Hermes callback

测试用 fake Hermes 可以在收到 prompt 后模拟：

1. 调 `task.status state=running text="runtime started"`；
2. 调 `task.finish text="runtime finished"`。

这保持与当前 `UdsTestRuntimePlugin` 的测试语义接近，但 runtime plugin 已切到 Hermes 路径。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 04 标记为 `in_progress`。
2. 读取 `runtime/host.rs` 和 `foreground.rs`，确认 generic-cli 路径不被破坏。
3. 新增 prompt builder：
   - 输入 `RuntimeAgentProfile`、message context、run；
   - 输出完整 prompt wrapper 文本；
   - Debug 不打印 token 原文。
4. 将 Hermes plugin `launch_run` 或新的 native submit helper 接到 TUI Gateway：
   - 获取或创建 session 可先用内存 session，Step 06 持久化；
   - 提交 prompt；
   - 收集 observation；
   - 等待或触发 local RPC callbacks。
5. 修改 foreground text route：
   - `runtime.hermes` 用 Hermes plugin；
   - 其他 runtime 暂走原测试 plugin 或 generic path；
   - 非 runtime agent 仍报错或 inbox only。
6. 增加 tests：
   - controller text -> Hermes prompt submit -> status/final outbox；
   - non-controller 被拒绝；
   - prompt wrapper 包含 `controller_verified: true` 和 run id；
   - prompt wrapper 不包含 DID private key、JWT；
   - `message.complete` observation 不会自动 finish；
   - failed fake Hermes 只产生 failed status，不产生 final。
7. 运行验证，review，修复后提交。

## 7. 验收标准

- [ ] `runtime.hermes` agent 的 controller text/plain 消息可进入 Hermes fake gateway。
- [ ] 每次消息都创建 run 和短期 run token，不使用 profile 长期 token。
- [ ] Prompt wrapper 包含必要 message/run/controller 上下文，且不包含 private key/JWT。
- [ ] Hermes 通过 local RPC `task.status` / `task.finish` 回传状态和 successful final。
- [ ] 非 controller 消息不进入 Hermes 执行链。
- [ ] Streaming observation 不写 authoritative final。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 05。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| Hermes execution focused | `cargo test -p awiki-deamon --locked hermes_message` | controller route、prompt wrapper、status/final tests 通过。 |
| local RPC regression | `cargo test -p awiki-deamon --locked local_rpc` | token 授权和 UDS 回归通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| secret/debug | `rg -n "auth_private_key|jwt_token|runtime_rpc_token" crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests` | 没有泄露；测试中的假 token 只用于断言脱敏。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查 controller 校验、run token 生命周期、prompt wrapper、local RPC side effects、final 主事实源和 generic-cli 回归。
- 安全 review：请求体身份字段不可信；prompt 不是授权边界；profile 不持久化 run token。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | `HermesRuntimePlugin::launch_run` 直接调用时，已校验 run/profile binding，但还缺少 task 与 run/profile/controller 的一致性校验。 | host 正常路径已有 `route_controller_text_task`，但 plugin 边界仍应自守。 |
| 已修复 | 增加 `context.run.task_id == context.task.task_id`、`context.task.agent_did == HermesProfileRecord.agent_did`、`context.task.sender_did == context.task.controller_did` 校验；补 `hermes_gateway_plugin_rejects_mismatched_task_context` 测试。 | 已重跑 focused 和全量测试。 |
| 残余风险 | 长驻 daemon foreground 按 `runtime_plugin_id == "runtime.hermes"` 路由尚未接入；真实 Hermes TUI Gateway callback 协议仍未实现。 | 按计划分别留给 Step 07 和真实 smoke/adapter 后续收敛；本步骤仅证明 host helper + fake Hermes 消息执行闭环。 |
| 测试新增或缺失 | 新增 `hermes_message` focused tests 4 个，并扩展 `hermes_gateway` 到 6 个默认匹配测试和 1 个 ignored smoke。 | 覆盖 success final、failed status 无 final、非 controller 拒绝、prompt debug redaction、observation 不自动 final、binding 校验。 |
| 文档更新或缺失 | 本步骤执行记录已回填；主计划明确 Step 04 review 状态和验证证据。 | 未新增 prompt wrapper 独立规范，当前由测试锁定。 |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：Hermes controller message execution、prompt builder、tests 和相关文档。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: route controller messages to hermes`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| RuntimePlugin v1 无法安全表达 Hermes async callbacks | 记录调用点和 race | 使用 fake gateway 同步 callback；或引入局部 helper | 当前步骤 | 如需 RuntimePluginV2，先更新主计划 |
| foreground 当前 test plugin 与 Hermes 路由冲突 | 记录 failing tests | 按 runtime_plugin_id 分支，保留 test runtime | 当前步骤 | 修复分支后再提交 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 14. Step 04 执行记录

### 已实现

- 新增 `plugins/hermes/prompt.rs`，定义 `HermesPromptWrapper`，由 daemon 构造包含 agent、controller、message、run、allowed actions 和安全规则的 prompt wrapper；`Debug` redacted `user_message`。
- 扩展 `HermesPromptOutcome`，允许 fake gateway 返回 local RPC callback；`FakeHermesGateway` 新增 `FinishSuccessfully`、`FailWithStatus`、`ObserveOnly` 三种行为，并记录 submitted prompts 供测试断言。
- `HermesRuntimePlugin::launch_run` 校验 run/profile/task/controller 绑定，创建 session，提交 wrapped prompt，并把 fake callback 中的占位 token 替换为 daemon 为本 run 签发的 runtime RPC token。
- 新增 `tests/hermes_message.rs`，验证 controller text -> Hermes prompt -> `task.status` / `task.finish` -> outbox/status/final 闭环、failed status 不发送 success final、非 controller 不进入 gateway、prompt/debug 不泄露 token/private key/JWT。
- 扩展 `tests/hermes_gateway.rs`，显式用 `ObserveOnly` 覆盖 `message.complete` observation 不自动产生 final，并补直接 launch 的 task context mismatch 拒绝测试。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | `HermesRuntimePlugin::launch_run` 直接调用时缺少 task/run/profile/controller 一致性校验。 | host 正常路径已经验证 controller，但 plugin 边界需要防止被错误 context 直接调用。 |
| 已修复 | 增加 task_id、task agent、sender/controller 校验；补 `hermes_gateway_plugin_rejects_mismatched_task_context` 测试。 | 已重跑 Step 04 验证。 |
| 残余风险 | foreground 长驻 daemon 的 `runtime_plugin_id` 路由尚未接入；真实 Hermes callback 协议仍未启用。 | 按计划留给 Step 07；Step 04 不声明完整 long-running foreground 集成。 |
| 测试新增或缺失 | 新增 `hermes_message` 4 个测试，扩展 `hermes_gateway` 到 6 个匹配测试和 1 个 ignored smoke。 | 本步骤没有真实 `msg.send` direct/direct-e2ee 测试，留 Step 05。 |
| 文档更新或缺失 | 主计划和本步骤记录已同步 Step 04 review/验证证据。 | 未新增用户文档；prompt wrapper 当前属于内部实现。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes_message` | 通过：4 个 focused tests。 |
| `cargo test -p awiki-deamon --locked hermes_gateway` | 通过：6 个匹配测试，1 个 ignored real smoke 被过滤。 |
| `cargo test -p awiki-deamon --test local_rpc_security --locked` | 通过：6 个 local RPC security tests。 |
| `cargo test -p awiki-deamon --locked` | 通过：52 个测试，1 ignored，doc tests 0 个。 |
| `rg -n "auth_private_key\|jwt_token\|runtime_rpc_token\|plugin.yaml\|Awiki Hermes Plugin\|plugins/awiki-runtime" crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests crates/awiki-deamon/docs/hermes-plugin/plan` | 通过但有预期命中：生产代码只有 runner token 替换点和 fake callback 参数名；测试 fixture/断言、local RPC security tests、文档非目标说明和 profile plugin 禁止断言可保留。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 6]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/03-tui-gateway-runner.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/04-message-execution.md
 M crates/awiki-deamon/src/plugins/hermes/gateway.rs
 M crates/awiki-deamon/src/plugins/hermes/mod.rs
 M crates/awiki-deamon/src/plugins/hermes/runner.rs
 M crates/awiki-deamon/tests/hermes_gateway.rs
?? crates/awiki-deamon/src/plugins/hermes/prompt.rs
?? crates/awiki-deamon/tests/hermes_message.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/03-tui-gateway-runner.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/04-message-execution.md`
  - `crates/awiki-deamon/src/plugins/hermes/gateway.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/prompt.rs`
  - `crates/awiki-deamon/src/plugins/hermes/runner.rs`
  - `crates/awiki-deamon/tests/hermes_gateway.rs`
  - `crates/awiki-deamon/tests/hermes_message.rs`

### 提交后状态

- 实现提交：待回填。
- 实现提交后 `git status --short --branch`：待回填。
- 遗留未提交变更：待回填。
- 账本收尾提交：待回填。

## 13. 风险、回滚与后续

- 风险：prompt wrapper 文本过长或泄露上下文；fake gateway 不能代表真实 Hermes 行为；foreground 长驻路由仍待 Step 07。
- 回滚/fallback：回滚后 Hermes profile 仍存在，但 controller 消息不会通过 Hermes prompt wrapper 执行。
- 后续文档：若新增 prompt wrapper 格式文档，后续 Step 07/08 的系统测试需引用；真实 `msg.send` 外发语义在 Step 05 实现。
