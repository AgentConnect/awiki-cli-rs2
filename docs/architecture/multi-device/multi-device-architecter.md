# AWiki 多设备与 ANP vNext 第一阶段架构方案

**版本：V1 简化方案（可信服务基线）**

> 本文描述 AWiki 第一阶段多设备目标架构，并明确 ANP vNext 需要增加的设备级协议能力。它不是当前代码实现清单，也不包含字段级 JSON Schema、完整错误码、测试向量、迁移计划，以及长期高安全版本的透明日志和强分叉检测。

---

## 1. 文档定位、架构目标与阶段边界

### 1.1 文档定位与适用范围

本文同时承担两项职责：

1. 定义 AWiki 产品内部如何管理一个 DID 下的多台设备；
2. 定义为实现跨域多设备互操作，ANP vNext 在 Identity、Direct E2EE、MLS 和 Federation 层需要增加的公开协议语义。

本文确定架构边界和第一阶段主流程，不把尚未注册的字段或占位 Profile 描述成当前 ANP 已有能力。正式 wire schema、Profile ID、错误码和一致性测试向量仍需落入 ANP 权威规范。


本方案只管理：

> 某个租户与身份作用域下的一个用户自持 Credential DID。

它不假设 Account、Persona、手机号、邮箱、Handle 与 DID 一一对应。一个账户可以拥有多个 Handle、Credential DID 或 Agent DID；完成 OTP 后仍必须由用户明确选择目标 Handle/DID，不能由手机号或邮箱自动推导唯一 DID。

| 概念 | 在本方案中的含义 |
| --- | --- |
| Account / OTP | AWiki 服务入口、账户定位与反滥用手段，不是 DID 控制证明 |
| Handle | 人类可读入口，通过 Handle/WNS 指向当前 DID |
| Credential DID | 多台设备共享的逻辑密码学身份 |
| `device_id` | 该 DID 下的独立密码学通信端点，不是新的业务身份 |

服务托管的 Agent DID、群组身份或其他非用户自持身份可以复用设备级 ANP 能力，但不能直接继承本文的 member/admin、根私钥传输和 Handle 恢复规则。

### 1.2 架构目标

第一阶段需要同时达到以下目标：

1. 一个用户自持 DID 可以在 PC、手机、平板等多种终端上使用，不预设手机是主设备；
2. 每台设备拥有独立的设备签名密钥、E2EE 密钥、Direct Ratchet State 和 MLS 私有状态；
3. DID Document 内嵌 `deviceManifest`，一次解析即可获得当前合法设备、公钥引用和跨域通信能力；
4. 普通设备可以登录和通信，但不持有 DID 根私钥，也不能管理其他设备；
5. 只有用户明确授权且已完成根密钥安全落库的管理设备可以批准或撤销设备；
6. Direct E2EE 按设备对建立独立会话，不能在多台设备之间复制 Ratchet State；
7. 同一 DID 的不同设备可以作为独立 MLS Client，但只有完成目标群的 Add/Commit 后才成为该群 Leaf；
8. 设备离线、永久撤销、设备丢失和全部管理设备不可用时都有单一、可解释的处理路径；
9. AWiki 内部设备角色不泄露为跨域 ANP 权限，AWiki admin 也不自动获得群管理权；
10. 附件继续采用“对象只加密上传一次，密钥通过 Direct E2EE 或 MLS 分发”的独立方案。

### 1.3 核心设计原则

方案首先坚持三个分离：

```text
设备加入权 ≠ 消息通信权 ≠ DID 管理权
```

```text
跨域 ANP 通信资格 ≠ AWiki 域内管理权限
```

```text
服务端记录的管理授权 ≠ 设备已经安全保存根私钥
```

进一步得到五项基本决策：

* DID 是多设备共享的逻辑身份，`device_id` 只是该身份下的密码学端点；
* 后续设备默认作为普通设备加入，根私钥默认不传输；
* DID 根私钥只用于 DID Document 和设备管理更新，不参与日常登录、Direct 密钥协商或 MLS 消息加密；
* 设备加入成功不等于获得 `role = admin`，获得 admin 授权也不等于已经管理就绪；
* 公开通信资格、AWiki 域内授权和本地私钥可用性由不同权威对象表达，不能互相推导。

普通设备和管理设备具有不同攻击面：普通设备被攻破主要影响该设备的请求身份、消息和本地数据；管理设备被攻破还可能泄露 DID 根私钥。因此后续设备默认是普通设备，每增加一台管理设备都会增加一份根私钥副本，用户必须明确授权。

### 1.4 第一阶段产品主路径

第一阶段只实现一条清晰主路径：

```text
AWiki 新建用户自持 DID
    始终携带 deviceManifest
    每台设备始终拥有 device_id
    每台设备使用独立签名密钥和 E2EE 密钥
    Direct 始终按设备建立会话和投递
    普通设备与管理设备只在 AWiki 域内区分
    根私钥只传给用户明确授权的新管理设备
```

旧 DID、无 Manifest DID 和旧 Profile 不进入 V1 Core 状态机，而由 Legacy Adapter 隔离处理。第一阶段不维护多套核心兼容分支。

### 1.5 第一阶段可信假设

第一阶段信任 AWiki Identity、Message 和 Handle/WNS 服务按照协议维护唯一当前状态，并使用数据库事务和 CAS 防止并发覆盖。

第一阶段仍防护：

* 网络窃听和篡改；
* Message Plane 读取 E2EE 明文；
* 未持有设备私钥的攻击者伪造设备；
* 服务端或中间人替换加入请求中的设备公钥；
* 丢失、被盗或被撤销设备继续获得未来数据；
* OTP 重放、加入会话重放和普通并发更新冲突。

第一阶段暂不防护 Identity 或 Handle 服务主动制造多份合法视图、长期回放旧状态或永久隔离不同设备。此类 Byzantine/split-view 防护属于后续安全增强。

### 1.6 第一阶段主动删除的复杂度

第一阶段不要求：

* `previous_document_hash`、`previous_registry_hash` 或 `previous_mapping_hash`；
* 保存完整历史 transition proof；
* 离线跨多个版本时补齐全部中间证明；
* transparency log、witness、设备间 checkpoint gossip；
* 复杂 fork reconciliation；
* 同 DID 换根恢复；
* Manifest 从有到无的核心协议分支；
* 根私钥传输的私有外层 Profile 和双 ACK；
* Direct 批量投递与部分成功聚合协议；
* 暂停/重新激活、管理员降级等非必要状态机。

---

## 2. 整体方案与架构分层

### 2.1 整体方案

AWiki 采用“共享 DID + 独立设备端点 + 公开设备投影 + 域内设备控制”的方案：

1. 用户通过 Account/OTP 进入 AWiki，并明确选择 Handle 和目标 Credential DID；
2. AWiki Identity Control 维护域内 Device Registry，决定设备是否 active、是 member 还是 admin，并记录服务端是否已确认首设备 bootstrap 就绪或验证后续设备的签名 imported ACK；
3. 需要跨域公开的最小设备集合被投影到根签名的 DID Document 内嵌 `deviceManifest`；
4. 远端 ANP Domain 只依据 DID Document、设备公钥和协商后的 Profile 判断通信资格；
5. Message Plane 按 `DID + device_id` 路由 PreKey、Mailbox、Direct/MLS 密文，但不持有消息明文或用户私钥；
6. 每台客户端在本地保存自己的私钥、Root Key Vault、Double Ratchet 和 MLS 私有状态。

整体关系如下：

