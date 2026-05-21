# 08-advanced-secure：Phase 6 direct E2EE 与 group E2EE

## 1. 目标

加密能力不进入 Phase 1。Phase 1 的 `MessageSecurityMode` 只支持 `DefaultPlain` / `Plain`。如果调用 `SecureDirect` 或 `GroupE2ee`，必须返回 `UnsupportedCapability`，不能静默降级。

Phase 6 再迁移 direct E2EE、group E2EE、secure outbox 和 MLS。

## 2. 普通发送入口保持高层

secure direct 不应暴露成：

```rust
send_ciphertext(peer, payload)
```

而应通过普通消息入口启用：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Direct(peer),
    body: MessageBody::Text { text, kind: MessageKind::Text },
    security: MessageSecurityMode::SecureDirect,
    ..Default::default()
})?;
```

## 3. Phase 6 diagnostics API

```rust
client.secure().direct_status(peer)
client.secure().repair_direct_session(peer)
client.secure().list_failed_outbox()
client.secure().retry_outbox(id)
client.secure().drop_outbox(id)
client.secure().group_status(group)
client.secure().repair_group_state(group)
```

## 4. internal only

不进入默认 public API：

```text
build_secure_init_payload
build_secure_ack_payload
ciphertext processing API
prekey path
one-time prekey store detail
MLS provider binary path
KeyPackage publish/update/recover raw operations
group_e2ee wire params
```

这些可作为 internal、test helper 或 diagnostic feature，而不是 SDK 默认主接口。

## 5. CLI 命令处理

`msg secure *` 和 `group e2ee *` 可以继续作为 CLI diagnostic 命令存在，但它们不决定 SDK 主接口形态。

## 6. 完成判定

Phase 6 完成时：

- 普通调用方通过 `messages().send(... SecureDirect)` 使用加密。
- 诊断调用方通过 `secure()` 查询和修复状态。
- KeyPackage、MLS provider、prekey、ciphertext 细节不泄漏到默认 public API。
