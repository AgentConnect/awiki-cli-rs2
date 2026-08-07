# AWiki 多设备架构设计（V1）

**状态：待 Review**

**定位：长期架构的 V1 落地基线**

> 本文固定 AWiki 多设备的总体架构、权威状态、核心流程和安全边界。精确 JSON/RPC
> 字段不在本文重复定义；注册与认证以
> [注册、迁移与认证统一方案](../../../../awiki-plan/20260718-awiki-multi-device-implementation/refactor/registration-auth-unification-plan.md)
> 为准，Join 与根密钥流程以
> [消息驱动流程](../../../../awiki-plan/20260718-awiki-multi-device-implementation/refactor/device-join-message-driven-flow.md)
> 和
> [消息与 RPC 契约](../../../../awiki-plan/20260718-awiki-multi-device-implementation/refactor/device-join-message-contract.md)
> 为准，实施顺序以
> [V1 总体执行计划](../../../../awiki-plan/20260718-awiki-multi-device-implementation/refactor/v1-overall-execution-plan.md)
> 为准。跨域设备与 E2EE 语义仍以 ANP 的
> [Device Manifest](../../../../anp/anp/docs/did/device-manifest-vnext-sdk.md)、
> [P5 Direct E2EE](../../../../anp/anp/docs/e2e/direct-e2ee-p5-sdk.md)和
> [P6 Group E2EE](../../../../anp/anp/docs/e2e/group-e2ee-p6-v2-sdk.md)
> 文档为准。

---

## 1. 架构目标

V1 不是临时旁路，而是长期多设备架构的第一个完整版本。它必须完成主体能力，同时把暂时不做的
高级能力放到清晰边界之外。

V1 必须形成以下闭环：

1. 新用户通过现有 `register` 一次完成身份和首设备注册；
2. 持有原 Legacy 根密钥的原设备可以一次性升级到 Manifest 身份；
3. 每台设备用自己的设备签名密钥认证并取得 access token；
4. 新设备通过消息驱动的 Join 加入已有 DID；
5. Message Service 提供可复用的通用 DID 系统通知能力；
6. 普通 member 可以在 Join 后通过普通 P5 接收根私钥，并原子升级为 ready admin；
7. 设备撤销和所有管理写操作保留必要的当前状态校验与最后管理员保护；
8. 每个模块切换时同步删除被替代的旧实现。

架构的简化原则是：

```text
一个注册入口
一个正式设备身份模型
一个设备认证方式
一个 Join 状态机
一个通用系统通知传输模块
一个 Join 后管理员升级流程
```

长期扩展通过版本化契约和独立状态转换增加，不通过在 V1 主路径中保留多套入口、灰度分支或并行
状态机实现。

---

## 2. V1 范围与非目标

### 2.1 V1 范围

- 根签名 DID Document 内嵌 `deviceManifest`；
- AWiki 域内 Device Registry；
- 首设备注册和单原设备 Legacy 升级；
- access-only 的设备认证；
- 通用 Message Service DID 系统通知；
- Challenge、SAS、双签 DID 更新和最终独立验证；
- 普通 P5 exact-device Direct E2EE；
- Join 后根密钥传输和管理员升级；
- 设备级 Direct E2EE、设备级 MLS Leaf、附件密钥分发；
- 永久设备撤销；
- 本地 SecretVault 和崩溃恢复所需的最小 pending 状态。

### 2.2 V1 明确不做

- 原设备丢失后的 Legacy Recovery；
- 多台复制了同一 Legacy 根密钥的设备并发升级；
- 新设备加入尚未升级的 Legacy 身份；
- Handle Recovery、同 DID 换根或创建新 DID 的恢复流程；
- access token blacklist 或新的在线 introspection 系统；
- 设备 refresh token；
- `suspended/reactivate`、管理员降级和 revoked 原地恢复；
- transparency log、witness 和主动恶意服务端 split-view 检测；
- 自动把新设备加入全部历史 MLS 群；
- 全量历史消息迁移和复杂多端冲突合并；
- 通知模板后台、偏好中心、营销编排、短信/邮件多渠道、已读中心、全文搜索和统计。

这些能力以后可以作为独立版本增加，但不得要求现有设备重新生成 DID、替换设备密钥，或让 V1
正式身份回退到另一套运行时模型。

---

## 3. 总体架构与职责分离

```text
AWiki Me / CLI / Agent
        |
        | register / DID-WBA / Join / completion
        v
User Service
  - User / Handle
  - DID Document checkpoint
  - Device Registry
  - Join state machine
  - access-token issuer
        |
        | transactional Outbox / Notification Intent
        v
Message Service
  - System Notification Sender
  - P3 persist / Mailbox / sync / realtime
  - P5 exact-device opaque ciphertext routing
        |
        v
AWiki devices
  - SecretVault
  - device signing / E2EE keys
  - optional DID root key
  - Direct Ratchet / MLS state
```

