# Step 02：im-core / awiki-me 恢复与已有身份 daemon subkey migration

主 Plan：[../plan.md](../plan.md)  
Step index：02  
状态：done

## 1. 执行状态

| 字段 | 值 |
|---|---|
| Status | done |
| Branch | `feature/release-0526/agent-im-hutong` / `awiki-cli-rs2`、`awiki-me`、`user-service` 当前分支 |
| Started | 2026-06-10T02:13:41Z |
| Completed | 2026-06-10T02:57:20Z |
| Commit | `awiki-cli-rs2` `4562474` (`im-core: ensure daemon subkey for recovered identities`)；`awiki-me` `6fd1411` (`awiki-me: ensure daemon subkey before bootstrap`)；`user-service` `1cafb30` (`user-service: preserve DID metadata on document update`) |
| Review evidence | 已 Review 主私钥使用边界、daemon package private/public/DID Document authentication 匹配、已有 `#daemon-key-1` 不覆盖、signed update 契约、Dart API 兼容、awiki-me Dart-only 约束、secret 日志/UI 泄露。发现并修复 user-service `update_document` 省略元数据字段会重置旧值的问题，并补上显式 `null` 与字段省略的契约测试。 |
| Verification evidence | `cd awiki-cli-rs2 && cargo fmt --check && cargo test -p im-core --locked && cargo test -p im-core-dart --locked`：通过，`im-core` 270 lib tests 与 integration tests 通过，`im-core-dart` 6 unit + 13 facade tests 通过；`cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh && cd packages/awiki_im_core && flutter test`：codegen stable，12 tests passed；`cd user-service && uv run pytest tests/app/did_auth -v && uv run ruff check src/user_service/app/did_auth tests/app/did_auth`：105 passed, 32 warnings，ruff 通过；`cd awiki-me && flutter analyze && flutter test`：No issues found，272 tests passed；三仓 `git diff --check` 通过；secret 搜索无新增日志/UI 泄露。 |
| Next action | Step 03：awiki-deamon bootstrap private package 早期校验 |

状态取值：`pending`、`in_progress`、`review`、`blocked`、`committed`、`done`。

## 2. 目标

- 结果：新注册、恢复账号、已有本地身份在 APP 发送 bootstrap 前都能获得可用 `DaemonSubkeyPrivatePackage`。
- 用户 / 系统可见行为：用户恢复账号或升级旧版本后，点击 bootstrap message agent 不会因为本地缺少 `daemon-key-1-private.pem` 直接失败；系统会幂等生成/补齐 daemon subkey、更新 signed DID Document、同步 user-service registry，然后再发送 bootstrap。
- 非目标：不在缺少用户主 `#key-1` 私钥时强行修改 DID Document；不把用户主私钥交给 Daemon；不自动创建多个 daemon key；不支持设备化 fragment。
- 完成标准：im-core 提供同步/异步 ensure API；recovery 路径能保存或补齐 package；awiki-me 使用 ensure API；无法安全重签时 fail closed 并给出可诊断错误。
- 本步骤新增前置小修：user-service `did-auth.update_document` 省略 `is_public` / `is_agent` / `role` / `endpoint_url` 时必须保留既有元数据，避免 im-core migration 只更新 DID Document 却把身份可见性或 agent 标记意外重置；显式传入字段的旧行为保持不变。

## 3. 设计方法

- 设计边界：daemon subkey private package 仍由 APP/im-core 本地生成和保存；user-service 只看到 signed DID Document / public registry，不接触 private material。
- 核心决策：新增 `ensure_daemon_subkey_package` 语义：如果本地已有 package 且 DID Document authentication 仍包含对应 public key，直接返回；如果没有 package 但本地有主 DID signing key，则生成 `#daemon-key-1`、插入 DID Document、用 `#key-1` 重签、调用 DID auth signed update、保存 package；如果本地没有主签名能力，返回明确错误。
- 契约 / API / 数据流：`awiki-me` 不再只调用 `loadDaemonSubkeyPackage`；bootstrap 前调用 `ensureDaemonSubkeyPackage`。成功后发送普通消息 JSON bootstrap，失败则提示重新恢复/重新登录。
- 兼容性：保留 `loadDaemonSubkeyPackage` 用于已有新用户；新增 ensure API optional 使用；旧 Flutter/Dart 调用不破坏。
- 迁移策略：本地 identity store 增加 migration：检测 `daemon-subkey-package.json` 或 legacy private PEM；检测 DID Document 中是否已有 `#daemon-key-1`；按状态决定返回、补写 package、重新生成、或要求 signed update。
- 风险控制：生成/重签过程不把主私钥或 daemon private key写入日志；所有错误消息不得包含 key material。

## 4. 实现方法

1. 在 `awiki-cli-rs2/crates/im-core` 增加 identity API：
   - `load_daemon_subkey_package` 保持原行为；
   - 新增 `ensure_daemon_subkey_package(selector)` 和 async 版本；
   - 返回结构使用 Step 04 最终 schema，如果 Step 04 尚未执行，先保持当前 schema 并在 Step 04 migration。
