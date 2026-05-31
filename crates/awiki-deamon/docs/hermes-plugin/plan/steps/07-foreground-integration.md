# Step 07: 长驻 daemon 集成与诊断

主计划: [../plan.md](../plan.md)  
步骤编号: 07  
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
| 下一步 | 等 Step 02-06 完成后，把 Hermes 接入长驻 daemon foreground |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：让长驻 `awiki-deamon foreground` 在真实 inbox polling/local RPC worker 中选择并运行 Hermes runtime，提供基础诊断、runner 生命周期和 observability。
- 系统可见结果：daemon foreground 处理发给 Hermes Runtime Agent 的 controller 消息，启动/复用 Hermes runner/session，状态/final/外发消息都经 local RPC 和 `im-core`；诊断命令能报告 profile、runner、session、installation 状态。
- 非目标：不做 approval/sandbox，不做持久 inbox cursor 完整重构，除非当前 Hermes E2E 必须；不做 App UI。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/foreground.rs` | 按 `runtime_plugin_id` 路由到 Hermes plugin；管理 runner 生命周期 | 保留 test runtime 用于系统测试 fallback。 |
| `crates/awiki-deamon/src/plugins/hermes/` | runner manager、installation diagnostics、shutdown | 避免 child process 泄漏。 |
| `crates/awiki-deamon/src/daemon_cli/mod.rs` / `src/main.rs` | 增加 Hermes status/doctor 命令，如 `hermes status` 或 agent status 扩展 | 命令命名需贴合现有 CLI。 |
| `crates/awiki-deamon/src/state/mod.rs` | 如需 audit event、runner status 字段 | 不重复保存 transcript。 |
| `crates/awiki-deamon/docs/local-dev.md` 或 Hermes docs | 记录本地运行、env、fake/real Hermes 模式 | 中文。 |
| `crates/awiki-deamon/tests/` | foreground fake Hermes E2E、diagnostics tests | 不依赖真实网络。 |

## 4. 依赖

- 前置步骤：Step 02、Step 03、Step 04、Step 05、Step 06。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 20、22 章；[../../../create/plan.md](../../../create/plan.md) 的长驻 daemon E2E 经验。
- 环境前置条件：Unix UDS；fake Hermes gateway；可选 local/remote message-service。

## 5. 设计

### foreground 路由

当前 `foreground.rs` 对 text/plain 消息使用 `UdsTestRuntimePlugin`。本步骤应改为：

```text
load_runtime_agent_profile(target_agent_did)
  -> match runtime_plugin_id
     "runtime.hermes" => HermesRuntimePlugin
     "generic-cli" / test => existing path
     other => inbox only or unsupported runtime error
```

### runner lifecycle

长驻 daemon 应持有 runner manager：

- daemon 启动时不必为所有 Hermes agent 启动 runner，可 lazy start；
- 第一次消息到达时 start runner；
- runner crash 时记录 audit 和 status；
- daemon shutdown 时 stop child processes；
- ready file 可以包含 Hermes installation summary 或保持 daemon ready 与 runtime ready 分离。

### local RPC worker

继续使用 UDS local RPC worker：

- `rpc.ping` 支持 smoke；
- `task.status`/`task.finish` 回 controller；
- `msg.send` 真实外发；
- 所有 runtime callback 都通过 same token path。

### diagnostics

建议提供只读诊断：

```text
awiki-deamon hermes status --agent-did <did> --state-root <path>
```

输出 JSON 至少包含：

- agent_did；
- runtime_profile_id；
- hermes_profile；
- hermes_home；
- skills version；
- installation installed/detail；
- active session count；
- runner status；
- last error（如有，脱敏）。

如果当前 CLI 结构不适合新增子命令，可先扩展 `agent status` 输出或新增 `doctor`，但必须记录。

### observability

新增 audit event 类型建议：

```text
hermes.profile.initialize
hermes.runner.start
hermes.runner.stop
hermes.session.create
hermes.prompt.submit
hermes.event.observe
hermes.error
```

audit 不记录 prompt 全文、token secret、private key、JWT。可记录 message_id/run_id/session_id/status 和错误 code。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 07 标记为 `in_progress`。
2. 审计 `foreground.rs` 当前流程，找出 `UdsTestRuntimePlugin` 固定点。
3. 引入 Hermes plugin factory：
   - 从 config/state/gateway factory 创建；
   - 支持 fake mode 用于 tests；
   - 支持 real mode 用 `AWIKI_HERMES_BIN`。
4. 调整 `process_inbox_once` / `route_message`：
   - daemon agent JSON command 流程保持不变；
   - runtime agent text/plain 走 `runtime_plugin_id`；
   - non-controller 拒绝不应导致 daemon loop 崩溃，按现有错误策略记录。
5. runner shutdown：
   - foreground 退出时停止 RPC worker 和 Hermes runners；
   - 测试 child process 或 fake runner stop 被调用。
6. 加诊断命令或状态输出：
   - 路径和输出遵循现有 JSON command 风格；
   - 不引入 awiki-cli crate 依赖。
7. 增加 tests：
   - foreground fake Hermes 处理一条 text/plain，产生 status/final；
   - fake Hermes `send-message` 通过 Step 05 sender；
   - diagnostics 输出不含 secret；
   - unsupported runtime 不影响 daemon loop；
   - runner stop 被调用。
8. 更新本地开发文档。
9. 运行验证，review，修复后提交。

## 7. 验收标准

- [ ] foreground 能按 `runtime_plugin_id == "runtime.hermes"` 路由到 Hermes plugin。
- [ ] fake Hermes foreground E2E 覆盖 inbox -> run -> local RPC -> status/final。
- [ ] `msg.send` 在 foreground callback 中走真实 message sender path。
- [ ] runner lifecycle 有 start/stop/error audit 或诊断。
- [ ] 诊断输出不泄露 token、JWT、private key、prompt 全文。
- [ ] daemon 不依赖 `crates/awiki-cli` 内部模块。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 08。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| foreground focused | `cargo test -p awiki-deamon --locked hermes_foreground` | fake Hermes foreground tests 通过。 |
| diagnostics focused | `cargo test -p awiki-deamon --locked hermes_status` | 诊断输出测试通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| workspace | `cargo test --workspace --locked` | 通过或记录资源限制和 focused 替代。 |
| dependency boundary | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | 无结果。 |
| secret 搜索 | `rg -n "rtok_|runtime_rpc_token.*println|auth_private_key|jwt_token" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无 token 原文日志。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查 long-running loop 稳定性、runner lifecycle、error handling、diagnostic output、日志脱敏、系统测试可接入性。
- 安全 review：foreground 不应在非 controller 消息上自动执行；runner env 不注入长期 secret。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：foreground Hermes routing、runner lifecycle、diagnostics、tests/docs。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: integrate hermes runtime into foreground`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| foreground loop 当前缺少持久 cursor 导致重复消息 | 记录重复处理测试 | 进程内去重保留；如 Hermes E2E 必须，新增 processed message 表需更新计划 | 当前/后续 | 不为本步骤扩大到完整 cursor，除非必须 |
| 真实 Hermes child process 无法可靠 stop | 记录进程树和 timeout | fake runner 先通过；真实 smoke 标记风险 | real mode | 发布前补进程管理修复 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 13. 风险、回滚与后续

- 风险：foreground 真实网络和真实 Hermes 两个不稳定因素叠加，容易造成 flaky tests；inbox cursor 仍可能是后续产品化风险。
- 回滚/fallback：保留 profile/session/schema，禁用 foreground Hermes route，回到 test runtime。
- 后续文档：若新增诊断命令，更新 daemon local-dev 或 command docs。
