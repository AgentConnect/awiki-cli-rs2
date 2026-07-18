# Awiki / ANP 单 DID 多设备与端到端加密整体架构方案

**版本：最终修订稿**

---

## 1. 架构目标

本方案解决以下问题：

1. 一个用户使用一个 DID，并可在 PC、手机、平板等多个设备上同时登录；
2. 每台设备使用独立的签名密钥和端到端加密密钥；
3. 所有已授权设备均可独立收发私聊和群聊消息；
4. 任意一个在线的已授权设备，都可以批准新设备加入；
5. 私聊采用设备级独立会话；
6. 群聊采用 MLS，每台设备作为独立 MLS Client；
7. 附件只加密和上传一次，附件密钥通过 E2EE 消息安全分发。

本方案采用经过修改的 **模式 A**：

> DID Document 的修改仍然必须由 DID 根私钥签名，但根私钥在新设备成功加入后，通过设备间端到端加密通道同步到新设备。

因此，最终所有完全入网的设备都持有同一份 DID 根私钥。

---

# 2. 基本安全边界

首先明确：

> **DID Document 中只能保存公钥，不能保存任何私钥。**

密钥的保存位置如下：

| 密钥                | 公钥位置                   | 私钥位置          | 是否跨设备共享 |
| ----------------- | ---------------------- | ------------- | ------- |
| DID 根控制密钥         | DID Document           | 所有已完成根密钥同步的设备 | 是       |
| 设备签名密钥            | DID Document           | 对应设备本地        | 否       |
| 设备 E2EE 密钥        | DID Document           | 对应设备本地        | 否       |
| Direct PreKey     | PreKey Service         | 对应设备本地        | 否       |
| MLS KeyPackage 密钥 | MLS KeyPackage Service | 对应设备本地        | 否       |
| 附件对象密钥            | E2EE 内层消息              | 发送端及合法接收设备    | 每个附件独立  |

根私钥虽然会在多个设备之间同步，但：

* 不用于日常登录；
* 不用于普通消息签名；
* 不用于私聊密钥协商；
* 不用于 MLS 消息加密；
* 只用于修改和重新签署 DID Document。

---

# 3. 总体密钥模型

每个 DID 包含一个账户级根密钥，以及每台设备自己的两类长期密钥：

```text
DID
│
├─ DID 根控制密钥
│   ├─ K_root_public
│   └─ K_root_private
│       └─ 加密保存在所有完全入网的设备中
│
├─ Device A
│   ├─ K_sign_A
│   └─ K_e2ee_A
│
├─ Device B
│   ├─ K_sign_B
│   └─ K_e2ee_B
│
└─ Device C
    ├─ K_sign_C
    └─ K_e2ee_C
```

## 3.1 设备签名密钥

设备签名密钥用于：

* 登录挑战签名；
* API 请求签名；
* 证明请求来自某个具体设备；
* 签署 PreKey Bundle；
* 绑定 MLS KeyPackage；
* 对 DID Document 更新请求做设备级审计签名；
* 对设备间同步消息进行来源认证。

所有签名必须通过 `purpose` 或签名上下文做用途隔离，例如：

```text
awiki.login.v1
awiki.device.join.v1
awiki.did.update.v1
awiki.prekey.bundle.v1
awiki.mls.keypackage.v1
awiki.device.sync.v1
```

## 3.2 设备 E2EE 密钥

设备 E2EE 密钥用于：

* 私聊初始密钥协商；
* 设备间 E2EE 同步；
* 新设备配对；
* 根私钥同步；
* 附件密钥安全传输；
* 为动态 PreKey、一次性 PreKey 和其他会话材料建立信任绑定。

MLS 可以使用设备签名密钥绑定动态生成的 MLS KeyPackage，不要求把所有 MLS 动态公钥永久写入 DID Document。

---

# 4. DID Document 与 Device Manifest

## 4.1 DID Document 是公钥授权的权威来源

DID Document 继续保存：

* DID 根公钥；
* 各设备签名公钥；
* 各设备 E2EE 公钥；
* 各公钥的 verification relationship；
* 可选的 `deviceManifest` 扩展。

例如：

