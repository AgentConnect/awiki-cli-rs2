# Plan：附件端到端加密传输

状态：in_progress  
DOC：`e2ee-attachment-cli-rs2/docs/e2ee-attachment/`  
Harness：`awiki-harness`  
创建时间：2026-06-02 08:00 CST  
恢复指针：Step 06：CLI、Dart、data-rs2 高层接口

## 1. 目标

- 任务目标：按 ANP-P7、P5、P6 和 AWiki 现有 Direct/Group E2EE 边界，完成附件端到端加密传输设计，并准备可执行的跨仓库实现计划。
- 预期行为：客户端可发送 direct/group E2EE 附件；服务端只保存密文对象和 opaque E2EE 消息；接收端通过 ticket 下载密文后本地校验和解密；CLI、Dart / data-rs2 使用高层接口。
- 非目标：不做服务端解密，不开放 public E2EE discovery，不新增附件密钥协商协议，不承诺 group 成员移除后的追溯撤回。
- 完成标准：本 Plan 和每个小 Plan 可指导后续 Codex Goal 分步实现、Review、验证和提交；架构方案文档说明核心安全边界、接口结论、服务端和端侧改动。

## 2. Harness 上下文

| 来源 | 作用 |
|---|---|
| `awiki-harness/README.md` | 确认 Harness 是控制面，子仓库仍是实现权威。 |
| `awiki-harness/context/00-context-map.md` | 路由到 Protocol、Message Flow、E2EE、Client Architecture、System Test。 |
| `awiki-harness/context/02-repo-map.md` | 确认 `message-service`、`awiki-cli-rs2`、`anp`、`awiki-system-test` 职责。 |
| `awiki-harness/context/03-cross-repo-architecture.md` | 确认服务端 opaque E2EE 边界和 `im-core` 高层 SDK 边界。 |
| `awiki-harness/context/20-rules-index.md` | 路由到架构与验证规则。 |
| `awiki-harness/context/30-tools-env.md` | 记录各仓库验证命令入口。 |
| `awiki-harness/context/40-verification.md` | 本任务按 L3 协议/E2EE/安全变更设计验证。 |
| `awiki-harness/context/50-task-workflow.md` | 采用 context、analysis、solution plan、verification 的任务结构。 |
| `awiki-harness/context/nodes/e2ee.node.md` | 确认私钥、会话状态和对象解密属于客户端/SDK。 |
| `awiki-harness/context/nodes/message-flow.node.md` | 确认 Rust `message-service` v2 不能按 legacy 行为推断。 |
| `awiki-harness/context/nodes/protocol.node.md` | 协议细节以 `anp/AgentNetworkProtocol` 为权威。 |
| `awiki-harness/context/nodes/client-architecture.node.md` | 确认 CLI/App 不拼 raw wire，复用 `im-core`。 |
| `awiki-harness/features/direct-e2ee.md` | Direct E2EE 当前跨仓库状态和 discovery gate。 |
| `awiki-harness/features/group-e2ee.md` | Group E2EE hidden/test-only 状态和 discovery gate。 |
| `awiki-harness/rules/architecture-principles.md` | 服务不得持有 E2EE 明文/私钥，CLI 壳不得复制 SDK 逻辑。 |
| `awiki-harness/rules/verification-policy.md` | L3 验证和 security review gate。 |

## 3. 影响分析

