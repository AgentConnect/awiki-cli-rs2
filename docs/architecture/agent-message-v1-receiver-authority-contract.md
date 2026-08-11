# `awiki.agent.message.v1` 接收端权威契约 V1

状态：`Frozen implementation contract / local candidate only / production activation pending`

契约版本：`awiki.agent.message.receiver-authority.v1`

本文冻结 `awiki.agent.message.v1` 从本地可解析候选进入可发送状态所需的接收端权威。
消息正文 schema 仍以
[`contracts/awiki-agent-message-v1.schema.json`](contracts/awiki-agent-message-v1.schema.json)
为唯一来源；本文只定义接收能力、紧急授权、Receiving Home 裁决和兼容行为，不增加第二个
消息发送 API 或第二套正文 decoder。

## 1. Owner 与信任边界

| 事实 | 唯一 owner | 边界 |
| --- | --- | --- |
| App installation 与 provider target | User Service `push_installations` | 仅当前认证账号及其 active DID 可写；不进入 Profile、DID Document 或消息正文 |
| `awiki.agent.message.v1` 接收能力 | User Service 对 active installation 的私有聚合 | 声明是产品兼容事实，不是身份、路由、Agent trust 或二进制 attestation |
| urgent 用户授权 | User Service 账号所有、当前 DID 投影的私有 preference | 当前仅由 authenticated App foreground get/set 消费；绝不决定结构化消息能否发送 |
| 消息入库裁决 | 目标 DID 所属 Receiving Home Message Service | 本地和跨域均在 recipient commit、event、outbox、Push 之前执行一次 |
| payload decoder 与 App typed projection | Rust `im-core` | Message Service 不复制正文 sanitizer/classifier；invalid exact schema 仍按既有 visible-invalid 规则处理 |
| 本地 schema/idempotency/security preflight | Rust `im-core` | 不查询或缓存远端接收能力；CLI dry-run 不能宣称已验证 |

发送方、发送方 Home、Agent Profile、Handle、DID path、payload 自报和 Push provider 回执都不是
receiver capability 或 urgent authorization 的权威。

## 2. Installation 能力声明闭集

现有 `POST /user-service/v1/push/rpc` 的 `upsert_installation` 请求兼容增加以下三个字段。
三者要么全部缺失，要么全部存在；
旧客户端缺失三者时仍可登记普通 Push target，但该 installation 对本 schema 判为不支持。

```json
{
  "client_product": "awiki-me",
  "client_version": "0.1.6+15",
  "capabilities": ["awiki.agent.message.v1"]
}
```

闭集规则：

- `client_product` V1 只接受精确值 `awiki-me`。
- `client_version` 是 1 到 64 字符的 header-safe 产品版本，只允许 ASCII 字母、数字、`. _ + -`，
  首字符必须为字母或数字。它用于诊断与迁移审计，不单独授权 capability，也不返回发送方。
- `capabilities` 是去重数组；V1 只接受精确成员 `awiki.agent.message.v1`，未知值拒绝而不是忽略。
- App 必须在每次成功启动后的 installation refresh、provider DeviceId 变化和版本变化时重新 upsert；
  downgrade 或不再支持时提交空声明组，即不带这三个字段，不得保留旧 capability。
- User Service 继续验证 authenticated `user_id + current owner_did`、provider target ownership 和
  active DID。声明不提升账号权限，也不证明 sender/Agent 可信。
- `app_id`、`platform`、`logical_device_id`、Handle、DID path 或版本号均不得推导 capability。

数据库允许迁移期的 nullable 旧行，但业务投影必须把缺失、部分、未知、畸形或 disabled 行统一
视为 not capable。禁止用默认值把旧行升级为 capable。

## 3. 聚合与混合版本行为

对目标 `recipient_did` 和精确 capability `awiki.agent.message.v1`，User Service 使用以下固定规则：

```text
eligible installation =
  owner_did == recipient_did
  AND status == active
  AND client_product == awiki-me
  AND capabilities contains exact awiki.agent.message.v1

receiver supported = count(eligible installation) >= 1
```

- 至少一台 active capable installation 即允许 Receiving Home committed message。
- 全部 installation 都是 legacy、disabled、缺失声明或不存在时，权威结果为 `unsupported`。
- capable 与 legacy 混合时，消息只提交一次；Push 只投递当前 capable targets。legacy targets 不收
  该结构化 Push，也不允许自动收到第二条普通文本 fallback。
- capability 在 message commit 后、Push claim 前变化时，worker 必须重新解析当前 targets；只向仍
  capable 的 installation 投递。结果为空只表示无 Push target，不撤销已 committed message，也不
  生成 fallback。