```json
{
  "id": "did:wba:example.com:user:alice",

  "verificationMethod": [
    {
      "id": "did:wba:example.com:user:alice#root",
      "type": "Multikey",
      "controller": "did:wba:example.com:user:alice",
      "publicKeyMultibase": "zRoot..."
    },
    {
      "id": "did:wba:example.com:user:alice#phone-sign",
      "type": "Multikey",
      "controller": "did:wba:example.com:user:alice",
      "publicKeyMultibase": "zPhoneSign..."
    },
    {
      "id": "did:wba:example.com:user:alice#phone-e2ee",
      "type": "Multikey",
      "controller": "did:wba:example.com:user:alice",
      "publicKeyMultibase": "zPhoneE2EE..."
    },
    {
      "id": "did:wba:example.com:user:alice#pc-sign",
      "type": "Multikey",
      "controller": "did:wba:example.com:user:alice",
      "publicKeyMultibase": "zPCSign..."
    },
    {
      "id": "did:wba:example.com:user:alice#pc-e2ee",
      "type": "Multikey",
      "controller": "did:wba:example.com:user:alice",
      "publicKeyMultibase": "zPCE2EE..."
    }
  ],

  "authentication": [
    "did:wba:example.com:user:alice#phone-sign",
    "did:wba:example.com:user:alice#pc-sign"
  ],

  "assertionMethod": [
    "did:wba:example.com:user:alice#root",
    "did:wba:example.com:user:alice#phone-sign",
    "did:wba:example.com:user:alice#pc-sign"
  ],

  "keyAgreement": [
    "did:wba:example.com:user:alice#phone-e2ee",
    "did:wba:example.com:user:alice#pc-e2ee"
  ],

  "deviceManifest": {
    "type": "ANPDeviceManifest",
    "version": "1.0",
    "epoch": "7",
    "devices": [
      {
        "device_id": "device-phone",
        "status": "active",
        "signing_key_id":
          "did:wba:example.com:user:alice#phone-sign",
        "e2ee_key_id":
          "did:wba:example.com:user:alice#phone-e2ee",
        "capabilities": [
          "request-signing",
          "direct-e2ee",
          "group-e2ee"
        ],
        "prekey_endpoint":
          "https://example.com/anp/prekeys/device-phone",
        "mls_key_package_endpoint":
          "https://example.com/anp/mls/device-phone"
      },
      {
        "device_id": "device-pc",
        "status": "active",
        "signing_key_id":
          "did:wba:example.com:user:alice#pc-sign",
        "e2ee_key_id":
          "did:wba:example.com:user:alice#pc-e2ee",
        "capabilities": [
          "request-signing",
          "direct-e2ee",
          "group-e2ee"
        ],
        "prekey_endpoint":
          "https://example.com/anp/prekeys/device-pc",
        "mls_key_package_endpoint":
          "https://example.com/anp/mls/device-pc"
      }
    ]
  }
}
```

## 4.2 Device Manifest 的定位

`Device Manifest` 是 DID Document 内的可选扩展，负责表达：

* 当前有哪些设备；
* 每个设备对应哪个签名公钥；
* 每个设备对应哪个 E2EE 公钥；
* 每个设备支持哪些 E2EE 能力；
* 每个设备的授权状态；
* PreKey 和 MLS KeyPackage 的查询位置；
* 当前设备集合版本。

它不保存：

* 根私钥；
* 设备私钥；
* Double Ratchet State；
* MLS Epoch Secret；
* 附件对象密钥；
* 实时在线状态。

## 4.3 Device Manifest 的启用规则

### 单设备

如果 DID 只有一个设备，可以不提供 Device Manifest。

通信方可以把唯一的：

* `authentication` 公钥；
* `keyAgreement` 公钥；

视为默认设备密钥。

### 多设备，但不使用 E2EE

如果用户有多台设备，但仅使用 HTTPS 或 `transport-protected` 消息，也可以不提供标准化 Device Manifest。

每个请求携带具体 `keyid`，服务端直接根据 DID Document 验证签名。

服务端内部仍然可以维护设备 ID 和推送关系，但不属于 ANP E2EE 互操作标准。

### 多设备 E2EE

如果一个 DID 有两个或以上有效 E2EE 设备，则必须提供 Device Manifest。

否则通信方无法可靠确定：

* 应向多少台设备发送；
* 哪个 PreKey Bundle 属于哪台设备；
* 哪些设备已经撤销；
* 哪个 MLS KeyPackage 对应哪个设备；
* 应向哪些设备 Mailbox 投递。

## 4.4 Manifest 验证规则

每个设备必须满足：

```text
signing_key_id 存在于 verificationMethod
signing_key_id 存在于 authentication
e2ee_key_id 存在于 verificationMethod
e2ee_key_id 存在于 keyAgreement
同一个 active key_id 只能属于一个 active device
device_id 在同一 DID 中唯一
```

Manifest 的 `epoch` 在每次设备增加、暂停、恢复或撤销时递增。

---

# 5. 首个设备与根密钥生成

## 5.1 首个设备不限定类型

首个设备可以是：

* PC；
* 手机；
* 平板；
* 其他支持 Awiki 的终端。

协议中不存在“手机一定是主设备”的要求。

首个设备称为：

```text
Bootstrap Device
```

## 5.2 首个设备初始化流程

