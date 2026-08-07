# 设备永久撤销 Core 接口（第一阶段）

**状态**：Core、Dart/Flutter SDK、AWiki Me 与 CLI 已实现，独立 rollout default-off；远端 E2E 待部署能力启用。

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

Dart/Flutter Host 使用 `multiDeviceDeviceRevokeEnabled` 与
`revokeDevice(selector:, targetDeviceId:, userPresenceConfirmed:)`。CLI 使用
`AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED=1 awiki-cli id device revoke --device <device_id>`，
且只允许前台交互式终端；用户必须重新输入目标设备 ID 和 `REVOKE`。两者均不接受版本、
hash、proof、generation 或任何密钥参数。

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
    -> 若 P6 v2 已启用，枚举当前 DID 的 active-owner 群并对目标
       (DID, device_id) Leaf 执行精确 Remove/Commit
    -> 删除 pending record
```

服务端事务负责 DID Document、Device Registry、目标 `auth_generation + 1`、token/session
失效及内部 outbox。Message Service 会持久化每个相关群的 MLS removal pending；当前设备只
立即处理自己作为 active P4 owner、且本机持有 active MLS controller Leaf 的群，并保留同
DID sibling Leaf 和 P4 业务成员。当前设备尚未加入的历史群，以及其他 owner 的群，由持有
对应本地 MLS 状态的 owner 设备执行 `group secure repair` 收敛。Message/Mailbox/PreKey/MLS
的后续收敛不改变 Identity 已生效的撤销结果。

## 4. 崩溃恢复与幂等

Core 在首次 RPC 前把业务意图写入认证加密的 Vault record。传输失败、响应丢失或进程
重启后，相同 `DID + target_device_id` 调用复用原 operation ID、预期 checkpoint 和
root-signed Document，但重新生成短期 admin proof。客户端不会重新构造第二个撤销事务。

版本/hash/Registry CAS 冲突、目标已失效或最后 ready admin 冲突会删除过期 intent，要求
重新拉取权威状态后再发起；普通传输失败保留 intent。服务器成功响应未被严格验证时，本地
Document 和 checkpoint 不前移。

Identity RPC 已成功但当前设备可执行的 P6 Remove 尚未完成时，pending record 同样保留；
重复公开撤销调用不会重做 Identity 事务，只会继续 SDK WAL/精确 Leaf 收敛。P6 rollout 未
启用时不运行该本地步骤，服务端 durable removal pending 仍保留。

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

Core、SDK、CLI 和 App focused tests 覆盖：

- 独立开关默认关闭，其他多设备开关不能隐式启用；
- member 调用者、self revoke 和最后 ready admin 拒绝；
- 普通设备与另一台 ready admin 的合法撤销；
- user-presence 过期；
- 响应丢失并重启后复用 operation ID、刷新 admin proof；
- 版本冲突清理旧 intent，服务端错误数据脱敏；
- 非法成功响应不推进本地状态；
- wire/result/Debug 不泄漏 proof、Document 内容或私钥形态。
- P6 撤销只选择 active-owner 群和精确 `(DID, device_id)` Leaf，保留 sibling，并可从 SDK
  prepared WAL 重放；
- CLI 默认关闭、拒绝脚本/`--dry-run`，且只输出安全撤销结果；
- App 在显式破坏性确认后才请求一次系统 user-presence，拒绝时不调用 Core；
- 撤销开关不隐式启用 Join，当前设备不显示撤销动作，成功后重新读取权威 Registry。
