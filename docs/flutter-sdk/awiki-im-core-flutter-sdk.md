# Awiki IM Core Flutter SDK

`packages/awiki_im_core` is a general-purpose Flutter/Dart SDK for `crates/im-core`. It is not an `awiki-me` adapter and must not expose app UI/cache DTOs such as `ChatMessage` or `ConversationSummary`.

## Layers

```text
Flutter app
  -> packages/awiki_im_core (Dart API, platform loader, generated FRB Dart glue)
  -> crates/im-core-dart (Rust-Dart facade, DTO mapping, lifecycle)
  -> crates/im-core (pure Rust IM SDK)
```

`crates/im-core` remains pure Rust. Flutter, Dart, FFI, codegen, and platform packaging belong only in the Dart package and `im-core-dart` facade.

## Supported platforms

v0.1 targets native Flutter on Android, iOS, macOS, and Linux:

- Android: `arm64-v8a`, `x86_64`, optional `armeabi-v7a`.
- iOS: device and simulator static-library XCFramework slices.
- macOS: `aarch64`/`x86_64` XCFramework slices.
- Linux: `x86_64-unknown-linux-gnu` shared library bundled through the Flutter Linux FFI plugin.

Windows is not declared as a v0.1 Flutter plugin platform. Flutter Web has a stub so dependent apps can analyze; calling `AwikiImCore.open` on Web throws `UnsupportedError` because `dart:ffi` cannot load the native Rust backend there.

## Lifecycle and blocking model

The Rust facade exposes opaque `DartImCore` and `DartImClient` objects. Each object has an explicit close/dispose path. After close, Rust calls return `DartImError` with code `object_closed`; the Dart wrapper mirrors this with `AwikiImCoreException`.

`im-core` is blocking-first. Public Dart APIs are `Future<T>` and must not expose synchronous IO, SQLite, or HTTP calls into widget build paths. FRB generated calls should use the worker-thread model and the wrapper must keep App-facing methods async.

### Local-state upgrade before Core open

Applications upgrading from release/0710 must inspect and, when required, run
the canonical local-state upgrade before `AwikiImCore.open`:

```dart
final inspection = await AwikiImCore.inspectLocalStateUpgrade(paths: paths);
if (inspection.eligibility == LocalStateUpgradeEligibility.required) {
  final result = await AwikiImCore.upgradeLocalState(paths: paths);
  assert(result.status == LocalStateUpgradeStatus.completed);
}
final core = await AwikiImCore.open(config: config, paths: paths);
```

Inspection is read-only. Upgrade performs the Core-owned cross-process lock,
SQLite online backup (including committed WAL state), shadow migration,
conservation/invariant validation, and cutover. A missing SQLite file is a fresh
install and returns `notRequired` without creating files. Ordinary Core open
continues to fail closed with `local_state_upgrade_required` for the exact
release/0710 schema 27 source; hosts must not delete, archive, or recreate the
database to bypass this gate. The canonical pre-open runner owns schema 27 only.
Post-canonical schemas 28 through the current version return `notRequired` and
remain available to the ordinary atomic schema migration performed by
`AwikiImCore.open`; the pre-open runner must not duplicate or block that
dispatch. Public reports
contain schema versions, aggregate counts, and owner-scoped legacy-to-canonical
conversation mappings required by the App overlay migration. They never expose
backup paths or message content. The mapping remains available after cutover so
an App crash between the Core and overlay journals can resume idempotently.

If a canary must be downgraded after cutover, dispose Core first and restore
the complete verified release/0710 backup with the new SDK/tooling:

```dart
final restored = await AwikiImCore.restoreLocalStateBackup(paths: paths);
assert(restored.restoredSchemaVersion == 27);
assert(restored.targetSafetyCopyAvailable);
```

Restore is not an in-process rollback for an open Core. It only accepts a
completed canonical-upgrade journal, keeps the current schema-36 target as a private
safety copy, and is idempotent across interruption. The public result exposes
versions/availability only, never filesystem backup paths.

Schema 28 stores have already completed the canonical-conversation cutover.
Ordinary Core open upgrades reviewed release predecessor shapes through schema 35.
It also recognizes the complete current/release shapes previously issued with versions
32 through 34 and atomically fills the missing hydration/checkpoint or v2 sync/read-recovery
side plus the schema-35 unresolved-message thread-binding association. Partial or
unrecognized same-number shapes fail closed; applications must not invoke
the release/0710 runner again or perform an automatic delete/archive fallback.
Schema 35 stores may predate `identity_transition_pending` or contain its narrower
pre-V4 shape. Ordinary Core open upgrades both reviewed variants transactionally to
schema 36; hosts must not clear identities or archive the database to work around them.

## DTO policy

Facade DTOs follow `im-core` public DTO semantics and use Dart-friendly primitives at the boundary. Time values remain ISO-8601 strings. The Dart wrapper may add convenience getters such as `AuthStatus.authenticated`, but it must not rename Rust DTO semantics such as `has_session` into a different facade meaning.

## Identity registration and legacy recovery

The SDK exposes `registerHandleWithPhone`, `registerHandleWithEmail`, and `recoverHandle` on `AwikiImCore`. These calls are core-level identity registry operations that map to `im-core` public identity DTOs; they do not depend on any `awiki-me` account gateway or UI model.

A successful registered result exposes `HandleRegistrationResult.accountId`,
which is the canonical User Service account ID persisted by Core. A
`join_required` result leaves `accountId` null; Flutter/App code must not decode
JWT claims or substitute a DID to manufacture it. Its
`HandleRegistrationJoinRequiredPreparation` is an opaque, process-local Core
preparation containing only a preparation ID, typed mode, user-presence
requirement, expected DID, and full Handle. The account verification token and
recovery transition remain inside Core and are never exposed to Dart/App code.
After an exact completed local identity retirement, Core may retain the stable
message account binding while the related credential is absent. Re-registering
that same Handle returns ordinary `join_required` when the retirement marker
exactly closes over the binding identity, DID, and protocol device; Flutter
hosts should keep presenting the explicit Join/Handle-Recovery choice rather
than translating the historical binding into an error.

The remote `registered.message` field is diagnostic text and is not exposed as
registration authority. Core validates the exact DID, Handle, domain, binding
generation, and device access token before returning `registered`; Flutter hosts
must branch only on the typed registration state and must not parse response
wording to distinguish first registration from phone-owned Legacy recovery.

`AwikiImCoreConfig.clientVersionInfo` is the typed product/release/version/build
input used by native Core. AWiki Me supplies `awiki-me/0714/<version>+<build>`
from its package metadata; Dart does not construct the native wire header. Core
injects the header into configured AWiki product HTTP/WS requests, while AWiki
Me's few direct User Service HTTP adapters reuse the same package facts. All new
User Service requests target `/user-service/v1/...` without an unversioned
fallback.

After the remote and local identity commits, exact-device P5 PreKey publication
is a recoverable completion step. When Group E2EE v2 is enabled, Core also
publishes a deterministic, retry-safe P6 KeyPackage family for the bootstrap
device so that another user can add the newly registered identity to an
encrypted group. Failures return the registered identity with the stable
`registration_prekey_publish_pending` or
`registration_group_key_package_publish_pending` warning; registration pending
cleanup failure uses `registration_pending_cleanup_required`. Flutter hosts
must preserve these warnings, must activate the committed identity, and must
not request another OTP or start a second registration. Later secure work
reuses Core's durable local publication state.

`recoverHandle` always requests the same canonical local-finalize behavior as the CLI. A
successful recovery of an existing full Handle rotates its DID without changing the stable local
identity ID, records old/current DID history, refreshes owner-DID snapshots, and enqueues any
Handle-backed group member rebind work. Dart hosts must not persist the returned DID as a new local
identity owner or supply a separate generated identity.

This legacy API is not the multi-device Handle Recovery flow below and must
never be used as its fallback.

For an explicit destructive settings action, native Flutter hosts may call
`AwikiImCore.deleteLocalIdentityData(selector)`. Unlike ordinary
`deleteLocalIdentity`, this removes every Core-owned local projection for the
selected stable identity before retiring its credential. It never deletes the
remote Handle/account or another local identity. Product-owned App databases
must be purged separately with the same stable owner identity ID; Web fails
closed with `UnsupportedError`.

