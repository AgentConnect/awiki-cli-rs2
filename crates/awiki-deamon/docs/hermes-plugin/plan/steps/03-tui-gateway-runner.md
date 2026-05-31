# Step 03: TUI Gateway runner 与 plugin 骨架

主计划: [../plan.md](../plan.md)  
步骤编号: 03  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-05-31 23:34:15 +0800 |
| 完成时间 | 2026-05-31 23:42:25 +0800 |
| 提交 | 实现提交 `fa4668a206404e61beda09db1d5675548ff30731`；账本收尾提交待回填 |
| 审查证据 | 2026-05-31 23:41:03 +0800 完成提交前 review：Gateway trait 隔离协议字段；fake gateway deterministic；`StdioHermesGateway` 只做 installation check，未虚假实现未知 `session.create` / `prompt.submit` 协议；发现 `launch_run` 缺少 run/profile binding 校验，已修复并补测试。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_gateway` 通过，5 个默认测试、1 个 ignored；`cargo test -p awiki-deamon --locked` 通过，47 个测试、1 ignored；`cargo test -p awiki-deamon --locked hermes_real_smoke -- --ignored --nocapture` 通过并记录 `skipped: AWIKI_HERMES_BIN is not set`；secret/debug 搜索仅命中测试 fixture/脱敏断言和安全说明；`git diff --check -- crates/awiki-deamon` 通过。 |
| 下一步 | 启动 Step 04 Controller 消息执行链与 Prompt Wrapper |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：为 daemon-side `runtime.hermes` 建立可测试的 TUI Gateway runner，支持 installation check、runner 启动、`session.create`、`prompt.submit` 和 streaming event observation。
- 系统可见结果：fake Hermes Gateway 下可以创建 session、提交 prompt、接收 message.delta/message.complete 或等价 event；真实 Hermes binary 存在时可执行 ignored smoke test。
- 非目标：不接入 controller inbox，不写最终回复事实源，不实现真实 `msg.send`，不做 session 持久化完整策略。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/plugins/hermes/mod.rs` | 新增 `HermesRuntimePlugin` 骨架和 module exports | plugin_id 固定 `runtime.hermes`。 |
| `crates/awiki-deamon/src/plugins/hermes/gateway.rs` | TUI Gateway trait、stdio adapter、fake adapter | 真实协议差异集中在此。 |
| `crates/awiki-deamon/src/plugins/hermes/runner.rs` | runner lifecycle、session.create、prompt.submit、event stream | MVP 可用同步封装，避免一次性 async trait 重构。 |
| `crates/awiki-deamon/src/runtime/mod.rs` | 如有必要新增 native runtime context/event 类型 | 保持与 `RuntimePlugin` v1 兼容。 |
| `crates/awiki-deamon/src/plugins/mod.rs` | 暴露 `hermes` module | 不影响 generic_cli。 |
| `crates/awiki-deamon/tests/` | fake gateway runner tests、真实 Hermes ignored smoke | 不要求 CI 有 Hermes。 |

## 4. 依赖

- 前置步骤：Step 01、Step 02。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 5-6、9、20 章。
- 环境前置条件：Rust tests；真实 smoke 需要 `AWIKI_HERMES_BIN` 指向 Hermes binary。

## 5. 设计

### adapter 分层

建议引入可替换 adapter：

```rust
trait HermesGateway {
    fn check_installation(&self) -> Result<RuntimeInstallStatus>;
    fn start(&self, profile: &HermesProfileRecord) -> Result<HermesRunnerRef>;
    fn create_session(&self, runner: &HermesRunnerRef, request: HermesSessionCreateRequest) -> Result<HermesSessionRef>;
    fn submit_prompt(&self, session: &HermesSessionRef, request: HermesPromptSubmitRequest) -> Result<HermesPromptOutcome>;
}
```

实现可以先是同步 trait，因为当前 `RuntimePlugin` v1 是同步接口；真实 stdio 进程内部可用线程读取 stdout events。

`HermesGateway` 至少两个实现：

- `StdioHermesGateway`：启动真实 Hermes TUI Gateway，管理 stdin/stdout JSON-RPC。
- `FakeHermesGateway`：测试用，返回 deterministic session id 和事件序列。

### stdio JSON-RPC adapter

真实 adapter 的职责：

