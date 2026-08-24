# DSH 多设备 Join 与管理的 Node SDK 增量合同

状态：planned，尚未进入当前公开 Node API（2026-08-23 根据独立代码复核修订）

跨仓导航：[Harness Feature](../../../awiki-harness/features/dsh-device-join.md) ·
[DSH 产品设计](../../../dsh-awiki/docs/dsh-device-join-design.md) ·
[现有 Node SDK](awiki-im-core-node.md) ·
[Core Device Join API](../api/im-core-public-api.md#51-device-join-host-facade)

## 1. 范围

本文定义 `@awiki/im-core-node` 为 DSH 新设备侧和 ready-admin 管理侧补齐的最小能力。Core 继续
拥有 OTP grant、Join 密码状态、SAS、Registry、approval handle、revoke、Vault 和身份激活；
Node 只提供 Promise-based typed facade。

管理设备能力一对一映射现有 Core public facade，不在 Node 重写 notification、proof、SAS、
Registry 或 revoke。服务端协议、数据库和 ANP wire 均不修改。

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
protocol device ID、challenge、token、proof、hash 或私钥。可信 Host 只在 session status 为
`unregistered` 时计算：

```text
resumable = side == new_device
  && localPhase in {
       pending, challenge_prepared, response_prepared,
       response_verified, approval_prepared, authorized
     }
```

`cancelled/expired` 是历史终态，不计入 conflict，也不阻止新的 OTP/begin。0 条 resumable 回
onboarding，精确 1 条恢复，多条返回 `join_local_state_conflict`。Host 不在 Node state root 内
另写 Join journal，因此 begin 已提交而 JS 尚未收到结果时，重启仍能发现同一 Core session。

`expiresAt` 仅供展示，不能由 Node/Host 本地时钟把非终态 session 改写成 expired；poll 的远端
terminal 或已提交的 Core local terminal phase 才是权威。

`resumePreparedRegistrationJoin()` 和 cancel 共享一个 local preflight。在打开 remote session
token 前，如果 exact local phase 已是 `cancelled` 或 `expired`，resume 直接构造无 SAS/identity、
`completed=false` 的通用 terminal progress，不调用 Core remote advance。这样 rejected 在当前
poll 仍保留 `remoteState='rejected'`，重启后只剩 local cancelled 时安全降级为 cancelled；expired
也不会因为 token 已清理而泄漏 `invalid_state`。

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

### 3.5 Ready-admin 管理 facade

新增默认身份的薄映射：

```ts
getCurrentDeviceSummary(): Promise<CurrentDeviceSummary>
getDeviceRegistry(): Promise<DeviceRegistrySnapshot>
listLocalDeviceJoinRequests(): Promise<readonly DeviceJoinRequestNotice[]>
startDeviceJoinVerification(input: {
  joinSessionId: string
  operationId: string
  challengeTtlSeconds: number
}): Promise<AdminDeviceJoinProgress>
getLocalDeviceJoinVerificationProgress(input: { joinSessionId: string }): Promise<AdminDeviceJoinProgress>
prepareDeviceJoinApproval(input: {
  joinSessionId: string
  sasConfirmed: boolean
}): Promise<DeviceJoinApprovalPrompt>
confirmDeviceJoinApproval(input: {
  approvalHandle: string
  userPresenceConfirmed: boolean
}): Promise<AdminDeviceJoinProgress>
rejectDeviceJoin(input: {
  joinSessionId: string
  reason: 'user_rejected' | 'sas_mismatch'
}): Promise<AdminDeviceJoinProgress>
revokeDevice(input: {
  targetDeviceId: string
  userPresenceConfirmed: boolean
}): Promise<DeviceRevokeResult>
```

约束：

- current summary 直接映射 Core identity device summary；只有 active admin-ready 才能执行 mutation；
- 方法行为一对一映射 Core public facade；Node Host-only DTO 可保留 Core public
  `did/joinSessionId/protocolDeviceId/registryVersion/authGeneration` 以维持精确绑定和 parity，但 Host
  不得用 version/generation 自行授权；DSH Browser 层必须改成 opaque refs 并移除这些 raw 字段；
- local request list 不做网络 I/O，也不 claim；但对本机已 claim 且已收到 ResponseVerified
  notification 的 session，它会验证 challenge response 并幂等推进本地 phase，这是 SAS 可读的
  必要 reducer step。Host 先用既有 `syncNow()` 提交 system notification，再 list requests，最后
  读取 local verification progress；
- `start` 是唯一 claim/challenge mutation，读取请求或 Registry 不 claim；
- SAS 只来自 local verification progress/prepare result，Debug/inspect 一律脱敏；
- approval handle single-use、process-local，只能交给可信 Host，禁止持久化或转发 Browser；
- approve 固定 member，不增加 role selector；
- reject/revoke 复用 Core public facade，不在 Node 猜测 terminal outcome；
- revoke open option `multiDeviceDeviceRevokeEnabled` 默认 false，DSH 必须显式开启；开关不替代
  ready-admin、self/last-admin、CAS 和 user-presence 检查。

Node 不替 Host 生成 operation ID 或 TTL。DSH Host 必须对同一 raw Join session 使用确定性的
operation ID，并固定 `challengeTtlSeconds=240`（且不超过 Core public 上限）。如果 start 的传输
结果未知，Host 不能用新 operation ID 盲重试：先 sync + list requests；若已由当前设备 claim，
进入 local progress 等待，若被其他设备 claim 则只读，只有仍 `canStartVerification=true` 时才允许
以同一 operation ID 重试。

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
- 增加 current summary、Registry、local request/progress、split approval/reject 和 revoke DTO/method；
- `prepared_registration_join_progress()` 从 `AuthorizedJoinActivationProgress.join` 原样复制
  expiry/SAS，并在 native 边界验证 SAS 形状；
- `refresh_prepared_registration_client()` 仍只在 authorized + consumed 后建立 identity-bound
  `ImClient`，wrapper 还必须验证返回 identity 存在后才设置 `completed=true`；
- `resume_authorized_join_activation()` 把 `require_enabled()` 移到 joined-device marker 分支；
  ordinary Join 在 Recovery gate 关闭、无 audience 时仍可 poll/activate，rebind 语义不放宽；
- cancel 在相同 mutation/write gate、operation timeout 和 state-root lock 下运行；
- `ImCoreNodeOpenOptions` 增加 `multiDeviceDeviceRevokeEnabled` 并映射 Core option，默认 false；
- approval handle 不进入通用 serialization helper，confirm 后按 Core 结果 consume/release；
- `clearLocalData()` / `close()` 的既有生命周期语义不变。

不得把 Core private DTO、account verification token、candidate private key、challenge、proof、
Registry hash、auth generation 或 raw service data 加入 N-API。

## 6. 错误闭集

复用现有稳定 Node error envelope，并至少覆盖：

| 场景 | 稳定语义 |
|---|---|
| continuation 缺失、已消费或跨进程 | `join_preparation_unavailable`，不可重试；重新请求 OTP |
| rebind 未完成真实 user presence | `user_presence_required`，不可在 DSH 降级 |
| 多条 resumable new-device local session | DSH Host 返回 `join_local_state_conflict`，禁止选择 newest/first；raw Node list 本身成功返回 |
| local session 已过期 | 由 local preflight 返回 typed expired progress，不打开已清理 token |
| 管理端拒绝或 SAS mismatch | `join_rejected`，不可自动重试 |
| session 尚待管理端 | 正常 progress，不作为错误 |
| status/cancel 网络失败 | `network` 或 closed remote，按 Core retryable 标记 |
| 授权后本地激活暂未完成 | 保留同一 session，并由 resume 精确续跑 |
| current device 非 ready-admin | `device_management_not_ready`，不发远端 mutation |
| approval handle 失效/重放 | `device_join_approval_expired` 或 closed invalid，不重新 prepare/approve |
| self/last-admin revoke | 保留 Core 稳定拒绝语义，不降级为 generic success |
| start verification 结果未知 | 不换 operation ID；Host sync/list 后按 can-start/current-claim/other-claim 收敛 |

错误消息不得包含 continuation、session ID、DID、SAS、手机号、OTP、token、路径或服务端原文。

## 7. 测试门禁

### Rust / N-API

- DTO shape：pending 无 SAS，verified 有脱敏 Debug 的 SAS，terminal 不泄漏；
- begin 透传 `userPresenceConfirmed=false/true`，不再固定 true；
- local list 只读且不含 SAS/remote/identity secret；begin 提交后在 JS 返回前 crash 仍能 exact-one
  restore；
- resumable 只统计允许的 new-device phase；cancelled/expired history 不产生 conflict；
- resume 对 local cancelled/expired 在打开 token 前返回 typed terminal progress；
- ordinary resume 在 Recovery gate off 时推进；rebind 仍要求正确 flag/audience；
- resume 同 session 推进并只在 authorized + consumed + identity 时完成；
- cancel 首次成功、exact-local cancelled 幂等、wrong ID/terminal 拒绝、outcome unknown 保留；
- Direct/Group E2EE gates 默认关闭时，ordinary Join 后 PreKey publication 和 `default-plain`
  Direct 可用，不为 Join 偷开 E2EE；
- bootstrap admin-ready/member/blocked current summary 映射；
- Registry 读取无 Join mutation；local request list 对未 claim 请求只读，对 current-claimed 的
  ResponseVerified notification 幂等推进本地验证；start 使用稳定 operation ID/240 秒 TTL；
- start response-loss 后 sync/list 收敛，不用新 ID 重复 start；response-verified SAS、
  prepare/confirm single-use approval handle；
- wrong SAS 由 Host 阻断，Node `sasConfirmed=false` / `userPresenceConfirmed=false` 不发 approve；
- reject、handled-by-other-admin、self/last-admin revoke、revoke outcome unknown 与 resume；
- JSON/Debug/error scan 不泄漏 SAS、approval handle、device ID/token/proof；
- close/clear 与 in-flight status/cancel 的 gate；
- `public_parity.rs` 增加 joining 和 admin management facade，并确认没有 raw/internal API。

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
