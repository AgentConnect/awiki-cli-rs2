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
})

try {
  const identity = await client.getDefaultIdentity()
  const conversations = identity ? await client.listConversations() : undefined
  const localTimeline = conversations?.items[0]
    ? await client.getLocalConversationTimeline({
        conversationId: conversations.items[0].id,
        limit: 50,
      })
    : undefined
  const group = identity ? await client.createGroup({ name: 'Release Crew' }) : undefined
  if (group) await client.addGroupMember({ groupDid: group.did, member: 'alice.awiki.info' })
  const profiles = identity
    ? await client.hydrateDisplayProfiles({ peers: ['alice.awiki.info'] })
    : []
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

可信 Node Host 可以让 Rust 为外部 transport 的请求准备 ANP 认证头。SDK 不发送请求：

```ts
const attempt = await client.prepareExternalHttpRequest({
  url: 'https://api.example.com/orders',
  method: 'POST',
  headers: [{ name: 'content-type', value: 'application/json' }],
  body: new TextEncoder().encode('{"productId":"123"}'),
})

// Apply attempt.headerPatch and send attempt.targetUrl/method with the exact body.
const retry = await attempt.handleResponse({
  statusCode: response.status,
  headers: [...response.headers].map(([name, value]) => ({ name, value })),
})
```

attempt 是 single-use opaque 对象。Rust 自动在 origin-scoped 进程内 Bearer cache 和当前设备
HTTP Message Signature 之间选择；成功响应只从 `Authentication-Info` 接受 Token。一次
`401` 最多产生一个 retry attempt。正文最大 4 MiB；`undefined` 表示无正文，空
`Uint8Array` 表示需要摘要绑定的显式空正文。

固定 verifier challenge 即使对无正文请求仍列出 `content-digest`，也不会阻止 GET/HEAD
重签；实际无正文签名仍省略 `Content-Digest`。多个合并的 `WWW-Authenticate` scheme 中，
Rust 只选择唯一、合法的 DID-WBA challenge。

`headerPatch` 含敏感凭证，禁止日志记录或序列化。生产只允许 HTTPS；可选
`externalHttpAllowInsecureLoopbackForTesting` 仅为 literal loopback 测试。该 API 不能暴露给
浏览器、模型工具或远程调用者。DSH 插件应使用其 Host-only `externalHttpAuth.dispatch`，而
不是直接让第三方编排 attempt。

## 生命周期与线程

- 一个 client 对应一个环境级 `ImCore` 和一个 default identity-bound `ImClient`，不会为
  每次请求重开 SQLite 或身份目录。
- 所有 I/O 都是 Promise；Rust async 任务不会阻塞 Node event loop。
- `close()` 开始后拒绝新任务，取消可安全取消的任务，等待已接受任务释放状态，再释放
  state-root 文件锁；重复调用是幂等的。
- `clearLocalData()` 在持有 state-root 锁时删除 SDK-owned 身份、本地数据库、缓存、临时文件和
  兼容元数据，再重新初始化空 Core；client 保持可用。它不删除远端账号或 Handle。
- JS GC 只作为异常退出兜底，Host teardown 必须显式等待 `close()`。

## state root

`stateRoot` 必须是绝对路径。同一路径在同一时间只能由一个实例/进程打开。Unix 上目录收紧为
`0700`、文件收紧为 `0600`。包不读取旧 TypeScript SDK 的 `identity.json`，也不提供 legacy
import。Node facade 在 process-exclusive `stateRoot/vault` 内部生成并私有保存稳定 root key，
以固定 context 打开 `VaultRequired` Core；Host 不接触或传入 root key。普通重启复用该 key，
`clearLocalData()` 删除 Vault 并在重新初始化时生成新 key。

## DTO 与错误

- ID/cursor 都是不透明字符串；字节数和 `registeredAtMs` 使用十进制字符串。
- 时间字段除 `registeredAtMs` 外均为 RFC 3339；附件 bytes 使用 `Uint8Array`/Buffer，绝不经
  JSON 或 base64。
- 抛出的 `ImCoreNodeError` 只包含 `{ code, safeMessage, retryable }`。底层服务正文、token、
  OTP、路径、私钥和附件内容不会进入 JS 错误。
- `createGroup` 固定创建 private、open-join、transport-protected 群，返回的
  `conversationId` 由 Core canonical identity 生成；`addGroupMember` 接受 Handle 或 DID。
- Native contract version 固定为 `4`；wrapper 拒绝缺少群管理、本地时间线或展示资料接口的旧 addon。

平台包、provenance 与许可证发行链由原生制品 workflow 维护。第一版已批准按
AGPL-3.0-only 分发，对应源码、SBOM、checksum 与构建来源随每个包提供。

## 原生制品

Tier 1 平台使用独立 optional package；wrapper 会显式区分 glibc 与 musl，不做运行期下载，也
不回退 TypeScript SDK。第一版五平台矩阵、AGPL test channel、SBOM、checksum、provenance 和
无源码安装验证见 `docs/node-sdk/awiki-im-core-node-artifacts.md`。仓库不包含自动 npm publish
job；正式 registry 发布仍是独立 release 动作。
