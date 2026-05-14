use super::did::generate_identity;
use super::store::{choose_default_identity_name, identity_summary_from_record};
use super::types::{IdentityError, SaveInput, LEGACY_LAYOUT_HINT};
use super::Manager;
use crate::config::Resolved;
use serde_json::{json, Map, Value};

pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

pub fn status(manager: &Manager) -> Result<CommandResult, IdentityError> {
    let current = manager.current();
    let legacy = manager.scan_legacy()?;
    let identities = manager.list()?;
    let active_identity = current.as_ref().ok().cloned();
    let mut warnings = Vec::new();
    if legacy.has_legacy {
        warnings.push(LEGACY_LAYOUT_HINT.to_string());
    }
    let mut summary = "Identity store is ready".to_string();
    if current.is_err() {
        summary = "No default identity is configured yet".to_string();
    } else if let Some(identity) = &active_identity {
        if !identity.user_state.ready_for_messaging {
            summary = "Default identity exists but user setup is incomplete".to_string();
            warnings.push(
                "Current identity is local-only. Register or recover a handle-backed user before using messaging."
                    .to_string(),
            );
        }
    }
    Ok(CommandResult {
        data: json!({
            "active_identity": active_identity,
            "identity_count": identities.len(),
            "legacy_scan": legacy,
        }),
        summary,
        warnings,
    })
}

pub fn list_identities(manager: &Manager) -> Result<CommandResult, IdentityError> {
    let identities = manager.list()?;
    let identity_count = identities.len();
    let current = manager.current().ok();
    let legacy = manager.scan_legacy()?;
    let mut warnings = Vec::new();
    if legacy.has_legacy {
        warnings.push(LEGACY_LAYOUT_HINT.to_string());
    }
    if current
        .as_ref()
        .is_some_and(|identity| !identity.user_state.ready_for_messaging)
    {
        warnings.push(
            "The default identity is local-only. Register or recover a handle-backed user before using messaging."
                .to_string(),
        );
    }
    Ok(CommandResult {
        data: json!({
            "identities": identities,
            "default_identity": current,
            "legacy_scan": legacy,
        }),
        summary: format!("Found {identity_count} local identities"),
        warnings,
    })
}

pub fn current_identity(manager: &Manager) -> Result<CommandResult, IdentityError> {
    match manager.current() {
        Ok(identity) => {
            let mut summary = format!("Current identity is {}", identity.identity_name);
            let mut warnings = Vec::new();
            if !identity.user_state.ready_for_messaging {
                summary = format!("Current identity {} is local-only", identity.identity_name);
                warnings.push(
                    "Register or recover a handle-backed user before using messaging commands."
                        .to_string(),
                );
            }
            Ok(CommandResult {
                data: json!({ "identity": identity }),
                summary,
                warnings,
            })
        }
        Err(IdentityError::NoDefaultIdentity(_)) => Ok(CommandResult {
            data: json!({ "identity": Value::Null }),
            summary: "No default identity is configured".to_string(),
            warnings: Vec::new(),
        }),
        Err(err) => Err(err),
    }
}

pub fn switch_default_identity(
    manager: &Manager,
    identity_name: &str,
) -> Result<CommandResult, IdentityError> {
    let summary = manager.set_default(identity_name)?;
    Ok(CommandResult {
        data: json!({
            "action": "set_default_identity",
            "identity": summary,
        }),
        summary: format!("Default identity switched to {}", identity_name),
        warnings: Vec::new(),
    })
}

