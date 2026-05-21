use std::path::PathBuf;

pub(crate) struct ClientIdentityRuntime {
    pub(crate) summary: crate::identity::IdentitySummary,
    pub(crate) did_document_path: PathBuf,
    pub(crate) private_key_path: PathBuf,
    pub(crate) auth_state_path: PathBuf,
    pub(crate) owner: LocalOwnerContext,
}

pub(crate) struct LocalOwnerContext {
    pub(crate) identity_id: crate::ids::IdentityId,
    pub(crate) current_did: crate::ids::Did,
}
