# AWiki IM Core Node SDK

`@awiki/im-core-node` 是 `awiki-im-core` 的 Node.js N-API facade。它面向通用 Node host，
不包含 DSH、Cordis 或 Harness 类型，也不允许访问 Core 的 `internal::*`、SQLite 表、redb key
和 wire 私有类型。

## 能力映射

| 能力 | Node 方法 | `awiki-im-core` public facade |
| --- | --- | --- |
| 默认身份 | `getDefaultIdentity` | `identities().default_identity_async` |
| OTP 第一阶段 | `requestRegistrationOtp` | `identities().request_registration_otp_async` |
| 完成注册 | `completeRegistration` | `identities().register_handle_async` |
| Profile | `updateDisplayName` | `identity().update_profile_async` |
| Directory | `resolvePeer` | `directory().resolve_peer_async` |
| 同步 | `syncNow` | `messages().sync_now_async` |
| 会话列表 | `listConversations` | `messages().conversations_async` |
| 历史 | `getHistory` | `messages().conversation_history_async` |
| 已读 | `markConversationRead` | `messages().mark_conversation_read_async` |
| 文本 | `sendText` | `messages().send_conversation_text_async` |
| 单附件发送 | `sendAttachment` | `attachments().send_conversation_async` |
| 附件下载 | `downloadAttachment` | `attachments().download_conversation_async` |
| 清空本地状态 | `clearLocalData` | Node 环境生命周期拥有的 state root |

同一份映射在 `crates/im-core-node/tests/public_parity.rs` 中作为可执行表维护。绑定层没有 legacy
import API；TypeScript SDK 的 `identity.json` 不会被读取或转换。

Node host 必须为每个 state root 提供稳定的 32-byte `vaultRootKey` 以及
`vaultWorkspaceId` / `vaultDeviceId` 上下文。Facade 只把 host 提供的 key 传给
`VaultRequired` IM Core，不生成、持久化、记录或返回 root key。

## 生命周期和并发

每个 `openImCoreNodeClient` 创建一个环境级 `ImCore`，并在已有默认身份时复用一个
identity-bound `ImClient`。I/O 方法全部返回 Promise，Rust async I/O 不在 Node event loop
上执行阻塞网络或数据库操作。

普通关闭的生命周期固定为 `open → closing → closed`：

- `clearLocalData()` 在持有 mutation/write gate 和 state-root 锁期间等待既有操作退出，释放
  Core/SQLite 句柄，只删除 `identities`、`local`、`cache`、`tmp`、`vault` 与兼容元数据，然后重新初始化
  空 Core；client 保持 open，同一 state root 不会暴露给其他实例；
- `close()` 进入 closing 后立即拒绝新任务；
- sync、下载等可安全取消的任务收到取消信号；
- 已接受任务释放读 gate 后才销毁 Core 并释放 state-root 锁；
- 重复 `close()` 幂等；GC drop 只是取消兜底，不替代 host 显式 teardown。

`listConversations` 先执行一次有界可靠同步，只在 `idle` 或 `changed` 时读取本地投影；超时、
认证撤销、需要恢复或可重试失败都返回结构化错误，不把陈旧投影伪装为成功。

## DTO 边界

- ID 和 cursor 是不透明字符串，任何 host 都不得重建 canonical conversation ID。
- `registeredAtMs` 和附件 byte size 是十进制字符串；其他时间是 RFC 3339。
- 可选输出字段缺失时是 `undefined`，只有 `getDefaultIdentity` 明确用 `null` 表示未注册。
- 上传、下载使用 `Uint8Array`/Buffer 直接跨 N-API；附件正文不经 JSON/base64。
- 当前范围只有单附件，不扩展多附件 UI 或领域 API。

## 状态与错误

`stateRoot` 必须为绝对路径并由单个进程/实例独占。Unix 目录权限收紧为 `0700`、文件为
`0600`；冲突返回 `state_in_use`，不会创建临时身份。无秘密的 `compatibility.json` 只保存
identity ID 到注册 Unix 毫秒的映射，使 `registeredAtMs` 重启稳定。

`clearLocalData()` 是不可撤销的本地操作，只清理上述 SDK-owned 路径，不删除远端账号或 Handle，
不跟随被替换为符号链接的运行目录，也不删除 state root 中未声明为 SDK-owned 的其他文件。

Rust 错误和 panic 都在 N-API 边界收敛为固定的
`{ code, safeMessage, retryable }`。原始 server message/data、token、OTP、路径、密钥和附件
bytes 不进入 JS 错误。未知 native/loader 异常统一为 `internal`。

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

当前构建只生成内部测试 addon。平台包、校验和、provenance 和许可证发行审批属于后续原生
制品步骤；审批完成前不得把该内部产物标记为正式 release。