首个设备完成手机号码或邮箱验证后：

1. 本地生成 DID 根密钥对；
2. 本地生成设备签名密钥对；
3. 本地生成设备 E2EE 密钥对；
4. 创建 DID；
5. 创建首个 DID Document；
6. 将根公钥和设备公钥写入 DID Document；
7. 使用根私钥签署 DID Document；
8. 把根私钥和设备私钥保存到本地安全存储；
9. 向服务端提交 DID Document。

此时：

```text
Bootstrap Device
  持有根私钥
  持有自己的设备签名私钥
  持有自己的设备 E2EE 私钥
```

---

# 6. 根私钥的本地保存

根私钥不能以普通文件、明文数据库或配置项方式保存。

每台设备应建立本地 `Root Key Vault`：

```text
K_root_private
    ↓ 使用设备本地 KEK 加密
Encrypted Root Key Blob
    ↓
设备本地安全存储
```

设备本地 KEK 应尽量由以下机制保护：

* Secure Enclave；
* Android Keystore / StrongBox；
* TPM；
* Windows Hello；
* 系统钥匙串；
* 本地用户登录凭据。

根密钥执行 DID 更新前，建议要求：

* 系统 PIN；
* 生物识别；
* 操作系统登录确认；
* 其他本地 user-presence 验证。

由于根私钥需要同步到其他设备，它不能被描述为“永远不可导出”。更准确的安全属性是：

> 根私钥平时以设备本地硬件绑定密钥加密保存，只在受控的签名或设备同步流程中短暂解封。

---

# 7. 新设备加入整体流程

新设备加入不依赖设备类型。以下流程同时适用于：

* 手机加入 PC；
* PC 加入手机；
* PC 加入另一个 PC；
* 平板加入 PC；
* 其他任意设备组合。

## 7.1 新设备进行手机号或邮箱验证

新设备首先通过手机号或邮箱完成服务端验证。

服务端根据验证信息：

* 查找对应用户账户；
* 定位已有 DID；
* 创建一次性加入会话；
* 将加入请求路由给已有设备。

需要明确：

> 手机号和邮箱验证只用于账户关联、反滥用和请求路由，不构成对 DID 控制权的密码学证明。

最终批准权仍然属于持有根私钥的已有设备。

## 7.2 新设备本地生成密钥

新设备本地生成：

```text
device_id_new
K_sign_new
K_e2ee_new
pairing_ephemeral_key
nonce
```

长期写入 DID Document 的公钥只有：

```text
设备签名公钥
设备 E2EE 公钥
```

临时配对公钥只在本次配对过程中使用，不写入 DID Document。

## 7.3 生成 6 位验证码

新设备生成或根据配对上下文派生一个 6 位数字验证码，例如：

```text
482917
```

验证码应绑定：

```text
pairing_session_id
new_device_id
new_device_signing_public_key
new_device_e2ee_public_key
nonce
```

验证码：

* 一次性使用；
* 有较短有效期；
* 加入完成后失效；
* 需要限制错误尝试次数；
* 不能作为 E2EE 密钥；
* 不能作为根密钥加密密钥。

其用途是让用户确认两个屏幕上的设备加入请求属于同一次操作。

## 7.4 新设备提交加入请求

加入请求至少包含：

```json
{
  "type": "awiki.device.join.request.v1",
  "did": "did:wba:...alice",
  "pairing_session_id": "pair-123",
  "verification_code": "482917",
  "new_device": {
    "device_id": "pc-2",
    "signing_public_key": "...",
    "e2ee_public_key": "...",
    "device_name": "Office PC",
    "device_type": "desktop",
    "capabilities": [
      "request-signing",
      "direct-e2ee",
      "group-e2ee"
    ]
  },
  "nonce": "...",
  "issued_at": "...",
  "expires_at": "...",
  "proof_of_possession": "..."
}
```

`proof_of_possession` 由新设备签名私钥生成，用于证明新设备确实持有与提交公钥对应的私钥。

## 7.5 加入请求的安全转发

加入请求由服务端转发给已有设备。

推荐将加入请求正文先通过已有设备的 E2EE 公钥加密。服务端只负责：

* 保存；
* 路由；
* 推送；
* 重试。

服务端不需要读取：

* 6 位验证码；
* 新设备公钥；
* 新设备详细信息；
* 用户批准结果中的敏感内容。

如果业务实现选择让服务器看到验证码，则该验证码只能用于会话匹配，不能被视为抵抗恶意服务端替换攻击的密码学证明。

## 7.6 老设备弹出确认窗口

老设备收到请求后，向用户展示：

```text
新设备名称
设备类型
大致登录位置
6 位验证码
签名公钥指纹
E2EE 公钥指纹
申请的设备能力
请求有效期
```

