# Identity Secret Storage And SecretVault

Status: active  
Authority: authoritative for `awiki-cli-rs2` client-side identity secret storage

本文档记录当前端侧私钥和相关 secret 的本地持久化方案。它覆盖
`im-core`、CLI、Dart/Flutter SDK、AWiki Me 和 `awiki-deamon` 的当前实现边界。
一次性执行计划和历史 plan 只作为背景；如果与本文档或当前代码冲突，以本文档和当前代码为准。

## 1. 快速结论

当前安全结论：

- `im-core` identity private material 已有 `SecretVault` contract；host 使用 `VaultRequired` 时，新注册、恢复、daemon subkey package persistence 和 JWT/token refresh 不写新明文私钥/JWT 文件。
- CLI 新 workspace 默认使用 `secret_storage.mode=vault_required`。root key 优先来自 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`，否则来自 `vault_dir/root-key.b64u` 本地私有文件；root key 不进入 workspace config、doctor 或 JSON/human 输出。
- AWiki Me 生产路径使用 Dart SDK `VaultRequired`，App-local root key 存在平台 secure storage；只有显式 E2E state root 使用私有 JSON test provider。
- daemon 的 agent identity 私钥、Personal Agent delegated 私钥和 `agent_auth_state.jwt_token` 已按 sentinel + `SecretRef` 方式保存，真实 secret seal 到 daemon SecretVault。
- Direct E2EE session/prekey local state 已通过 SecretVault envelope 加密落盘。

仍需单独规划的事项：

- App -> daemon bootstrap 的 daemon subkey private key 传输仍是临时明文 DTO；daemon 持久化必须加密，传输加密后续改为端到端加密 bootstrap envelope。
- `file:` / `local:` / 裸路径 delegated `key_ref` 仍作为兼容入口存在；新 daemon-owned delegated key 应使用 `vault:`。
- root key rotation、backup/recovery UX、安全擦除、Group MLS private state、外部签名 API 和 DID 子密钥授权撤销语义不在本文档覆盖的已完成范围内。

## 2. 权威来源和放置位置

主技术文档放在 `awiki-cli-rs2/docs/architecture/identity-secret-storage.md`，原因是：

- `crates/im-core` 拥有 `SecretVault`、`KeyMaterialProvider`、identity vault status/migrate/verify、Direct E2EE 本地 secret envelope 和 SDK public contract。
- `crates/awiki-cli` 拥有 CLI `secret_storage` 配置、root key 来源、`id vault *` 命令和 redacted diagnostics。
- `crates/awiki-deamon` 与 CLI 平行复用 `im-core`，但有自己的 daemon SecretVault 和 `daemon.db` secret ref 存储。
- `crates/im-core-dart` 与 `packages/awiki_im_core` 只暴露 Dart/Flutter SDK facade，不负责 root key 的生成、持久化、备份或恢复。

App 侧的具体接入说明放在 `awiki-me/docs/identity-secret-storage.md`。`awiki-harness` 只维护索引和跨仓库摘要，不复制本文件正文。

## 3. 当前加密持久化范围

下表列出当前已经纳入加密落盘或 vault ref 管理的 secret：

| 范围 | Secret | 当前 owner | 当前持久化 |
|---|---|---|---|
| Identity | DID auth/signing private key、E2EE static signing/agreement key、identity auth/JWT state | `im-core` | `SecretVault`；host 使用 `VaultRequired` 时不写新明文 PEM/JWT |
| CLI identity | CLI 注册/恢复得到的 identity 私钥和 auth state | `crates/awiki-cli` + `im-core` | 默认 `secret_storage.mode=vault_required`；root key 来自 env 或本地私有 root-key 文件 |
| App identity | AWiki Me 当前账号 identity 私钥和 auth state | `awiki-me` host + `im-core` | App 使用 Dart SDK `VaultRequired`，root key 放在平台 secure storage；E2E 才使用私有 JSON test provider |
| Daemon agent | daemon/runtime agent DID 私钥 | `crates/awiki-deamon` | `daemon.db` 只保存 `<awiki-secret-vault-ref>` sentinel 和 `SecretRef` JSON；真实私钥 seal 到 daemon SecretVault |
| Personal Agent delegated key | `user_delegated_identity.private_key_material` | `crates/awiki-deamon` | sentinel + `private_key_ref_json`；真实 delegated private key seal 到 daemon SecretVault |
| Daemon auth token | `agent_auth_state.jwt_token` | `crates/awiki-deamon` | `jwt_token` 列只保存 sentinel；真实 bearer token seal 到 daemon SecretVault |
| Direct E2EE | session state、signed prekey private key、one-time prekey private key | `im-core` secure direct local state | SQLite 中保存 SecretVault envelope；新写入没有 vault 时拒绝明文 fallback |
| Daemon delegated inbox | `inbox_auth_key_ref` | daemon + `im-core` | 新路径使用 `vault:` key ref，密钥 seal 到 `im-core` file vault |

不在本轮范围内或仍是已知例外：

- App -> daemon bootstrap DTO 中的 `user_subkey_package.private_key_pem` 传输仍可为明文；daemon 接收后的持久化必须进入 vault。传输层后续应改为端到端加密 bootstrap envelope。
- `file:`、`local:` 和裸路径 delegated `key_ref` 仍作为 caller-provided 兼容入口存在；新 daemon-owned delegated key 应使用 `vault:`。
- CLI / App / daemon root key rotation、backup、recovery UX 和安全擦除尚未完成。
- Group MLS private state 不在本轮 SecretVault 加固范围内。
- 外部签名服务、公共签名 API、DID 子密钥授权撤销语义不是本文件范围。

## 4. `im-core` SecretVault 方案

`im-core` 暴露窄口的 no-prompt local secret vault API，入口是 `crates/im-core/src/vault.rs`。加密记录格式和 crypto 实现仍留在 `im-core` internal，host 只提供 root key 和上下文。

核心约束：

- host 打开 SDK 时可以传 `ImCoreOpenOptions` 和 `ImCoreSecretVaultOptions`。
- `ImCoreSecretVaultOptions` 包含 32-byte `DeviceVaultRootKey`、`vault_dir`、`workspace_id` 和 `device_id`。
- `SecretVault` 保存每条 secret 的 AEAD ciphertext，并把 workspace、device、identity、DID、kind、key id/version、schema、cipher、KDF 和 no-prompt policy 绑定到认证 metadata。
- DID-WBA auth、业务签名、secure direct static key material 读取路径收敛到内部 `KeyMaterialProvider`，业务流程不应直接读 `private.key`、`key-*-private.pem`、`e2ee-agreement-private.pem` 或 `auth.json`。
- `KeyMaterialProvider` 明确区分 `device_request_signing_private_pem` 与 `did_document_root_private_pem`：前者只服务登录、HTTP auth、Direct、Group 和 Attachment 等日常设备请求；后者只服务 DID Document 创建、重签和更新。调用方不得用其中一个 accessor 代替另一个。
- legacy File/Hosted/Vault 身份仍只有同一把 `key-1`，只能经内部 `LegacyKey1RoleAdapter` 显式映射为上述两个角色；这是兼容语义，不得用于创建新的多设备身份。
- vNext Vault 使用 `IdentityDeviceSigningPrivate` 保存设备签名密钥，并采用 side-by-side `VNextVaultKeyMaterialRefs`：设备签名 ref 必需，DID root ref 可空。普通 member 没有 root ref 时仍可进行日常签名，调用 DID root accessor 必须 fail closed。root/device signing 的 `SecretKind` 不可互换。
- 现有 `vault_migration` metadata 保持 legacy schema 不变；vNext refs 的正式 metadata 持久化与迁移由后续 genesis/Join 切片接入。本切片的 key role、ref 和 provider 类型均为 im-core 内部边界，不进入 ANP wire、DID Document 或 App 公共 DTO。
- identity vault status/migrate/verify 只返回 backend、metadata、warning、missing items 和兼容文件保留状态，不返回 root key、private PEM、JWT、完整 `SecretRef` 或 ciphertext。

三种 identity secret storage policy：

| Policy | 用途 | 安全语义 |
|---|---|---|
| `FileCompat` | 兼容旧身份和显式 legacy 迁移 | 不启用 vault；不能作为私钥安全验收证据 |
| `VaultPreferred` | 迁移期检查/过渡 | 有 vault context 时使用 vault；缺失时可能暴露 legacy 状态，不能作为最终安全闭环 |
| `VaultRequired` | 新工作区和生产 App/CLI | 缺 root key、缺 vault context、metadata 不匹配、metadata 损坏或 verify 失败时 fail closed，不回退新明文持久化 |

`VaultRequired` 下注册、恢复、daemon subkey package persistence 和 JWT/token refresh 都必须走 vault-backed persistence，不写新的明文私钥/JWT 文件。

### 4.1 多设备管理设备的 DID root 导入

AWiki 的 root 导入是域内、默认关闭的能力，边界停在现有设备级 Direct E2EE
结构化 JSON 的加解密 hook，不扩展 ANP P5、AAD 或跨域协议。DID root private key
与用于打开本地 Vault 的 `DeviceVaultRootKey` / KEK 是两种不同密钥：前者只作为
`IdentityRootPrivate` secret seal 到接收设备的 `SecretVault`，Identity Registry 只保存
`SecretRef`，Vault record 由本地 KEK 加密。Core 自己拥有的 RootKeyEnvelope 字符串、
`SecretBytes` 和序列化缓冲区使用 zeroizing secret 类型，并尽量缩短第三方密钥解析对象的
作用域；这里不宣称第三方解析库内部的所有临时分配都具备 zeroize 保证。根密钥明文只允许
短暂存在于 Direct AEAD 边界，不进入普通消息、日志、通知、崩溃报告或常规备份。

root 导入的本地非秘密状态包括：

- Identity index schema v5 的可选 `root_key_import`：保存已消费的 Direct `message_id`、
  DID/root key id、内部 Document version/hash、两端 device、expiry，以及唯一的设备签名
  imported completion；不保存 root private key。
- identity 目录中的短生命周期 pending reservation：只用于 Vault seal 与 index commit
  之间的崩溃恢复，不包含私钥或 Direct 明文。完全相同 reservation 可恢复；未过期冲突
  fail closed；旧 reservation 过期后，若当前接收设备仍是 not-ready admin，新的合法
  `message_id` 可在确认 Vault 中是同一 root 后原子替换 ACK reservation，且不重新 seal。
- imported completion 是唯一业务 ACK。相同 Envelope/ACK 重放复用首次持久化的完整签名
  声明，不重新 seal 或覆盖 root；同一 `message_id` 的不同 Envelope 被拒绝。
- `root_key_import.management_token_operation_id` 是端内保存的 token-issue 幂等状态，不是
  ANP、DID Document 或跨域字段。首次签发前仅在确认尚无持久化值时生成，并在远端调用前
  原子落盘；精确重试复用原值，只有服务端明确返回该 operation 已过期时才通过 CAS 轮换。
- management-ready 响应丢失时，旧 generation bearer 只通过不刷新、不签名降级、不持久化
  响应 token 的临时 transport 探测。仅精确 JSON-RPC 码 `device.auth_generation_stale` 可证明
  generation 已推进；`device.unauthenticated`、HTTP 401 和其他错误都 fail closed。
- V1 不猜测“服务端已 ready、端内尚未 commit”这一不确定状态。如果此时旧 generation bearer
  已过期且服务端不能返回上述精确 stale code，流程保持 fail closed，用户必须先重新认证或进入
  显式恢复流程；扩大服务端幂等查询/恢复时间窗留待后续版本。

一致性顺序为：在共享 IdentityIndex mutation lock 内验证当前 Document/Registry 和设备资格，
写入非秘密 pending reservation，以 seal-if-absent 发布 root Vault record，再原子写入同一份 index image（root
`SecretRef` + consumed reservation + signed completion + checkpoint），最后尽力删除 pending。
index image 是本地导入线性化点。若进程在 Vault seal 后、index commit 前退出，后续只能在
重新验证后的 root 与既有 Vault record 完全相同时复用该 record，绝不覆盖不同 root。
通过 Registry checkpoint 校验的当前 DID Document 会在 index commit 后原子更新到本地
projection；若 projection 写入失败，导入仍按已提交状态处理，并由同一 Envelope 的精确重放
重试投影。文件型 provider 每次读取该 projection，因此当前已打开的 client 无需重建即可看到
最新文档。
management-ready ACK 可能对应比 Envelope 更新的 Document checkpoint。端内先验证解析到的
Document 与 Registry hash、当前设备 key/role/generation，再提交 auth/checkpoint，最后原子更新
Document projection；投影失败返回固定的 exact-retry 本地错误。重试进入 AlreadyConverged 时
仍须重新解析并校验当前 Document/Registry 后修复 projection，但不得再次签发 management token，
也不得改变已提交的 auth ref、operation id 或 authorization generation；同版本不同 hash 拒绝。
该投影重试本身不刷新 access token；若持久化 token 已过期，先由正常会话刷新路径更新 auth
state，再以新的当前 bearer 重试投影，避免把 token refresh 隐式混入 root-import 幂等状态机。
正常刷新按 identity 模式分流：legacy identity 继续使用 DID-auth `get_me`；vNext identity 必须从
权威 Vault auth state 读取 rotating refresh token，调用同域内部
`device_token_refresh(operation_id, refresh_token)`，不得回退设备签名。operation id 由当前 refresh
token 确定性派生，因此丢响应只会形成服务端 exact replay。客户端在写入前严格核对返回 token pair
的 profile、access/refresh purpose、DID、user、device、key、auth generation、scopes、audience、
expiry 与当前授权，并拒绝旧 refresh token 重放。验证通过后，在同一个权威 auth
`SecretRef`/`key_version` 的实例锁及 identity 目录专用 0600 文件锁（跨 provider/进程）边界内
按旧 refresh 摘要做 CAS，一次性替换 access 与 rotated
refresh token；当前已是同一目标 pair 时幂等成功，auth state 已前进时拒绝迟到响应，随后重新打开写后核验；
401、错误 claims 或写入失败都 fail closed，响应 header token 不得进入 vNext auth state。该步骤
不得改变 root-import operation id、authorization generation 或 Identity index 中的 auth ref。
端内 JWT payload 检查只承担结构和授权绑定校验，不声称本地完成 JWT 签名验证：旧 refresh
来自权威 Vault record，并由 user-service 在 refresh RPC 中验签、消费 JTI 和复核实时 Registry；
轮换结果来自配置的 TLS endpoint，后续业务服务仍会独立验签 access token。
同一进程中长期存活的 vNext `KeyMaterialProvider` 还维护可推进的 root/auth `SecretRef`
指针；推进前严格核对 Vault context、identity、DID、secret kind、key id/version 并成功
解封目标记录。导入 root ref 只接受 `key_version=1`，并从私钥重算公钥指纹核对已签名的
imported completion。fresh import、Envelope replay 和精确恢复都从 Identity index 的权威 ref
修复当前 provider，因此导入后不要求重建 client。被替代记录不会在推进时立即删除，避免
破坏仍引用旧 ref 的其他 live provider，后续由单独的 Vault compaction 处理。
V1 明确约束每个本地 identity 只有一个活动 runtime。多个同时存活的 client/provider 不保证
内存 `SecretRef` 广播收敛；非当前执行 runtime 需在重开或自身 exact-retry 时修复。跨 runtime
主动广播留作后续版本，不在本期增加全局协调器。
本流程只面向使用 canonical `did_document.json` 的新建 vNext identity；同时保留 legacy
`did.json` 的旧身份仍由兼容 adapter 处理，不进入本期 root-import 核心状态机，避免双文件
读取优先级造成 projection 分歧。

所有 IdentityStore index 的 load-modify-save，以及本地 identity 删除路径，共享同一 OS
mutation lock；index 本身通过 private temp file、`fsync` 和跨平台 atomic replace 替换
（Unix rename；Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`）。前者防止
并发 stale writer 丢失 root `SecretRef`、completion、token/device state 等字段，后者防止
崩溃留下截断 JSON。上述 schema、lock、pending 和 Document version/hash 都是端内实现状态，
不进入 DID Document、ANP SDK model 或跨域 wire。

