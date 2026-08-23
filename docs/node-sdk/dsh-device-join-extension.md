# DSH 新设备 Join 的 Node SDK 增量合同

状态：planned，尚未进入当前公开 Node API（2026-08-23 根据独立代码复核修订）

跨仓导航：[Harness Feature](../../../awiki-harness/features/dsh-device-join.md) ·
[DSH 产品设计](../../../dsh-awiki/docs/dsh-device-join-design.md) ·
[现有 Node SDK](awiki-im-core-node.md) ·
[Core Device Join API](../api/im-core-public-api.md#51-device-join-host-facade)

## 1. 范围

本文只定义 `@awiki/im-core-node` 为 DSH **新设备侧**补齐的最小能力。Core 继续拥有 OTP grant、
prepared-registration continuation、Join 密码状态、SAS 派生、Vault 和身份激活；Node 只提供
Promise-based typed facade。

管理设备的 notification inbox、verify、approve、Registry 和 revoke 已由 AWiki Me/CLI 承担，
不为 DSH 第一阶段扩展 Node API。服务端协议、数据库和 ANP wire 均不修改。

## 2. 复用的现有接口

以下接口保持语义不变：

```ts
completeRegistrationWithOutcome(input): Promise<RegistrationOutcome>
beginPreparedRegistrationJoin(input): Promise<PreparedRegistrationJoinProgress>
resumePreparedRegistrationJoin(input): Promise<PreparedRegistrationJoinProgress>
```

`RegistrationOutcome.status === 'existing_handle'` 仍返回 Host-only
`ExistingHandleRegistration`。`continuationId` 是当前进程中 Core preparation 的一次性引用，
不得进入 Browser、日志、配置或持久化；Host crash 后必须重新走真实 OTP。

现有 `resumePreparedRegistrationJoin()` 对 ordinary Join 也会调用 Core
`resume_authorized_join_activation()`，而该函数当前无条件执行 Handle Recovery gate。这是必须
修复的实现缺口，不是 DSH 部署前置：只有存在 joined-device Recovery marker 的 rebind session
才检查 recovery flag/audience；ordinary session 直接 poll。

## 3. 精确 API 增量

### 3.1 准确传入 user presence

当前 native binding 在 `beginPreparedRegistrationJoin()` 内部固定提交
`user_presence_confirmed=true`。它改为由可信 Host 显式传入：

```ts
export interface PreparedRegistrationJoinInput {
  readonly continuationId: string
  readonly operationId: string
  readonly ttlSeconds?: number
  readonly userPresenceConfirmed: boolean
}
```

- `mode='ordinary'` 时 DSH 必须传 `false`；Core 不把它升级为高风险确认。
- `mode='handle_recovery_rebind'` 且 `requiresUserPresence=true` 时，调用方只有在真实平台认证通过
  后才可传 `true`。
- DSH 第一阶段没有系统认证适配，因此对 rebind 失败关闭，不调用 begin。

Node 不接受“交互式 Browser 点击”“Agent 已批准”或环境变量作为 user-presence 证明。

### 3.2 新设备进度

`PreparedRegistrationJoinProgress` 增加两个 display-safe 字段：

```ts
export interface PreparedRegistrationJoinProgress {
  readonly joinSessionId: string
  readonly did: string
  readonly localPhase: PreparedRegistrationJoinLocalPhase
  readonly remoteState: PreparedRegistrationJoinRemoteState
  readonly expiresAt: string
  readonly sas?: string
  readonly completed: boolean
  readonly identity?: NodeIdentity
}
```

约束：

- `expiresAt` 必须直接来自 Core session，使用 RFC 3339 UTC；
- `sas` 只能复制 Core `DeviceJoinProgress.sas`，存在时必须是 6 位十进制字符串；
- Node 不计算、不缓存、不持久化 SAS；Rust/TypeScript `Debug`、`inspect` 和错误必须把它显示为
  `<redacted>` 或完全省略；
- `completed=true` 当且仅当 `localPhase='authorized'`、`remoteState='consumed'` 且
  `identity` 存在；
- terminal cancelled/rejected/expired 不返回 identity，也不能回退为 pending；
- `resumePreparedRegistrationJoin()` 只推进相同 `joinSessionId`，不重复 begin 或 OTP exchange。

`did` 和 `joinSessionId` 只供可信 Host 绑定当前 Core session；DSH Browser Remote 不转发它们。

### 3.3 Core-owned restart restore

新增只读本地 session 投影：

```ts
export interface LocalDeviceJoinSession {
  readonly joinSessionId: string
  readonly side: 'new_device' | 'admin'
  readonly localPhase: PreparedRegistrationJoinLocalPhase
  readonly expiresAt: string
}

listLocalDeviceJoinSessions(): Promise<readonly LocalDeviceJoinSession[]>
```

它只映射 `ImCore::device_join().local_sessions()`，不读取网络、不返回 SAS、remote state、DID、
protocol device ID、challenge、token、proof、hash 或私钥。可信 Host 用它做启动发现：0 条回
onboarding，精确 1 条 new-device session 恢复，多条失败关闭。Host 不在 Node state root 内另写
Join journal，因此 begin 已提交而 JS 尚未收到结果时，重启仍能发现同一 Core session。

`expiresAt` 仅供展示，不能由 Node/Host 本地时钟把非终态 session 改写成 expired；poll 的远端
terminal 或已提交的 Core local terminal phase 才是权威。

### 3.4 显式取消

新增：

```ts
cancelPreparedRegistrationJoin(input: {
  readonly joinSessionId: string
}): Promise<LocalDeviceJoinSession>
```

实现通过 `ImCore::device_join().cancel_new_device_join()` 收敛远端和本地，并返回 typed local
terminal summary：

- 第一次成功后 local token 会被清理；再次取消必须先读取 exact local session，只有同一 ID 且
  `localPhase='cancelled'` 时幂等返回，不能再次打开已删除 token；
- authorized/consumed、expired 或 ID 不匹配失败关闭；远端 rejected 在 Core 本地会收敛为
  cancelled，Host 已知 rejected 时不得再调用 cancel；
- transport outcome 不确定时返回 closed retryable error，Core session 保留，不得假装取消成功；
- 不增加 admin-side cancel、role selector、raw RPC 或内部 state 读取。

## 4. N-API 与版本

该变更修改 native input、native output 并增加 native method，必须：

1. native API version 从 `9` 提升到 `10`；
2. Rust crate、wrapper 和全部平台包进入同一个新的 patch 版本；建议下一个版本为 `0.1.8`，
   实际版本由发布任务确认；
3. loader 拒绝 v9 addon，不能用 optional field 静默兼容缺少 cancel/SAS 的旧二进制；
4. `stage-package.mjs`、pack audit、checksums、SBOM、provenance 和 packed-install gate 使用同一
   committed source OID；
5. DSH 只固定依赖已经发布且 root wrapper + 五个平台 addon integrity 闭合的新版本，不使用
   workspace link 作为远端通过证据。

## 5. Rust/TypeScript 映射

- `NodePreparedRegistrationJoinInput` 增加 `user_presence_confirmed: bool`；
- `NodePreparedRegistrationJoinProgress` 增加 `expires_at` 与 `sas`；
- 增加只读 local-session DTO/list；
- `prepared_registration_join_progress()` 从 `AuthorizedJoinActivationProgress.join` 原样复制
  expiry/SAS，并在 native 边界验证 SAS 形状；
- `refresh_prepared_registration_client()` 仍只在 authorized + consumed 后建立 identity-bound
  `ImClient`，wrapper 还必须验证返回 identity 存在后才设置 `completed=true`；
- `resume_authorized_join_activation()` 把 `require_enabled()` 移到 joined-device marker 分支；
  ordinary Join 在 Recovery gate 关闭、无 audience 时仍可 poll/activate，rebind 语义不放宽；
- cancel 在相同 mutation/write gate、operation timeout 和 state-root lock 下运行；
- `clearLocalData()` / `close()` 的既有生命周期语义不变。

不得把 Core private DTO、account verification token、candidate private key、challenge、proof、
Registry hash、auth generation 或 raw service data 加入 N-API。

## 6. 错误闭集

复用现有稳定 Node error envelope，并至少覆盖：

| 场景 | 稳定语义 |
|---|---|
| continuation 缺失、已消费或跨进程 | `join_preparation_unavailable`，不可重试；重新请求 OTP |
| rebind 未完成真实 user presence | `user_presence_required`，不可在 DSH 降级 |
| 多条 resumable new-device local session | `join_local_state_conflict`，禁止选择 newest/first |
| session 过期 | `join_expired`，不可恢复；重新请求 OTP |
| 管理端拒绝或 SAS mismatch | `join_rejected`，不可自动重试 |
| session 尚待管理端 | 正常 progress，不作为错误 |
| status/cancel 网络失败 | `network` 或 closed remote，按 Core retryable 标记 |
| 授权后本地激活暂未完成 | 保留同一 session，并由 resume 精确续跑 |

错误消息不得包含 continuation、session ID、DID、SAS、手机号、OTP、token、路径或服务端原文。

## 7. 测试门禁

### Rust / N-API

- DTO shape：pending 无 SAS，verified 有脱敏 Debug 的 SAS，terminal 不泄漏；
- begin 透传 `userPresenceConfirmed=false/true`，不再固定 true；
- local list 只读且不含 SAS/remote/identity secret；begin 提交后在 JS 返回前 crash 仍能 exact-one
  restore；
- ordinary resume 在 Recovery gate off 时推进；rebind 仍要求正确 flag/audience；
- resume 同 session 推进并只在 authorized + consumed + identity 时完成；
- cancel 首次成功、exact-local cancelled 幂等、wrong ID/terminal 拒绝、outcome unknown 保留；
- Direct/Group E2EE gates 默认关闭时，ordinary Join 后 PreKey publication 和 `default-plain`
  Direct 可用，不为 Join 偷开 E2EE；
- close/clear 与 in-flight status/cancel 的 gate；
- `public_parity.rs` 增加 local list/cancel，并确认未暴露 admin-only facade。

### TypeScript wrapper

- `copyPreparedRegistrationJoinProgress()` 精确复制 expiry/SAS/identity，拒绝畸形 native 值；
- public types、type tests、runtime tests 和 native loader v10 gate；
- JSON/inspect/error 扫描不出现测试 SAS、OTP、continuation 或 token；
- packed install 在当前平台调用真实 v10 addon，而不是仓库内未打包 binding。

### 推荐命令

```bash
cargo fmt --check
cargo clippy -p awiki-im-core --all-targets --all-features -- -D warnings
cargo clippy -p awiki-im-core-node --all-targets --all-features -- -D warnings
cargo test -p awiki-im-core --locked -j1
cargo test -p awiki-im-core-node --locked -j1
pnpm --filter @awiki/im-core-node run build
pnpm --filter @awiki/im-core-node run typecheck
pnpm --filter @awiki/im-core-node run test
```

完整 workspace、root + 五个平台制品和 `awiki-info-testing` E2E 属于后续实施/发布 gate；
`production-awiki-ai` 只做无写只读 smoke。本文提交本身不代表 API v10 或 npm patch 已发布。
