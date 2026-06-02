# 附件端到端加密传输方案

状态：draft  
创建时间：2026-06-02 08:00 CST  
关联 Plan：[plan.md](plan.md)

## 1. 目标

本方案为 AWiki 的附件端到端加密传输提供跨仓库设计，覆盖协议承载、服务端对象控制面、客户端对象加解密、上层 CLI、Dart / data-rs2 接口和系统测试。

预期结果：

- direct E2EE 和 group E2EE 消息可以携带附件 manifest。
- 附件对象在上传前由发送端本地加密，服务端只保存密文对象和非秘密元数据。
- 接收端通过原消息发送方 DID 解析附件控制服务，获取短时 Download Ticket，下载密文，校验摘要后本地解密。
- 服务端能够在不解密 P5/P6 消息的前提下，为合法接收者建立 Access Grant。
- CLI、Dart / data-rs2 和 App 侧只使用高层 SDK 接口，不拼装 P7/P5/P6 wire payload，不暴露对象密钥。

非目标：

- 不实现服务端解密。
- 不把 `object_key_b64u`、`nonce_b64u`、P5 ratchet key、MLS secret 或对象明文写入 message-service。
- 不新增独立附件密钥协商协议。
- 不改变 public discovery 默认关闭 `anp.direct.e2ee.v1`、`direct-e2ee`、`anp.group.e2ee.v1`、`group-e2ee` 的策略。
- 不承诺 group 成员移除后的追溯撤回；已拿到对象密钥或对象内容的成员不能被协议层强制擦除。

## 2. 上下文与当前状态

### 2.1 协议基线

ANP-P7 `anp/AgentNetworkProtocol/message/07-attachments-and-object-transfer.md` 固定了三层模型：

- Message plane：`direct.send` / `group.send` 携带 `application/anp-attachment-manifest+json`。
- Control plane：`attachment.create_slot`、`attachment.commit_object`、`attachment.abort_object`、`attachment.get_download_ticket`。
- Data plane：独立 HTTPS `PUT upload_uri` 和 `GET object_uri`，下载时使用 `Authorization: Bearer {download_ticket}`。

P7 的 E2EE 附件规则：

- `encryption_info.mode` 只有 `none` 和 `object-e2ee`。
- `object-e2ee` 的 MTI 是 `chacha20-poly1305`、32 字节对象密钥、12 字节 nonce。
- `object_key_b64u` 和 `nonce_b64u` 只能出现在 direct/group E2EE 内层 manifest。
- 控制面 `create_slot` / `commit_object` 不得传输对象密钥或 nonce。
- `attachment_manifest.size` 和 `digest` 对应上传对象字节；在 `object-e2ee` 下即密文字节。

P5/P6 的关键承载：

- Direct E2EE：外层 `meta.content_type` 是 `application/anp-direct-init+json` 或 `application/anp-direct-cipher+json`，内层 `Application Plaintext.application_content_type` 是 `application/anp-attachment-manifest+json`。
- Group E2EE：外层是 `application/anp-group-cipher+json`，内层 `Group Application Plaintext.application_content_type` 是 `application/anp-attachment-manifest+json`。

### 2.2 message-service 当前状态

`message-service` 已有 P7 基础控制面：

- `crates/im-attachment/src/handlers.rs` 实现 slot、commit、abort、ticket、对象 HTTP。
- `crates/im-attachment/src/manifest.rs` 实现 base manifest 校验和 `attachment_access_grants` 生成。
- `crates/im-storage/migrations/202604090001_p7_v050_attachment_alignment.sql` 已预留 `object-e2ee`、`direct-e2ee`、`group-e2ee` 枚举。
- `docs/api/ANP-client-server-api-attachment.md` 当前明确写着尚未实现 `object-e2ee` 和 direct/group E2EE attachment authorization flow。

主要缺口：

- `create_slot` / `commit_object` 仍通过 base-only 策略拒绝 `object-e2ee`。
- base manifest 校验要求 `encryption_info.mode = none`。
- direct/group E2EE 消息服务端只看到 opaque cipher，无法直接解析内层 attachment manifest。
- E2EE 附件没有 Access Grant 生成路径。
- `get_download_ticket` 当前只接受 `message_security_profile = transport-protected`。

### 2.3 e2ee-attachment-cli-rs2 当前状态

端侧已有高层附件和 secure 能力骨架：

