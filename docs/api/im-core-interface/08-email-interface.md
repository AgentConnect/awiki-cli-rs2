# 08. Email Interface

本文定义 Email / Mail 能力迁入 SDK 后的 public API 形态。它不属于 Phase 1 IM MVP；独立 Email 阶段打开默认命令面后，CLI `mail.*` 必须通过 SDK 执行，不回退到 legacy mail implementation。

## 1. 结论

Email 是独立产品域，但它复用与 IM 相同的身份、auth session、本地状态和 realtime notification 投递链路。因此推荐把 Email 作为 `im-core` 的高层 service 引入，而不是保留 `awiki-cli` legacy mail fallback，也不是第一步拆出独立 `mail-core`。

推荐入口：

```rust
impl ImClient {
    pub fn email(&self) -> crate::email::EmailService<'_>;
}
```

阶段规则：

```text
1. Phase 1 / IM MVP baseline：`mail.*` 不属于 IM MVP，可保持 unsupported。
2. Email E1+：加入 `client.email()` 和 typed Email DTO。
3. Email E6+：CLI `mail.*` 从 unsupported 改为调用 SDK，默认 dispatch 不进入 legacy `crate::mail`。
4. Email E7+：Dart bridge 和 `packages/awiki_im_core` package facade 同步暴露 Email。
5. SDK public API 不暴露 CLI `mail::SendRequest`、RPC params、raw JSON payload、SQLite connection 或输出文件路径。
```

## 2. Config

`ImCoreConfig` 需要增加 mail 服务 endpoint。它是环境配置，不进入业务 request。

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCoreConfig {
    pub service_base_url: ServiceEndpoint,
    pub did_domain: String,
    pub user_service_endpoint: Option<ServiceEndpoint>,
    pub message_service_endpoint: Option<ServiceEndpoint>,
    pub mail_service_endpoint: Option<ServiceEndpoint>,
    pub transport_policy: MessageTransportPolicy,
}
```

解析规则：

```text
1. CLI adapter 从 `Resolved.mail_service_url` 填充 `mail_service_endpoint`。
2. 若 `mail_service_url` 未配置，可按 CLI 当前规则 fallback 到 `service_base_url`。
3. SDK 内部调用 `/mail/rpc` 时使用 `mail_service_endpoint.unwrap_or(service_base_url)`。
4. live / system test 使用 awiki.ai 平台域名：
   - `service_base_url = https://awiki.ai`
   - `did_domain = awiki.ai`
   - `anp_service_endpoint = https://awiki.ai/anp-im/rpc`
   - `anp_service_did = did:wba:awiki.ai`
   - `mail_service_url = https://mail.awiki.ai`，除非线上部署明确要求复用 `https://awiki.ai`
```

## 3. IDs

`email/dto.rs`：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailMessageId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailFolder(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailAddress(String);

impl EmailMessageId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

impl EmailFolder {
    pub fn inbox() -> Self;
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}

impl EmailAddress {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self>;
    pub fn as_str(&self) -> &str;
}
```

P1 Email 可以只做非空和基础 `@` 校验；不要在第一版引入复杂 RFC mailbox parser，除非 mail-service 已经要求完全兼容。

## 4. Service

`email/service.rs`：

```rust
pub struct EmailService<'a> {
    pub(crate) client: &'a crate::core::ImClient,
}

impl EmailService<'_> {
    pub fn account(&self) -> crate::ImResult<EmailAccount>;

    pub fn inbox(
        &self,
        query: EmailInboxQuery,
    ) -> crate::ImResult<crate::ids::Page<EmailMessageSummary>>;

    pub fn read(&self, id: EmailMessageId) -> crate::ImResult<EmailMessage>;

    pub fn mark_read(
        &self,
        request: EmailMarkReadRequest,
    ) -> crate::ImResult<EmailMarkReadResult>;

    pub fn send(&self, request: SendEmailRequest) -> crate::ImResult<SendEmailResult>;

    pub fn send_with_attachments(
        &self,
        request: SendEmailWithAttachmentsRequest,
    ) -> crate::ImResult<SendEmailResult>;

    pub fn download_attachment(
        &self,
        request: EmailAttachmentDownloadRequest,
    ) -> crate::ImResult<EmailAttachmentContent>;

    pub fn notifications(
        &self,
        query: EmailNotificationQuery,
    ) -> crate::ImResult<crate::ids::Page<EmailNotification>>;
}
```

