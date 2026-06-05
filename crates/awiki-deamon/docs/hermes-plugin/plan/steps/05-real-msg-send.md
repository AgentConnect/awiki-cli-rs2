# Step 05: 真实 `msg.send` 外发消息

主计划: [../plan.md](../plan.md)  
步骤编号: 05  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| 状态 | done |
| 分支 | `feature/release-0526/hermes-plugin-cli-rs2` |
| 开始时间 | 2026-06-01 00:02:56 +0800 |
| 完成时间 | 2026-06-01 00:38:29 +0800 |
| 提交 | 实现提交 `9ee0ac805f897d132a4b4127eed56bf8b4c68ed4` |
| 审查证据 | 2026-06-01 00:34:49 +0800 完成提交前 review：确认 `msg.send` 经 `im-core` 真实发送路径，不再伪装成 status payload；recipient/text/security 校验和 token recipient scope 生效。2026-06-05 修正：Hermes Skill 和 `awiki-deamon-runtime send` 只支持普通消息，不暴露 direct/group E2EE 路径。 |
| 验证证据 | 启动前 `git status --short --branch` 无未提交变更；`cargo fmt --all --check` 通过；`cargo test -p awiki-deamon --locked runtime_message_send_params_validate_and_map_security` 通过，1 个测试；`cargo test -p awiki-deamon --locked msg_send` 通过，3 个匹配测试；`cargo test -p awiki-deamon --locked hermes_message` 通过，6 个测试；`cargo test -p awiki-deamon --locked hermes_profile` 通过，3 个测试；`cargo test -p awiki-deamon --locked` 通过，54 个测试、1 ignored；`cargo test --workspace --locked` 通过；`git diff --check -- crates/awiki-deamon` 通过；边界/secret/plugin 搜索结果已记录在执行记录。 |
| 下一步 | 启动 Step 06 Hermes session 持久化与 resume/reset |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：让 Hermes 通过 daemon CLI wrapper/local RPC 调用 `msg.send` 时，daemon 发送真正的 ANP direct/group 普通消息给目标。
- 系统可见结果：`msg.send` 不再被实现成 status payload 模拟；recipient scope 越权会被拒绝；目标 DID 能通过 message-service 收到 Hermes agent 外发的文本消息。
- 非目标：不做 inbox.list/conversation.read；不扩展 approval；不在 Hermes Skill 暴露加密发送路径。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/outbox/mod.rs` | 分离 status/final controller outbox 与 runtime `msg.send` direct outbox | `send_message` 必须发给 recipient。 |
| `crates/awiki-deamon/src/local_rpc/mod.rs` | 加强 `msg.send` params 校验、recipient 必填策略、side effect | 不信任请求体身份字段。 |
| `crates/awiki-deamon/src/im_core_adapter.rs` | 如需增加 agent identity client helper | 不重拼 message-service wire。 |
| `crates/awiki-deamon/src/security/runtime_token.rs` | 如需明确 allowed_recipients 缺省策略 | 建议 Hermes run 对 `msg.send` 默认要求 non-empty recipient scope，除非明确配置允许任意。 |
| `crates/awiki-deamon/src/plugins/hermes/skills.rs` | 更新 `awiki-messaging` Skill 文案 | 明确 send-message 成功才可声称已发送。 |
| `crates/awiki-deamon/tests/` | 单元和集成测试：recipient scope、真实 outbox adapter、fake im-core adapter | 避免默认打真实网络。 |
| `../awiki-system-test/tests_v2/daemon/` | 后续 Step 08 可新增系统测试；本步骤可先准备 focused helper | 跨仓变更需单独提交记录。 |

## 4. 依赖

- 前置步骤：Step 04。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 7、13、15、19、21.4 章；Harness message-flow 边界。
- 环境前置条件：本地 unit tests；真实 direct send 需要可用 user-service/message-service 和 agent auth token。

## 5. 设计

### 语义修正

当前 `RuntimeOutbox::send_message` 在 `ControllerRuntimeOutbox` 中会构造 `awiki.agent.status.v1` payload，状态为 `"message"`。Hermes 设计要求该语义必须改变：

```text
local RPC msg.send
  -> daemon 校验 token/method/recipient scope
  -> 使用 context.agent_did 对应 agent identity
  -> im-core 普通 direct/group message send
  -> message-service 投递目标 DID
