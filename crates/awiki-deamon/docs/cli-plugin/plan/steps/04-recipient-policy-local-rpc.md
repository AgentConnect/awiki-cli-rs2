# Step 04：Recipient policy、handle resolve 与 `msg.send` 审计

主 Plan：[../plan.md](../plan.md)  
Step index：04  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `feature/release-0526/codex-plugin-cli-rs2` |
| Started | 待记录 |
| Completed | 待记录 |
| Commit | 待记录 |
| Review evidence | 待记录 |
| Verification evidence | 待记录 |
| Next action | 设计并实现 recipient policy 到 local RPC 的授权链路 |

## 2. 目标

- 结果：`msg.send` 能发送给 profile/run policy 授权的非 controller DID/handle，并记录 handle resolve、recipient 授权、security mode、send result 或失败原因。
- 用户 / 系统可见行为：Codex/Hermes 等 runtime 通过同一 daemon CLI wrapper 调 `msg.send`，wrapper 返回成功前 runtime 不得声称发送完成。
- 非目标：不实现 approval UI，不默认开放全网通配，不新增 runtime 直连 message-service。
- 完成标准：local RPC tests 覆盖 controller-only、非 controller allowlist、handle allowlist、未授权拒绝、send result audit 和 token redaction。

## 3. 设计方法

- 设计边界：授权事实来自 daemon state 中的 token/profile/run policy，不信任 RPC 请求体自报身份字段。
- 核心决策：`RuntimeTokenScope.allowed_recipients` 不再固定 controller；它由 CLI runtime profile `recipient_policy_json` 和当前 run policy 生成。
- 契约 / API / 数据流：
  1. runtime wrapper 调 `msg.send`，参数包含 `to` 或 `recipient`、`text`、可选 `security`。
  2. daemon 解析原始 recipient。
  3. 如果是 handle，daemon resolve 为 DID。
  4. 授权同时检查原始 handle 和 resolved DID。
  5. 通过 IM Core SDK 发送 direct text / direct-e2ee。
  6. 写 audit：原始 recipient、resolved DID、security、授权结果、message id 或失败原因。
- 兼容性：Memory outbox 继续只记录测试副作用；foreground real outbox 走 IM Core SDK。
- 迁移策略：默认 policy 可以是 controller-only；显式 allowlist 才允许其他 DID/handle。
- 风险控制：send 失败时 local RPC 返回 `ok=false` 或 error，不能只写 status payload。

## 4. 实现方法

1. 定义 `RecipientPolicy`：
   - `allowed_dids: Vec<String>`
   - `allowed_handles: Vec<String>`
   - `allow_controller: bool`
   - `allowed_security: Vec<RuntimeMessageSecurity>`
   - MVP 不支持全局通配；如后续需要必须单独审批。
2. 在 `run_controller_text_task` 或 profile/run 准备阶段，从 `cli_runtime_profile.recipient_policy_json` 生成 token scope。
3. 修改 local RPC `msg.send` 流程，让 recipient resolve 在授权前完成；如果当前 SDK 没有 handle resolver，先补最小 adapter 或阻塞本步骤。
4. 修改 `RuntimeTokenScope::allows_recipient` 或新增 `allows_any_recipient_candidate`，同时接受 raw handle 和 resolved DID。
5. 扩展 `RuntimeOutbox::send_message` 返回 send result 或 message id；若 trait 变更影响大，可新增 result record API。
6. 新增 audit event，例如 `runtime.msg_send.authorize`、`runtime.msg_send.sent`、`runtime.msg_send.failed`。
7. 增加 final 幂等基础：`task.finish` 重复调用不得重复发送 final；若完整实现放到 Step 07，本步骤至少定义 state/API 钩子。
8. 扩展 `CliWrapperRequest::msg_send` 支持 `security` 和 handle/DID 参数。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/security/runtime_token.rs` | recipient candidates 授权 | 不信任请求体身份 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/local_rpc/mod.rs` | `msg.send` resolve + authorize + side effect | 核心链路 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/outbox/mod.rs` | send result / security / handle-DID model | 真实发送仍在 IM Core outbox |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/cli_wrapper/mod.rs` | wrapper request 参数扩展 | 与 Hermes/Codex 共用 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/src/runtime/host.rs` | token scope 由 recipient policy 生成 | 不再固定 controller-only |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/local_rpc_security.rs` | 扩展授权、handle、send result、audit 测试 | 重点测试面 |
| `codex-plugin-cli-rs2/crates/awiki-deamon/tests/hermes_message.rs` | 确认 Hermes `msg.send` 仍走同一链路 |  |

## 6. 依赖

- 前置步骤：Step 02、Step 03。
- 外部文档或决策：`generic_cli_runtime_plugin_design.md` 的 `msg.send` 必须语义。
- 环境前提：能在测试中 mock handle resolver 或使用确定性 fake resolver。

## 7. 验收标准

- [ ] token scope 不再硬编码只允许 controller DID。
- [ ] profile policy 可以允许一个非 controller DID，并允许 `msg.send` 成功。
- [ ] profile policy 可以允许一个 handle，daemon resolve 后对 raw handle 和 resolved DID 做授权检查。
- [ ] 未授权 recipient 被拒绝，且不产生 outbox send side effect。
- [ ] unsupported `security` 或 policy 不允许的 `security` 被拒绝。
- [ ] audit 不记录 token 原文，记录 token id、recipient、resolved DID、security、结果。
- [ ] wrapper 返回失败时，runtime 不能把消息发送当作成功。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd codex-plugin-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Local RPC tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test local_rpc_security --locked` | recipient policy、handle、send result、audit tests 通过。 |
| Hermes message tests | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --test hermes_message --locked` | Hermes callback 兼容通过。 |
| Daemon crate | `cd codex-plugin-cli-rs2 && cargo test -p awiki-deamon --locked` | daemon crate 全部通过。 |
| Secret grep | `cd codex-plugin-cli-rs2 && rg -n "rtok_|runtime_rpc_token.*println|registration_token|jwt_token|auth_private_key" crates/awiki-deamon/src crates/awiki-deamon/tests/local_rpc_security.rs` | 生产代码不得泄漏 token/secret；测试 fixture 预期命中需解释。 |

## 9. Review 环节

- Review 时机：local RPC / outbox / tests 完成后、commit 前。
- Review 重点：授权来源、handle resolve 时序、send result 是否真实、audit 是否足够、token 与 DID 私钥是否不泄漏、默认 policy 是否 fail closed。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 待记录 |  |
| 已修复问题 | 待记录 |  |
| 剩余风险 | 待记录 |  |
| 新增或缺失测试 | 待记录 |  |
| 已更新或缺失文档 | 待记录 |  |

## 10. Commit 要求

- Commit 时机：实现、验证、Review 完成后。
- Commit 范围：recipient policy、local RPC/outbox、tests、必要 docs。
- Commit 信息建议：`daemon: enforce recipient policy for runtime msg send`
- Commit 后记录 hash、`git status --short --branch` 和 carry-over。

## 11. 风险、假设与回滚

- 风险：handle resolver 不稳定会影响 `msg.send` 正确性。缓解：先用 fake resolver tests 固化接口；真实 resolver 不可用时标记 blocker。
- 假设：MVP 默认不允许全网发送。
- 回滚：回退本步骤后恢复 controller-only 或现有 token scope；后续 Codex driver 不应启用 `msg.send` 非 controller 能力。