已有 `root_key_import` 的 identity 只能以同 alias、DID、unique id、Vault context 和同一组
设备密钥做 rootless vNext 重存。重存前会验证现有 Vault root 与 DID public 一致，并逐字节
验证设备私钥；auth state 按当前 bearer 语义验证，而不是把可能包含 refresh token/expiry 的
版本化 JSON 与 access-only JSON 逐字节比较。重存原样保留全部 Vault refs、refresh token 和
import record；普通 save 不允许借同一 key id 覆盖设备私钥，token 轮换继续使用专用 mutation
路径。

## 5. CLI 当前方案

CLI 的配置入口是 workspace `config.yaml` 的 `secret_storage`：

```yaml
secret_storage:
  mode: vault_required
  vault_dir: .awiki/identity-vault
  workspace_id: cli-workspace-id
  device_id: cli-device-id
```

当前默认策略：

- 新写入的 workspace config 默认 `mode: vault_required`。
- `file_compat` 只作为显式 legacy plaintext storage 兼容开关；`id create` / `id import-v1` 这类 legacy 明文身份写入入口需要显式 `file_compat`。
- 新身份应使用 `id register` 或 `id recover`，并通过 `VaultRequired` 进入 `im-core`。

CLI root key 来源：

1. 优先读取环境变量 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`，值必须是 base64/base64url 编码的 32-byte root key。
2. 如果环境变量不存在，读取 `secret_storage.vault_dir/root-key.b64u`。
3. 如果普通 SDK open、注册或恢复等 live 路径需要 root key 且本地文件不存在，CLI 会创建本地私有 root-key 文件。
4. `id vault status` 和 dry-run mutation 只报告计划，不创建 root-key 文件。

重要事项：

- root key 永远不写入 `config.yaml`。
- `config show`、doctor、diagnostics、JSON output、human output、error 和 dry-run plan 只能显示 root key 是否存在和来源，不能显示 root key 原文。
- `id vault status` 可以在缺少 root key material 时以 redacted `checked_without_vault_context` 方式检查配置和 legacy 状态。
- `id vault migrate` live path 必须有 vault root key；dry-run 只输出 redacted plan。
- `id vault cleanup-plaintext` 当前是 migration-gated / preflight surface；本 build 还没有 CLI-safe live plaintext cleanup API，不能宣称会删除 legacy compatibility files。

## 6. Dart/Flutter SDK 当前方案

`packages/awiki_im_core` 暴露 Dart open options：

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

SDK facade 只负责把 host 提供的 root key 和 vault context 传给 Rust：

- 不生成 root key。
- 不持久化 root key。
- 不提供 root key rotation/backup/recovery。
- 不暴露 generic secret open API、private PEM、JWT 原文、bearer token persistence path 或完整 `SecretRef`。
- `identityVaultStatus`、`migrateIdentityVault`、`verifyIdentityVault` 只返回 redacted DTO。

Flutter Web 是 stub，不能运行 native vault-backed backend。

## 7. AWiki Me 当前方案

AWiki Me 通过 Dart SDK 以 `VaultRequired` 打开 `im-core`。当前首发实现已采用 UUID Storage Scope clean cut：

```text
Tenant Registry
  -> immutable storage_scope_id
    -> storage-scopes/<uuid>/im-core/identity-vault
    -> platform account scope/<uuid>
    -> workspace awiki-me.scope.v1.<uuid>
    -> device context awiki-me.scope-device.v1.<uuid>
      -> AwikiImCoreOpenOptions.vaultRequired