四个职责必须分开：

```text
身份注册
  = 创建 User、Handle、DID 和首设备

设备准入
  = 把设备公钥加入已有 DID 的 Manifest 和 Registry

会话认证
  = 已授权设备用设备签名取得 access token

管理员升级
  = Join 后导入根私钥，并通过独立 completion 原子授予 ready admin
```

首设备可以在一个 `register` 事务中同时完成身份注册和首设备准入，但这不产生第二套注册接口或
长期认证协议。Join、认证和管理员升级是独立的状态转换，任何一个流程都不能顺带修改另一个流程
负责的权限。

---

## 4. 权威状态与数据模型

### 4.1 单一真相源

| 数据 | 权威来源 | 作用 |
| --- | --- | --- |
| 用户业务身份 | DID | 联系人、私聊和群成员的业务身份 |
| 公开设备及 key binding | 根签名 DID Document 的 `deviceManifest` | 跨域验证合法设备端点 |
| 域内设备状态、角色和 generation | User Service Device Registry | AWiki 授权决策 |
| DID 根私钥 | 持有该能力设备的本地 SecretVault | DID Document 更新和根持有证明 |
| 设备签名私钥 | 当前设备本地 SecretVault | DID-WBA、设备证明和业务签名 |
| 设备 E2EE 私钥 | 当前设备本地 SecretVault | P5、PreKey、Ratchet 和设备级加密 |
| 当前业务会话 | access token | Bearer 业务请求 |

`deviceManifest` 和 Registry 表达不同层面的事实：

- Manifest 是公开、可验证的通信设备集合和公钥绑定；
- Registry 是 AWiki 域内的角色、readiness、撤销状态和 `auth_generation`；
- 远端 ANP 实现不读取 AWiki Registry；
- User Service 的设备认证和管理写必须同时检查 Manifest 与 Registry。

### 4.2 DID Document 与 Manifest

新版本 DID Document 必须：

- 由当前 DID 根密钥签名；
- 内嵌 `deviceManifest`，不再通过独立 Manifest HTTP 资源维护；
- 为每台设备声明稳定的随机 `device_id`；
- 明确绑定设备 signing key 和 E2EE key；
- 保持根 key、设备 signing key 和设备 E2EE key 的角色分离；
- 通过 verification relationship 表达各 key 的用途。

Manifest 不公开：

- `member/admin`；
- `management_ready`；
- `auth_generation`；
- access token 或 scope；
- 本地 root capability；
- Join、根传输或通知内部状态。

### 4.3 Device Registry

V1 Registry 只需要以下核心字段：

```text
did
device_id
signing_key_id
e2ee_key_id
status = active | revoked
role = member | admin
management_ready = true | false
auth_generation
```

V1 允许的有效授权组合是：

| 状态 | 含义 | 是否可通信 | 是否可管理设备 |
| --- | --- | --- | --- |
| `active + member + management_ready=false` | 普通设备 | 是 | 否 |
| `active + admin + management_ready=true` | 已就绪管理设备 | 是 | 是 |
| `revoked + management_ready=false` | 已永久撤销设备 | 否 | 否 |

V1 不产生 `active + admin + management_ready=false`。新设备永远先以 member 加入；根密钥导入
completion 成功时，User Service 在一个事务中直接把它变成 ready admin。

### 4.4 本地 DeviceIdentity

新版本本地身份只有一种正式形态：

```text
device_id
device_signing_key_id + private SecretRef
device_e2ee_key_id + private SecretRef
Manifest authorization checkpoint
Registry authorization checkpoint
access token（可空、可替换）
root capability = absent | pending | active
```

root capability 的含义：

- `absent`：普通 member，没有根私钥；
- `pending`：根私钥已安全导入本地 Vault，但远端仍是 member，只能用于本次 completion；
- `active`：本地根私钥可读，且 Registry 确认为 ready admin。

`pending` 不是服务端角色，也不能用于一般 DID 管理。只有本地 `active` 与远端
`active + admin + management_ready=true` 同时成立时，客户端才投影为可管理设备。

Legacy `key-1` 只是升级输入，不是新版本的另一种正式运行时身份。

---

## 5. 密钥与设备定位

每台设备至少有三类不同用途的密钥：

| 密钥 | 持有者 | 用途 | 是否可替代其他角色 |
| --- | --- | --- | --- |
| DID root key | 首设备及完成升级的 ready admin | DID Document 更新、root possession proof | 否 |
| device signing key | 每台设备 | DID-WBA、Join/管理设备证明、PreKey 等签名 | 否 |
| device E2EE key | 每台设备 | P5 X3DH/Session、设备级 E2EE | 否 |

客户端必须显式保存并使用当前设备的 `device_signing_key_id`。服务端不能默认选择 DID Document
中第一把 `authentication` key，也不能只根据 DID 猜测设备。

设备签名认证成功后，User Service 按以下绑定精确定位设备：

