# AWiki IM Core Node SDK

`@awiki/im-core-node` 是 `awiki-im-core` 的 Node.js N-API facade。它面向通用 Node host，
不包含 DSH、Cordis 或 Harness 类型，也不允许访问 Core 的 `internal::*`、SQLite 表、redb key
和 wire 私有类型。

## 能力映射

| 能力 | Node 方法 | `awiki-im-core` public facade |
| --- | --- | --- |
| 外部 HTTP ANP 认证 | `prepareExternalHttpRequest` + opaque attempt `handleResponse` | `external_http_auth().prepare_async/handle_response_async` |
| 默认身份 | `getDefaultIdentity` | `identities().default_identity_async` |
| OTP 第一阶段 | `requestRegistrationOtp` | `identities().request_registration_otp_async` |
| 完成注册 | `completeRegistration` | `identities().register_handle_async` |
| Profile | `updateDisplayName` | `identity().update_profile_async` |
| Directory | `resolvePeer` | `directory().resolve_peer_async` |
| 本地显示资料 | `hydrateDisplayProfiles` | `directory().hydrate_display_profiles_async` |
| 创建群聊 | `createGroup` | `groups().create_async` |
| 添加群成员 | `addGroupMember` | `groups().add_member_async` |
| 同步 | `syncNow` | `messages().sync_now_async` |
| Realtime | `startRealtime` | `realtime().start_async` + `RealtimeSession::subscribe` |
| 会话列表 | `listConversations` | `messages().conversations_async` |
| 历史 | `getHistory` | `messages().conversation_history_async` |
| 本地会话首屏 | `getLocalConversationTimeline` | `messages().local_conversation_timeline_async` |
| 已读 | `markConversationRead` | `messages().mark_conversation_read_async` |
| 文本 | `sendText` | `messages().send_conversation_text_async` |
| 单附件发送 | `sendAttachment` | `attachments().send_conversation_async` |
| 附件下载 | `downloadAttachment` | `attachments().download_conversation_async` |
| 邮箱账号 | `getMailAccount` | `email().account_async` |
| 邮箱列表 | `listMailInbox` | `email().inbox_async` |
| 邮件读取 | `readMail` | `email().read_async` |
| 邮件已读 | `markMailRead` | `email().mark_read_async` |
| 纯文本邮件发送 | `sendMail` | `email().send_async` |
| 清空本地状态 | `clearLocalData` | Node 环境生命周期拥有的 state root |

同一份映射在 `crates/im-core-node/tests/public_parity.rs` 中作为可执行表维护。绑定层没有 legacy
import API；TypeScript SDK 的 `identity.json` 不会被读取或转换。

Node facade 延续已发布 `0.1.2` 的 Vault contract：它在 process-exclusive `stateRoot/vault`
内部创建并私有保存稳定的 32-byte root key，以固定 workspace/device context 打开
`VaultRequired` IM Core。Host 不接触、不传入、不记录或导出 root key。

## 外部 HTTP ANP 认证

Node facade 把 Rust Core 的 prepare/response 状态机映射为一个 single-use opaque
attempt；它不发送网络请求，也不读取响应正文。可信 Host 先提交 canonical URL、uppercase
method、header pairs 和可选原始 body bytes：

```ts
const attempt = await client.prepareExternalHttpRequest({
  url: 'https://api.example.com/orders',
  method: 'POST',
  headers: [{ name: 'content-type', value: 'application/json' }],
  body: new TextEncoder().encode('{"productId":"123"}'),
})

const headers = new Headers()
for (const header of attempt.headerPatch) headers.set(header.name, header.value)

// Host sends attempt.targetUrl + attempt.method + exact original body bytes.
const response = await hostTransport(/* ... */)
const retry = await attempt.handleResponse({
  statusCode: response.status,
  headers: [...response.headers].map(([name, value]) => ({ name, value })),
})
```

