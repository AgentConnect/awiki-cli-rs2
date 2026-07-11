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
- daemon 的 agent identity 私钥、Message Agent delegated 私钥和 `agent_auth_state.jwt_token` 已按 sentinel + `SecretRef` 方式保存，真实 secret seal 到 daemon SecretVault。
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
| Message Agent delegated key | `user_delegated_identity.private_key_material` | `crates/awiki-deamon` | sentinel + `private_key_ref_json`；真实 delegated private key seal 到 daemon SecretVault |
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
- identity vault status/migrate/verify 只返回 backend、metadata、warning、missing items 和兼容文件保留状态，不返回 root key、private PEM、JWT、完整 `SecretRef` 或 ciphertext。

三种 identity secret storage policy：

| Policy | 用途 | 安全语义 |
|---|---|---|
| `FileCompat` | 兼容旧身份和显式 legacy 迁移 | 不启用 vault；不能作为私钥安全验收证据 |
| `VaultPreferred` | 迁移期检查/过渡 | 有 vault context 时使用 vault；缺失时可能暴露 legacy 状态，不能作为最终安全闭环 |
| `VaultRequired` | 新工作区和生产 App/CLI | 缺 root key、缺 vault context、metadata 不匹配、metadata 损坏或 verify 失败时 fail closed，不回退新明文持久化 |

`VaultRequired` 下注册、恢复、daemon subkey package persistence 和 JWT/token refresh 都必须走 vault-backed persistence，不写新的明文私钥/JWT 文件。

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

AWiki Me 通过 Dart SDK 打开 `im-core`，生产路径使用 `VaultRequired`：

```text
StoredAwikiImCoreVaultSecretProvider
  -> one namespace-scoped secret bundle
  -> AwikiImCoreOpenOptions.vaultRequired
  -> im-core identity SecretVault
```

当前 root key 持久化策略：

- 生产和普通 custom state root 使用 `SecureAppKeyValueStore`，macOS 上写入一个
  `awiki_me.im_core.identity_vault.<namespace>.secrets_v1` Keychain item。
- `secrets_v1` 是结构化 JSON bundle，包含 `schema`、`root_key_b64` 和 `device_id`。
- 新版本不迁移、不读取旧的 `.root_key_b64` / `.device_id` 拆分 key；这是一次不向后兼容的
  AWiki Me 本地 vault secret 存储模型调整。
- 只有显式 E2E mode，也就是设置 `AWIKI_E2E_APP_STATE_ROOT` 时，才使用 `awiki_me_im_core_vault.json` 私有 file test provider。
- 普通 `appStateRoot` override 不会把 root key 移到 JSON。
- E2E JSON 可能包含 `secrets_v1` bundle 以及其中的 base64 root key，必须保持 local/untracked。

App state namespace 决定 vault 路径和 workspace id：

```text
<app support>/im-core/<namespace>/identity-vault
vaultWorkspaceId = awiki-me-<namespace>
deviceId = app-device-<stable-random>
```

身份激活前必须先验证 vault：

```text
identityVaultStatus
  -> migrateIdentityVault when legacy metadata is absent
  -> verifyIdentityVault
  -> switchIdentity
  -> ensureSession
```

如果已有 vault metadata 但不能选中或验证，App fail closed，不使用新 root key 重新 seal 旧明文。

详细 App 接入说明见 `awiki-me/docs/identity-secret-storage.md`。

### 7.1 AWiki Me 首发 Storage Scope host contract

`release/0707` 与 `release/0710` 尚未上线。AWiki Me 已批准首个正式版本采用
UUID Storage Scope clean cut；当前 namespace-scoped 代码将在 App 实施步骤中替换。
完整 App schema 和 lifecycle 权威位于
`awiki-me/docs/storage-scope-vault-contract.md`。本文件只冻结 im-core 的 host-neutral 边界：

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
