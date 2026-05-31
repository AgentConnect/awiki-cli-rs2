use super::upgrader::{Context, MigrationError};
use crate::workspace_upgrade::legacy_identity as identity;

pub(crate) fn apply_workspace_v2_to_v3_replace_existing_k1_dids(
    context: &mut Context,
) -> Result<(), MigrationError> {
    let manager = identity::Manager::new(context.resolved.paths.clone());
    let identities = match manager.list() {
        Ok(identities) => identities,
        Err(err) => {
            context.warnings.push(format!(
                "Automatic existing k1 to e1 DID replacement was skipped: {err}"
            ));
            return Ok(());
        }
    };
    replace_k1_dids_for_summaries(context, identities)
}

pub(crate) fn replace_k1_dids_for_summaries(
    context: &mut Context,
    identities: Vec<identity::IdentitySummary>,
) -> Result<(), MigrationError> {
    if identities.is_empty() {
        return Ok(());
    }
    if let Err(err) = crate::cli_http::new_http_client(&context.resolved.ca_bundle) {
        context.warnings.push(format!(
            "Automatic k1 to e1 DID replacement was skipped: {err}"
        ));
        return Ok(());
    }
    let manager = identity::Manager::new(context.resolved.paths.clone());
    for summary in identities {
        if !is_k1_did(&summary.did) {
            continue;
        }
        let record = match manager.load(&summary.identity_name) {
            Ok(record) => record,
            Err(err) => {
                context.warnings.push(format!(
                    "Automatic DID replacement skipped for identity {} ({}): {}",
                    summary.identity_name, summary.did, err
                ));
                continue;
            }
        };
        if !is_k1_did(&record.did) {
            continue;
        }
        if let Err(err) = validate_handle_did(&record.did) {
            context.warnings.push(format!(
                "Automatic DID replacement skipped for identity {} ({}): {}",
                summary.identity_name, record.did, err
            ));
            continue;
        }
        let mut result = match identity::replace_did(
            &context.resolved,
            &manager,
            identity::ReplaceDidParams {
                identity_name: summary.identity_name.clone(),
                ..identity::ReplaceDidParams::default()
            },
        ) {
            Ok(result) => result,
            Err(err) => {
                context.warnings.push(format!(
                    "Automatic DID replacement failed for identity {} ({}): {}",
                    summary.identity_name, record.did, err
                ));
                continue;
            }
        };
        for warning in result.warnings.drain(..) {
            context.warnings.push(format!(
                "Automatic DID replacement completed with warning for identity {}: {}",
                summary.identity_name, warning
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_workspace_v2_to_v3_replace_existing_k1_dids(
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

fn validate_handle_did(did: &str) -> Result<(), String> {
    let trimmed = did.trim();
    if !trimmed.starts_with("did:wba:") {
        return Err(format!("invalid input: invalid did {did:?}"));
    }
    let parts = trimmed.split(':').collect::<Vec<_>>();
    if parts.len() < 5 {
        return Err(format!("invalid input: invalid did {did:?}"));
    }
    if path_unescape(parts[2]).is_none() {
        return Err(format!("invalid input: invalid did domain {:?}", parts[2]));
    }
    let Some(first_path_segment) = parts.get(3) else {
        return Err("invalid input: missing did path segments".to_string());
    };
    if first_path_segment.eq_ignore_ascii_case("user") {
        return Err("invalid input: current did is not a handle did".to_string());
    }
    Ok(())
}

fn path_unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let hi = hex_value(bytes[index + 1])?;
        let lo = hex_value(bytes[index + 2])?;
        output.push((hi << 4) | lo);
        index += 3;
    }
    String::from_utf8(output).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
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

    #[test]
    fn validate_handle_did_matches_go_replace_did_preflight() {
        assert!(validate_handle_did("did:wba:example.test:alice:k1_legacy").is_ok());
        assert_eq!(
            validate_handle_did("did:wba:example.test:user:k1_legacy").unwrap_err(),
            "invalid input: current did is not a handle did"
        );
        assert_eq!(
            validate_handle_did("bad").unwrap_err(),
            "invalid input: invalid did \"bad\""
        );
        assert_eq!(
            validate_handle_did("did:wba:%zz:alice:k1_legacy").unwrap_err(),
            "invalid input: invalid did domain \"%zz\""
        );
    }
}
