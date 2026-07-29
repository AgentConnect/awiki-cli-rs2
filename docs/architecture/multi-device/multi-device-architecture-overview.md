# AWiki 多设备架构概览（V1）

**状态：待 Review**

> 本文用于快速理解 V1。完整状态、事务和安全边界以
> [多设备架构设计](./multi-device-architecter.md)为准；精确 JSON/RPC 契约以
> [新设备加入消息与 RPC 契约](../../../../plan/20260718-awiki-multi-device-implementation/refactor/device-join-message-contract.md)
> 为准。

---

## 1. 一句话架构

```text
一个 DID
  + 根签名 deviceManifest 公开合法设备及公钥
  + AWiki Device Registry 保存域内角色和状态
  + 每台设备用独立 signing key 认证
  + ready admin 才持有并使用 DID root key
```

V1 是长期架构的首个完整版本，不是临时旁路。它完成注册、Legacy 单设备升级、设备认证、消息驱动
Join、通用系统通知、根密钥传输、管理员升级和设备撤销；Recovery 等高级能力以后独立增加。

---

## 2. 三层边界

| 层次 | 权威内容 |
| --- | --- |
| 跨域公开层 | DID、根签名 DID Document、`deviceManifest`、设备公钥、P3/P5、MLS |
| AWiki 域内控制层 | Device Registry、Join、角色、readiness、`auth_generation`、access-token issuer |
| 设备本地安全层 | SecretVault、设备私钥、可选 root key、Ratchet/MLS 私有状态 |

Manifest 只说明某台设备是不是公开合法端点，不公开 `member/admin`、token、Join 状态或本地 root
能力。Registry 是 AWiki 域内授权权威，远端 ANP 节点不读取它。

---

## 3. 一个正式设备身份模型

每台设备本地保存：

```text
device_id
device signing key
device E2EE key
Manifest / Registry checkpoint
access token（可空）
root capability = absent | pending | active
```

每类 key 的用途固定：

- DID root key：只用于 DID Document 管理和 root possession proof；
- device signing key：用于 DID-WBA、设备证明和业务签名；
- device E2EE key：用于 P5、PreKey 和设备级加密。

三者不能相互替代。客户端必须显式使用本机 `device_signing_key_id`；服务端由签名 `keyid` 精确
定位 Manifest entry 和 active Registry device，不能默认选择 DID Document 中第一把 key。

V1 只有两种 active 授权状态：

```text
active member + management_ready=false
active admin  + management_ready=true
```

不存在 `active admin + management_ready=false`。Join 永远先得到 member；root completion 成功时
直接原子升级为 ready admin。

---

## 4. 注册与 Legacy 升级

### 新用户

继续使用现有 `register`：

```text
客户端生成 root + device keys
  -> 构造恰好一个 bootstrap device 的 Manifest DID Document
  -> register
  -> User Service 一个事务创建 User、Handle、DID checkpoint、Registry 和首设备
  -> 首设备成为 active ready admin
  -> 返回 access_token
```

服务端必须在 DTO 裁剪前读取原始签名 DID JSON：

```text
无 deviceManifest -> Legacy register adapter
有合法单设备 deviceManifest -> Multi-device register adapter
Manifest 出现但无效 -> 拒绝，不回退 Legacy
```

不再保留 `device_genesis`、Genesis grant 或第二套首设备注册流程。

Phone、Email、AlreadyVerified、Invite 等原注册能力仍由同一个 `register` 入口处理，不能因
多设备改造而收窄为 phone-only。

### Legacy 原设备

如果生产环境存在无 Manifest 的 Legacy 身份，V1 允许：

```text
持有可用 Legacy key-1 的原设备
  -> 把 key-1 作为原 DID root
  -> 生成独立 device signing/E2EE key 和 device_id
  -> 通过既有 update_document
  -> 同 DID、同 Handle 一次性升级为单设备 Manifest/Registry
```

V1 不处理复制了多份 Legacy root 的并发升级，不处理原设备/根密钥丢失，也不允许未升级 Legacy
直接进入新设备 Join。

---

## 5. 统一设备认证

设备 token 由 DID-WBA 认证层签发，不属于 `get_me` handler：

```text
支持 DID-WBA 的 User Service RPC
  + fresh 本机设备签名
  + Manifest / active Registry 校验成功
  + 业务请求成功
  -> 标准认证响应头返回新的 access token
```

`get_me` 只是没有其他业务请求时推荐使用的无副作用 bootstrap RPC。Bearer 请求不续期；V1 没有
设备 refresh token、`device_token_issue` 或 `device_token_refresh`。

Token 至少绑定 DID、user、`device_id`、`key_id`、`auth_generation`、scope、audience 和 expiry。
member 可以通信并提交本机 root-import completion；只有 ready admin token 才有
`device:manage`。

V1 不新增 blacklist/introspection，但保留当前 Manifest、Registry、key、scope 和 generation
检查。撤销设备不能再通过签名取得新 token。

---

## 6. 通用系统通知

Message Service 提供一个可复用的 DID System Notification Sender：

```text
可信业务事务
  -> Outbox / Notification Intent
  -> Message Service DID 签署 P3 通知
  -> 现有 persist / Mailbox / sync / realtime
  -> 客户端系统通知分发器
```

V1 支持稳定事件 ID、设备接收范围、`text/plain`、`application/json`、Origin Proof、离线持久化、
幂等和重试。内置业务类型至少覆盖 Join 系列；可选发送无秘密的 root-imported 刷新通知。

系统通知发送者必须是目标用户当前 DID Document 中
`ANPMessageService.serviceDid` 指向的 Message Service DID，并验证 P3
`auth.origin_proof`。域内设备 fan-out 不进入公开 P3 metadata。

