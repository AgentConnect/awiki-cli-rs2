# Step 04：im-core secure attachment send

主 Plan：[../plan.md](../plan.md)  
Step index：04  
状态：draft

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | pending |
| Branch | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` |
| Started |  |
| Completed |  |
| Commit |  |
| Review evidence |  |
| Verification evidence |  |
| Next action | 等 Step 02、03 完成后启动 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`client.messages().send(MessageBody::Attachment, MessageSecurityMode::E2eeRequired)` 可发送 direct/group E2EE 附件。
- 用户 / 系统可见行为：SDK 自动加密对象、上传、commit、构造 E2EE inner manifest、发送 secure message，并附带非秘密 grant refs。
- 非目标：不改下载，不改 CLI/Dart public UX。
- 完成标准：direct secure attachment send 和 group secure attachment send 的 unit/contract tests 通过；plain attachment 继续工作。

## 3. 设计方法

- 设计边界：高层 message service 是 canonical 入口；`attachments().send()` 只是便捷封装。
- 核心决策：不新增低层 public API；扩展 internal secure send 从 text-only 到 application plaintext。
- 契约 / API / 数据流：upload object 完成后，secure sender 使用 full internal manifest 加密；RPC params 的 `client.attachment_grant_refs` 只带 redacted/non-secret refs。
- 兼容性：`MessageSecurityMode::Plain` 或 default 仍走 base attachment；`E2eeRequired` direct/group 根据目标选择 direct/group E2EE。
- 迁移策略：本地 projection 要能存附件消息 view；如 existing projection 只支持 text，补 attachment projection。
- 风险控制：测试确认 secure RPC body 不泄漏 object key 到 outer body/client refs。

## 4. 实现方法

1. 修改 `crates/im-core/src/messages/service.rs`：
   - `validate_attachment_security` 不再对 `E2eeRequired` 返回 unsupported。
   - direct/group E2EE 分支接受 `MessageBody::Attachment`。
   - plain attachment 分支可转调 existing `AttachmentUploadRuntime` 或保留现状。
2. 扩展 `AttachmentUploadRuntime`：
   - 抽出 `prepare_and_commit_object(security) -> PreparedCommittedAttachment`。
   - 对 E2EE security 使用 Step 03 object crypto。
   - 只负责 slot/put/commit，不直接发送 base manifest。
3. 扩展 direct secure sender：
   - 新增 internal input 支持 `ApplicationPlaintextPayload`。
   - `application_content_type = application/anp-attachment-manifest+json`。
   - `payload = full internal AttachmentMessage`。
   - `client.attachment_grant_refs` 放入 authenticated RPC params 的 `client` context。
4. 扩展 group E2EE sender：
   - 使用 `GroupApplicationPlaintext { application_content_type, payload }`。
   - `group.e2ee.send` params 同样带 `client.attachment_grant_refs`。
5. 更新 `AttachmentService::send()`：
   - 增加 `security` 字段后，对 E2EE 转调 `messages().send()`。
   - 保持原默认 plain 行为。
6. 本地 projection：
   - direct/group secure outgoing projection 保存附件 manifest 的 redacted/local plaintext view。
   - 避免把 key/nonce 存入普通 business SQLite message content；如需要下载密钥，需通过 secure message decrypt 路径重新获得，或 internal secure projection 单独加密保存。首期推荐不持久化 key/nonce 到普通 DB。
7. 补 tests：
   - direct E2EE attachment send RPC shape。
   - group E2EE attachment send RPC shape。
   - `client.attachment_grant_refs` 没有 key/nonce。
   - missing secure state 仍返回 secure error，不降级。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `e2ee-attachment-cli-rs2/crates/im-core/src/messages/service.rs` | 高层 Attachment + E2eeRequired 路由 | canonical API |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/attachment_runtime/upload.rs` | 抽出 upload/commit，不直接发送 | 复用 plain |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/secure_direct/send.rs` | 支持 attachment Application Plaintext | 不泄漏 outer body |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/secure_direct/async_send.rs` | async direct secure attachment | 如有对应路径 |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/group_e2ee/runtime.rs` | 支持 attachment GroupApplicationPlaintext | feature-gated |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/wire/attachment.rs` | create/commit params 支持 object-e2ee | 非秘密 refs |
| `e2ee-attachment-cli-rs2/crates/im-core/src/attachments/service.rs` | 便捷封装转调 message service | 默认 plain |
| `e2ee-attachment-cli-rs2/crates/im-core/tests/secure_api.rs` | 增加 API shape |  |

## 6. 依赖

- 前置步骤：Step 02、Step 03。
- 外部文档或决策：direct/group E2EE public discovery 仍关闭。
- 环境前提：`im-core` secure direct/group E2EE tests 可运行；group E2EE feature 如需显式启用。

## 7. 验收标准

- [ ] `MessageBody::Attachment + E2eeRequired` direct target 走 direct E2EE。
- [ ] `MessageBody::Attachment + E2eeRequired` group target 走 group E2EE。
- [ ] outer E2EE body 只含 opaque cipher，不含 manifest 明文或 key/nonce。
- [ ] `client.attachment_grant_refs` 只含非秘密 refs。
- [ ] plain attachment send 不回归。
- [ ] Review 发现已经修复或明确记录。
- [ ] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd e2ee-attachment-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Secure attachment send | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core secure_attachment_send --locked` | direct/group shape tests 通过。 |
| Secure regression | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core secure --locked` | secure direct 回归通过。 |
| E2EE regression | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core e2ee --locked` | e2ee 相关回归通过。 |
| Attachment regression | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment --locked` | plain attachment 回归通过。 |
| Secret grep | `cd e2ee-attachment-cli-rs2 && rg -n "object_key_b64u|nonce_b64u" crates/im-core/src/internal/secure_direct crates/im-core/src/internal/group_e2ee crates/im-core/tests` | 只允许 internal plaintext construction 和 tests 命中，不在 outer params/client refs。 |

## 9. Review 环节

Review 重点：

- 高层接口是否避免新增不必要 public API。
- secure send 是否发生自动 plain 降级。
- full manifest 是否只进入 E2EE inner plaintext。
- grant refs 是否不含 key/nonce 且只进入 `client` context。
- local projection 是否避免把 key/nonce 写入普通 message store。

## 10. Commit 要求

- Commit 前记录 `e2ee-attachment-cli-rs2` 的 `git status --short --branch`。
- Commit 只包含 im-core secure attachment send、tests 和必要 docs。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

## 11. 风险、假设与回滚

- 风险：direct/group secure sender text-only 假设较深。缓解：抽象 application plaintext，保留 text tests。
- 风险：本地下载需要 key/nonce 但 projection redacted。缓解：Step 05 从已解密历史/notification 中恢复 manifest；如不可行，先记录受限能力再设计 encrypted local secure attachment key store。
- 回滚：恢复 Attachment + E2eeRequired unsupported，保留 object crypto。
