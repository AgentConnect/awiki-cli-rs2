# secure 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P6  
**职责**：direct E2EE、group E2EE、secure outbox、附件 E2EE 编排和修复流程。

## 1. 目标

`secure` 负责 IM secure 业务编排。SDK public API 表达的是“这条消息必须端到端加密”和“这个会话/群组的安全状态如何”，而不是让调用方处理 ciphertext、prekey、MLS KeyPackage、MLS notice 或 provider path。

普通发送仍然通过 `messages().send()`：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Text {
        text: "hello".to_string(),
        kind: MessageKind::Text,
    },
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})
```

群组和附件也使用同一套发送入口：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group),
    body: MessageBody::Attachment {
        input: AttachmentInput::LocalFile(path),
        caption: None,
        mime_type: Some("image/png".to_string()),
    },
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})
```

P6 可保留 `MessageSecurityMode::SecureDirect` / `GroupE2ee` 作为迁移期兼容别名，但长期 SDK 语义应收敛到 target-independent 的 `MessageSecurityPolicy::E2eeRequired`。

## 2. Public API 形态

```rust
impl ImClient {
    pub fn secure(&self) -> SecureService<'_>;
}

pub struct SecureService<'a> {
    client: &'a ImClient,
}

impl SecureService<'_> {
    pub fn direct(&self, peer: PeerRef) -> DirectSecureConversation<'_>;
    pub fn group(&self, group: GroupRef) -> GroupSecureConversation<'_>;
    pub fn outbox(&self) -> SecureOutboxService<'_>;
}

pub struct DirectSecureConversation<'a> {
    client: &'a ImClient,
    peer: PeerRef,
}

impl DirectSecureConversation<'_> {
    pub fn status(&self) -> ImResult<DirectSecureStatus>;
    pub fn prepare(&self) -> ImResult<DirectSecurePrepareResult>;
    pub fn repair(&self) -> ImResult<DirectSecureRepairResult>;
}

pub struct GroupSecureConversation<'a> {
    client: &'a ImClient,
    group: GroupRef,
}

impl GroupSecureConversation<'_> {
    pub fn status(&self) -> ImResult<GroupSecureStatus>;
    pub fn prepare(&self) -> ImResult<GroupSecurePrepareResult>;
    pub fn repair(&self) -> ImResult<GroupSecureRepairResult>;
}

pub struct SecureOutboxService<'a> {
    client: &'a ImClient,
}

impl SecureOutboxService<'_> {
    pub fn list_failed(&self) -> ImResult<Vec<SecureOutboxEntry>>;
    pub fn retry(&self, outbox_id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
    pub fn drop(&self, outbox_id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
}
```

`prepare()` 是业务语义：准备和 peer/group 的安全发送能力。底层可以 publish prekey、初始化 direct session、同步 MLS state 或处理 pending notices，但这些都不进入默认 public API。

## 3. 私聊 E2EE 职责

- `messages().send(Direct, E2eeRequired)` 内部执行 direct E2EE send。
- `secure().direct(peer).status()` 返回会话是否 ready / needs repair / waiting for peer。
- `secure().direct(peer).prepare()` 显式准备 direct secure 会话。
- `secure().direct(peer).repair()` 修复 direct secure 会话状态。
- direct incoming decrypt 作为 `messages().inbox()`、`history()`、`realtime()` 的内部 projection。
- secure ACK/init/control payload 和 ratchet/session 细节只在 internal/compat/debug API 中出现。

默认 public DTO 不暴露：

```text
session_id
send_n / recv_n
skipped_key_count
prekey id / signed prekey id
direct control plaintext payload
raw ciphertext
```

## 4. 群组 E2EE 职责

- `groups().create()` / group policy 表达 `GroupSecurityProfile::E2eeRequired`。
- `messages().send(Group, E2eeRequired)` 内部执行 group E2EE send。
- `secure().group(group).status()` 返回 group secure readiness。
- `secure().group(group).prepare()` 同步或准备本地 group secure state。
- `secure().group(group).repair()` 处理修复流程。
- group membership 变更 API 内部处理 MLS update，不要求调用方手动 publish KeyPackage 或 process notice。

默认 public DTO 不暴露：

```text
MLS epoch
KeyPackage
Welcome / Commit / Proposal
pending notice raw payload
provider stdout/stderr
MLS provider binary path
```

## 5. 附件 E2EE 职责

附件 E2EE 不单独暴露“加密附件 API”。调用方通过 `messages().send(... Attachment ..., E2eeRequired)` 表达意图，SDK 内部完成：