| 领域 / 仓库 / 模块 | 影响 | 权威文档或代码 |
|---|---|---|
| ANP 协议 | 复用 P7 object-e2ee、P5/P6 内层 attachment manifest，不新增业务方法。 | `anp/AgentNetworkProtocol/message/05-direct-end-to-end-encryption.md`、`06-group-end-to-end-encryption.md`、`07-attachments-and-object-transfer.md` |
| message-service attachment | 放开 `object-e2ee` 控制面、增加 E2EE grant refs、ticket 支持 direct/group E2EE。 | `message-service/docs/api/ANP-client-server-api-attachment.md`、`message-service/crates/im-attachment/src/*` |
| message-service direct/group | direct/group E2EE accepted 后创建 Access Grant，但不解密 inner plaintext。 | `message-service/crates/im-direct/src/service.rs`、`message-service/crates/im-group/src/handlers.rs` |
| e2ee-attachment-cli-rs2 im-core | 对象加解密、manifest、secure attachment send/download、高层 API。 | `e2ee-attachment-cli-rs2/crates/im-core/src/attachments/*`、`internal/attachment_runtime/*`、`internal/secure_direct/*`、`internal/group_e2ee/*` |
| e2ee-attachment-cli-rs2 CLI | `msg send --file --secure required` 和 `msg attachment download` 走高层 SDK，不输出密钥。 | `e2ee-attachment-cli-rs2/crates/awiki-cli/src/cli_shell/msg_handlers.rs`、`m_core_cli_adapter/*` |
| Dart / data-rs2 | DTO 增加安全策略和 redacted result；data-rs2 需实际定位后对齐。 | `e2ee-attachment-cli-rs2/crates/im-core-dart/src/dto/attachment.rs`、`packages/awiki_im_core` |
| awiki-system-test | 增加 direct/group E2EE 附件 E2E 和 negative tests。 | `awiki-system-test/docs/direct-e2ee-system-tests.md`、`group-e2ee-system-tests.md`、`tests_v2/cli/*` |

## 4. 假设与开放问题

### 假设

- 发送 E2EE 附件的请求先经过 sender-home，sender-home 能在最终 accepted 后创建 Access Grant。
- `client.attachment_grant_refs` 是域内实现提示，不作为跨域协议 body 转发。
- `data-rs2` 是上层 data binding/facade；当前工作区未检出，后续执行先确认路径。
- `MessageBody::Attachment + MessageSecurityMode::E2eeRequired` 可以作为 canonical 高层 SDK 入口。

### 开放问题

- `data-rs2` 的实际仓库路径、crate/package 名和现有接口命名。
- `AttachmentSendResult.manifest` 是否继续公开；E2EE 场景建议 redacted 或废弃。
- 首期 object-e2ee 大文件上限和内存策略。
- group 跨域直连远端 group host 是否存在绕过 sender-home 的产品路径。

## 5. 总体设计方法

- 设计边界：P7 控制面和数据面保持不变；P5/P6 继续承载加密业务内容；服务端只用非秘密 grant refs 建授权。
- 关键决策：对象密钥只在 E2EE 内层 manifest；Access Grant 通过 sender-home 本地 `client.attachment_grant_refs` 建立。
- 兼容性策略：plain attachment 不变；E2EE discovery 不自动公开；旧客户端无法解密时不能下载。
- 数据、协议、配置或迁移策略：message-service 现有 schema 已预留枚举，优先避免新 migration；如需要索引或字段再单步迁移。
- 风险控制：每步都有 Review gate；重点审查 key/nonce 泄漏、grant 绕过、ticket 绑定、group member policy、CLI/Dart 输出脱敏。

## 6. 任务拆分

| Step | 标题 | 依赖 | 产出 | 小 Plan 文档 | Commit gate | 状态 |
|---|---|---|---|---|---|---|
| 01 | 服务端 P7 object-e2ee 控制面 | 无 | message-service 支持 `object-e2ee` slot/commit/ticket 基础策略 | [steps/01-service-object-e2ee-control.md](steps/01-service-object-e2ee-control.md) | 必须 | done |
| 02 | 服务端 E2EE Access Grant | Step 01 | direct/group E2EE accepted 后基于非秘密 grant refs 建 grant | [steps/02-service-e2ee-access-grants.md](steps/02-service-e2ee-access-grants.md) | 必须 | done |
| 03 | im-core 对象加解密与 manifest 模型 | Step 01 | 客户端 object crypto、manifest、redacted DTO 基础 | [steps/03-im-core-object-crypto-manifest.md](steps/03-im-core-object-crypto-manifest.md) | 必须 | done |
| 04 | im-core secure attachment send | Step 02、Step 03 | `MessageBody::Attachment + E2eeRequired` 发送 direct/group E2EE 附件 | [steps/04-im-core-secure-attachment-send.md](steps/04-im-core-secure-attachment-send.md) | 必须 | done |
| 05 | im-core 下载校验与解密 | Step 02、Step 03 | E2EE 附件下载、ticket、digest、decrypt、plaintext write | [steps/05-im-core-download-decrypt.md](steps/05-im-core-download-decrypt.md) | 必须 | done |
| 06 | CLI、Dart、data-rs2 高层接口 | Step 04、Step 05 | CLI UX、Dart DTO、data-rs2 对接结论和实现 | [steps/06-cli-dart-data-interfaces.md](steps/06-cli-dart-data-interfaces.md) | 必须 | pending |
| 07 | 系统测试、文档与集成收口 | Step 01-06 | system-test、文档同步、最终集成证据 | [steps/07-system-tests-docs-integration.md](steps/07-system-tests-docs-integration.md) | 必须 | pending |