模板后台、偏好中心、营销、多渠道、已读中心、搜索和统计不属于 V1。

---

## 7. 消息驱动 Join

```text
新设备 OTP 定位已有 DID
  -> 生成 candidate device keys 并签署 Join Request
  -> HTTPS 创建 Join Session
  -> User Service 写 JoinRequested Outbox
  -> 通用系统通知到达所有 ready admin
  -> 用户在一台旧设备点击开始验证
  -> 原子 submit_challenge：claim + encrypted Challenge
  -> 新设备通过 HTTP status 取得 Challenge 并响应
  -> 两台设备独立计算并人工比较六位 SAS
  -> 旧设备完成 user-presence 并提交双签 DID 更新
  -> User Service 原子提交 Document + Registry + consumed
  -> 新设备重新 resolve 并验证自己的 Manifest entry
  -> fresh device-signed request 取得 member access token
```

旧 ready admin 由 Message 通知驱动，不后台轮询 Join 列表或状态；尚未取得 DID 身份的新设备可以
轮询自己的 `device_join_status`。

`claim` 和 Challenge 是同一个 CAS，不保留“已认领但没有 Challenge”的中间写状态。SAS 不经过
服务器、Outbox、Message 或日志。

Join 的固定结果是：

```text
active + member + management_ready=false
root capability = absent
```

---

## 8. 根密钥传输与管理员升级

管理员升级是 Join 后的独立事务：

```text
旧 ready admin 检查目标、Registry、Manifest 和 P5 能力
  -> 用户只确认一次本次根传输，不额外要求系统 PIN/生物识别
  -> 已有 Session：普通 P5 Cipher 携带 RootKeyEnvelope
     无 Session：首个普通 P5 Init 直接携带 RootKeyEnvelope
  -> 新设备解密、复验当前状态、原子导入 pending Root Vault
  -> HTTPS device_root_import_complete
     外层 importing-device Object Proof
     内层 root-possession Object Proof
  -> User Service 原子设置 role=admin + management_ready=true
     并递增 auth_generation
  -> 新设备确认 Registry ready，把 pending root 提升为 active
  -> fresh device-signed request 取得管理 access token
```

根密钥传输复用普通 P5 exact-device 路径，不增加 root 专用 Profile、`delivery_class`、私有 sidecar、
第二套 Mailbox 或 Ratchet。没有 Session 时不发送空 Init，也不要求第二次确认。

V1 没有 E2EE imported ACK；P5 Reply 只收敛 Session，不能表示根导入成功。只有 Registry ready
且本地 active root 可读时，客户端才显示为可管理设备。

任一步失败都不回滚 Join，设备继续作为普通 member。

---

## 9. 消息、MLS、附件与撤销

- 普通 P3 Base 继续按业务 DID 发送；
- P5 Direct E2EE 为每个 exact device 建立独立 PreKey、Session、Ratchet 和密文投递；
- V1 逐设备 `direct.send`，不增加 `deliveries[]` 批量协议；
- 同一 DID 的每台设备是独立 MLS Leaf，不共享 MLS private state；
- 新设备不自动加入所有历史群，也不自动获得历史消息；
- 附件对象只加密上传一次，附件 key 经各设备 P5 或当前 MLS epoch 分发。

设备移除是永久 revoke：

```text
Manifest 删除设备/key
Registry 标记 revoked
document/registry version 递增
目标 auth_generation 递增
停止新 token、PreKey、Mailbox 和 KeyPackage
各 MLS 群异步 Remove/Commit
```

任何管理 mutation 都必须保证提交后仍至少有一台 ready admin。V1 没有最后管理员丢失后的
Recovery，因此这条保护不能绕过。

---

## 10. 版本、事务与部署

V1 只保留：

```text
document_version + document_hash
registry_version
per-device auth_generation
```

关键事务：

- 注册：User、Handle、DID checkpoint、Registry、首设备一个事务；
- Join approve：Document、Registry、Join consumed 一个 CAS；
- root completion：role、readiness、generation、幂等结果一个事务；
- 通知、Mailbox 清理和 MLS Remove 在提交后幂等收敛。

部署顺序：

```text
additive migration
  -> Message Service 兼容设备 principal、通知和 P5
  -> User Service 统一 register/auth/Join/completion
  -> im-core 与 AWiki Me 切换
  -> 跨服务和真实 E2E
  -> 删除过渡代码与旧字段
```

---

## 11. 必须删除的旧架构

V1 不能长期保留以下并行路径：

- `device_genesis` 和 Genesis grant；
- Join 后 `device_token_issue`；
- 设备 refresh token 和 refresh RPC；
- 身份/认证 rollout gate；
- 旧 admin 的 Join 轮询；
- 分离的 claim 与 Challenge；
- Join 直接产生 admin 或 `AdminAwaitingRoot`；
- root `delivery_class`、sidecar、空 Init、二次确认；
- imported ACK 驱动 readiness；
- 可执行的 V1 Recovery 假入口。

仍保留的兼容能力只有旧客户端的无 Manifest `register`，以及原设备持有 Legacy root 时的一次性
单设备升级。

---

## 12. 总结

V1 的稳定骨架是：

> Manifest 管公开设备，Registry 管域内权限，设备签名管日常认证，SecretVault 管本地密钥，
> 通用 Message 通知驱动旧设备审批，普通 P5 承载 root，独立 completion 原子授予管理员。

这套边界覆盖第一版主体能力。未来的 Recovery、透明日志、复杂通知产品和历史迁移可以独立演进，
不需要破坏 V1 身份模型或重新引入并行注册、Token、Join 和根传输流程。