- provider 投递失败、App 未运行或旧客户端最终如何展示，不改变 Message Service 已接受事实；它们
  也不能被描述为 App 已收到或已展示。

能力感知解析复用现有内部接口：

`POST /user-service/internal/push/installations/resolve`

V1 请求在原字段上增加：

```json
{
  "recipient_did": "did:wba:receiver.example:user:alice",
  "provider": "aliyun_emas",
  "required_capability": "awiki.agent.message.v1"
}
```

V1 响应在原字段上增加闭合对象，并且 `installations` 只返回 eligible targets：

```json
{
  "recipient_did": "did:wba:receiver.example:user:alice",
  "installations": [],
  "capability": {
    "schema": "awiki.agent.message.v1",
    "status": "unsupported"
  }
}
```

`capability` 只允许 `schema` 与 `status`；`status` 只允许 `supported|unsupported`。
`supported` 必须对应至少一项 eligible installation，`unsupported` 必须对应空数组。Receiving Home
遇到 DID 不一致、状态与数组矛盾、未知字段/枚举、重复 installation、非目标 owner 或响应畸形时，
按 authority unverified 失败关闭。旧调用不传 `required_capability` 时保持当前“返回全部 active
provider targets”的语义，旧 Message Service 可忽略新增响应字段。

## 4. Urgent authorization 三态

User Service 为每个账号当前 owner DID 与 schema 保存一个私有 preference：

```text
enabled   = 用户明确允许紧急样式
disabled  = 用户明确关闭紧急样式
unset     = 从未明确选择或迁移后没有可信记录
```

认证 App 复用 `POST /user-service/v1/push/rpc`，新增
`get_agent_notification_preference` 与 `set_agent_notification_preference` 两个 method。set mutation
只接受闭合字段：

```json
{
  "schema": "awiki.agent.message.v1",
  "urgent": "enabled"
}
```

`urgent` mutation 只允许 `enabled|disabled`；两个 authenticated method 的响应只允许
`schema`、`urgent`、`updated_at`，其中没有记录的 get 投影为 `unset`。当前没有 Message Service
internal preference resolve，也不得为没有生产 caller 的后台链路预建该接口。App foreground 是
当前唯一消费方；preference 读取失败在 App 内独立降级为 normal，不能把 capability-supported send
变成失败，也不向发送方暴露。DID 轮换/恢复时，这一账号所有 preference 作为
User Service 原子账号状态迁移到新 active DID；旧 DID 不再可读写。无法证明同一账号所有权时不得
复制，目标状态为 `unset`。

有效展示决策固定为：

```text
requested level == urgent
AND server preference == enabled
AND App verified Agent policy passes
AND message authoritative age is within 0..15 minutes
AND foreground conversation mute/rate-limit/platform permission policy passes
```

- `disabled`、`unset`、authenticated App read 超时/错误/畸形、同步失败或本地状态不确定都等价为
  fail-closed normal。
- urgent preference 读取失败不得让 capability-supported 消息发送失败，也不得把错误暴露给发送方。
- `level=urgent` 始终只是 sender request semantic。User Service 不改写消息正文；当前仅 App
  foreground 生成 host-owned presentation decision，Message Service 不读取 preference。
- 本地 toggle 未得到 User Service 成功确认前，App 可以显示保存中或失败，但不得产生有效 urgent。
  禁止用历史 enabled cache 绕过一次失败的权威读取。
- 关闭授权必须即时影响后续 App foreground decision；已经 committed 的消息仍保留为普通卡片事实。
- background/killed 的 server trust、conversation mute 与 preference 聚合权威当前不存在，因此这一
  阶段不产生后台 urgent 行为。相关 preference consumption/API 延后到这些权威同时定义后，不能用
  User Service preference 单点读取代替完整安全策略。

## 5. Receiving Home 唯一裁决时序

### 5.1 同 Home

```text
Core local preflight
  -> sender authentication / origin proof
  -> Receiving Home validates target ownership and ordinary Direct context
  -> detect exact schema string awiki.agent.message.v1
  -> User Service capability-aware resolve
  -> supported: recipient commit + sync event + outbox atomically
  -> unsupported/unverified: reject before all recipient mutations
  -> Push worker re-resolves capable targets; current phase does not read urgent preference
```

### 5.2 跨 Home

```text
sender Core local preflight
  -> Sender Home validates sender and forwards the unchanged Direct request
  -> Receiving Home validates federation proof, target ownership and ordinary Direct context
  -> Receiving Home performs its local User Service capability resolve
  -> supported: recipient commit, then return ordinary accepted result
  -> unsupported/unverified: return stable sanitized error, no recipient mutation
```

