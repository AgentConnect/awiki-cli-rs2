use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct ClientIdentityRuntime {
    pub(crate) summary: crate::identity::IdentitySummary,
    pub(crate) did_document_path: PathBuf,
    pub(crate) private_key_path: PathBuf,
    pub(crate) e2ee_agreement_private_key_path: PathBuf,
    pub(crate) auth_state_path: PathBuf,
    pub(crate) key_provider: Arc<dyn crate::internal::key_provider::KeyMaterialProvider>,
    pub(crate) owner: LocalOwnerContext,
}

pub(crate) struct LocalOwnerContext {
    pub(crate) identity_id: crate::ids::IdentityId,
    pub(crate) current_did: crate::ids::Did,
    pub(crate) sync_account: Option<SyncAccountSeed>,
}

pub(crate) struct SyncAccountSeed {
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) identity_generation: std::sync::OnceLock<String>,
    pub(crate) device_auth_generation: String,
    pub(crate) device_signing_key_id: String,
    pub(crate) device_e2ee_key_id: String,
    pub(crate) role: crate::internal::identity_device_state::DeviceAuthorizationRole,
    pub(crate) management_ready: bool,
}

impl SyncAccountSeed {
    pub(crate) fn new(
        account_id: String,
        protocol_device_id: crate::ids::ProtocolDeviceId,
        identity_generation: Option<String>,
        device_auth_generation: String,
        device_signing_key_id: String,
        device_e2ee_key_id: String,
        role: crate::internal::identity_device_state::DeviceAuthorizationRole,
        management_ready: bool,
    ) -> Self {
        let generation = std::sync::OnceLock::new();
        if let Some(identity_generation) = identity_generation {
            let _ = generation.set(identity_generation);
        }
        Self {
            account_id,
            protocol_device_id,
            identity_generation: generation,
            device_auth_generation,
            device_signing_key_id,
            device_e2ee_key_id,
            role,
            management_ready,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncAccountContext {
    pub(crate) account_id: String,
    pub(crate) protocol_device_id: String,
    pub(crate) device_auth_generation: String,
}