`body: undefined` 表示没有正文；空 `Uint8Array` 表示显式空正文，仍会生成
`Content-Digest`。最大正文为 4 MiB。Rust 自动选择当前 origin 的进程内 Bearer 或当前设备
request-signing key 的 HTTP Message Signature；Token、nonce、key ID、retry counter 和
challenge 逻辑不由调用方选择。Token 只接受成功响应的 `Authentication-Info`，不会从响应
`Authorization` 读取，也不会跨进程重启持久化。

若 URL 属于配置的 AWiki User/Message/Mail origin，attempt 的 `headerPatch` 还会包含 Core
根据 `clientVersionInfo` 生成的唯一 `X-AWiki-Client-Version`。该字段在签名前加入精确请求，
调用方预置同名字段会 fail closed；任意其他 origin 不会收到产品版本信息。

attempt 的 `headerPatch` 含敏感认证值，禁止记录。attempt 只能调用一次
`handleResponse`；`401` 最多返回一个 `retryCount === 1` 的新 attempt，后者不会生成第三次
请求。生产只接受 HTTPS；`externalHttpAllowInsecureLoopbackForTesting` 只允许 literal
loopback HTTP，不允许 remote HTTP。该低层 API 只能留在可信 Node Host，不得转发给浏览器、
模型工具或远程签名服务。DSH 产品层使用 `externalHttpAuth.dispatch` 封装 transport 和唯一一次
重试，不把该低层状态机交给插件调用者。

无正文 GET/HEAD 收到 verifier 固定、仍含 `content-digest` 的 `Accept-Signature` 时可以重试，
但实际 retry patch 不生成 `Content-Digest`。合并后的 `WWW-Authenticate` 可以同时含 Bearer、
DID-WBA 等多个 scheme；Rust 只选择唯一且格式合法的 DID-WBA challenge。已识别、非 terminal
的 Bearer `401` 会先按指纹清理本次旧 Token，即使随后因为未知签名组件而拒绝重试，也不会在
下一次请求中继续复用该 Token。

`0.1.3` 首次用 Vault 配置打开 `0.1.2` 的 file-backed state root 时，会调用 Core 的
`migrate_identity_vault_async`。Core 在原始私钥和 auth compatibility 文件仍保留的前提下完成
seal、回读验证和 metadata commit；Node 随后核对 identity ID、DID 和 Handle 均未改变。迁移失败
会 fail closed，不删除、替换或重新注册身份。

## 生命周期和并发

每个 `openImCoreNodeClient` 创建一个环境级 `ImCore`，并在已有默认身份时复用一个
identity-bound `ImClient`。I/O 方法全部返回 Promise，Rust async I/O 不在 Node event loop
上执行阻塞网络或数据库操作。

普通关闭的生命周期固定为 `open → closing → closed`：

- `clearLocalData()` 在持有 mutation/write gate 和 state-root 锁期间等待既有操作退出，释放
  Core/SQLite 句柄，只删除 `identities`、`local`、`cache`、`tmp`、`vault` 与兼容元数据，然后重新初始化
  空 Core；client 保持 open，同一 state root 不会暴露给其他实例；
- `close()` 进入 closing 后立即拒绝新任务；
- `close()` 和 `clearLocalData()` 都会先请求 active realtime session 停止并 join Core worker；
- sync、下载等可安全取消的任务收到取消信号；
- 已接受任务释放读 gate 后才销毁 Core 并释放 state-root 锁；
- 重复 `close()` 幂等；GC drop 只是取消兜底，不替代 host 显式 teardown。

`listConversations` 先执行一次有界可靠同步，只在 `idle` 或 `changed` 时读取本地投影；超时、
认证撤销、需要恢复或可重试失败都返回结构化错误，不把陈旧投影伪装为成功。