```text
1. 生成 attachment content key。
2. 加密文件或 bytes。
3. 上传 encrypted blob。
4. 生成 encrypted attachment manifest。
5. 用 direct/group E2EE 加密 manifest。
6. 发送消息。
```

下载时通过 attachments API 自动解密：

```rust
client.attachments().download(DownloadAttachmentRequest {
    message_id,
    attachment_id,
    destination,
})
```

默认 public DTO 不暴露：

```text
content key
nonce
MAC/tag
wrapped key
encrypted manifest body
ciphertext URL signing details
```

## 6. DTO 原则

public DTO 应表达领域状态：

```rust
pub struct DirectSecureStatus {
    pub peer: PeerRef,
    pub resolved_peer: Option<Did>,
    pub state: DirectSecureState,
    pub can_send_secure: bool,
    pub pending_outbox_count: u32,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub enum DirectSecureState {
    Ready,
    Preparing,
    WaitingForPeer,
    NeedsRepair,
    Unavailable,
    Unknown,
}

pub struct GroupSecureStatus {
    pub group: GroupRef,
    pub profile: GroupSecurityProfile,
    pub state: GroupSecureState,
    pub can_send_secure: bool,
    pub local_readiness: GroupSecureLocalReadiness,
    pub pending_work: GroupSecurePendingWork,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

pub enum GroupSecureState {
    Ready,
    Syncing,
    NeedsRepair,
    WaitingForMembershipUpdate,
    MissingLocalState,
    Unavailable,
    Unknown,
}
```

状态字段使用 enum，不用裸 `String`；身份字段使用 `Did` / `PeerRef` / `GroupRef`，不要用裸 `String`。

## 7. 不作为默认 public API

以下内容不进入默认 public API：

```rust
send_direct_cipher(...)
process_ciphertext(...)
publish_prekey_bundle(...)
publish_key_package(...)
process_mls_notice(...)
build_secure_init_payload(...)
build_group_e2ee_add_rpc_params(...)
raw_ciphertext(...)
```

这些只能存在于 `internal`、`compat`、test helper、advanced diagnostics feature 或 CLI-only adapter 中。

## 8. 路径边界

- direct E2EE session、signed prekey、one-time prekey 等中间状态存入 `im-core` local SQLite / local secure store，并按 `owner_identity_id` 隔离。
- `e2ee_outbox` 使用 `(owner_identity_id, outbox_id)` 作为本地 owner-scoped key；DID recover/replace 只刷新 `owner_did` snapshot，不移动 secure outbox ownership。
- SDK public API 不接收、返回或配置 direct session/prekey 文件路径；也不暴露 `p5-e2ee-sessions`、`p5-signed-prekeys`、`p5-one-time-prekeys` 等 legacy 目录。
- `ImCorePaths.local_state` 提供 SQLite root/path；direct secure runtime 通过 internal local_state repository 读写，不自行拼 CLI `identity_dir`。
- secure outbox 暂时保留现有 SQLite `e2ee_outbox.plaintext` 明文 payload，但 public `SecureOutboxEntry` 只返回摘要，不返回 plaintext。
- 群组 MLS provider state/path selection 必须按 `owner_identity_id + device_id` scoped；是否迁入 SQLite 由后续 group E2EE 长期方案决定。
- DID 私钥文件、MLS provider binary 路径等由 CLI 或 App 通过 `ImCorePaths` / host config 选择并传入。
- `im-core` 不自行发现 workspace。
- 文件权限、目录创建、备份、清理策略仍由 CLI 或 App 负责。
- Phase 7 如需扩展外部 provider，再考虑 non-default 的 `SecureSessionStore`、`PrekeyStore`、`SecureOutboxStore`、`CryptoProvider`、`MlsProvider` trait。

## 9. Discovery 和 diagnostics 门禁

- Direct E2EE public discovery 继续 disabled。默认 DID/service discovery 不 advertise `anp.direct.e2ee.v1` 或 `direct-e2ee`。
- Group E2EE public discovery 继续 disabled。默认 DID/service discovery 不 advertise `anp.group.e2ee.v1` 或 `group-e2ee`。
- CLI/App/Dart public DTO、doctor、日志和文档只能暴露 high-level secure status、problem、repair summary 和计数。
- 不得暴露 private keys、JWT、message plaintext、secure outbox plaintext、raw ciphertext、direct session counters、ratchet keys、KeyPackage、Welcome、Commit、Proposal、raw MLS notice、provider stdout/stderr/path、raw SQLite rows 或 backup contents。
