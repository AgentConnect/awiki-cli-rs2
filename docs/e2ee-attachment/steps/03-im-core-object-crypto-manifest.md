# Step 03：im-core 对象加解密与 manifest 模型

主 Plan：[../plan.md](../plan.md)  
Step index：03  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` |
| Started | 2026-06-02 09:51 CST |
| Completed | 2026-06-02 10:38 CST |
| Commit | `e2ee-attachment-cli-rs2:91a9da2` |
| Review evidence | Review 覆盖 AEAD、manifest full/redacted、控制面禁密钥、public DTO 脱敏、plain 兼容和下游 Rust DTO 编译影响；发现并修复公开 prepared/descriptor 曾持有 key/nonce 的泄漏风险，改为 internal-only secrets；移除 compat 解密 helper。 |
| Verification evidence | `cargo fmt --all --check` 通过；`cargo test -p im-core attachment_object_crypto --locked` 2 passed；`cargo test -p im-core attachment_manifest --locked` lib 2 passed、attachment_api 2 passed；`cargo test -p im-core attachment_api --locked` 1 passed；`cargo check -p im-core` 通过；`cargo check -p im-core-dart` 通过；`git diff --check` 通过；secret grep 已复核。 |
| Next action | Step 04：接入 secure attachment send |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：`im-core` 能生成和解析 P7 `object-e2ee` 附件对象、manifest、grant refs 和 redacted public DTO。
- 用户 / 系统可见行为：SDK 内部可把明文附件变成密文上传对象；public result 不泄漏对象 key/nonce。
- 非目标：不接 secure send wire，不改 CLI/Dart。
- 完成标准：对象加解密、digest/size、manifest 序列化、redaction 和 negative tests 通过。

## 3. 设计方法

- 设计边界：对象 crypto 在 `im-core` internal attachment runtime；CLI/Dart 不直接操作 key/nonce。
- 核心决策：internal manifest 可以含 key/nonce，public DTO/redacted manifest 不含 key/nonce。
- 契约 / API / 数据流：`PreparedAttachment` 拆分为 plaintext metadata 和 upload object metadata；`size/digest` 指向上传对象字节。
- 兼容性：plain attachment path 继续生成 `mode=none` manifest。
- 迁移策略：不改本地 DB schema，除非测试证明投影需要 metadata 字段。
- 风险控制：测试证明 E2EE manifest redaction 不泄密，decrypt 前先 verify。

## 4. 实现方法

1. 在 `crates/im-core/Cargo.toml` 增加合适 AEAD 依赖，优先选择仓库已有或 RustCrypto `chacha20poly1305`；若 ANP SDK 已暴露对象 AEAD helper，优先复用 ANP SDK。
2. 新增 `crates/im-core/src/internal/attachment_runtime/object_crypto.rs`：
   - 生成 32 字节 key、12 字节 nonce；
   - encrypt/decrypt `ChaCha20-Poly1305`，AAD 为空；
   - base64url encode/decode；
   - 校验 key/nonce 长度。
3. 扩展 `crates/im-core/src/attachments/manifest.rs`：
   - `EncryptionInfo` 结构；
   - `AttachmentDescriptor` 包含 `object_cipher`、`object_key_b64u`、`nonce_b64u`、`plaintext_size`；
   - `build_attachment_manifest_internal()` 支持完整 E2EE manifest；
   - `redact_attachment_manifest()` 移除 key/nonce。
4. 扩展 upload prepare 结构：
   - `PreparedPlainAttachment`：filename/mime/plaintext_size/plaintext bytes 或 source path；
   - `PreparedUploadObject`：upload bytes/size/digest/object mode/encryption info。
5. 生成 `AttachmentGrantRef` internal DTO，只含非秘密字段。
6. 修改 `UploadedAttachment` public DTO 增加 `object_encryption_mode`、`plaintext_size_bytes`，不增加 key/nonce。
7. 补单元测试：
   - encrypt/decrypt roundtrip；
   - wrong digest / wrong key / wrong nonce 失败；
   - same plaintext 多次加密 key/nonce/digest 不固定；
   - manifest full vs redacted；
   - plain manifest 兼容。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `e2ee-attachment-cli-rs2/crates/im-core/Cargo.toml` | 加 AEAD 依赖或启用 ANP helper | 优先复用现有 SDK |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/attachment_runtime/object_crypto.rs` | 新增对象加解密 | internal only |
| `e2ee-attachment-cli-rs2/crates/im-core/src/internal/attachment_runtime/mod.rs` | 注册模块 |  |
| `e2ee-attachment-cli-rs2/crates/im-core/src/attachments/manifest.rs` | 完整 P7 manifest 和 redaction | 防泄漏 |
| `e2ee-attachment-cli-rs2/crates/im-core/src/attachments/dto.rs` | public DTO 扩展 | 无 key/nonce |
| `e2ee-attachment-cli-rs2/crates/im-core/tests/attachment_api.rs` | 增加 public API shape tests |  |

## 6. 依赖

- 前置步骤：Step 01 推荐先完成；本步骤可在不连服务端的情况下先实现。
- 外部文档或决策：ANP-P7 object-e2ee MTI。
- 环境前提：`e2ee-attachment-cli-rs2` Rust workspace 可测试。

## 7. 验收标准

- [x] 对象加密使用随机 32 字节 key 和 12 字节 nonce。
- [x] `size`/`digest` 指向密文字节，`plaintext_size` 指向明文字节。
- [x] full E2EE manifest 只在 internal 路径使用。
- [x] public result / redacted manifest / CLI-ready DTO 不包含 key/nonce。
- [x] plain attachment manifest 不回归。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd e2ee-attachment-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| Crypto tests | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_object_crypto --locked` | roundtrip 和 negative tests 通过。 |
| Manifest tests | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_manifest --locked` | full/redacted/plain tests 通过。 |
| Public API tests | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment_api --locked` | DTO shape 通过。 |
| Secret grep | `cd e2ee-attachment-cli-rs2 && rg -n "object_key_b64u|nonce_b64u" crates/im-core/src crates/im-core/tests` | 只允许 internal manifest/crypto/tests 命中。 |

## 9. Review 环节

Review 重点：

- AEAD 使用是否符合 P7：ChaCha20-Poly1305、AAD 为空、key/nonce 长度固定。
- 是否有 key/nonce 进入 public DTO、日志、错误消息。
- redacted manifest 是否默认用于 public result。
- 内存/临时文件策略是否对大文件有明确限制或 follow-up。

## 10. Commit 要求

- Commit 前记录 `e2ee-attachment-cli-rs2` 的 `git status --short --branch`。
- Commit 只包含 im-core object crypto、manifest、DTO 和测试。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

## 11. 风险、假设与回滚

- 风险：新增依赖影响 release graph。缓解：优先复用 ANP SDK helper；如新增依赖，锁定并测试 workspace。
- 风险：public manifest 泄密。缓解：redaction tests 和 Review。
- 回滚：保留 plain manifest，移除 object crypto 模块。
