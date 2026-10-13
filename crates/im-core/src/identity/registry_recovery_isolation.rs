//! Read-only isolation of exact, committed Recovery projection conflicts.
use super::*;
use crate::internal::{identity_handle_recovery_operation as operations, identity_store};

pub(super) fn conflict() -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("identity.local_registry_conflict".to_owned()),
        message: "this Handle has an unfinished local identity transition".to_owned(),
        data: None,
    }
}

pub(super) fn parse(core: &crate::ImCore, raw: &[u8]) -> crate::ImResult<RegistrySnapshot> {
    match parse_registry(raw) {
        Ok(snapshot) => return Ok(snapshot),
        Err(crate::ImError::Service {
            code: Some(code), ..
        }) if code == "identity.local_registry_conflict" => {}
        Err(error) => return Err(error),
    }
    let index = identity_store::parse_index_payload(raw)?;
    let mut handles = BTreeMap::<&str, usize>::new();
    let mut ids = BTreeSet::new();
    let mut dids = BTreeSet::new();
    for entry in index.credentials.values() {
        validate_unique_registry_value(&mut ids, "identity_id", &entry.unique_id)?;
        validate_unique_registry_value(&mut dids, "live DID", &entry.did)?;
        if !entry.full_handle.is_empty() {
            *handles.entry(&entry.full_handle).or_default() += 1;
        }
    }
    let mut blocked = BTreeSet::new();
    for (handle, count) in handles {
        if count < 2 {
            continue;
        }
        let records =
            operations::list_handle(&core.inner().sdk_paths().local_state.sqlite_path, handle)?;
        let active = records
            .iter()
            .filter(|record| {
                crate::internal::identity_handle_recovery_context::is_actionable(
                    record.lifecycle_class,
                )
            })
            .collect::<Vec<_>>();
        let [record] = active.as_slice() else {
            return Err(conflict());
        };
        let marker = crate::internal::identity_transition_pending::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &record.operation_id,
        )?
        .ok_or_else(conflict)?;
        if marker.owner_identity_id != record.owner_identity_id || marker.handle != handle {
            return Err(conflict());
        }
        let pair =
            identity_store::recovery_duplicate_projection(&index, &marker).ok_or_else(conflict)?;
        blocked.insert(pair.predecessor_alias);
        blocked.insert(pair.successor_alias);
    }
    if blocked.is_empty() {
        return Err(conflict());
    }
    let mut available = index.clone();
    available
        .credentials
        .retain(|alias, _| !blocked.contains(alias));
    if blocked.contains(&available.default_credential_name) {
        available.default_credential_name.clear();
    }
    let encode = |value: &identity_store::IndexPayload| {
        serde_json::to_vec(value).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
    };
    let mut snapshot = parse_registry(&encode(&available)?)?;
    for alias in blocked {
        let mut one = index.clone();
        one.default_credential_name.clear();
        one.credentials.retain(|key, _| key == &alias);
        snapshot
            .blocked_entries
            .extend(parse_registry(&encode(&one)?)?.entries);
    }
    Ok(snapshot)
}
