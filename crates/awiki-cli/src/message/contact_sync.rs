use super::types::MessageError;
use crate::config::Resolved;
use crate::identity::wire::build_handle_lookup_by_did_rpc_call;
use crate::store::{self, ContactRecord};
use serde_json::Value;

pub(crate) fn sync_peer_handle(
    resolved: &Resolved,
    connection: &mut rusqlite::Connection,
    owner_did: &str,
    peer_did: &str,
    known_handle: &str,
    source_type: &str,
    source_group_id: &str,
) -> Result<String, MessageError> {
    let owner_did = owner_did.trim();
    let peer_did = peer_did.trim();
    if owner_did.is_empty() || peer_did.is_empty() || owner_did == peer_did {
        return Ok(String::new());
    }
    let resolved_handle = store::resolve_contact_handle_by_did(connection, owner_did, peer_did)
        .map_err(|err| MessageError::Internal(format!("contact handle lookup failed: {err}")))?;
    let mut handle = normalize_handle_value(known_handle);
    if handle.is_empty() {
        handle = resolved_handle.clone();
    }
    if handle.is_empty() {
        let mut phase = crate::traceutil::handle_lookup_phase("contact_sync_by_did");
        let lookup = lookup_handle_by_did(resolved, peer_did);
        phase.finish();
        if let Some(lookup) = lookup? {
            handle = normalize_handle_value(
                lookup
                    .get("handle")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
        }
    }
    if handle.is_empty() {
        return Ok(resolved_handle);
    }
    let messaged = true;
    let now = store::now_utc();
    store::upsert_contact(
        connection,
        ContactRecord {
            owner_did: owner_did.to_string(),
            did: peer_did.to_string(),
            handle: handle.clone(),
            source_type: source_type.to_string(),
            source_group_id: source_group_id.to_string(),
            messaged: Some(messaged),
            first_seen_at: now.clone(),
            last_seen_at: now,
            ..ContactRecord::default()
        },
    )
    .map_err(|err| MessageError::Internal(format!("contact sync failed: {err}")))?;
    Ok(handle)
}

pub(crate) fn sync_direct_peer_handles(
    resolved: &Resolved,
    connection: &mut rusqlite::Connection,
    record_owner_did: &str,
    messages: &[Value],
    known_handle: &str,
    source_type: &str,
) -> Vec<String> {
    let mut seen = Vec::new();
    let mut warnings = Vec::new();
    for message in messages {
        let sender_did = message
            .get("sender_did")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let receiver_did = message
            .get("receiver_did")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut peer_did = sender_did;
        if peer_did == record_owner_did {
            peer_did = receiver_did;
        }
        let peer_did = peer_did.trim();
        if peer_did.is_empty() || peer_did == record_owner_did {
            continue;
        }
        if seen.iter().any(|did| did == peer_did) {
            continue;
        }
        seen.push(peer_did.to_string());
        if let Err(err) = sync_peer_handle(
            resolved,
            connection,
            record_owner_did,
            peer_did,
            known_handle,
            source_type,
            "",
        ) {
            warnings.push(format!(
                "Failed to sync contact handle for {peer_did}: {err}"
            ));
        }
    }
    warnings
}

pub(crate) fn peer_dids_for_handle_from_store(
    resolved: &Resolved,
    owner_did: &str,
    handle: &str,
    current_did: &str,
) -> Result<Vec<String>, MessageError> {
    let handle = normalize_handle_value(handle);
    if handle.is_empty() {
        return Ok(merge_peer_dids(current_did, &[]));
    }
    let connection = store::open(&resolved.paths)
        .map_err(|err| MessageError::Internal(format!("open local message store: {err}")))?;
    store::ensure_schema(&connection).map_err(|err| {
        MessageError::Internal(format!("ensure local message store schema: {err}"))
    })?;
    let dids = store::list_dids_by_handle(&connection, owner_did, &handle)
        .map_err(|err| MessageError::Internal(format!("list contact DIDs by handle: {err}")))?;
    Ok(merge_peer_dids(current_did, &dids))
}

pub(crate) fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_string(),
        _ => value.to_string(),
    }
}

fn merge_peer_dids(current: &str, historical: &[String]) -> Vec<String> {
    let mut seen = Vec::with_capacity(historical.len() + 1);
    let mut result = Vec::with_capacity(historical.len() + 1);
    let current = current.trim();
    if !current.is_empty() {
        seen.push(current.to_string());
        result.push(current.to_string());
    }
    for did in historical {
        let did = did.trim();
        if did.is_empty() || seen.iter().any(|known| known == did) {
            continue;
        }
        seen.push(did.to_string());
        result.push(did.to_string());
    }
    result
}

fn lookup_handle_by_did(resolved: &Resolved, did: &str) -> Result<Option<Value>, MessageError> {
    let call = build_handle_lookup_by_did_rpc_call(did)?;
    let client = crate::identity::client::Client::new(resolved)?;
    match client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params) {
        Ok(value) => Ok(Some(value)),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handle_value_matches_go_contact_sync() {
        assert_eq!(normalize_handle_value(" WBA://Alice.Example "), "alice");
        assert_eq!(normalize_handle_value("bob"), "bob");
        assert_eq!(normalize_handle_value("  "), "");
    }

    #[test]
    fn merge_peer_dids_keeps_current_first_and_deduplicates_history() {
        assert_eq!(
            merge_peer_dids(
                " did:peer:new ",
                &[
                    "did:peer:old".to_string(),
                    "did:peer:new".to_string(),
                    " ".to_string(),
                ],
            ),
            vec!["did:peer:new".to_string(), "did:peer:old".to_string()]
        );
    }
}
