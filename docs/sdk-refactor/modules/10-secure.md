# secure 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P6  
**职责**：direct E2EE、group E2EE、secure outbox 和修复流程。

## 1. 目标

`secure` 负责 IM secure 业务编排，包括 direct E2EE 和 group E2EE。P1 不实现 secure public flow。`MessageSecurityMode::SecureDirect` / `GroupE2ee` 在 P1 返回 `UnsupportedCapability`。

secure 的目标不是让调用方处理 ciphertext、prekey、MLS KeyPackage，而是让普通发送仍然通过：

```rust
client.messages().send(SendMessageRequest {
    security: MessageSecurityMode::SecureDirect,
    ..
})
```

## 2. Direct secure 职责

- `direct_status(peer)`。
- `init_direct_session(peer)`。
- `repair_direct_session(peer)`。
- `prepare_direct_session(peer)`：供 `messages().send(... SecureDirect ...)` 内部使用。
- `list_failed_outbox()`。
- `retry_outbox(outbox_id)`。
- `drop_outbox(outbox_id)`。
- `flush_outbox(peer)`。
- `sync_unread_secure_inbox()`。

## 3. Group E2EE 职责

- `group_status(group)`。
- `repair_group_state(group)`。
- group message 解密和 incoming secure processing 作为 messages/realtime 内部步骤。
- KeyPackage、pending notices、member recovery/update/rejoin 等可作为 diagnostic/advanced feature。

## 4. 接口草案

```rust
pub struct SecureDiagnosticsService<'a> {
    client: &'a ImClient,
}

impl SecureDiagnosticsService<'_> {
    pub fn direct_status(&self, peer: PeerRef) -> ImResult<DirectSecureStatus>;
    pub fn init_direct_session(&self, peer: PeerRef) -> ImResult<DirectSessionStatus>;
    pub fn repair_direct_session(&self, peer: PeerRef) -> ImResult<DirectSessionStatus>;
    pub fn list_failed_outbox(&self) -> ImResult<Vec<SecureOutboxEntry>>;
    pub fn retry_outbox(&self, outbox_id: SecureOutboxId) -> ImResult<SecureOutboxResult>;

    pub fn group_status(&self, group: GroupRef) -> ImResult<GroupE2eeStatus>;
    pub fn repair_group_state(&self, group: GroupRef) -> ImResult<GroupE2eeStatus>;
}
```

## 5. 不作为默认 public API

以下内容不进入默认 public API：

```rust
send_direct_cipher(...)
process_ciphertext(...)
publish_key_package(...)
process_mls_notice(...)
build_secure_init_payload(...)
build_group_e2ee_add_rpc_params(...)
```

这些只应在 internal、diagnostic feature 或 CLI-only module 中出现。

## 6. 路径边界

- 私钥文件、session 文件、OPK 文件、MLS provider binary 路径等由 CLI 或 App 选择并传入。
- `im-core` 不自行推导。
- 文件权限、目录创建、备份、清理策略仍由 CLI 或 App 负责。
- Phase 7 可把这些路径封装为 `SecureSessionStore`、`PrekeyStore`、`SecureOutboxStore`、`CryptoProvider`、`MlsProvider` 等 trait。