## 7. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

| Step | 状态 | 分支 | 开始时间 | 完成时间 | Commit | Review 证据 | 验证证据 | 下一步 |
|---|---|---|---|---|---|---|---|---|
| 01 | done | `message-service: release/0526` | 2026-06-02 09:06 CST | 2026-06-02 09:13 CST | `message-service:b93964bfe59bcdd200375c69a29f00ff0de55855` | Review 覆盖 policy 组合、key/nonce 不进入控制面、ticket 仍依赖 grant、commit 失败路径；发现并修复 focused test 未覆盖新增用例和 `plaintext_size` 解析时机。 | `cd message-service && cargo fmt --all --check` 通过；`cargo test -p im-attachment attachment -- --nocapture` 7 passed, 0 failed, 15 filtered；`cargo test -p im-types attachment -- --nocapture` 0 passed, 0 failed, 0 filtered；`cargo check -p message-service` 通过；`git diff --check` 通过。 | 启动 Step 02 |
| 02 | done | `message-service: release/0526` | 2026-06-02 09:15 CST | 2026-06-02 09:51 CST | `message-service:938fcde85fc9a8ed859ed617ecf36ee5163ea1dc` | Review 覆盖 E2EE grant refs 非秘密字段、authoritative object 校验、direct/group accepted 后写 grant、跨域不转发 `client`、ticket 绑定和 group membership；发现并修复 group E2EE replay 在 refs 校验前未先命中 idempotency 的问题，补 group 服务级 accepted/replay 测试。Secret grep 命中仅为 `plaintext_size` 元数据、key/nonce 拒绝逻辑和测试数据，未发现生产路径保存对象 key/nonce/明文。 | `cd message-service && cargo fmt --all --check` 通过；`cargo test -p im-attachment grant -- --nocapture` 5 passed, 0 failed, 22 filtered；`cargo test -p im-direct direct_e2ee -- --nocapture` 6 passed, 0 failed, 27 filtered；`cargo test -p im-group group_e2ee -- --nocapture` 22 passed, 0 failed, 26 filtered；`cargo check -p message-service` 通过；`git diff --check` 通过；secret grep 已复核。 | 启动 Step 03 |
| 03 | done | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` | 2026-06-02 09:51 CST | 2026-06-02 10:38 CST | `e2ee-attachment-cli-rs2:91a9da2` | Review 覆盖 AEAD 算法、32 字节 key/12 字节 nonce、AAD 为空、ciphertext size/digest、控制面 key/nonce 禁止、redacted manifest/public DTO 脱敏、plain manifest 兼容和下游 Rust DTO 编译影响；发现并修复公开 `PreparedAttachment` / `AttachmentDescriptor` 曾持有 key/nonce 的泄漏风险，改为 internal-only `ObjectE2eeAttachmentSecrets`，同时移除 compat 解密 helper。Secret grep 命中仅为 internal manifest/crypto 和测试。 | `cargo fmt --all --check` 通过；`cargo test -p im-core attachment_object_crypto --locked` 2 passed, 0 failed, 238 filtered；`cargo test -p im-core attachment_manifest --locked` lib 2 passed、attachment_api 2 passed，0 failed；`cargo test -p im-core attachment_api --locked` 1 passed, 0 failed, 20 filtered；`cargo check -p im-core` 通过；`cargo check -p im-core-dart` 通过；`git diff --check` 通过；secret grep 已复核。 | 启动 Step 04 |
| 04 | done | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` | 2026-06-02 10:39 CST | 2026-06-02 11:58 CST | `e2ee-attachment-cli-rs2:5c97627` | Review 覆盖高层 Attachment + E2eeRequired 路由、object-e2ee upload/commit、direct/group inner plaintext、outer body/client refs 非秘密边界、本地 projection redaction、plain attachment 兼容和 no-`blocking` async direct 路径；发现并修复 async direct E2EE 附件在无 `blocking` feature 时先上传/commit 再返回 unsupported 的孤儿密文对象风险，补 `prepare_and_commit_object` focused test 验证 PUT 仅上传密文、控制面无 key/nonce、full/redacted/grant refs 边界。Secret grep 命中仅为测试断言和测试内读取 full manifest。 | `cargo fmt --all --check` 通过；`cargo check -p im-core` 通过；`cargo check -p im-core --features group-e2ee` 通过；`cargo test -p im-core attachments_upload_runtime_prepare_object_e2ee_uploads_ciphertext_only --locked` 1 passed；`cargo test -p im-core secure_attachment_send --locked` 1 passed；`cargo test -p im-core secure_attachment_send --features group-e2ee --locked` 2 passed；`cargo test -p im-core secure --locked` 66 lib passed，phase1a 2 passed，realtime_loop 2 passed，secure_api 10 passed；`cargo test -p im-core e2ee --locked` 21 lib passed，attachment_api 2 passed，phase1a 3 passed；`cargo test -p im-core attachment --locked` 23 lib passed，attachment_api 21 passed，phase1a 1 passed，realtime_projection 7 passed；`cargo test -p im-core e2ee --features group-e2ee --locked` 76 lib passed，attachment_api 2 passed，phase1a 3 passed；`cargo check -p im-core-dart` 通过；`git diff --check` 通过；secret grep 已复核。 | 启动 Step 05 |
| 05 | done | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2` | 2026-06-02 12:00 CST | 2026-06-02 12:03 CST | `e2ee-attachment-cli-rs2:e0670208d823c817261f17f5335eb8dbacca3b03` | Review 覆盖校验顺序、object-e2ee key/nonce 仅 internal selection 使用、public `DownloadedAttachment.selection` / `AttachmentDownloadResult.selection` redacted、ticket profile direct/group 推导、sync/async local-file 解密失败不写出、secure-aware history projection 和 plain attachment 兼容；未发现需修复问题。Secret grep 命中仅为 internal crypto/manifest/selection/download 解密逻辑和测试断言，Dart/public DTO 未新增 key/nonce 输出。 | `cargo fmt --all --check` 通过；`cargo check -p im-core` 通过；`cargo check -p im-core --features group-e2ee` 通过；`cargo test -p im-core attachments_download_runtime --locked` 13 passed, 0 failed, 235 filtered；`cargo test -p im-core attachment --locked` lib 29 passed、attachment_api 22 passed、phase1a 1 passed、realtime_projection 7 passed，0 failed；`cargo test -p im-core attachment_object_crypto --locked` 2 passed, 0 failed；`cargo test -p im-core secure --locked` 66 lib passed、phase1a 2 passed、realtime_loop 2 passed、secure_api 10 passed，0 failed；`cargo test -p im-core e2ee --locked` 24 lib passed、attachment_api 3 passed、phase1a 3 passed，0 failed；`cargo test -p im-core e2ee --features group-e2ee --locked` 79 lib passed、attachment_api 3 passed、phase1a 3 passed，0 failed；`cargo test -p im-core decrypt --locked` 4 passed, 0 failed；`cargo test -p im-core attachment_download --locked` 0 passed、0 failed、过滤词未命中实际下载测试，已用 `attachments_download_runtime` 覆盖；`cargo check -p im-core-dart` 通过；`git diff --check` 通过；secret grep 已复核。 | 启动 Step 06 |
| 06 | pending | `e2ee-attachment-cli-rs2: feature/release-0526/e2ee-attachment-cli-rs2`，`data-rs2: 待确认` |  |  |  |  |  | 等 Step 04、05 |
| 07 | pending | `awiki-system-test: release/0526`，相关实现仓库分支同上 |  |  |  |  |  | 等 Step 01-06 |

## 8. Codex Goal 执行协议

- 将本 Plan 作为执行进度的唯一事实来源。
- 启动或恢复前，读取本 Plan、当前小 Plan、执行台账和当前 `git status`。
- 同一时间只执行一个步骤，除非本 Plan 明确标记多个步骤彼此独立且可以并行。
- 恢复时，从第一个状态不是 `done` 的步骤继续。
- 每个步骤依次执行：标记 `in_progress`、实现、验证、Review、修复 Review 发现、提交、记录证据、标记 `done`。
- 上一个依赖步骤的完成工作未提交前，不要开始下一个依赖步骤。
- 改变范围、顺序、验收标准、公开契约、数据模型或验证策略前，先更新本 Plan。
- 涉及系统测试的最终集成步骤必须在 `awiki-system-test` 下执行 remote mode，使用 `awiki.info` 域名，并记录通过/失败/跳过数量、失败或跳过原因和关键环境配置。

## 8.1 Codex Goal 提示词

```text
请以 `e2ee-attachment-cli-rs2/docs/e2ee-attachment/plan.md` 为唯一规划入口，按文档执行完整实现。