```

App 只有显式 `StorageScopeProvisioner` 可以通过 exclusive create 生成 scope root key；runtime 只调用
`readExisting`，不会 get-or-create、扫描旧 namespace 或执行 identity migration。生产与开发使用不同
application identity / Keychain service；只有显式 `AWIKI_E2E_APP_STATE_ROOT` 使用权限受限的 file test
provider。tenant display name、backend URL、DID host、`awiki.ai` 和 `tenant-default` 都不参与
path、Keychain account 或 vault context 派生。

完整 App schema、lifecycle 与平台 locator 权威位于
`awiki-me/docs/storage-scope-vault-contract.md`、`awiki-me/docs/identity-secret-storage.md` 和
`awiki-me/docs/scope-secret-platform.md`。本文件只冻结 im-core 的 host-neutral 边界。


- App 的 `tenant_profile_id`、`storage_scope_id`、registry、manifest 和平台 Keychain
  locator 属于 AWiki Me，不进入 `im-core` public model。
- App 从不可变 scope UUID 确定性派生并永久冻结 host context：

  ```text
  workspace_id = awiki-me.scope.v1.<scope_uuid>
  device_id    = awiki-me.scope-device.v1.<scope_uuid>
  ```

- `ImCoreSecretVaultOptions` 继续只接收 host 提供的 root key、vault directory、
  workspace ID 和 device ID；SDK 不生成、不持久化、不恢复 App root key。
- runtime open 只能使用 host 的 `readExisting` 结果。只有 App 的显式 scope
  provisioning transaction 可以 `createExclusive` root key。已有 scope 缺 key、ACL denied、
  envelope corrupt、context mismatch 或 verify failure 都必须 fail closed。
- Production 不保留 `awiki.ai`、`tenant-default`、split key 或 namespace bundle resolver；
  这些只属于预发布 developer archive/reset 范围。
- App、CLI、daemon 继续使用彼此独立的 root-key provider 和 vault context。
- im-core diagnostics 必须保持 redacted，并允许 host 区分 vault unavailable、metadata
  unverified、workspace/device context mismatch 和 record open/verification failure；不得要求
  host 解析 human error string。

Host 调用 `verify_identity_vault` 时使用以下稳定 error code 分支，human message 仅用于诊断，
不得作为程序判断依据：

| Dart error code | 含义 | Host 动作 |
|---|---|---|
| `identity_vault_unavailable` | 未提供 SecretVault context | fail closed，检查 host secret provider |
| `identity_vault_metadata_missing` | Identity Registry 缺少 Vault metadata | fail closed；仅显式 provisioning/migration 可创建 metadata |
| `identity_vault_metadata_unverified` | metadata 未进入 verified 状态 | fail closed，不回退明文 |
| `identity_vault_workspace_mismatch` | host workspace 与 metadata 不一致 | fail closed，检查 scope/context 派生 |
| `identity_vault_device_mismatch` | host device 与 metadata 不一致 | fail closed，检查 scope/context 派生 |
| `identity_vault_record_open_failed` | record 缺失、不可读、损坏或 AEAD 打开失败 | fail closed；该错误刻意不区分错误 root key 与密文损坏 |
| `identity_vault_verification_failed` | record 打开后内容/身份验证失败 | fail closed，保留原数据供诊断 |

`IdentityVaultStatus.missing` 对 workspace/device 分别使用
`identity_vault_workspace_match` 和 `identity_vault_device_match`。这些是 machine-readable
诊断 item；warning 和 `Display` 文本不能参与 host 控制流。为避免 oracle 和 secret 泄漏，
wrong root key 与 AEAD/ciphertext integrity failure 统一报告为
`identity_vault_record_open_failed`。

该 contract 不改变 SecretVault record schema，也不把 App scope UUID作为新的 im-core
领域类型。未来 App root rotation、backup 或 database encryption 必须在同一 scope/account
内版本化演进；这些能力落地前不得在 SDK 文档中宣称已经支持。

## 8. Daemon 当前方案

daemon 有两个 vault 相关边界：

1. daemon 自己的 `DaemonSecretVault`，root key 来自 `AWIKI_DAEMON_VAULT_ROOT_KEY_B64`。
2. daemon 使用 `im-core` file vault 的路径，例如 delegated inbox `vault:` key ref，需要 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` 或对应 host root key source。