```text
HTTP Signature keyid
    -> 当前根签名 DID Document authentication key
    -> deviceManifest signing key reference
    -> exact device_id
    -> 当前 active Registry row
```

任意一步不唯一、不匹配或已经撤销，都必须拒绝。DID root key 不能替代 device signing key 取得
设备 access token。

---

## 6. 注册与 Legacy 升级

### 6.1 一个 `register` 入口

服务端保留现有 `register`，并根据客户端提交的原始签名 DID Document 做确定性分派：

```text
原始 JSON 不含 deviceManifest
  -> LegacyRegisterAdapter

原始 JSON 含 deviceManifest
  -> MultiDeviceRegisterAdapter
  -> 必须恰好包含一个 bootstrap device

deviceManifest 出现但为 null、空、类型错误、引用错误或 proof 无效
  -> 明确拒绝
  -> 不得回退 Legacy
```

分派必须发生在 DTO 归一化或未知字段裁剪之前。proof 验证、canonical hash 和最终持久化都基于
保留下来的完整签名 DID JSON，避免客户端签名内容与服务端实际校验或存储内容不同。

### 6.2 新用户注册事务

```text
客户端生成 root、device signing、device E2EE 和随机 device_id
  -> 构造恰好一个 bootstrap device 的根签名 DID Document
  -> 调用既有 register
  -> User Service 校验注册凭证、Handle、DID、proof 和 Manifest
  -> 一个数据库事务创建：
       User
       Handle binding
       DID Document + document checkpoint
       Device Registry
       首个 active ready admin
  -> 返回 access_token
  -> 客户端原子保存 DeviceIdentity 和全部 SecretRef
```

Manifest 注册初始化：

```text
document_version = 1
document_hash = canonical signed document hash
registry_version = 1
bootstrap auth_generation = 1
```

事务内的存储方法只能 `add/flush`，由最外层统一 commit。邮件、积分和其他外部副作用只能在提交后
执行，或通过既有 Outbox 收敛。

新注册本地事务提交成功后，Core 必须继续生成并发布该 exact device 的 P5 PreKey Bundle，成功
后才删除加密 PendingRegistration 并报告注册流程完成。发布失败时保留同一 pending、同一
device E2EE key 和同一 PreKey 身份供精确重试，不得重新调用 `register` 或重新生成身份。旧
`publish_v2_prekeys_after_genesis*` 实现只允许改名为 registration 语义并迁移调用点，不能随
Genesis 入口一起删除。

`register` 响应继续使用现有 `access_token` 字段，不增加 refresh token，也不增加
`device_genesis`、Genesis grant 或另一套首设备注册入口。

原注册已有的 Phone、Email、AlreadyVerified、Invite 等验证能力继续由同一个入口处理；多设备
不能把注册收窄为 phone-only。无 Manifest 的旧客户端继续获得原 Legacy 响应；新版本客户端在
完成一次性升级后必须切换到正式设备 access token，不能把 Legacy token 当作新身份的可选模式。

客户端可以保留加密 pending record 处理“服务端已成功、本地尚未提交”，但重试必须复用同一身份
和密钥，不能把 pending record 发展为远端 Genesis 协议。

正常注册 pending 只有在 User Service 明确返回稳定
`device.document_proof_expired` 时，才允许保留同一 DID、root/device/daemon keys 与 device ID，
刷新顶层 root proof 和 document hash 并有界重试一次。网络失败、5xx、通用文档错误和错误文案
均不能触发重签；模糊结果继续优先 reconciliation，避免把已提交注册误判为可重放请求。

### 6.3 单原设备 Legacy 升级

V1 支持一个严格的一次性升级：

```text
原设备本地仍持有可用 Legacy key-1
  -> 明确把 key-1 作为现有 DID root key
  -> 新生成 device_id、device signing key、device E2EE key
  -> 持久化加密 PendingLegacyUpgrade
  -> 构造同 DID、同 Handle、同 root、单设备 Manifest 的新文档
  -> 通过既有 update_document 提交
  -> User Service 原子更新 DID checkpoint 并初始化 Registry
  -> 原设备使用新 device signing key 发起 fresh DID-WBA 请求
  -> 保存 access token，完成本地 DeviceIdentity 提升
```

升级约束：

- 不创建新 DID，不更换 Handle；
- 不把根 key 或旧 E2EE key 当作新 device signing key；
- 只允许“无 Manifest → 单设备 Manifest”一次转换；
- 远端成功、本地失败时复用同一 pending key material 和文档；
- 身份状态切换为 vNext 时不得删除 Legacy `key-2`/`key-3`、既有 PreKey、Session、Ratchet
  或 MLS state；它们至少作为历史解密与显式迁移材料保留，直至另行定义并满足兼容窗口、迁移
  完成证明和安全清理条件；
- 本版本只保证一台原设备持有一份 Legacy 根密钥时的升级；
- 复制了根密钥的多个旧设备并发升级、原设备丢失和 Legacy Recovery 均不支持；
- 尚未升级的 Legacy 身份不能进入新设备 Join。

