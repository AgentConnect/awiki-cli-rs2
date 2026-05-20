# secure 模块接口设计

**所属 crate**：`crates/im-core`  
**模块职责**：direct E2EE、group E2EE、secure outbox 和修复流程。

## 1. 目标

`secure` 负责 IM secure 业务编排，包括 direct E2EE 和 group E2EE。Phase A 使用调用方传入的私钥路径、session/outbox/prekey 路径和 MLS 状态目录；Phase B 再按需抽象为外部 crypto 能力。

## 2. Direct secure 职责

- `direct_status(actor, peer)`。
- `init_direct_session(actor, peer)`。
- `repair_direct_session(actor, peer)`。
- `process_direct_incoming(actor, message)`。
- `send_direct_cipher(actor, peer, plaintext)`。
- `list_failed_outbox(actor)`。
- `retry_outbox(actor, outbox_id)`。
- `drop_outbox(actor, outbox_id)`。
- `flush_outbox(actor, peer)`。
- `sync_unread_secure_inbox(actor)`。

## 3. Group E2EE 职责

- `group_status(actor, group)`。
- `publish_key_package(actor, request)`。
- `pending_notices(actor, group_filter)`。
- `repair_notices(actor, group_filter)`。
- `recover_member(actor, group, member, device)`。
- `update_member_key(actor, group, member, device)`。
- `rejoin_member(actor, group, member, role)`。
- `process_leave_request(actor, group, member, request_id, reason)`。
- `decrypt_group_message(actor, group, message)`。

## 4. Phase A 路径需求

- `IdentityPaths.e2ee_private_path`：direct secure 和 group E2EE 所需的本地私钥。
- `SecureStatePaths.direct_session_dir`：direct session 文件目录。
- `SecureStatePaths.signed_prekey_dir`：signed prekey 文件目录。
- `SecureStatePaths.one_time_prekey_dir`：one-time prekey 文件目录。
- `SecureStatePaths.secure_outbox_dir`：secure outbox 文件或 SQLite 状态所在目录。
- `SecureStatePaths.mls_state_dir`：group E2EE / MLS 本地状态目录。
- `SecureStatePaths.mls_provider_binary`：如果当前实现仍依赖外部 MLS provider binary，则由 CLI 解析后显式传入。
- `LocalStatePaths.database_file`：需要与消息、群组、本地 outbox 共享状态时使用。

## 5. 接口草案

```rust
pub struct SecureService<'a> {
    core: &'a ImCore,
}

impl SecureService<'_> {
    pub async fn direct_status(
        &self,
        actor: ActorContext,
        peer: PeerRef,
    ) -> ImResult<DirectSecureStatus>;

    pub async fn init_direct_session(
        &self,
        actor: ActorContext,
        peer: PeerRef,
        paths: &SecureStatePaths,
    ) -> ImResult<DirectSessionStatus>;

    pub async fn repair_direct_session(
        &self,
        actor: ActorContext,
        peer: PeerRef,
        paths: &SecureStatePaths,
    ) -> ImResult<DirectSessionStatus>;

    pub async fn send_direct_cipher(
        &self,
        actor: ActorContext,
        peer: PeerRef,
        plaintext: MessageBody,
        paths: &SecureStatePaths,
    ) -> ImResult<SendMessageResult>;

    pub fn list_failed_outbox(
        &self,
        actor: ActorContext,
        paths: &SecureStatePaths,
    ) -> ImResult<Vec<SecureOutboxEntry>>;

    pub async fn retry_outbox(
        &self,
        actor: ActorContext,
        outbox_id: SecureOutboxId,
        paths: &SecureStatePaths,
    ) -> ImResult<SecureOutboxResult>;

    pub async fn group_status(
        &self,
        actor: ActorContext,
        group: GroupRef,
    ) -> ImResult<GroupE2eeStatus>;

    pub async fn publish_key_package(
        &self,
        actor: ActorContext,
        request: PublishKeyPackageRequest,
        paths: &SecureStatePaths,
    ) -> ImResult<PublishKeyPackageResult>;

    pub async fn decrypt_group_message(
        &self,
        actor: ActorContext,
        group: GroupRef,
        message: MessageRecord,
        paths: &SecureStatePaths,
    ) -> ImResult<MessageBody>;
}
```

## 6. 边界说明

- secure 是 IM core 功能，因为它决定消息能否发送、接收、重试、展示。
- 私钥文件、session 文件、OPK 文件、MLS provider binary 路径等由 CLI 或 App 选择并传入；`im-core` 不自行推导。
- 文件权限、目录创建、备份、清理策略仍由 CLI 或 App 负责。
- Phase B 可把这些路径封装为 `SecureSessionStore`、`PrekeyStore`、`SecureOutboxStore`、`CryptoProvider`、`MlsProvider` 等 trait。