Skill Token claim is intentionally not exposed through the Dart facade in v1. The raw one-time
Token is consumed only by the CLI/Rust onboarding path; App code signs and copies the instruction
but does not pass the Token into im-core, persist it, or manage the resulting Skill Agent identity.
The explicit v1-journal recovery entrypoint is likewise Rust/CLI-only; Flutter must not delete
legacy onboarding artifacts or start a replacement claim that would create a second Agent DID.

## Identity secret storage

Native hosts can open `AwikiImCore` with explicit identity SecretVault options:

```dart
final core = await AwikiImCore.open(
  config: config,
  paths: paths,
  openOptions: AwikiImCoreOpenOptions.vaultRequired(
    identitySecretVault: ImCoreSecretVaultOptions(
      rootKey: DeviceVaultRootKey.fromList(rootKeyBytes),
      vaultDir: vaultDir,
      workspaceId: workspaceId,
      deviceId: deviceId,
    ),
  ),
);
```

`AwikiImCoreOpenOptions.fileCompat()` keeps the compatibility default. Use
`vaultPreferred` only as a migration-period mode; production App safety should
use `vaultRequired` and fail closed when the host cannot provide the root key or
matching vault context.

The Dart SDK does not generate, persist, rotate, or back up the host root key.
The host must get it from its own no-prompt secure storage path and pass it only
for `open`. `DeviceVaultRootKey.toString()` is redacted, and App code must not
log generated DTOs or `openOptions` that could contain root key bytes. The SDK
does not expose generic `open_secret()`, private key, or raw `SecretRef` APIs.
Auth APIs may still return `bearerToken` in session DTOs for existing flows;
callers must treat it as sensitive and must not log or persist it outside the
SDK-owned auth/session storage path.

Identity vault diagnostics are exposed as narrow facade methods:

```dart
final status = await core.identityVaultStatus(selector);
final migration = await core.migrateIdentityVault(selector);
final verification = await core.verifyIdentityVault(selector);
```

Status/migration/verification DTOs report backend, metadata, warnings, and
plaintext compatibility retention only. They do not expose root key material,
JWTs, bearer tokens, or secret refs. Flutter Web remains a stub and cannot run
the native vault-backed backend.

Native hosts can read the current identity's safe device projection without
opening any private key:

```dart
final device = await core.identityDeviceSummary(selector);
```

The result distinguishes legacy/member/admin readiness and exposes only the
protocol device ID and public key IDs. It intentionally omits Vault references,
root-key presence flags, and internal Document/Registry/auth checkpoints.

For a native vNext client, the stable message-sync binding is obtained only
through:

```dart
final binding = await client.activeSyncAccountBinding();
```

`ActiveSyncAccountBinding` contains exactly
`ownerIdentityId`, `accountId`, `currentDid`, `protocolDeviceId`,
`identityGeneration`, and `deviceAuthGeneration`. Generations remain decimal
`String` values and must not be parsed as Dart `int`. Core may perform an
authoritative WNS lookup when an upgraded identity has no persisted Handle
generation, so the API is asynchronous. Legacy/hosted identities, missing
authoritative state, offline lookup failure, and binding mismatch all fail
closed; App adapters must surface the typed error and must not derive a
fallback from `IdentitySummary.deviceId`, DID, or local vault settings. Flutter
Web exposes the same method shape but throws `UnsupportedError` because no
native Core exists.

The Rust-only trusted-host APIs `generate_vnext_agent_bootstrap`,
`prepare_vnext_agent_legacy_upgrade`, and
`client_with_device_identity_material` are intentionally not exposed through
the Dart/Flutter facade. Their DTOs contain root/device private material and a
Device Access token, do not implement Serde, and exist only for a native
Daemon/Skill host that immediately transfers secrets to its own SecretVault.
Flutter App identities continue to use the persisted native Identity Registry
and the ordinary `activeSyncAccountBinding()` facade.

The Rust-only `local_hydrated_incoming_recovery_async` typed-page API is also
not exposed through Dart. It exists for Daemon crash compensation, requires an
exact active binding, and uses a Core-issued owner-bound continuation token;
it is not an App timeline API or a public Sync v2 cursor.

## Multi-device Join

Device Join 是 native SDK 的正式产品能力，不再受 Join host-local rollout gate 控制。新设备立即用
`DeviceJoinAccountVerificationGrant.fromToken(...)` 包装短期账号验证结果；
该 grant 没有 getter、copy/JSON API 或泄露内容的 `toString`，并由
`beginDeviceJoin` 一次性消费。重启恢复与候选设备侧收敛继续使用
`localDeviceJoinSessions`、`pollNewDeviceJoin` 和 `cancelNewDeviceJoin`。

注册返回 `join_required` 时不走上述 host-supplied grant 接口。Host 只把 opaque
`preparationId` 和正式 user-presence 结果传给
`beginPreparedRegistrationDeviceJoin(...)`；Core 自行校验稳定 owner、消费内部 token，并在
Recovery rebind 模式下先持久化 joined-device marker，再创建远端 Join。该 preparation
故意不跨进程持久化；App/Core 重启后必须重新发起注册验证，不能缓存 token、推断 owner 或
拼装独立 JSON continuation。
本地凭证已通过 Core 完成退役、但同账号消息 binding 仍保留时，重新提交同 Handle 会继续得到
ordinary `join_required`；App 仍显示 Join/Recovery 选择。缺失、未完成或不匹配的 retirement
证据由 Core 失败关闭，Dart 不检查 SQLite、marker 或 identity registry 来自行降级。

现有管理设备不通过 Registry pending 列表或 admin HTTP polling 发现请求。Core 在完成系统通知
验证、durable dedupe 和本地 reducer commit 后，才发出可信
`system_notification_changed` 事件；host 收到该信号后调用
`localDeviceJoinRequests(selector)` 读取已验证、secret-free 的本地请求投影。打开页面或读取请求
不得自动占有 Session。用户明确点击“开始验证”后，host 才调用
`startDeviceJoinVerification(...)`，将 claim 与 Challenge 提交合并为一个操作；拒绝请求使用
带 `DeviceJoinRejectReason.userRejected` 或 `DeviceJoinRejectReason.sasMismatch` 的
`rejectDeviceJoin(...)`。

该验证把目标 DID 的 `ANPMessageService.serviceDid` 仅作为 Home Service 域信任锚；P3
Business Origin 是同域保留 path 下独立、E1 绑定的 System Notification Agent DID。Flutter
host 不接收该身份的动态配置，也不需要处理自定义 profile。

只有响应已经验证后，两端才显示本地推导的六位 SAS。管理设备收到可信通知并刷新本地请求后，
调用 `localDeviceJoinVerificationProgress(selector:, joinSessionId:)` 读取短期 SAS。该接口是
纯本地读取，只接受已经进入 `ResponseVerified` 或 `ApprovalPrepared` 的本地管理端 Session；
它不发起 HTTP/RPC、不轮询远端、不写通知，也不推进 Join 状态。

新设备的 `pollNewDeviceJoin` 在远端仍为 `responseVerified` 时，从本地 restart-safe
transcript 与 Vault pairing secret 按需重新派生同一 SAS；SAS 本身仍不落盘。这样响应已经
提交后发生 App 重启或一次无界面轮询，不会让该 Join 永久失去可比较的 SAS。
远端进入 `consumed` 后，Core 使用与实时 Join 相同的文档解析边界校验最终
`JsonWebKey2020` OKP Ed25519/X25519 设备方法，再提交 Vault activation record 和本地身份；
该边界也供 hosted device auth、Root 和 P5 使用，Flutter host 不需要也不能自行转换这些
verification methods。

用户确认 SAS 一致后，host 调用 `prepareDeviceJoinApproval`，再在真实本地 user presence 后调用
`confirmDeviceJoinApproval`。approval API 不接受 role，Join 结果固定为 rootless
`member`；Registry 中既有设备的 member/admin role 仍可用于授权设备展示。approval handle
只保留在进程内，不得记录或持久化。

