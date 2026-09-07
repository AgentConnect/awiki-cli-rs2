//! Read-only metadata for host workspace migrations, outside the stable SDK API.
//!
//! Parsing and schema/custody validation stay with the Core identity store. This
//! projection never loads secret files, opens a vault, or rewrites the registry.

use crate::internal::identity_store::IdentityStore;
use crate::paths::IdentityRegistryPaths;

#[derive(Debug)]
pub struct WorkspaceIdentityIndex {
    pub schema_version: i64,
    pub identities: Vec<WorkspaceIdentityOwner>,
}

#[derive(Debug)]
pub struct WorkspaceIdentityOwner {
    pub owner_identity_id: String,
    pub did: String,
}

pub fn read_workspace_identity_index(
    paths: &IdentityRegistryPaths,
) -> crate::ImResult<WorkspaceIdentityIndex> {
    let index = IdentityStore::new(paths).load_index()?;
    Ok(WorkspaceIdentityIndex {
        schema_version: index.schema_version,
        identities: index
            .credentials
            .into_values()
            .map(|entry| WorkspaceIdentityOwner {
                owner_identity_id: entry.unique_id,
                did: entry.did,
            })
            .collect(),
    })
}
