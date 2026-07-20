# 设备永久撤销 Core 接口（第一阶段）

**状态**：Core implemented，独立 rollout default-off；Host/App、CLI 与远端 E2E 后续完成。

## 1. 定位与边界

`DeviceRevokeService` 实现 AWiki **域内**设备管理控制流。它不修改 ANP
跨域协议，也不新增 DID Document 扩展字段。第一阶段只支持永久撤销：

```text
active device -> revoked
```

不支持 suspended/reactivate、管理员降级或 revoked 设备原地恢复。

`document_version`、`document_hash`、`registry_version`、`auth_generation`、operation ID
及 root/admin proof 都是 AWiki 域内控制面字段，不进入 ANP、公开 Core DTO 或跨域消息。

## 2. 公开入口与门禁

入口为：

```rust
core.device_revoke().revoke(DeviceRevokeRequest {
    identity,
    target_device_id,
    user_presence_confirmed,
}).await
```

`ImCoreOpenOptions::multi_device_device_revoke_enabled` 是独立开关，默认 `false`；启用
Join、Root Transfer 或 Direct E2EE 均不会隐式打开撤销。开关启用后仍必须满足：

- 使用 `IdentitySecretStoragePolicy::VaultRequired` 且 Vault 可用；
- 调用设备为本地 `active + admin + management_ready`；
- 根私钥存在于该设备 Vault；
- Host 已完成前台 OS PIN、生物识别或等价 user-presence；
- 目标不是调用设备自身；
- 操作不会撤销最后一台 ready admin。

任何条件不满足都在状态提交前 fail closed。

## 3. 域内执行流程

```text
读取最新 Registry 与 DID Document
    -> 校验 checkpoint、当前 ready admin 与目标设备
    -> 使用 ANP SDK 从完整 Document/Manifest 删除目标设备及其 verification relationships
    -> Vault 解封根私钥并重新签署新 DID Document
    -> 先把稳定 operation ID 和公开签名材料密封到 Vault pending record
    -> 使用当前设备签名密钥生成短期 admin proof
    -> 调用同域 device_revoke RPC（root proof + current-admin proof + CAS）
    -> 严格验证 target/status/auth_generation/checkpoint
    -> 服务端成功后才更新本地 DID Document 和内部 checkpoint
    -> 删除 pending record
```

服务端事务负责 DID Document、Device Registry、目标 `auth_generation + 1`、token/session
失效及内部 outbox。Message/Mailbox/PreKey/MLS 的后续收敛不改变 Identity 已生效的撤销结果。

## 4. 崩溃恢复与幂等

Core 在首次 RPC 前把业务意图写入认证加密的 Vault record。传输失败、响应丢失或进程
重启后，相同 `DID + target_device_id` 调用复用原 operation ID、预期 checkpoint 和
root-signed Document，但重新生成短期 admin proof。客户端不会重新构造第二个撤销事务。

版本/hash/Registry CAS 冲突、目标已失效或最后 ready admin 冲突会删除过期 intent，要求
重新拉取权威状态后再发起；普通传输失败保留 intent。服务器成功响应未被严格验证时，本地
Document 和 checkpoint 不前移。

## 5. 公开 DTO 与秘密隔离

公开请求只包含：

- 本地身份选择器；
- 目标不透明 `device_id`；
- Host 的 user-presence 确认结果。

公开结果只包含：

- DID；
- 目标 `device_id`；
- `revoked` 状态。

公开 DTO、`Debug`、日志和错误不得包含 operation ID、内部 checkpoint、DID Document、
root/admin proof、access/refresh token 或任何私钥。pending record 中只保存公开 Document
与签名材料，但仍强制放入 Vault，以防本机篡改 exact-retry 管理操作。

## 6. 验证覆盖

Core focused tests覆盖：

- 独立开关默认关闭，其他多设备开关不能隐式启用；
- member 调用者、self revoke 和最后 ready admin 拒绝；
- 普通设备与另一台 ready admin 的合法撤销；
- user-presence 过期；
- 响应丢失并重启后复用 operation ID、刷新 admin proof；
- 版本冲突清理旧 intent，服务端错误数据脱敏；
- 非法成功响应不推进本地状态；
- wire/result/Debug 不泄漏 proof、Document 内容或私钥形态。