Join model 只暴露安全的 Session、设备、Registry role/status、expiry、请求生命周期和短期 SAS
事实，不暴露 OTP/Join token、完整 Join Request/proof、pairing/private key、shared secret、
root material、Challenge/ciphertext 或 Document version/hash。Join session/progress 也不暴露
Registry/auth generation。

显式 `identityDeviceRegistry` 返回的 `DeviceJoinRegistrySnapshot` 是唯一例外：它以 decimal
`String` 返回 `registryVersion`，其专用 `DeviceRegistryAuthorizedDeviceSummary` 以 decimal
`String` 返回 `authGeneration`。App 只能用它们做 display-only account-state cache 的单调替换，
不能据此授权 Join、revoke 或 root transfer；安全动作仍必须调用 fresh Core。两个版本都不得
转换为 Dart `int`。User Service 当前以 `u64` 维护这两个权威值；Rust facade 必须在跨 FFI
之前转换成十进制字符串，生成的 DCO/SSE codec 也必须使用 `String` 编解码。

SAS 只允许短暂存在于 `DeviceJoinProgress`，不得进入 `DeviceJoinRequestNotice`、realtime event、
持久化 DTO 或日志；相关模型的字符串与 Debug 输出必须保持脱敏。
`system_notification_changed` 只携带 event ID、闭合 notification type 和可靠同步 hint，不透传
raw P3 payload。Flutter Web 保留同形 API，但该 native 流程仍返回 unsupported。

## Multi-device Handle Recovery

Native hosts opt in with `ImCoreOpenOptions.multiDeviceHandleRecoveryEnabled`; the default is
`false`. The generated Dart facade exposes `requestHandleRecoveryOtp`,
`prepareHandleRecovery`, `activateHandleRecovery`, `resumeHandleRecovery`,
`handleRecoveryStatus`, `listHandleRecoveryOperations`,
`discardHandleRecoveryPreAttempt`, `quarantineHandleRecoveryKeyUnavailable`,
`authorizedHandleRecoveryReceipt`, `activateAuthorizedJoin`, and
`resumeAuthorizedJoinActivation`. V4.0 progress is the closed
`awaitingFactor → readyToCommit → remoteOutcomeUnknown|remoteCommitted →
identityTransitionPending → applied` projection, with `quarantinedKeyUnavailable` as the explicit
key-loss escape state. Phone, OTP, Recovery Grant, proof,
private keys, JWT, Vault refs, ciphertext, and filesystem paths are never returned.
`HandleRecoveryProgress` also carries the secret-free impact counts used by confirmation UI and
an optional Core-authorized Registry epoch reset tuple. Authorized Join returns
`AuthorizedJoinActivationProgress`, which pairs ordinary `DeviceJoinProgress` with that same reset
projection; App code must never infer the account/owner/old DID/generation/source tuple from Join.
The public methods are wrappers on `AwikiImCore` itself; callers do not import generated APIs or
access its private native handle. Flutter Web exposes the same signatures and fails closed as
unsupported.

`requestHandleRecoveryOtp` accepts a canonical full Handle, phone, and an optional identity selector;
Core creates and returns the opaque operation ID plus its authoritative local owner ID. The host
passes that exact ID to `prepareHandleRecovery`, activate, resume,
status, discard, or quarantine. Core does not guess a pending Recovery from an identity scope and
does not return `null` for an unknown ID. The public failure enum is closed to
`factorRetryRequired`, `resultAbsent`, `outcomeUnknown`, `localKeyUnavailable`,
`localTransitionPending`, `localMigrationUnsupported`, and `unknownEpoch`; no V3 aliases are
accepted.

The host may supply an exact identity selector and always supplies foreground user-presence
confirmation. A global flow passes `null`; Core resolves an exact local Handle match or, when
no local match exists, bootstraps a new local identity from the phone-verified Handle's public
WNS binding. `null` never selects the current/default identity implicitly. Native Core owns the
keys, proof, exact retry, stable-owner local epoch reset, fresh JWT,
new P5 PreKey publication, and transport-only group rebind. Recovery never migrates old
Ratchet/MLS material and never creates P6 or `awaitingP6` state. Only exact Handle-backed
`transport-protected` groups are eligible; every missing, DID-only, E2EE, malformed, or
conflicting profile fails closed.

If the remote Commit has succeeded but fresh-JWT or P5 PreKey finalization cannot finish because
of a retryable transport/auth/session/service/serialization failure, activate/resume throws the
stable `localTransitionPending` failure while preserving the same durable operation. Flutter code
must query and resume that exact operation; it must not translate the state into “not prepared” or
start another Recovery. Once the operation becomes `applied`, Core clears its stale retry error.

`legacyRegistryEpochAdoptionAuthority(selector)` is the narrow bridge for an App upgrading an
already active legacy local device-registry epoch. It returns only the exact owner/account/DID/
generation/device tuple and an opaque provenance ID. It returns `null` if any Handle Recovery
transition marker exists, including completed markers. Dart must not synthesize this authority.

V4.0 adds no CLI command, Daemon task, Agent recovery flow, or process-global current identity.
A later host surface must call these same typed APIs rather than own recovery state. The older
one-shot phone-owned Legacy `recoverHandle` API is not a V3 Manifest compatibility path and cannot
resume, query, or authorize a V4 operation.

## Management-device root-key transfer

Root transfer is the single native V1 path and has no separate rollout option.
The host first obtains an identity-scoped `AwikiImClient`, then calls:

```dart
final prepared = await client.rootKeyTransfer.prepare(
  recipientDeviceId: justJoinedDeviceId,
);
// Verify and display prepared.recipient before prompting the user.
final accepted = await client.rootKeyTransfer.confirmAndSend(
  authorizationHandle: prepared.authorizationHandle,
  userPresenceConfirmed: confirmedByOperatingSystem,
);
```

`prepare` returns an opaque, short-lived authorization handle plus the exact
secret-free recipient summary and expiry. The handle must only be passed back
to the same client's `confirmAndSend`; its string projection is redacted.
Core generates the message ID. The host cannot provide a root key, PreKey,
session, checkpoint, proof, nonce, ciphertext, completion proof, or timeout.

`RootKeyTransferSendResult` contains only DID, sender/recipient device IDs,
Core-generated message ID, and accepted time. Acceptance means that encrypted
delivery was accepted; it does not claim that recipient import or management
readiness completed. Sender-side list, import status, and retry APIs are not
public.

RootKeyEnvelope, P5 state, imported completion, Vault state, and transport
recovery remain entirely inside native Core. Public failures are the typed
`RootKeyTransferException(code:, retryable:)` closed union. Flutter Web returns
`root_transfer.unsupported` and has no plaintext or JavaScript fallback.

## Permanent device revocation

Native hosts opt in with
`AwikiImCoreOpenOptions(multiDeviceDeviceRevokeEnabled: true)`; the option is
independent from Join and defaults to false. After the host obtains foreground
OS user presence, it calls `revokeDevice` with an identity selector, the exact
opaque target device ID, and `userPresenceConfirmed: true`.

The result contains only DID, target device ID, and `revoked` status. Internal
Document/Registry versions and hashes, `auth_generation`, operation IDs,
documents, proofs, tokens, and key material never enter the Dart API. Native
Core rejects self-revocation and revoking the final ready management device;
Flutter Web exposes the typed surface but keeps the operation unsupported.

Failures expose `AwikiImCoreException.deviceRevokeOutcomeCategory` with the closed
`DeviceRevokeOutcomeCategory.cancelledBeforeSubmit`, `rejectedBeforeCommit`, and
`outcomeUnknown` values. Apps must refresh the authoritative device Registry after
`outcomeUnknown`; they must not classify the outcome by matching `message`. A successful result
does not claim every encrypted group has converged. Affected groups may remain send-paused until a
current owner device explicitly repairs that group.

## Multi-device Direct E2EE rollout

Native hosts select the exact-device P5 v2 Direct product path with
`AwikiImCoreOpenOptions(multiDeviceDirectE2eeEnabled: true)`. The option is
host-local, defaults to false, and is independent from Join, root transfer,
device revoke, Handle Recovery, and Group E2EE. It is never serialized into
ANP, a DID Document, or a cross-domain request. Enabling it changes only Core's
Direct product routing; it does not expose ciphertext, ratchet state, control
JSON, or internal delivery ledgers to Dart.