```

status/final 仍发给 controller；`msg.send` 发给 params.to/recipient。

### recipient scope

`rpc_recipient` 已从 params 的 `to` 或 `recipient` 提取目标。Step 05 必须明确：

- `msg.send` params 中 `to`/`recipient` 必填；
- 空 recipient 拒绝；
- token 中 `allowed_recipients` 为 Some 时必须匹配；
- Hermes run 是否允许 None 表示任意 recipient，需要显式配置，不得默认悄悄开放。

建议策略：

```text
Hermes MVP 默认 allowed_recipients = None 仅表示不做 recipient 限制，但需要在 profile/agent policy 中显式允许；
更保守的实现：Hermes run 默认 allowed_recipients = Some(vec![controller_did])，需要协作目标时由 controller message 或后续 policy 提供名单。
```

如果实现者选择任一策略，必须更新主计划变更日志和安全 review 记录。

### 普通消息安全模式

Hermes Skill 和 `awiki-deamon-runtime send` 只支持普通消息。产品 wrapper 不暴露 `--security` 参数；daemon 侧保持防御校验，手写 local RPC 若传入非普通 security 会被拒绝。

local RPC 侧的产品请求形态：

```json
{
  "to": "alice",
  "text": "hello"
}
```

产品请求不传 `security`。底层兼容测试可以保留旧 security parser，但 Hermes Skill/CLI 这条产品链路不得生成或描述加密参数。

### test adapter

为了不在 unit tests 打真实网络，建议定义内部 trait：

```rust
trait RuntimeMessageSender {
    fn send_runtime_message(&self, agent_did: &str, recipient: &str, text: &str, security: RuntimeMessageSecurity) -> Result<RuntimeMessageSendResult>;
}
```

生产实现使用 `ImCoreAgentOutbox` / `ImClient.messages().send_async`；测试实现记录 recipient/text/security。

## 6. 细节与流程

1. 更新主计划执行账本，将 Step 05 标记为 `in_progress`。
2. 审计当前 `send_message` 调用链：`local_rpc -> RuntimeOutbox -> ControllerRuntimeOutbox/RuntimeCallbackOutbox/MemoryRuntimeOutbox`。
3. 分离 outbox：
   - `send_status` 和 `send_final` 继续回 controller；
   - `send_message` 根据 recipient 真实 direct send；
   - `MemoryRuntimeOutbox` 继续记录 Message kind，用于 unit tests。
4. 加强 params validation：
   - 缺少 `to`/`recipient` 返回错误；
   - 缺少 text 或 text 空白返回错误；
   - unsupported security 返回错误。
5. 确认 token audit：
   - 授权成功/失败均写 audit；
   - audit 可记录 recipient hash 或 recipient DID，但不能记录 token secret；
   - method_level 保持可检索。
6. 更新 `awiki-messaging` Skill 文案。
7. 增加 tests：
   - `msg.send` 缺 recipient 被拒绝；
   - recipient scope 不匹配被拒绝，且没有 outbox side effect；
   - scope 匹配时调用 message sender，recipient/text/security 正确；
   - status/final 仍回 controller；
   - request debug spoof 字段不影响 agent_did/run_id；
   - Hermes Skill/CLI 不生成 security 参数，Hermes token policy 拒绝非普通消息。
8. 如可用本地服务，运行 focused real-send smoke；否则记录未运行原因，Step 08 必须补系统测试。
9. 进入 review，修复后提交。

## 7. 验收标准

- [x] `msg.send` side effect 是真实 direct/group 普通消息发送路径，不是 status payload 模拟。
- [x] `msg.send` recipient 和 text 参数有明确校验。
- [x] recipient scope 越权被拒绝并有 audit。
- [x] status/final 回 controller 的行为保持不回归。
- [x] Hermes Skill/CLI 只支持普通消息，不暴露加密发送路径。
- [x] `awiki-outbound-messaging` Skill 与真实发送语义一致。
- [x] 审查发现 已修复或明确记录。
- [x] 本步骤创建一个聚焦提交后才进入 Step 06 或 Step 07。

## 8. 验证方式

| 检查 | 命令或方法 | 预期证据 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过。 |
| local RPC send focused | `cargo test -p awiki-deamon --locked msg_send` | recipient scope、side effect、参数校验测试通过。 |
| Hermes messaging focused | `cargo test -p awiki-deamon --locked hermes_messaging` | Skill / fake Hermes send-message tests 通过。 |
| daemon 全量 | `cargo test -p awiki-deamon --locked` | 通过。 |
| 当前仓库 workspace | `cargo test --workspace --locked` | 通过或记录资源限制和 focused 替代。 |
| 边界搜索 | `rg -n "crates/awiki-cli|awiki_cli" crates/awiki-deamon` | daemon 不依赖 awiki-cli 内部模块。 |
| secret/audit | `rg -n "rtok_|runtime_rpc_token.*println|auth_private_key|jwt_token" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无 token 原文日志；测试只用于脱敏断言。 |
| real send smoke | `AWIKI_DAEMON_* cargo test -p awiki-deamon --locked hermes_real_msg_send -- --ignored` | 环境具备时通过；否则记录 not-run。 |