---

## 7. 设备认证与 access token

### 7.1 统一签发规则

设备 access token 由 User Service 的 DID-WBA 认证层统一签发，不属于 `get_me` handler。

只要请求满足以下条件：

1. 调用的是支持 DID-WBA 认证的 User Service RPC；
2. 请求携带 fresh HTTP Signature；
3. `keyid` 精确指向本机 device signing key；
4. Manifest 与当前 active Registry 设备绑定校验成功；
5. 业务请求成功；

认证中间件就在标准认证响应头中返回新的 access token。客户端原子替换本地旧 token。

`get_me` 只是没有其他业务请求时推荐使用的无副作用 bootstrap RPC，不是专用 Token API，也不是
唯一能返回 access token 的 RPC。

Bearer 请求不续期、不延长 token，也不在响应中隐式换取新 token。V1 不存在设备 refresh token、
`device_token_issue` 或 `device_token_refresh`。

`register` 是创建身份的特殊入口，继续通过既有响应字段直接返回首个 access token；后续 token
统一通过设备签名认证响应头取得。

### 7.2 Token 绑定

新版本设备 access token 至少绑定：

```text
did / sub
user_id
device_id
key_id
auth_generation
scopes
aud
iat / nbf / exp / jti
access-token profile / purpose / type
```

V1 scope：

- 所有 active 设备：`device:read`、`message:connect`；
- active member：额外具有只用于本机
  `device_root_import_complete` 的 `device:root-import-complete`；
- active ready admin：具有 `device:manage`，不再需要 root-import completion scope。

`device:root-import-complete` 不能批准、撤销或管理其他设备。旧 member token 也不会因为 Registry
角色变化而自动获得 `device:manage`。

### 7.3 Token 到期与重新认证

```text
有未过期且 scope 足够的 access token
  -> 继续 Bearer 业务请求

没有 token / token 到期 / 首次 401 / 已确认角色变化
  -> 清除当前 Bearer
  -> 使用本机 device signing key 对当前业务 RPC 签名
  -> 无待执行业务时调用 get_me
  -> 从成功响应头取得新 access token
  -> 原子替换本地 token
  -> 原业务最多重试一次
```

根私钥不参与日常登录。被撤销设备不能取得新 token。

V1 不新增 access-token blacklist 或 introspection，但保留现有服务已经执行的
Manifest、Registry、key、scope 和 `auth_generation` 当前状态检查。敏感写操作不能只相信 token
中的旧快照。

---

## 8. 通用 Message Service DID 系统通知

### 8.1 模块定位

系统通知是 Message Service 的通用能力，不是 Join 的私有旁路：

```text
可信 Producer 的业务事务
  -> System Notification Outbox / Notification Intent
  -> Message Service System Notification Sender
  -> 现有 P3 persist / Mailbox / sync / realtime
  -> 客户端 System Notification Dispatcher
```

V1 通知内核支持：

- 可信 producer；
- 稳定 `event_id` 和幂等重试；
- 目标用户 DID 与域内设备接收范围；
- `text/plain` 和 `application/json`；
- 标准 P3 `auth.origin_proof`；
- 现有 Message 持久化、Mailbox、同步和 realtime；
- TTL、离线投递和失败重试。

V1 内置业务类型至少包括 Join 系列。根导入完成不新增刷新通知；客户端只以新 access token、
Registry ready admin 和本地 active root 的收敛结果刷新状态。后续业务通过新的 versioned
payload type 接入同一发送器，不新建业务专用通知通道。

### 8.2 通知信任链

系统通知发送者使用目标用户当前根签名 DID Document 中
`ANPMessageService.serviceDid` 指向的 Message Service DID：

1. 客户端解析目标用户当前 DID Document；
2. 选择唯一合法的 `ANPMessageService`；
3. 要求通知 `meta.sender_did` 精确等于它的 `serviceDid`；
4. resolve `serviceDid` 并验证其 `authentication` key；
5. 验证 P3 `auth.origin_proof`；
6. 验证 payload type 在客户端允许表中，且 payload DID 等于本地身份。

P8 hop signature 不能替代 P3 Origin Proof。普通用户消息的 `meta.sender_did` 仍是业务用户 DID；
只有 Message Service 自己生成的系统通知才使用 Message Service DID 作为业务发送者。

域内 fan-out 可以选择全部 active 设备、全部 ready admin 或明确设备集合，但设备选择不写入
公开 P3 Base metadata，也不依赖 payload 中的 `recipient_device_id` 做真实路由。

系统通知不能携带 OTP/account token、Join token、Challenge 明文、SAS、共享秘密、root key 或
任何设备私钥。结构化 JSON 必须放在 `body.payload`，不能序列化后塞进 `body.text`。

---

## 9. 消息驱动的新设备 Join

### 9.1 传输边界

