use super::auth::{PersistToken, Session};
use super::client::Client;
use super::did::generate_identity;
use super::legacy_store::{choose_default_identity_name, identity_summary_from_record};
use super::types::{IdentityError, SaveInput};
use super::wire::DID_AUTH_RPC_ENDPOINT;
use super::Manager;
use crate::workspace_config::Resolved;
use serde_json::{json, Value};

pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

pub fn create_migration_identity(
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

pub fn import_v1_migration(
    manager: &Manager,
    name: &str,
    all: bool,
) -> Result<CommandResult, IdentityError> {
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

pub(crate) fn load_identity_for_mutation(
    resolved: &Resolved,
    manager: &Manager,
    requested: &str,
) -> Result<super::types::StoredIdentity, IdentityError> {
    let identity_name = if requested.trim().is_empty() {
        if resolved.active_identity.trim().is_empty() {
            manager
                .current()
                .map_err(|err| match err {
                    IdentityError::NoDefaultIdentity(_) => IdentityError::NotFound(
                        "identity not found: no active identity is configured".to_string(),
                    ),
                    err => err,
                })?
                .identity_name
        } else {
            resolved.active_identity.clone()
        }
    } else {
        requested.trim().to_string()
    };
    manager.load(&identity_name)
}

pub(crate) fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &super::types::StoredIdentity,
) -> Result<Session, IdentityError> {
    let mut session = new_auth_session(resolved, manager, record, record.jwt_token.as_str())?;
    let base_url = resolved.service_base_url.trim();
    let did_auth_url = crate::workspace_config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    let token = record.jwt_token.trim();
    if !token.is_empty() && !base_url.is_empty() {
        session.set_bearer(base_url, token);
        session.set_bearer(&did_auth_url, token);
    }
    if token.is_empty() {
        let client = Client::new(resolved)?;
        if let Err(err) = client.ensure_jwt(&mut session, &did_auth_url, "identity_bootstrap") {
            return match err {
                IdentityError::Service(err) => Err(IdentityError::Service(err)),
                err => Err(IdentityError::Internal(format!(
                    "active identity does not have a JWT yet: {err}"
                ))),
            };
        }
    }
    Ok(session)
}

fn new_auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &super::types::StoredIdentity,
    jwt_token: &str,
) -> Result<Session, IdentityError> {
    if record.identity_name.trim().is_empty() {
        return Err(IdentityError::AuthRequired(
            "authentication required: active identity is required".to_string(),
        ));
    }
    let paths = manager.paths_for_identity(&record.identity_name)?;
    let identity_name = record.identity_name.clone();
    let persist_manager = manager.clone();
    let persist_identity_name = identity_name.clone();
    let persist_token: PersistToken = Box::new(move |token| {
        persist_manager.update_jwt(&persist_identity_name, token)?;
        Ok(())
    });
    let mut session = Session::new(
        &paths.did_document_path,
        &paths.key1_private_path,
        identity_name,
        record.did.as_str(),
        jwt_token,
        Some(persist_token),
    );
    let base_url = resolved.service_base_url.trim();
    let did_auth_url = crate::workspace_config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
    }
    Ok(session)
}
