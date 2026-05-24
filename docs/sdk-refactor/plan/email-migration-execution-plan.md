# Email 迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`
**适用阶段**：当前 IM cutover 之后的独立 Email 迁移阶段
**接口规格**：`docs/sdk-refactor/Interface/08-email-interface.md`
**目标**：在不恢复 legacy `mail` 默认 fallback 的前提下，把 Email 远端 RPC、本地通知视图和 CLI `mail.*` 命令迁入 SDK 边界。

---

## 0. 约束

执行本计划前先阅读并遵守：

```text
docs/sdk-refactor/implementation-playbook.md
docs/sdk-refactor/README.md
docs/sdk-refactor/architecture.md
docs/sdk-refactor/public-api.md
docs/sdk-refactor/im-core-cli-boundary.md
docs/sdk-refactor/Interface/README.md
docs/sdk-refactor/Interface/01-crate-layout.md
docs/sdk-refactor/Interface/02-core-interface.md
docs/sdk-refactor/Interface/03-identity-auth-interface.md
docs/sdk-refactor/Interface/05-cli-adapter-interface.md
docs/sdk-refactor/Interface/06-implementation-map.md
docs/sdk-refactor/Interface/08-email-interface.md
docs/sdk-refactor/modules/04-local-state.md
docs/sdk-refactor/modules/11-realtime.md
docs/sdk-refactor/plan/cli-im-core-cutover-plan.md
docs/architecture/awiki-mail-cli.md
docs/architecture/mail-notification-unification-plan.md
docs/architecture/mail-notification-compatibility-checklist.md
docs/architecture/mail-notification-validation-runbook.md
```

依赖文档优先级：

```text
1. Email public API、DTO、config、wire contract 和 CLI/SDK 边界以 `docs/sdk-refactor/Interface/08-email-interface.md` 为准。
2. 迁移执行顺序、PR 切片和验证清单以本文为准。
3. 如果本文和 Interface/08 在 public/internal 边界上冲突，先修正文档，不允许按临时实现扩大 SDK public API。
```

IM cutover baseline 文档曾明确 `mail.*` 默认 unsupported。Email 迁移完成并打开默认命令面后，不允许为了兼容旧 CLI 把 `mail.*` 默认 dispatch 回 `crates/awiki-cli/src/mail/*`。

系统测试要求：

```text
Email 系统测试统一使用 awiki.ai 域名。
默认配置：
  service_base_url=https://awiki.ai
  did_domain=awiki.ai
  anp_service_endpoint=https://awiki.ai/anp-im/rpc
  anp_service_did=did:wba:awiki.ai
  mail_service_url=https://mail.awiki.ai
如果线上 mail-service 明确复用主域，再把 mail_service_url 调整为 https://awiki.ai，并在验证记录中写明原因。
```

---

## 1. 总体结论

Email 迁移应作为 IM cutover 之后的独立阶段处理，推荐阶段名：

```text
Email E1：接口和 crate 边界
Email E2：wire builder / normalization 迁入 im-core
Email E3：远端 account/inbox/read/mark-read/send/download 真迁移
Email E4：本地 mail notification 视图迁入 im-core local_state
Email E5：CLI adapter 与 dry-run/render/file-write 切换
Email E6：CLI cutover classifier 打开 mail 默认命令面
Email E7：Dart bridge / package facade
Email E8：awiki.ai 系统测试与 legacy 清理
```

迁移原则沿用 SDK refactor 主线：

```text
主策略：leaf-file / 小子模块级迁移
辅策略：2-5 个强相关文件组成一个垂直业务切片
禁止：整体搬 `crates/awiki-cli/src/mail` 后直接作为 public API
禁止：让 `im-core` 依赖 `awiki-cli`
禁止：SDK public API 暴露 `identity_name`、`Resolved`、`Manager`、RPC params、raw JSON payload、SQLite connection、output path
```

Email 的推荐位置是 `crates/im-core/src/email`，而不是新建仓库。原因：

```text
1. Email 复用 ImClient 身份绑定、JWT/DID auth、本地 sqlite owner 隔离和 realtime notification projection。
2. CLI 迁移目标仍是 `parse flags -> build ImCore/ImClient -> call SDK -> render`。
3. 单独 `mail-core` 会过早复制 identity/auth/local_state 边界，增加二次同步成本。
4. 后续如果产品边界要求独立发布，可在 im-core API 稳定后再拆 crate。
```

---

## 2. 现有实现盘点

迁移前 Rust legacy Email 实现在：

