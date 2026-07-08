# Step 02: Hermes profile 与 Awiki Skills 安装

主计划: [../plan.md](../plan.md)  
步骤编号: 02  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-05-31 23:19:11 +0800 |
| 完成时间 | 2026-05-31 23:32:43 +0800 |
| 提交 | 实现提交 `f8a0ae9b9994ebb7ebbee2aef48364a9d5fc6261`；账本收尾提交 `56b4c7ea50f498777bbe42d34e0f3a706f7f1f8f` |
| 审查证据 | 2026-05-31 23:31:00 +0800 完成提交前 review：schema migration 为 additive；profile home 派生在 `state_root/runtime/hermes/profiles/` 下；profile 不写 run token、DID 私钥或 JWT；发现并修复 profile 文案中的精确敏感字段名和 wrapper 配置夸大真实进程能力问题；同步 schema version 测试。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked hermes_profile` 通过，3 个测试；`cargo test -p awiki-deamon --locked` 通过，42 个测试；secret 搜索仅命中测试断言和安全说明；禁止 plugin 搜索仅命中测试断言；`git diff --check -- crates/awiki-deamon` 通过。 |
| 下一步 | 启动 Step 03 TUI Gateway runner 与 plugin 骨架 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：为 `runtime.hermes` Runtime Agent 创建本地 Hermes profile，并由 daemon 自动安装 Awiki Skills。
- 系统可见结果：`runtime.agent.create` 创建 Hermes agent 后，daemon 能落库 `hermes_profiles`，在 profile home 写入 SOUL.md/profile config/Skills，并完成无副作用 smoke test。
- 非目标：不启动真实消息执行，不调用 `msg.send`，不调用 `task.finish`，不安装 Hermes Python plugin，不写长期 run token。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/state/mod.rs` | 新增 `hermes_profiles` schema、migration、CRUD | schema version 递增，必须兼容旧 DB。 |
| `crates/awiki-deamon/src/plugins/hermes/` | 新增 profile manager、skill installer、installation checker | 可新增 `mod.rs`, `profile.rs`, `skills.rs`, `install.rs`。 |
| `crates/awiki-deamon/src/config.rs` | 如需新增 Hermes home/cache 路径，从 `state_root/runtime/hermes` 派生 | 路径必须在 `state_root` 下。 |
| `crates/awiki-deamon/src/commands/mod.rs` | `runtime.agent.create` 对 `runtime: "hermes"` 调用 Hermes 初始化 | user-service registration token 流程保持不变。 |
| `crates/awiki-deamon/docs/hermes-plugin/` | 记录 profile layout、Skills 内容和 smoke test 约束 | 中文。 |
| `crates/awiki-deamon/tests/` | 增加 profile/Skills 初始化测试 | 不依赖真实 Hermes。 |

## 4. 依赖

- 前置步骤：Step 01。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 的第 6-9、16 章。
- 环境前置条件：可写临时目录；无需真实 Hermes binary。

## 5. 设计

### Hermes profile 表

新增 `hermes_profiles`，建议字段与设计文档保持一致：

```sql
CREATE TABLE hermes_profiles (
  agent_did TEXT PRIMARY KEY,
  runtime_profile_id TEXT NOT NULL,
  hermes_profile TEXT NOT NULL,
  hermes_home TEXT NOT NULL,
  hermes_version TEXT,
  awiki_skills_version TEXT,
  status TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

实现要求：

- `agent_did` 与 daemon `agent_definition` 逻辑关联，不复制 DID 私钥。
- `hermes_home` 必须落在 `DaemonConfig.state_root` 下，默认可用：

```text
<state_root>/runtime/hermes/profiles/<stable-agent-segment>/
```

- `hermes_profile` 是 Hermes profile 名，使用稳定安全 segment，例如 `awiki_<handle>` 或 `profile_hermes_<handle>`。
- schema migration 使用 `CREATE TABLE IF NOT EXISTS` 和 `add_column_if_missing` 风格，旧 DB 可直接升级。

### profile 文件布局

建议落地：

```text
<hermes_home>/
├── SOUL.md
├── awiki-profile.json
└── skills/
    ├── awiki-runtime/
    │   └── SKILL.md
    ├── awiki-messaging/
    │   └── SKILL.md
    └── awiki-collaboration/
        └── SKILL.md