```text
Account / OTP
      ↓ 明确选择
Handle / WNS ───────────────→ 当前 Credential DID
                                  │
                       AWiki Identity Control
                                  │
                    ┌─────────────┴─────────────┐
                    │                           │
          AWiki Device Registry      DID Document + deviceManifest
          域内角色与管理资格          跨域设备公钥与通信资格
                    │                           │
                    │                           └──→ Remote ANP Domain / Group Host
                    │
                    └──→ Message Plane ──→ 每设备 PreKey / Mailbox / 密文路由

AWiki Client 本地：设备私钥 / Root Key Vault / Ratchet / MLS State
```

### 2.2 三层架构边界

```text
┌────────────────── 设备本地安全层 ──────────────────┐
│ 设备私钥、Root Key Vault、Ratchet/MLS 私有状态      │
└────────────────────────┬───────────────────────────┘
                         │
┌────────────────── AWiki 域内控制层 ─────────────────┐
│ 设备加入、Device Registry、member/admin、token      │
│ 根私钥传输、撤销、Handle 恢复                       │
└────────────────────────┬───────────────────────────┘
                         │
┌────────────────── 跨域公开协议层 ───────────────────┐
│ Handle/WNS、DID Document、deviceManifest            │
│ 设备公钥、Direct E2EE、MLS                           │
└────────────────────────┬───────────────────────────┘
                         │
                 Remote ANP Domain / Group Host
```

| 层次 | 权威内容 | 不得承载 |
| --- | --- | --- |
| 跨域公开协议层 | Handle → DID、公开设备通信资格、设备公钥、Direct/MLS 能力 | AWiki 管理角色、token、根私钥导入状态 |
| AWiki 域内控制层 | 设备生命周期、member/admin、授权、撤销、恢复 | 用户私钥、Ratchet/MLS 私有状态 |
| 设备本地安全层 | 私钥、KEK、根私钥安全落库、E2EE/MLS 私有状态 | 跨域公开授权真值 |

### 2.3 四类权威对象

| 对象 | 负责回答 | 不负责回答 |
| --- | --- | --- |
| Handle/WNS | 当前人类可读 Handle 指向哪个 DID | 用户是否仍持有旧根私钥 |
| DID Document + `deviceManifest` | 哪些设备是当前合法的跨域密码学端点 | 设备在 AWiki 中是 member 还是 admin |
| AWiki Device Registry | 哪些设备可以在 AWiki 域内登录、被投递或管理身份，以及服务端是否已确认 bootstrap 就绪或验证签名 imported ACK | 私钥是否仍物理存在于设备中 |
| Local Device State | 本设备是否实际持有私钥并具备执行能力 | 其他设备的公开授权资格 |

对象关系固定为：

```text
AWiki Device Registry
    产生当前可跨域通信设备的最小公开投影
        ↓ 根签名发布
DID Document.deviceManifest
        ↓ 远端解析与验证
跨域 Direct / MLS 设备资格

Local Device State
    独立决定本设备是否真的持有密钥和 management-ready
```

因此，公钥出现在 Manifest 中只代表它可以成为通信端点；Registry 中出现 `role = admin` 只代表域内授权。Registry 的 `management_ready = true` 只记录服务端已经确认 bootstrap 就绪或验证 imported ACK；Local Device State 才决定根私钥当前是否真实可用。执行管理操作时两者都必须满足。

### 2.4 端到端生命周期

```text
首次创建
    首设备生成根密钥和设备密钥，创建带 Manifest 的新 DID
        ↓
添加普通设备
    OTP 定位 + 私钥持有证明 + ECDH/SAS + 旧管理设备双签批准
        ↓
添加管理设备
    先完成设备加入，再通过 Direct E2EE 导入根私钥并确认管理就绪
        ↓
日常通信
    Direct 按设备建立独立会话，MLS 按群组规则增加独立设备 Leaf
        ↓
设备撤销
    Registry 撤销 + DID Document 删除公开资格 + Message/MLS 异步收敛
        ↓
全部管理设备不可用
    恢复 Handle、生成新根密钥并创建新的 e1_ DID
```

上图只描述生命周期主线。具体加入、根密钥传输、消息投递、撤销和恢复分别见第 8、9、11、13、14 章。

### 2.5 ANP vNext 与 AWiki 域内能力边界

本文中的“ANP 多设备能力”是多个版本化 Profile 的架构统称，不是可以直接写入 wire 的单一 Profile ID。

ANP vNext 需要公开定义：

* DID Document 顶层 `deviceManifest` 扩展及验证规则；
* Direct 的发送/接收 `device_id` 和设备级投递语义；
* PreKey、Session、Ratchet、AAD 与 Mailbox 的设备绑定；
* 同一 DID 多个 MLS Client/Leaf 的资格绑定；
* Federation 对设备寻址、文档刷新和 stale device set 的处理。

AWiki 域内继续负责：

* member/admin、`management_ready` 和 Device Registry；
* 设备 token、加入/恢复会话和用户确认；
* 根私钥控制消息与 imported ACK；
* Handle 恢复、通知和冷静期。

远端 ANP 实现只需要知道某个 `device_id` 是否是当前合法通信端点，不需要理解 AWiki 管理角色。对端不支持所需 ANP vNext 能力时，不得静默退化为共享 Ratchet、共享 MLS 私钥或明文；旧版兼容由第 6.2 节的 Legacy Adapter 单独处理。

### 2.6 两条授权验证链

跨域通信与 AWiki 域内 API 使用不同的授权链：

```text
跨域 ANP 通信：
    DID Document 根 proof
        + deviceManifest 中的设备资格与 Profile
        + 目标 Profile 要求的设备 key / 请求 purpose
        + 设备签名或 E2EE 会话认证
```

```text
AWiki 域内 API：
    当前设备公钥
        + Device Registry 的 active
        + 绑定 device_id 的 token
        + 当前 auth_generation

设备管理操作再增加：
    role = admin
        + management_ready = true
        + 本地根私钥当前可用
        + 本地 user-presence
        + 当前管理设备签名
        + DID 根签名
```

公开 Manifest 资格不能推导 `role = admin`。Group Host 还必须叠加目标群自己的成员和 Add/Remove 规则，不能因为设备是 AWiki admin 就授予群管理权。

### 2.7 核心架构不变量

第一阶段后续所有协议和实现都必须保持以下不变量：

1. 新建 AWiki DID 从 genesis 起始终携带完整 `deviceManifest`；
2. `deviceManifest` 是 DID Document 内嵌扩展，不存在独立 Manifest HTTP 状态源；
3. Manifest 只表达跨域通信资格，Registry 才表达 AWiki 域内角色和能力；
4. 设备签名/E2EE 私钥、Ratchet State 和 MLS 私有状态永不在设备间复制；
5. 根私钥默认不传输，只能发给用户明确授权的待就绪管理设备；
6. Message Plane 只处理授权元数据和密文，不能解密聊天或根私钥控制 JSON；
7. AWiki admin 不自动拥有跨域群管理权，MLS Leaf 仍由目标群规则授权；
8. Handle 恢复只恢复人类可读入口，并创建新的密码学 DID，不能声称找回旧根私钥。
9. `device_id` 必须由密码学安全随机源生成且保持不透明，不能直接使用或可逆编码硬件序列号、IMEI、MAC 地址等可追踪标识。
10. DID Document 和 Device Registry 只能保存公钥、引用和授权状态，绝不能保存任何设备私钥或 DID 根私钥。
11. DID 根控制私钥是身份管理授权密钥；设备本地 KEK 只用于加密保护 Root Key Vault 中的根私钥密文，不能替代根私钥签署 DID 更新。

---

## 3. 设备角色与密钥

### 3.1 普通设备

普通设备可以：