```text
crates/awiki-cli/src/mail/types.rs
crates/awiki-cli/src/mail/wire.rs
crates/awiki-cli/src/mail/client.rs
crates/awiki-cli/src/mail/service.rs
crates/awiki-cli/src/mail/mod.rs
crates/awiki-cli/src/app/mail_handlers.rs
crates/awiki-cli/tests/mail_wire_contract.rs
crates/awiki-cli/tests/mail_contract.rs
crates/awiki-cli/tests/mail_live_contract.rs
```

Go 参考实现在：

```text
../awiki-cli/internal/mail/types.go
../awiki-cli/internal/mail/client.go
../awiki-cli/internal/mail/service.go
../awiki-cli/internal/cli/mail.go
../awiki-cli/internal/mail/service_test.go
../awiki-cli/internal/cli/mail_test.go
```

迁移前能力：

| CLI 命令 | 现有 RPC / 数据源 | 说明 |
| --- | --- | --- |
| `mail inbox` | `/mail/rpc` `mail.getInbox` | 参数 `folder`, `limit`, `offset`, `unread_only`。 |
| `mail read` | `/mail/rpc` `mail.getMessage` | 参数 `message_id`。 |
| `mail mark-read` | `/mail/rpc` `mail.markRead` | 参数 `message_ids`, `is_read`。 |
| `mail account` | `/mail/rpc` `mail.getMailbox` | 参数 `{}`。 |
| `mail send` | `/mail/rpc` `mail.send` | 参数 `to`, `cc`, `subject`, `body_text`, `body_html`。 |
| `mail attachment download` | `/mail/rpc` `mail.getAttachment` | 参数 `message_id`, `attachment_index`，服务返回 base64。 |
| `mail notify` | local sqlite | 读取 runtime listener 写入的 `source_kind=mail` / `mail.notification` 本地通知。 |

迁移前 Rust cutover baseline：

```text
1. `cmdmeta::cutover_status()` 把 mail capability 标成 unsupported。
2. CLI dispatch 前有 default cutover guard，阻止 `mail.*` 进入 legacy handler。
3. `mail_contract.rs` 和 `mail_live_contract.rs` 验证 mail 命令不会访问 legacy RPC server，也不会写附件输出文件。
4. `mail_wire_contract.rs` 在迁移前保护 legacy wire builder 与 Go contract 一致；E2/E6 后由 `crates/im-core/tests/email_wire_contract.rs` 接管核心断言。
```

---

## 3. 范围

### 3.1 本阶段要做

```text
1. 在 im-core 增加 Email public DTO / service shape。
2. 将 `/mail/rpc` wire builder 和 response normalization 迁入 im-core internal/email_wire。
3. 通过 ImClient runtime 注入身份、auth、owner 和 mail endpoint。
4. 实现 account / inbox / read / mark_read / send / download_attachment。
5. 将 mail notification 本地读取和 normalize 迁入 im-core local_state/email。
6. 增加 CLI im_core_adapter/email.rs。
7. 将 `mail.*` handler 改成 SDK 调用，附件写文件仍留 CLI。
8. 更新 cutover classifier：Email E6 前 unsupported，E6 后 supported/default_surface。
9. 增加 im-core-dart bridge、generated Dart API、package model/facade/export。
10. 增加 awiki.ai mail 系统测试 selector 文档和验证记录。
```

### 3.2 本阶段不做

```text
SMTP / IMAP / POP account setup
folder create/delete/rename
search
reply / reply-all / forward
delete / archive / move / star
draft
server-side filters
mail compose attachment upload
完整 MIME parser
重写 Hermes/OpenClaw prompt
把 mail notification 从 IM websocket projection 中拆出去
```

---

## 4. 目录设计

建议新增：

```text
crates/im-core/src/email/
  mod.rs
  dto.rs
  service.rs

crates/im-core/src/internal/email_wire/
  mod.rs
  account.rs
  inbox.rs
  read.rs
  mark_read.rs
  send.rs
  attachment.rs
  normalize.rs

crates/im-core/src/internal/email_runtime/
  mod.rs
  auth.rs
  remote.rs
  notification.rs

crates/im-core/src/internal/local_state/email.rs

crates/im-core/tests/
  email_wire_contract.rs
  email_notification_contract.rs

crates/awiki-cli/src/im_core_adapter/email.rs
crates/awiki-cli/tests/mail_contract.rs
crates/awiki-cli/tests/mail_live_contract.rs

crates/im-core-dart/src/api/email.rs
crates/im-core-dart/src/dto/email.rs
packages/awiki_im_core/lib/src/generated/api/email.dart
packages/awiki_im_core/lib/src/generated/dto/email.dart
packages/awiki_im_core/lib/src/models/email.dart
```