- `crates/im-core/src/messages/dto.rs` 有 `MessageBody::Attachment` 和 `MessageSecurityMode::E2eeRequired`。
- `crates/im-core/src/attachments/service.rs` 有 `attachments().send()` / `download()`。
- `crates/im-core/src/internal/attachment_runtime/upload.rs` 实现 base 上传、commit、manifest 发送。
- `crates/im-core/src/internal/attachment_runtime/download.rs` 实现 ticket 获取和对象下载。
- `crates/im-core/src/internal/secure_direct/*` 和 `crates/im-core/src/internal/group_e2ee/*` 已实现文本 E2EE。
- `crates/im-core-dart/src/dto/attachment.rs`、`packages/awiki_im_core` 已有附件 DTO。

主要缺口：

- `MessageService::send()` 对 `MessageBody::Attachment` 一律返回 unsupported。
- 附件上传只生成明文摘要并使用 `object_encryption_mode = none`。
- manifest builder 只能生成 `encryption_info.mode = none`。
- direct/group secure sender 主要面向 text，不支持 attachment inner plaintext。
- 下载路径不校验长度/摘要，不解析 `encryption_info`，不做 object decrypt。
- Dart / data 绑定层的附件发送请求缺少安全策略字段。

### 2.4 data-rs2 状态

当前工作区未检出名为 `data-rs2` 的仓库，也未搜索到明确同名接口文档。后续执行时需要确认其实际路径和 API 命名。本方案先按已有 `im-core-dart` / `packages/awiki_im_core` 设计绑定层形态：

- 对 data-rs2 不暴露 raw P7 wire。
- 对 data-rs2 不暴露对象密钥、nonce、raw cipher、ratchet/MLS state。
- 只暴露高层附件发送、下载、状态和错误 DTO。

## 3. 核心设计

### 3.1 总体链路

发送 direct E2EE 附件：

1. SDK 读取附件明文字节。
2. SDK 生成随机对象密钥 `K` 和 nonce `N`。
3. SDK 用 `ChaCha20-Poly1305(K, N, aad="")` 生成密文对象 `C`。
4. SDK 计算 `sha-256(C)`，记录 `size = len(C)`、`plaintext_size = len(P)`。
5. SDK 调 `attachment.create_slot`，其中 `intended_message_security_profile = direct-e2ee`、`object_encryption_mode = object-e2ee`、`expected_size = len(C)`、`expected_digest = sha-256(C)`。
6. SDK 通过 HTTPS PUT 上传密文 `C`。
7. SDK 调 `attachment.commit_object`，仍不传 `K` / `N`。
8. SDK 构造 E2EE 内层 `AttachmentMessage`，manifest 内包含 `object_key_b64u`、`nonce_b64u`、`plaintext_size`。
9. SDK 发送 `direct.send` E2EE init/cipher，内层 content type 是 `application/anp-attachment-manifest+json`。
10. 同一请求的域内 `client.attachment_grant_refs` 携带不含密钥的授权提示。sender-home 在最终 accepted 后校验并创建 Access Grant。

Group E2EE 附件相同，只是消息发送走 `group.e2ee.send`，内层 plaintext 是 P6 `Group Application Plaintext`。

接收下载：

1. SDK 拉取历史或监听事件，并在本地解密 direct/group E2EE 消息。
2. SDK 从内层 attachment manifest 选择目标附件。
3. SDK 根据原消息 `sender_did` 解析其 public `ANPMessageService.serviceDid`。
4. SDK 调 `attachment.get_download_ticket`，`message_security_profile` 使用 `direct-e2ee` 或 `group-e2ee`。
5. SDK 用 ticket 对 `object_uri` 发起 HTTPS GET，拿到密文字节。
6. SDK 先校验密文字节长度与 `sha-256(C)`。
7. SDK 使用内层 manifest 中的 `object_key_b64u` / `nonce_b64u` 本地解密。
8. SDK 校验 `len(P) == plaintext_size` 后，把明文字节写入文件或返回 memory bytes。

### 3.2 Access Grant 的关键设计

问题：E2EE 的 attachment manifest 在 P5/P6 内层密文里，服务端不能解密，但 P7 要求消息 accepted 后建立 Access Grant。不能通过服务端解密解决，也不能把对象密钥放到外层。

本方案采用 sender-home 本地域内非秘密授权提示：