用户必须比较新旧设备屏幕上的 6 位验证码。

用户点击确认后，老设备验证：

1. 手机号或邮箱验证会话有效；
2. 加入请求未过期；
3. 新设备签名的持有证明有效；
4. 验证码与本次请求绑定；
5. 请求未被重复使用；
6. 新设备公钥未与现有设备冲突。

## 7.7 老设备更新 DID Document

用户确认后，老设备执行一次原子更新：

1. 将新设备签名公钥加入 `verificationMethod`；
2. 将新设备签名公钥加入 `authentication`；
3. 将新设备签名公钥加入 `assertionMethod`；
4. 将新设备 E2EE 公钥加入 `verificationMethod`；
5. 将新设备 E2EE 公钥加入 `keyAgreement`；
6. 在 Device Manifest 中增加新设备；
7. 增加 Manifest epoch；
8. 增加 DID Document version；
9. 使用本地根私钥重新签署 DID Document；
10. 向服务端提交更新。

## 7.8 DID Document 更新双证明

因为所有设备最终都会持有同一份根私钥，建议 Awiki 更新接口不仅验证根签名，还验证发起更新的设备签名。

更新请求应包含：

```json
{
  "expected_document_version": "18",
  "new_document": {},
  "root_proof": {},
  "authorizing_device_id": "device-pc-1",
  "authorizing_device_proof": {}
}
```

服务端验证：

```text
root_proof：
    新 DID Document 确实由根私钥签署

authorizing_device_proof：
    本次更新由当前 DID Document 中仍然 active 的设备发起
```

这样可以：

* 记录是哪台设备批准了新设备；
* 防止仅持有一份旧根密钥的已撤销设备继续更新；
* 增强共享根密钥模式下的设备撤销能力；
* 避免根签名无法区分设备来源的问题。

首个 DID Document 创建时，可以使用根签名和首设备持有证明作为特殊 genesis 流程。

## 7.9 服务端完成更新

服务端检查：

* 当前版本是否等于 `expected_document_version`；
* 根签名是否合法；
* 批准设备是否在旧版本中处于 active；
* 批准设备签名是否合法；
* Device Manifest 与 verification relationships 是否一致；
* 新设备持有证明是否合法；
* 加入会话是否仍然有效。

更新成功后：

1. 保存新的 DID Document；
2. 向新设备返回成功结果和新版本号；
3. 向所有已有设备广播设备增加事件；
4. 使加入会话失效。

## 7.10 新设备验证 DID Document

新设备收到成功响应后，必须重新通过标准 DID 解析入口获取最新 DID Document，并检查：

```text
自己的签名公钥已被加入 authentication
自己的 E2EE 公钥已被加入 keyAgreement
自己的 device_id 已出现在 Device Manifest
device status = active
Manifest epoch 和服务端响应一致
DID Document 根签名有效
```

完成上述验证后，新设备在身份授权层面加入成功。

---

# 8. 根私钥同步到新设备

## 8.1 同步时机

新设备的公钥正式进入 DID Document 后，已有设备立即通过设备间端到端加密通道发送根私钥。

顺序必须是：

```text
先授权新设备公钥
    ↓
新设备验证最新 DID Document
    ↓
再发送根私钥
```

不能在用户批准和 DID Document 更新完成前发送根私钥。

## 8.2 根密钥同步通道

同步通道可以使用：

* 配对过程中建立的临时 ECDH 会话；
* HPKE；
* 新设备 E2EE 公钥建立的加密信封；
* 后续已经建立的设备间 Direct E2EE 会话。

推荐使用配对临时密钥建立一次性会话密钥：

```text
K_pair = ECDH(
    old_device_ephemeral_private,
    new_device_ephemeral_public
)
```

然后使用 HKDF 派生根密钥包装密钥：

```text
K_wrap = HKDF(
    K_pair,
    "AWIKI-ROOT-KEY-SYNC-V1" ||
    did ||
    old_device_id ||
    new_device_id ||
    pairing_session_id
)
```

## 8.3 根密钥包

根密钥包在加密前应绑定：

```text
DID
根公钥 ID
根密钥算法
根私钥材料
DID Document version
Device Manifest epoch
发送设备 ID
接收设备 ID
pairing_session_id
issued_at
expires_at
随机 nonce
```

最终传输的是：

```json
{
  "type": "awiki.root-key-envelope.v1",
  "sender_device_id": "pc-1",
  "recipient_device_id": "pc-2",
  "pairing_session_id": "pair-123",
  "document_version": "19",
  "manifest_epoch": "8",
  "ciphertext": "...",
  "sender_device_proof": "..."
}
```

服务端只能看到加密信封，不能看到根私钥。

## 8.4 新设备接收和保存

新设备解密后必须：