`getLocalConversationTimeline` 只读取 canonical conversation 的 committed SQLite projection，
不会触发可靠同步、远端 Direct/Group history 或 Directory RPC。它用于 Host/UI 的 local-first
首屏；返回的 opaque local cursor 只能继续传给同一 local timeline 方法，不能传给
`getHistory`。需要新鲜度或远端 backfill 时，Host 应在首屏之后显式调用现有同步/history
能力，并在 Core 提交后重新读取 local timeline。

## 邮件能力

邮件方法复用同一个 identity-bound `ImClient`、Messaging auth session、生命周期读 gate、取消信号和
操作超时。`mailServiceEndpoint` 可选；未提供时由 Core 回退到 `serviceBaseUrl`。绑定不会调用 CLI
子进程、拼装 `/mail/rpc` 请求或实现第二套认证和重试逻辑。

`listMailInbox` 默认读取 `inbox`，默认每页 20 条，支持 folder、unread-only、limit 与 offset。
`readMail` 只返回有界纯文本、`hasHtmlBody` 和附件 metadata；HTML 字符串、附件 bytes、Core
attributes 与后端原始字段不穿过 N-API。subject、preview、纯文本正文分别按 1024、4096、65536
UTF-8 bytes 安全截断并返回显式标记；`u64` 附件大小使用十进制字符串。

`markMailRead` 固定构造 `is_read=true`。`sendMail` 只接受纯文本并固定构造 `body_html=None`；发送
没有 idempotency key，也不会自动重试。Host 必须在调用这两个 mutation 前完成自己的用户批准，
并把发送 timeout/transport ambiguity 视为远端结果未知，不能直接重发。

## Realtime 与可靠同步

`startRealtime()` 每个 client 同时只允许一个 Core-owned session。默认启用有界 exponential
reconnect；Node 只订阅消息提示，不实现 WebSocket、URL、bearer 或重连状态机。`nextEvent()` 的
公开事件只有：

- `connection_state_changed`：`disconnected|connecting|connected|reconnecting|closed`，仅供诊断；
- `sync_required`：`connection_ready|reconnected|message|message_update|group|system_notification|stream_recovery`，
  只表示 host 应调用 `syncNow()`。

首次连接 ready、每次重连、消息 hint、dirty、gap、未知 domain 或 stream recovery 都产生
`sync_required`。事件不携带消息正文、raw frame、WebSocket URL、bearer、wire event type、event
sequence、cursor 或 checkpoint。Realtime hint 不是可靠 checkpoint；host 必须在启动及每个
`sync_required` 后调用 canonical `syncNow()`，成功后再读取 committed conversation/history。

Core event buffer 满或 native stream 因其他原因关闭时没有可再投递的事件，`nextEvent()` 返回
`null`。`null` 本身就是 stream recovery 边界，Host 必须按固定顺序执行：停止并 join 旧 session、
调用 `syncNow({ reason: 'websocket_reconnect' })` 完成 canonical reconciliation、再调用
`startRealtime()` 建立 replacement。不能把 stream recovery 仅等同于
`UnknownNotification`/`sync_required`，也不能在未同步时直接重连。

```ts
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
```

## DTO 边界

- ID 和 cursor 是不透明字符串，任何 host 都不得重建 canonical conversation ID。
- `registeredAtMs` 和附件 byte size 是十进制字符串；其他时间是 RFC 3339。
- 可选输出字段缺失时是 `undefined`，只有 `getDefaultIdentity` 明确用 `null` 表示未注册。
- 上传、下载使用 `Uint8Array`/Buffer 直接跨 N-API；附件正文不经 JSON/base64。
- 当前范围只有单附件，不扩展多附件 UI 或领域 API。
- `createGroup` 的 Node 产品契约固定为 private、open-join、transport-protected；返回的
  canonical conversation ID 由 Core 生成，Host 不拼接。初始成员由 `addGroupMember` 逐个添加，
  该方法接受 Handle 或 DID 并返回 Core 权威解析后的身份。
