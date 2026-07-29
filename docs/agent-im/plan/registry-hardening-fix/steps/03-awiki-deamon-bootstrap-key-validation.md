# Step 03：awiki-deamon bootstrap private package 早期校验

主 Plan：[../plan.md](../plan.md)  
Step index：03  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2` 当前分支 |
| Started | 2026-06-10T03:00:09Z |
| Completed | 2026-06-10T03:40:25Z |
| Commit | `awiki-cli-rs2` `1596856` (`awiki-deamon: validate delegated bootstrap keys`) |
| Review evidence | Review 完成；发现并修复 `controller_did` 未显式绑定 `user_subkey_package.user_did` 的缺口；确认坏 package / resolve failure 不落库、secret 不进入 Debug/audit hash、resolver 不信任 delegated shadow identity cache、delegated inbox sync 前复查当前 DID Document。 |
| Verification evidence | `cargo fmt --check -p awiki-deamon` 通过；`cargo test -p awiki-deamon --locked -j1 bootstrap -- --nocapture` 21 passed；`cargo test -p awiki-deamon --locked -j1 user_delegated -- --nocapture` 9 passed；`cargo test -p awiki-deamon --locked -j1` 全量通过，lib 105 passed，集成测试套件通过，Hermes 真实环境 3 ignored；`git diff --check` 通过；secret 搜索无新增日志/audit 泄露。 |
| Next action | Step 04：key package schema 与 APP action capability 收口 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：Daemon 在保存 user delegated identity、创建 personal agent binding 或启动 delegated inbox sync 前，先验证 APP 发送的 private package 与当前 DID Document `authentication` 一致。
- 用户 / 系统可见行为：错误 key package、过期 package、public/private 不匹配、DID Document 不含该 authentication method、public key 不一致时，bootstrap 被拒绝并返回可诊断错误，不创建坏 binding 或不可用 Personal Agent。
- 非目标：不新增第二条 pairing channel；不解决 bootstrap 传输加密；不让 Hermes runtime 接触 private key；不改变 message-service 运行时授权模型。
- 完成标准：bootstrap validation 覆盖 key parse、private/public derive、DID Document authentication、public key match、expires_at、allowed scopes 和 secret redaction。

## 3. 设计方法

- 设计边界：message-service 会做最终 DID proof 校验，但 Daemon 需要在本地早拒绝坏 package，避免本地状态、runtime agent 和用户体验进入不可用状态。
- 核心决策：bootstrap 分成两层校验：
  - `schema_validation`：已有字段、schema、sender、scope、forbidden private state；
  - `cryptographic_identity_validation`：解析 private key，derive public key，与 package public key 和远端 DID Document verification method 一致。
- 契约 / API / 数据流：Daemon 从 bootstrap package 得到 `user_did` 和 `verification_method`，解析当前 DID Document；确认 `authentication` 包含 method，method entry public key 与 package public key 一致，private key derive public key 与 package public key 一致。
- 兼容性：如果 Step 04 尚未切到 key package v2，本步骤先支持现有 v1；Step 04 再统一 schema。legacy v1 仍必须经过同样 crypto 校验。
- 迁移策略：已有已存储 identity 可在 Daemon 启动或下一次 bootstrap replay 时执行 lazy validation；发现无效则标记 `invalid` 或 `revoked`，停止 inbox sync。
- 风险控制：DID Document resolve 失败时不要创建 binding；区分 retryable network/resolve error 与 terminal key mismatch。日志只记录 DID、verification method、错误码，不记录 private/public full material。

## 4. 实现方法

1. 在 `awiki-deamon` bootstrap 模块引入 validation context：
   - DID resolver / im-core identity loader 接口；
   - current time provider；
   - key parser / public derivation helper。
2. 扩展 `validate_user_subkey_package`：
   - 解析 private key PEM/multibase；
   - derive Ed25519 public key；
   - 将 public key 编码为 Multikey/Multibase；
   - 与 package `public_key_multibase` 比对；
   - 验证 `expires_at` 未过期；
   - 验证 `verification_method == {user_did}#daemon-key-1`。