Blocking-first 规则与 IM Phase 1 一致。Dart / Flutter facade 暴露 `Future` API，由 bridge 层调用 Rust blocking service。

## 5. Request DTOs

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailInboxQuery {
    pub folder: EmailFolder,
    pub limit: crate::ids::PageLimit,
    pub offset: u32,
    pub unread_only: bool,
}

impl Default for EmailInboxQuery {
    fn default() -> Self {
        Self {
            folder: EmailFolder::inbox(),
            limit: crate::ids::PageLimit(20),
            offset: 0,
            unread_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMarkReadRequest {
    pub message_ids: Vec<EmailMessageId>,
    pub is_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentInput {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmailRequest {
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmailWithAttachmentsRequest {
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<EmailAttachmentInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentDownloadRequest {
    pub message_id: EmailMessageId,
    pub attachment_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailNotificationQuery {
    pub limit: crate::ids::PageLimit,
}
```

不进入 SDK request：

```text
identity_name
mail_service_url
output file path
dry-run flag
CLI format / jq
raw JSON-RPC method name
```

## 6. Response DTOs

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAccount {
    pub mailbox_address: Option<EmailAddress>,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessageSummary {
    pub id: EmailMessageId,
    pub folder: Option<EmailFolder>,
    pub from: Vec<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub preview: Option<String>,
    pub received_at: Option<String>,
    pub sent_at: Option<String>,
    pub unread: bool,
    pub has_attachments: bool,
    pub attachment_count: Option<u32>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessage {
    pub summary: EmailMessageSummary,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<EmailAttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentMetadata {
    pub index: u32,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentContent {
    pub message_id: EmailMessageId,
    pub attachment_index: u32,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMarkReadResult {
    pub updated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmailResult {
    pub accepted: bool,
    pub message_id: Option<EmailMessageId>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailNotification {
    pub id: crate::ids::MessageId,
    pub mailbox_address: Option<EmailAddress>,
    pub from_addr: Option<String>,
    pub subject: String,
    pub preview: Option<String>,
    pub has_attachments: bool,
    pub received_at: Option<String>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttribute {
    pub key: String,
    pub value: String,
}
```

说明：

```text
1. SDK 默认返回 typed DTO，不返回完整 `serde_json::Value`。
2. 未能稳定归一化的 mail-service 字段放入 `attributes`，不能把完整 raw payload 塞进去。
3. Attachment 内容在 SDK 中是 bytes；CLI 负责根据 `--output` 写文件和设置权限。
4. `mail notify` 是本地 notification 视图，不调用远端 mail-service。
5. 发送附件 bytes 只在内部 wire 编码为 canonical standard Base64；下载必须严格解码并核对服务声明 size。
```

## 7. Internal Wire Contract

Email 第一版必须兼容当前 Rust 和 Go 实现的 JSON-RPC contract。

| SDK API | Endpoint | RPC method | Profile | Params |
| --- | --- | --- | --- | --- |
| `email().inbox()` | `/mail/rpc` | `mail.getInbox` | read-heavy | `folder`, `limit`, `offset`, `unread_only` |
| `email().read()` | `/mail/rpc` | `mail.getMessage` | read-heavy | `message_id` |
| `email().mark_read()` | `/mail/rpc` | `mail.markRead` | default | `message_ids`, `is_read` |
| `email().account()` | `/mail/rpc` | `mail.getMailbox` | default | `{}` |
| `email().send()` | `/mail/rpc` | `mail.send` | default | `to`, `cc`, `subject`, `body_text`, `body_html` |
| `email().send_with_attachments()` | `/mail/rpc` | `mail.send` | default | `to`, `cc`, `subject`, `body_text`, `body_html`, `attachments[]` |
| `email().download_attachment()` | `/mail/rpc` | `mail.getAttachment` | read-heavy | `message_id`, `attachment_index` |
| `email().notifications()` | local sqlite | none | local | owner identity + limit |

当前 legacy 行为需要保留：

```text
folder 为空 -> inbox
limit <= 0 -> 20
body_html 为空 -> JSON null
attachment_index < 0 在 CLI adapter 层拒绝；SDK public DTO 使用 u32
message_ids 为空 -> InvalidInput(field = "message_ids")
to 为空 -> InvalidInput(field = "to")
subject 为空 -> InvalidInput(field = "subject")
body_text 为空 -> InvalidInput(field = "body_text")
legacy `SendEmailRequest` / `email().send()` -> 保持旧 params，不发送 `attachments` 字段
`SendEmailWithAttachmentsRequest` 的零附件 -> 不发送 `attachments` 字段
非空附件 -> `filename`, `content_type`, canonical standard `content_base64`
发送成功 -> 仅接受 `accepted=true` 且 `status=sent`
下载成功 -> `index` 与请求一致、Base64 canonical、解码长度等于 `size`
```

## 8. Auth And Identity

Email 通过 `ImClient` 注入身份，不在 request 中传 `identity_name`。

SDK 内部要求：

```text
1. 当前身份必须 ready_for_messaging。
2. 使用与 IM 相同的 DID auth / JWT session。
3. Session scope 必须记住：
   - `service_base_url`
   - `${service_base_url}/user-service/did-auth/rpc`
   - `mail_service_endpoint`
4. 如果本地 JWT 为空，应通过 did-auth endpoint refresh 一次，并持久化更新。
5. 401 / session expired 可按现有 auth service 规则 refresh once and retry。
```

错误映射建议：

| 情况 | `ImError` |
| --- | --- |
| 无可用身份 | `IdentityRequired` / `DefaultIdentityMissing` |
| 身份未注册 user / handle | `IdentityNotReady` |
| 无 JWT 且 DID auth refresh 失败 | `AuthRequired` 或 `SessionExpired` |
| mail-service HTTP/RPC 400 | `InvalidInput` 或 `Service` |
| mail-service HTTP/RPC 401 | `AuthRequired` |
| mail-service HTTP/RPC 404 | `MessageNotFound` 或 `Service` |
| mail-service 不可达 | `TransportUnavailable` |

## 9. CLI Adapter Boundary

在 `crates/awiki-cli/src/im_core_adapter/` 增加：

```text
email.rs
```

Adapter 负责：

```text
mail inbox flags -> EmailInboxQuery
mail read --id -> EmailMessageId
mail mark-read args -> EmailMarkReadRequest
mail send flags -> SendEmailRequest
mail attachment download flags -> EmailAttachmentDownloadRequest
mail notify flags -> EmailNotificationQuery
Email DTO -> CLI output data / summary
EmailAttachmentContent -> output file write with CLI permissions
```

Adapter 不负责：

```text
拼 `/mail/rpc`
创建 auth session
读取 sqlite connection
手工拼 owner DID
调用 legacy `crate::mail` service
```

## 10. Dart / Flutter Facade Boundary

`crates/im-core-dart` 需要暴露 Email bridge API：

```text
account(client)
inbox(client, folder, limit, offset, unread_only)
read(client, message_id)
mark_read(client, message_ids, is_read)
send(client, DartSendEmailRequest)
download_attachment(client, message_id, attachment_index)
notifications(client, limit)
```

`packages/awiki_im_core` 需要暴露：

```dart
AwikiImClient.email
EmailApi.account()
EmailApi.inbox(...)
EmailApi.read(...)
EmailApi.markRead(...)
EmailApi.send(...)
EmailApi.downloadAttachment(...)
EmailApi.notifications(...)
```

Dart public model 规则：

```text
1. package facade 导出 `src/models/email.dart`，不导出 generated/FRB DTO 作为 public API。
2. Dart Email request/model 不含 identity_name、dry-run、output path、raw JSON-RPC method。
3. `AwikiImCoreConfig.mailServiceEndpoint` 映射到 `ImCoreConfig.mail_service_endpoint`。
4. `EmailAttachmentContent.bytes` 已是 bytes/list<int>，App 不再处理 base64。
5. Dart facade 与 Rust SDK 保持同一业务语义；不得重新拼 `/mail/rpc`。
```

## 11. Out Of Scope

第一轮 Email 迁移不做：

```text
SMTP / IMAP / POP account setup
folder create/delete/rename
search
reply / reply-all / forward
delete / archive / move / star
draft
server-side filters
attachment preview / forward / inline open
rich MIME parser
mail notification host-notify prompt 重写
```

这些能力需要 mail-service API 明确后再扩展接口。
