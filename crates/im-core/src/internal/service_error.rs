//! Stable semantic classification for remote service errors.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StaleTargetBindingHint {
    pub(crate) current_did: Option<String>,
    pub(crate) full_handle: Option<String>,
}

pub(crate) fn stale_target_binding_from_error(
    error: &crate::ImError,
    owner_did: &str,
) -> Option<StaleTargetBindingHint> {
    let crate::ImError::Service { code, data, .. } = error else {
        return None;
    };
    let data = data.as_ref()?;
    if data.get("reason").and_then(serde_json::Value::as_str) != Some("stale_did") {
        return None;
    }
    let stable_code = code.as_deref();
    let numeric_code = data.get("json_rpc_code").and_then(|value| match value {
        serde_json::Value::Number(value) => value.as_i64().map(|value| value.to_string()),
        serde_json::Value::String(value) => Some(value.trim().to_owned()),
        _ => None,
    });
    if stable_code != Some("anp.invalid_target_binding")
        && stable_code != Some("1406")
        && numeric_code.as_deref() != Some("1406")
    {
        return None;
    }

    let current_did = data
        .get("current_did")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| canonical_peer_did(value, owner_did));
    let full_handle = data
        .get("full_handle")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some(StaleTargetBindingHint {
        current_did,
        full_handle,
    })
}

fn canonical_peer_did(value: &str, owner_did: &str) -> Option<String> {
    let value = value.trim();
    if value == owner_did.trim() || crate::ids::Did::parse(value).is_err() {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(test)]
mod tests;
