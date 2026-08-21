use im_core::{IdentitySelector, ImClient, ImCore};

use crate::cli_output::ExitError;

pub fn build_im_core(resolved: &crate::workspace_config::Resolved) -> Result<ImCore, ExitError> {
    let config = super::core_config::build_im_core_config(resolved)?;
    let paths = super::paths::build_im_core_paths(resolved)?;
    let options = super::vault::build_im_core_open_options(resolved)?;
    let core = ImCore::new_with_options(config, paths, options)
        .map_err(|err| super::error::map_im_error(err, "build im-core"))?;
    core.identities()
        .migrate_identity_custody()
        .map_err(|err| super::error::map_im_error(err, "migrate identity custody"))?;
    Ok(core)
}

pub async fn build_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<ImCore, ExitError> {
    let config = super::core_config::build_im_core_config(resolved)?;
    let paths = super::paths::build_im_core_paths(resolved)?;
    let options = super::vault::build_im_core_open_options(resolved)?;
    let core = ImCore::open_with_options(config, paths, options)
        .await
        .map_err(|err| super::error::map_im_error(err, "build im-core"))?;
    core.identities()
        .migrate_identity_custody_async()
        .await
        .map_err(|err| super::error::map_im_error(err, "migrate identity custody"))?;
    Ok(core)
}

pub fn build_im_client(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<ImClient, ExitError> {
    let core = build_im_core(resolved)?;
    core.client(selector)
        .map_err(|err| super::error::map_im_error(err, "build im-client"))
}

pub async fn build_im_client_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<ImClient, ExitError> {
    let core = build_im_core_async(resolved).await?;
    core.client_async(selector)
        .await
        .map_err(|err| super::error::map_im_error(err, "build im-client"))
}
