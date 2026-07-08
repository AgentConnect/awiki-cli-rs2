# Step 02：ANP SDK / im-core optional params

主 Plan：[../plan.md](../plan.md)  
Step index：02  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` |
| Started | 2026-06-09T10:39:30Z |
| Completed | 2026-06-09T12:28:11Z |
| Commit | `f0a5389 im-core: add delegated inbox signing options` |
| Review evidence | 2026-06-09 Review：检查 optional 参数兼容、ANP proof keyid / target DID、owner/key 本地校验、Dart binding 同步、delegated E2EE 边界、错误命名残留和两篇设计文档边界。发现并修复 4 项：`history.rs` delegated auth 被 move 后又借用的编译风险；delegated inbox/history local proof target 原先缺少配置化 service DID；delegated group history 会静默忽略 `InboxHistoryOptions` 并进入 group/E2EE projection 路径；Plan/设计文档中 user-service public method 管理、daemon key fragment 固定值和私钥所有权边界残留。 |
| Verification evidence | `cd awiki-cli-rs2 && cargo test -p im-core --locked`：267 lib tests passed，所有 integration/doc tests passed；`cd awiki-cli-rs2 && cargo test -p im-core-dart --locked`：6 unit + 13 facade + 0 doc tests passed；`cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh`：Done；`cd awiki-cli-rs2/packages/awiki_im_core && flutter test`：12 tests passed；`cd awiki-cli-rs2 && git diff --check`：通过；Step 02 naming check 和设计残留检查：无命中。 |
| Next action | 创建 Step 02 聚焦 commit；随后从 Step 03 继续 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：ANP SDK / `im-core` 支持 delegated signing 与 delegated inbox/history optional 参数，老调用不传参数时行为不变。
- 用户 / 系统可见行为：Daemon 可以指定 `meta.sender_did = user_did`，用 `user_did#daemon-key-1` 签名发送普通消息；也可以用同一子 key 证明 `inbox_owner_did` 的普通 inbox/history 读取权限。
- 非目标：不修改 ANP `origin_proof` 核心结构；不实现 ANP delegated proof；不让 SDK 返回或处理 E2EE 明文 projection。
- 完成标准：Rust DTO/API、proof builder、wire client 和 Dart binding 支持 optional 参数；旧测试通过；新增 delegated send/inbox/history 测试通过。

## 3. 设计方法

- 设计边界：SDK 只表达调用者要用哪个 logical sender / inbox owner 和哪个 verification method 签名；服务端仍做最终 DID Document 与 policy 判定。
- 核心决策：新增 inbox 命名固定为 `InboxHistoryOptions`、`inbox_owner_did`、`inbox_auth_verification_method`、`inbox_auth_key_ref`、`inbox_auth`、`ScopedInboxToken`。
- 契约 / API / 数据流：send 增加 `logical_sender_did`、`signing_verification_method`、`signing_key_ref`、`actor_agent_did` optional 参数；inbox/history 增加 `InboxHistoryOptions`，支持 DID proof 或后续 token。
- 兼容性：所有新增字段都是 optional；老调用继续使用当前 identity/session 默认 key。
- 迁移策略：Dart binding 生成代码同步更新；老 Dart API 可保留重载或默认参数。
- 风险控制：SDK 本地尽早拒绝 verification method 不属于 owner DID、本地无 key、scope 不允许、请求 E2EE inbox projection 等错误；本步骤必须增加显式测试矩阵，证明老调用不变和 E2EE projection 被拒绝。

## 4. 实现方法

1. 阅读 `awiki-cli-rs2/crates/im-core/src/messages/dto.rs`、`service.rs` 和当前 send/history/inbox 调用链。
2. 在 Rust DTO 中新增 delegated send options 与 `InboxHistoryOptions`；保持 serde 默认兼容。
3. 修改 `awiki-cli-rs2/crates/im-core/src/internal/proof/origin.rs`，允许 proof 使用指定 `signing_verification_method` 与 `signing_key_ref`，同时保持 `meta.sender_did` 为 `logical_sender_did`。
4. 修改 `awiki-cli-rs2/crates/im-core/src/internal/wire/inbox.rs` 和 `history.rs`，支持 `inbox_owner_did` 和 `inbox_auth_verification_method` 生成 DID proof；预留 `ScopedInboxToken` 类型但 MVP 可先走 DID proof。
5. 增加本地校验：owner DID 与 verification method DID 部分一致；E2EE projection/private state 请求直接拒绝。
6. 更新 `awiki-cli-rs2/docs/api/im-core-interface/*`，说明 optional 参数和老调用兼容。
7. 如 Rust bridge DTO 变更，运行或更新 codegen，补充 Dart API 和 tests。