* 登录并签署日常请求；
* 进行设备级 Direct E2EE；
* 作为独立设备参与 MLS；
* 接收自己的设备 Mailbox。

普通设备不持有 DID 根私钥，不能批准或撤销其他设备。

### 3.2 管理设备

Registry 可以先授予 `role = admin`，再等待根私钥安全导入。为避免把“已授权”误写成“已就绪”，本文区分：

```text
待就绪管理设备
    active + admin + management_ready=false
    已获授权并可以通信，但不能执行设备管理

管理设备
    active + admin + management_ready=true
    根私钥已经在本地生成或安全导入，可以执行设备管理
```

管理设备具备普通设备的全部能力，并且：

* 在 AWiki Device Registry 中的 `role = admin`；
* `management_ready = true`；
* 本地使用 KEK 安全保存 DID 根私钥；
* 可以在本地 user-presence 确认后批准或撤销设备。

V1 不再维护独立的 `device-admin` capability 字段：`role = admin` 表示域内管理授权，`management_ready = true` 表示服务端已确认首设备 bootstrap 就绪或后续设备根密钥导入；两者和本地根私钥可用性共同决定设备能否执行管理操作。

首个设备默认是管理设备。后续设备默认作为普通设备加入；只有用户明确选择“允许此设备管理其他设备”后，才进入根私钥传输流程。

### 3.3 密钥模型

| 密钥 | 用途 | 私钥保存位置 |
| --- | --- | --- |
| DID 根控制密钥 | 签署 DID Document 和设备管理更新 | 已就绪管理设备本地 |
| 设备签名密钥 | 登录、请求签名、Join Request、ACK | 对应设备本地 |
| 设备 E2EE 长期密钥 | Direct 初始建链、加密 challenge | 对应设备本地 |
| 配对临时密钥 | 本次加入 ECDH 和 SAS | 本次加入会话内存 |
| Direct PreKey | 建立设备级 Direct 会话 | 对应设备本地 |
| MLS KeyPackage 私钥 | 建立设备 MLS Leaf | 对应设备本地 |

设备签名私钥、设备 E2EE 私钥、Double Ratchet State 和 MLS 私有状态永不在设备间复制。

DID 根私钥默认不传输，只向用户明确授权的新管理设备传输。

### 3.4 密钥用途与用途隔离

设备签名密钥不仅用于登录，还用于把动态密码学材料和控制事件绑定到具体设备，包括：

* 登录挑战与日常 API 请求签名；
* Join Request、challenge 响应和 imported ACK；
* Signed PreKey Bundle；
* MLS KeyPackage；
* 自有设备同步事件和安全通知。

不同用途必须使用明确的 `purpose` 或签名上下文隔离，例如：

```text
awiki.login.v1
awiki.device.join.v1
awiki.did.update.v1
awiki.prekey.bundle.v1
awiki.mls.keypackage.v1
awiki.device.sync.v1
awiki.device.control-ack.v1
```

同一个签名不能从一个用途重放到另一个用途。

设备 E2EE 长期密钥用于：

* 获取并验证设备级 PreKey 后建立 Direct 会话；
* 自有设备之间的结构化 JSON 同步；
* 已授权管理设备之间的根私钥控制消息；
* 私聊附件密钥和其他会话材料的安全传输。

配对临时密钥只服务于一次 Join Session 的 ECDH/SAS，不能写入 DID Document，也不能直接替代正式 Direct 会话。

### 3.5 Root Key Vault 与本地保存

普通设备不创建 Root Key Vault。管理设备按以下方式保存根私钥：

```text
K_root_private
    ↓ 使用设备本地 KEK 加密
Encrypted Root Key Blob
    ↓
Root Key Vault / 系统安全存储
```

KEK 应尽量由 Secure Enclave、Android Keystore/StrongBox、TPM、Windows Hello、系统钥匙串或等价平台能力保护。执行 DID 管理签名前，应要求系统 PIN、生物识别、操作系统登录确认或等价 user-presence。

根私钥明文不得进入：

* 普通文件、明文数据库或配置项；
* 日志、遥测、分析系统和崩溃报告；
* 剪贴板、通知预览、附件或普通聊天历史；
* 常规云备份和未加密导出。

根私钥可以在受控签名或管理设备间 E2EE 传输时短暂解封，因此不能宣称“永远不可导出”。准确边界是：平时由设备本地 KEK 加密，使用后立即清除明文缓冲区。

---

## 4. 第一阶段权威状态

第一阶段只保留三个版本维度：

```text
DID Document
    document_version
    document_hash

AWiki Device Registry
    registry_version

每台设备
    auth_generation
```

其他状态仍有独立职责，但不再形成额外的版本/hash 链：

* `deviceManifest` 是 DID Document 的内嵌子对象，不单独版本化；
* Local Device State 只保存私钥、本地 KEK 和密码学会话状态；
* Handle/WNS 只维护当前 Handle → DID 绑定和普通审计记录；
* 在线/离线是瞬时状态，不是授权状态源。

### 4.1 DID Document 与内嵌 `deviceManifest`

AWiki 第一阶段新建的 DID 从 genesis 起始终携带 `deviceManifest`，即使当前只有一台设备。

示意：

```json
{
  "id": "did:wba:example.com:user:alice:e1_<root-fingerprint>",
  "verificationMethod": [
    {
      "id": "did:wba:...alice#root",
      "type": "Multikey",
      "publicKeyMultibase": "zRoot..."
    },
    {
      "id": "did:wba:...alice#device-a-sign",
      "type": "Multikey",
      "publicKeyMultibase": "zDeviceSign..."
    },
    {
      "id": "did:wba:...alice#device-a-e2ee",
      "type": "Multikey",
      "publicKeyMultibase": "zDeviceE2EE..."
    }
  ],
  "authentication": [
    "did:wba:...alice#device-a-sign"
  ],
  "keyAgreement": [
    "did:wba:...alice#device-a-e2ee"
  ],
  "service": [
    {
      "id": "did:wba:...alice#message-service",
      "type": "ANPMessageService",
      "serviceEndpoint": "https://example.com/anp/message"
    }
  ],
  "deviceManifest": {
    "type": "ANPDeviceManifest",
    "devices": [
      {
        "device_id": "device-a",
        "signing_key_id": "did:wba:...alice#device-a-sign",
        "e2ee_key_id": "did:wba:...alice#device-a-e2ee",
        "profiles": [
          "<anp-vnext-device-direct>",
          "<anp-vnext-device-mls>"
        ]
      }
    ]
  },
  "document_version": 1,
  "proof": {}
}
```

`document_hash` 按 ANP vNext 固定的规范化规则计算，由 DID resolve 结果或更新响应携带。它不需要写回被计算的文档对象。

`deviceManifest` 只表达：

* 当前合法的跨域通信设备；
* 每台设备引用的签名/E2EE 公钥；
* 每台设备可使用的公开 ANP Profile。

它不表达：

* `member/admin`；
* 根私钥是否存在；
* 谁批准了设备；
* token 状态；
* 恢复流程状态。

`deviceManifest` 不使用独立 HTTP 请求、独立 proof、独立 hash 或独立 CAS。完整 Manifest 只作为 DID Document 的一部分发布。

携带规则固定为：

* 创建 DID、resolve 完整 DID Document，以及添加或撤销设备后的 Document 更新，都携带当前完整 `deviceManifest`；
* 日常 Direct/MLS 消息、Join Request、Registry API、token 和根私钥控制 JSON 不重复携带 Manifest；
* 通信方需要设备集合时 resolve 或使用已验证的当前 DID Document，不能把业务消息中的临时设备列表当成授权依据。

