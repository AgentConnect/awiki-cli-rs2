# Identity Secret Storage And SecretVault

Status: active；多设备 V1 目标待 Review
Authority: authoritative for `awiki-cli-rs2` client-side identity secret storage

本文档记录端侧私钥和相关 secret 的本地持久化架构。它覆盖
`im-core`、CLI、Dart/Flutter SDK、AWiki Me 和 `awiki-deamon`。除第 4.1 节外，各节主要记录
当前实现边界；第 4.1 节固定待落地的多设备 V1 目标。当前代码若与第 4.1 节冲突，应视为实施差距，
不能反向把旧 ACK、sidecar 或 refresh-token 状态机解释为目标架构。

## 1. 快速结论

当前基础与 V1 目标结论：

- `im-core` identity private material 已有 `SecretVault` contract；V1 要求 host 使用
  `VaultRequired` 时，新注册、Legacy 升级、设备 Join/管理员升级、daemon subkey package
  persistence 和 access-token 替换都不写新明文私钥/JWT 文件。
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
| CLI identity | CLI 注册、Legacy 升级及既有兼容命令得到的 identity 私钥和 auth state | `crates/awiki-cli` + `im-core` | 默认 `secret_storage.mode=vault_required`；root key 来自 env 或本地私有 root-key 文件 |
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
- 现有 `vault_migration` metadata 保持 legacy schema 不变；vNext refs 的正式 metadata 持久化与迁移由统一 `register`、Legacy 升级、Join 和管理员升级流程接入。key role、ref 和 provider 类型均为 im-core 内部边界，不进入 ANP wire、DID Document 或 App 公共 DTO。
- identity vault status/migrate/verify 只返回 backend、metadata、warning、missing items 和兼容文件保留状态，不返回 root key、private PEM、JWT、完整 `SecretRef` 或 ciphertext。

三种 identity secret storage policy：

| Policy | 用途 | 安全语义 |
|---|---|---|
| `FileCompat` | 兼容旧身份和显式 legacy 迁移 | 不启用 vault；不能作为私钥安全验收证据 |
| `VaultPreferred` | 迁移期检查/过渡 | 有 vault context 时使用 vault；缺失时可能暴露 legacy 状态，不能作为最终安全闭环 |
| `VaultRequired` | 新工作区和生产 App/CLI | 缺 root key、缺 vault context、metadata 不匹配、metadata 损坏或 verify 失败时 fail closed，不回退新明文持久化 |

`VaultRequired` 下新注册、Legacy 升级、设备 Join/管理员升级、daemon subkey package persistence
和 access-token 替换都必须走 vault-backed persistence，不写新的明文私钥/JWT 文件。

### 4.1 多设备管理设备的 DID root 导入

AWiki 的 root 导入是 V1 域内能力，复用普通设备级 P5 Direct E2EE，不扩展 ANP P5、AAD
或跨域协议。DID root private key 与用于打开本地 Vault 的
`DeviceVaultRootKey` / KEK 是两种不同密钥：

- DID root private key 是用户身份管理能力；
- DeviceVaultRootKey/KEK 是当前宿主的本地加密根；
- 接收设备只把 DID root 作为 `IdentityRootPrivate` seal 到自己的 SecretVault；
- Identity index 只保存 `SecretRef` 和无秘密状态，不保存 root 明文。

发送端只有在当前 Registry 确认为 ready admin、目标仍为 member、Manifest/key 绑定有效且
普通 P5 Session 或 PreKey 已经准备好后，才允许请求用户对本次 exact-device 传输确认一次。
V1 不为根传输增加系统 PIN/生物识别步骤。
已有 Session 时发送标准 Cipher；没有 Session 时，标准 Init 的第一个业务明文直接携带
RootKeyEnvelope。V1 不发送空 Init、不要求第二次确认，也不建立 root 专用
`delivery_class`、sidecar、Mailbox 或 Ratchet。

Core 自己拥有的 RootKeyEnvelope、`SecretBytes` 和序列化缓冲区使用 zeroizing secret 类型，
并尽量缩短第三方密钥解析对象的作用域。这里不宣称第三方解析库内部的所有临时分配都具备
zeroize 保证。root 明文只允许短暂存在于发送端 Vault → P5 AEAD 和接收端 P5 AEAD → Vault
边界，不进入普通消息、History、通知预览、搜索、日志、崩溃报告、Dart/CLI DTO 或常规备份。

接收端在普通消息投影前截获合法 RootKeyEnvelope，重新验证当前 DID Document、Registry、
两端设备/key、P5 AAD、expiry 和 root public fingerprint。验证完成后的本地状态为：

```text
root capability = pending
Registry         = active member
```

pending root 必须与 active root 使用不同的 capability/ref 状态。普通
`did_document_root_private_pem` accessor 不得向一般 DID 管理流程返回 pending root；只有本次
`device_root_import_complete` proof builder 可以在严格绑定的操作范围内读取它。