开始前先读取：
- `e2ee-attachment-cli-rs2/docs/e2ee-attachment/plan.md`
- 当前第一个未 done 的 Step 文档
- 主 Plan 的执行台账、Codex Goal 执行协议、验证策略、Blocked 处理和 Plan 变更记录
- 当前 `git status --short --branch`

请从第一个状态不是 `done` 的步骤开始，一次只执行一个步骤。每步都要按对应小 Plan 实现、验证、Review、修复或记录 Review 发现，然后创建一个聚焦 commit，并回填主 Plan 执行台账和 Step 执行状态。需要改变范围、顺序、验收标准、公开契约、数据模型或验证策略时，先更新 Plan 变更记录。

所有步骤完成后，执行最终全局 Review 和整体验证，记录实际命令、通过/失败/跳过数量、失败或跳过原因、剩余风险和最终工作区状态。

核心注意点：服务端不得保存附件对象密钥/nonce/明文；`client.attachment_grant_refs` 只能包含非秘密授权引用且不得跨域转发；CLI/Dart/data-rs2 只走高层 SDK；public direct/group E2EE discovery 继续关闭；最终系统测试在 `awiki-system-test` 使用 `AWIKI_SYSTEM_TEST_MODE=remote` 和 `awiki.info`。
```

## 9. 小 Plan 摘要

### Step 01：服务端 P7 object-e2ee 控制面

- 小 Plan：[steps/01-service-object-e2ee-control.md](steps/01-service-object-e2ee-control.md)
- 目标：message-service 控制面接受合法 `object-e2ee`，拒绝非法组合和密钥字段。
- 验证方式：`cargo test -p im-attachment attachment -- --nocapture`、`cargo test -p im-types attachment -- --nocapture`、`cargo check -p message-service`。

### Step 02：服务端 E2EE Access Grant

- 小 Plan：[steps/02-service-e2ee-access-grants.md](steps/02-service-e2ee-access-grants.md)
- 目标：direct/group E2EE 消息 accepted 后基于非秘密 grant refs 创建 Access Grant。
- 验证方式：`cargo test -p im-direct direct_e2ee attachment -- --nocapture`、`cargo test -p im-group group_e2ee attachment -- --nocapture`。

### Step 03：im-core 对象加解密与 manifest 模型

- 小 Plan：[steps/03-im-core-object-crypto-manifest.md](steps/03-im-core-object-crypto-manifest.md)
- 目标：客户端生成 P7 object-e2ee 密文对象和完整/redacted manifest。
- 验证方式：`cargo test -p im-core attachment_object_crypto --locked`、`cargo test -p im-core attachment_manifest --locked`。

### Step 04：im-core secure attachment send

- 小 Plan：[steps/04-im-core-secure-attachment-send.md](steps/04-im-core-secure-attachment-send.md)
- 目标：高层 `MessageBody::Attachment + E2eeRequired` 可发送 direct/group E2EE 附件。
- 验证方式：`cargo test -p im-core secure_attachment_send --locked`、`cargo test -p im-core secure --locked`、`cargo test -p im-core e2ee --locked`。

### Step 05：im-core 下载校验与解密

- 小 Plan：[steps/05-im-core-download-decrypt.md](steps/05-im-core-download-decrypt.md)
- 目标：下载 E2EE 附件时自动 ticket、校验、解密和明文输出。
- 验证方式：`cargo test -p im-core attachment_download --locked`、`cargo test -p im-core decrypt --locked`。

### Step 06：CLI、Dart、data-rs2 高层接口

- 小 Plan：[steps/06-cli-dart-data-interfaces.md](steps/06-cli-dart-data-interfaces.md)
- 目标：CLI 和 Dart/data 绑定使用高层接口，不暴露密钥。
- 验证方式：`cargo test -p awiki-cli --locked msg_attachment`、`cargo test -p awiki-cli --locked msg_secure`、`bash scripts/flutter/codegen-check.sh`。

### Step 07：系统测试、文档与集成收口

- 小 Plan：[steps/07-system-tests-docs-integration.md](steps/07-system-tests-docs-integration.md)
- 目标：补系统测试、同步文档、完成最终 Review 和 remote 验证。
- 验证方式：`awiki-system-test` focused suites 和最终 remote suite。

## 10. Review 策略

- 每步骤 Review：优先找安全边界、密钥泄漏、授权绕过、兼容性破坏、缺失测试和文档漂移。
- 全局 Review：检查服务端、端侧、CLI、Dart/data、system-test 是否围绕同一合同闭环。
- 契约 / 安全 / 隐私 Review：确认 key/nonce/明文只在客户端内存和 E2EE 内层 manifest，服务端不持有。
- 文档 Review：确认 API docs、architecture docs、Harness feature map 如行为变化已同步。

## 11. 验证策略

| 层级 | 命令 / 检查 | 预期证据 |
|---|---|---|
| Unit | `cd message-service && cargo test -p im-attachment attachment -- --nocapture` | object-e2ee 控制面和 ticket 策略通过。 |
| Unit | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core attachment --locked` | object crypto、manifest、download 通过。 |
| Unit | `cd e2ee-attachment-cli-rs2 && cargo test -p im-core secure --locked && cargo test -p im-core e2ee --locked` | secure 现有回归通过。 |
| CLI | `cd e2ee-attachment-cli-rs2 && cargo test -p awiki-cli --locked msg_attachment && cargo test -p awiki-cli --locked msg_secure` | CLI parser/adapter/output 脱敏通过。 |
| Dart | `cd e2ee-attachment-cli-rs2 && bash scripts/flutter/codegen-check.sh` | Dart/FRB 绑定和生成文件一致。 |
| Workspace | `cd message-service && cargo test --workspace`、`cd e2ee-attachment-cli-rs2 && cargo test --workspace --locked` | 记录通过或非本任务失败。 |
| System / E2E | `cd awiki-system-test && AWIKI_SYSTEM_TEST_MODE=remote E2E_DID_DOMAIN=awiki.info AWIKI_CLI_RUST_REPO=../e2ee-attachment-cli-rs2 CARGO_BUILD_JOBS=1 uv run --no-sync awiki-system-test` | 记录 passed/failed/skipped、失败原因、关键环境。 |
| Docs | `git diff --check`、路径/链接存在检查 | 文档和 Markdown 链接无明显错误。 |