### 4.2 AWiki Device Registry

Registry 是 AWiki 域内设备授权的唯一状态源：

```json
{
  "did": "did:wba:...alice",
  "registry_version": 3,
  "devices": [
    {
      "device_id": "device-a",
      "status": "active",
      "role": "admin",
      "management_ready": true,
      "auth_generation": 1
    },
    {
      "device_id": "device-b",
      "status": "active",
      "role": "member",
      "management_ready": false,
      "auth_generation": 1
    }
  ]
}
```

第一阶段持久化状态只包含：

```text
status = active | revoked
role = member | admin
management_ready = true | false
auth_generation = 单调递增整数
```

`pending` 只属于 Join Session，不是正式设备状态。

第一阶段不实现：

* suspended / reactivate；
* admin 降级为 member；
* revoked 设备原地恢复。

需要重新启用的设备必须生成新密钥和新 `device_id`，重新完成加入。

### 4.3 Local Device State

设备本地保存：

* 设备签名/E2EE 私钥；
* Root Key Vault；
* 根私钥是否已经本地安全落库；
* Direct Ratchet State；
* MLS 私有状态；
* 最近接受的 DID Document version/hash。

首设备因为在本地生成并保存根私钥，可以直接声明 `management_ready = true`；后续管理设备只有提交有效的 imported ACK 后才能变为 true。该字段只表示设备曾证明管理就绪，不证明根私钥此后仍物理存在。

---

## 5. 版本、并发与新鲜度

### 5.1 服务端并发控制

设备管理更新使用：

```text
expected_document_version
expected_registry_version
operation_id / nonce
```

Identity Control Plane 在同一个数据库事务中：

1. 检查两个 expected version；
2. 验证根签名和当前管理设备签名；
3. 写入新的 DID Document；
4. 更新 Device Registry；
5. 更新相关设备 `auth_generation`；
6. 消费 Join/Recovery Session 和 nonce；
7. 提交事务。

CAS 冲突时，客户端重新拉取当前状态、重新展示影响并重新提交，不做 last-write-wins。

### 5.2 客户端验证规则

客户端只 pin：

```text
最高 document_version + document_hash
```

处理规则：

* 低于已接受 version：拒绝并强制刷新；
* 同 version、同 hash：接受；
* 同 version、不同 hash：告警并停止敏感操作；
* 更高 version 且当前根签名有效：接受并更新本地 pin。

第一阶段不要求客户端取得 v18 到 v25 的全部中间证明，也不要求透明日志 consistency proof。

### 5.3 Handle 与 Registry

Registry 只使用当前 `registry_version` 做 CAS，不维护 registry hash 链。

Handle/WNS 只维护当前 Handle → DID 绑定。恢复时通过数据库事务检查 Handle 仍指向预期旧 DID，再原子换绑，不维护公开 mapping hash 链。

### 5.4 服务间收敛

第一阶段的 Identity、Message 和 Handle 组件可以共库或同部署，不要求物理拆成独立微服务。

身份事务提交后，通过普通可靠内部事件通知 Message Plane 刷新设备资格、token 和 Mailbox。事件不是新的安全状态源；事件丢失或消费者重启时，Message Plane 直接从可信 Identity 状态读取当前版本。

---

## 6. ANP vNext 协议调整

AWiki 第一阶段需要推动以下公开协议调整：

| 协议部分 | 第一阶段调整 |
| --- | --- |
| Identity/Discovery | 注册 DID Document 顶层 `deviceManifest` 扩展 |
| Direct | 增加发送/接收 `device_id` 和设备级投递语义 |
| Direct E2EE | PreKey、Session、Ratchet 和 Mailbox 绑定 DID + device_id |
| MLS | 允许同一 DID 的不同设备使用独立 KeyPackage/Leaf |
| Federation | 按当前 DID Document 验证设备资格，陈旧时重新 resolve |

### 6.1 AWiki V1 Core 固定规则

AWiki 新 DID：

```text
始终有 deviceManifest
始终有 device_id
始终使用设备级 Direct
不允许从有 Manifest 静默降回无 Manifest
```

### 6.2 Legacy Adapter

旧 DID、无 Manifest DID 和旧 Profile 不进入 AWiki V1 Core 状态机，由独立 Legacy Adapter 处理。

Adapter：

* 只能把旧 DID 表达为一个 legacy endpoint；
* 不得伪造根签名 Manifest 或公开 `device_id`；
* 不得进行隐藏的多设备 E2EE fan-out；
* 不得访问 AWiki Registry、管理角色或根私钥控制消息；
* 不满足安全条件时直接 fail closed。

旧 DID 升级到 V1 时，必须通过一次根签名 DID Document 更新列出全部 eligible 设备。迁移完成后不再返回 Legacy Adapter。

---

## 7. 首个设备初始化

首个设备可以是 PC、手机、平板或其他终端，不存在“手机必须是主设备”的要求。

流程：

```text
账户验证并选择/创建 Handle
    ↓
设备本地生成根密钥、设备签名密钥和设备 E2EE 密钥
    ↓
创建带首设备 deviceManifest 的 DID Document
    ↓
创建设备 Registry：active + admin + management_ready=true
    ↓
根签名 + 首设备签名证明
    ↓
数据库事务提交 DID、Registry 和 Handle 绑定
```

根私钥和设备私钥必须在本地使用系统安全存储或 KEK 加密保存。

手机号或邮箱只负责账户验证和 Handle 入口，不能代替根私钥或设备私钥持有证明。

---

## 8. 新设备加入：五步流程

加入流程与终端类型无关，也不依赖摄像头或二维码。

### 第一步：OTP 定位账户和 DID

新设备完成手机号或邮箱验证，服务端列出可选 Handle/DID，用户明确选择目标 DID。

OTP 只用于：

* 定位账户；
* 创建一次性 Join Session；
* 把请求路由给现有管理设备；
* 限制无关加入请求。

OTP 不是 DID 控制证明，也不能直接授权设备。

### 第二步：新设备生成密钥并签署 Join Request

新设备本地生成：

```text
device_id_new
K_sign_new
K_e2ee_new
sk_pair_new / pk_pair_new
```

随后提交完整 Join Request：

```json
{
  "type": "awiki.device.join.v1",
  "join_session_id": "join-123",
  "did": "did:wba:...alice",
  "device_id": "device-b",
  "signing_public_key": "...",
  "e2ee_public_key": "...",
  "pairing_public_key": "...",
  "requested_role": "member",
  "issued_at": "...",
  "expires_at": "...",
  "signature": "..."
}
```

`signature` 由 `K_sign_new` 对排除 signature 字段后的完整 Join Request 签署。

新设备只能请求普通设备。是否授予 admin 由用户在旧管理设备上决定。

### 第三步：一次 challenge、临时 ECDH 和 6 位 SAS

旧管理设备认领 Join Session 后：

1. 验证 Join Request 签名；
2. 生成高熵随机 challenge；
3. 使用第一阶段固定的 HPKE/ECIES 套件和 `K_e2ee_new` 公钥加密 challenge；
4. 生成自己的配对临时密钥 `sk_pair_old / pk_pair_old`；
5. 把加密 challenge 和 `pk_pair_old` 发送给新设备。

新设备：

1. 使用 `K_e2ee_new` 私钥解密 challenge；
2. 计算 challenge hash；
3. 使用 `K_sign_new` 对 challenge hash、Join Request hash 和 session ID 签名；
4. 返回签名响应。

一次 challenge 同时证明：

* 新设备持有设备 E2EE 私钥；
* 新设备持有设备签名私钥；
* 响应属于本次 Join Session。