`daemon.db` 当前 secret 存储规则：

- `agent_identity.auth_private_key_pem`、`e2ee_signing_private_key_pem`、`e2ee_agreement_private_key_pem` 只保存 `<awiki-secret-vault-ref>` sentinel。
- 对应 `*_private_key_ref_json` 保存 `SecretRef` JSON，真实私钥 seal 到 daemon SecretVault。
- `user_delegated_identity.private_key_material` 只保存 sentinel，对应 `private_key_ref_json` 保存 `SecretRef`。
- `agent_auth_state.jwt_token` 只保存 sentinel，对应 `jwt_token_ref_json` 保存 `SecretRef`，真实 bearer token seal 到 daemon SecretVault。
- 缺少 daemon vault root key 或缺少 ref 时，secret 读取/持久化 fail closed，不回退明文。
- `im_core_adapter` 的 Message/im-core SDK 主路径使用 hosted in-memory identity material，不写 `private.key`、`e2ee-agreement-private.pem` 或 `auth.json`。
- user-service inventory DID-auth 使用内存态 `DidAuthMaterial` 签名，不再通过兼容 PEM/auth.json 文件落盘。

daemon runtime backend 不能持有 DID 私钥，不能直连 message-service，也不能绕过 daemon local RPC 回传状态、final 或消息。