## 12. 文档更新

- Harness 文档：若公开能力、测试入口或跨仓库 feature 状态变化，更新 `awiki-harness/features/direct-e2ee.md`、`awiki-harness/features/group-e2ee.md` 或 repo profile。
- 子仓库文档：更新 `message-service/docs/api/ANP-client-server-api-attachment.md`、`message-service/docs/architecture/*e2ee*`、`e2ee-attachment-cli-rs2/docs/api/im-core-public-api.md`、`docs/flutter-sdk/*`、CLI docs。
- 本次生成的任务文档：`e2ee-attachment-cli-rs2/docs/e2ee-attachment/*`。

## 13. Commit 计划

- 每个完成、验证、Review 通过的步骤创建一个聚焦 commit。
- Commit 前记录 `git status` 和纳入文件。
- Commit 后记录 commit hash 和工作区状态。
- 跨仓库步骤在各仓库分别创建聚焦 commit，不把 `message-service` 和 `e2ee-attachment-cli-rs2` 修改混到同一个仓库 commit。
- 只有最终集成确实修改文件时才创建最终集成 commit。

## 14. Blocked 处理

| Blocker | Step | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|---|
| `data-rs2` 未检出 | 06 | 当前工作区未找到 `data-rs2` 路径 | 先按 Dart / high-level data facade 设计 | Step 06 | 执行时先定位仓库；找不到则记录为外部待办，不阻塞 CLI/Dart |
| group 跨域 sender-home acceptance 链路不明确 | 02、07 | P7 不标准化 Access Grant 同步协议 | 采用 sender-home 本地 grant refs，要求发送路径经过 sender-home | group 跨域 E2E | 系统测试覆盖 AWiki 实际拓扑；若发现直连远端 group host，先更新 Plan |

