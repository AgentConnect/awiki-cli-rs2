# im-core P6 v2 内部产品编排

状态：内部集成切片，尚未切换公共 Core、Dart 或 AWiki Me 产品入口。

## 1. 边界

`internal/group_e2ee/v2_product.rs` 在现有 P6 v2 SDK runtime 之上提供内部编排：

- 当前设备生成并发布设备绑定的 KeyPackage；
- create、add、remove 的本地 prepare、Host 提交、精确回包校验和 finalize；
- 显式 abort，以及进程重启后的 `reconcile_pending_v2`；
- 标准 `V2GroupNoticeMetadata + V2E2eeNotice` 控制通知消费；
- MLS Application Message 的一次加密、原密文提交和设备本地解密。

本层不改变 P4 业务成员语义，不决定群 owner 策略，也不提供 legacy 降级路径。
生产网络必须通过 `GroupE2eeV2Host` 的认证 RPC Adapter；测试 double 只存在于测试模块。

## 2. 状态与安全规则

每个 runtime 必须具有真实 `device_id` 和精确 `GroupMlsOwnerScope`。同一 DID 的不同设备
使用不同的 scoped SQLite/OpenMLS 状态；KeyPackage 私钥、Leaf 私钥、epoch secret 和 MLS
数据库不得复制给兄弟设备。

Host 返回 accepted 后，Core 仍须逐项核对：

```text
operation_id
RFC 9421 signed-payload digest
group_did / group_state_ref
crypto_group_id / epoch
目标 member DID + device_id
message_id（消息发送）
```

全部匹配后才允许 finalize。网络结果不确定或回包不匹配时，pending commit 保持
`prepared`，重启后的 reconcile 明确返回 `host_recheck_required=true`；不得自动假定成功，
也不得自动 abort。只有上层确认 Host 给出确定拒绝后才能显式 abort。

OpenMLS 与 SDK metadata 使用不同 SQLite 连接，因此这里不声称跨存储原子事务，只复用
SDK 的可恢复 WAL：`preparing -> prepared -> accepted -> finalized/aborted`。

## 3. 收件与 timeline

`group.e2ee.notice` 只能进入 `consume_notice`，成功结果固定为 `ConsumedControl`，不生成普通
timeline message。`group.incoming` 只有通过标准结构校验、精确 recipient DID/device 校验和
MLS 解密后，才返回 Application plaintext 供后续业务投影。

当前切片记录一个发布阻塞项：发起设备在 Host accepted 后已本地 finalize，再收到 Host
广播的自身 Commit echo 时，现有 SDK 会因本地 epoch 已前进而 fail closed。Core 不把该错误
伪装为成功；SDK 必须基于 finalized pending journal 对 operation、group、commit、epoch 和
actor device 做精确匹配后，才能把自身 echo 记为幂等 receipt。

## 4. 尚未完成

- 公共 Core/Dart API 与 AWiki Me 状态展示；
- 普通消息读取/realtime 管线切换到 v2 control/application 分流；
- durable message outbox 与远端 `awiki.info` 多设备 MLS E2E；
- P6 草案 extension code point 的正式发布门禁。