## 9. 审查流程

- 实现后、提交前必须进行审查。
- 检查 side effect 方向、recipient scope、audit、错误处理、SDK 边界、普通消息语义和系统测试缺口。
- 安全 review：`allowed_recipients` 缺省策略必须被明确审查；不能默认开放外发到任意 DID 而无记录。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 初始实现风险是 `msg.send` 可能继续被 foreground status 诊断计数为 controller status message；另一个安全决策点是 Hermes run token 若默认 `allowed_recipients = None` 会等价于任意 DID 外发。 | 两者都会扩大 Step 05 行为面：前者混淆 status/final 与 direct send 观测，后者扩大 Hermes callback 的外发权限。 |
| 已修复 | `ControllerRuntimeOutbox::send_message` 改为只调用 `ControllerOutboxSender::send_runtime_message`，不递增 `sent_status_messages` / `status_message_ids`；`run_controller_text_task` 签发 token 时使用 `Some(vec![profile.controller_did.clone()])`，controller text run 默认只能向 controller DID `msg.send`。 | 更宽 recipient policy 留后续显式配置，不由 prompt 文本决定。 |
| 残余风险 | 未执行真实网络 `hermes_real_msg_send` smoke；仓库当前没有该 ignored test 入口，且本步骤不引入外部服务依赖。 | Step 08 必须在 `../awiki-system-test` remote 模式记录真实普通消息外发结果；若跳过或失败必须写明原因。 |
| 测试新增或缺失 | 新增/扩展 `msg_send` focused tests、`hermes_message` fake callback tests、`local_rpc_security` 参数校验和 spoof 字段测试、`outbox` security 映射单测、`hermes_profile` Skill 文案断言。 | 未新增 live message-service smoke，避免 unit tests 默认打真实网络。 |
| 文档更新或缺失 | 已更新 Hermes messaging Skill 文案；主计划变更日志记录 controller-only recipient scope 默认策略；本步骤执行记录回填实现、验证和风险。 | 未更新独立 local RPC 用户文档，因为该接口仍是 daemon wrapper 内部能力。 |

## 10. 提交要求

- 提交时机：实现、验证、review 修复完成后。
- 提交范围：`msg.send` 真实发送、recipient scope、安全测试、Skill 文案。
- 提交前状态：记录 `git status --short --branch`。
- 纳入文件：记录纳入提交的文件。
- 提交后证据：记录 commit hash 和提交后 `git status --short --branch`。
- 遗留未提交变更：明确记录。
- 建议提交信息：`daemon: send real messages from hermes callbacks`

## 11. 阻塞处理