1. 验证发送设备签名；
2. 验证发送设备仍然 active；
3. 验证包中的 DID 与当前 DID 一致；
4. 根据根私钥重新计算根公钥；
5. 检查计算出的根公钥与 DID Document 中的根公钥完全一致；
6. 检查 Document version 和 Manifest epoch；
7. 将根私钥重新使用本设备的硬件绑定 KEK 加密；
8. 清理内存中的明文根私钥；
9. 向老设备发送签名确认回执。

根密钥同步成功后，新设备具备：

* 修改 DID Document；
* 批准未来新设备；
* 撤销其他设备；

等 DID 管理能力。

## 8.5 加入成功和管理就绪

可以区分两个本地状态：

```text
authorized：
    设备已经写入 DID Document，可以登录和参与 E2EE

management-ready：
    设备已经安全接收根私钥，可以管理其他设备
```

用户界面可以在 DID Document 更新后显示“设备已加入”，但在根密钥同步完成前，不应允许该设备批准其他新设备。

## 8.6 根密钥同步失败

如果批准设备在 DID Document 更新后离线：

* 新设备仍然是已授权设备；
* 新设备可以向其他 active 设备发送 `root_key_sync_request`；
* 任何已经持有根私钥的 active 设备都可以完成同步；
* 服务端不能自行生成或恢复根私钥。

---

# 9. PC 与 PC 的关联

PC 与 PC 的关联使用与手机、PC 之间完全相同的流程，不要求摄像头或扫码。

可支持以下交互方式：

* 在 PC1 输入 PC2 显示的 6 位验证码；
* PC1、PC2 同时显示验证码并由用户比较；
* 二维码；
* 深度链接；
* USB；
* 局域网发现；
* 服务端中转。

密码学流程不变：

```text
手机号或邮箱关联
    ↓
PC2 生成设备密钥
    ↓
PC2 展示 6 位验证码
    ↓
加入请求由服务器转发给 PC1
    ↓
PC1 用户确认
    ↓
PC1 使用根私钥更新 DID Document
    ↓
PC2 验证 DID Document
    ↓
PC1 与 PC2 建立 E2EE 通道
    ↓
根私钥加密同步给 PC2
```

因此，移动设备不是协议中的必要角色。

---

# 10. 离线状态下的设备管理

## 10.1 离线不是授权状态

Device Manifest 只定义：

```text
active
suspended
revoked
```

不定义：

```text
online
offline
```

设备是否在线是服务端的临时连接状态，不应写入 DID Document。

对应关系如下：

| 连接状态    | 授权状态      | 行为                   |
| ------- | --------- | -------------------- |
| Offline | Active    | 仍是合法设备，消息可进入 Mailbox |
| Online  | Active    | 正常登录和收发消息            |
| Offline | Suspended | 不应产生新的 E2EE 投递       |
| Online  | Revoked   | 必须拒绝登录和消息操作          |

## 10.2 一个设备离线

如果 PC2 离线，但仍然 active：

* 其他设备正常工作；
* 给 PC2 的设备级密文进入 PC2 Mailbox；
* PC2 上线后拉取；
* PC2 不影响其他设备批准新设备。

## 10.3 至少一个设备在线

因为所有完全入网的设备都持有根私钥，只要有任意一个 active 设备在线，就可以：

* 批准新设备；
* 修改 DID Document；
* 撤销其他设备；
* 向新设备同步根私钥。

## 10.4 所有设备都离线

新设备可以完成：

* 手机号或邮箱验证；
* 创建加入会话；
* 提交加入请求。

但不能完成：

* DID Document 更新；
* 根签名；
* 根私钥同步。

请求只能在服务端保持 pending，等待任意已有设备上线。

服务端不得绕过已有设备批准流程。

## 10.5 新设备离线

如果新设备在授权后离线：

* 已有设备可以暂存一个短生命周期的根密钥加密信封；
* 根密钥信封到期后必须重新生成；
* 不建议服务器无限期保存根密钥包；
* 新设备重新上线后可以向任意 active 设备重新请求同步。

---

# 11. 并发更新控制

多个设备都持有根私钥后，可能同时更新 DID Document。

每次更新必须携带：

```text
expected_document_version
expected_document_hash
new_document_version
device_manifest_epoch
```

服务端使用 compare-and-swap：

```text
只有当前版本仍等于 expected_document_version，
才接受本次更新。
```

例如：

```text
PC1：18 → 19，成功
Phone：18 → 19，失败
```

Phone 必须重新拉取版本 19，合并自己的修改后提交：

```text
19 → 20
```

服务端不得自动覆盖新版本，也不得接受旧版本回滚。

---

# 12. 日常登录与请求签名

日常请求不使用根私钥。

请求携带：