## Multi-device group encryption rollout

Native hosts opt in with
`AwikiImCoreOpenOptions(multiDeviceGroupE2eeEnabled: true)`; the option defaults
to false and is independent from the Join flow and root-transfer gate. It is local
configuration and is not sent as an ANP, DID Document, or cross-domain field.
When enabled, `client.secure.group(groupDid).status()` and `repair()` read the
device-scoped P6 v2 state and return only redacted readiness and repair facts.
`GroupSecureRepairResult` reports `addedDevices`, `removedDevices`, and
`remainingDevices` for the selected group reconciliation.
They never return raw KeyPackages, Welcome/Commit data, Leaf identifiers, MLS
secrets, provider paths, or SQLite rows.

Group inventory is explicitly paged:

```dart
final first = await client.groups.listGroups(limit: 100);
final next = first.hasMore
    ? await client.groups.listGroups(limit: 100, cursor: first.nextCursor)
    : null;

final firstMembers = await client.groups.listMembers(
  groupDid,
  limit: 100,
);
final moreMembers = await client.groups.listMembers(
  groupDid,
  limit: 100,
  cursor: firstMembers.nextCursor,
);
```

`GroupReadResult` exposes `nextCursor`, `hasMore`, `pageGroupDid`, and
`groupStateVersion`. The cursor is opaque. `groupStateVersion` remains a canonical decimal
`String`, not a Dart `int`; `pageGroupDid/groupStateVersion` come from the Host member-page
response and must not be filled from request arguments. The Dart wrapper returns one page and does
not automatically enumerate a whole roster.

When the Host projects `device_revocation_pending`, group secure status is never `ready`:
an active owner with local controller state receives `needsRepair`, a non-owner receives
`waitingForMembershipUpdate`, and a device without controller state receives
`missingLocalState`. These are read-only readiness facts. Status does not mutate MLS, and malformed
or unavailable Host maintenance state fails closed to `unavailable`.

## Directory profile metadata

`client.directory.resolvePeer(handle)` and `client.directory.lookupHandle(handle)` return a
`DirectoryResolution.conversationId` together with the resolved DID/Handle. When the directory
provides a stable user ID and full Handle, this is the canonical `dm:peer-scope:v1:*` identity and
must be used by App start-conversation flows before the first message arrives. The same result can
also carry `DirectoryResolution.profile` populated from the WNS Handle Resolution Document
`profile` object. This profile is a DID Subject Profile projection, not routing or security metadata.

The Dart `UserProfile` model uses these standard display fields:

- `displayName`
- `avatarUri`
- `profileUri`
- `description`
- `subjectType`
- `agentKind`
- `agentCapabilities`
- `versionId`
- `ttl`

相同 `UserProfile` 模型从已认证的 User Service Profile mutation 返回时，还可以包含
`profileVersion`。它是账号 Profile 域的 canonical non-negative decimal string，允许
`"0"` 并始终保留为 Dart `String?`。它与 WNS DID Subject Profile 展示元数据 `versionId`
相互独立；旧响应只有 `versionId` 时 `profileVersion` 为 `null`。

`agentKind` 与 `agentCapabilities` 来自 User Service 的结构化 Agent inventory 投影，用于
共享类型标签和邀请前的友好禁用状态。App 不得根据 DID/Handle/昵称补造 capability，也不得
把该公开字段当作最终授权；群邀请提交仍接受服务端 admission 的最终裁决。

Legacy compatibility fields remain available:

- `bio` maps to / from `description` where needed.
- `avatarUrl` maps to / from `avatarUri` where needed.
- Older service inputs such as `nick_name`, `name`, `avatar_url`, and `avatar` are normalized by `im-core`.

Display fields must not be used for routing, authentication, authorization, service endpoint selection, E2EE binding, or security-profile negotiation. Apps should keep Handle or DID visible on profile and recipient-confirmation surfaces, especially for high-risk operations.

`client.directory.hydrateDisplayProfiles(peers)` reads only the local `im-core` contact/profile cache. It does not call WNS or User Service, and is intended for hot UI paths such as conversation lists, contact lists, and member lists. A returned `DisplayProfile` has `cacheHit = false` when the peer is absent locally, `isStale = true` when the Persona Profile TTL has expired, and `legacyFallback = true` when the visible name only came from an old contact `name/nick_name`. The app may render that stale value immediately and schedule one coalesced remote refresh; it should always fall back through `displayName -> handle -> did` without blocking list rendering. A stored Persona Profile, including one with no display name, takes precedence over legacy contact names. Remote refresh must be explicit through `resolvePeer`, `lookupHandle`, `loadPublicProfile`, or the send-time security verification path. A successful `loadPublicProfile` refresh also persists mutable display fields into an already verified Persona projection, so a later Core/client recreation hydrates the latest nickname and avatar. It does not create a Persona or route for a contact-only peer and does not replace the Persona's verified Handle.

`core.updateDisplayNameProjection(identityId: identityId, displayName: displayName)` updates only the selected local `IdentitySummary.displayName` projection. It is owner-ID scoped and idempotent, and is intended for an App that has already obtained an authoritative Account State Profile snapshot. It never changes the current identity, DID, Handle, device binding, authentication material, or routing state. Apps must still fence the result to the active session before publishing it to UI state.

Remote Handle lookup, Profile resolve, and public-profile calls are idempotent reads. On
`transportUnavailable`, Core replays the identical read once; service errors are not replayed,
and this does not enable retries for mutations without an idempotency identity.

`client.directory.relationStatus(peer)` is the authenticated remote relationship status query despite its retained Dart method name. `RelationStatus` exposes `isFollowing`, `isFollower`, `isFriend`, `isBlocked`, `isBlockedBy`, `isContact`, and `messaged` independently. Its nullable `relationship` field is only the caller's outbound local projection (`following` or `none`) and must not be treated as the combined relationship state. A consumer derives `friend` only when both directional flags and `isFriend` agree; missing or contradictory directional truth fails closed.

## Group display metadata

`CreateGroupRequest.avatarUri` maps to `group_profile.avatar_uri`; `CreateGroupRequest.name` remains the Flutter convenience input for `group_profile.display_name`. `GroupSummary` and `GroupSnapshot` expose `displayName` and `avatarUri`; the old `name` field is retained as a compatibility projection of `displayName`.

Group creation service DID is resolved from `AwikiImCoreConfig.anpServiceDid`. If it is absent, group create returns `invalid_input(field = anp_service_did)`.

These display fields are UI metadata only. They must not be used for routing, authorization, membership checks, E2EE binding, or service endpoint selection.

## Message retry

`retryMessage` is explicitly unsupported in v0.1 and returns `unsupported_capability("message-retry")`. The SDK must not rebuild a send request from display message DTOs because those DTOs can lose target, body, security, idempotency, and retry-plan information.

## Local-first message reads

`client.messages.conversations(...)` returns durable local conversations from `im-core`. Current schema version 34 reads conversation existence from the schema-28 `conversation_registry` and left-joins the message-derived `conversation_summaries`, so a validated empty conversation remains visible. Protocol/control records (including group lifecycle records) do not materialize a message summary; until the first user-visible message, `messageCount` remains `0` and `lastMessage` remains `null`. The API is paged: pass `cursor: page.nextCursor` to continue, and stop when `hasMore` is false or `nextCursor` is null. A single page is capped at 100 items by `PageLimit::new`. The cursor is opaque and follows `activity_at DESC, conversation_id DESC`; callers must not parse it or treat it as an offset.

Before opening a newly resolved Direct conversation or a newly created/joined Group conversation, commit its existence:

```dart
await client.messages.ensureConversation(canonicalConversationId);
```

The call is idempotent and fail-closed: Direct requires an owner-scoped
peer-scope route bound to a verified `peerPersonaId`; Group requires an active
local membership addressed by canonical Group DID. App-local rows may be used
only as a temporary optimistic overlay while this call completes.

