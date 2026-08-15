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

当前仓库内构建产生内部测试 addon；正式平台包、provenance 与许可证发布门禁在后续原生制品
步骤完成。AGPL/商业发行审批完成前不得把该内部制品作为正式 release 发布。

## 原生制品

候选平台使用独立 optional package；wrapper 会显式区分 glibc 与 musl，不做运行期下载，也不
回退 TypeScript SDK。当前候选矩阵、临时 artifact 构建、SBOM、checksum、provenance 和无源码
安装验证见 `docs/node-sdk/awiki-im-core-node-artifacts.md`。Tier 1 与发行许可尚未获批，因此仓库
没有 npm publish 命令或 job。