不再定义独立 confirmation 临时密钥、confirmation transcript hash 或 `pairing_context_proof`。

新旧设备随后分别计算：

```text
K_pair = ECDH(local_pairing_private, remote_pairing_public)
K_sas  = HKDF(K_pair, "awiki-device-join-sas-v1")
SAS    = Truncate6(HMAC(K_sas, Canonical(JoinTranscript)))
```

双方基于同一个 Join Transcript 计算 6 位 SAS。Transcript 至少包含：

* DID 和 Join Session ID；
* 新旧设备 ID；
* Join Request hash；
* challenge hash；
* 双方配对临时公钥；
* 新设备签名/E2EE 公钥；
* 当前 document version/hash。

SAS 不通过服务器发送，也不由服务器生成。用户必须人工比较两端数字。

6 位 SAS 只有约 20 bit，因此必须：

* 一次性、短时有效；
* 任一端不匹配就作废整个 Join Session；
* 重试时生成新的 challenge 和配对临时密钥；
* 对创建和失败次数限频。

### 第四步：用户确认并提交双签更新

旧管理设备展示：

* 新设备名称和类型；
* 6 位 SAS；
* 设备公钥指纹；
* 将授予 member 还是 admin。

用户确认后，必须通过系统 PIN、生物识别或操作系统登录完成本地 user-presence。

旧管理设备构造新 DID Document 和 Registry：

* 将新设备公钥和 `device_id` 加入 DID Document/deviceManifest；
* Registry 新增 `status = active`；
* 默认 `role = member`；
* 用户选择管理设备时设为 `role = admin, management_ready = false`；
* 初始化该设备 `auth_generation = 1`；
* 递增 document version 和 registry version。

更新由两份签名保护：

```text
DID 根签名
    证明 DID 控制权批准更新

当前管理设备签名
    证明更新由当前有效 admin 发起
```

服务端验证当前 Registry、Join Request、challenge 响应、SAS 会话确认、双签和 expected versions，并在一个数据库事务中提交状态和消费 Join Session。

### 第五步：新设备重新拉取并验证

新设备不能只相信“加入成功”响应，必须重新 resolve 最新 DID Document，并验证：

* 根签名有效；
* document version/hash 不低于本地已接受值；
* 自己的签名公钥和 E2EE 公钥存在；
* 自己的 `device_id` 存在于 `deviceManifest`；
* 公钥引用与本地生成的公钥一致。

随后新设备通过认证 API 获取自己的 Registry 角色。

结果：

* member：可以通信，不接收根私钥；
* admin 且 `management_ready = false`：可以通信，但暂不能管理设备；
* admin 完成第 9 章根私钥导入后：成为可执行管理操作的管理设备。

---

## 9. 根私钥传输：一个 Envelope 和一个 ACK

根私钥传输只属于 AWiki 域内功能，不进入跨域 ANP 标准。

### 9.1 复用现有 Direct E2EE

新旧设备使用已经建立的标准设备级 Direct E2EE 会话。网络外层仍是现有 JSON-RPC `direct.send`，加密消息内部使用当前协议已经支持的：

```text
application_content_type = application/json
payload = JSON object
```

不增加独立私有 Profile、专用 Mailbox 或第二套 Ratchet，也不把根私钥作为文本或附件发送。

为了让服务端在不解密业务 JSON 的情况下执行短 TTL、禁止跨域和完成后清理，AWiki 域内 `direct.send` 只增加最小的可见路由元数据：

```text
delivery_class = awiki-root-key-control
message_id
sender_device_id
recipient_device_id
expires_at
```

这些字段绑定到已认证请求并进入 AEAD AAD。`delivery_class` 只说明这是 AWiki 域内根密钥控制密文，使服务端能执行授权和留存策略；它不暴露根私钥或内层 JSON，也不是新的 ANP Profile。V1 不把这个类别扩展成通用控制消息框架。

服务端只允许它在同一 AWiki DID 的 V1 Core 设备之间使用。V1 只有两种合法方向：

```text
RootKeyEnvelope：
    sender    = active + admin + management_ready=true
    recipient = active + admin + management_ready=false

imported ACK：
    sender    = active + admin + management_ready=false
    recipient = 原 Envelope 的发送设备
    并携带第 9.4 节的签名 completion
```

ACK 首次提交时只要求 importing device 仍具备待就绪资格。原 Envelope 的发送设备在入队和新设备导入检查时必须有效；若它在导入后被撤销，服务端仍可完成 importing device 的状态提交，只是不再向已撤销设备投递通知。

该类别禁止跨域发送、禁止进入 Legacy Adapter、聊天历史、通知预览和普通备份，并使用短 TTL。普通 Direct 的 RPC accepted 响应只表示密文已入队，不表示根私钥已经导入。

### 9.2 RootKeyEnvelope

管理设备发送一个结构化 JSON：

```json
{
  "system_type": "awiki.device.root-key.v1",
  "message_id": "root-message-123",
  "did": "did:wba:...alice",
  "root_key_id": "did:wba:...alice#root",
  "document_version": 4,
  "document_hash": "...",
  "sender_device_id": "device-a",
  "recipient_device_id": "device-b",
  "expires_at": "...",
  "root_private_key": "..."
}
```

整个对象位于 Direct E2EE 密文内部。

该消息：

* 只发送给指定设备；
* 使用现有设备 Mailbox 和上述域内投递类别；
* 不进入聊天 UI、通知预览、普通历史或备份；
* 使用标准 `message_id` 做一次消费和重试；
* 不再定义额外 `transfer_id`。

### 9.3 新设备导入

新设备解密后：

1. 检查 sender/recipient/DID 与当前设备会话一致；
2. 重新拉取当前 DID Document 和 Registry 角色；
3. 确认发送端仍为已就绪管理设备，接收端仍为待就绪管理设备；
4. 根据根私钥计算根公钥；
5. 与 DID Document 中的根公钥完全比较；
6. 使用本地 KEK 原子保存根私钥；
7. 持久化 `message_id` 已消费状态；
8. 清除明文缓冲区；
9. 生成一个 imported ACK。

### 9.4 单一 imported ACK

新设备构造一个不含秘密的完成声明，并用设备签名私钥签名：

```json
{
  "type": "awiki.device.root-key-imported.v1",
  "ack_for_message_id": "root-message-123",
  "did": "did:wba:...alice",
  "sending_device_id": "device-a",
  "importing_device_id": "device-b",
  "root_key_id": "did:wba:...alice#root",
  "root_public_key_fingerprint": "...",
  "document_version": 4,
  "document_hash": "...",
  "result": "imported",
  "imported_at": "...",
  "device_signature": "..."
}
```

`device_signature` 对排除该字段后的完整完成声明做规范化签名。

内层 E2EE JSON 使用当前结构化消息能力承载：

```json
{
  "system_type": "awiki.device.root-key-imported.v1",
  "completion": {
    "type": "awiki.device.root-key-imported.v1",
    "ack_for_message_id": "root-message-123",
    "result": "imported",
    "device_signature": "..."
  }
}
```

其中 `completion` 是上面的完整签名完成声明，此处仅省略重复字段。同一个声明同时作为：

* 返回发送设备的 E2EE JSON 中的 `system_type` 业务对象；
* 同一次 `direct.send` 的服务可见 `completion` 头。

这仍是一条 ACK 消息和一次设备签名，不是“E2EE receipt + complete ACK”两套回执。外层只复制不含秘密的完成声明，内层 JSON 仍通过 Direct E2EE 发送。

服务端先按 `ack_for_message_id` 查询标准消息幂等记录。若已经保存完全相同的签名完成声明，直接返回既有成功，不再重复修改 Registry；若同一 ID 对应不同声明则拒绝。