| 阻塞项 | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| `im-core` 当前 public API 无法按 agent identity 发送普通消息 | 记录缺失 API 和编译错误 | 查 `im_core_adapter` helper | 普通消息验收 | 如需改 `im-core` public API，更新主计划并拆跨仓步骤 |
| 远端/本地 message-service 不可用 | 记录 URL、错误、健康检查 | 使用 fake sender unit tests | real smoke | Step 08 必须补系统测试证据 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |
| 2026-06-01 | Hermes controller text 触发的 run token 默认只允许 `msg.send` 到 controller DID | 收敛 recipient scope 安全假设，避免默认开放任意 DID 外发 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |
| 2026-06-05 | Hermes Skill/CLI 出站消息只支持普通消息 | 本期产品链路不做加密消息；避免 peer/prekey 状态影响联调 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 14. Step 05 执行记录

### 已实现

- 新增 `RuntimeMessageSend` 和 `RuntimeMessageSecurity`，统一解析 `msg.send` 参数：`to`/`recipient` 必填且非空，`text` 必填且非空。Hermes Skill/CLI 产品链路只生成普通消息请求，不生成 security 参数。
- `local_rpc` 的 `RpcMethod::MsgSend` side effect 改为先构造 `RuntimeMessageSend`，再调用 `RuntimeOutbox::send_message`；授权仍由 runtime token 的 method scope 和 recipient scope 在 side effect 前完成，不信任请求体中的 spoof 字段。
- `ImCoreAgentOutbox` 新增 `send_text_async` / `send_text`，生产路径通过 `im-core` `messages().send_async` 发送 `MessageBody::Text`；Hermes controller final、welcome、主动外发和附件发送均使用 `MessageSecurityMode::DefaultPlain`。
- `ControllerRuntimeOutbox::send_message` 不再构造 `awiki.agent.status.v1` payload；status/final 仍回 controller，`msg.send` 只走 direct message path，且不计入 foreground status 发送计数。
- `run_controller_text_task` 为 Hermes controller message run 签发带 recipient policy 的 runtime token；Hermes 默认允许 controller、active handle lookup 和 group target，但 `allowed_message_security` 只允许 `default_plain`。
- fake Hermes 新增 `SendMessage` 行为，覆盖 Hermes callback 发起 `msg.send`；`MemoryRuntimeOutbox` 记录 recipient、text 和 security mode，供 focused tests 验证 side effect。
- 更新 `awiki-outbound-messaging` Skill 文案，统一使用 `awiki-deamon-runtime send`，支持 handle/group 的文本和“caption + 附件”同一条普通消息，不出现加密发送参数。

### Review 记录

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | `msg.send` 若复用 status payload 计数会让 foreground 诊断误把 direct send 当作 controller status；recipient scope 默认开放会扩大 Hermes 外发权限。 | 已按真实 direct send 和 controller-only 默认策略修正。 |
| 已修复 | `send_message` 生产路径只调用 `ImCoreAgentOutbox::send_text`；status/final 继续用 `send_status_payload`；run token 默认 recipient scope 为 controller DID；补参数校验、security 映射和越权测试。 | prompt 仍不是安全边界，local RPC token 才是授权边界。 |
| 残余风险 | 没有真实网络 `hermes_real_msg_send` smoke。 | Step 08 远端完整系统测试必须记录真实普通消息结果；若跳过或失败必须写明原因。 |
| 测试新增或缺失 | 新增 `runtime_message_send_params_validate_and_map_security`、`msg_send_requires_recipient_text_and_supported_security`、`msg_send_records_direct_message_side_effect_with_security_mode`、Hermes fake send-message tests，并扩展 Skill 文案断言。 | 没有新增 live smoke 测试入口。 |
| 文档更新或缺失 | 主计划假设和变更日志已记录 controller-only recipient scope 默认策略；本步骤记录真实 send path 和验证证据。 | 未修改 Harness 文档，未发生跨仓控制面规则变化。 |