```json
{
  "client": {
    "attachment_grant_refs": [
      {
        "attachment_id": "att-001",
        "object_uri": "https://objects.example.com/objects/obj-abc",
        "size": "1048592",
        "digest": {
          "alg": "sha-256",
          "value_b64u": "BASE64URL_SHA256_OF_CIPHERTEXT"
        },
        "mime_type": "application/pdf",
        "object_encryption_mode": "object-e2ee",
        "plaintext_size": "1048576"
      }
    ]
  }
}
```

约束：

- `attachment_grant_refs` 只进入 sender-home 的域内用户入口，用于 Access Grant 生成。
- 它不属于 P5/P6 inner plaintext，不替代 `AttachmentMessage`。
- 它不被转发到对端服务；现有 `build_forwarded_request()` 只转发 `meta/auth/body`，这一点应保持。
- 它不得包含 `object_key_b64u`、`nonce_b64u`、raw plaintext、raw ciphertext、文件路径或本地临时路径。
- 服务端必须基于 `attachment_objects` 校验 `owner_did`、`attachment_id`、`object_uri`、`size`、`digest`、`object_encryption_mode`、`plaintext_size` 和 committed 状态。
- direct 场景在最终 accepted 后为发送方和目标 DID 创建 direct grant。
- group 场景在最终 accepted 后为 `group_did` 创建 group grant，ticket 签发时仍检查 requester 当前 group membership。
- 如果缺少 `attachment_grant_refs`，E2EE 消息仍可 accepted，但后续下载 ticket 会因无 grant 返回 `anp.attachment.grant_not_found`。产品路径应把这视为发送实现错误并在 SDK 层避免。

这个设计不改变 ANP-P7 的密钥分发规则：对象密钥仍只通过 E2EE 内层 manifest 给能解密消息的接收者。grant refs 只让服务端知道“这个已提交密文对象和这个 accepted 消息绑定”，不足以解密对象。

### 3.3 服务端设计

#### 控制面

`attachment.create_slot`：

- 允许组合：
  - `transport-protected + none`
  - `direct-e2ee + object-e2ee`
  - `direct-e2ee + none`
  - `group-e2ee + object-e2ee`
  - `group-e2ee + none`
- 拒绝 `transport-protected + object-e2ee`。
- `object-e2ee` 时 `expected_size` 应是密文长度。
- 请求体仍不允许 `object_key_b64u` 和 `nonce_b64u`；当前 `deny_unknown_fields` 可以作为基础防线。

`attachment.commit_object`：

- 允许 `object_encryption_mode = object-e2ee`。
- `object-e2ee` 必须提供 `plaintext_size`。
- 继续校验已上传对象的实际 size/digest 与 commit 声明一致。
- 存储 `object_encryption_mode` 和 `plaintext_size`，不存密钥。

`attachment.get_download_ticket`：

- 接受 `message_security_profile = transport-protected | direct-e2ee | group-e2ee`。
- direct ticket 使用 `(attachment_id, object_uri, message_id, message_security_profile, message_target_did)` 查 grant。
- group ticket 使用 `(attachment_id, object_uri, message_id, message_security_profile, group_did)` 查 grant，并检查 requester 当前仍满足 group access policy。
- 继续要求 `requester_did == meta.sender_did`。

#### 消息 accepted 后建 grant

新增 E2EE grant collector：

- base collector 继续解析 `body.payload` 的 `AttachmentMessage`。
- E2EE collector 从 `req.client_context.attachment_grant_refs` 读取非秘密引用。
- direct E2EE 在 `direct.send` 最终 accepted 后调用 direct E2EE collector。
- group E2EE 在 `group.e2ee.send` 最终 accepted 后调用 group E2EE collector。

服务端不得尝试解析或解密 P5/P6 inner plaintext。服务端只把 E2EE 消息当 opaque payload 存储和投递。

### 3.4 SDK / im-core 设计

#### Public API 结论