首次处理时验证：

* 原 `ack_for_message_id` 是尚未完成且未过期的 `awiki-root-key-control` 密文；
* 原消息发送端、接收端和 DID 与完成声明一致，且发送端在 Envelope 入队时是已就绪管理设备；
* importing device 当前仍为 `active + admin + management_ready=false`；
* `device_signature` 可由该设备当前签名公钥验证；
* 完成声明中的 document version/hash 与原 Envelope 的授权快照一致；
* 当前 DID Document 仍包含 importing device，且签名声明中的根公钥指纹与当前根公钥一致。

ACK 不要求当前 Document 仍停留在 Envelope 的版本；无关设备更新可以并发发生。只要原授权快照有效且上述当前资格未变化，就可以完成导入确认。

验证成功后，这一个 ACK 用于：

* 通知发送设备导入成功；
* 删除原 Mailbox 控制密文；
* 在 Registry 中把新设备 `management_ready` 标记为 true；
* 让新设备开放设备管理功能。

Identity Control 以一个小事务设置 `management_ready = true`，递增 `registry_version` 和该设备 `auth_generation`，并保存以原 `message_id` 为键的完成幂等记录。该记录至少保留到 ACK 重试窗口结束，不引入新的 `transfer_id` 或版本链。Mailbox 删除可以在事务提交后幂等执行；即使清理重试，接收端的已消费记录和短 TTL 也会阻止再次导入。设备随后重新获取包含管理 scope 的 token。

ACK 响应或向原发送设备的通知丢失时，新设备根据已消费的 `message_id` 幂等重发同一个签名结果；服务端从完成记录返回成功，根私钥不得重复导入。向原发送设备的 E2EE 通知是尽力投递，不影响 `management_ready` 的权威结果。

服务端记录只能证明设备曾声明导入成功，不能证明根私钥此后仍物理存在。

---

## 10. 日常登录和设备授权

日常登录和消息请求不使用 DID 根私钥。

设备 token 至少绑定：

```text
DID
device_id
设备签名 key_id
auth_generation
scope
```

第一阶段规则：

* access token 使用短 TTL；
* refresh 时读取当前 Registry；
* 敏感操作实时检查当前设备 status、role 和 auth_generation；
* 撤销设备时递增 auth_generation、撤销 refresh token 并关闭在线连接；
* 不实现签名 auth-state lease、事件 gap 证明和 registry hash 检查。

管理操作额外要求：

* `status = active`；
* `role = admin`；
* `management_ready = true`；
* 当前管理设备签名；
* DID 根签名；
* 本地 user-presence。

---

## 11. 设备级 Direct E2EE

私聊采用 Signal/Sesame 风格的多设备模型：逻辑联系人仍是 DID，但密码学会话和密文投递始终落到具体 `device_id`。

### 11.1 每个设备对独立会话

假设 Alice 使用 A1，Bob 有 B1、B2、B3：

```text
Alice A1 ↔ Bob B1
Alice A1 ↔ Bob B2
Alice A1 ↔ Bob B3
```

每个设备对独立拥有：

* PreKey Bundle 和 Session ID；
* Double Ratchet Root Key、Sending/Receiving Chain；
* 消息计数器和重放状态；
* 独立的确认与重试状态。

Mailbox 是接收设备级资源，可以容纳来自多条会话的密文，不属于某一条 Ratchet State。

设备级 PreKey 必须绑定 DID、`device_id`、设备签名 key、有效期和发布时的 `document_version`，并由设备签名。获取方使用当前 DID Document 和 `deviceManifest` 验证设备资格。

正式会话至少绑定双方 DID、双方 `device_id`、设备 key、Session ID、Direct Profile，以及建链时验证的 document version/hash。该版本只是建链快照；无关文档更新不要求重建会话。不同设备不得共享设备私钥、消息密钥或 Double Ratchet State。

### 11.2 一条逻辑消息，多份设备密文

Alice 发送一条私聊消息时，客户端生成一个 `logical_message_id`，再为 Bob 当前每个 eligible 设备分别加密：

```text
Plaintext + logical_message_id
    ├─ Session A1-B1 → Ciphertext / message_id B1
    ├─ Session A1-B2 → Ciphertext / message_id B2
    └─ Session A1-B3 → Ciphertext / message_id B3
```

`logical_message_id` 用于 UI 将多次设备投递显示为一条消息；每份设备密文仍有独立 `message_id`、Ratchet 序号、确认和重试状态。

第一阶段继续逐设备调用 `direct.send`，不恢复 `deliveries[]` 批量协议：

```text
resolve 当前 DID Document
    ↓
读取 deviceManifest 中的目标设备
    ↓
逐设备建立/复用 Direct Session
    ↓
逐设备加密、发送和重试
```

客户端可以并行执行这些独立调用。服务端只把密文放入对应设备 Mailbox，不能读取明文、替客户端加密或执行隐藏 fan-out；某个设备失败也不回滚其他已成功投递。

Direct E2EE 内层可以承载文本和结构化内容。自有设备同步及业务回执使用 `application/json`；附件描述继续使用附件专项方案定义的 `application/anp-attachment-manifest+json`。外层 accepted 只表示密文已接受或入队；送达、已读、撤回和表情等业务事件仍通过 E2EE JSON 发送。

### 11.3 设备集合变化、离线与历史

发送或新建会话前，发送方使用当前 DID Document 的 `document_version + document_hash`，并在每次逐设备 `direct.send` 的认证元数据中绑定该快照，使服务端可以检测陈旧设备集合；不绑定独立 Manifest epoch/hash。

设备集合发生变化时，服务端返回 stale device set，发送方重新 resolve：

* 为新增设备建立新会话并补充尚未成功的当前消息；
* 停止向已删除设备发送未来消息；
* 保留其他未变化设备的独立 Ratchet State；
* 不追溯完整历史 document transition 链。

离线设备的密文进入其 Mailbox，每个 `message_id` 独立确认和重试。新设备默认只能解密加入后的 Direct 密文；历史消息必须通过独立 E2EE 备份或可信设备迁移获得，不能复制旧设备 Ratchet State。

### 11.4 自有设备同步

同一 DID 的设备之间也建立独立 Direct E2EE 会话：

```text
Alice A1 ↔ Alice A2
Alice A1 ↔ Alice A3
```

例如 A1 向 Bob 发送消息时，A1 同时向自己的其他 active 设备发送相同 `logical_message_id` 的规范化业务消息，而不是复制发往 Bob 的设备密文，使各端显示同一条已发送消息。

对端发给 Alice 的消息已经按第 11.2 节分别投递到 A1、A2、A3，不再由某台自有设备二次转发。

自有设备同步复用 `direct.send + application/json`，内层示意：

```json
{
  "system_type": "awiki.device.sync.v1",
  "sync_event_id": "sync-123",
  "owner_did": "did:wba:...alice",
  "source_device_id": "A1",
  "recipient_device_id": "A2",
  "event_type": "message.sent",
  "logical_message_id": "msg-123",
  "payload": {}
}
```

这是 AWiki 域内应用语义，不增加专用 Mailbox、第二套 Ratchet 或跨域 ANP Profile。

同步内容可以包括：

* 自己发送的消息；
* 附件引用、完整性信息和附件密钥；
* 送达/已读、删除、撤回和表情；
* 会话设置、联系人和群组的非秘密状态；
* 设备加入、撤销和身份恢复安全通知。

第一阶段优先保证已发送消息副本、附件引用/密钥和安全事件；其余业务状态可以逐步启用。事件使用 `sync_event_id` 幂等消费，不增加新的身份 version/hash 链。