## 9. Direct E2EE 本地状态

Direct E2EE 不只需要加密传输，也需要加密本地保存。本地 session state、signed prekey private key 和 one-time prekey private key 都是长期或中期敏感材料，泄露后可能影响后续解密、身份冒充或重放防护。

当前 `im-core` secure direct local state 规则：

- 新写入的 direct session/prekey secret 通过 SecretVault envelope 保存到 SQLite blob。
- 没有 SecretVault context 时，新 direct secret write 拒绝明文 fallback。
- 历史明文 blob 仍有兼容读路径，用于旧数据迁移/过渡；不能把这个兼容读路径作为新写入方案。
- Debug/status/report 不应输出 session id 之外的 secret state、prekey private material 或 plaintext。

## 10. 进程和系统级密钥边界

App、CLI 和 daemon 是不同宿主进程。当前设计不假设一个系统 keychain item 能被所有进程无弹窗共享读取：

- CLI 使用 CLI workspace 的 root key source。
- AWiki Me 使用 App-local platform secure storage 或 E2E private file provider。
- daemon 使用 `AWIKI_DAEMON_VAULT_ROOT_KEY_B64` 打开 daemon SecretVault，并在需要 `im-core` file vault 时提供 `im-core` root key。

这意味着“用户登录后无弹窗自动读取”的实现必须由每个宿主进程自己的 no-prompt root key provider 保证，不能依赖另一个进程已经解锁过的系统密钥项。