```

`awiki-profile.json` 只写低风险配置：

- agent DID、runtime_profile_id、controller_did；
- daemon CLI wrapper 路径或命令名；
- local RPC socket path；
- skills version；
- 明确 `run_capability_token` 不在 profile 中持久化。

不得写入：

- `runtime_rpc_token`；
- 可用于 `msg.send`、`task.finish` 的 profile token；
- DID private key；
- user JWT；
- Hermes Python plugin 配置。

### Skills 内容

`awiki-outbound-messaging/SKILL.md`：

- 指导 Hermes 外发消息必须调用 `awiki-deamon-runtime send`；
- 说明 daemon 会校验 run token、method scope、recipient scope；
- 禁止直接连接 message-service 或声称未成功发送的消息已发送。

### smoke test

初始化 smoke test 只允许：

- profile 目录可创建；
- Skills 文件存在且内容非空；
- daemon CLI wrapper 路径存在或命令可定位；
- local RPC socket path 配置格式正确；
- 可选 `rpc.ping` 使用专门低风险 test token 或 profile health token；不得使用可写 run token。

如果当前 daemon 尚无独立 wrapper binary，smoke test 可先检查 `CliWrapperRequest` 序列化和 `rpc.ping` handler，真实进程调用留给 Step 07。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 02 标记为 `in_progress`。
2. 读取 Step 01 输出，确认契约未变。
3. 在 `state/mod.rs` 增加 schema version 和 `hermes_profiles` migration；新增 store/load/upsert 方法。
4. 新增 Hermes profile manager：
   - 从 `RuntimeAgentProfile` 和 `DaemonConfig` 派生 stable profile name；
   - 创建目录；
   - 写 SOUL.md 和 `awiki-profile.json`；
   - 安装或更新 Skills；
   - 写入 `hermes_profiles`。
5. 修改 `create_runtime_agent` 流程：当 `runtime_plugin_id == "runtime.hermes"` 时，在 agent/profile/workspace 已落库后调用 Hermes 初始化。
6. 初始化失败时：
   - 不删除已成功注册的 DID，避免破坏 user-service 已兑换 token 的事实；
   - 将 Hermes profile 状态标记 `failed`；
   - 向 controller 回 failed 状态；
   - audit 记录失败原因但不记录 secret。
7. 增加测试：
   - Hermes runtime agent create 会写 `agent_definition`、`runtime_profile`、`hermes_profiles`；
   - profile 文件和 3 个 Skill 存在；
   - profile 内容不包含 token/private key；
   - 初始化不会创建 `plugins/awiki-runtime/plugin.yaml`；
   - 旧 DB migration 后有 `hermes_profiles`。
8. 运行验证，进入 review，修复发现后提交。

## 7. 验收标准

- [ ] `runtime.agent.create` 支持 `runtime: "hermes"` 并创建 Hermes profile。
- [ ] `hermes_profiles` schema 和 CRUD 有测试覆盖。
- [ ] Awiki Skills 由 daemon 写入 Hermes profile，且内容指导 wrapper/local RPC，不声称自己是安全边界。
- [ ] 初始化 smoke test 不使用可写 run token、不发送真实 ANP 消息、不调用 final。
- [ ] profile 中不持久化 DID 私钥、JWT、runtime_rpc_token 或可写 profile token。
- [ ] 未创建 Hermes plugin 目录、`plugin.yaml`、`tools.py` 等 Python plugin 文件。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 03。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| daemon focused | `cargo test -p awiki-deamon --locked hermes_profile` | profile/Skills/schema 测试通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| secret 搜索 | `rg -n "runtime_rpc_token|private.key|auth_private_key|jwt_token" <tmp profile evidence or tests>` | profile 产物测试确认不包含 secret；生产代码仅允许字段名处理。 |
| 禁止 plugin | `rg -n "plugin.yaml|plugins/awiki-runtime|tools.py|__init__.py" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无生产安装逻辑。 |
| 文档空白 | `git diff --check -- crates/awiki-deamon` | 通过。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查 schema migration、路径安全、secret 泄露、初始化失败语义、Skill 文案和用户可见状态。
- 安全 review：profile 初始化不得产生长期可写能力；Skills 不得绕过 daemon。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：Hermes profile/Skills 初始化、schema、测试和直接相关文档。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: initialize hermes profiles and skills`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| Hermes profile layout 与真实 Hermes 版本不一致 | 记录 Hermes version、profile docs 或错误 | 将 layout 封装在 profile manager；fake profile 测试先通过 | 真实 Hermes smoke | 先提交 daemon layout，Step 03 真实 gateway 时调整计划 |
| schema migration 无法兼容旧 DB | 记录旧 DB fixture 和错误 | 增加 migration test，使用 additive schema | 当前步骤 | 修复后才能提交 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 14. Step 02 执行记录

### 已实现

- 在 `state/mod.rs` 新增 `HermesProfileRecord`、`hermes_profiles` schema version 6、additive migration、`upsert_hermes_profile` 和 `load_hermes_profile`。
- 在 `plugins/hermes` 实现 Hermes profile manager：派生 `hermes_home`、写 `SOUL.md`、写 `awiki-profile.json`、安装 3 个 Awiki Skills、执行无副作用 smoke check，并禁止创建 Hermes Python plugin 目录。
- 在 `commands::create_runtime_agent` 中，仅当 `runtime_plugin_id == "runtime.hermes"` 时调用 Hermes profile 初始化；失败时记录 failed profile 和 audit，并复用现有 failed status 发送路径。
- 在 `cli_wrapper` 新增 `rpc_ping` 请求构造，作为 profile smoke test 的低风险 wrapper 请求形状证据，不调用真实 UDS 或可写 token。
- 新增 `tests/hermes_profile.rs` 覆盖 schema/CRUD、旧 DB migration、runtime agent create profile/Skills 安装、无 secret、无 plugin 目录和 wrapper `rpc.ping` 请求形状。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | profile config/SOUL 中最初出现精确 `runtime_rpc_token` 字段名；profile config 最初写成真实 `awiki-deamon local-rpc` 命令，可能夸大当前 wrapper 进程能力；schema version 断言仍为 5。 | 都属于 Step 02 直接问题。 |
| 已修复 | 将 profile 文案改为 `run capability token`；wrapper 配置改为 `library:awiki_deamon::cli_wrapper; process wrapper wired in Step 07`；新增 `CliWrapperRequest::rpc_ping` 形状测试；schema version 测试更新到 6。 | 已重跑验证。 |
| 残余风险 | 真实 Hermes profile layout 和真实 Hermes binary 兼容性尚未验证。 | 按计划留给 Step 03 真实 gateway/smoke。 |
| 测试新增或缺失 | 新增 `hermes_profile` focused tests 3 个。 | 不依赖真实 Hermes binary，不发送真实消息。 |
| 文档更新或缺失 | 本步骤执行记录已回填；未新增独立 profile layout 文档。 | 当前 layout 已在代码和测试中锁定，后续如真实 Hermes 需要调整再更新设计文档。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked hermes_profile` | 通过：3 个 focused tests。 |
| `cargo test -p awiki-deamon --locked` | 通过：42 个测试，doc tests 0 个。 |
| `rg -n "runtime_rpc_token\|private.key\|auth_private_key\|jwt_token" crates/awiki-deamon/src/plugins/hermes crates/awiki-deamon/tests/hermes_profile.rs` | 通过但有预期命中：测试断言确保 profile dump 不含这些字段名；生产代码仅包含“JWT stays in daemon-managed storage”安全说明。 |
| `rg -n "plugin.yaml\|plugins/awiki-runtime\|tools.py\|__init__.py" crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过但有预期命中：仅测试断言和 Step 01 contract test；生产代码无 Hermes Python plugin 安装逻辑。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 2]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md
 M crates/awiki-deamon/src/cli_wrapper/mod.rs
 M crates/awiki-deamon/src/commands/mod.rs
 M crates/awiki-deamon/src/plugins/hermes/mod.rs
 M crates/awiki-deamon/src/state/mod.rs
 M crates/awiki-deamon/tests/state_bootstrap.rs
?? crates/awiki-deamon/tests/hermes_profile.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md`
  - `crates/awiki-deamon/src/cli_wrapper/mod.rs`
  - `crates/awiki-deamon/src/commands/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/state/mod.rs`
  - `crates/awiki-deamon/tests/hermes_profile.rs`
  - `crates/awiki-deamon/tests/state_bootstrap.rs`

### 提交后状态

- 实现提交：`f8a0ae9b9994ebb7ebbee2aef48364a9d5fc6261`
- 实现提交纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/01-contract-baseline.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/02-profile-skills.md`
  - `crates/awiki-deamon/src/cli_wrapper/mod.rs`
  - `crates/awiki-deamon/src/commands/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/state/mod.rs`
  - `crates/awiki-deamon/tests/hermes_profile.rs`
  - `crates/awiki-deamon/tests/state_bootstrap.rs`
- 实现提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 3]
```

- 遗留未提交变更：无。
- 账本收尾提交：`56b4c7ea50f498777bbe42d34e0f3a706f7f1f8f`

## 13. 风险、回滚与后续

- 风险：profile layout 可能需要随 Hermes 真实版本调整；Skill 文案可能过强导致模型误以为具备权限。
- 回滚/fallback：回滚本步骤提交会移除 Hermes profile 初始化；已注册 runtime agent 可继续作为普通 agent 记录存在。
- 后续文档：若 profile layout 固化，补充 `crates/awiki-deamon/docs/hermes-plugin/profile-layout.md` 或更新设计文档。