2. 实现 ensure 状态机：
   - `package_present_and_matches_document`：校验 method、public key、private/public match、DID Document authentication；
   - `document_has_daemon_key_but_package_missing`：如果无法恢复 private key，返回 `daemon_subkey_private_missing`；不得生成另一把同 fragment key 覆盖服务端 public key；
   - `document_missing_daemon_key_and_key1_available`：生成 daemon subkey，插入 DID Document，重签，调用 signed update；
   - `key1_missing`：fail closed，提示重新恢复。
3. 修复 recovery 路径：
   - 恢复流程如果生成新的 DID Document，应与新注册路径一致生成 daemon key package；
   - 如果恢复的是服务端已有 DID Document，按 ensure 状态机补齐。
4. 扩展 identity store：
   - 保存 daemon key package；
   - 支持 legacy package 读取；
   - 记录 migration version 或 package schema version。
5. 扩展 im-core-dart / packages binding：
   - 暴露 `ensureDaemonSubkeyPackage`；
   - web stub / unsupported platform API 名称一致；
   - codegen 更新。
6. 修改 awiki-me：
   - `identityCorePort` 增加 ensure 方法；
   - `agents_provider.bootstrapMessageAgent` 先 ensure，再构造 bootstrap；
   - 错误 UI 不显示 secret；区分 retryable network error 和 terminal missing-main-key error。
7. 增加测试：
   - 新注册仍生成 package；
   - recovery 生成 package；
   - 旧 identity 无 package 但有 key1 可补齐并调用 signed update；
   - 旧 identity 有 DID Document daemon key 但无 private package 时 fail closed；
   - awiki-me bootstrap fallback 调 ensure。

## 5. 路径

| 仓库 / 模块 / 文件 | 计划变更 | 备注 |
|---|---|---|
| `awiki-cli-rs2/crates/im-core/src/internal/identity_registration_runtime.rs` | 保持新注册 package 生成，与新 ensure helper 复用 | 避免逻辑重复 |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_recovery_runtime.rs` | 恢复路径生成或 ensure daemon package | 当前缺口 |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_daemon_subkey.rs` | 增加 private/public match、package migration helper | 可与 Step 03 共享 |
| `awiki-cli-rs2/crates/im-core/src/internal/identity_wire/*` | 增加 signed update 调用或复用现有 update_document | 需对齐 Step 01 |
| `awiki-cli-rs2/crates/im-core/src/identity/*` | 暴露 ensure API / DTO | public SDK surface |
| `awiki-cli-rs2/crates/im-core-dart/*` | FFI/Dart binding | 保持 native/web API 一致 |
| `awiki-cli-rs2/packages/awiki_im_core/*` | Dart package API/codegen | 需要 codegen check |
| `user-service/src/user_service/app/did_auth/schemas.py` | `UpdateDocumentRequest` 元数据字段改为 optional | 省略字段保留旧值 |
| `user-service/src/user_service/app/did_auth/service.py` | `update_document` 写库时使用旧值 fallback | 避免 migration 回归 |
| `user-service/tests/app/did_auth/*` | 补省略元数据字段的回归测试 | Step 02 的 user-service 前置小修 |
| `awiki-me/lib/src/application/ports/identity_core_port.dart` | 增加 ensure 方法 | App 抽象层 |
| `awiki-me/lib/src/data/im_core/*` | 调用 binding ensure API | Dart-only |
| `awiki-me/lib/src/presentation/agents/*` | bootstrap 前 ensure | UI/provider |
| `awiki-cli-rs2/crates/im-core/tests/*` | identity migration tests | 覆盖缺口 |
| `awiki-me/test/*` | provider/service tests | 覆盖 bootstrap fallback |

## 6. 依赖

- 前置步骤：Step 01 完成，提供 signed DID Document update 与 registry 同步语义。
- 外部文档或决策：主 Plan 第 4 节关于缺少主 key 时 fail closed 的假设。
- 环境前提：能运行 Rust cargo tests、Dart/Flutter package tests、awiki-me Flutter tests。

## 7. 验收标准

- [x] `im-core` 新增 `ensure_daemon_subkey_package` 同步/异步 API，旧 `load` API 不变。
- [x] `user-service update_document` 省略 `is_public` / `is_agent` / `role` / `endpoint_url` 时保留旧值，显式传入仍按传入值更新；`role` / `endpoint_url` 显式 `null` 可清空。
- [x] 恢复账号路径不再固定保存 `daemon_subkey_package: None`；新恢复身份能 bootstrap。
- [x] 旧身份缺少 package 且本地有主签名 key 时，能生成 daemon key、重签 DID Document、调用 signed update 并保存 package。
- [x] 旧身份 DID Document 已有 daemon public key 但本地无对应 private key 时 fail closed，不覆盖同 fragment public key。
- [x] awiki-me bootstrap 前调用 ensure API；错误不泄露 secret。
- [x] Dart native/web API 名称一致，codegen 更新。
- [x] Review 发现已经修复或明确记录。
- [x] 本步骤在进入下一步之前已经创建聚焦 commit。