| 方向 | 传输 |
| --- | --- |
| 新设备 ↔ User Service | HTTPS JSON-RPC；新设备可以轮询自己的 Join Session |
| User Service → 旧 ready admin | 通用系统通知；旧设备不得轮询 Join 列表或状态 |
| 旧 ready admin → User Service | 用户明确操作触发的 Challenge、Approve、Reject RPC |
| 新设备 → DID Resolver | 最终独立解析和验证 |

新设备尚未拥有目标 DID 身份，因此 OTP 只用于定位账号和创建 Join Session，不能直接授权设备或
签发目标 DID 的 access token。

### 9.2 Join 主流程

```text
1. 新设备完成 OTP，选择已有 Handle/DID
2. 本地生成 device_id、signing/E2EE key 和临时 pairing key
3. 签署 Join Request，调用 device_join_create
4. User Service 保存 pending Session 并写 JoinRequested Outbox
5. 通用通知模块向所有 ready admin 投递 join-requested
6. 用户在一台旧设备点击“开始验证”
7. 旧设备生成加密 Challenge，并调用 device_join_submit_challenge
8. User Service 在一个 CAS 中同时 claim admin 和保存 Challenge
9. 新设备轮询 status，解密 Challenge、签署响应并本地计算 SAS
10. User Service 验证响应，并通知已认领旧设备显示 SAS
11. 用户人工比较两端独立计算的六位 SAS
12. 旧设备完成系统 user-presence，提交双签 DID Document 更新
13. User Service 原子提交 Document、Registry 和 Join consumed
14. 新设备轮询到 consumed 后重新 resolve DID Document
15. 新设备验证自己的 Manifest entry 和全部 key binding
16. 新设备用本机 signing key 发起 fresh DID-WBA 请求并取得 member token
```

SAS 不由服务器生成，也不能进入数据库、Outbox、Message、日志或通知预览。

### 9.3 Join 状态机

```text
pending
  |
  | first valid device_join_submit_challenge
  v
challenge_sent
  |
  | valid challenge response
  v
response_verified
  |
  | valid approve + Document/Registry CAS
  v
consumed
```

任一非终态可以进入：

```text
cancelled
rejected
expired
```

`consumed/cancelled/rejected/expired` 是不可逆服务端终态。客户端关闭页面不能代替取消；端侧只有
确认服务端终态或持久化了可重试的终态写意图后，才能删除候选私钥和 pairing secret。

### 9.4 Join 批准事务与结果

旧设备批准时：

- 当前设备必须仍是 ready admin；
- 管理员 Object Proof、Join Request Proof、response signature 和 DID root proof 全部有效；
- expected document/registry version 必须匹配；
- 新文档只新增请求中的 exact device 和 key；
- Registry row 由 User Service 从已验证文档和 Join Request 派生，客户端不提交 Registry。

一个 CAS 事务同时完成：

```text
DID Document + Manifest 更新
document checkpoint 更新
Registry 新增 active member
registry_version 递增
新设备 auth_generation = 1
Join Session -> consumed
JoinCompleted Outbox
```

无论用户是否选择“继续升级为管理员”，Join 的唯一结果都是：

```text
active + member + management_ready=false
root capability = absent
```

它已经可以通信，但不能管理设备。管理员升级只能在 Join 完成后进入第 10 章的独立流程。

---

## 10. 根密钥传输与管理员升级

### 10.1 状态边界

管理员升级不属于 Join CAS：

```text
active member
  -> 普通 P5 传输 RootKeyEnvelope
  -> 本地 pending root 导入
  -> HTTPS completion
  -> User Service 原子授予 ready admin
  -> 本地 root capability 提升为 active
  -> fresh DID-WBA 取得管理 access token
```

传输或 completion 失败不会回滚 Join；目标设备保持普通 member。

### 10.2 发送前检查与一次确认

旧 ready admin 在接触根私钥前必须：

1. 重新确认自己仍是 `active + admin + management_ready=true`；
2. 确认目标仍是 `active + member + management_ready=false`；
3. resolve 当前 DID Document，核对两端 exact device 和 key；
4. 检查已有 P5 Session，或获取并验证目标 PreKey Bundle；
5. 固定本次 `message_id`、目标设备和 Document/Registry checkpoint。

检查通过后，用户只明确确认一次本次根密钥传输。V1 不要求系统 PIN/生物识别，不要求第二次
确认，也不先发送空 Init。这一次确认必须绑定目标设备、key、`message_id`、checkpoint 和短时
有效期，不能复用为全局解锁状态。

### 10.3 复用普通 P5

根私钥是普通 P5 `Application Plaintext` 中的 `application/json` payload：

```text
已有 Session
  -> 标准 P5 Cipher 携带 RootKeyEnvelope

没有 Session
  -> 标准 P5 Init 的首个业务明文直接携带 RootKeyEnvelope
  -> 接收端按普通 P5 自动生成 Reply
```

