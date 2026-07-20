# im-core P6 v2 内部产品编排

状态：内部产品编排仍在集成；默认关闭的 Core/Dart/AWiki Me 状态与修复入口已接入。

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

对于协议要求 origin auth 的 create/add/remove/send，Core 在提交前使用 RFC 9421 origin
proof 将本地 `method + meta + body`（包括 `operation_id`）绑定到签名的
`content-digest`。这些值属于本地请求关联信息；当前协议的 Host result 不回显请求 digest，
因此不把本地重算值描述成 Host 回包校验。publish/get 使用其协议定义的认证边界和 typed
result，不虚构 origin proof。

Host 同步返回 accepted 后，Core 只按协议实际定义的 typed result 逐项核对：

```text
group_did / group_state_ref
crypto_group_id / epoch
目标 member DID + device_id
operation_id + message_id（仅协议实际回显这些字段的消息发送结果）
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

发起设备在 Host accepted 后已本地 finalize，再收到 Host 广播的自身 Commit echo 时，SDK
只在 finalized pending journal 与 actor DID/device、operation、group/state、subject、epoch 和
Commit bytes/digest 全部精确且唯一匹配时记录幂等 receipt，不会再次 merge Commit。重启后的
精确 replay 返回同一控制结果；任何不匹配继续 fail closed。

## 4. 公开状态边界与尚未完成项

- lifecycle/send 产品管线完整切换到 v2；
- 普通消息读取/realtime 管线切换到 v2 control/application 分流；
- durable message outbox 与远端 `awiki.info` 多设备 MLS E2E；
- P6 草案 extension code point 的正式发布门禁。

公共 `secure.group().status()/repair()` 通过本地
`multi_device_group_e2ee_enabled` gate 选择 P6 v2。该 gate 默认关闭，不进入 ANP、DID
Document 或跨域请求。状态路径只调用 SDK typed inspection/reconcile API，不直接读取 SDK
SQLite schema；返回值只包含 readiness、修复状态和计数。
