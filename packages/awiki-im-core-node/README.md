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
  // 经产品层显式二次确认后，可调用 await client.clearLocalData()
}
finally {
  await client.close()
}
```

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
import。

## DTO 与错误

- ID/cursor 都是不透明字符串；字节数和 `registeredAtMs` 使用十进制字符串。
- 时间字段除 `registeredAtMs` 外均为 RFC 3339；附件 bytes 使用 `Uint8Array`/Buffer，绝不经
  JSON 或 base64。
- 抛出的 `ImCoreNodeError` 只包含 `{ code, safeMessage, retryable }`。底层服务正文、token、
  OTP、路径、私钥和附件内容不会进入 JS 错误。

平台包、provenance 与许可证发行链由原生制品 workflow 维护。第一版已批准按
AGPL-3.0-only 分发，对应源码、SBOM、checksum 与构建来源随每个包提供。

## 原生制品

Tier 1 平台使用独立 optional package；wrapper 会显式区分 glibc 与 musl，不做运行期下载，也
不回退 TypeScript SDK。第一版五平台矩阵、AGPL test channel、SBOM、checksum、provenance 和
无源码安装验证见 `docs/node-sdk/awiki-im-core-node-artifacts.md`。仓库不包含自动 npm publish
job；正式 registry 发布仍是独立 release 动作。
