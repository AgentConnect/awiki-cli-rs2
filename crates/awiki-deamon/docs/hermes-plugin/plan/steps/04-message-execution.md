# Step 04: Controller 消息执行链与 Prompt Wrapper

主计划: [../plan.md](../plan.md)  
步骤编号: 04  
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
| 下一步 | 等 Step 03 完成后，把 controller text/plain 消息投递到 Hermes |

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
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

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

## 13. 风险、回滚与后续

- 风险：prompt wrapper 文本过长或泄露上下文；fake gateway 不能代表真实 Hermes 行为。
- 回滚/fallback：回滚后 Hermes profile 仍存在，但 controller 消息不会执行。
- 后续文档：若新增 prompt wrapper 格式文档，后续 Step 07/08 的系统测试需引用。