`lib.rs` 导出规则：

```rust
#[cfg(feature = "email")]
pub mod email;

mod internal;
```

`prelude.rs` 只有在 Email 阶段正式进入默认 API 后才导出 Email 类型。若先放在 non-default feature，则不要让默认 prelude 暴露 `EmailService`。

Cargo feature：

```toml
[features]
default = ["blocking", "sqlite", "http"]
email = []
```

如果产品决定 Email 成为默认 SDK 能力，可在 E6 前单独 PR 把 `email` 纳入 default，并同步更新 public API / README。

---

## 5. PR 顺序

### PR E1：Email interface scaffold

目标：

```text
1. 按 Interface/08 增加 `im_core::email` DTO 和 `EmailService` skeleton。
2. `ImCoreConfig` 增加 `mail_service_endpoint: Option<ServiceEndpoint>`。
3. CLI config adapter 填充 mail endpoint，但 CLI mail 命令仍被 cutover guard 拦截。
4. `client.email()` 若未启用 feature，保持不可见；若启用但未实现，返回 UnsupportedCapability。
```

验证：

```bash
cargo test -p im-core --locked
cargo test -p awiki-cli --test mail_contract --locked
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

### PR E2：Email wire builder 迁入 im-core

迁移来源：

```text
crates/awiki-cli/src/mail/wire.rs
../awiki-cli/internal/mail/service.go
```

目标：

```text
1. 在 `internal/email_wire` 建立 JSON-RPC builder。
2. 保持 `/mail/rpc` method/profile/params 与 Go/Rust legacy 一致。
3. 将 `mail_wire_contract.rs` 的核心断言复制到 `crates/im-core/tests/email_wire_contract.rs`。
4. awiki-cli legacy `mail::wire` 可以临时 wrapper 到 im-core compat，也可以保留原状；但 im-core 不反向依赖 CLI。
```

必须覆盖：

```text
mail.getInbox default folder/limit
mail.getMessage message_id required
mail.markRead message_ids required
mail.getMailbox empty params
mail.send to/subject/body_text required, body_html empty -> null
mail.getAttachment message_id required, negative index CLI 层拒绝，internal 可保留 i64 validator
ServiceError HTTP/RPC mapping 到 ImError
```

### PR E3：Remote Email service 真迁移

目标：

```text
1. 实现 `EmailService::account/inbox/read/mark_read/send/download_attachment`。
2. 使用 ImClient runtime 注入身份和 auth，不在 request 中传 identity_name。
3. 从 `ImCoreConfig.mail_service_endpoint` 选择 mail base URL。
4. 复用 auth ensure / refresh once and retry 规则。
5. 将远端 raw result normalize 为 Email DTO。
```

验收重点：

```text
ready_for_messaging=false -> IdentityNotReady
mail_service_endpoint 空且 service_base_url 空 -> TransportUnavailable / InvalidInput
401 -> refresh once and retry
404 read/download -> MessageNotFound 或稳定 Service mapping
attachment content_base64 decode 在 SDK 完成，返回 bytes
```

不允许：

```text
Email public DTO 带 `serde_json::Value raw`
业务 request 带 `identity_name`
SDK 写 output file path
```

### PR E4：Mail notification 本地视图迁移

迁移来源：

```text
crates/awiki-cli/src/mail/service.rs normalize_notification_*
crates/awiki-cli/src/store/query.rs
crates/awiki-cli/src/store/messages.rs local_mail_notification_predicate
docs/architecture/mail-notification-unification-plan.md
```

目标：

```text
1. 在 im-core local_state 中提供 owner-scoped mail notification 查询。
2. 同时识别：
   - `content_type = "mail.notification"`
   - `metadata.source_kind = "mail"`