Message Service 只处理普通 P5 exact-device opaque ciphertext 和可信路由 tuple，不解密
`system_type`，也不新增：

- root 专用 ANP Profile；
- `delivery_class`；
- 私有 completion sidecar；
- 第二套 Mailbox 或 Ratchet；
- 空 Init 后的第二次发送流程。

发送端在网络发送前原子保存 Ratchet/Session 状态和可精确重试的同一密文；一旦密文可能被接受，
重试必须复用同一 `message_id` 和密文字节，不能重新派生 message key。

### 10.4 接收与 pending Root Vault

新设备在普通聊天投影前识别合法的
`system_type=awiki.device.root-key-envelope.v1`，并执行：

1. 验证 P5 metadata、AAD、两端 DID/device 和 replay state；
2. 重新读取当前 DID Document 与 Registry；
3. 确认 sender 仍是 ready admin、recipient 仍是 member；
4. 核对两端 Manifest key、root key id、消息绑定和 expiry；
5. 严格解析规定格式的 Ed25519 PKCS#8 root private key；
6. 由私钥重算 root public key，并与当前 DID Document 完全比较；
7. 使用本机 KEK 把 root seal 到 pending Vault record；
8. 原子保存消息已消费标记和待提交 completion；
9. 立即清除 plaintext、root buffer 和临时序列化对象。

pending root 只能为本次 completion 生成 root possession proof，不能用于普通 DID 管理。
RootKeyEnvelope 永远不进入聊天时间线、通知预览、History、搜索、普通同步或普通备份。

### 10.5 独立 HTTP completion

新设备通过 `device_root_import_complete` 提交：

- active member access token 和专用 `device:root-import-complete` scope；
- importing device signing key 的外层 ANP Object Proof；
- pending root key 的内层 ANP Object Proof；
- 与 P5 `message_id` 相同的 `operation_id`；
- Message Service 已接受的 exact-device route 绑定。

User Service 必须重新验证：

- 当前 Manifest、Registry、key、角色和 readiness；
- sender 仍是 ready admin；
- recipient 仍是目标 active member；
- 两层 Object Proof；
- 普通 P5 可信路由 tuple；
- expiry 和幂等键。

一个数据库事务直接完成：

```text
target role = admin
target management_ready = true
target auth_generation += 1
registry_version += 1
保存 completion hash / 幂等结果
写无秘密 Outbox
```

不存在 `admin + management_ready=false` 中间状态。

客户端在响应后重新读取 Registry。只有确认远端 ready 后，才把本地 pending root 提升为 active
root ref；随后用本机 signing key 发起 fresh DID-WBA 请求，取得含 `device:manage` 和新
`auth_generation` 的 access token。

P5 accepted、本地 Vault 导入、completion HTTP 200 或系统通知本身都不能单独作为管理员事实。
最终事实只来自：

```text
Registry ready admin
AND
本地 active root 可读
```

V1 不返回旧设备的 E2EE imported ACK，不以 P5 Reply 表示导入成功，也不保留 completion sidecar
或根导入刷新通知。客户端只根据本机 completion 收敛流程刷新 UI。

---

## 11. 日常消息、Direct E2EE、MLS 与附件

### 11.1 普通消息与设备级安全 Overlay

普通 P3 Base 消息仍按业务 DID 发送，不把设备选择器扩散到所有消息。

需要设备级密码学的 P5 Direct E2EE：

- 从当前根签名 DID Document 的 Manifest 取得 eligible devices；
- 每个设备对拥有独立 PreKey、Session、Double Ratchet 和 replay state；
- 每个接收设备得到一份独立密文和 `message_id`；
- V1 逐设备调用 `direct.send`，不增加 `deliveries[]` 批量协议；
- `logical_message_id` 只用于客户端把多次设备投递聚合成一条业务消息；
- 一台设备失败不回滚其他已经成功的设备投递。

同一 DID 的自有设备也使用普通 P5 exact-device 会话同步已发送消息、附件引用和必要安全事件，
不能复制设备私钥、Ratchet State、消息密钥、MLS 私有状态或 DID root key。

### 11.2 MLS

群组业务成员仍按 DID 表达，但每台设备是独立 MLS Client/Leaf：

- 每台设备独立持有 KeyPackage、Leaf 私钥和 MLS 状态；
- 新设备通过群自身授权执行 Add/Commit/Welcome；
- 加入 Manifest 不等于自动进入全部历史群；
- 撤销设备后，各群异步执行 Remove/Commit；
- Remove/Commit 完成前，不能继续向仍包含待撤销 Leaf 的 epoch 发送新应用消息。

### 11.3 附件

附件对象只加密并上传一次：

- 私聊中，附件引用和同一个 `object_key` 经各设备独立 P5 Session 分发；
- 群聊中，附件引用和 `object_key` 放入当前 MLS Application Message；
- Object Service 只保存密文；
- `object_key` 不进入对象 URI、日志或通知预览；
- V1 不增加多设备专用 Download Ticket 或附件状态机。

