# AWiki 多设备架构概览（V1）

**版本：第一阶段可信服务基线**

> 本文用于快速理解 AWiki 多设备方案。协议字段、安全细节和异常处理以[完整架构文档](./multi-device-architecter.md)为准。

---

## 1. 目标与取舍

V1 只实现一条清晰主路径：

```text
新 AWiki DID 始终包含 deviceManifest
每台设备拥有独立 device_id、签名密钥和 E2EE 密钥
Direct 和 MLS 都按设备工作
普通设备负责通信
管理设备额外持有根私钥并管理设备
```

核心原则是：

```text
设备加入权 ≠ 消息通信权 ≠ DID 管理权
跨域通信资格 ≠ AWiki 域内管理权限
```

第一阶段信任 AWiki Identity、Message 和 Handle/WNS 服务按协议维护唯一当前状态，通过数据库事务和 CAS 处理并发。

V1 防护网络篡改、设备私钥伪造、公钥替换、请求重放和撤销设备继续接收未来数据；暂不处理恶意服务端制造分叉视图、透明日志和多 witness 等问题。

---

## 2. 三层架构边界

| 层次 | 主要职责 |
| --- | --- |
| 跨域公开协议层 | Handle/WNS、DID Document、`deviceManifest`、设备公钥、Direct E2EE、MLS |
| AWiki 域内控制层 | Device Registry、设备角色、加入与撤销、token、根私钥传输、Handle 恢复 |
| 设备本地安全层 | 设备私钥、Root Key Vault、KEK、Ratchet 和 MLS 私有状态 |

`deviceManifest` 只公开设备的跨域通信资格，不公开 `member/admin`、根私钥导入状态或 token。

AWiki Device Registry 是域内设备授权状态源。远端 ANP 节点只需判断设备是否为合法通信端点，不需要理解 AWiki 管理角色。

---

## 3. 设备角色

普通设备可以登录、签署日常请求、进行 Direct E2EE 和参与 MLS，但不持有 DID 根私钥，也不能批准其他设备。

管理设备具备普通设备的全部能力，并在本地安全保存根私钥。

为避免“已授权”与“已完成根密钥导入”混淆，管理设备分为：

```text
待就绪管理设备
    active + admin + management_ready=false

已就绪管理设备
    active + admin + management_ready=true
```

首个设备默认是已就绪管理设备。后续设备默认作为普通设备加入，只有用户明确授权后才成为管理设备。

---

## 4. 最小状态与版本

V1 只保留三个版本维度：

```text
DID Document
    document_version
    document_hash

AWiki Device Registry
    registry_version

Device
    auth_generation
```

`deviceManifest` 是 DID Document 的内嵌扩展，不使用独立 HTTP 请求、版本、hash 或 proof。

新 AWiki DID 从 genesis 起始终携带完整 Manifest；添加或撤销设备时随 DID Document 一起更新。日常 Direct 消息不携带 Manifest，通信方按需 resolve 当前 DID Document。

Registry 只保留：

```text
status = active | revoked
role = member | admin
management_ready = true | false
```

客户端只 pin 已接受的最高 `document_version + document_hash`：

* 更低版本拒绝；
* 同版本同 hash 接受；
* 同版本不同 hash 告警并停止敏感操作；
* 更高版本且根签名有效时接受。

服务端通过数据库事务、expected version 和 CAS 防止并发覆盖。V1 不保存前驱链，不提供任意版本间 transition proof，也不实现透明日志或复杂分叉修复。

---

## 5. ANP vNext 调整

ANP vNext 需要增加四类公开能力：

1. DID Document 顶层 `deviceManifest` 扩展；
2. Direct 的发送与接收 `device_id`；
3. PreKey、Session、Ratchet 和 Mailbox 按 DID + device_id 绑定；
4. 同一 DID 的不同设备使用独立 MLS KeyPackage 和 Leaf。

AWiki 域内的 `member/admin`、Registry、token、根私钥传输和 Handle 恢复不属于跨域 ANP 协议。

无 Manifest 的旧 DID 和旧 Profile 由独立 Legacy Adapter 处理，不进入 V1 Core 状态机，也不能访问管理角色或根私钥控制消息。

---

## 6. 首设备初始化

首设备可以是 PC、手机或其他终端。

```text
用户验证并创建 Handle
    ↓
本地生成根密钥和设备密钥
    ↓
创建包含首设备 Manifest 的新 e1_ DID
    ↓
创建 active + admin + management_ready=true 的 Registry
    ↓
事务提交 DID、Registry 和 Handle 绑定
```

手机号或邮箱只负责账户入口，不能替代根私钥或设备私钥持有证明。

---

## 7. 新设备加入

加入流程固定为五步，不依赖扫码。

1. **OTP 定位身份**
   手机号或邮箱用于定位账户、Handle 和 DID，并创建一次性 Join Session。

2. **新设备生成密钥**
   新设备生成签名密钥、E2EE 密钥和配对临时密钥，并用设备签名密钥签署完整 Join Request。