pub fn create_identity(
    resolved: &Resolved,
    manager: &Manager,
    display_name: &str,
    identity_name: &str,
) -> Result<CommandResult, IdentityError> {
    let existing = manager.list()?;
    let alias = choose_default_identity_name(identity_name, &existing, display_name);
    let generated = generate_identity(
        &resolved.did_domain,
        &resolved.anp_service_endpoint,
        &resolved.anp_service_did,
    )?;
    let record = manager.save(SaveInput {
        identity_name: alias,
        did: generated.did,
        unique_id: generated.unique_id,
        display_name: display_name.to_string(),
        did_document: Some(generated.did_document),
        key1_private_pem: generated.key1_private_pem,
        key1_public_pem: generated.key1_public_pem,
        e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
        e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
        ..SaveInput::default()
    })?;
    let summary = identity_summary_from_record(&record);
    Ok(CommandResult {
        data: json!({ "action": "create_identity", "identity": summary }),
        summary: format!("Created local identity {}", record.identity_name),
        warnings: vec![
            "This identity is local-only until you complete `awiki-cli id register --handle <handle> ...` or recover an existing handle.".to_string(),
        ],
    })
}

pub fn use_plan(identity_name: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "set_default_identity",
                "identity_name": identity_name,
                "writes": ["index.json"],
                "side_effect": true,
                "previous_source": "identity_index",
            }
        }),
        summary: "Dry run: default identity switch planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn refresh_token_plan(manager: &Manager, selected: &str) -> CommandResult {
    let identity_name = if selected.trim().is_empty() {
        manager
            .current()
            .ok()
            .map(|identity| identity.identity_name)
            .unwrap_or_default()
    } else {
        selected.to_string()
    };
    CommandResult {
        data: json!({
            "plan": {
                "action": "refresh_token",
                "identity_name": identity_name,
                "remote_calls": ["did-auth.get_me"],
                "local_writes": ["auth.json"],
                "auth_flow": "did_auth_get_me_without_stored_bearer",
            }
        }),
        summary: "Dry run: JWT refresh planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn replace_did_plan(
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> CommandResult {
    let mut remote_params = Map::new();
    remote_params.insert(
        "new_did_document".to_string(),
        json!("generated_e1_document"),
    );
    if let Some(value) = is_public {
        remote_params.insert("is_public".to_string(), json!(value));
    }
    if let Some(value) = is_agent {
        remote_params.insert("is_agent".to_string(), json!(value));
    }
    if let Some(value) = role {
        remote_params.insert("role".to_string(), json!(value));
    }
    if let Some(value) = endpoint_url {
        remote_params.insert("endpoint_url".to_string(), json!(value));
    }
    CommandResult {
        data: json!({
            "plan": {
                "action": "replace_did",
                "identity_name": identity_name,
                "dangerous": true,
                "remote_calls": ["did-auth.replace_did"],
                "remote_params": remote_params,
                "local_writes": [
                    "index.json",
                    "identity.json",
                    "auth.json",
                    "did_document.json",
                    "key-1-private.pem",
                    "key-1-public.pem",
                    "e2ee-signing-private.pem",
                    "e2ee-agreement-private.pem",
                    ".legacy-backup/replace-did",
                    "sqlite.owner_did_rebind",
                    "sqlite.e2ee_cleanup",
                ],
            }
        }),
        summary: "Dry run: DID replacement planned".to_string(),
        warnings: vec![replace_did_danger_warning().to_string()],
    }
}

pub fn replace_did_danger_warning() -> &'static str {
    "Dangerous command: replace-did creates a new e1 DID and key material, replaces the selected identity's current DID, and rebinds local SQLite owner state. The old DID material is backed up locally and remains sensitive. Verify the target identity and prefer --dry-run first."
}

pub fn import_v1(manager: &Manager, name: &str, all: bool) -> Result<CommandResult, IdentityError> {
    let result = if all {
        manager.import_all_legacy()?
    } else {
        manager.import_legacy(name.to_string())?
    };
    Ok(CommandResult {
        data: json!({ "result": result }),
        summary: "Legacy identity import completed".to_string(),
        warnings: Vec::new(),
    })
}

pub fn sanitize_public_value(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut sanitized = Map::new();
            for (key, value) in object {
                if matches!(key.as_str(), "user_id" | "userId" | "UserID") {
                    continue;
                }
                sanitized.insert(key, sanitize_public_value(value));
            }
            Value::Object(sanitized)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_public_value).collect()),
        other => other,
    }
}
