use crate::identity;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub const SECURE_SESSION_DIR_NAME: &str = "p5-e2ee-sessions";
pub const PENDING_CONFIRMATION_STATUS: &str = "pending-confirmation";

pub fn pending_confirmation_peer_dids(
    manager: Option<&identity::Manager>,
    identity_name: &str,
) -> Vec<String> {
    let Some(manager) = manager else {
        return Vec::new();
    };
    if identity_name.trim().is_empty() {
        return Vec::new();
    }
    let Ok(paths) = manager.paths_for_identity(identity_name) else {
        return Vec::new();
    };
    pending_confirmation_peer_dids_in_identity_dir(Path::new(&paths.identity_dir))
}

pub fn pending_confirmation_peer_dids_in_identity_dir(identity_dir: &Path) -> Vec<String> {
    let mut entries = match std::fs::read_dir(identity_dir.join(SECURE_SESSION_DIR_NAME)) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_json_file(path))
            .collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };
    entries.sort();

    let mut peers = Vec::with_capacity(entries.len());
    let mut seen = HashSet::new();
    for path in entries {
        let Ok(payload) = read_json_file(&path) else {
            continue;
        };
        if string_from_value(payload.get("status")) != PENDING_CONFIRMATION_STATUS {
            continue;
        }
        let peer_did = string_from_value(payload.get("peer_did"));
        if peer_did.trim().is_empty() || !seen.insert(peer_did.clone()) {
            continue;
        }
        peers.push(peer_did);
    }
    peers
}

pub fn read_json_file(path: &Path) -> anyhow::Result<Value> {
    let raw = std::fs::read(path)?;
    Ok(serde_json::from_slice(&raw)?)
}

fn is_json_file(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("json")
}

fn string_from_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}