Sender Home 不得预查、缓存或代理 receiver capability；跨域请求不得携带 capability、App version、
installation、urgent preference 或 effective presentation 字段。Receiving Home 的内部 bearer 与
authority response 不跨域。accepted response 也不含这些事实。

Message Service 只需在 generic JSON object 中识别精确 `schema` 字符串并选择 authority gate；
它不复制 Core 的正文字符安全、长度或展示 sanitizer。来自非 Core sender 的 malformed exact-schema
正文若通过普通 JSON wire shape 并具备 receiver capability，仍由 Core 投影为 visible invalid，
不能因 broad `awiki.*` classifier 被静默隐藏。

## 6. 稳定错误码与 mutation 语义

| code | 条件 | retryable | 接收端 mutation |
| --- | --- | --- | --- |
| `agent_message_direct_only` | target 不是普通 Direct | `false` | 无 |
| `agent_message_transport_protected_only` | Direct E2EE、Group E2EE 或无法证明普通安全上下文 | `false` | 无 |
| `receiver_capability_unsupported` | User Service 成功返回 `unsupported` | `false`（禁止自动重试） | 无 |
| `receiver_capability_unverified` | authority 超时、不可用、未配置、认证失败、响应畸形/矛盾/DID 不匹配 | `true` | 无 |

`retryable=true` 只表示同一 `client_message_id + idempotency_key` 可在权威恢复后安全重试；不得生成
新 ID。`unsupported` 未来因用户升级而改变时，可以由用户/已授权 workflow 发起新的显式尝试，
但当前调用不得自动轮询或降级双发。

urgent preference 的 `unset` 或 App 读取失败不是 send error，不得返回
`urgent_authorization_*` 给 sender。诊断只允许记录有界分类和计数，不记录 DID、provider DeviceId、
版本、偏好值或消息正文。User Service 的原始错误、HTTP body 和数据库错误不得透传。

## 7. Core 与 CLI dry-run 语义

`validate_agent_message_send_request` 只保留以下本地校验：

- exact schema 的闭合 decoder 与 unsafe/size 检查；
- Direct-only 与 transport-protected-only；
- 非空稳定 `client_message_id` 与 `idempotency_key`。

校验通过后不得再本地返回 `receiver_capability_unverified`。真实 `send` 复用现有 Direct 发送栈，
由 Receiving Home 的权威拒绝或接受；Core 不新增 capability boolean/version 参数，也不调用远端
User Service。sender-local pending/failed projection 仍遵循现有 Core send-state 语义，但不等于
接收端 commit。

CLI `--dry-run` 不访问网络、不解析 Handle、不产生 local echo。对 exact schema 的成功 plan 必须
显式包含：

```json
{
  "receiver_capability_check": "deferred_to_receiving_home",
  "receiver_capability_verified": false,
  "urgent_authorization_checked": false
}
```

dry-run 的 `ok=true` 只证明本地 request 形状、scope、安全模式和幂等字段通过；不得描述为 receiver
supported、server accepted、Push eligible、urgent authorized 或 AWiki Me displayed。普通文本 fallback
仍必须由当前 workflow 单独授权，且与 structured send 二选一。

## 8. 隐私与失败关闭

- 公共 Profile、DID Document、Handle discovery、Agent inventory 和 P3 message 中禁止加入 receiver
  capability、installation 或 urgent preference。
- sender 只能看到 accepted，或上表两个 capability 错误码；不能区分 installation 数量、平台、
  版本、provider、授权三态、App 在线状态或具体失败原因。
- User Service capability-aware resolve 只允许内部 bearer 的 Receiving Home 调用；preference 没有
  Message Service internal read，App 只能读取/修改自己当前账号/DID 的 preference 与 installation。
- capability authority 无法证明时必须在 recipient commit 前失败；不得先写 inbox/event/outbox 再补查。
- App foreground 无法证明 urgent authority 时只降 normal；不得阻断消息、沿用旧 enabled、提升
  priority、响铃、振动、wake 或绕过 DND。background/killed 当前也固定为非 urgent。
- Push 是 committed message 的 best-effort projection。provider accepted 不是 App 收到或展示证据，
  provider result ambiguous 时不得盲目重复产生另一条消息。

## 9. 迁移顺序

1. User Service 先增加 nullable installation 声明字段、私有 preference 表、authenticated App
   get/set 及 capability-aware installation internal resolve；旧 installation 全部按 unsupported，
   preference 全部按 unset。不增加 Message Service internal preference resolve。