### 4.1 必须覆盖的 SDK 测试矩阵

| 场景 | 输入 | 预期 |
|---|---|---|
| 旧 send 调用 | 不传 delegated send options | 使用当前 identity/session 默认 sender 与 key，行为与改动前一致。 |
| 旧 inbox/history 调用 | 不传 `InboxHistoryOptions` | 使用当前 owner/session 默认 inbox 读取逻辑，行为与改动前一致。 |
| delegated send | `logical_sender_did=user_did`、`signing_verification_method=user_did#daemon-key-1`、`signing_key_ref` 指向本地 APP/Daemon key | proof 中 `meta.sender_did=user_did` 且 `keyid=user_did#daemon-key-1`。 |
| wrong owner key | `logical_sender_did` 或 `inbox_owner_did` 与 verification method DID 不一致 | SDK 本地拒绝，不发请求。 |
| missing key | `signing_key_ref` / `inbox_auth_key_ref` 本地不存在 | SDK 本地拒绝，不发请求。 |
| revoked / disallowed scope mock | 本地 policy mock 标记 scope 不允许 | SDK 本地拒绝；服务端仍是最终判定。 |
| E2EE projection request | delegated inbox/history 请求 plaintext、metadata projection、private state 或 E2EE private scope | SDK 本地拒绝，不发请求。 |
| token 预留路径 | `inbox_auth=ScopedInboxToken` 但 MVP 未启用 token | 不影响 DID proof 主路径；未启用时返回明确 unsupported 或走后续 feature flag。 |

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/im-core/src/messages/dto.rs` | 新增 delegated send options 与 `InboxHistoryOptions` | 字段 optional |
| `awiki-cli-rs2/crates/im-core/src/messages/service.rs` | 透传 optional 参数 | 老调用不变 |
| `awiki-cli-rs2/crates/im-core/src/internal/proof/origin.rs` | proof builder 支持指定 keyid/key ref | 不改 ANP proof 结构 |
| `awiki-cli-rs2/crates/im-core/src/internal/wire/inbox.rs` | delegated inbox auth 支持 | 使用 `inbox_*` 命名 |
| `awiki-cli-rs2/crates/im-core/src/internal/wire/history.rs` | delegated history auth 支持 | 若 history 独立实现 |
| `awiki-cli-rs2/packages/awiki_im_core/lib/src/generated/*` | bridge 生成更新 | 仅在 DTO/API 变更时 |
| `awiki-cli-rs2/docs/api/im-core-interface/*` | 接口文档更新 | 明确 optional 与 E2EE 拒绝 |

## 6. 依赖

- 前置步骤：Step 01 至少确认 daemon key DID URL、key package 和 registry 契约；实现可先以接口草案为准。
- 外部文档或决策：`awiki-cli-rs2/docs/agent-im/agent_im_core_design.md` 第 5.5 节；`awiki-cli-rs2/docs/agent-im/agent_delegated_identity_message_proof_plan.md` 第 3.3、5.8、5.9 节。
- 环境前提：Rust toolchain 和 Flutter bridge/codegen 环境可用。

## 7. 验收标准

- [x] send optional 参数能生成 `meta.sender_did = user_did` 且 `keyid = user_did#daemon-key-1` 的 proof。
- [x] inbox/history optional 参数能以 `inbox_owner_did` 和 `inbox_auth_verification_method` 发起普通消息读取请求。
- [x] `ScopedInboxToken` 类型或预留路径不影响 MVP DID proof 主路径。
- [x] 老 send/inbox/history 调用不传参数时行为与改动前一致。
- [x] SDK 本地拒绝错误 owner/key 组合和 E2EE projection/private state 请求。
- [x] 上述 SDK 测试矩阵全部有 Rust 测试；Dart API 变更时有对应 Dart 测试或 fixture。
- [x] Dart binding 如有变化已经同步。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Rust unit | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | delegated proof、optional 参数、旧调用回归测试通过。 |
| Bridge/codegen | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh` | 如果 binding 变更，生成代码一致。 |
| Dart package | `cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | Dart API 可用；旧调用测试通过。 |
| Naming | `PATTERN="$(printf '%s|%s|%s|%s|%s|%s' 'message_''owner|message_''auth' 'Message''Access' 'Scoped''Message' 'mailbox_''owner' 'Scoped''Mailbox' 'Scoped''MailboxToken')" && rg -n "$PATTERN" awiki-cli-rs2/crates/im-core awiki-cli-rs2/packages/awiki_im_core awiki-cli-rs2/docs/api/im-core-interface` | 不出现错误新增命名；若历史无关残留需说明。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：optional 参数兼容性、ANP proof 语义、owner/key 校验、Dart binding 同步、E2EE projection 拒绝、错误命名残留。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 4 项 | `history.rs` move/borrow 编译风险；delegated inbox/history proof target 需要使用配置化 message-service DID；delegated group history 不应静默忽略 `InboxHistoryOptions`；Plan/设计文档有 registry/device/default 旧措辞残留。 |
| 已修复问题 | 已修复 | `history.rs` 先保存 `service_did` 再消费 auth；wire auth 增加 `service_did` 并从 `anp_service_did` 或 `did:wba:<did_domain>` 生成；group history 携带 delegated options 时本地返回 `delegated-group-history` unsupported；文档残留已清理。 |
| 剩余风险 | 已记录 | `ScopedInboxToken` 仍是 MVP 后路径，当前传入会明确 unsupported；最终撤销实时性仍依赖 message-service DID Document cache 刷新；Step 02 只实现 SDK/bridge optional 参数，服务端策略在 Step 07 落地。 |
| 新增或缺失测试 | 已补充 | Rust 覆盖 delegated send、wrong owner、missing key、delegated inbox/history proof、E2EE filter、ScopedInboxToken unsupported、delegated group history reject；Dart bridge/public package 覆盖 optional 参数构造和映射。 |
| 已更新或缺失文档 | 已更新 | 更新 `awiki-cli-rs2/docs/api/im-core-interface/04-message-interface.md`、两篇 Agent IM 设计文档、主 Plan 和 Step 文档；未发现缺失的 Step 02 文档项。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：只包含 `im-core`、Dart binding 和直接相关文档/测试。
- Commit 前状态：`## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 3]`，包含 Step 02 相关 `im-core`、`im-core-dart`、`packages/awiki_im_core`、API 文档、Agent IM 设计/Plan 文档修改。
- 纳入文件：`crates/im-core/**` delegated signing/inbox source/tests；`crates/im-core-dart/**` bridge DTO/API/mapping/tests/generated；`packages/awiki_im_core/**` generated/public Dart API/tests；`docs/api/im-core-interface/04-message-interface.md`；`docs/agent-im/agent_im_core_design.md`；`docs/agent-im/agent_delegated_identity_message_proof_plan.md`；`docs/agent-im/plan/plan.md`；`docs/agent-im/plan/steps/01-user-service-did-delegated-subkey.md`；`docs/agent-im/plan/steps/02-im-core-delegated-signing-inbox-options.md`。
- Commit 后证据：`f0a5389 im-core: add delegated inbox signing options`；实现 commit 后状态为 `## feature/release-0526/agent-im-hutong...origin/feature/release-0526/agent-im-hutong [ahead 4]`，无未提交文件。该 hash 由后续台账回填提交记录，避免同仓库提交自引用导致 hash 不稳定。
- 遗留未提交变更：无。
- 建议消息：`im-core: add delegated inbox signing options`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| bridge/codegen 脚本不可用 | 待填写 | 记录脚本错误，手动检查生成差异 | 当前步骤 / Step 06 | 不跳过 API 一致性；需要替代验证 |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-09 | 创建 Step 02 小 Plan | 初始计划拆分 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |
| 2026-06-09 | Review 后补充 service DID target、delegated group history 拒绝和文档残留清理 | 实现 Review 发现 proof target 不能硬编码；delegated options 不应进入 group/E2EE projection；Plan/设计文档需与最新私钥所有权和 message-service 授权边界一致 | [../plan.md#15-plan-变更记录](../plan.md#15-plan-变更记录) |

## 13. 风险、回滚与后续文档

- 风险：SDK API 命名漂移导致后续 APP/Daemon 不一致。
- 回滚 / 回退：保留老调用路径，回滚 optional 参数接入；服务端 delegated 功能可独立关闭。
- 后续文档：更新 `awiki-cli-rs2/docs/api/im-core-interface/*`，在主 Plan 台账记录实际 DTO 名称。