- 只有依赖允许且风险已记录时，才继续另一个 pending 步骤。
- 只有没有安全假设、回退方案或独立下一步时，才询问用户。

## 15. Plan 变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需要 Review |
|---|---|---|---|---|
| 2026-06-02 | 初始计划 | 用户要求设计 E2EE 附件传输方案并放入指定目录 | 全部 | 是 |

## 16. 风险与回滚

| 风险 | 缓解措施 | 回滚 / 回退方案 |
|---|---|---|
| 对象密钥泄漏到 public DTO 或日志 | redacted DTO、secret grep、Review gate | 回滚 DTO/输出变更，保留 internal-only 字段 |
| 服务端 grant refs 被滥用 | 校验 object owner、committed、digest、size、mode、message context | 禁用 E2EE grant collector，仅 plain attachment 可下载 |
| 破坏 plain attachment | 保留 plain 分支和现有测试 | 回滚到 `transport-protected + none` 路径 |
| group removed member 仍拿到新 ticket | ticket 时检查当前 group membership | 回滚 group-e2ee attachment ticket 支持，保留 direct |
| object-e2ee 大文件内存压力 | 首期加上明确大小限制和 temp 策略 | 限制 E2EE 附件大小，plain streaming 保持 |

## 17. 最终全局 Review 与整体验证

- 触发条件：所有步骤完成、Review、验证并提交后执行。
- Review 范围：`message-service`、`e2ee-attachment-cli-rs2`、`awiki-system-test`、相关 docs、公开 DTO、CLI 输出、执行台账。
- 重点关注：跨步骤一致性、plain 回归、ticket 绑定、grant refs 安全、Dart/data redaction、系统测试证据、未提交变更。
- 整体验证命令 / 检查：按第 11 节执行，至少包含两个实现仓库 workspace tests 和 `awiki-system-test` remote/focused E2E。
- Review 发现：执行后回填。
- 已修复问题：执行后回填。
- 剩余风险：执行后回填。
- 最终证据：执行后回填。
- 最终 `git status`：执行后回填。
- 如果本阶段修改文件：记录 Review、验证和最终集成 commit。