2. Message Service 接入 Receiving Home pre-commit gate 和 stable error sanitizer；在 gate ready 前 Core
   继续保留现有 `receiver_capability_unverified` 本地阻塞，避免出现无裁决窗口。
3. Message Service Push worker 改为 capability-filtered target resolve；不得读取 urgent preference，
   也不得改变普通消息 Push。
4. AWiki Me 在 installation upsert 上报闭合声明，并把 toggle 改为 User Service acknowledged state；
   旧本地 `true` 不自动视为 server enabled，必须由一次成功 owner-authenticated mutation 确认。
   当前仅 foreground presentation 消费该 preference。
5. 只有 User Service、Message Service、App 和契约测试同时就绪后，Core/CLI 才删除本地 authority-gap
   blocker，保留本地 preflight 并开放真实请求。
6. background/killed urgent consumption 延后，直到 server trust、conversation mute 与 preference
   聚合权威形成一个闭合设计；届时必须另行冻结 API、迁移和设备验收，不复活本阶段删除的无 caller
   internal endpoint。
7. rollout/rollback 不重写历史消息。回滚 Core send gate 即停止新 structured send；已 committed 行仍按
   visible valid/invalid 规则读取，且不得生成 retro-notification。

## 10. 接受矩阵

| 场景 | send | committed | Push | effective urgent | 期望 code/说明 |
| --- | --- | --- | --- | --- | --- |
| 一台 active capable | 接受 | 是 | 该 target | 依 preference/policy | ordinary accepted |
| capable + legacy 混合 | 接受 | 一条 | 只 capable | 依 preference/policy | 不双发 |
| 只有 legacy/disabled/缺失声明 | 拒绝 | 否 | 无 | 无 | `receiver_capability_unsupported` |
| capability authority 超时/畸形 | 拒绝 | 否 | 无 | 无 | `receiver_capability_unverified` |
| foreground + urgent enabled | 接受 | 是 | capable targets | 仍须 age/trust/mute/rate/platform | App authenticated read；不向 sender 暴露 |
| foreground + urgent disabled | 接受 | 是 | capable targets | normal | 不影响 send |
| foreground + urgent unset | 接受 | 是 | capable targets | normal | fail-closed |
| foreground + urgent read 失败 | 接受 | 是 | capable targets | normal | App 降级；不返回 send error |
| background/killed + 任意 preference | 接受 | 是 | capable targets | 当前固定非 urgent | 等待 trust+mute authority；无 server preference read |
| commit 后 capability 被撤销 | 已接受 | 保留 | worker 重查后跳过 | normal/无 Push | 不撤销、不 fallback |
| exact schema Group/E2EE | 拒绝 | 否 | 无 | 无 | scope/security stable code |
| CLI valid dry-run | 仅 plan | 否 | 无 | 未检查 | `receiver_capability_verified=false` |
| 跨 Home sender 试探 capability | 不允许 | 否 | 无 | 无 | Receiving Home 只返回 sanitized code |

最低证据要求：User Service schema/storage/migration/API tests；Message Service same-home/cross-home
pre-commit、零 mutation、mixed target 与 error sanitization tests；Core/CLI local preflight/dry-run/真实
server error mapping tests；AWiki Me installation/preference sync 与 foreground fail-closed presentation
tests；System
Test 覆盖双 Home acceptance matrix。单元测试、server acceptance、provider acceptance、模拟 Push 均不
等于真实设备收到、响铃、振动或全屏展示证据。

## 11. 实现 handoff

- **Authority / User Service**：按第 2、3、4、9 节实现 installation schema/storage migration、私有
  preference、authenticated App get/set、capability-aware installation resolve、DID owner/rotation
  约束及 closed response tests；不提供 Message Service internal preference read。
- **Message Service**：按第 3、5、6、8 节在 Receiving Home recipient transaction 前接入 gate；
  复用现有 installation directory；Push claim 时重新过滤 targets，不读取 preference。
- **Core / CLI**：按第 6、7、9 节拆除 authority-gap blocker而不拆除本地 validator；不得增加
  caller-supplied capability；dry-run 增加三个明确的未验证字段并保留零副作用。
- **AWiki Me**：按第 2、4、9 节登记 capability、同步 server-acknowledged toggle；当前只在 foreground
  presentation 叠加 verified Agent、权威时间、mute、rate limit 和平台权限策略。
- **System Test / reviewer**：按第 10 节逐项验证 mutation-before-authority 不存在、跨域 Receiving Home
  唯一裁决、隐私不泄露、fallback 不双发，以及 background/killed 不读取 preference、不产生 urgent；
  真实 Push/设备/E2E 未执行时必须标记 `UNVERIFIED`。
