# 03. Identity & Auth Interface

## 1. Identity Selector

`identity/dto.rs`：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentitySelector {
    Default,
    Id(crate::ids::IdentityId),
    Did(crate::ids::Did),
    Handle(crate::ids::Handle),
    LocalAlias(String),
}
```

说明：

- `LocalAlias` 对应 CLI `--identity alice` / 本地 credential name。
- 不使用 `Name(String)`，避免和 profile display name 混淆。
- `Default` 只是一种 selector，不是 SDK 全局可变当前身份。

## 2. Identity Summary

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentitySummary {
    pub id: crate::ids::IdentityId,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub readiness: IdentityReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityReadiness {
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<IdentityMissingItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityMissingItem {
    DidDocument,
    PrivateKey,
    AuthState,
    Handle,
    MessageEndpoint,
    Other(String),
}
```

P1 public summary 不包含：

```text
private key path
DID document path
auth token
auth path
secure state path
SQLite owner filter
```

## 2.1 Identity Secret Storage

Identity private material is still an internal SDK concern. Hosts that need
vault-backed persistence open the SDK with `ImCoreOpenOptions`:

```rust
let options = ImCoreOpenOptions::default().with_identity_secret_vault(
    IdentitySecretStoragePolicy::VaultRequired,
    ImCoreSecretVaultOptions::new(root_key, vault_dir, workspace_id, device_id),
);
let core = ImCore::open_with_options(config, paths, options).await?;
```

Policy semantics:

- `FileCompat`: compatibility default when no explicit open options are passed.
- `VaultPreferred`: migration-period mode; may report legacy/file state and must
  not be used as final proof that private material is safe.
- `VaultRequired`: missing root key, missing vault context, corrupt metadata, or
  wrong workspace/device context fails closed instead of falling back to new
  plaintext persistence.

The root key is a host-provided no-prompt secret. It is not part of
`ImCoreConfig`, identity summaries, public auth DTOs, CLI config output, or Dart
model serialization.

## 3. Identity Registry

`identity/registry.rs`：

```rust
pub struct IdentityRegistry<'a> {
    pub(crate) core: &'a crate::core::ImCore,
}

impl IdentityRegistry<'_> {
    pub fn list(&self) -> crate::ImResult<Vec<IdentitySummary>>;

    pub fn default_identity(&self) -> crate::ImResult<Option<IdentitySummary>>;

    pub fn resolve(&self, selector: IdentitySelector) -> crate::ImResult<IdentitySummary>;

    pub fn vault_status(
        &self,
        selector: IdentitySelector,
    ) -> crate::ImResult<IdentityVaultStatus>;

    pub fn migrate_identity_vault(
        &self,
        selector: IdentitySelector,
    ) -> crate::ImResult<IdentityVaultMigrationReport>;

    pub fn verify_identity_vault(
        &self,
        selector: IdentitySelector,
    ) -> crate::ImResult<IdentityVaultVerificationReport>;

    pub fn register_handle(
        &self,
        request: RegisterHandleRequest,
    ) -> crate::ImResult<HandleRegistrationResult>;

    pub fn plan_default_identity_change(
        &self,
        selector: IdentitySelector,
    ) -> crate::ImResult<DefaultIdentityChange>;

    pub(crate) fn load_runtime(
        &self,
        selector: IdentitySelector,
    ) -> crate::ImResult<crate::internal::identity_runtime::ClientIdentityRuntime>;
}
```

`load_runtime` 只能是 `pub(crate)`，供 `ImCore::client` 使用。

Vault DTO boundary:

- `IdentityVaultStatus` reports the selected backend (`file_compat` or `vault`),
  storage policy, vault availability, metadata presence/verification,
  workspace/device summary, plaintext compatibility retention, warnings, and
  missing items.
- `IdentityVaultMigrationReport` reports whether migration ran, whether the
  result verified, and whether plaintext compatibility files remain.
- `IdentityVaultVerificationReport` verifies that the selected identity can be
  opened through the vault-backed provider.
- Verification failures use stable typed `ImError::IdentityVault` reasons. The
  Dart facade maps them to `identity_vault_unavailable`,
  `identity_vault_metadata_missing`, `identity_vault_metadata_unverified`,
  `identity_vault_workspace_mismatch`, `identity_vault_device_mismatch`,
  `identity_vault_record_open_failed`, or
  `identity_vault_verification_failed`. Hosts must branch on the code, not the
  human message. Wrong root keys and corrupt/authentication-failing records
  intentionally share `identity_vault_record_open_failed`.
- These DTOs do not expose root keys, JWTs, private PEM, full `SecretRef` JSON,
  ciphertext internals, or local auth/token file contents.