优先不新增低层公开接口。推荐 canonical 入口：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Attachment {
        input,
        caption,
        mime_type,
    },
    security: MessageSecurityMode::E2eeRequired,
    client_message_id,
    delivery,
})
```

`client.attachments().send(target, AttachmentSendRequest)` 保留并增强：

- 增加可选 `security: MessageSecurityMode` 或复用 `AttachmentSendRequest.delivery` 外的安全策略字段。
- 默认继续 plain/base，避免破坏现有调用。
- 当 `security = E2eeRequired | SecureDirect | GroupE2ee` 时内部转调 `client.messages().send(MessageBody::Attachment, security)`，而不是另写一条 wire 路径。

这样可以让 CLI、Dart、data-rs2 使用同一条高层能力，减少 raw wire 和重复加密逻辑。

#### 内部模块

建议新增或扩展以下内部模块：

- `crates/im-core/src/internal/attachment_runtime/object_crypto.rs`
  - `encrypt_object_e2ee(plaintext, mime, filename) -> EncryptedAttachmentPayload`
  - `decrypt_object_e2ee(ciphertext, encryption_info) -> Vec<u8>`
  - 长度、base64url、key/nonce 长度校验。
- `crates/im-core/src/attachments/manifest.rs`
  - `AttachmentDescriptor` 扩展为完整 P7 `EncryptionInfo`。
  - builder 支持 `mode=object-e2ee`。
  - public result 默认 redacted，不输出 key/nonce。
- `crates/im-core/src/internal/attachment_runtime/upload.rs`
  - 拆分 prepare、encrypt、slot、put、commit、manifest。
  - 支持 `AttachmentObjectSecurity::None | ObjectE2ee`。
  - 生成 `attachment_grant_refs` 供 message send RPC 放入 `client` context。
- `crates/im-core/src/internal/secure_direct/send.rs`
  - 从 text-only 扩展为 application plaintext send。
  - 支持 `application_content_type = application/anp-attachment-manifest+json` 和 `payload = AttachmentMessage`。
- `crates/im-core/src/internal/group_e2ee/runtime.rs`
  - 从 text-only 扩展为 `GroupApplicationPlaintext { application_content_type, payload }`。
  - group E2EE send RPC 带 `client.attachment_grant_refs` 本地域内提示。
- `crates/im-core/src/internal/attachment_runtime/download.rs`
  - 通过高层消息读取/secure normalizer 找到已解密 manifest。
  - 取票据时使用正确 `message_security_profile`。
  - 下载后先校验密文字节长度和摘要，再本地解密和校验 `plaintext_size`。

#### DTO 与脱敏

`UploadedAttachment` 建议扩展：

```rust
pub struct UploadedAttachment {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub object_encryption_mode: String,
    pub plaintext_size_bytes: Option<u64>,
}
```

规则：

- `size` / `size_bytes` 仍表示 P7 manifest 的上传对象大小，即 `object-e2ee` 下的密文大小。
- UI/CLI 展示原始文件大小时使用 `plaintext_size_bytes`。
- `AttachmentSendResult.manifest` 在 public API 是否保留需要 Review。若保留，E2EE manifest 会含对象密钥，应改为 redacted manifest 或只在 internal 返回完整 manifest。
- `DownloadedAttachment.selection` 当前是 public field。实现 E2EE 后不能把含 `object_key_b64u` / `nonce_b64u` 的 selection 暴露出去，应拆分 internal selection 与 public redacted selection。

### 3.5 CLI 设计

不新增低层 E2EE 附件命令。用户入口：

```bash
awiki-cli msg send --to DID --file ./report.pdf --secure required
awiki-cli msg send --group GROUP_DID --file ./design.png --secure required
awiki-cli msg attachment download --with DID --message-id MSG --attachment-id ATT --output ./report.pdf
awiki-cli msg attachment download --group GROUP_DID --message-id MSG --attachment-id ATT --output ./design.png
```

CLI 责任：

- 解析 `--file`、`--mime-type`、`--filename`、`--caption`、`--secure required`。
- 构造 high-level `SendMessageRequest` 或 `AttachmentSendRequest`。
- 调 SDK，不拼 P7/P5/P6 wire payload。
- 下载时只显示高层结果和 warnings。
- 默认输出不得包含 full manifest、object key、nonce、ciphertext、ticket、JWT、private key、secure session state、MLS provider path。

兼容：

- `--secure on` 如已有 deprecated alias，继续映射到 `required`。
- plain `msg send --file` 继续走 `transport-protected + mode=none`。
- 当 peer/group secure state 不可用时，返回 secure status / repair 提示，而不是自动降级为 plain。

### 3.6 Dart / data-rs2 设计

`im-core-dart` / `packages/awiki_im_core`：

- `DartAttachmentSendRequest` 增加 `security: DartMessageSecurityMode`，默认值在 Dart API 层保持 plain/default。
- 或新增统一的 `sendAttachmentMessage()`，内部仍映射到 Rust `MessageBody::Attachment`。
- `DartUploadedAttachment` 增加 `objectEncryptionMode`、`plaintextSizeBytes`。
- `DartDownloadedAttachment` 返回明文字节或明文文件路径；不返回密文字节。
- `manifestJson` 对 E2EE 发送结果必须 redacted 或不返回。不能把对象密钥透给 App/data 层。

data-rs2：

- 当前未检出，后续执行先确认实际路径。
- 若 data-rs2 是 App/data facade，应只暴露同等高层接口：
  - `sendAttachment(target, input, security, caption, options)`
  - `downloadAttachment(thread, messageId, attachmentId, destination)`
  - `AttachmentTransferState` / `AttachmentTransferError`
- 不暴露 `attachment.create_slot`、`commit_object`、ticket、object key、nonce、raw manifest、raw cipher。
- 如果已有通用 message send 接口支持 `MessageBody::Attachment + security`，则不新增专门 data-rs2 API，只补 DTO 和文档。

## 4. 安全与隐私

必须满足：

- 对象密钥每个对象随机生成，不复用 `(K, N)`。
- `object_key_b64u` 和 `nonce_b64u` 不进入控制面请求、Access Grant、Download Ticket、服务端存储、日志、CLI 输出、Dart public DTO、data-rs2 public DTO。
- 服务端只保存密文对象和不可解密的 E2EE 消息。
- 下载必须先验证密文长度和 digest，再解密。
- 解密失败必须拒绝附件并返回明确错误，不能写出部分明文。
- direct/group E2EE public discovery 继续关闭，除非另有安全评审。
- group 移除成员后拒绝签发新的 ticket，但不承诺追溯撤回已获取内容。

## 5. 错误模型

复用 P7 错误段：

- `anp.attachment.encryption_policy_violation`
- `anp.attachment.grant_not_found`
- `anp.attachment.unauthorized_requester`
- `anp.attachment.digest_mismatch`
- `anp.attachment.decrypt_failed`
- `anp.attachment.object_unavailable`
- `anp.attachment.ticket_binding_mismatch`

SDK 映射：

- 服务端 P7 错误透传为 `ImError::Remote` 或已有 RPC error 类型。
- 本地 digest mismatch / decrypt failed 应映射为明确的 `ImError`，并避免输出 key/nonce。
- secure state 不可用返回 secure problem，不自动降级。

## 6. 验证策略

最低验证等级按 L3 执行：

- message-service focused tests：P7 object-e2ee policy、commit 校验、E2EE grant refs、ticket direct/group。
- im-core focused tests：object crypto、manifest redaction、secure attachment send、download verify/decrypt。
- CLI contract tests：`msg send --file --secure required`、download 自动解密、JSON 输出脱敏。
- Dart / data binding tests：DTO 兼容、redaction、high-level API shape。
- awiki-system-test：direct E2EE 附件 E2E、group E2EE 附件 E2E、无 grant / wrong digest / group removed member negative tests。

最终系统测试必须在 `awiki-system-test` 下执行 remote mode：

```bash
cd awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info \
  AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 \
  CARGO_BUILD_JOBS=1 \
  uv run --no-sync awiki-system-test
