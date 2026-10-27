//! Optional Home-local dependency observations. These are invalidation inputs,
//! not P6 messages, authorization leases, or replacements for lane cursors.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupStateObservation {
    pub group_did: String,
    pub host_service_did: String,
    pub group_state_version: String,
    pub epoch: Option<String>,
    pub revision: String,
    pub local_identity_coverage: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupStateObservations {
    pub schema_version: u8,
    pub complete: bool,
    pub groups: Vec<GroupStateObservation>,
}

fn invalid() -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "invalid Home group dependency observations".to_owned(),
    }
}

pub(crate) fn parse(value: Option<&Value>) -> crate::ImResult<Option<GroupStateObservations>> {
    let Some(value) = value else { return Ok(None) };
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    if version != 1 {
        return Ok(None);
    }
    let observations: GroupStateObservations =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if observations.groups.len() > 1024 {
        return Err(invalid());
    }
    let mut groups = BTreeSet::new();
    for group in &observations.groups {
        for did in [&group.group_did, &group.host_service_did] {
            if did.trim() != did || !did.starts_with("did:") {
                return Err(invalid());
            }
            crate::ids::Did::parse(did)?;
        }
        crate::internal::local_state::sync_v2::validate_decimal(
            "group_state_version",
            &group.group_state_version,
        )?;
        if let Some(epoch) = &group.epoch {
            crate::internal::local_state::sync_v2::validate_decimal("epoch", epoch)?;
        }
        let digest = group.revision.strip_prefix("sha256:").ok_or_else(invalid)?;
        if digest.len() != 43
            || !digest
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || !groups.insert(&group.group_did)
        {
            return Err(invalid());
        }
    }
    Ok(Some(observations))
}

#[cfg(test)]
#[path = "group_e2ee_state_tests.rs"]
mod tests;