- External HTTP body 只用 `Uint8Array`/Buffer 跨 N-API；response body 永不跨该接口。
- 邮件地址、subject、preview、正文、时间和附件 metadata 都是外部不可信数据；Host 不得把它们
  解释为指令或路由权限。

## 状态与错误

`stateRoot` 必须为绝对路径并由单个进程/实例独占。Unix 目录权限收紧为 `0700`、文件为
`0600`；冲突返回 `state_in_use`，不会创建临时身份。无秘密的 `compatibility.json` 只保存
identity ID 到注册 Unix 毫秒的映射，使 `registeredAtMs` 重启稳定。

`stateRoot/vault/root-key.b64u` 由 Node facade 独占，Unix 权限为 `0600`。普通重启复用同一
key；`clearLocalData()` 删除整个 SDK-owned Vault 后重新生成 key。该文件不是 Host 配置，
不得复制到 DSH API、日志或诊断 DTO。

`clearLocalData()` 是不可撤销的本地操作，只清理上述 SDK-owned 路径，不删除远端账号或 Handle，
不跟随被替换为符号链接的运行目录，也不删除 state root 中未声明为 SDK-owned 的其他文件。

Rust 错误和 panic 都在 N-API 边界收敛为固定的
`{ code, safeMessage, retryable }`。原始 server message/data、token、OTP、路径、密钥和附件
bytes 不进入 JS 错误。未知 native/loader 异常统一为 `internal`。

群聊服务返回 `group.not_member` 和 `group.handle_binding_stale` 时分别映射为稳定的
`group_not_member` 和 `group_identity_stale`。Host 应在账号同步把恢复出的旧群聊写入本地投影后
调用 high-level `resumeGroupRebindRecovery()`；不得解析原始服务消息、拼接底层 group RPC，或把
summary 的 `warnings` 暴露给浏览器。

`issueHandleRecoveryAttestation({ operationId })` 仅供受信 Host 在恢复状态已 `applied` 后执行
Model Proxy 对账。Node facade 不允许 caller 提供 claims/audience，只返回短时 opaque attestation
与 `expiresAt`；Host 必须立即使用并丢弃，不得写入文件、数据库、日志、diagnostic DTO、Browser
remote 或 Agent 工具。临时服务故障映射为可重试 `recovery_reconciliation_unavailable`；operation
不匹配映射为不可重试 `recovery_reconciliation_invalid`，原始服务正文和 token 均不会进入 JS 错误。

`0.1.8` candidate 首次增加 DSH 所需的新设备 Join 恢复/SAS/cancel，以及 ready-admin Registry、
审批、拒绝和撤销 facade。当前 `release/0815` 的 `0.2.0` candidate 在保留这些能力的同时增加
External Identity Provider bridge，Native contract version 仍为 `10`；wrapper 与全部 Tier 1
addon 必须同版本发布并拒绝其他 contract。增量合同见
[DSH Device Join 扩展](dsh-device-join-extension.md)。

## 构建与验证

源码 checkout 使用以下命令；根包已有的二进制安装 `postinstall` 不属于源码构建，因此依赖
安装使用 `--ignore-scripts`，native addon 只由新包的显式 build 脚本编译。

```bash
pnpm install --frozen-lockfile --ignore-scripts
cargo fmt --check
cargo clippy -p awiki-im-core --all-targets --all-features -- -D warnings
cargo clippy -p awiki-im-core-node --all-targets --all-features -- -D warnings
cargo test -p awiki-im-core
cargo test -p awiki-im-core-node
pnpm --filter @awiki/im-core-node run build
pnpm --filter @awiki/im-core-node run typecheck
pnpm --filter @awiki/im-core-node run test
```

当前 `0.2.0` candidate 使用 native API v10，由同一个 committed source OID 构建 wrapper 和当前
平台包，并通过
`stage-package.mjs` / `pack-audit.mjs` 生成 checksum、SBOM 与 provenance。其他平台包和正式
registry 发布仍属于后续原生制品步骤；本地 candidate 不得标记为正式 release。