---

## 12. 版本、事务与并发

V1 保留三个域内版本维度：

```text
DID Document record
  document_version
  document_hash

Device Registry
  registry_version

Device
  auth_generation
```

它们不进入普通跨域 P3 wire。客户端只 pin 已接受的最高
`document_version + document_hash`：

- 更低版本拒绝；
- 同版本同 hash 接受；
- 同版本不同 hash 触发安全错误并停止敏感操作；
- 更高版本只有在 root proof、DID 和 Manifest 全部有效时才接受。

核心状态转换：

| 操作 | Document | Registry | Device generation |
| --- | --- | --- | --- |
| Manifest 新注册 | 初始化为 1 | 初始化为 1 | 首设备初始化为 1 |
| Legacy 单设备升级 | version/hash 更新 | 初始化 | 首设备初始化为 1 |
| Join approve | version/hash 递增 | version 递增并新增 member | 新设备初始化为 1 |
| Root completion | 不修改 | version 递增并把 member 原子变为 ready admin | 目标设备递增 |
| Device revoke | 删除 Manifest entry 并递增 | version 递增并标记 revoked | 目标设备递增 |
| Fresh DID-WBA token | 不修改 | 不修改 | 读取当前值 |

User、Handle、DID checkpoint、Registry 和首设备注册必须是一个数据库事务。Join approve 中的
Document、Registry 和 Session consumed 必须是一个 CAS。Root completion 的角色、readiness、
generation 和幂等结果必须是一个事务。

Message 持久化、通知投递、Mailbox 清理和 MLS Remove 属于事务后的幂等收敛，不宣称跨服务全局
瞬时原子。

---

## 13. 撤销与账号安全底线

V1 把移除设备统一为永久 revoke：

```text
Registry -> revoked
DID Document / Manifest 删除设备和相关 key
document_version / registry_version 递增
目标 auth_generation 递增
阻止新 token、PreKey 和 KeyPackage
关闭或拒绝后续在线访问
删除尚未领取的未来 Mailbox 密文
各 MLS 群异步 Remove/Commit
```

设备管理、Join approve、root completion、token 签发等敏感操作必须读取当前
Manifest/Registry/key/role/readiness，不能只相信旧 token scope。

任何会撤销或禁用管理设备的事务都必须保证提交后仍至少存在一台
`active + admin + management_ready=true`。V1 不提供最后 ready admin 丢失后的 Recovery，因此
这一保护是必须保留的账号安全底线，不能为了简化流程而绕过。

撤销只能阻止生效后的未来访问，不能远程擦除设备已经获得的明文、密钥或历史数据。

---

## 14. 本地持久化与崩溃恢复

SecretVault 必须分别保存：

- DID root private key；
- device signing private key；
- device E2EE private key；
- access token；
- Direct PreKey/Session/Ratchet secret；
- 后续纳入保护范围的 MLS private state。

本地只允许为以下不确定窗口保存最小、加密、可恢复状态：

- `PendingRegistration`；
- `PendingLegacyUpgrade`；
- Join candidate 和终态写意图；
- P5 已持久化 ciphertext/retry state；
- `PendingRootImport` 与完全相同的 completion params/proofs。

原则：

1. 网络调用前先持久化会影响身份唯一性的密钥和 operation identity；
2. 远端结果不确定时复用同一请求或同一密文，不生成第二套密钥；
3. access token 只有一个当前值，不保存 refresh token；
4. RootKeyEnvelope 明文不落普通文件、日志、数据库业务表或常规备份；
5. pending root 与 active root 使用不同能力状态；
6. Registry 未 ready 时，pending root 不得被普通 root accessor 使用；
7. SecretVault 写入、Identity index 引用和消费标记必须有明确本地线性化点；
8. 已撤销、Vault 不可读或本地/远端 checkpoint 冲突时 fail closed。

SecretVault 的具体 record、锁和宿主 root-key 边界以
[Identity Secret Storage](../identity-secret-storage.md)为准。

---

## 15. 逻辑组件职责

| 组件 | V1 职责 | 不负责 |
| --- | --- | --- |
| AWiki Client / im-core | 密钥生成和 Vault、设备签名、Join/SAS、DID 验证、P5/MLS、本地状态机 | 自行写 Registry、相信通知即授权 |
| User Service | 注册事务、DID checkpoint、Registry、Join CAS、root completion、token issuer、Outbox | 保存用户私钥、发送私有通知协议 |
| Message Service | 通用系统通知、P3 persist/Mailbox/sync/realtime、P5 opaque route、可信 route tuple | 解密 P5、决定 admin 权限 |
| DID Resolver | 返回当前根签名 DID Document | 暴露 AWiki Registry |
| Handle/WNS | Handle 与当前 DID 映射 | 恢复或生成用户根私钥 |
| Remote Domain / Group Host | 按公开 DID/Manifest 验证跨域设备资格 | 读取 AWiki Registry 或本地 root 状态 |