### 验证记录

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过。 |
| `cargo test -p awiki-deamon --locked runtime_message_send_params_validate_and_map_security` | 通过：1 个测试。 |
| `cargo test -p awiki-deamon --locked msg_send` | 通过：3 个匹配测试。 |
| `cargo test -p awiki-deamon --locked hermes_message` | 通过：6 个测试。 |
| `cargo test -p awiki-deamon --locked hermes_profile` | 通过：3 个测试。 |
| `cargo test -p awiki-deamon --locked` | 通过：54 个测试，1 ignored，doc tests 0 个。 |
| `cargo test --workspace --locked` | 通过：workspace 各 crate 单元、集成和 doc tests 均通过。 |
| `git diff --check -- crates/awiki-deamon` | 通过。 |
| `rg -n "crates/awiki-cli\|awiki_cli" crates/awiki-deamon/Cargo.toml crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过：无命中，daemon 未依赖 awiki-cli 内部模块。 |
| `rg -n "rtok_\|runtime_rpc_token.*println\|auth_private_key\|jwt_token" crates/awiki-deamon/src crates/awiki-deamon/tests` | 通过但有预期命中：测试假 token/脱敏断言、生产字段名、状态存储、`Debug` 脱敏实现、fake callback placeholder；未发现 token 原文日志。 |
| `rg -n "plugin.yaml\|Awiki Hermes Plugin\|plugins/awiki-runtime" crates/awiki-deamon/src crates/awiki-deamon/tests crates/awiki-deamon/docs/hermes-plugin` | 通过但有预期命中：文档非目标说明和测试禁止断言；生产代码无 Hermes Python plugin 安装逻辑。 |
| `rg -n "hermes_real_msg_send\|real_msg_send" crates/awiki-deamon/src crates/awiki-deamon/tests` | 无命中；真实网络 smoke 未运行，因为当前仓库没有该 ignored test 入口，真实 remote 验证留 Step 08。 |

### 提交前状态

- `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 9]
 M crates/awiki-deamon/docs/hermes-plugin/plan/plan.md
 M crates/awiki-deamon/docs/hermes-plugin/plan/steps/05-real-msg-send.md
 M crates/awiki-deamon/src/foreground.rs
 M crates/awiki-deamon/src/local_rpc/mod.rs
 M crates/awiki-deamon/src/outbox/mod.rs
 M crates/awiki-deamon/src/plugins/hermes/gateway.rs
 M crates/awiki-deamon/src/plugins/hermes/mod.rs
 M crates/awiki-deamon/src/runtime/host.rs
 M crates/awiki-deamon/tests/hermes_message.rs
 M crates/awiki-deamon/tests/hermes_profile.rs
 M crates/awiki-deamon/tests/local_rpc_security.rs
```

- 纳入文件：
  - `crates/awiki-deamon/docs/hermes-plugin/plan/plan.md`
  - `crates/awiki-deamon/docs/hermes-plugin/plan/steps/05-real-msg-send.md`
  - `crates/awiki-deamon/src/foreground.rs`
  - `crates/awiki-deamon/src/local_rpc/mod.rs`
  - `crates/awiki-deamon/src/outbox/mod.rs`
  - `crates/awiki-deamon/src/plugins/hermes/gateway.rs`
  - `crates/awiki-deamon/src/plugins/hermes/mod.rs`
  - `crates/awiki-deamon/src/runtime/host.rs`
  - `crates/awiki-deamon/tests/hermes_message.rs`
  - `crates/awiki-deamon/tests/hermes_profile.rs`
  - `crates/awiki-deamon/tests/local_rpc_security.rs`

### 提交后状态

- 实现提交：`9ee0ac805f897d132a4b4127eed56bf8b4c68ed4`
- 实现提交后 `git status --short --branch`：

```text
## feature/release-0526/hermes-plugin-cli-rs2...origin/feature/release-0526/hermes-plugin-cli-rs2 [ahead 10]
```

- 遗留未提交变更：无。

## 13. 风险、回滚与后续

- 风险：recipient scope 过宽会产生滥发风险；普通消息链路仍依赖 message-service、handle 解析和 group 可达性。
- 回滚/fallback：禁用 `awiki-outbound-messaging` Skill 的 send 指令，保留 status/final。
- 后续文档：如果新增 recipient policy，更新 Hermes design 和 local RPC docs；除非另有计划变更，Skill 不新增加密发送参数。