普通同步绝不能复制设备私钥、Double Ratchet State、消息密钥、MLS Leaf 私钥/Epoch Secret 或 DID 根私钥。根私钥只能走第 9 章的专用 Envelope/ACK。新设备默认接收未来同步事件，历史回填仍由独立备份或迁移方案负责。

## 12. MLS 群聊与附件

### 12.1 MLS 设备模型

群组的业务成员仍按 DID 表达，但同一 DID 的每台设备是独立 MLS Client/Leaf：

```text
Alice DID
    ├─ Phone → MLS Leaf 3
    └─ PC    → MLS Leaf 8

Bob DID
    ├─ Phone → MLS Leaf 5
    └─ PC    → MLS Leaf 12
```

每台设备独立持有 KeyPackage 私钥、Leaf 私钥和 MLS 本地状态，禁止在设备间复制。

动态 KeyPackage 不写入 DID Document。它由设备签名密钥认证，并绑定 DID、`device_id` 和设备 key。Group Host 使用 KeyPackage 前，应 resolve 当前 DID Document，确认设备仍在 `deviceManifest` 中、签名 key 有效且允许目标 MLS Profile；跨域验证不依赖 AWiki Device Registry。

新设备需要进入某个群时，由该群按自身授权执行：

```text
Add KeyPackage → Commit → Epoch N+1 → Welcome
```

设备进入 Manifest 不等于自动进入用户的所有历史群组，AWiki admin 也不自动拥有群管理权。

群消息在 MLS 协议层通常只产生一份 Application Ciphertext，再投递给当前 epoch 的合法 Leaf；这与 Direct 的逐设备独立加密不同。离线设备上线后必须按顺序处理 Commit 和 Application Message。

撤销设备时，停止服务端未来投递不能替代 MLS 撤销。每个相关群仍需执行：

```text
Remove Device Leaf → Commit → Epoch N+1
```

新 epoch 生效后，被撤销 Leaf 才失去未来群消息的密码学访问能力。新设备默认不能解密加入前的历史群消息和附件密钥；历史迁移由独立 E2EE 备份或可信设备迁移负责。

### 12.2 附件对象与密钥分发

附件字段、算法和对象生命周期继续以[附件 E2EE 专项方案](../../e2ee-attachment/e2ee-attachment-transfer-design.md)为权威。多设备架构只规定以下关系。

发送端为每个附件生成随机 `object_key` 和 nonce：

```text
附件明文
    ↓ 使用 object_key 做对象级加密
附件密文
    ↓
只上传一次到 Object Service
```

对象服务只保存附件密文。附件描述至少在概念上包含对象引用、算法/nonce、大小和完整性信息；`object_key` 只能位于 Direct 或 MLS 的 E2EE 内层消息中，不能出现在对象 URI、日志或通知预览中。

#### 私聊附件

附件本体只加密、上传一次，但附件引用和同一个 `object_key` 需要通过每台接收设备的独立 Direct Session 分别交付：

```text
Object Ciphertext：上传一次

A1 ↔ B1：Attachment Manifest + object_key
A1 ↔ B2：Attachment Manifest + object_key
A1 ↔ B3：Attachment Manifest + object_key
```

每份 Direct 密文使用独立 `message_id` 并逐设备调用 `direct.send`。发送者自己的其他设备通过第 11.4 节自有设备同步获得同一附件引用和密钥，不重复上传对象，也不共享 Ratchet State。

#### 群聊附件

群聊附件同样只加密、上传一次。附件引用和 `object_key` 放入 MLS Application Message，由当前 epoch 的合法 Leaf 解密，不需要为每个设备单独包装附件密钥。

#### Download Ticket 边界

```text
Download Ticket
    = 允许设备下载附件密文

object_key
    = 允许设备在本地解密附件
```

多设备继续复用附件专项方案现有的下载授权，不新增多设备专用 Ticket 类型、Ticket epoch 或附件状态机。撤销设备可以阻止其取得未来 Ticket 和新附件密钥，但不能收回它已经下载的密文、密钥或明文。

## 13. 离线、撤销与设备丢失

### 13.1 离线

离线不改变授权状态：

* active 设备的密文进入自己的 Mailbox；
* 普通设备离线不影响其他设备；
* 只要有一台在线且根私钥可用的 admin，就能管理设备；
* 没有可用 admin 时，新设备只能创建 pending Join Session。

### 13.2 永久撤销

第一阶段把“移除设备”统一为永久 revoke：

设备管理更新不得撤销最后一台 `active + admin + management_ready=true` 的设备；最后一台已就绪管理设备不可用时只能进入第 14 章恢复流程。待就绪 admin 不计入可用管理设备数量。

1. Registry 把设备标为 `revoked`；
2. 从 DID Document/deviceManifest 删除设备；
3. 删除对应 verification relationship；
4. 递增 document version 和 registry version；
5. 递增目标设备 auth_generation；
6. 撤销 token、PreKey 和 KeyPackage；
7. 删除尚未领取的未来 Mailbox 密文；
8. 各 MLS 群异步提交 Remove/Commit。

Identity 事务只原子更新 DID Document、Registry、auth_generation 和撤销会话。Message Plane 和群组在提交后读取当前权威状态并收敛，不宣称全局瞬时原子撤销。

### 13.3 设备丢失

普通设备丢失：执行永久 revoke。

管理设备丢失：

* 先执行永久 revoke；
* 假设根私钥可能已泄露；
* 若仍有其他 admin，可继续使用当前 DID，但高风险场景建议执行第 14 章 Handle 恢复并创建新 DID；
* 若最后一台 admin 丢失，只能进入 Handle 恢复。

撤销只保护状态生效后的未来数据，不能远程擦除已经解密的数据或旧根私钥副本。

---

## 14. 唯一恢复路径：Handle 恢复并创建新 DID

第一阶段只支持 `did:wba ... e1_...` 用户身份。

由于根公钥指纹属于 DID 标识：

```text
根公钥改变
    ⇒ DID 必须改变
```

因此恢复不是找回旧根私钥，也不是同 DID 换根，而是：

> 恢复 AWiki Handle，并创建新的密码学 DID 身份。

### 14.1 恢复流程

Recovery Session 与 Join Session 完全分离，不能把已经通过的加入会话升级成恢复会话。它至少绑定：

```text
purpose = recovery
tenant/account
完整 Handle
expected_old_did
recovery_session_id
```

OTP 必须短时有效且一次消费；Recovery Session 使用覆盖冷静期的有界有效期，也只能消费一次。两者都限制创建、发送和失败次数。旧管理设备取消恢复时，必须由当前 `active + admin + management_ready=true` 设备签名，并通过 CAS 取消同一个 Recovery Session；取消与最终提交只能有一个成功。

```text
1. 手机号或邮箱验证
2. 用户明确选择“恢复 Handle 并重置身份”
3. 通知旧设备和绑定渠道
4. 进入冷静期，允许旧 admin 取消
5. 冷静期结束后再次验证和确认
6. 新设备本地生成新根密钥、设备签名/E2EE 密钥
7. 创建新的 e1_ DID、DID Document、deviceManifest 和 Registry genesis
8. 数据库事务把 Handle 从旧 DID 换绑到新 DID
9. 旧身份标记为 recovered/superseded，旧 token 和管理入口失效
10. 新设备重新验证新身份并安全保存根私钥
```

### 14.2 恢复事务

第一阶段把 Identity 与 Handle 恢复状态放在同一事务存储中，用一个数据库事务完成换绑；暂不设计跨服务 cutover 协议。未来物理拆分服务时再单独设计可靠切换机制。

事务检查：