```json
{
  "sender_did": "did:wba:...alice",
  "device_id": "device-pc",
  "keyid": "did:wba:...alice#pc-sign",
  "nonce": "...",
  "issued_at": "...",
  "expires_at": "...",
  "signature": "..."
}
```

服务端验证：

1. `keyid` 存在于 DID Document；
2. `keyid` 位于 `authentication`；
3. 如果存在 Device Manifest，则该 key 属于对应 `device_id`；
4. 设备状态为 active；
5. 请求签名有效；
6. nonce 未使用；
7. audience 和 purpose 正确；
8. 请求未过期。

验证通过后可以签发绑定设备的短期 access token。

因此，普通请求验证链仍然是：

```text
DID Document
    ↓
设备签名公钥
    ↓
请求签名
```

Device Manifest 仅用于设备映射和状态判断，不构成新的公钥授权链。

---

# 13. 私聊端到端加密

私聊采用 Signal/Sesame 风格的多设备模型。

## 13.1 每设备独立会话

假设 Bob 有三个设备：

```text
B1 Phone
B2 PC
B3 Tablet
```

Alice 的 A1 分别与其建立独立会话：

```text
A1 ↔ B1
A1 ↔ B2
A1 ↔ B3
```

每条会话拥有独立：

* PreKey Bundle；
* Session ID；
* Root Key；
* Sending Chain；
* Receiving Chain；
* Ratchet State；
* 消息计数器。

不同设备不能共享 Double Ratchet State。

## 13.2 一条消息，多份设备密文

Alice 发送一条逻辑消息时：

```text
Plaintext
  ├─ Session A1-B1 → Ciphertext B1
  ├─ Session A1-B2 → Ciphertext B2
  └─ Session A1-B3 → Ciphertext B3
```

可以通过一个批量请求提交：

```json
{
  "logical_message_id": "msg-123",
  "recipient_did": "did:wba:...bob",
  "device_manifest_epoch": "12",
  "deliveries": [
    {
      "recipient_device_id": "B1",
      "delivery_id": "delivery-B1",
      "ciphertext": "..."
    },
    {
      "recipient_device_id": "B2",
      "delivery_id": "delivery-B2",
      "ciphertext": "..."
    },
    {
      "recipient_device_id": "B3",
      "delivery_id": "delivery-B3",
      "ciphertext": "..."
    }
  ]
}
```

服务器拆分到：

```text
Mailbox B1
Mailbox B2
Mailbox B3
```

客户端界面只显示一条逻辑消息。

## 13.3 设备集合版本

发送时必须绑定：

```text
recipient_did
device_manifest_epoch
device_manifest_hash
```

如果服务端发现设备集合已变化，返回：

```text
device_set_changed
```

发送方重新拉取 DID Document 和 Device Manifest：

* 为新增设备补充加密；
* 停止向已撤销设备投递；
* 不重复发送已经成功的 delivery。

---

# 14. 自有设备之间的同步

发送者自己的其他设备也需要收到端到端加密副本。

例如 Alice 的 A1 给 Bob 发消息，而 Alice 还有 A2：

```text
A1 → Bob B1
A1 → Bob B2
A1 → Bob B3
A1 → Alice A2
```

A1 和 A2 之间同样建立独立设备级 E2EE 会话。

设备同步内容可以包括：

* 自己发送的消息；
* 已读状态；
* 删除和撤回；
* 表情回应；
* 会话设置；
* 联系人和群组状态；
* 附件引用和附件密钥。

不能同步：

* 设备签名私钥；
* 设备 E2EE 私钥；
* Double Ratchet State；
* MLS Leaf 私钥。

根私钥同步是一个专门的设备入网流程，不等同于普通消息同步。

---

# 15. 群聊端到端加密

群聊采用 MLS。

## 15.1 业务成员与密码学 Client 分离

业务成员仍然按 DID 表达：

```text
Alice DID
Bob DID
Carol DID
```

每台设备作为独立 MLS Client/Leaf：

```text
Alice DID
  ├─ Alice Phone → Leaf 3
  └─ Alice PC    → Leaf 8

Bob DID
  ├─ Bob Phone   → Leaf 5
  └─ Bob PC      → Leaf 12
```

同一 DID 可以对应多个 MLS Leaf。

## 15.2 群消息发送

发送设备生成一份 MLS Application Ciphertext：

```text
Plaintext
    ↓ MLS Encrypt
一个 MLS Ciphertext
    ↓
广播给群组当前所有 Leaf
```

群聊不需要像私聊一样针对每台设备分别生成完整消息密文。

但每台设备仍拥有独立：

* MLS Leaf；
* MLS 签名状态；
* KeyPackage；
* Leaf Index；
* MLS 本地状态。

## 15.3 新设备加入已有群组