## 8. 验证方式

| 检查项 | 命令 / 方法 | 预期证据 |
|---|---|---|
| im-core | `cd awiki-cli-rs2 && cargo test -p im-core --locked` | identity registration/recovery/migration tests 通过。 |
| im-core-dart | `cd awiki-cli-rs2 && cargo test -p im-core-dart --locked` | FFI/facade tests 通过。 |
| Flutter codegen | `cd awiki-cli-rs2 && scripts/flutter/codegen-check.sh` | generated API up to date。 |
| Flutter package | `cd awiki-cli-rs2/packages/awiki_im_core && flutter test` | Dart package tests 通过。 |
| awiki-me | `cd awiki-me && flutter analyze && flutter test` | App provider/bootstrap fallback tests 通过。 |
| Security | `rg -n "BEGIN PRIVATE|private_key|privateKey" awiki-cli-rs2/crates/im-core awiki-me/lib` | 只有合法 secret handling、测试 fixture 或 redaction 代码；无日志/UI 泄露。 |

如果某个命令不能运行，必须记录原因、影响和替代证据。

## 9. Review 环节

- Review 时机：本步骤代码实现完成后、commit 前。
- Review 重点：是否误用用户主私钥；是否可能覆盖已有 daemon public key；signed update 调用是否与 Step 01 契约一致；旧 API 是否兼容；错误消息和日志是否泄露 secret；awiki-me 是否仍符合 Dart-only 约束。
- Review 结论必须在 commit 前记录；必须修复必要问题，或明确记录剩余风险。

| Review 项 | 结果 | 备注 |
|---|---|---|
| 发现问题 | 2 项 | 1. im-core migration 调用 `update_document` 时不传 `is_public` / `is_agent` / `role` / `endpoint_url`，原 user-service schema 会把省略字段落成默认值，可能破坏旧身份元数据。2. 初始修复若只按 `None` fallback，会把显式 `role=null` / `endpoint_url=null` 误当成省略字段，破坏“显式传入仍按传入值更新”的契约。 |
| 已修复问题 | 2 项 | `UpdateDocumentRequest` 的元数据字段改为 optional，并通过 `model_fields_set` 区分省略与显式传入；新增省略字段保留旧值、显式 false/null 覆盖旧值的测试。 |
| 剩余风险 | 已记录 | im-core 在远端 signed update 成功后才保存本地 DID Document 和 daemon package；如果远端成功但本地保存失败，远端 DID Document 会已有 `#daemon-key-1`，本地仍缺 package，下一次 ensure 会 fail closed 为 `daemon_subkey_private_missing`。这是安全优先行为，后续可补恢复/补偿流程。Step 04 仍需修正 `private_key_multibase` 承载 PEM 的 schema 命名。 |
| 新增或缺失测试 | 已新增 | 新增 recovery 持久化 package、旧身份 signed update migration、已有 daemon key 无 private package fail closed、`update_document` 元数据省略/显式覆盖、Dart bootstrap ensure 调用等测试。 |
| 已更新或缺失文档 | 已更新 | 已回填本 Step 文档和主 Plan 台账；user-service API 说明在 schema Field 描述中体现。更完整 API 文档可在 Step 06 文档收口时统一检查。 |

## 10. Commit 要求

- Commit 时机：本步骤实现、验证、Review 都完成后。
- Commit 范围：`awiki-cli-rs2` identity/FFI/package 变更和 `awiki-me` bootstrap fallback 可按仓拆分 commit。
- Commit 前状态：记录相关仓 `git status --short --branch`。
- 纳入文件：记录每个 commit 包含的文件。
- Commit 后证据：记录 commit hash 和 commit 后 `git status`。
- 遗留未提交变更：必须记录原因以及为什么安全。
- 建议消息：`im-core: ensure daemon subkey for recovered identities`、`awiki-me: ensure daemon subkey before bootstrap`

## 11. Blocked 处理

| Blocker | 证据 | 已尝试方案 | 影响范围 | 下一步决策 |
|---|---|---|---|---|
| 缺少 signed DID update API | 已解决 | 复用 Step 01 已存在的 `did-auth.update_document`，im-core 新增 `update_document` RPC builder | - | 已完成 |
| 旧身份缺少主 key | 已处理 | `load_key1_private_pem` 缺失时 fail closed 为 `key1_private` | 当前用户路径 | 用户需重新恢复/重新绑定；不自动修改 DID Document |

## 12. Plan 变更记录

| 日期 | 变更 | 原因 | 主 Plan 变更记录链接 |
|---|---|---|---|
| 2026-06-10 | 创建 Step 02 | 补恢复/已有身份 daemon package 缺口 | `../plan.md#15-plan-变更记录` |

## 13. 风险、回滚与后续文档

- 风险：migration 触发 signed update，可能受网络或 user-service 可用性影响。
- 回滚 / 回退：保留旧 `loadDaemonSubkeyPackage`；ensure 失败时不发送 bootstrap。
- 后续文档：更新 im-core identity API 文档和 awiki-me bootstrap 行为说明。
