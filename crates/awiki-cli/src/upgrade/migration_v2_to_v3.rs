use super::upgrader::{Context, MigrationError};
use crate::identity::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExistingK1Boundary {
    NoK1DID,
    HasK1DID,
}

impl ExistingK1Boundary {
    pub(crate) fn has_k1_did(self) -> bool {
        matches!(self, Self::HasK1DID)
    }
}

pub(crate) fn apply_workspace_v2_to_v3_replace_existing_k1_boundary(
    context: &mut Context,
) -> Result<ExistingK1Boundary, MigrationError> {
    let manager = Manager::new(context.resolved.paths.clone());
    let identities = match manager.list() {
        Ok(identities) => identities,
        Err(err) => {
            context.warnings.push(format!(
                "Automatic existing k1 to e1 DID replacement was skipped: {err}"
            ));
            return Ok(ExistingK1Boundary::NoK1DID);
        }
    };
    if identities.iter().any(|summary| is_k1_did(&summary.did)) {
        return Ok(ExistingK1Boundary::HasK1DID);
    }
    Ok(ExistingK1Boundary::NoK1DID)
}

pub(crate) fn validate_workspace_v2_to_v3_replace_existing_k1_boundary(
    _context: &Context,
) -> Result<(), MigrationError> {
    Ok(())
}

pub(crate) fn is_k1_did(did: &str) -> bool {
    did.rsplit(':')
        .next()
        .unwrap_or(did)
        .trim()
        .starts_with("k1_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_k1_did_matches_go_suffix_check() {
        assert!(is_k1_did("did:wba:example.test:user:k1_legacy"));
        assert!(is_k1_did(" k1_direct "));
        assert!(!is_k1_did("did:wba:example.test:user:e1_current"));
        assert!(!is_k1_did("did:wba:example.test:user:xk1_legacy"));
    }
}