`VaultRequired` registration, recovery, daemon subkey package persistence, and
JWT/token refresh must persist secret material through SecretVault. Existing
legacy PEM/auth.json files are a compatibility bridge until explicit cleanup is
available; migration failure must not delete them.

## 4. Register Handle

P1 只做注册，不做 recover/replace DID。

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterHandleRequest {
    pub local_alias: Option<String>,
    pub requested_handle: crate::ids::Handle,
    pub verification: VerificationInput,
    pub profile: InitialProfile,
    pub make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationInput {
    Otp { code: String },
    AlreadyVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRegistrationResult {
    pub identity: IdentitySummary,
    pub default_identity_change: Option<DefaultIdentityChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultIdentityChange {
    pub previous: Option<IdentitySummary>,
    pub next: IdentitySummary,
    pub requires_default_identity_write: bool,
    pub warnings: Vec<String>,
}
```

`DefaultIdentityChange` 不返回本地路径。CLI 如果需要展示 default identity 文件路径，应在 `awiki-cli::im_core_adapter` 中根据 `ImCorePaths` / CLI resolved paths 渲染。这样可以保持 SDK public DTO 不泄漏本地路径细节。

CLI 仍负责：

```text
OTP 输入方式
local_alias 命名规则
身份文件权限
default identity 文件是否实际写入
default identity 文件路径展示
输出提示
```

SDK 负责：

```text
注册业务流程
服务端返回 normalize
身份摘要生成
注册结果中的默认身份变更计划
```

## 5. Auth Service

`auth/dto.rs` / `auth/service.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBundle {
    pub subject: crate::ids::Did,
    pub scope: AuthScope,
    pub expires_at: Option<String>,
    pub refreshed: bool,
    pub bearer_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUpdate {
    pub subject: crate::ids::Did,
    pub previous_expires_at: Option<String>,
    pub new_expires_at: Option<String>,
    pub refreshed: bool,
    pub bearer_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub subject: crate::ids::Did,
    pub has_session: bool,
    pub expires_at: Option<String>,
    pub needs_refresh: bool,
    pub warnings: Vec<String>,
}

pub struct AuthService<'a> {
    pub(crate) client: &'a crate::core::ImClient,
}

impl AuthService<'_> {
    pub fn login(&self) -> crate::ImResult<SessionBundle>;

    pub fn ensure_session(&self, scope: AuthScope) -> crate::ImResult<SessionBundle>;

    pub fn refresh_session(&self) -> crate::ImResult<SessionUpdate>;

    pub fn status(&self) -> crate::ImResult<AuthStatus>;
}
```

P1 不暴露：

```rust
pub fn bearer_token(&self) -> &str
pub fn auth_state_path(&self) -> &Path
pub fn did_auth_payload(...)
pub fn save_jwt(...)
```

When the identity runtime is vault-backed, auth/session refresh stores updated
token state through SecretVault instead of rewriting plaintext auth files.
`SessionBundle` and `SessionUpdate` may still return `bearer_token` to the
calling SDK host for existing auth flows; that token is sensitive and must not
be logged, persisted by callers, or copied into diagnostics. Identity vault
status/migration/verification DTOs do not expose bearer tokens or vault refs.

## 6. Internal Runtime

`internal/identity_runtime.rs`：

```rust
pub(crate) struct ClientIdentityRuntime {
    pub(crate) summary: crate::identity::IdentitySummary,
    pub(crate) did_document_path: std::path::PathBuf,
    pub(crate) private_key_path: std::path::PathBuf,
    pub(crate) auth_state_path: std::path::PathBuf,
    pub(crate) owner: LocalOwnerContext,
}

pub(crate) struct LocalOwnerContext {
    pub(crate) identity_id: crate::ids::IdentityId,
    pub(crate) current_did: crate::ids::Did,
}
```

`owner_identity_id` 是长期 owner key；`current_did` 用于兼容现有 DID owner 字段和远端请求。

`private_key_path` and `auth_state_path` are compatibility/location fields, not a
business-read contract. DID-WBA proof generation, business signing, secure
direct static key loading, daemon subkey package persistence, and auth/JWT
refresh must use the runtime `KeyMaterialProvider`. When verified vault metadata
and the host workspace/device context match, the provider is vault-backed.

## 7. Auth Retry Contract

消息发送内部调用顺序：

```text
1. client.auth().ensure_session(AuthScope::Messaging)
2. send HTTP/RPC request
3. 如果服务端返回 session expired / 401-like error：
   a. client.auth().refresh_session()
   b. retry once
4. 仍失败则返回 ImError::Service 或 ImError::SessionExpired
```

这个流程在 SDK 内部完成，CLI/App 不实现 401 retry。