新设备完成 DID 入网后，需要对用户所在的每个活跃群组执行：

```text
MLS Add New Device KeyPackage
Commit
Epoch N → N+1
Welcome → New Device
```

这不会在业务层重复增加同一个 DID，只增加一个新的密码学 Leaf。

## 15.4 设备撤销

撤销某台设备时，对每个群组执行：

```text
MLS Remove Device Leaf
Commit
Epoch N → N+1
```

只有新 epoch 生效后，被撤销设备才无法解密未来群消息。

新设备默认无法解密加入前的历史 MLS 消息。历史记录由单独的端到端加密迁移或备份机制处理。

---

# 16. 附件传输

## 16.1 附件对象只加密一次

发送端为每个附件生成：

```text
随机 object_key
随机 nonce
```

然后：

```text
附件明文
    ↓ Object-Level Encryption
附件密文
    ↓
上传一次到 Object Service
```

无论接收方有多少设备，附件本体都不需要重复加密或上传。

## 16.2 私聊附件

Bob 有三个设备时：

```text
一个附件密文
一个 object_uri
一个 object_key
```

但附件 Manifest 需要分别通过三条设备会话交付：

```text
A1 ↔ B1：
  object_uri + object_key + nonce

A1 ↔ B2：
  object_uri + object_key + nonce

A1 ↔ B3：
  object_uri + object_key + nonce
```

因此：

> 附件内容只加密、上传一次；附件密钥通过每台设备的独立 E2EE 会话分别安全交付。

## 16.3 群聊附件

群聊中：

```text
附件加密、上传一次
    ↓
object_key 放入 MLS 加密的 Attachment Manifest
    ↓
一份 MLS Ciphertext 广播给所有设备
```

群聊不需要为每个设备单独包装附件密钥。

## 16.4 Download Ticket

Download Ticket 只负责授权下载附件密文，不等同于附件解密密钥：

```text
Download Ticket：
    允许设备从对象服务下载密文

object_key：
    允许设备在本地解密密文
```

服务端不能通过 Download Ticket 获得附件明文。

---

# 17. 设备撤销与丢失处理

## 17.1 正常移除设备

正常移除时：

1. 从 `authentication` 删除设备签名公钥；
2. 从 `keyAgreement` 删除设备 E2EE 公钥；
3. 在 Device Manifest 中将设备标记为 revoked 或移除；
4. 递增 Manifest epoch；
5. 使用根私钥重新签署 DID Document；
6. 使设备 access token 和 refresh token 失效；
7. 停止 PreKey 和 KeyPackage 发布；
8. 删除尚未领取的设备 Mailbox；
9. 从所有 MLS 群组移除该设备 Leaf；
10. 要求被移除设备安全删除根私钥和其他本地密钥。

## 17.2 丢失或被盗设备

因为所有完全入网设备都持有根私钥，丢失设备与普通设备密钥丢失不同。

即使从 DID Document 中删除其设备签名公钥和 E2EE 公钥，该设备可能仍然保留根私钥副本。

因此建议 Awiki DID 更新接口强制要求：

```text
根私钥签名
+
当前 active 设备签名
```

已撤销设备的设备签名公钥不再 active，因此即使保留旧根私钥，也不能再通过 Awiki 服务更新 DID Document。

但仍需要明确：

> 如果怀疑根私钥已经被攻击者从设备中提取，仅撤销设备公钥并不能从密码学上证明根私钥已经安全。

这种情况下必须执行：

* 根密钥轮换；或者
* 创建新 DID 并迁移 Handle/WNS 指向。

如果 DID 标识本身绑定根公钥，根密钥轮换可能导致 DID 改变。

## 17.3 撤销只能保护未来数据

撤销设备可以阻止其：

* 登录；
* 获取未来私聊密文；
* 获取新的附件密钥；
* 解密新的 MLS epoch；
* 发布新的 PreKey；
* 获取新的 Download Ticket。

但不能删除该设备已经获得的：

* 历史消息明文；
* 历史附件；
* 历史会话密钥；
* 用户主动导出的数据。

---

# 18. 所有设备不可用时的恢复

根私钥多端同步解决的是：

```text
只要还有一台设备可用，
就能继续管理 DID。
```

但如果所有设备都不可用：

* 无法批准新设备；
* 无法修改 DID Document；
* 无法撤销旧设备；
* 无法证明对原 DID 的控制。

因此可以增加可选恢复机制：

```text
根私钥
    ↓ 使用随机恢复密钥加密
Encrypted Root Recovery Package
    ↓
服务端或用户存储
```

恢复密钥可以保存为：

* 恢复二维码；
* 高熵恢复词；
* 离线恢复文件；
* 多份秘密分片；
* 外部硬件。