Generated `DartConversation` and `DartConversationSnapshotItem` now expose a
required `conversationId`, optional `peerPersonaId` / `canonicalGroupDid`, and a
required `resolutionState`. A resolved Direct must have `peerPersonaId`; a
resolved Group must have `canonicalGroupDid`. New App code must not fall back to
`threadId` when any of these canonical facts is missing. `DartGroupMember`
separates `membershipId`, `peerPersonaId`, and `credentialDid`; the credential
DID is not the membership identity. Conversation and snapshot projections may
carry an optional `title`; for Group rows this is the committed local Group
profile display name, not an App-local custom title.

Conversation list startup and realtime updates use snapshot / patch helpers under
the same `client.messages` namespace:

```dart
final snapshot = await client.messages.loadConversationSnapshot();
await client.messages.clearConversationSnapshot();
final patches = client.messages.watchConversationPatches();
final repair = await client.messages.repairConversationStore();
final timelinePatches = client.messages.watchConversationTimelinePatches(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  limit: 100,
);
final timelineRepair = await client.messages.repairConversationTimelineStore(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  limit: 100,
);
```

`loadConversationSnapshot` reads a non-authoritative redb snapshot generated from
committed `conversation_summaries`. `clearConversationSnapshot` only clears that
discardable snapshot cache for the current owner; it does not clear SQLite local
projection, runtime store, read state, or reliable checkpoint. `watchConversationPatches` streams versioned
`ConversationStorePatch` values (`reset`, `upsert`, `remove`, `reorder`,
`repairRequired`) emitted only after the underlying local projection commit
succeeds. The conversation store is keyed only by canonical `conversationId`;
`remove` and `reorder` carry that ID instead of thread kind/id or a legacy alias.
The bound watch subscribes before reading and emits exactly one initial `reset`
from the canonical SQLite projection. The non-authoritative redb snapshot is
available only through the explicit `loadConversationSnapshot` legacy
acceleration API and is not used as an initial watch seed.
After a committed invalidation, the runtime store compares the full projected
items with its current state. Identical items do not advance the store version
and do not emit a synthetic `reset`; one-row material changes emit
`upsert`/`remove`, while material multi-row changes may emit `reset`. Explicit
repair, lag, overflow, stream-close, and version-gap paths keep their repair
semantics.
Snapshot format v3 invalidates older discardable redb snapshots and rebuilds them
from SQLite so Group first paint retains the committed profile title across a
restart. `repairConversationStore` returns a reset/repair patch after lag,
overflow, stream close, or version gaps. `watchConversationTimelinePatches` and
`repairConversationTimelineStore` expose the same committed-projection rule for an
opened conversation timeline keyed by `ConversationReadRef.conversationId`;
remote history best-effort pages or realtime hints must not become authoritative
timeline patches before persistence succeeds. `watchThreadPatches(ThreadRef)` and
`repairThreadStore(ThreadRef)` remain compatibility adapters for CLI/legacy paths,
not the AWiki Me display-chain owner. A
realtime incoming message becomes patch-visible only after `im-core` has
committed its SQLite local projection; failed or skipped realtime projection
does not emit an authoritative conversation/thread patch.

Cancelling any of these Dart patch streams is a native lifecycle barrier. The
bridge retains a cancellation signal and the Rust worker handle after stream
attachment, wakes an idle `next_patch()` call, and joins the worker before the
stop call completes. Conversation-list, conversation-timeline, and legacy
thread patch streams use the same rule; an attached stream must never make its
stop API a no-op.

Remote history, conversation catch-up, and realtime incoming messages share one
Core canonical-ingress gate. A Direct wire DID must resolve to a verified
Persona before the message row is committed. Until then Core stores the record
in its durable resolution backlog and exposes neither a `dm:<DID>` conversation
nor a timeline patch; verified Persona projection later replays it under the
single canonical conversation ID.

For an online first inbound Direct, Core resolves the wire peer DID through the
Handle authority and commits that verified Persona projection before the
message. A missing, conflicting, malformed, or DID-mismatched lookup remains in
the durable backlog and emits no authoritative patch; later `syncNow` calls retry
a bounded pending set and atomically replay the message plus its remote-thread
binding after verified Persona projection. The SDK never synthesizes a Persona
from the DID itself.

These APIs currently live under `client.messages` for SDK compatibility. If a
future `client.conversations` namespace is added, it must wrap the same core
store and DTOs rather than introducing another source of truth.

Conversation snapshot / store DTOs are core-only. They must not carry
`awiki-me` App-only fields such as `hidden`, `pinned`, `muted`, `customTitle`,
`avatarSeed`, `peerLifecycleState`, `ConversationSummary`, or `ChatMessage`.
AWiki Me applies those presentation fields in its own application layer; see
`awiki-me/docs/conversation-presentation-ownership.md`.

`client.messages.localConversationTimeline(conversation, limit: ..., cursor: ...)`
is the App timeline first-paint API. It reads the committed local projection by
canonical `conversationId` and does not call remote history. `localHistory(thread, ...)`
and `history(thread, ...)` remain migration adapters:

- `localConversationTimeline` / `localHistory` read only the local SQLite projection and return an opaque local cursor;
- local timeline APIs return only complete hydrated rows. A metadata-only sync discovery may already increase conversation activity/unread counts while its body is still absent; the SDK does not expose that placeholder as a normal Message;
- it does not call message-service history RPCs, directory lookup, or remote E2EE projection;
- it is the correct API for chat first paint before background reconcile;
- `history` keeps remote history + projection/reconcile semantics and should be called in the background when freshness is required.

Both APIs are async `Future<MessagePage>` methods. Apps must not bypass the SDK or read SQLite directly.

## Conversation send and local echo

Conversation UI sends should use the conversationId-first APIs when the App already has a selected conversation:

```dart
final sent = await client.messages.sendConversationText(
  const SendConversationTextRequest(
    conversation: ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    text: 'hello',
  ),
);

final payload = await client.messages.sendConversationPayload(
  const SendConversationPayloadRequest(
    conversation: ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    payloadJson: '{"text":"@agents summarize","mentions":[]}',
  ),
);
```

`im-core` resolves the conversation to the storage route, persists a pending local
projection row, emits patches after the local transaction succeeds, and updates
the same durable row to accepted/sent/failed after network send. Apps render
`Message.metadata.sendState` / retry data from the SDK message DTO. They must not
create a second durable optimistic message store for text or payload sends.

Conversation UI attachment sends use the attachment namespace with the same
conversation identity rule:

```dart
final attachment = await client.attachments.sendConversation(
  SendConversationAttachmentRequest(
    conversation: const ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    input: const AttachmentInput.bytes(
      filename: 'note.txt',
      mimeType: 'text/plain',
      bytes: [104, 101, 108, 108, 111],
    ),
    caption: 'hello',
    clientMessageId: 'msg-app-attachment-001',
    idempotencyKey: 'op-msg-app-attachment-001',
  ),
);
```

`client.attachments.sendConversation(...)` resolves the canonical
`ConversationReadRef.conversationId` inside `im-core`, writes the durable message
projection, and returns the same `AttachmentSendResult` shape as the legacy
target API. AWiki Me conversation UI should use this API for initial attachment
sends and retries. Local file previews may be rendered as transient UI state
while upload is in progress, but list/detail/send correctness must come from the
SDK projection and patch stream.

## Conversation read watermark

Conversation-level mark-read is exposed as a watermark-first message API:

```dart
final result = await client.messages.markConversationRead(
  const ConversationReadRef(
    conversationId: 'dm:peer-scope:v1:alice:bob',
  ),
  watermark: const ReadWatermark(
    lastReadThreadSeq: '991',
    lastReadMessageId: 'msg_direct_991',
  ),
  fallbackMaxMessageIds: 500,
);
```

`watermark` is optional. When the App omits it, the SDK computes the highest
visible committed thread watermark from `im-core` local projection / thread
store. App code must not page through `history()`, read SQLite, or collect
unread message ids just to clear unread state.

Watermark semantics:

- direct threads use direct message `server_seq`;
- group threads use the group thread-local `server_seq` projection, which may be
  backed by `group_event_seq` on the service side;
- neither value is the account-level reliable sync `event_seq`;
- `lastReadMessageId` is for idempotency and diagnostics, not the ordering source;
- `readAt` is an audit/display timestamp and does not participate in
  authorization or checkpoint logic.