- 构造命令，工作目录和环境变量；
- 启动 Hermes TUI Gateway 进程；
- 发送 JSON-RPC request；
- 读取 line-delimited 或 framed response/event；
- 将 Hermes event 归一化为 daemon 内部 `HermesRuntimeEvent`；
- 进程退出时清理 child process。

必须避免：

- 在 daemon core 到处散落 Hermes JSON 字段；
- 在 prompt 中包含 token 原文以外的 secret；
- 将 streaming `message.complete` 直接写成 authoritative final。

### plugin 骨架

`HermesRuntimePlugin` 可以先实现 `RuntimePlugin` v1 的 `check_install_status`，但 `launch_run` 在本步骤只做最小 fake prompt submit 或返回明确未接入错误。完整消息执行链放到 Step 04。

如果为了测试可以让 `launch_run` 在 fake gateway 下提交 prompt 并返回 `RuntimeLaunchOutcome { status: Running, callbacks: vec![] }`，也必须在文档中说明 Step 04 才接管 local RPC final。

### event 模型

内部事件建议包含：

```text
runner.ready
session.created
prompt.submitted
message.delta
message.complete
tool.call.observed
error
runner.exited
```

其中 `message.complete` 只是 observation，不作为 run final 主事实源。主事实源仍是 Hermes 通过 wrapper/local RPC 调用 `task.finish`，Step 04 实现。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 03 标记为 `in_progress`。
2. 读取 Step 02 profile manager，确认可以拿到 `hermes_home`、`hermes_profile`、Skills version。
3. 新增 `plugins/hermes` module，先实现 types 和 fake gateway tests。
4. 实现 stdio adapter：
   - `AWIKI_HERMES_BIN` 或 config 指定真实 Hermes binary；
   - 未配置 binary 时 `check_install_status.installed = false`，detail 说明缺少 binary；
   - ignored smoke test 显式 skip 或 fail with context，不影响默认测试。
5. 加入 runner lifecycle：
   - `start_runner(agent_did)`；
   - `get_or_create_session` 的占位调用可先创建 native session，持久化在 Step 06；
   - `submit_prompt` 记录 observation events。
6. 增加 tests：
   - fake gateway start -> session.create -> prompt.submit -> events；
   - installation checker 无 binary 时返回 not installed；
   - debug/display 不泄露 prompt 中的 token；
   - `message.complete` 不自动产生 `task.finish` callback。
7. 运行验证，review，修复后提交。

## 7. 验收标准

