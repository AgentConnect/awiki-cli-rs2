//! Vault is authoritative; SQLite is a reconstructible non-secret index.
use super::wire;
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::{
    SealSecretRequest, SecretKind, SecretMetadata, SecretRef, SecretVault,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    pub id: String,
    #[serde(default)]
    pub resume_operation_id: Option<String>,
    pub tenant_id: String,
    pub config: wire::ConfigurationPayload,
    pub config_hash: String,
    pub dpop_private: String,
    pub remote_challenge: Option<wire::PhoneChallengeResponse>,
    pub discovery: Option<wire::PhoneDiscoveryResponse>,
    pub pst: Option<wire::PhoneSessionResponse>,
    pub targets: Vec<wire::Target>,
    pub selected: Option<wire::Target>,
    pub nonces: std::collections::BTreeMap<String, String>,
    pub exact_requests: std::collections::BTreeMap<String, ExactRequest>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.dpop_private.zeroize();
        if let Some(s) = self.discovery.as_mut() {
            s.discovery_token.zeroize();
        }
        if let Some(s) = self.pst.as_mut() {
            s.access_token.zeroize();
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExactRequest {
    pub method: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    pub url: String,
    pub body_b64u: String,
    pub idempotency_key: String,
}
impl Drop for ExactRequest {
    fn drop(&mut self) {
        self.body_b64u.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Operation {
    pub id: String,
    pub session_id: String,
    pub kind: crate::pin_recovery::PinRecoveryKind,
    pub target: wire::Target,
    #[serde(default)]
    pub account_user_id: Option<String>,
    pub realm_ids: Vec<String>,
    pub identity: Option<crate::internal::identity_provider::ProviderIdentityRef>,
    pub identity_generation: Option<u64>,
    pub device_label: String,
    pub phase: crate::pin_recovery::PinRecoveryPhase,
    pub remote: Option<wire::OperationResponse>,
    #[serde(default)]
    pub last_status_query_started_at: Option<u64>,
    pub envelope: Option<wire::BackupResponse>,
    pub wrapping_key_b64u: Option<String>,
    pub recovered_root_b64u: Option<String>,
    pub attempt: Option<wire::AttemptResponse>,
    pub attempt_running: bool,
    pub registration_payloads: std::collections::BTreeMap<String, ExactRequest>,
    pub receipts: Vec<String>,
    #[serde(default)]
    pub custody_ingress_started: bool,
    pub custody_pending: Option<serde_json::Value>,
    pub device_challenge: Option<wire::DeviceChallengeResponse>,
    pub commit_body: Option<serde_json::Value>,
    pub commit_attempted: bool,
    pub recovered_identity: Option<crate::identity::IdentitySelector>,
    pub secure_readiness: crate::pin_recovery::PinRecoverySecureReadiness,
    pub last_error: Option<crate::pin_recovery::PinRecoveryError>,
    #[serde(default)]
    pub v4_cleanup: Option<super::v4_cleanup::Claim>,
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Some(k) = self.wrapping_key_b64u.as_mut() {
            k.zeroize();
        }
        if let Some(k) = self.recovered_root_b64u.as_mut() {
            k.zeroize();
        }
        if let Some(attempt) = self.attempt.as_mut() {
            for token in &mut attempt.realm_tokens {
                token.access_token.zeroize();
            }
        }
    }
}
#[derive(Clone)]
pub(crate) struct Store {
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
    workspace: String,
    device: String,
    index_path: std::path::PathBuf,
    lock_path: std::path::PathBuf,
    trusted_tenants: Vec<crate::pin_recovery::PinRecoveryTenantConfig>,
}
impl Store {
    pub fn new(core: &crate::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(super::failure("vault_required", false));
        }
        let context = core
            .inner()
            .identity_vault()
            .ok_or_else(|| super::failure("vault_required", false))?;
        let state = &core.inner().sdk_paths().local_state.sqlite_path;
        Ok(Self {
            trusted_tenants: core.inner().pin_recovery_tenants.clone(),
            vault: context.vault(),
            workspace: context.workspace_id().into(),
            device: context.vault_context_device_id().as_str().into(),
            index_path: state.with_extension("pin-recovery.sqlite"),
            lock_path: state.with_extension("pin-recovery.lock"),
        })
    }
    pub fn from_vault_for_cleanup(
        vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
        workspace: String,
        device: String,
        sqlite_path: &std::path::Path,
    ) -> Self {
        Self {
            vault,
            workspace,
            device,
            index_path: sqlite_path.with_extension("pin-recovery.sqlite"),
            lock_path: sqlite_path.with_extension("pin-recovery.lock"),
            trusted_tenants: Vec::new(),
        }
    }
    pub fn lock_sync(&self) -> crate::ImResult<std::fs::File> {
        if let Some(parent) = self.lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&self.lock_path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(file)
    }
    pub async fn lock(&self) -> crate::ImResult<std::fs::File> {
        let path = self.lock_path.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(path)?;
            fs2::FileExt::lock_exclusive(&file)?;
            Ok(file)
        })
        .await
        .map_err(|_| super::failure("local_state_unavailable", true))?
    }
    fn reference(&self, prefix: &str, id: &str) -> crate::ImResult<SecretRef> {
        uuid::Uuid::parse_str(id).map_err(|_| super::failure("invalid_local_handle", false))?;
        Ok(SecretRef {
            workspace_id: self.workspace.clone(),
            device_id: self.device.clone(),
            identity_id: None,
            did: None,
            kind: SecretKind::IdentityPinRecoveryPending,
            key_id: format!("pin-recovery/{prefix}/{id}"),
            key_version: 1,
        })
    }
    fn save<T: Serialize>(&self, prefix: &str, id: &str, value: &T) -> crate::ImResult<()> {
        let r = self.reference(prefix, id)?;
        let raw = serde_json::to_vec(value)
            .map_err(|_| super::failure("local_state_unavailable", true))?;
        self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: r.workspace_id,
                device_id: r.device_id,
                identity_id: None,
                did: None,
                kind: r.kind,
                key_id: r.key_id,
                key_version: 1,
                policy: crate::internal::secret_vault::SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(raw),
        })?;
        Ok(())
    }
    fn load<T: serde::de::DeserializeOwned>(&self, prefix: &str, id: &str) -> crate::ImResult<T> {
        let r = self.reference(prefix, id)?;
        let raw = self
            .vault
            .open(&r)
            .map_err(|_| super::failure("invalid_local_handle", false))?;
        serde_json::from_slice(raw.expose_secret())
            .map_err(|_| super::failure("corrupt_pending", false))
    }
    pub fn list_operations(&self) -> crate::ImResult<Vec<Operation>> {
        let prefix = "pin-recovery/operation/";
        let refs = self.vault.list()?;
        let mut operations = Vec::new();
        for r in refs.into_iter().filter(|r| {
            r.workspace_id == self.workspace
                && r.device_id == self.device
                && r.kind == SecretKind::IdentityPinRecoveryPending
                && r.key_id.starts_with(prefix)
                && r.key_version == 1
        }) {
            let id = r
                .key_id
                .strip_prefix(prefix)
                .ok_or_else(|| super::failure("corrupt_pending", false))?;
            operations.push(self.operation(id)?);
        }
        Ok(operations)
    }
    pub fn list_sessions_for_cleanup(&self) -> crate::ImResult<Vec<Session>> {
        let prefix = "pin-recovery/session/";
        self.vault
            .list()?
            .into_iter()
            .filter(|r| {
                r.workspace_id == self.workspace
                    && r.device_id == self.device
                    && r.kind == SecretKind::IdentityPinRecoveryPending
                    && r.key_version == 1
                    && r.key_id.starts_with(prefix)
            })
            .map(|r| {
                self.historical_session_for_cleanup(
                    r.key_id
                        .strip_prefix(prefix)
                        .ok_or_else(|| super::failure("corrupt_pending", false))?,
                )
            })
            .collect()
    }
    pub fn delete_session_for_cleanup(&self, id: &str) -> crate::ImResult<()> {
        self.vault.delete(&self.reference("session", id)?)
    }
    pub fn historical_session_for_cleanup(&self, id: &str) -> crate::ImResult<Session> {
        let s: Session = self.load("session", id)?;
        if s.id != id || s.tenant_id != s.config.tenant_id {
            return Err(super::failure("corrupt_pending", false));
        }
        Ok(s)
    }
    pub fn save_session(&self, session: &Session) -> crate::ImResult<()> {
        self.save("session", &session.id, session)
    }
    pub fn session(&self, id: &str) -> crate::ImResult<Session> {
        let s: Session = self.load("session", id)?;
        if s.id != id {
            return Err(super::failure("corrupt_pending", false));
        }
        if !self.trusted_tenants.iter().any(|t| {
            t.tenant_id == s.tenant_id
                && t.deployment_id == s.config.deployment_id
                && t.control_base_url == s.config.control_base_url
        }) {
            return Err(super::failure("capability_unavailable", false));
        }
        Ok(s)
    }
    pub fn operation(&self, id: &str) -> crate::ImResult<Operation> {
        let o: Operation = self.load("operation", id)?;
        if o.id != id {
            return Err(super::failure("corrupt_pending", false));
        }
        self.index(&o)?;
        Ok(o)
    }
    pub fn save_operation(&self, operation: &Operation) -> crate::ImResult<()> {
        self.save("operation", &operation.id, operation)?;
        self.index(operation)
    }
    #[cfg(feature = "sqlite")]
    fn index(&self, operation: &Operation) -> crate::ImResult<()> {
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = rusqlite::Connection::open(&self.index_path)
            .map_err(|_| super::failure("local_state_unavailable", true))?;
        db.execute_batch("PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS pin_recovery_operations_v1(operation_id TEXT PRIMARY KEY, target_id TEXT NOT NULL, did TEXT NOT NULL, phase TEXT NOT NULL);").map_err(|_| super::failure("local_state_unavailable",true))?;
        let phase = serde_json::to_string(&operation.phase)
            .map_err(|_| super::failure("corrupt_pending", false))?;
        db.execute("INSERT INTO pin_recovery_operations_v1(operation_id,target_id,did,phase) VALUES(?1,?2,?3,?4) ON CONFLICT(operation_id) DO UPDATE SET phase=excluded.phase",rusqlite::params![operation.id,operation.target.target_id,operation.target.did,phase]).map_err(|_| super::failure("local_state_unavailable",true))?;
        Ok(())
    }
    #[cfg(not(feature = "sqlite"))]
    fn index(&self, _: &Operation) -> crate::ImResult<()> {
        Err(super::failure("sqlite_required", false))
    }
}
