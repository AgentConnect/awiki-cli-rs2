# Step 07: 长驻 daemon 集成与诊断

主计划: [../plan.md](../plan.md)  
步骤编号: 07  
状态：review

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | review |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-06-01 00:55:12 +0800 |
| 完成时间 | 未完成 |
| 提交 | 未提交 |
| 审查证据 | 2026-06-01 01:10:41 +0800 完成提交前 review：确认 foreground text route 已按 `runtime_plugin_id` 选择 Hermes 或 legacy test runtime；确认非 controller text 在进入 gateway 前被拒绝；确认 `agent-status` Hermes 诊断不输出 token/JWT/private key/prompt，并修复 `last_error` 可能透传敏感 audit detail 的风险；残余风险为真实 `StdioHermesGateway` 的 `session.create`/`prompt.submit` 仍是 Step 03 skeleton，需 Step 08/后续真实 adapter 验证。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_foreground` 通过，2 个 focused tests；`cargo test -p awiki-deamon --locked hermes_status` 通过，1 个 focused test；`cargo test -p awiki-deamon --locked` 通过，58 个测试、1 ignored；`cargo test --workspace --locked` 通过，所有 workspace crate 和 doc-tests 无失败；`git diff --check -- crates/awiki-deamon` 通过；awiki-cli 边界搜索无命中；secret 搜索仅命中测试脱敏样例、既有密钥/JWT 状态字段、诊断敏感标记列表和 prompt wrapper 相关代码。 |
| 下一步 | 提交 Step 07 实现后回填提交 hash，并启动 Step 08 整体验证 |

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

- [x] foreground 能按 `runtime_plugin_id == "runtime.hermes"` 路由到 Hermes plugin。
- [x] fake Hermes foreground 路由 helper 覆盖 text route -> run -> callback -> status/final；真实 inbox polling + remote message-service 留 Step 08 验证。
- [x] `msg.send` 在 foreground callback 中沿用 Step 05 的真实 message sender path，direct send 不再伪装成 status payload。
- [x] runner lifecycle 有 lazy 诊断状态；local RPC worker 使用 nonblocking UDS listener，可在 foreground shutdown 时 stop。
- [x] 诊断输出不泄露 token、JWT、private key、prompt 全文；`last_error` 已做敏感片段保守脱敏。
- [x] daemon 不依赖 `crates/awiki-cli` 内部模块。
- [x] 审查发现 已修复或明确记录。
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
| 发现 | `agent-status` 初始实现直接取 `hermes.error` audit detail 的 `error`/`reason` 字符串，未来如果错误 detail 错带 `rtok_`、JWT、private key 或 registration token 片段，诊断 JSON 会被动泄露；另一个缺口是真实 `StdioHermesGateway` 仍未实现未知 Hermes 协议。 | 诊断输出是本步骤新增对外面，因此需要 fail-closed；真实 Hermes adapter 超出 Step 07 已知协议范围，不能虚假实现。 |
| 已修复 | 新增 `public_hermes_error_detail`，对 `rtok_`、`tok_`、`runtime_rpc_token`、`registration_token`、`jwt`、`auth_private_key`、`private_key`、`secret`、`bearer` 等敏感片段返回稳定 `hermes.error` 摘要，并限制公开错误长度；管理命令测试新增含敏感 audit detail 的断言。 | 普通非敏感错误仍可作为诊断摘要返回。 |
| 残余风险 | production foreground 路由会使用 `StdioHermesGateway::from_env`，但真实 `session.create`/`prompt.submit` 仍是 Step 03 skeleton；没有在本步骤跑 live inbox polling + message-service + real Hermes 端到端；runner 状态目前诊断为 `lazy`，没有独立 child-process manager 状态。 | Step 08 必须用 `../awiki-system-test` remote `awiki.info` 完整测试记录真实结果；真实 Hermes adapter 未落地前不能声明 real Hermes ready。 |
| 测试新增或缺失 | 新增 foreground 内部测试：Hermes route 使用 fake gateway 并持久化 session、非 controller text 在 gateway 前被拒绝、conversation_id 不带 prompt 明文；新增管理命令测试：Hermes status 输出 profile/installation/session 且不泄露 secret。 | 未新增 live system-test；本步骤只覆盖 daemon 内部可确定路径。 |
| 文档更新或缺失 | 主计划和本步骤记录 Step 07 进度、审查、验证和残余风险；未新增单独 local-dev 文档，因为命令面选择为扩展既有 `agent-status` JSON。 | `agent-status` 扩展符合本步骤“如果 CLI 结构不适合新增子命令，可先扩展 agent status 输出”的设计。 |

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

## 14. Step 07 执行记录

### 已实现

- `foreground.rs` 的 text/plain runtime route 改为先读取 `RuntimeAgentProfile`，当 `runtime_plugin_id == "runtime.hermes"` 时加载 `hermes_profiles` 并创建 `HermesRuntimePlugin::with_state`；其他 runtime 保留 `UdsTestRuntimePlugin` 旧路径。
- production foreground 使用 `StdioHermesGateway::from_env` 作为 Hermes gateway factory；测试通过 `run_runtime_text_message_with_gateway` 注入 `FakeHermesGateway`，避免默认依赖真实 Hermes binary 或真实网络。
- Hermes foreground fake route 能通过 `run_controller_text_task` 签发 run token、提交 prompt、执行 fake callback，并经 `MemoryRuntimeOutbox` 观察 status/final；同一路由会写入 `hermes_native_sessions` active session。
- 非 controller text 使用既有 `controller_did` 校验，在创建 Hermes session 或提交 prompt 前失败；fake gateway create/submit 计数保持为空。
- `conversation_id` 对 direct message 只投影 peer DID，不复制 prompt 文本或 message body。
- `agent-status` JSON 对 Hermes runtime agent 新增 `hermes` 节点，包含 `agent_did`、`runtime_profile_id`、`hermes_profile`、`hermes_home`、`awiki_skills_version`、`profile_status`、`installation`、`active_session_count`、`runner_status` 和 `last_error`。
- `DaemonState` 新增 `count_active_hermes_sessions_for_agent`，供诊断输出 active session 数。
- `agent-status` 读取最新 `hermes.error` audit detail 作为只读诊断摘要；若包含 token/JWT/private key/secret/bearer 等敏感片段，返回稳定 `hermes.error`，不透传明文。
- 没有新增 `hermes status` 子命令；本步骤按既有 CLI 结构扩展 `agent-status`，保持管理命令面更小。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | `last_error` 初始实现可能透传未来错误 detail 中的敏感片段；真实 `StdioHermesGateway` 仍没有实现 Hermes `session.create`/`prompt.submit` 协议；runner status 目前是 lazy 诊断而非真实 child-process 状态。 | 诊断泄露已修复；真实 Hermes adapter 风险记录到 Step 08。 |
| 已修复 | 增加敏感片段检测和错误摘要长度限制；测试向 audit 写入含 `rtok_`、`jwt_token`、`auth_private_key` 的 `hermes.error`，断言 `agent-status` 输出只显示 `hermes.error`。 | 普通错误摘要仍可用于诊断。 |
| 残余风险 | production route 会走 `StdioHermesGateway::from_env`，但真实 Hermes session/prompt 协议仍未接线；本步骤没有 live inbox polling + remote message-service + real Hermes 系统测试；如果后续 foreground 并发处理同 conversation，Step 06 的首次同 route 并发创建仍可能由唯一约束 fail-closed。 | Step 08 远端系统测试必须明确通过/失败/跳过；真实 Hermes ready 不能只靠 fake route 证明。 |
| 测试新增或缺失 | 新增 3 个 foreground 内部测试和 1 个 Hermes status 管理命令测试；focused/full/workspace 验证均通过。 | 没有新增 `awiki-system-test` 用例，系统级验证留 Step 08。 |
| 文档更新或缺失 | 主计划和本步骤记录实现、验证、review 和残余风险；未更新 Harness 文档，未改变跨仓控制面规则。 | 未新增 local-dev 文档，因为命令面是既有 `agent-status` JSON 扩展。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes_foreground` | 通过：2 个 focused tests。 |
| `cargo test -p awiki-deamon --locked hermes_status` | 通过：1 个 focused test。 |
| `cargo test -p awiki-deamon --locked` | 通过：58 个测试、1 ignored，doc tests 0 个。 |
| `cargo test --workspace --locked` | 通过：`awiki-cli`、`awiki-deamon`、`im-core`、`awiki_im_core`、`xtask` 和 doc-tests 均无失败。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |
| `rg -n "crates/awiki-cli\|awiki_cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过：无命中，命令退出码 1 表示未找到。 |
| `rg -n "rtok_\|runtime_rpc_token.*println\|auth_private_key\|jwt_token\|prompt" crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过但有预期命中：测试脱敏样例、prompt wrapper 代码和断言、既有 agent auth/private key/JWT 状态字段、foreground JWT 选项传递、fake token placeholder、诊断敏感标记列表；未发现 token 原文 println/log。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 13]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/07-foreground-integration.md
 M crates/awiki-deamon/src/daemon_cli/mod.rs
 M crates/awiki-deamon/src/foreground.rs
 M crates/awiki-deamon/src/state/mod.rs
 M crates/awiki-deamon/tests/agent_registration_management.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/07-foreground-integration.md`
  - `crates/awiki-deamon/src/daemon_cli/mod.rs`
  - `crates/awiki-deamon/src/foreground.rs`
  - `crates/awiki-deamon/src/state/mod.rs`
  - `crates/awiki-deamon/tests/agent_registration_management.rs`

### 提交后状态

- 实现提交：待回填。
- 提交后 `git status --short --branch`：待回填。
