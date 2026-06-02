# Step 05：im-core 下载校验与解密

主 Plan：[../plan.md](../plan.md)  
Step index：05  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` |
| Started | 2026-06-02 12:00 CST |
| Completed | 2026-06-02 12:03 CST |
| Commit | `e2ee-attachment-cli-rs2:e0670208d823c817261f17f5335eb8dbacca3b03` |
| Review evidence | Review 覆盖校验顺序、public redaction、ticket profile、sync/async local-file 失败不写出、secure-aware history projection、plain 兼容和 secret grep；未发现需修复问题。 |
| Verification evidence | `cargo fmt --all --check`、`cargo check -p im-core`、`cargo check -p im-core --features group-e2ee`、`cargo test -p im-core attachments_download_runtime --locked`、`cargo test -p im-core attachment --locked`、`cargo test -p im-core attachment_object_crypto --locked`、`cargo test -p im-core secure --locked`、`cargo test -p im-core e2ee --locked`、`cargo test -p im-core e2ee --features group-e2ee --locked`、`cargo test -p im-core decrypt --locked`、`cargo check -p im-core-dart`、`git diff --check` 均通过；`cargo test -p im-core attachment_download --locked` 过滤词 0 命中，已记录并用 `attachments_download_runtime` 覆盖。 |
| Next action | 启动 Step 06：CLI、Dart、data-rs2 高层接口 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`client.attachments().download()` 能下载 plain 和 E2EE 附件，E2EE 路径自动校验密文字节、解密并输出明文。
- 用户 / 系统可见行为：CLI/App 请求下载附件时拿到明文文件或 memory bytes；digest mismatch/decrypt failed 返回明确错误。
- 非目标：不新增低层 ticket API，不把密钥暴露给上层。
- 完成标准：download runtime 正负测试通过，文件写入为原始明文，public DTO 不含密钥。

## 3. 设计方法

- 设计边界：下载 runtime 负责 ticket、data plane、verify、decrypt、sink；上层只选择 message/thread/attachment/destination。
- 核心决策：无论 plain 或 E2EE，先校验下载字节长度和 sha-256；E2EE 再解密和校验 plaintext_size。
- 契约 / API / 数据流：selection 需要包含 `encryption_info`，但 public `DownloadedAttachment.selection` 必须 redacted。
- 兼容性：plain 下载保持原 UX，新增 digest 校验可能暴露历史坏对象，应作为正确行为。
- 迁移策略：不改服务端；若本地历史无法提供解密后的 manifest，应优先通过 secure history normalizer 获取。
- 风险控制：原子写文件，解密失败不写出明文；memory path 不返回密文。

## 4. 实现方法

1. 扩展 `crates/im-core/src/attachments/selection.rs`：
   - 解析 `encryption_info`、`message_security_profile`、`sender_did`、`message_id`。
   - internal selection 包含 key/nonce；public selection redacted。
2. 更新历史/消息读取路径：
   - direct/group E2EE history/inbox/listener 已有 decrypt normalizer 时，确保 attachment manifest 被投影为可选择内容。
   - 如果普通 history 返回 opaque secure body，download 应先调用 secure-aware read/normalize，而不是直接解析 opaque body。
3. 更新 `crates/im-core/src/internal/wire/attachment.rs` 的 ticket params：
   - `message_security_profile` 根据 selection 设置为 `transport-protected/direct-e2ee/group-e2ee`。
   - direct E2EE `message_target_did` 使用 requester DID；group 使用 `group_did`。
4. 更新 `crates/im-core/src/internal/attachment_runtime/download.rs`：
   - data plane GET 后读密文字节。
   - 校验 `len(downloaded) == size`。
   - 校验 sha-256 digest。
   - `mode=none` 直接写出。
   - `mode=object-e2ee` 解密、校验 `plaintext_size`，写出明文。
5. 错误处理：
   - digest mismatch 映射 P7 风格错误。
   - decrypt failed 不输出 key/nonce。
   - ticket grant not found 给出可诊断 warning/error。
6. 补 tests：
   - plain download 仍可用。
   - E2EE memory download returns plaintext。
   - E2EE local file download writes plaintext and atomic behavior。
   - wrong digest、wrong key、wrong plaintext_size fail。
   - public DTO/selection redacted。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/attachment_runtime/download.rs` | verify/decrypt/sink | 核心实现 |
| `e2ee-attachment-cli-rs2/crates/im-core/src/attachments/selection.rs` | 解析 encryption_info 和 redaction | 区分 internal/public |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/wire/attachment.rs` | ticket params 支持 E2EE profile |  |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/realtime/attachment_projection.rs` | 如需 secure attachment projection 则更新 |  |
| `e2ee-attachment-cli-rs2/crates/im-core/tests/attachment_api.rs` | 下载正负测试 |  |

## 6. 依赖

- 前置步骤：Step 02、Step 03。
- 外部文档或决策：P7 receiver verification and decryption steps。
- 环境前提：secure direct/group decrypt normalizer 可供附件消息读取。