The SDK first uses the service `read_state.mark_read` contract. When the service
does not support the endpoint, the SDK falls back to local unread-id lookup and
legacy direct `inbox.mark_read(message_ids)`. Group fallback on an old service is
local/pending only. Results must expose `updatedCount`, `remoteAcknowledged`,
`partial`, `fallbackUsed`, `pendingRemoteAck`, `effectiveWatermark`, and
`warnings`; any returned message ids are legacy fallback diagnostics only.
`effectiveWatermark` is the locally committed read watermark. Therefore
`remoteAcknowledged == false && pendingRemoteAck == true` is still local-first
success when `effectiveWatermark` covers the requested target; callers must not
discard the result or convert it into a void fire-and-forget boundary.

`markConversationRead` does not expose or advance the reliable sync checkpoint.
`remoteAcknowledged` and `pendingRemoteAck` describe only read-ack state.
`markThreadRead(ThreadRef)` remains available as a CLI/legacy adapter, but AWiki Me
must route visible conversations by `ConversationReadRef.conversationId`.

## Reliable message sync

Reliable sync is exposed as high-level async message APIs. The Dart SDK must not expose
SQLite, WebSocket frames, `since_event_seq`, `next_event_seq`, or checkpoint
load/store primitives.

Schema 32 introduces Core-private v2 binding, cursor, applied-event receipt,
recovery, and read-mutation outbox tables; schema 33 adds the private per-owner
bootstrap installation ID. Schema 34 adds owner-scoped remote thread bindings,
the unresolved Direct read-state backlog, and monotonic remote read-state
versions. Schema 35 durably associates an unresolved message with its opaque
remote-thread binding so Persona replay writes both against one canonical
conversation. `syncNow` is the v2 ordinary Direct/Group main path; none of those
rows, account/device binding, cursor, recovery token, snapshot cutoff/limit, or
snapshot count is exposed to Dart. `syncDelta` remains a separate v1
compatibility facade. Message edit, recall, delete, tombstone, Push, and
E2EE/MLS multi-device synchronization remain outside this stage.

`syncNow` 的 ordinary account stream 明确不包含 Direct E2EE/P5 ciphertext。Native Dart
wrapper 在 P5 gate 开启时，会先使用 exact-device、
`body.security_profile=direct-e2ee` 的本域 secure hydration，再在 Core 内重新加载同一
stable identity 的 client，最后执行 ordinary `syncNow`；这确保 Root 导入推进设备认证代次后
普通同步不会继续使用旧 client。Rust CLI 前台 Inbox 遵循相同顺序。
该“重新加载”在 Dart bridge 中不是无条件替换：Core 先验证同一 Core、owner、DID、账号和
Protocol Device，再把新授权 runtime 绑定到原有 conversation/message/system-notification
Store。由此，刷新前已建立的 Patch session 保持同一 Store 和单调版本；任何 scope 不一致都
fail closed。
secure hydration 在每页本地提交后只 ACK 已成功消费的 P5 raw delivery，并有 100 页硬上限；
ACK/收敛失败保留已提交本地数据但不返回完整前台成功。
该窄化方法不是 ordinary/Legacy Inbox fallback，也不新增独立 Dart public API；它是
`MessageApi.syncNow` 内部的 Core-owned 前置阶段。Flutter
不得把空 local projection 当成是否需要 E2EE hydration 的启发式判断。

AWiki Me 默认启用 `syncNow`，并对所有合法账号/设备 binding 使用同一协议；Dart SDK 不暴露
账号 allowlist、设备 cohort 或百分比 rollout 参数。显式关闭只用于全局应急回滚，P5/P6
E2EE 的独立默认关闭开关不受影响。

Expected public shape:

```dart
final outcome = await client.messages.syncNow(
  const MessageSyncRequest(
    limit: 100,
    reason: 'app_resume',
  ),
);

final page = await client.messages.syncConversationAfter(
  SyncConversationAfterRequest(
    conversation: const ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:alice:bob',
    ),
    afterServerSeq: '991',
    limit: 100,
  ),
);

final diagnostics = await client.messages.syncDiagnostics();
```

`syncNow` semantics:

- Rust `im-core` reads the active account/device binding and v2 cursor from
  private SQLite; Dart supplies only `reason` and optional `limit`.
- When fenced or missing, Rust runs `sync.bootstrap`. A new device takes the
  tail-only path and atomically commits binding, Group/read baseline, and cursor.
  An existing-device upgrade may instead enter compact recovery without first
  inventing a local cursor.
- Rust exactly hydrates all required `message.created` events through
  `message.get_batch` in ordered chunks of 8, leaving compact-JSON
  framing/escaping headroom under the service's 16 MiB hard response budget,
  then atomically commits receipts, canonical local projections, and the next
  cursor only after all chunks are complete.
- Required `group.member_changed` and `group.profile_updated` events atomically
  commit both Group state and one read `awiki.group.system_event.v1` timeline
  message. Its canonical `<group_did>:<group_event_seq>` ID makes local,
  realtime, v1, and v2 arrival order idempotent. Flutter reads the result from
  the normal local timeline; it does not synthesize or merge a second event.
- Required hydration, schema, owner mismatch, or non-resolution route failure
  leaves both receipt and cursor unchanged. A peer that is only
  `identity_unresolved` is different: Core atomically stores the message and
  remote-thread binding in the durable resolution backlog together with the
  receipt/cursor advance, then retries authoritative DID-to-Handle resolution
  on later sync calls.
- A compact-recovery response is handled inside the same call:
  delta/bootstrap → process-local opaque token → strict snapshot validation and
  atomic merge → post-anchor delta. The snapshot merges current Direct/Group
  read state, active Group state, and recent ordinary messages without deleting
  older local messages. The successful call ends in the existing
  `MessageSyncStatus.changed` or `MessageSyncStatus.idle`; snapshot or
  post-anchor failure ends in `retryableFailure`, while an authorization or
  generation fence ends in `authRevoked`. HTTP 401/403 from authenticated
  sync or its JWT refresh, JSON-RPC `1401` after Core's bounded auth retry,
  and the live Registry fence codes `anp.device_not_eligible` /
  `anp.device_state_changed` are also terminal `authRevoked`, not retryable
  network failures. There is no second recover API.
- The raw recovery token remains on the Rust process stack only and is never
  written to SQLite or logs. Dart cannot observe or persist the token, recovery
  cursor/anchor, cutoff, policy limit, or returned snapshot count.
- Core serializes recovery per owner and commits a snapshot only when the
  previous cursor, recovery-id hash, authorized anchor, and recovery phase still
  match in SQLite. Snapshot decoding is closed-schema, rejects duplicate
  event IDs/sequences and pre-cutoff messages, and never lets Dart choose the
  48-hour/500-message policy.
- `committedIncomingMessages` contains only incoming messages whose projection
  transaction committed, with `CommittedMessageSource.liveDelta`; realtime
  hints, snapshot hydration, and Group system timeline records never appear in
  this list.
- Local read advancement and durable outbox enqueue are one SQLite transaction.
  Unsent entries coalesce to their maximum watermark; an in-flight payload is
  immutable and a higher watermark creates a successor. Startup changes stale
  in-flight rows to retryable. A service response or authenticated remote
  read-state event atomically MAX-merges the watermark, updates message
  projections, and acknowledges covered outbox entries. Every retry reuses the
  outbox operation ID as ANP `meta.operation_id`; the v2 body carries only the
  user DID, opaque `{ kind, thread_key }`, and watermark fields, with no
  account/device/origin selector or duplicate body operation ID. Before send,
  Core claims the exact operation; an in-flight predecessor blocks its
  successor. Core accepts an ACK only when the closed response echoes the exact
  DID/thread, is final and non-partial, and its server watermark covers the sent
  watermark. Transport, parse, validation, and local-commit failures all return
  the claim to retryable state.
- An unresolved Direct read state is durably keyed by its opaque service
  conversation reference. Snapshot/cursor commit is allowed even when no recent
  message establishes the canonical local conversation; the first later
  ordinary message binding replays and removes the backlog in the same
  transaction. Core never guesses this binding from a DID.