3. normalize 为 `EmailNotification`。
4. 保持 `mail notify` 不访问远端 mail-service。
```

验收重点：

```text
legacy `mail.notification` 行兼容
current `source_kind=mail` 行兼容
subject 空 -> "(no subject)"
thread_id `mail:<address>` -> 去掉 `mail:` 前缀
has_attachments 支持 bool/number/string
查询必须由 ImClient owner 注入，CLI 不传 owner_did
```

### PR E5：CLI Email adapter

目标：

```text
1. 新增 `crates/awiki-cli/src/im_core_adapter/email.rs`。
2. 解析 `mail.*` flags 到 SDK DTO。
3. 将 SDK Email DTO render 成当前 CLI JSON envelope 兼容形态。
4. `mail attachment download` 只在 CLI 层写文件，权限保持目录 0700 / 文件 0600。
5. dry-run 继续由 CLI 输出 plan，不调用 SDK 或 legacy mail service。
```

保留 CLI 职责：

```text
--to / --cc split
--limit / --offset / --attachment-index 解析
--output 路径处理
base64 不再从 CLI 解码，SDK 已返回 bytes；CLI 只写 bytes
ExitError / hint / pretty/json render
```

### PR E6：打开 CLI mail cutover

目标：

```text
1. `cmdmeta::cutover_status()` 将 `mail.*` 从 unsupported 改成 supported/default_surface。
2. `default_cutover_boundary_error()` 不再拦截 mail。
3. handler 默认路径调用 im_core_adapter/email，不进入 `crate::mail` legacy service。
4. 更新 schema：mail command 的 cutover status 不再是 unsupported。
5. 更新 `mail_contract.rs`：从 unsupported contract 改为 SDK dispatch contract。
```

本 PR 必须确认：

```text
unsupported mail tests 已替换，不是删除覆盖。
legacy RPC server 测试改为断言 SDK mail client 发出预期 request。
attachment 测试改为 SDK 返回 bytes 后 CLI 写文件。
dry-run 仍不访问网络、不写文件。
```

### PR E7：Dart bridge / package facade

目标：

```text
1. 在 `crates/im-core-dart` 暴露 Email API：account/inbox/read/mark_read/send/download_attachment/notifications。
2. 增加 Dart bridge DTO，并把 `mail_service_endpoint` 纳入 Dart config mapping。
3. `SendEmailRequest` 从 Dart 映射到 typed `im_core::email::SendEmailRequest`，不传 raw RPC params。
4. package facade 增加 `AwikiImClient.email` 和 `EmailApi`，并导出 `src/models/email.dart`。
5. 生成并检查 `packages/awiki_im_core/lib/src/generated/api/email.dart` 与 `generated/dto/email.dart`。
```

验收重点：

```text
Dart Email model 不含 CLI-only 字段：identity_name / output path / dry-run / raw JSON-RPC method
EmailAttachmentContent.bytes 在 Dart 侧保持 bytes/list<int>，不要求 App 再解 base64
Flutter package stub test 能构造 Email config/model
```

验证：

```bash
cargo check -p im-core-dart --locked
cargo test -p im-core-dart --test facade_contract --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && /home/ecs-user/develop/flutter/bin/flutter test test/awiki_im_core_stub_test.dart
```

### PR E8：awiki.ai 系统测试与 legacy 清理

目标：

```text
1. 在 awiki-system-test 中启用 Email selector，域名固定 awiki.ai。
2. 记录系统测试验证报告到 `docs/verification/`。
3. 清理或收紧 `crates/awiki-cli/src/mail/*` legacy 模块。
4. 更新 docs/architecture/awiki-mail-cli.md 的实现归属。
```

legacy 清理顺序：

```text
1. 先删除默认 dispatch 对 legacy mail service 的引用。
2. 再将 legacy wire tests 对应断言迁到 im-core。
3. 最后删除或 `pub(crate)` 收紧 `crates/awiki-cli/src/mail/*`。
```

---

## 6. CLI 命令映射

| CLI | SDK API | CLI 保留职责 |
| --- | --- | --- |
| `mail account` | `client.email().account()` | render summary / JSON envelope |
| `mail inbox --folder --limit --offset --unread` | `client.email().inbox(query)` | flag parse, dry-run plan |
| `mail read --id` | `client.email().read(id)` | required flag validation |
| `mail mark-read <ids...>` | `client.email().mark_read(request)` | arg list validation |
| `mail send --to --cc --subject --body --html` | `client.email().send(request)` | address list split, dry-run plan |
| `mail attachment download --message-id --attachment-index --output` | `client.email().download_attachment(request)` | output path, file permissions, render saved path |
| `mail notify --limit` | `client.email().notifications(query)` | dry-run plan, render local notification view |

---

## 7. Tests

### 7.1 Unit / contract tests

推荐 selectors：

```bash
cargo test -p im-core --test email_wire_contract --locked
cargo test -p im-core --test email_notification_contract --locked
cargo test -p awiki-cli --test mail_contract --locked
cargo test -p awiki-cli --test mail_live_contract --locked
cargo test -p awiki-cli --test cli_cutover_command_surface_contract --locked
cargo test -p awiki-cli --test cmdmeta_schema_contract --locked
cargo test -p awiki-cli --test im_core_adapter_policy_contract --locked
```

每个 PR 至少跑：

```bash
cargo test -p im-core --locked
cargo test -p im-core --test email_wire_contract --test email_notification_contract --locked
cargo test -p awiki-cli --test mail_contract --locked
cargo test -p awiki-cli --test mail_live_contract --locked
```

E6 后 legacy `mail_wire_contract` 不再作为 awiki-cli selector 保留；其核心断言必须迁入 `crates/im-core/tests/email_wire_contract.rs`。awiki-cli 侧通过 `mail_contract` 和 `mail_live_contract` 覆盖 SDK dispatch、dry-run、render envelope、附件写文件和无 legacy fallback。

### 7.2 Dart / Flutter tests

推荐 selectors：

```bash
cargo check -p im-core-dart --locked
cargo test -p im-core-dart --test facade_contract --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && /home/ecs-user/develop/flutter/bin/flutter test test/awiki_im_core_stub_test.dart
```

### 7.3 Boundary tests

必须保留：

```bash
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|crate::app|crate::cli|crate::output|awiki_cli" crates/im-core/src crates/im-core/tests
```

必须新增断言：

```text
Email request DTO 不含 identity_name
Email public response 不含 raw serde_json::Value
Email attachment SDK 返回 bytes，CLI 才写文件
mail notify 不访问远端 mail-service
dry-run 不访问网络、不写文件
```

### 7.4 System tests

系统测试只在 E8 执行，使用 awiki.ai：

```bash
cd /home/ecs-user/awiki-space/awiki-system-test
AWIKI_CLI_UNDER_TEST=rust \
AWIKI_CLI_RUST_REPO=/home/ecs-user/awiki-space/mail-cli-rs2 \
AWIKI_ENABLE_MAIL_TESTS=1 \
E2E_MAIL_DOMAIN=awiki.ai \
E2E_MAIL_SERVICE_URL=https://mail.awiki.ai \
PYTHONDONTWRITEBYTECODE=1 \
uv run pytest -p no:cacheprovider -ra -q tests_v2/mail
```

如果系统测试仓库使用不同环境变量名，以 `awiki-system-test` 当前 test fixture 为准；但域名仍必须是 `awiki.ai`，验证记录必须写明实际命令和环境变量。

---

## 8. Acceptance

Email 迁移完成标准：

```text
1. `client.email()` public API 存在，且不暴露 CLI / wire / SQLite 类型。
2. Remote account/inbox/read/mark_read/send/download_attachment 都通过 im-core 执行。
3. `mail notify` 通过 im-core local_state owner-scoped 查询执行。
4. CLI `mail.*` 默认命令面从 unsupported 改为 supported，且不进入 legacy fallback。
5. Dry-run 行为保持无网络、无文件写入。
6. Attachment download 的文件写入仍由 CLI 管理权限和路径。
7. Unit / contract tests 通过。
8. Dart bridge / generated API / package facade 已包含 Email，并通过 codegen check 与 package stub test。
9. awiki.ai Email 系统测试通过，验证记录落到 `docs/verification/`。
10. legacy `crates/awiki-cli/src/mail/*` 已删除或收紧为迁移期不可达代码，并有 TODO/issue 指向最终删除。
```

---

## 9. 风险与回滚

| 风险 | 应对 |
| --- | --- |
| mail-service response 字段不稳定 | 先 normalize 稳定字段，剩余字段进入 `EmailAttribute`，不暴露 raw payload。 |
| mail endpoint 线上域名不一致 | 系统测试固定 awiki.ai，并在验证记录写明 `mail_service_url`。 |
| CLI mail 命令提前打开 | E6 前保留 cutover unsupported contract。 |
| Auth scope 漏掉 mail_service_url | E3 增加测试断言 session scope 包含 mail endpoint。 |
| notification 旧数据丢失 | E4 同时兼容 `mail.notification` 和 `source_kind=mail`。 |
| 附件文件写入进入 SDK | Boundary test 检查 SDK request/response 不含 output path，CLI test 覆盖权限。 |

推荐回滚：

```text
E1-E5 回滚：保持 CLI cutover guard，用户仍看到稳定 unsupported。
E6 回滚：只把 mail cutover classifier 改回 unsupported，不恢复 legacy fallback。
E7 回滚：移除 Dart facade/export，保留 Rust SDK 和 CLI cutover。
E8 回滚：保留 SDK 能力但重新隐藏 CLI default surface，直到 awiki.ai 系统测试恢复稳定。
```
