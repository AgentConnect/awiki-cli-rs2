use im_core::{IdentitySelector, ImClient, ImCore};

use crate::cli_output::ExitError;

pub fn build_im_core(resolved: &crate::workspace_config::Resolved) -> Result<ImCore, ExitError> {
    let config = super::core_config::build_im_core_config(resolved)?;
    let paths = super::paths::build_im_core_paths(resolved)?;
    ImCore::new(config, paths).map_err(|err| super::error::map_im_error(err, "build im-core"))
}

pub fn build_im_client(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<ImClient, ExitError> {
    let core = build_im_core(resolved)?;
    core.client(selector)
        .map_err(|err| super::error::map_im_error(err, "build im-client"))
}