- `message.read_state_updated` requires an explicit `thread_kind`; read-only
  sync commits publish the same conversation/thread patch contract as message
  pages, so Flutter never needs to poll or merge read state itself. Group read
  invalidations always use the canonical `group:<group_did>` conversation ID,
  including replicas that have not yet persisted a remote-thread binding.
- `syncDelta` retains the earlier v1 checkpoint behavior and remains isolated
  from the v2 cursor.
- `syncDiagnostics()` performs a local-only read and returns typed
  `lastSuccessAt`, sync/recovery `mode`, `pendingMutationCount`,
  `dirtyDomains`, `retryState`, and optional `nextRetryAt`. The model is safe
  for product status UI: it contains no raw cursor/epoch, full account/device
  identifier, recovery token/anchor/hash, message body/payload, or auth token.
  Flutter code must not reconstruct those private values from other DTOs.
- Successful delta/snapshot cleanup is bounded and best-effort inside Core.
  Flutter does not receive a cleanup control API and must not interpret cleanup
  failure as rollback of a committed sync. Terminal mutation/recovery records
  are retained for seven days; live pending/in-flight/retryable work remains
  durable.

`syncDelta` compatibility semantics:

- Rust `im-core` reads the current global message checkpoint from local SQLite.
- The checkpoint is partitioned by stable local `ownerIdentityId` and the
  service event-stream subject. The current service uses canonical DID as that
  subject, so a recovered DID starts at `0` and never inherits the old DID's
  event sequence.
- Rust `im-core` sends `sync.delta` to the home message service and injects
  `since_event_seq` internally.
- Rust `im-core` applies all returned events and advances the checkpoint only after
  the local apply transaction succeeds.
- For an owner-scoped ordinary P3 Direct event that carries metadata but no body,
  Rust checks the exact `message_id` and `server_seq`, resolves the peer through
  the authoritative Handle directory, groups missing targets by Direct peer,
  and hydrates `direct.get_history` from immediately before that peer's earliest
  missing sequence. The verified peer scope is resolved once per authoritative
  history page and reused for its messages, avoiding both one history request
  per metadata event and one directory lookup per message. A later local thread
  sequence does not satisfy the exact-message check. The checkpoint advances
  only after every required message in the page is committed; hydration failure
  leaves it unchanged. This plain-message recovery path does not create or select
  P5 E2EE. A stable conversation ID remains a presentation/storage route:
  ordinary Direct history is persisted with immutable `direct + peer DID` wire
  identity so it can merge with the sender device's local projection without
  weakening wire-conflict checks.
- If the service returns `snapshot_required=true`, the SDK returns a failed-closed
  result: no checkpoint advance, no local projection wipe, and diagnostic fields for
  the App to surface a degraded sync state.
- Direct metadata-only hydration is entirely Core-owned. Hosts receive a successful
  delta only after all exact plain Direct targets are committed; there is no App-side
  second hydration loop or public list of pending conversation IDs.
- Dart callers can choose `limit` and `reason`; they cannot choose or store the
  v1 checkpoint or v2 cursor.

`syncConversationAfter` semantics:

- It is a thread-local freshness API for direct/group chat surfaces.
- Blocking and async Core use the account-authorized, plain-only
  `sync.thread_after` service method. The private request body contains exactly
  `thread_key`, `after_server_seq`, and `limit`; no account, device, peer DID, or
  Group DID selector is accepted. Direct resolves `thread_key` only through the
  durable canonical-conversation binding and fails closed if that authoritative
  binding is absent; Group uses its Group DID as the thread key.
- `afterServerSeq` is a thread-local message sequence, not the account-level
  `event_seq`.
- `afterServerSeq` is a caller freshness hint. If Core has an earlier durable
  hydration gap, both blocking and async implementations clamp the effective
  cursor to `min(afterServerSeq, earliestGap - 1)`; when omitted, Core starts
  before the earliest gap or at the local maximum when no gap exists.
- It does not read or advance the reliable global checkpoint.
- The returned page contains only `server_seq > effectiveAfterServerSeq`
  messages in ascending `server_seq` order. It can therefore include a missing
  sequence below the caller's original hint; `nextAfterServerSeq` is also based
  on the effective cursor.
- Core defensively admits only ordinary plaintext Direct/Group messages.
  E2EE/MLS/device-ciphertext rows are discarded even if a service page includes
  them.
- Returned messages are not UI truth until `im-core` persists them to the local
  projection and the App reloads/repairs through the conversation timeline.
`syncThreadAfter(ThreadRef)` remains a compatibility wrapper and should not be the
AWiki Me display-chain routing owner.

`discovered`, `hydrated`, and migration-only `legacyProbe` are Core-private
SQLite recovery states, not Dart Message fields. Dart/App code must not infer
them from an empty body, store a parallel hydration flag, or advance a cursor
past a Core-known gap. A metadata-only duplicate cannot erase an existing body;
a full history/catch-up projection hydrates the same message ID in place.

Realtime integration:

- Realtime events may include a readonly `RealtimeSyncHint` with `eventId`,
  `eventSeq`, and `eventType`.
- App code may use the hint to schedule `syncDelta` after duplicate/gap/dirty
  detection.
- Receiving a realtime hint or successfully projecting a realtime notification must
  not advance the reliable checkpoint.
- Successfully projecting a realtime incoming message to local SQLite does emit
  committed conversation/thread patches for active subscribers; the hint alone
  is never an authoritative patch source.
- Before reliable sync supplies a thread-local `serverSequence`, an incoming
  realtime projection uses the recipient-side receive timestamp rather than the
  sender-provided `sentAt`. Once two timeline rows both have `serverSequence`,
  consumers must order them by that sequence and use time only as the fallback
  for mixed or legacy rows.

The SDK must not add public APIs named `loadGlobalCheckpoint`, `storeGlobalCheckpoint`,
`setGlobalCheckpoint`, or equivalents. Any checkpoint inspection needed for debugging
must stay behind internal diagnostics or test-only interfaces.

## Message payloads and ANP P9 mentions

`SendPayloadRequest.payloadJson` accepts any JSON object up to the SDK payload
size limit. It no longer requires a top-level `schema` field. Existing
`awiki.agent.*` control payloads may continue to include `schema`, but App code
must not add a fake `schema`/`protocol` field merely to send ANP-P9 mention
payloads.

The Dart SDK exposes ANP-P9 mention DTO helpers:

```dart
final payload = MessageMentionPayload(
  text: '@agents please summarize this discussion.',
  mentions: const [
    MessageMention(
      id: 'men_1',
      range: MessageMentionRange(start: 0, end: 7),
      target: MessageMentionTarget.groupSelector(MessageMentionSelector.agents),
    ),
  ],
);

validateMessageMentionPayloadJson(payload.toPayloadJson());
await client.messages.sendPayload(
  SendPayloadRequest(
    target: const MessageTarget.group('did:wba:example.com:group:team'),
    security: MessageSecurityMode.defaultPlain,
    payloadJson: payload.toPayloadJson(),
  ),
);
```

P9 mention DTOs intentionally do not add sender, proof, profile, content type, or
selector expansion fields. Single-target identity is the target DID; optional
`displayName` is only a UI snapshot and must not be used for routing,
authentication, authorization, E2EE binding, or runtime policy decisions.

Flutter/App integration gates:

```bash
cd packages/awiki_im_core && flutter test test/message_payload_api_test.dart
cd ../.. && scripts/flutter/codegen-check.sh
```

AWiki Me adds App-level composer, mapper, and highlight coverage in
`awiki-me` focused tests. The desktop App + CLI peer group scenario sends a
schema-less `@agents` P9 payload through `MessagingService.sendMentionText` and
verifies the projected payload text can be read back by both the App and CLI
history. Daemon prompt execution is validated separately by
`cargo test -p awiki-deamon --locked mention` and the dedicated Agent IM /
daemon integration gate.

## Realtime ownership

The native SDK exposes realtime as a high-level session and event stream:

```dart
final capability = await client.realtime.capability();
if (capability.runnerExposed) {
  final session = await client.realtime.start();
  final eventsSub = client.events.listen((event) {
    // MessageReceived / GroupUpdated / HostNotification / connection state, etc.
  });
  final stateSub = client.connectionStates.listen((state) {
    // connected / reconnecting / closed, without transport details.
  });

  await session.stop();
  await eventsSub.cancel();
  await stateSub.cancel();
}
```

