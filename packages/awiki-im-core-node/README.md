# @awiki/im-core-node

`@awiki/im-core-node` 是 `awiki-im-core` 的 Node.js Promise facade。包只暴露 AWiki IM
领域 DTO，不包含 DSH、Cordis、SQLite、redb、ANP wire 或 Rust internal 类型。

```ts
import { openImCoreNodeClient } from '@awiki/im-core-node'

const client = await openImCoreNodeClient({
  stateRoot: '/absolute/private/path/awiki/im-core',
  serviceBaseUrl: 'https://awiki.info',
  didDomain: 'awiki.info',
  userServiceEndpoint: 'https://awiki.info',
  messageServiceEndpoint: 'https://awiki.info',
  mailServiceEndpoint: 'https://mail.awiki.info', // 可省略并回退到 serviceBaseUrl
})

try {
  const identity = await client.getDefaultIdentity()
  const conversations = identity ? await client.listConversations() : undefined
  const group = identity ? await client.createGroup({ name: 'Release Crew' }) : undefined
  if (group) await client.addGroupMember({ groupDid: group.did, member: 'alice.awiki.info' })
  const profiles = identity
    ? await client.hydrateDisplayProfiles({ peers: ['alice.awiki.info'] })
    : []
  const localTimeline = group
    ? await client.getLocalConversationTimeline({ conversationId: group.conversationId, limit: 50 })
    : conversations?.items[0]
      ? await client.getLocalConversationTimeline({
          conversationId: conversations.items[0].id,
          limit: 50,
        })
      : undefined
  if (identity) {
    let realtime = await client.startRealtime()
    try {
      for (;;) {
        const event = await realtime.nextEvent()
        if (event === null) {
          await realtime.stop()
          await client.syncNow({ reason: 'websocket_reconnect' })
          realtime = await client.startRealtime()
          continue
        }
        if (event.kind === 'sync_required') {
          await client.syncNow({
            reason: event.cause === 'reconnected' ? 'websocket_reconnect' : 'websocket_hint',
          })
        }
      }
    }
    finally {
      await realtime.stop()
    }
  }
  // 经产品层显式二次确认后，可调用 await client.clearLocalData()
}
finally {
  await client.close()
}
```

`getLocalConversationTimeline` 只读取 Core 已提交的本地 conversation projection，不发起
同步、远端 history 或 Directory RPC。它适合首屏显示；远端刷新应在后台进行，并在 Core
提交后重新读取该 local timeline。它返回的 local cursor 不得传给 `getHistory`。

## 外部 HTTP ANP 认证

默认 Node client 不返回认证 Header patch，也不把签名器作为普通 TypeScript API 暴露。
DSH Host 必须通过受限 Provider lease 的 `authenticatedHttp.dispatch` 完成签名和发送，普通
插件只能提交精确请求并接收响应。

当目标属于配置的 AWiki User/Message/Mail origin 时，Core 会把配置中的
`X-AWiki-Client-Version` 作为受管 header 加入同一个 attempt，再对完整请求签名；Host 必须发送
`headerPatch` 中的该字段，调用方不能自行覆盖。其他外部 origin 不继承产品版本 header。

## DSH External Identity Provider

DSH Host 将 `ctx.anpIdentity.acquireProvider(...)` 返回的 Host-only lease 作为
`identityProvider` 传给 `openImCoreNodeClient()`。打开时会校验
`anp-identity-provider-ts/1`、必需 capability、Provider readiness 与 Store schema；任一项不匹配
都 fail closed，不会隐式改用另一个身份 Store。

Provider 回调全部是异步 Promise。签名 payload、签名结果和 HTTP body 使用独立 Buffer 槽，不经
JSON/base64；公开 DID 快照在 Rust session 内缓存，普通签名和 Origin Proof 各只跨 TS 一次。
Host teardown 必须先 `await client.close()`，再 dispose Provider lease。当前 External bridge 在
sealed ECDH 响应携带可验证 AAD 上下文之前明确返回 capability unavailable，不会降级为把 raw
shared secret 交给 JavaScript。

## 生命周期与线程

- 一个 client 对应一个环境级 `ImCore` 和一个 default identity-bound `ImClient`，不会为
  每次请求重开 SQLite 或身份目录。
- 所有 I/O 都是 Promise；Rust async 任务不会阻塞 Node event loop。
- `close()` 开始后拒绝新任务，取消可安全取消的任务，等待已接受任务释放状态，再释放
  state-root 文件锁；重复调用是幂等的。
- 每个 client 同时只允许一个 Core-owned realtime session；`close()` 和 `clearLocalData()` 会先
  stop/join active session。
- `clearLocalData()` 在持有 state-root 锁时删除 SDK-owned 身份、本地数据库、缓存、临时文件和
  兼容元数据，再重新初始化空 Core；client 保持可用。它不删除远端账号或 Handle。
- JS GC 只作为异常退出兜底，Host teardown 必须显式等待 `close()`。

## state root

`stateRoot` 必须是绝对路径。同一路径在同一时间只能由一个实例/进程打开。Unix 上目录收紧为
`0700`、文件收紧为 `0600`。包不读取旧 TypeScript SDK 的 `identity.json`，也不提供 legacy
import。Node facade 在 process-exclusive `stateRoot/vault` 内部生成并私有保存稳定 root key，
以固定 context 打开 `VaultRequired` Core；Host 不接触或传入 root key。普通重启复用该 key，
`clearLocalData()` 删除 Vault 并在重新初始化时生成新 key。