逻辑职责不要求当前立刻拆成更多微服务。长期物理拆分时，仍必须保持同样的权威边界和幂等事务
语义。

---

## 16. 兼容边界与旧代码删除

V1 最终架构不允许新旧多设备流程长期并存。

必须删除的旧架构语义包括：

- 专用 `device_genesis`、Genesis grant 和首设备灰度分流；
- Join 后 `device_token_issue`；
- 设备 refresh token 与 `device_token_refresh`；
- 设备认证/注册的多层 rollout gate；
- 旧设备后台轮询 Join 列表或 Join 状态；
- 分离的 `device_join_claim` 与后续 Challenge 写；
- Join 直接创建 admin 或 `AdminAwaitingRoot`；
- root control `delivery_class`、私有 sidecar、空 Init、二次确认；
- E2EE imported ACK、ACK 驱动 readiness 和 completion tombstone；
- 把 `get_me` 描述成专用 Token 获取接口；
- V1 Recovery 的可执行入口或假流程。

仍保留的兼容范围只有：

1. 现有 `register` 对无 Manifest 的旧客户端继续走 Legacy adapter；
2. 原设备持有 Legacy root 时的一次性单设备升级；
3. 与多设备身份无关的旧消息兼容路径按各自迁移计划处理。

代码删除不是发布后的可选清理。修改一个模块时，可以先删后改、先改后删或交错进行，但该模块
退出对应实施步骤前，旧 RPC、DTO、配置、状态分支、存储字段、测试和文档必须一起删除。精确删除
顺序以 V1 总体执行计划为准。

---

## 17. 部署与演进边界

部署遵循依赖顺序：

```text
1. additive 数据库迁移
2. Message Service 先兼容设备 principal 和通用通知/P5 能力
3. User Service 上线统一 register、认证、Join 和 completion
4. im-core / AWiki Me 切换新正式身份和新流程
5. 跨服务与真实 App/CLI E2E 验证
6. 删除过渡代码、旧字段和旧测试
```

具体三步实施和每步退出门槛见 V1 总体执行计划。数据库列可以短期 additive，但运行时旧行为不能
无期限保留。

V1 上线期间允许把“User Service 不轮换 JWT signing key”设为明确的部署约束：在 signing key
保持不变、Message Service 仍配置对应验签公钥时，当前方案可以正常验签 access token，不阻塞
上线。这不代表 Message Service 已完整支持 JWT `kid`；在首次轮换 signing key 之前，必须先
实现并验证按 `kid` 选择多把/轮换公钥，否则新 key 签发的 token 会验签失败。

未来扩展必须遵守：

- 新通知业务通过新的 payload type 接入通用通知模块；
- 新设备状态或管理能力通过明确版本和迁移增加；
- Recovery 作为独立安全方案设计，不复用 Join 或 Legacy 升级；
- 新的跨域能力进入新的 ANP Profile/version；
- 新的本地恢复机制不能改变现有 DID/Manifest/Registry 的权威关系；
- 不兼容变更必须显式版本化，不能静默改变 V1 wire 或状态含义。

---

## 18. V1 完整流程摘要

```text
新用户：
  本地生成 root + device keys
  -> 含一个 bootstrap Manifest 的现有 register
  -> User/Handle/DID/Registry 一个事务
  -> 首设备成为 ready admin

Legacy 原设备：
  旧 key-1 作为 root
  -> 生成独立 device keys
  -> 同 DID/Handle 一次性升级为单设备 Manifest
  -> fresh device-signed request 取得 access token

日常认证：
  任一成功的 fresh DID-WBA User Service 请求
  -> 认证响应头返回新 access token
  -> get_me 只是无业务请求时的 bootstrap

加入设备：
  OTP 定位
  -> 通用系统通知驱动旧 admin
  -> 原子 claim + Challenge
  -> 两端独立 SAS
  -> 双签 Document/Registry CAS
  -> 新设备成为 member
  -> fresh device-signed request 取得 member token

升级管理员：
  ready admin 一次确认
  -> 普通 P5 Init/Cipher 携带 RootKeyEnvelope
  -> 新设备 pending Vault 导入
  -> 带设备 proof + root proof 的 HTTP completion
  -> Registry 原子变为 ready admin
  -> 本地 root 激活
  -> fresh device-signed request 取得管理 token

撤销设备：
  Manifest 删除 + Registry revoked + generation 递增
  -> Message/PreKey/KeyPackage/MLS 异步收敛
  -> 始终保留至少一台 ready admin
```

最终核心原则：

> V1 使用根签名 Manifest 表达公开设备资格，使用 Registry 表达域内授权，使用设备签名完成日常
> 认证，使用 SecretVault 保护本地能力。注册、Join、认证和管理员升级各自只有一个清晰入口；
> 高级能力以后通过版本化扩展增加，不在第一版保留并行旧架构。