如果没有任何恢复包，且所有设备均不可用，则原 DID 无法恢复，只能创建新 DID。

服务端不得仅凭手机号或邮箱验证直接重新生成根私钥或绕过 DID 控制。

---

# 19. 服务端职责与信任边界

## 19.1 服务端可以做什么

服务端负责：

* 手机号或邮箱验证；
* 关联加入请求与已有账户；
* 路由设备加入请求；
* 保存和发布 DID Document；
* 对 DID Document 更新做版本控制；
* 验证根签名和设备签名；
* 维护设备 Mailbox；
* 路由 E2EE 密文；
* 保存附件密文；
* 发放 Download Ticket；
* 使设备 token 失效；
* 广播设备增加、删除和安全事件。

## 19.2 服务端不能做什么

服务端不能：

* 生成用户根私钥；
* 获得根私钥明文；
* 替用户批准新设备；
* 修改设备公钥；
* 解密设备加入请求；
* 解密根密钥包；
* 解密私聊消息；
* 解密 MLS 群消息；
* 解密 E2EE 附件；
* 在所有设备离线时自行添加设备。

手机号或邮箱验证只是一层中心化账户验证，不替代 DID 根签名。

---

# 20. 必须接受的安全权衡

根私钥多端同步显著提高了可用性：

```text
任意一个在线设备
    都可以管理 DID 和批准新设备
```

但同时扩大了根密钥攻击面：

```text
任意一台设备被完全攻破
    都可能导致根私钥泄露
```

因此本方案的安全属性是：

> **DID 根密钥的安全强度取决于所有持有根私钥设备中安全性最弱的一台。**

为降低风险，必须实施：

1. 根私钥使用设备本地硬件绑定密钥加密；
2. 根私钥不进入普通日志、数据库导出、剪贴板或文件分享；
3. DID 更新操作要求本地用户确认；
4. 根密钥同步必须使用 E2EE 通道；
5. 根密钥包必须绑定发送和接收设备；
6. 每次设备增加和根密钥同步通知所有已有设备；
7. DID 更新同时要求根签名和当前 active 设备签名；
8. 丢失设备疑似泄露根密钥时执行根密钥轮换或 DID 迁移；
9. 6 位验证码只能用于人工确认，不能用于加密根密钥；
10. 所有设备使用不透明随机 `device_id`，不在公共 DID Document 中暴露设备序列号等敏感信息。

---

# 21. 最终架构摘要

```text
用户身份：
    一个 DID

DID Document：
    保存根公钥和所有设备公钥
    保存可选 Device Manifest
    不保存任何私钥

首个设备：
    可以是 PC 或手机
    生成 DID 根密钥和首个设备密钥
    本地保存根私钥

后续设备：
    验证手机号或邮箱
    生成自己的签名密钥和 E2EE 密钥
    显示 6 位验证码
    通过服务器向已有设备提交加入请求
    已有设备经用户确认后使用根私钥更新 DID Document

根私钥同步：
    新设备正式写入 DID Document 后
    已有设备通过设备间 E2EE 通道发送根私钥
    新设备使用本地硬件密钥加密保存
    最终所有完全入网设备均持有根私钥

离线管理：
    任意一个 active 设备在线即可批准新设备
    所有设备离线时只能等待或使用恢复机制
    离线 active 设备继续拥有独立 Mailbox

日常请求：
    使用对应设备签名私钥
    不使用根私钥

私聊：
    每个设备对维护独立 Double Ratchet
    一条逻辑消息生成多份设备密文
    一次批量提交，服务器按设备 Mailbox 拆分

自有设备同步：
    设备之间建立独立 E2EE 会话
    不共享设备私钥和 Ratchet State

群聊：
    每台设备作为独立 MLS Client/Leaf
    每条群消息通常只产生一份 MLS 密文

附件：
    附件本体加密、上传一次
    私聊中附件密钥按设备分别交付
    群聊中附件密钥通过 MLS 统一交付

设备撤销：
    删除设备公钥授权
    失效登录会话和 Mailbox
    移除 MLS Leaf
    疑似根密钥泄露时必须轮换根密钥或迁移 DID
```

该方案最终形成以下分层：

```text
DID
    = 长期用户身份

DID 根密钥
    = DID Document 管理权限
    = 在所有完全入网设备间安全同步

设备签名密钥
    = 具体设备的请求与行为身份

设备 E2EE 密钥
    = 具体设备的加密通信端点

Device Manifest
    = 设备与 DID 公钥之间的可选映射
    = 多设备 E2EE 场景下必须存在

Direct E2EE
    = 每个设备对独立 Ratchet

Group E2EE
    = 每台设备一个 MLS Leaf

Attachment E2EE
    = 对象加密一次，密钥通过消息层分发
```