## Realtime

`startRealtime()` 复用 Core `RealtimeService::start_async()` 与 reconnect runner。公开事件仅有连接
状态和 `sync_required`；后者覆盖首次 ready、reconnected、消息 hint、dirty/gap 与 stream
recovery。Host 必须把它当作调用 `syncNow()` 的调度提示，再读取 committed history。事件不暴露
消息正文、raw frame/URL/bearer、event sequence、cursor 或 checkpoint，hint 也不具备 checkpoint
语义。Core event buffer 满或 native stream 结束时，`nextEvent()` 返回 `null`；Host 必须把它视为
stream recovery，按 `stop old session → syncNow({ reason: 'websocket_reconnect' }) → startRealtime()`
恢复，不得只退出监听循环，也不得跳过 canonical sync。

`syncNow()` 返回的 `olderHistoryExcluded` 是 Schema 3 bounded-history 成功标记，不是容量错误。
公开结果只包含安全计数、closed warnings 和 changed conversation IDs，不包含 cursor、page ref、
token、manifest 或消息正文。

## Root Transfer 与本机认证

`prepareRootKeyTransfer()` 和 `confirmAndSendRootKeyTransfer()` 只映射 Core 的 exact-device
Root Transfer。短生命周期 `authorizationHandle` 只能保留在可信 Host，不能进入 Browser、日志或
持久化；Root material、P5 密文和 ACK 均不进入 TypeScript。

Darwin Host 可调用 `confirmUserPresence()` 请求系统级 device-owner authentication。取消、拒绝、
不可交互和非 Darwin 平台均返回 `false`，调用方不得改用 Browser 点击、确认词或 keyring 解锁替代。
认证成功只证明一次本机用户操作；Host 仍须重新检查业务上下文，再用同一 authorization handle
调用 Core。

exact-device session 始终要求版本化 WebSocket：已协商 P6 lane 时使用既有
`awiki.sync.event.v3.p6-delivery-context.v1` 和同一 bootstrap client instance；未协商 P6 lane
（包括不编译 group-e2ee 的 Node build）时使用既有 `awiki.sync.event.v3`，不得伪造 P6 activation。

## DTO 与错误

- ID/cursor 都是不透明字符串；字节数和 `registeredAtMs` 使用十进制字符串。
- 时间字段除 `registeredAtMs` 外均为 RFC 3339；附件 bytes 使用 `Uint8Array`/Buffer，绝不经
  JSON 或 base64。
- 抛出的 `ImCoreNodeError` 只包含 `{ code, safeMessage, retryable }`。底层服务正文、token、
  OTP、路径、私钥和附件内容不会进入 JS 错误。
- `createGroup` 固定创建 private、open-join、transport-protected 群，返回的
  `conversationId` 由 Core canonical identity 生成；`addGroupMember` 接受 Handle 或 DID。
- 当前 `0.2.2` 源码 candidate 的 Native contract version 为 `11`，增加 Root Transfer 与 Darwin
  user-presence facade，并保留 `0.2.1` 的 Host-only External
  Identity Provider Promise bridge，并保留 `0.1.8` 引入的新设备 Join 恢复/SAS/cancel、
  ready-admin Registry、审批/拒绝和设备撤销；同时保留 prepared registration
  Join、Recovery、Profile、完整群成员管理、P9 mention 与 Payload send。registry `0.1.5` 是
  v5，包含 external HTTP auth、local timeline、群管理展示、realtime 与 mail facade；`0.1.6`
  是上一版 v8 candidate，已发布 `0.1.7` 为 v9。`0.2.2` 必须同步发布 v11 wrapper 与全部平台 addon，wrapper 拒绝其他
  版本的 addon。

## 邮件

`getMailAccount()`、`listMailInbox()`、`readMail()`、`markMailRead()` 与 `sendMail()` 直接复用
Core `EmailService`，不会调用 CLI 或自行构造邮件 RPC。读取结果不包含 HTML、后端 attributes 或
附件 bytes；subject、preview 和纯文本正文有明确的 UTF-8 byte 上限与 truncation 标记。

`markMailRead()` 只支持标为已读。`sendMail()` 只发送纯文本，不接受附件或 HTML，也没有
idempotency key 和自动重试。Node host 必须在调用这两个 mutation 前取得产品层批准；发送 timeout
或 transport loss 后必须按“远端结果未知”处理，不能自动再次发送。所有邮件字段都应当作外部
不可信数据，不能解释为指令。

平台包、provenance 与许可证发行链由原生制品 workflow 维护。第一版已批准按
AGPL-3.0-only 分发，对应源码、SBOM、checksum 与构建来源随每个包提供。

## 原生制品

Tier 1 平台使用独立 optional package；wrapper 会显式区分 glibc 与 musl，不做运行期下载，也
不回退 TypeScript SDK。第一版五平台矩阵、AGPL test channel、SBOM、checksum、provenance 和
无源码安装验证见 `docs/node-sdk/awiki-im-core-node-artifacts.md`。仓库不包含自动 npm publish
job；正式 registry 发布仍是独立 release 动作。
