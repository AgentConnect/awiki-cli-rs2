# secure 模块接口设计

**阅读顺序**：10 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：direct E2EE、group E2EE、secure outbox 和修复流程。

## 1. 目标

`secure` 负责 IM secure 业务编排，包括 direct E2EE 和 group E2EE。Phase A 使用调用方传入的私钥路径、session/outbox/prekey 路径和 MLS 状态目录；Phase B 再按需抽象为外部 crypto 能力。

## 2. Direct secure 职责

- `direct_status(peer)`。
- `init_direct_session(peer)`。
- `repair_direct_session(peer)`。
- `process_direct_incoming(message)`。
- `prepare_direct_session(peer)`：供 `client.messages().send(... SecureDirect ...)` 内部使用，必要时也可作为诊断/修复入口。
- `list_failed_outbox()`。
- `retry_outbox(outbox_id)`。
- `drop_outbox(outbox_id)`。
- `flush_outbox(peer)`。
- `sync_unread_secure_inbox()`。

## 3. Group E2EE 职责

- `group_status(group)`。
- `publish_key_package(request)`。
- `pending_notices(group_filter)`。
- `repair_notices(group_filter)`。
- `recover_member(group, member, device)`。
- `update_member_key(group, member, device)`。
- `rejoin_member(group, member, role)`。
- `process_leave_request(group, member, request_id, reason)`。
- `process_group_event(event)`：供 realtime/messages 内部投影和诊断使用。

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
    client: &'a ImClient,
}

impl SecureService<'_> {
    pub async fn direct_status(
        &self,
        peer: PeerRef,
    ) -> ImResult<DirectSecureStatus>;

    pub async fn init_direct_session(
        &self,
        peer: PeerRef,
    ) -> ImResult<DirectSessionStatus>;

    pub async fn repair_direct_session(
        &self,
        peer: PeerRef,
    ) -> ImResult<DirectSessionStatus>;

    pub fn list_failed_outbox(&self) -> ImResult<Vec<SecureOutboxEntry>>;

    pub async fn retry_outbox(
        &self,
        outbox_id: SecureOutboxId,
    ) -> ImResult<SecureOutboxResult>;

    pub async fn group_status(
        &self,
        group: GroupRef,
    ) -> ImResult<GroupE2eeStatus>;

    pub async fn publish_key_package(
        &self,
        request: PublishKeyPackageRequest,
    ) -> ImResult<PublishKeyPackageResult>;

    pub async fn repair_group_state(
        &self,
        group: GroupRef,
    ) -> ImResult<GroupE2eeStatus>;
}
```

公开接口挂在 `ImClient` 上，自动使用身份绑定的 E2EE 私钥、direct session、prekey、secure outbox 和 MLS state。`SecureStatePaths`、`IdentityPaths` 和 MLS provider binary path 不应出现在 App/CLI 主业务调用参数中。

普通 secure 消息发送不建议暴露为 `send_direct_cipher(peer, plaintext)` 这类低层 API。App/CLI 应调用 `client.messages().send(SendMessageRequest { security: MessageSecurityMode::SecureDirect, ... })`，由 messages 模块编排 discovery、auth、secure session、outbox、本地投影和远端发送。group message 解密和 incoming secure 处理也应作为 messages/realtime 投影流程的内部步骤，除诊断/修复接口外不让调用方直接传 ciphertext 或 `MessageRecord`。

## 6. 边界说明

- secure 是 IM core 功能，因为它决定消息能否发送、接收、重试、展示。
- 私钥文件、session 文件、OPK 文件、MLS provider binary 路径等由 CLI 或 App 选择并传入；`im-core` 不自行推导。
- 文件权限、目录创建、备份、清理策略仍由 CLI 或 App 负责。
- Phase B 可把这些路径封装为 `SecureSessionStore`、`PrekeyStore`、`SecureOutboxStore`、`CryptoProvider`、`MlsProvider` 等 trait。