## 11. 日志、诊断和测试红线

不得输出到日志、audit、UI、E2E report、doctor、JSON/human output、panic/debug dump：

- root key bytes 或 base64/root-key 文件内容；
- private PEM、private key multibase、raw `SecretRef` JSON；
- JWT、bearer token、Authorization header；
- Direct E2EE session/prekey private material；
- App bootstrap private DTO 内容；
- 本机 secret 文件原文。

允许输出：

- mode、backend、vault available、metadata present/verified；
- workspace/device id 等非 secret context；
- missing item 和 warning code；
- redacted sentinel，例如 `<awiki-secret-vault-ref>`；
- root key source 名称，例如 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`、`local_private_file`、`local_private_file_pending_create`。

## 12. 关键验证入口

代码变更时至少选择相关 focused gate：

```bash
cd awiki-cli-rs2
cargo test -p awiki-im-core --test vault_api --locked -j1
cargo test -p awiki-im-core identity_vault --locked -j1
cargo test -p awiki-cli --test identity_cli_surface_contract --locked -j1
cargo test -p awiki-cli --test diagnostics_contract --locked -j1
cargo test -p awiki-deamon --locked -j1 secret_vault
cargo test -p awiki-deamon --locked -j1 agent_auth_token
cargo test -p im-core-dart --locked -j1
```

App 侧变更还需要：

```bash
cd awiki-me
dart analyze
dart run tests/unit/runner.dart
dart run tests/e2e/runner.dart --case full
```

系统测试按 workspace 约定使用 remote `awiki.info`：

```bash
cd awiki-system-test
AWIKI_SYSTEM_TEST_MODE=remote \
E2E_DID_DOMAIN=awiki.info \
E2E_USER_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_URL=https://awiki.info \
E2E_MESSAGE_SERVICE_WS_URL=wss://awiki.info/im/ws \
uv run awiki-system-test --show-command
```

## 13. 相关文档

- `docs/architecture/im-core-sdk-architecture.md`
- `docs/api/im-core-interface/03-identity-auth-interface.md`
- `docs/api/im-core-interface/05-cli-adapter-interface.md`
- `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`
- `packages/awiki_im_core/README.md`
- `crates/awiki-deamon/docs/local-dev.md`
- `awiki-me/docs/identity-secret-storage.md`
- `awiki-me/docs/testing.md`
