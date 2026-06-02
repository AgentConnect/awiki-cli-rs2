# Step 06：CLI、Dart、data-rs2 高层接口

主 Plan：[../plan.md](../plan.md)  
Step index：06  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2`，`data-rs2: 未检出` |
| Started | 2026-06-02 12:06 CST |
| Completed | 2026-06-02 13:03 CST |
| Commit | `e2ee-attachment-cli-rs2:e10a74b` |
| Review evidence | Review 覆盖 CLI 薄壳边界、Dart/Flutter DTO 脱敏、secure 便捷 API 高层复用、plain 兼容、filename override、generated bridge、文档同步和 `data-rs2` 未检出记录；修复 async secure attachment 便捷发送递归 future 风险和 secure 便捷路径 filename override 丢失风险。 |
| Verification evidence | `cargo fmt --all --check`、`cargo check -p im-core-dart --locked`、`cargo check -p im-core --features group-e2ee --locked`、`cargo test -p awiki-cli --test msg_attachment_contract --locked`、`cargo test -p awiki-cli --locked msg_secure`、`cargo test -p awiki-cli --test attachment_live_contract --locked`、`cargo test -p im-core-dart --locked attachment`、`cargo test -p im-core attachment_api --locked`、`bash scripts/flutter/codegen-check.sh`、`cd packages/awiki_im_core && dart test`、`git diff --check`、secret grep 通过或已复核；宽过滤 `cargo test -p awiki-cli --locked msg_attachment` 因 unrelated `msg inbox` 子进程阻塞改用精确测试替代。 |
| Next action | 启动 Step 07：系统测试、文档与集成收口 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：CLI、Dart / Flutter SDK 和 data-rs2 使用高层附件 E2EE 能力，不拼 raw wire，不暴露密钥。
- 用户 / 系统可见行为：`msg send --file --secure required` 和附件下载按预期工作；Dart/data 可选择 secure required。
- 非目标：不新增低层 control-plane/ticket/object-key API。
- 完成标准：CLI contract tests、Dart codegen/check、data-rs2 接口确认或记录外部 blocker。

## 3. 设计方法

- 设计边界：CLI 是壳；Dart/data 是 facade；业务和加密在 `im-core`。
- 核心决策：如果已有通用 message send 支持 attachment + security，优先复用；专门 attachment API 只做便捷封装。
- 契约 / API / 数据流：CLI flags -> SDK DTO -> `MessageBody::Attachment` / `AttachmentSendRequest`；Dart DTO 增加 `security`。
- 兼容性：无 `--secure` 的文件发送保持 plain；`--secure required` 失败时不降级。
- 迁移策略：FRB 生成文件按项目脚本更新；data-rs2 未检出时不伪造实现。
- 风险控制：输出脱敏测试；manifestJson redacted。

## 4. 实现方法

1. CLI：
   - 检查 `crates/awiki-cli/src/cli_shell/msg_handlers.rs` 和 parser 是否已有 `--file` + `--secure` 组合。
   - 让 `msg send --file --secure required` 构造 high-level SDK request。
   - `msg attachment download` 复用 `client.attachments().download()`，不自己取 ticket 或解密。
   - JSON/table 输出增加 `object_encryption_mode`、`plaintext_size`，不输出 full manifest key/nonce。
2. `m_core_cli_adapter`：
   - 添加 attachment send/download adapter tests。
   - 错误提示区分 secure state unavailable、grant not found、digest mismatch、decrypt failed。
3. Dart / FRB：
   - `crates/im-core-dart/src/dto/attachment.rs` 的 `DartAttachmentSendRequest` 增加 `security: DartMessageSecurityMode`。
   - `DartUploadedAttachment` 增加 `object_encryption_mode`、`plaintext_size_bytes`。
   - `manifest_json` 对 E2EE result 改为 redacted 或空/废弃字段，并更新 README。
   - 运行 codegen check。
4. `packages/awiki_im_core`：
   - 更新 Dart public models 和 tests。
   - 确认 App 层只拿高层明文结果。
5. data-rs2：
   - 先在工作区定位实际仓库路径和 `AGENTS.md`。
   - 如果存在，按其现有接口风格增加 `security` 和 redacted attachment DTO。
   - 如果仍不存在，在主 Plan 台账记录：未检出，需用户提供路径；不阻塞已检出仓库实现。
6. 文档：
   - 更新 `docs/api/im-core-public-api.md`、`docs/api/im-core-interface/04-message-interface.md`、`docs/flutter-sdk/awiki-im-core-flutter-sdk.md`。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `e2ee-attachment-cli-rs2/crates/awiki-cli/src/cli_shell/msg_handlers.rs` | `--file --secure required` 走 SDK | 不拼 raw wire |
| `e2ee-attachment-cli-rs2/crates/awiki-cli/src/m_core_cli_adapter/messages.rs` | adapter 映射和输出 |  |
| `e2ee-attachment-cli-rs2/crates/awiki-cli/tests/msg_attachment_contract.rs` | CLI contract tests |  |
| `e2ee-attachment-cli-rs2/crates/awiki-cli/tests/msg_secure*` | secure flag 回归 |  |
| `e2ee-attachment-cli-rs2/crates/im-core-dart/src/dto/attachment.rs` | Dart DTO 扩展 |  |
| `e2ee-attachment-cli-rs2/packages/awiki_im_core/lib/src/*` | 生成/手写 API 更新 | 按项目脚本 |
| `e2ee-attachment-cli-rs2/packages/awiki_im_core/test/*` | Dart tests |  |
| `data-rs2` | 待定位后按 AGENTS 执行 | 当前未检出 |

## 6. 依赖

- 前置步骤：Step 04、Step 05。
- 外部文档或决策：data-rs2 实际路径。
- 环境前提：Rust CLI tests、Flutter/Dart codegen 工具可用。

## 7. 验收标准

- [x] CLI `msg send --file --secure required` 使用 `im-core` high-level API。
- [x] CLI download 输出明文文件/memory，不输出 ticket/key/nonce。
- [x] Dart DTO 支持 secure attachment request 和 redacted result。
- [x] data-rs2 若存在，接口按同一高层边界实现；若不存在，记录 blocker 和后续动作。
- [x] 文档同步 public API 和 Flutter SDK。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| Format | `cd e2ee-attachment-cli-rs2 && cargo fmt --all --check` | 格式通过。 |
| CLI attachment | `cd e2ee-attachment-cli-rs2 && cargo test -p awiki-cli --locked msg_attachment` | attachment CLI contract 通过。 |
| CLI secure | `cd e2ee-attachment-cli-rs2 && cargo test -p awiki-cli --locked msg_secure` | secure flag 回归通过。 |
| Dart facade | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core-dart --locked attachment` | Rust Dart facade tests 通过。 |
| Flutter codegen | `cd e2ee-attachment-cli-rs2 && bash scripts/flutter/codegen-check.sh` | 生成文件一致。 |
| Dart tests | `cd e2ee-attachment-cli-rs2/packages/awiki_im_core && dart test` | 如环境可用则通过；不可用记录原因。 |
| Secret grep | `cd e2ee-attachment-cli-rs2 && rg -n "object_key_b64u|nonce_b64u|download_ticket" crates/awiki-cli crates/im-core-dart packages/awiki_im_core` | public CLI/Dart 不输出秘密；测试/内部字段命中需说明。 |

## 9. Review 环节

Review 重点：

- CLI 是否仍是薄壳，没有拼 P7/P5/P6 wire。
- Dart/data DTO 是否没有 key/nonce/ticket。
- `manifestJson` 是否 redacted 或废弃，不泄漏 full manifest。
- 错误提示是否足够诊断但不暴露秘密。
- data-rs2 未检出时是否明确记录而非假实现。

Review 结果：

- CLI 仅解析 `--file`、`--mime-type`、caption 和 `--secure`，发送和下载都通过 `im-core` 高层 API；未新增 raw P7/P5/P6 wire 拼装。
- `AttachmentSendRequest.security` 支持 plain 默认和 secure required；secure 便捷路径内部复用 `MessageBody::Attachment + security`，返回 `AttachmentSendResult` 时从 redacted manifest 还原 public `UploadedAttachment`。
- Dart / Flutter DTO 增加 `security`、`objectEncryptionMode`、`plaintextSizeBytes`；`manifestJson` 仍只承接 im-core public redacted manifest。
- 修复发现：
  - `attachments().send_async` 直接调 `messages().send_async` 会形成 async recursive future；改为 `MessageService` crate 内 `send_secure_attachment*` 专用入口。
  - secure 便捷路径可能丢失 `filename` override；将 `MessageBody::Attachment` 扩展为包含 `filename`，保持 canonical message send 与 attachment convenience API 语义一致。
- `data-rs2` 重新定位未找到实际仓库：`find .. -maxdepth 2 -type d \( -name '*data*rs2*' -o -name 'data-rs2' -o -name '*data*' \) -print` 仅发现通用 `data` 目录，未发现 `data-rs2`；记录为外部待办，不伪造实现。
- Secret grep 命中仅为测试脱敏断言和 attachment live 测试 fixture 中的 `download_ticket_b64u` 模拟值；CLI/Dart/Flutter public 输出代码未新增 key/nonce/ticket 泄漏。

验证结果：

- `cargo fmt --all --check` 通过。
- `cargo check -p im-core-dart --locked` 通过。
- `cargo check -p im-core --features group-e2ee --locked` 通过。
- `cargo test -p awiki-cli --test msg_attachment_contract --locked`：3 passed, 0 failed。
- `cargo test -p awiki-cli --locked msg_secure`：6 个匹配测试通过，其余 filtered。
- `cargo test -p awiki-cli --test attachment_live_contract --locked`：4 passed, 0 failed。
- `cargo test -p im-core-dart --locked attachment`：facade_contract 2 passed, 0 failed。
- `cargo test -p im-core attachment_api --locked`：attachment_api 1 passed, 0 failed。
- `bash scripts/flutter/codegen-check.sh` 通过。
- `cd packages/awiki_im_core && dart test`：7 passed。
- `git diff --check` 通过。
- `rg -n "object_key_b64u|nonce_b64u|download_ticket" crates/awiki-cli crates/im-core-dart packages/awiki_im_core` 已复核，命中仅为测试脱敏断言和 live fixture。
- `cargo test -p awiki-cli --locked msg_attachment` 曾卡在 unrelated `awiki-cli msg inbox --scope direct --unread --limit 20 --format json` 子进程，已终止并用精确 `msg_attachment_contract` 覆盖本步骤附件合同。
- 一次串联验证命令在 `msg_secure` 阶段长时间无输出被终止，已用单独通过的 `msg_secure`、`attachment_live_contract`、Dart tests 和 secret grep 证据替代。

提交记录：

- 实现提交：`e2ee-attachment-cli-rs2:e10a74b`。
- Commit 前状态：仅本步骤实现文件、API/Flutter/design 文档和 Step 台账文档有未提交修改。
- 实现提交包含：CLI、im-core、im-core-dart、Flutter generated/model/tests、API/Flutter/design 文档；不包含主 Plan/Step 台账回填。
- 后续台账提交：本文件与主 Plan 回填。

## 10. Commit 要求

- Commit 前记录相关仓库 `git status --short --branch`。
- `e2ee-attachment-cli-rs2` 修改创建一个聚焦 commit。
- 如果定位并修改 `data-rs2`，在其仓库按自身规则创建独立 commit。
- Commit 后记录 hash 和 post-commit status 到主 Plan 执行台账。

## 11. 风险、假设与回滚

- 风险：FRB 生成文件体积大。缓解：按脚本生成并单独核对 diff。
- 风险：Dart public API 破坏兼容。缓解：给 `security` 提供默认路径或新增方法。
- 回滚：CLI/Dart 暂不暴露 secure attachment，保留 im-core 能力。