## 7. 验收标准

- [x] plain 下载校验长度和 digest 后输出原字节。
- [x] E2EE 下载校验密文字节后输出明文。
- [x] digest mismatch / decrypt failed / plaintext_size mismatch 不写出有效附件。
- [x] ticket request 使用正确 message security profile。
- [x] public result 不含 key/nonce。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd e2ee-attachment-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Download tests | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_download --locked` | plain/E2EE 正负测试通过。 |
| Crypto regression | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_object_crypto --locked` | 加解密回归通过。 |
| Secure read regression | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core secure --locked` | secure read/decrypt 回归通过。 |
| Diff | `cd e2ee-attachment-cli-rs2 && git diff --check` | 无 whitespace 错误。 |

实际验证记录：

- `cargo fmt --all --check`：通过。
- `cargo check -p im-core`：通过。
- `cargo check -p im-core --features group-e2ee`：通过。
- `cargo test -p im-core attachments_download_runtime --locked`：13 passed, 0 failed, 235 filtered。
- `cargo test -p im-core attachment --locked`：lib 29 passed、attachment_api 22 passed、phase1a 1 passed、realtime_projection 7 passed，0 failed。
- `cargo test -p im-core attachment_object_crypto --locked`：2 passed, 0 failed。
- `cargo test -p im-core secure --locked`：lib 66 passed、phase1a 2 passed、realtime_loop 2 passed、secure_api 10 passed，0 failed。
- `cargo test -p im-core e2ee --locked`：lib 24 passed、attachment_api 3 passed、phase1a 3 passed，0 failed。
- `cargo test -p im-core e2ee --features group-e2ee --locked`：lib 79 passed、attachment_api 3 passed、phase1a 3 passed，0 failed。
- `cargo test -p im-core decrypt --locked`：4 passed, 0 failed。
- `cargo test -p im-core attachment_download --locked`：0 passed, 0 failed；过滤词未命中实际下载测试，保留为 Plan 原命令证据，实际下载覆盖使用 `attachments_download_runtime`。
- `cargo check -p im-core-dart`：通过。
- `git diff --check`：通过。
- Secret grep：`object_key_b64u|nonce_b64u` 命中仅在 internal crypto/manifest/selection/download 解密逻辑和测试断言；Dart/public DTO 未新增密钥输出。

## 9. Review 环节

Review 重点：

- 校验顺序是否为长度/digest -> decrypt -> plaintext_size。
- 解密失败或 atomic write 失败是否留下半成品文件。
- public `DownloadedAttachment` 和 `AttachmentSelection` 是否 redacted。
- E2EE ticket request 的 target service 是否仍按原消息 sender DID 解析。

实际 Review 记录：

- 校验顺序为 `verify_downloaded_ciphertext` 先校验下载字节长度和 sha-256，`object-e2ee` 再调用 `decrypt_object_e2ee`，最后校验 `plaintext_size`。
- sync 和 async local-file 路径均在校验/解密成功后才调用 atomic write；digest mismatch、wrong key、plaintext_size mismatch 测试确认不写目标文件且无临时文件残留。
- public `AttachmentSelection` 只保留 `message_security_profile`、`object_encryption_mode`、`object_cipher`、`plaintext_size` 等非秘密字段；key/nonce 只在 `InternalAttachmentSelection` 和 full manifest 测试内存在。
- ticket request direct/group 分别使用 `direct-e2ee` / `group-e2ee`，并继续按原消息 `sender_did` 解析 attachment service。
- 未发现需修复问题；剩余风险是 realtime projection 对 object-e2ee manifest 仍保留 warning UX，下载 runtime 已可处理，后续 Step 06/07 按 CLI/系统测试收口。

## 10. Commit 要求

- Commit 前记录 `e2ee-attachment-cli-rs2` 的 `git status --short --branch`。
- Commit 只包含 download/decrypt、selection、wire params 和 tests。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

实际 Commit 记录：

- Pre-commit status：`crates/im-core/src/attachments/selection.rs`、`crates/im-core/src/internal/attachment_runtime/download.rs`、`crates/im-core/src/internal/message_runtime/read.rs`、`crates/im-core/src/internal/wire/attachment.rs`、`crates/im-core/tests/attachment_api.rs` modified。
- Implementation commit：`e2ee-attachment-cli-rs2:e0670208d823c817261f17f5335eb8dbacca3b03`，commit message `支持 im-core E2EE 附件下载解密`。
- Post-commit status：实现提交后工作区干净，随后仅回填本 Plan / Step 文档。

## 11. 风险、假设与回滚

- 风险：本地普通历史没有完整 E2EE inner manifest。缓解：接入 secure-aware read；无法实现时记录受限并补 repair/fetch path。
- 风险：新增 digest 校验影响坏历史对象。缓解：这是 P7 必须行为，错误信息清晰。
- 回滚：保留 plain download，E2EE download 返回 unsupported。
