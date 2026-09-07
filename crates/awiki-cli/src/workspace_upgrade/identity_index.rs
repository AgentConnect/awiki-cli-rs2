//! Host path adapter for Core-owned, read-only identity migration metadata.

pub(super) fn read(
    paths: &crate::workspace_config::Paths,
) -> im_core::ImResult<im_core::compat::identity_index::WorkspaceIdentityIndex> {
    let root = std::path::PathBuf::from(&paths.identity_dir);
    im_core::compat::identity_index::read_workspace_identity_index(
        &im_core::paths::IdentityRegistryPaths {
            registry_path: root.join("index.json"),
            identity_root_dir: root,
            default_identity_path: None,
        },
    )
}