3. **证明私钥并比较 SAS**
   旧管理设备使用新设备 E2EE 公钥加密随机 challenge；新设备解密后，用签名密钥签署 challenge hash。随后双方执行临时 ECDH，并根据完整加入上下文独立计算 6 位 SAS。

4. **旧管理设备确认**
   用户比较两端 SAS，确认设备信息并完成本地 user-presence。旧设备使用根签名和当前管理设备签名提交 DID Document 与 Registry 更新。

5. **新设备重新验证**
   新设备重新 resolve DID Document，验证根签名、版本、自己的设备公钥和 Manifest 条目后才启用。

普通设备加入后即可通信。被授予 admin 的设备先处于 `management_ready=false`，完成根私钥导入后才能管理其他设备。

---

## 8. 根私钥传输

根私钥传输复用现有设备级 Direct E2EE，不增加私有 Profile、专用 Mailbox 或第二套 Ratchet。

已就绪管理设备通过现有 `direct.send` 发送加密的 JSON `RootKeyEnvelope`。服务端只看到绑定到认证请求和 AEAD AAD 的最小投递元数据，例如控制消息类别、设备 ID、消息 ID 和短 TTL，不能读取根私钥。

新设备解密后：

1. 验证发送端、接收端和当前设备角色；
2. 根据私钥计算根公钥并与 DID Document 比较；
3. 使用本地 KEK 原子保存根私钥；
4. 清除明文并返回一个设备签名的 `imported ACK`。

同一个 ACK 既作为返回发送设备的加密 JSON，也携带不含秘密的签名完成声明供服务端验证。

验证成功后，该 ACK 同时用于：

* 通知发送设备；
* 删除原控制密文；
* 将新设备标记为 `management_ready=true`。

这是一个 ACK、一次设备签名，不再并存 E2EE receipt 和 server complete ACK。

---

## 9. 日常通信、MLS 与撤销

Direct 按设备建立独立 PreKey、Session、Ratchet 和 Mailbox。发送方从当前 Manifest 获取接收设备列表，V1 对各设备逐一加密、发送和重试，不定义批量投递协议。

MLS 中，同一 DID 的每台设备拥有独立 Leaf。设备需要进入某个群时按该群规则执行标准 Add/Commit/Welcome；设备撤销后再通过 Remove/Commit 异步移除。新设备不自动加入全部历史群，也不自动获得历史消息。

离线不改变设备授权状态，密文继续进入该设备 Mailbox。

设备移除统一采用永久 revoke：

```text
Registry 标记 revoked
DID Document 删除设备和公钥
递增版本与 auth_generation
撤销 token、PreKey 和 KeyPackage
停止未来 Mailbox 投递
异步移除 MLS Leaf
```

V1 不支持暂停、重新激活、管理员降级或 revoked 设备原地恢复。

---

## 10. 唯一恢复路径

对于 `did:wba ... e1_...`，根公钥变化会导致 DID 变化，因此 V1 不实现同 DID 换根。

唯一恢复流程是：

```text
手机号或邮箱验证
    ↓
通知旧设备并进入冷静期
    ↓
允许旧管理设备取消
    ↓
再次验证和确认
    ↓
新设备本地生成新根密钥
    ↓
创建新的 e1_ DID 和 Registry genesis
    ↓
事务把 Handle/WNS 换绑到新 DID
    ↓
旧身份标记为 recovered/superseded
```

恢复后必须重置联系人安全提示、Direct 会话和群组成员关系，不能静默继承旧 DID 信任。

产品文案必须表述为：

> 恢复 AWiki Handle，并创建新的密码学 DID 身份。

不能表述为“通过手机号恢复原 DID 私钥”。

---

## 11. 后续延迟能力

以下能力不进入 V1：

* previous-hash 状态链和任意版本 transition proof；
* transparency log、witness 和强 split-view 检测；
* 同 DID 换根恢复；
* Manifest 可选或移除的核心分支；
* Direct 批量投递和部分成功聚合；
* 根密钥私有 Profile 或双回执；
* 暂停、重新激活和管理员降级；
* 全量自有设备业务状态同步；
* 自动加入全部历史 MLS 群；
* 多设备专用附件状态机；
* 物理微服务拆分后的复杂签名事件链。

这些能力应在需求明确后作为独立协议版本增加，而不是提前进入 V1 核心状态机。

---

## 12. 总结

V1 用 DID Document + 内嵌 Manifest 表达跨域设备资格，用 AWiki Registry 表达域内角色，用设备本地安全存储保护私钥。

正常加入依赖设备私钥证明、双方独立 SAS、用户确认、根签名和当前管理设备签名；根私钥只在用户明确授权后，通过现有 Direct E2EE 传给新管理设备。

恢复只恢复 Handle 控制权，并创建新的密码学 DID。通过可信服务、最小版本状态、单一路径和独立 Legacy Adapter，第一阶段可以在保持核心安全边界的同时显著降低实现与测试复杂度。