WebSocket remains an `im-core` internal transport concern. Transport details such as WebSocket URLs, raw frames, ping/pong, request IDs, bearer headers, and dispatch queues are internal to `im-core` and must not become Dart public API. App code should configure only `AwikiImCoreConfig.transportPolicy` and consume `client.events` / `client.connectionStates`.

`AwikiImClient` owns one serialized logical Realtime lifecycle. `start`,
`stop`, `dispose`, and the secure-Inbox prelude of `syncNow` cannot overlap.
When Core reports a real authorization-context change, the SDK stops the old
native session and starts one replacement with the same `RealtimeOptions`,
while preserving the public event streams and logical `RealtimeSession`
handle. Native callbacks are generation-fenced, so an obsolete session cannot
publish after restart. An equivalent identity reload does not restart
Realtime. Native shutdown stops the Realtime session before cancelling its
Dart event subscription because the native stream closes as a consequence of
that stop; reversing the order can make subscription cancellation wait on the
stop that has not yet run. Both cleanup steps are still attempted and the first
failure is reported.

For any native client with an exact vNext account/device binding, Core requires
the server to echo `awiki.sync.changed.v2`. `NoSubProtocol` is surfaced as a
typed transport failure and is never retried as a Legacy connection. Flutter
must not add a client-kind flag or fallback switch for this behavior.

Flutter Web still receives a stub and does not support native realtime.

## Attachments And E2EE

`client.attachments.sendConversation(SendConversationAttachmentRequest(...))` is the conversationId-first attachment send API for apps that already have a selected conversation. `client.attachments.send(AttachmentSendRequest(...))` remains a high-level target-first compatibility facade for CLI, daemon, legacy callers, or surfaces that do not yet hold a canonical `ConversationReadRef`. `AttachmentSendRequest.security` defaults to `MessageSecurityMode.defaultPlain`; callers can set `MessageSecurityMode.e2eeRequired` for direct or group E2EE attachment messages.

Plain attachment messages use `application/anp-attachment-manifest+json` with a JSON
manifest payload. Realtime, read, sync, conversation snapshots, and local
projection paths must expose that manifest as `MessageBodyView.payload`, while
also attaching the attachment summary and download action when available. The
manifest content type is not an unsupported body type.

Secure attachment sends do not expose P7 control-plane calls, download tickets, object keys, nonces, raw ciphertext, secure session state, or MLS provider paths to Dart. `AttachmentSendResult.manifestJson` is the public redacted manifest projection. For E2EE attachments it may include `encryption_info.mode = object-e2ee`, `object_cipher`, and `plaintext_size`, but must not contain `object_key_b64u` or `nonce_b64u`.

`UploadedAttachment.sizeBytes` / `size` describe the uploaded object bytes. For `object-e2ee` this is ciphertext size. `UploadedAttachment.plaintextSizeBytes` carries the original plaintext size when available.

App 下载必须优先传 `AttachmentDestination.localFile(path)`，避免对象经过
`Rust Vec -> FFI -> Dart Uint8List -> file` 的完整内存复制。Core 把未完成字节保存在固定
`$path.awiki-part`，自动重取 ticket、请求 Range、校验 size/digest，并在成功后原子发布
`path`。Dart 只读取暂存文件长度展示进度，不参与拼接或完整性判断。
Windows Core 在原子发布前使用扩展长度绝对路径，因此长 storage scope / message ID 缓存
路径不要求用户修改系统 `LongPathsEnabled`。

```dart
final download = client.attachments.download(
  DownloadAttachmentRequest(
    thread: thread,
    messageId: messageId,
    destination: AttachmentDestination.localFile(stagingPath),
    overwrite: true,
  ),
);

// 用户主动暂停；返回 true 表示找到了该路径的活动传输。
final cancelled = await client.attachments.cancelDownload(stagingPath);
```

主动取消以 `attachment_transfer_cancelled` 结束原 Future，但 `.awiki-part` 保留；再次对同一路径
调用 `download` 即继续。App 不应为主动取消显示失败提示。网络中断、30 秒无字节进度、响应提前
结束和 Range 不一致分别使用稳定的 `attachment_transfer_*` 错误码，Host 可提示用户重试，
但不得自行绕过 Core 校验后发布部分文件。

## Codegen

Generated files are committed so the package can be checked out and analyzed without requiring codegen first:

- `crates/im-core-dart/src/frb_generated.rs`
- `packages/awiki_im_core/lib/src/generated/bridge_generated.dart`

Run:

```bash
scripts/flutter/codegen-check.sh
```

`codegen-check.sh` runs the bridge generator and fails if the committed generated Rust/Dart files are not already in sync. If `flutter_rust_bridge_codegen` CLI flags change, update this script but keep the same input/output paths.

## Build commands

Rebuild all native SDK artifacts after Rust SDK changes:

```bash
scripts/flutter/build-sdk-native.sh
```

The one-step script runs:

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-apple.sh
scripts/flutter/build-android.sh
```

The Android and Linux shared libraries, Apple XCFrameworks, and Windows DLL
include the compile-time `group-e2ee` implementation.
Product discovery and use remain controlled by
`multiDeviceGroupE2eeEnabled`; compiling the feature does not enable it by
default.

Linux native artifacts are host-specific and are built explicitly on Linux.
Single-platform builds remain available:

```bash
scripts/flutter/build-sdk-native.sh --macos-only
scripts/flutter/build-sdk-native.sh --ios-only
scripts/flutter/build-sdk-native.sh --android-only
scripts/flutter/build-sdk-native.sh --linux-only
```

The platform-only commands preserve the complete SDK artifact by default. A
consumer that intentionally ships one native architecture can request a thin
artifact without changing that default:

```bash
scripts/flutter/build-sdk-native.sh --macos-only --macos-arch arm64
scripts/flutter/build-sdk-native.sh --macos-only --macos-arch x86_64
scripts/flutter/build-sdk-native.sh --android-only --android-abi arm64-v8a
```

Full Android builds require `cargo-ndk`. Full Apple builds must run on macOS with Xcode and Rust Apple targets installed. Linux native builds must run on Linux with the Flutter Linux desktop prerequisites available in the consuming app, such as `clang`, `cmake`, `ninja-build`, `pkg-config`, GTK development headers, and Xvfb for headless integration tests. Use `--dry-run` to print the selected build steps without compiling native artifacts.

The iOS XCFramework targets iOS 13+ for physical devices and x86_64 simulators. The arm64 simulator slice targets iOS 14+, matching the platform's availability. `build-apple.sh` passes those minimum versions to C dependencies as well as Cargo and rejects an archive containing a higher minimum OS before packaging. Override them only for an explicit compatibility test with `AWIKI_IOS_DEPLOYMENT_TARGET` and `AWIKI_IOS_ARM64_SIMULATOR_DEPLOYMENT_TARGET`.

Linux builds generate:

```text
packages/awiki_im_core/linux/lib/libawiki_im_core.so
```

The file is copied from `target/<target>/release/libawiki_im_core.so` and is ignored by git, matching the existing Android and Apple native artifact policy. The package's `linux/CMakeLists.txt` adds the generated `.so` to `awiki_im_core_bundled_libraries`, so Flutter installs it into the app bundle's `lib/` directory. The Dart loader opens it as `libawiki_im_core.so`, relying on the Flutter Linux runner's `$ORIGIN/lib` rpath.

## Common local errors

- Missing `../anp/anp/rust` sibling checkout: this workspace depends on a sibling ANP Rust crate.
- Missing `cargo-ndk`: required for full Android native library builds.
- Linux `libawiki_im_core.so` missing from the app bundle or native smoke fails to load it: run `scripts/flutter/build-sdk-native.sh --linux-only` before testing a Flutter Linux app that calls `AwikiImCore.open`.
- Linux dynamic library load error: verify the app bundle contains `lib/libawiki_im_core.so` and that the Flutter Linux runner preserves `$ORIGIN/lib` in `CMAKE_INSTALL_RPATH`.
- iOS symbols not found: verify the podspec vendored XCFramework path and `-force_load` slice path.
- FRB generated files stale: run `scripts/flutter/codegen-check.sh`.