- [ ] `runtime.hermes` plugin module 存在，plugin id 稳定。
- [ ] Hermes TUI Gateway 访问被封装在 adapter，不泄漏到 daemon core。
- [ ] fake gateway 可确定性验证 runner/session/prompt/event。
- [ ] 未安装真实 Hermes 时默认测试不失败，安装检查可解释原因。
- [ ] 真实 Hermes smoke test 可通过环境变量启用，并能记录实际 binary/version。
- [ ] streaming observation 不作为 authoritative final。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 04。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| Hermes fake focused | `cargo test -p awiki-deamon --locked hermes_gateway` | fake gateway runner/session/prompt/event 测试通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| ignored smoke collect | `cargo test -p awiki-deamon --locked hermes_real_smoke -- --ignored` | 有 `AWIKI_HERMES_BIN` 时通过；无 binary 时记录 skip 或 not-run 原因。 |
| secret/debug | `rg -n "runtime_rpc_token|rtok_|private key|jwt" crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests` | 无 token 原文日志；测试 fixture 中如有假 token 必须用于脱敏断言。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查 adapter 边界、进程生命周期、stdout/stderr 错误处理、event 主事实源边界、测试是否不依赖真实 Hermes。
- 安全 review：真实 stdio 进程 env 不应注入长期 secret；prompt 不作为安全机制。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：Hermes gateway/runner skeleton、fake tests、真实 smoke 测试和相关文档。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: add hermes tui gateway runner skeleton`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| Hermes TUI Gateway frame 协议不明确 | 记录真实命令、stderr、样例输出 | fake adapter 先落；真实 adapter behind ignored test | 真实 Hermes smoke | 后续根据 Hermes 文档或 binary 调整 adapter |
| 现有同步 RuntimePlugin 难以表达 streaming | 记录需要改动的 trait 和调用点 | 先用 runner 内部收集 events，Step 04 再接执行链 | 当前步骤设计 | 如需引入 RuntimePluginV2，先更新主计划 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 14. Step 03 执行记录

### 已实现

- 新增 `plugins/hermes/gateway.rs`，定义 `HermesGateway` trait、runner/session/prompt/event 类型、`FakeHermesGateway` 和 `StdioHermesGateway` installation checker。
- 新增 `plugins/hermes/runner.rs`，定义 `HermesRunner` 和 `HermesRuntimePlugin` 骨架；`RuntimePlugin::launch_run` 在 fake gateway 下 start -> session.create -> prompt.submit，并返回 `Running`、无 callbacks。
- `HermesPromptSubmitRequest` 的 `Debug` 输出会 redacted prompt，避免 prompt 中 token 或 secret 进入日志。
- 新增 `tests/hermes_gateway.rs`，覆盖 fake runner/session/prompt/event、`message.complete` 不产生 final callback、profile binding 校验、installation checker、prompt debug redaction、ignored real smoke。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | `HermesRuntimePlugin::launch_run` 初版未校验 `RuntimeLaunchContext` 与绑定 `HermesProfileRecord` 是否一致。 | 可能在后续 foreground 路由时误用错误 profile runner。 |
| 已修复 | 增加 agent_did、runtime_profile_id、runtime_plugin_id binding 校验；补 `hermes_gateway_plugin_rejects_mismatched_profile_binding` 测试。 | 已重跑 focused 和全量测试。 |
| 残余风险 | 真实 Hermes TUI Gateway 的 framing、JSON-RPC method 和 event schema 仍未接入。 | `StdioHermesGateway` 在本步骤只做 installation check；真实协议留给 Step 04/后续 smoke 按计划收敛。 |
| 测试新增或缺失 | 新增 `hermes_gateway` focused tests 5 个默认测试和 1 个 ignored smoke。 | 默认测试不依赖真实 Hermes binary。 |
| 文档更新或缺失 | 本步骤执行记录已回填；未新增 adapter runbook。 | 真实 protocol 确认后再补。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes_gateway` | 通过：5 个默认测试，1 个 ignored real smoke 被过滤。 |
| `cargo test -p awiki-deamon --locked` | 通过：47 个测试，1 ignored，doc tests 0 个。 |
| `cargo test -p awiki-deamon --locked hermes_real_smoke -- --ignored --nocapture` | 通过：1 个 ignored smoke 执行；当前 `AWIKI_HERMES_BIN` 未设置，测试输出 `skipped: AWIKI_HERMES_BIN is not set`。 |
| `rg -n "runtime_rpc_token\|rtok_\|private key\|jwt" crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests` | 通过但有预期命中：测试 fixture、脱敏断言、既有 local RPC security tests 和安全说明；生产 Hermes gateway/runner 无 token 原文日志。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 4]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/03-tui-gateway-runner.md
 M crates/awiki-deamon/src/plugins/hermes/mod.rs
?? crates/awiki-deamon/src/plugins/hermes/gateway.rs
?? crates/awiki-deamon/src/plugins/hermes/runner.rs
?? crates/awiki-deamon/tests/hermes_gateway.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/03-tui-gateway-runner.md`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/gateway.rs`
  - `crates/awiki-deamon/src/plugins/hermes/runner.rs`
  - `crates/awiki-deamon/tests/hermes_gateway.rs`

### 提交后状态

- 实现提交：`fa4668a206404e61beda09db1d5675548ff30731`
- 实现提交纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/03-tui-gateway-runner.md`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/gateway.rs`
  - `crates/awiki-deamon/src/plugins/hermes/runner.rs`
  - `crates/awiki-deamon/tests/hermes_gateway.rs`
- 实现提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 5]
```

- 遗留未提交变更：无。
- 账本收尾提交：待回填。

## 13. 风险、回滚与后续

- 风险：真实 Hermes TUI Gateway API 可能和假设不同；同步封装可能限制后续 cancel/streaming。
- 回滚/fallback：回滚本步骤后 profile/Skills 仍可保留，消息执行不能进入 Hermes。
- 后续文档：真实 Hermes 命令和 event schema 确认后，更新 Hermes design 或新增 adapter runbook。
