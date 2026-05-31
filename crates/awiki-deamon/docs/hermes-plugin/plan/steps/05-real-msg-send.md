# Step 05: 真实 `msg.send` 外发消息

主计划: [../plan.md](../plan.md)  
步骤编号: 05  
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
| 下一步 | 等 Step 04 完成后，把 `msg.send` 改为真实 ANP direct/direct-e2ee 外发 |

允许状态：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 目标：让 Hermes 通过 daemon CLI wrapper/local RPC 调用 `msg.send` 时，daemon 发送真正的 ANP direct/direct-e2ee 消息给目标 DID。
- 系统可见结果：`msg.send` 不再被实现成 status payload 模拟；recipient scope 越权会被拒绝；目标 DID 能通过 message-service 收到 Hermes agent 外发的文本消息。
- 非目标：不做 handle.resolve；不做 inbox.list/conversation.read；不扩展 approval；direct-e2ee 若环境缺少前置密钥可记录为后续 L3 gate，但 direct plain 必须真实通过。

## 3. 范围

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `crates/awiki-deamon/src/outbox/mod.rs` | 分离 status/final controller outbox 与 runtime `msg.send` direct outbox | `send_message` 必须发给 recipient。 |
| `crates/awiki-deamon/src/local_rpc/mod.rs` | 加强 `msg.send` params 校验、recipient 必填策略、side effect | 不信任请求体身份字段。 |
| `crates/awiki-deamon/src/im_core_adapter.rs` | 如需增加 agent identity client helper 或 direct-e2ee mode 选择 | 不重拼 message-service wire。 |
| `crates/awiki-deamon/src/security/runtime_token.rs` | 如需明确 allowed_recipients 缺省策略 | 建议 Hermes run 对 `msg.send` 默认要求 non-empty recipient scope，除非明确配置允许任意。 |
| `crates/awiki-deamon/src/plugins/hermes/skills.rs` | 更新 `awiki-messaging` Skill 文案 | 明确 send-message 成功才可声称已发送。 |
| `crates/awiki-deamon/tests/` | 单元和集成测试：recipient scope、真实 outbox adapter、fake im-core adapter | 避免默认打真实网络。 |
| `../awiki-system-test/tests_v2/daemon/` | 后续 Step 08 可新增系统测试；本步骤可先准备 focused helper | 跨仓变更需单独提交记录。 |

## 4. 依赖

- 前置步骤：Step 04。
- 外部文档或决策：[../../hermes_runtime_plugin_design.md](../../hermes_runtime_plugin_design.md) 第 7、13、15、19、21.4 章；Harness message-flow 和 E2EE 边界。
- 环境前置条件：本地 unit tests；真实 direct send 需要可用 user-service/message-service 和 agent auth token。

## 5. 设计

### 语义修正

当前 `RuntimeOutbox::send_message` 在 `ControllerRuntimeOutbox` 中会构造 `awiki.agent.status.v1` payload，状态为 `"message"`。Hermes 设计要求该语义必须改变：

```text
local RPC msg.send
  -> daemon 校验 token/method/recipient scope
  -> 使用 context.agent_did 对应 agent identity
  -> im-core messages.send direct/direct-e2ee
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

### direct / direct-e2ee

MVP 可支持参数：

```json
{
  "to": "did:wba:...",
  "text": "hello",
  "security": "default_plain|direct_e2ee"
}
```

若不加 `security`，使用 `MessageSecurityMode::DefaultPlain` 或 SDK default。direct-e2ee 支持应复用 `im-core`，不在 daemon 中处理密钥。

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
   - direct-e2ee security 参数只通过 SDK adapter，不在 daemon 中解密或持钥。
8. 如可用本地服务，运行 focused real-send smoke；否则记录未运行原因，Step 08 必须补系统测试。
9. 进入 review，修复后提交。

## 7. 验收标准

- [ ] `msg.send` side effect 是真实 direct/direct-e2ee send path，不是 status payload 模拟。
- [ ] `msg.send` recipient 和 text 参数有明确校验。
- [ ] recipient scope 越权被拒绝并有 audit。
- [ ] status/final 回 controller 的行为保持不回归。
- [ ] direct-e2ee 仅通过 `im-core` 能力调用，daemon 不持有 E2EE 私钥或明文密钥。
- [ ] `awiki-messaging` Skill 与真实发送语义一致。
- [ ] 审查发现 已修复或明确记录。
- [ ] 本步骤创建一个聚焦提交后才进入 Step 06 或 Step 07。

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
- 检查 side effect 方向、recipient scope、audit、错误处理、SDK 边界、E2EE 明文边界和系统测试缺口。
- 安全 review：`allowed_recipients` 缺省策略必须被明确审查；不能默认开放外发到任意 DID 而无记录。

| 审查项 | 结果 | 备注 |
|---|---|---|
| 发现 | 待记录 |  |
| 已修复 | 待记录 |  |
| 残余风险 | 待记录 |  |
| 测试新增或缺失 | 待记录 |  |
| 文档更新或缺失 | 待记录 |  |

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
| `im-core` 当前 public API 无法按 agent identity 发送 direct-e2ee | 记录缺失 API 和编译错误 | 先验证 direct plain；查 `im_core_adapter` helper | direct-e2ee 验收 | 如需改 `im-core` public API，更新主计划并拆跨仓步骤 |
| 远端/本地 message-service 不可用 | 记录 URL、错误、健康检查 | 使用 fake sender unit tests | real smoke | Step 08 必须补系统测试证据 |

## 12. 计划变更记录

| 日期 | 变更 | 原因 | 主计划变更日志链接 |
|---|---|---|---|
| 2026-05-31 | 创建步骤文档 | 初始计划拆分 | [../plan.md#14-计划变更日志](../plan.md#14-计划变更日志) |

## 13. 风险、回滚与后续

- 风险：recipient scope 过宽会产生滥发风险；direct-e2ee 前置状态不足会导致真实环境失败。
- 回滚/fallback：禁用 `awiki-messaging` Skill 的 send-message 指令，保留 status/final。
- 后续文档：如果新增 `security` 参数或 recipient policy，更新 Hermes design 和 local RPC docs。