3. 增加 DID Document validation：
   - 加载/解析 user DID Document；
   - `authentication` 必须包含 `verification_method`；
   - `verificationMethod` 中该 method 的 `controller` 和 `publicKeyMultibase` 与 package 一致；
   - DID Document proof 校验如本地已有 verifier 能力则执行；否则至少不接受未绑定 public method。
4. 调整 `process_bootstrap_envelope`：
   - 在 `store_bootstrap_state` 前完成 crypto/DID validation；
   - validation 失败不写 user delegated identity、bootstrap replay、app personal agent binding；
   - audit 只记录 redacted error code。
5. 调整 delegated inbox shadow DID Document 生成：
   - 不只信任本地 package public key；在 sync 前可复查当前 DID Document authentication；
   - revoked 或不在 authentication 时停止 sync 并标记状态。
6. 增加测试：
   - private/public mismatch rejected；
   - method owner mismatch rejected；
   - DID Document authentication missing rejected；
   - DID Document public key mismatch rejected；
   - expired package rejected；
   - resolve retryable error 不创建 binding；
   - Debug/Display/audit 不泄露 secret。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/bootstrap.rs` | 增强 package 与 DID Document validation | 核心入口 |
| `awiki-cli-rs2/crates/awiki-deamon/src/app_bridge/secret_store.rs` | 补 key parsing / redaction helper 或迁移到新模块 | 保证 secret 不泄露 |
| `awiki-cli-rs2/crates/awiki-deamon/src/inbox/user_delegated.rs` | sync 前复查 current authentication 或处理 revoked state | 避免撤销后继续 poll |
| `awiki-cli-rs2/crates/awiki-deamon/src/state/*` | 必要时新增 identity validation status | 可 lazy migration |
| `awiki-cli-rs2/crates/awiki-deamon/tests/*` | bootstrap validation 集成测试 | 覆盖坏 package |

## 6. 依赖

- 前置步骤：Step 01 提供 registry/DID Document 撤销语义；Step 02 确保 APP package 能补齐。
- 外部文档或决策：bootstrap 仍走普通消息 JSON；Daemon 不处理 E2EE private state。
- 环境前提：能运行 `awiki-deamon` cargo tests。

## 7. 验收标准

- [x] bootstrap 保存状态前验证 private/public match。
- [x] bootstrap 保存状态前验证当前 DID Document `authentication` 包含 `user_did#daemon-key-1`。
- [x] package public key 与 DID Document method public key 不一致时拒绝。
- [x] package 过期、method owner 错、scope 含 E2EE/private state 时拒绝。
- [x] DID resolve retryable error 不创建 binding，并允许后续重试。
- [x] 被撤销 key 的 existing binding / inbox sync 不继续处理用户消息。
- [x] 所有错误、Debug、audit、test snapshot 不泄露 private key。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| awiki-deamon | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1` | 通过；lib 105 passed，`agent_registration_management` 21 passed，`generic_cli_runtime_mvp` 22 passed，`hermes_contracts` 5 passed，`hermes_gateway` 19 passed / 3 ignored，`hermes_message` 15 passed，`hermes_profile` 3 passed，`local_rpc_security` 23 passed，`state_bootstrap` 2 passed，doc-tests 0 passed。 |
| Targeted | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 bootstrap -- --nocapture` | 通过；21 passed，覆盖 private/public mismatch、DID auth missing/public mismatch、expired package、resolve failure 不落库、controller/user DID 绑定和 redaction。 |
| Inbox revoked | `cd awiki-cli-rs2 && cargo test -p awiki-deamon --locked -j1 user_delegated -- --nocapture` | 通过；9 passed，覆盖 delegated inbox cursor/replay、E2EE opaque 不 dispatch、system payload 不作为用户文本处理。 |
| Secret search | `rg -n "BEGIN PRIVATE|private_key_multibase|private_key_pem|z-private|secret" awiki-cli-rs2/crates/awiki-deamon/src awiki-cli-rs2/crates/awiki-deamon/tests` | 已复核；命中集中在 secret handling、redaction tests、fixtures 和既有状态测试；未发现新增日志/audit 泄露。 |
| Diff | `cd awiki-cli-rs2 && git diff --check` | 通过，无 whitespace 错误。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：bad package 是否会落库；resolver 失败是否创建坏 binding；secret redaction 是否完整；DID Document authentication 与 public key 是否都验证；runtime/Hermes 是否仍不能读取 private key；重放 bootstrap 是否幂等。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 已发现 1 项并修复 | bootstrap envelope 已校验 `controller_did == message sender`，但未显式要求 `user_subkey_package.user_did == controller_did`；已补校验和测试，避免合法 private package 被错绑到另一个 controller 的 bootstrap。 |
| 已修复问题 | 已修复 | 新增 private/public derive 校验、DID Document `id` / `authentication` / `verificationMethod.controller` / `publicKeyMultibase` 校验、`expires_at` RFC3339 和过期校验、`key_type` Ed25519 校验、DID resolve failure 不落库、`payload_hash` 不包含 private key material、local resolver 忽略 `delegated-inbox-*` shadow identity cache、delegated inbox sync 前复查当前 DID Document。 |
| 剩余风险 | 已记录 | Daemon 本地仍不验证 DID Document proof，只绑定当前解析到的 DID Document method/auth/public key；`did:wba` / `did:web` HTTP resolve fail closed 可能影响首次 bootstrap 可用性；bootstrap private package 仍按 MVP 决策通过普通消息明文 JSON 传输；旧 `private_key_multibase` 承载 PEM 的 schema 命名留给 Step 04 收口。 |
| 新增或缺失测试 | 已新增 | 新增 bootstrap validation、private/public mismatch、DID auth missing、DID public mismatch、expired package、resolve failure 不落库、secret hash 不含 private key、resolver 忽略 shadow identity、controller/user DID 绑定和 secret key parser 测试；未补 DID Document proof verifier 测试，原因是 Daemon 当前未实现本地 proof 验证能力。 |
| 已更新或缺失文档 | 已更新本计划 | 回填主 Plan 和本 Step 证据；核心设计文档的 schema v2 / capability 说明留给 Step 04，ANP SDK optional API 文档留给 Step 05。 |

## 10. Commit 要求

- Commit 时机：已完成；本步骤实现、验证、Review 后提交。
- Commit 范围：只包含 `awiki-deamon` bootstrap validation、secret helper、foreground fixture 和 delegated inbox sync 复查相关改动。
- Commit 前状态：`feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 37]`，仅本 Step 代码文件和本计划文档未提交。
- 纳入文件：`crates/awiki-deamon/src/app_bridge/bootstrap.rs`、`crates/awiki-deamon/src/app_bridge/message_control.rs`、`crates/awiki-deamon/src/app_bridge/secret_store.rs`、`crates/awiki-deamon/src/foreground.rs`、`crates/awiki-deamon/src/inbox/user_delegated.rs`。
- Commit 后证据：`1596856` (`awiki-deamon: validate delegated bootstrap keys`)；代码 commit 后只剩本计划文档待回填。
- 遗留未提交变更：本计划文档回填独立提交，不影响代码 Step commit。

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 无可用 DID resolver | 已解决 | 实现 `BootstrapDidDocumentResolver` trait、生产默认 resolver、测试 mock resolver；生产 resolver 对 `did:wba` / `did:web` 走 HTTP DID Document resolve，对非 HTTP DID method 使用本地 identity cache | 当前步骤 | 已完成；HTTP resolve fail closed 的可用性风险已记录 |
| private key 编码不明确 | 已缓解 | 将 PEM / base58btc multibase parse 和 Ed25519 public derive helper 移入 `secret_store`；继续兼容 legacy v1 字段 | 当前步骤 / Step 04 | Step 04 统一 key package schema v2 和字段命名 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 03 | 增强 Daemon bootstrap 早期安全校验 | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：DID resolve 依赖网络，可能让 `did:wba` / `did:web` bootstrap 首次 fail closed；Daemon 本地尚未验证 DID Document proof；bootstrap private package 仍走普通消息明文 JSON；schema v1 字段名仍存在历史命名问题。
- 回滚 / 回退：不得回退到创建坏 binding；如 resolver 不可用，保持 schema validation 与 private/public 校验，但 binding / personal agent 创建必须 fail closed，等待 bootstrap retry。
- 后续文档：Step 04 更新 daemon bootstrap key package schema v2、APP action capability 行为和核心设计文档；Step 05 更新 ANP SDK optional API 文档。