* Recovery Session 的用途、Handle、旧 DID 和账户绑定正确，且已通过冷静期和再次确认；
* Recovery Session 尚未被取消或消费；
* Handle 当前仍指向预期旧 DID；
* 新 DID 未被占用；
* 新 DID Document 中的 Handle service 精确反向声明该 Handle；
* 新根 proof 和首设备证明有效。

事务执行：

* 插入新 DID Document 和新 Registry genesis；
* 将旧 DID 在 AWiki 中标为 recovered/superseded；
* 递增旧 Registry 的 `registry_version` 以及所有旧设备的 `auth_generation`，撤销旧 token；
* 取消旧 DID 的 pending Join Session、待完成根传输和其他管理会话；
* 原子更新 Handle → 新 DID；
* 通过 CAS 消费 Recovery Session；
* 成功后才签发新 token。

所有设备管理入口都把 recovered/superseded 作为拒绝条件，不能再用旧根私钥或旧会话提交管理更新。

事务提交后，Message Plane 根据旧 DID 的 recovered/superseded 状态停止新的实时连接、Mailbox 投递和设备控制消息，撤销托管的 PreKey/KeyPackage，并幂等删除尚未领取的旧 Mailbox 密文；MLS 群组再异步移除旧设备 Leaf。这些动作只约束 AWiki 控制的服务和当前 Handle 身份，不能让旧 DID、远端缓存或已经泄露的私钥在全网消失。

新 DID 的 document version 和 registry version 都从 genesis 初始值开始，不引用旧 DID 的 document hash。旧身份与新身份的关系只由 Handle 换绑和服务端 recovered/superseded 审计记录表达。

不维护 mapping hash 链，不定义两套 recovery 线性化点，也不支持同 DID recovery authority。

### 14.3 恢复后的信任变化

恢复后：

* 旧 DID 仍可用于历史签名和审计，但不再是当前 Handle 身份；
* 联系人必须看到身份安全重置，不能静默继承旧 DID 信任；
* Direct E2EE 会话必须重新建立；
* 群组需要移除旧 DID/设备 Leaf，并按群规则加入新 DID；
* 旧设备已经保存的数据和私钥无法远程擦除。

产品文案不得写成：

```text
通过手机号恢复原 DID 私钥
```

正确表述是：

```text
恢复 AWiki Handle，并创建新的密码学 DID 身份
```

---

## 15. 逻辑组件职责

第一阶段按逻辑职责划分，不要求物理拆成多个微服务。

| 组件 | 职责 |
| --- | --- |
| AWiki Client | 生成和保存密钥、计算 SAS、验证 DID Document、执行 E2EE/MLS |
| Identity Control | Join/Recovery Session、Device Registry、DID 更新、CAS、token 授权 |
| Message Plane | PreKey、Mailbox、Direct/MLS 密文路由和普通 ACK |
| Handle/WNS | 当前 Handle → DID 映射、恢复通知和换绑 |
| Remote Domain/Group Host | 验证公开设备资格和执行 Direct/MLS 规则 |

Identity 和 Message 可以共享数据库或通过普通内部 API/事件同步。无论物理部署方式如何：

* Identity/Registry 是域内设备授权权威；
* DID Document 是跨域公开授权权威；
* Message Plane 不解密业务消息；
* Handle/WNS 不生成用户根私钥；
* 远端实现不读取 AWiki Registry。

---

## 16. 第一阶段安全边界

### 16.1 能够保证

* 每台设备使用独立签名/E2EE 密钥；
* Join Request 和 challenge 证明两类设备私钥持有；
* 双方独立 SAS 检测公钥替换；
* user-presence、根签名和当前 admin 签名共同批准新设备；
* version CAS 防止正常并发覆盖；
* 根私钥默认不传输，只经 Direct E2EE 发送给授权 admin；
* revoked 设备在状态收敛后不再获得未来数据；
* OTP 恢复不会被描述为找回旧根私钥。

### 16.2 不能保证

* Identity/Handle 服务被攻破后的强 split-view 防护；
* 无 transparency log 时的全局唯一视图证明；
* 全网瞬时撤销；
* 对已经泄露的历史明文或私钥远程擦除；
* 抵抗服务端拒绝服务或无限延迟；
* 手机号/邮箱恢复达到已有管理设备批准的同等信任等级。

### 16.3 第一阶段客户端异常处理

* DID Document 根签名无效：拒绝；
* document version 降低：拒绝并刷新；
* 同 version 不同 hash：安全告警，停止敏感操作；
* 更高 version 且根签名有效：接受；
* Registry CAS 冲突：拉取当前状态并重新确认；
* Identity 权威状态不可用：新加入、撤销和恢复 fail closed；
* Message Plane 暂时不可用：保留本地密文状态并重试。

---

## 17. 后续阶段能力

以下能力不进入第一阶段核心实现：

1. 完整 previous-hash 状态链和任意版本 transition proof；
2. transparency log、witness 和主动恶意服务 split-view 检测；
3. 同 DID 换根 recovery authority；
4. Manifest 可选/移除和多种兼容模式进入核心状态机；
5. Direct 批量 `deliveries[]` 和部分成功聚合；
6. 根私钥传输私有 Profile、双回执或多层控制状态机；
7. suspended/reactivate、admin 降级和 revoked 原地恢复；
8. 自有设备完整历史回填、全量双向业务状态同步和复杂冲突合并；
9. 自动把新设备加入全部历史 MLS 群；
10. 多设备专用附件票据和新的附件状态机；
11. 物理多服务拆分后的签名事件链和复杂 gap recovery。

这些能力需要时应作为独立协议版本或专项方案增加，不应重新把分支散入第一阶段核心流程。

---

## 18. 第一阶段完整流程摘要

```text
创建身份：
    首设备本地生成根密钥和设备密钥
    创建始终含 deviceManifest 的新 e1_ DID
    首设备成为 active admin

添加普通设备：
    OTP 定位 DID
    新设备生成三类密钥并签 Join Request
    一次加密 challenge 证明签名/E2EE 私钥
    新旧设备 ECDH 并独立显示 6 位 SAS
    用户在旧 admin 上确认和完成 user-presence
    根签名 + 当前 admin 签名提交更新
    新设备拉取验证后成为 member

添加管理设备：
    先完成普通加入
    Registry 授予 admin，management_ready=false
    通过现有 Direct E2EE 发送 RootKeyEnvelope JSON
    新设备安全落库并返回一个签名 imported ACK
    ACK 删除原密文并把设备标为管理就绪

日常消息：
    resolve 当前 deviceManifest
    为每台设备建立独立 Direct Session
    第一阶段逐设备独立发送和重试

自有设备同步：
    复用设备间 Direct E2EE 传递结构化 JSON
    同步已发送消息、附件引用/密钥和安全事件

群聊与附件：
    每台设备作为独立 MLS Leaf
    附件对象只加密上传一次，密钥经 Direct 或 MLS 分发

设备撤销：
    Registry 标记 revoked
    DID Document/Manifest 删除设备
    auth_generation 递增、token/PreKey 失效
    Message 和各 MLS 群异步收敛

没有管理设备：
    OTP/邮箱验证 + 通知 + 冷静期 + 再次确认
    新设备本地生成新根和设备密钥
    创建新 e1_ DID
    Handle 原子换绑到新 DID
    旧 DID 显示 recovered/superseded
```

核心原则：

> 第一阶段使用“可信权威当前快照 + 数据库事务/CAS + 客户端验证当前根 proof”的简单模型。设备级密码学隔离保持严格，但不把长期 Byzantine 分叉防护、复杂兼容和双重确认状态机提前带入第一期。
