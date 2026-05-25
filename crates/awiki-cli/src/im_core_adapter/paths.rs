use std::path::PathBuf;

use im_core::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};

use crate::output::ExitError;

pub fn build_im_core_paths(resolved: &crate::config::Resolved) -> Result<ImCorePaths, ExitError> {
    build_im_core_paths_from_parts(
        &resolved.paths.identity_dir,
        &resolved.paths.database_file,
        &resolved.paths.cache_dir,
        &resolved.paths.state_dir,
    )
}

pub(crate) fn build_im_core_paths_from_parts(
    identity_root_dir: &str,
    sqlite_path: &str,
    cache_dir: &str,
    runtime_dir: &str,
) -> Result<ImCorePaths, ExitError> {
    let identity_root_dir = required_path("identity_root_dir", identity_root_dir)?;
    let sqlite_path = required_path("sqlite_path", sqlite_path)?;
    let cache_dir = required_path("cache_dir", cache_dir)?;
    let runtime_dir = required_path("runtime_dir", runtime_dir)?;
    Ok(ImCorePaths {
        identities: IdentityRegistryPaths {
            registry_path: identity_root_dir.join(crate::identity::types::INDEX_FILE_NAME),
            default_identity_path: Some(identity_root_dir.join("default")),
            identity_root_dir,
        },
        local_state: LocalStatePaths { sqlite_path },
        runtime: RuntimePaths {
            cache_dir,
            temp_dir: runtime_dir.join("tmp"),
        },
    })
}

fn required_path(kind: &'static str, raw: &str) -> Result<PathBuf, ExitError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ExitError::new(
            "invalid_config",
            2,
            format!("{kind} is required to build ImCorePaths."),
            "Resolve awiki-cli workspace paths before building ImCore.",
        ));
    }
    Ok(PathBuf::from(trimmed))
}