```

如果只做 focused 验证，也必须记录通过、失败、跳过数量和关键环境配置。

## 7. 兼容与发布

- plain attachment 行为保持不变。
- 旧客户端看到 E2EE 附件消息时，如果不能解密 P5/P6，只能看到 opaque secure message，不能下载附件。
- 新客户端对缺少 grant 的历史 E2EE 附件应报告可修复错误；是否提供 sender-side repair 命令留后续评审。
- 服务端 public discovery 不因本功能自动打开 direct/group E2EE。
- 如果后续 ANP-P7 标准新增正式 grant hint 字段，本实现应把 `client.attachment_grant_refs` 收敛或迁移到标准字段。

## 8. 待确认问题

1. `data-rs2` 的实际仓库路径和 public API 命名当前不可见，执行时需先定位。
2. `AttachmentSendResult.manifest` 是否继续作为 public 字段返回需要 API Review；E2EE 场景建议改为 redacted 或废弃。
3. 大文件 object-e2ee 首期是否设定低于普通附件的大小上限。P7 MTI 是单次 ChaCha20-Poly1305，对超大文件需要内存和临时文件策略评审。
4. group 跨域且 group host 不等于 sender-home 时，sender-home 如何获得最终 acceptance。当前方案假设发送路径经过 sender-home，sender-home 可在最终 accepted 后创建 grant；若存在客户端直连远端 group host，则必须禁用或补 sender-home 回调/代理。
