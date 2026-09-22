use serde::{Deserialize, Serialize};

use super::DidMethod;

/// Method support for product entrypoints, never evidence of device authority.
/// Every write still checks the current account, device and custody state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityMethodCapabilities {
    pub method: DidMethod,
    pub handle_recovery: bool,
    pub root_import: bool,
    pub root_transfer: bool,
    pub services_update: bool,
}

pub fn identity_method_capabilities(did: &str) -> crate::ImResult<IdentityMethodCapabilities> {
    let invalid = || crate::ImError::invalid_input(Some("did".into()), "Unsupported identity DID");
    if did.chars().any(|ch| ch.is_whitespace() || ch.is_control()) || did.contains(['#', '?']) {
        return Err(invalid());
    }
    let method = if did.starts_with("did:web:") {
        anp::authentication::did_resolver::build_did_web_resolution_url(did)
            .map_err(|_| invalid())?;
        DidMethod::Web
    } else if did.strip_prefix("did:wba:").is_some_and(|value| {
        !value.is_empty() && value.split(':').all(|segment| !segment.is_empty())
    }) {
        DidMethod::Wba
    } else {
        return Err(invalid());
    };
    Ok(IdentityMethodCapabilities {
        method,
        handle_recovery: method == DidMethod::Wba,
        root_import: method == DidMethod::Wba,
        root_transfer: method == DidMethod::Wba,
        services_update: method == DidMethod::Web,
    })
}

#[cfg(test)]
#[path = "method_capabilities_tests.rs"]
mod tests;
