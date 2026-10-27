use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityObservation {
    pub did: String,
    pub document_version: String,
    pub document_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityObservations {
    pub schema_version: u8,
    pub complete: bool,
    pub identities: Vec<IdentityObservation>,
}

fn invalid() -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "invalid direct identity observations".into(),
    }
}

pub(crate) fn parse(value: Option<&Value>) -> crate::ImResult<Option<IdentityObservations>> {
    let Some(value) = value else { return Ok(None) };
    let schema = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    if schema != 1 {
        return Ok(None);
    }
    let observations: IdentityObservations =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if observations.identities.len() > 1024 {
        return Err(invalid());
    }
    let mut dids = BTreeSet::new();
    for entry in &observations.identities {
        crate::ids::Did::parse(&entry.did)?;
        if entry.did.trim() != entry.did || !dids.insert(&entry.did) {
            return Err(invalid());
        }
        crate::internal::local_state::sync_v2::validate_decimal(
            "document_version",
            &entry.document_version,
        )?;
        let hash = entry
            .document_hash
            .strip_prefix("sha256:")
            .ok_or_else(invalid)?;
        if entry.document_version == "0"
            || hash.len() != 43
            || !hash
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(invalid());
        }
    }
    Ok(Some(observations))
}

#[cfg(test)]
#[path = "direct_e2ee_state_tests.rs"]
mod tests;
