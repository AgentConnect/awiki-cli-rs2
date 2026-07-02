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
}
