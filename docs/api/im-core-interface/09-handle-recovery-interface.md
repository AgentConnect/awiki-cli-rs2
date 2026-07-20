# Handle Recovery Core 接口（第一阶段）

**状态**：Core implemented，rollout default-off；Dart/CLI 接线后续完成。

## 1. 定位与边界

`HandleRecoveryService` 实现 AWiki **域内**恢复控制面，不是 ANP 跨域协议。V1 只支持：

```text
恢复原 AWiki Handle
    -> 新设备本地生成全新的 root/device keys
    -> 创建不同的新 vNext DID
    -> Handle CAS 切换到新 DID
```

它不恢复旧根私钥、不执行同 DID 换根，也不调用 legacy
`IdentityRegistry::recover_handle()`。旧接口继续作为独立 legacy 路径存在。

`document_version`、`document_hash`、`registry_version` 和 Handle mapping generation
只用于 AWiki 域内一致性校验与 CAS，不进入 ANP、DID Document 扩展或公开 Core DTO。

## 2. 打开条件

入口为：

```rust
core.handle_recovery()
```

`ImCoreOpenOptions::multi_device_handle_recovery_enabled` 默认 `false`。启用后仍必须使用
`IdentitySecretStoragePolicy::VaultRequired` 和可用的 `SecretVault`；否则在网络或身份变更前
fail closed。Dart facade 和 CLI 尚未开放该开关，因此当前产品路径仍保持关闭。

## 3. 生命周期

Core 暴露以下高层操作：

| 操作 | 作用 |
| --- | --- |
| `begin` | 使用 begin 专用账户验证 grant 创建或语义幂等重试 Recovery Session |
| `status` | 使用 Vault 中的 session token 获取冷静期状态 |
| `cancel` | 由旧 DID 当前 `AdminReady` 设备签名取消 |
| `finalize` | 再确认后生成全新身份并提交 Handle CAS |
| `resume_activation` | 同步恢复本地激活；只接受尚未过期的已持久化 access token，过期时返回 `SessionExpired` |
| `resume_activation_async` | 异步恢复本地激活；access token 过期时先获取新的 management-ready token pair |
| `mark_activation_complete` | Host 确认切换完成后幂等清除 pending record |

begin grant 和 reconfirmation grant 是 write-only 类型，`Debug` 始终脱敏；session token、
DeviceProof、新 DID Document、私钥、内部 checkpoint 和返回 token pair 仅存在于 E2EE/HTTPS
传输或加密 Vault pending record，不向 Host DTO 返回。

pending JSON 的序列化缓冲区使用 `Zeroizing<Vec<u8>>`，交给 Vault 的副本由
`SecretBytes` 接管并在 Drop 时清零；pending record、Recovery session/result 也在 Drop 时清零
账户 grant、session/access/refresh token 和所有新生成私钥。相关 `Debug` 与错误只输出脱敏状态。

## 4. 崩溃恢复与身份隔离

Core 在 RPC 前先把稳定 operation ID、当前请求证据和后续生成材料写入一个稳定 Vault
record。响应丢失或进程重启后保持原 operation、Handle CAS、Document 和密钥不变；已过期的
OTP grant 或 DeviceProof 可以在同一业务请求上刷新，再做语义幂等重试，避免生成第二套身份。
远端 cutover 成功后，pending record 一直保留到 Host 明确确认本地激活完成。

如果 finalize 的成功响应丢失时间超过 access token TTL，服务端安全重放可能返回原来的过期
token pair。Core 会保留已验证的 cutover 结果，并使用新 DID 的设备签名私钥调用现有
`device_token_issue` 获取新的 `management-ready` token pair；过期 token 不会落入本地身份。
凭据签发 operation 在 Vault 中单独持久化。只有该 operation 的安全重放本身也返回过期 token
时才轮换凭据 operation；Recovery cutover operation、Document、Handle CAS 和生成密钥始终不变。
刷新或网络失败时 pending record 继续保留，可由 `resume_activation_async` 重试。

新身份使用新的本地 `IdentityId`/OwnerScope 和全新的 root、设备签名、设备 E2EE 密钥。
保存时不 merge/copy 旧 Direct Ratchet、MLS、root secret、群换绑任务或历史解密状态；旧本地
身份记录保留为独立身份，不能被新 DID 静默继承。

## 5. 安全校验

- begin/finalize 使用不同用途的账户验证 grant；首次 OTP 不能替代最终再确认；
- begin 前和 finalize 前都重新读取同域权威 Handle 绑定；finalize 精确绑定原 mapping
  generation，服务端只允许递增一代；
- finalize 只接受与本地生成 Document、bootstrap device、ready-admin generation 1 和原账户
  `user_id` 一致的结果及设备 token；
- cancel 必须同时回读当前认证 Registry 和最新 DID Document；设备须在 Registry 中为
  `active + admin + management-ready`，并与 Manifest 公钥绑定一致。这里的 admin 状态是
  AWiki 域内授权，不扩展 ANP Manifest；普通设备、已撤销设备和本地状态过期的旧设备不能取消；
- 公开进度只包含 session ID、Handle、旧/新 DID、阶段和时间，不包含秘密或内部版本链。
