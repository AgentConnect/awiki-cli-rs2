# 模块设计：advanced-secure（Phase 3+）

## 1. 阶段定位

加密能力不进入第一阶段。第一阶段普通消息只支持 `DefaultPlain` / `Plain`。

如果调用：

```rust
MessageSecurityMode::SecureDirect
MessageSecurityMode::GroupE2ee
```

第一阶段应返回：

```rust
ImError::UnsupportedCapability { capability: "secure-direct" }
```

或：

```rust
ImError::UnsupportedCapability { capability: "group-e2ee" }
```

不要静默降级成明文发送。

## 2. Phase 3 public diagnostics

Phase 3 可以增加：

```rust
impl SecureDiagnosticsService<'_> {
    pub fn direct_status(&self, peer: PeerRef) -> ImResult<DirectSecureStatus>;
    pub fn repair_direct_session(&self, peer: PeerRef) -> ImResult<DirectSessionStatus>;
    pub fn list_failed_outbox(&self) -> ImResult<Vec<SecureOutboxEntry>>;
    pub fn retry_outbox(&self, id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
    pub fn drop_outbox(&self, id: SecureOutboxId) -> ImResult<SecureOutboxResult>;
    pub fn group_status(&self, group: GroupRef) -> ImResult<GroupE2eeStatus>;
    pub fn repair_group_state(&self, group: GroupRef) -> ImResult<GroupE2eeStatus>;
}
```

## 3. 普通发送入口

即使 Phase 3 支持加密，普通发送仍应走：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Text { text, kind: MessageKind::Text },
    security: MessageSecurityMode::SecureDirect,
    ..
})
```

而不是：

```rust
send_direct_cipher(peer, ciphertext)
build_secure_init_payload(...)
process_group_mls_commit(...)
```

## 4. internal only

以下不进入默认 public API：

```rust
ciphertext payload
secure init/ack plaintext payload
signed prekey / one-time prekey storage APIs
MLS KeyPackage publish flow
MLS provider binary path
group e2ee notice processing
raw decrypt/encrypt message record
```

CLI 可以保留某些 diagnostic 命令，但它们不决定 SDK 主接口。

## 5. Provider 抽象后移

`CryptoProvider`、`MlsProvider`、`SecureSessionStore`、`PrekeyStore`、`SecureOutboxStore` 都是 Phase 4 议题。Phase 3 先复用显式路径和现有实现。