root 导入的本地逻辑事务至少提交：

- seal-if-absent 的 pending root Vault record；
- 已消费的 P5 `message_id` 和 exact-device 绑定；
- Document/Registry checkpoint；
- 完整且可精确重试的 completion params/proofs 或其受保护记录；
- completion 状态和 expiry；
- 指向 pending record 的 `SecretRef`。

文件型 Vault 与 Identity index 不能依赖“两个 rename 天然原子”。实现必须在共享
IdentityIndex mutation lock 内使用短生命周期 reservation、幂等 seal-if-absent、原子 index
replace 和写后校验形成明确线性化点。进程在 Vault seal 后、index commit 前退出时，只能在重新
验证后复用内容和 context 完全相同的 record，绝不能覆盖不同 root、重新消费 P5 ratchet key 或
生成另一份 completion。

本地事务成功后，新设备通过 HTTPS 提交一次
`device_root_import_complete`：

- 外层使用当前 importing-device signing key 的 ANP Object Proof；
- 内层使用 pending DID root 的 ANP Object Proof；
- `operation_id` 固定为 RootKeyEnvelope 的 P5 `message_id`；
- 网络失败或响应不确定时，重发完全相同的 params 和 proofs。

User Service 只有在重新验证当前 Manifest/Registry、两层 proof 和普通 P5 可信路由 tuple 后，
才能在一个事务中把 member 直接改为 `admin + management_ready=true` 并递增
`auth_generation`。V1 不产生 `admin + management_ready=false`，不发送 E2EE imported ACK，
也不以 P5 Reply、HTTP accepted 或系统通知作为 readiness 事实。

completion 返回后，Core 必须重新读取 Registry：

- 远端仍是 member：保持 pending，不开放管理能力；
- 远端成为 ready admin 且 active root 可重新打开：在本地事务中把 pending ref 提升为 active；
- completion 终态失败或过期：删除本次 pending root 和待完成状态，设备继续作为 member；
- 响应不确定：按同一 operation 精确重试，并以 Registry 事实收敛。

只有“Registry ready admin + 本地 active root 可读”同时成立，host 才能看到 ready admin。
密文删除和可选的 `root-key-imported` 系统通知在远端事务后幂等收敛，不参与本地 root 激活的
授权判断。

管理员升级后，Core 使用明确的本机 device signing key 发起 fresh DID-WBA User Service
请求，从标准认证响应头取得新的管理 access token。`get_me` 只是没有其他业务 RPC 时的
bootstrap。Bearer 请求不续期，V1 不保存 rotating refresh token，也不调用
`device_token_issue` 或 `device_token_refresh`。

access auth state 只保存一个当前 access token。替换前必须核对返回 token 的 profile/purpose、
DID、user、device、key、`auth_generation`、scopes、audience 和 expiry，并在 identity mutation
lock 下以当前 auth ref/version 做 CAS。写入后重新打开核验；错误 claims、迟到响应或写入失败都
fail closed。JWT payload 的端内解析只承担结构和授权绑定检查，不宣称替代业务服务的 JWT
签名验证。

所有 IdentityStore index 的 load-modify-save、root promotion、access-token 替换和本地
identity 删除路径共享同一 OS mutation lock；index 使用 private temp file、`fsync` 和跨平台
atomic replace。schema、lock、pending、Document/Registry checkpoint 和 `SecretRef` 都是端内
实现状态，不进入 DID Document、ANP wire 或 App 公共 DTO。

普通 identity save 不得借相同 key id 覆盖设备私钥、root ref、pending completion 或 auth
state。对已有 vNext identity 的重存必须核对 alias、DID、unique id、Vault context、全部 key
material 和当前权威 refs，并原样保留未被本次专用 mutation 改变的状态。

旧实现中的 `root_key_import` imported-completion/ACK、management-token issue operation、
refresh-token pair 和 completion sidecar 不是目标 schema。实现切换对应模块时必须迁移或删除
这些旧字段、DTO、恢复分支和测试，不能让它们继续成为第二套运行时状态机。

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
- V1 多设备新身份使用 `id register` 并通过 `VaultRequired` 进入 `im-core`。
- 旧 `id recover` / Handle Recovery 入口已从 V1 CLI 和 Core facade 删除；未来 Recovery
  必须作为独立安全方案重新设计，不能复用 Join 或 Legacy 升级。

CLI root key 来源：

1. 优先读取环境变量 `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64`，值必须是 base64/base64url 编码的 32-byte root key。
2. 如果环境变量不存在，读取 `secret_storage.vault_dir/root-key.b64u`。
3. 如果普通 SDK open、注册等 live 路径需要 root key 且本地文件不存在，CLI 会创建本地私有 root-key 文件。
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
