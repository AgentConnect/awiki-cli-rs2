use super::types::{
    CommandResult, ContentError, CreatePageParams, IdentitySummary, RenamePageParams,
    UpdatePageParams, DID_AUTH_RPC_ENDPOINT,
};
use super::wire::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_list_pages_rpc_call, build_rename_page_rpc_call, build_update_page_rpc_call,
    create_page_summary, delete_page_summary, get_page_summary, page_action_result,
    page_delete_result, page_list_result, page_rename_result, page_update_result,
    rename_page_summary, update_page_summary,
};
use super::Client;
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use serde_json::Value;

pub fn create_page(
    resolved: &Resolved,
    manager: &Manager,
    params: CreatePageParams,
) -> Result<CommandResult, ContentError> {
    let call = build_create_page_rpc_call(params.clone())?;
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let mut result = page_action_result("create_page", &identity, page);
    result.summary = create_page_summary(&params.slug);
    Ok(result)
}

pub fn list_pages(resolved: &Resolved, manager: &Manager) -> Result<CommandResult, ContentError> {
    let call = build_list_pages_rpc_call();
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(page_list_result(&identity, &result))
}

pub fn get_page(
    resolved: &Resolved,
    manager: &Manager,
    slug: &str,
) -> Result<CommandResult, ContentError> {
    let call = build_get_page_rpc_call(slug)?;
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let mut result = page_action_result("get_page", &identity, page);
    result.summary = get_page_summary(slug);
    Ok(result)
}

pub fn update_page(
    resolved: &Resolved,
    manager: &Manager,
    params: UpdatePageParams,
) -> Result<CommandResult, ContentError> {
    let call = build_update_page_rpc_call(params.clone())?;
    let changed_fields = update_changed_fields(&params);
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let mut result = page_update_result(&identity, changed_fields, page);
    result.summary = update_page_summary(&params.slug);
    Ok(result)
}

pub fn rename_page(
    resolved: &Resolved,
    manager: &Manager,
    params: RenamePageParams,
) -> Result<CommandResult, ContentError> {
    let call = build_rename_page_rpc_call(params.clone())?;
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let mut result = page_rename_result(&identity, &params.slug, &params.to, page);
    result.summary = rename_page_summary(&params.slug, &params.to);
    Ok(result)
}

pub fn delete_page(
    resolved: &Resolved,
    manager: &Manager,
    slug: &str,
) -> Result<CommandResult, ContentError> {
    let call = build_delete_page_rpc_call(slug)?;
    let record = require_active_identity(resolved, manager)?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let delete_result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    let mut result = page_delete_result(&identity, slug, delete_result);
    result.summary = delete_page_summary(slug);
    Ok(result)
}

fn require_active_identity(
    resolved: &Resolved,
    manager: &Manager,
) -> Result<StoredIdentity, ContentError> {
    let identity_name = if resolved.active_identity.trim().is_empty() {
        manager
            .current()
            .map_err(|err| match err {
                crate::identity::IdentityError::NoDefaultIdentity(_) => {
                    crate::identity::IdentityError::NotFound(
                        "identity not found: no active identity is configured".to_string(),
                    )
                }
                err => err,
            })?
            .identity_name
    } else {
        resolved.active_identity.clone()
    };
    Ok(manager.load(&identity_name)?)
}

fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<Session, ContentError> {
    if record.identity_name.trim().is_empty() {
        return Err(ContentError::AuthIdentityRequired);
    }
    let paths = manager.paths_for_identity(&record.identity_name)?;
    let identity_name = record.identity_name.clone();
    let persist_manager = manager.clone();
    let persist_identity_name = identity_name.clone();
    let persist_token: crate::authsdk::PersistToken = Box::new(move |token| {
        persist_manager.update_jwt(&persist_identity_name, token)?;
        Ok(())
    });
    let mut session = Session::new(
        &paths.did_document_path,
        &paths.key1_private_path,
        identity_name,
        record.did.as_str(),
        record.jwt_token.as_str(),
        Some(persist_token),
    );
    let base_url = resolved.service_base_url.trim();
    let did_auth_url = crate::config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
    }
    let token = record.jwt_token.trim();
    if !token.is_empty() && !base_url.is_empty() {
        session.set_bearer(base_url, token);
        session.set_bearer(&did_auth_url, token);
    }
    if token.is_empty() {
        let client = Client::new(resolved)?;
        if let Err(err) = client.ensure_jwt(&mut session, &did_auth_url) {
            return match err {
                ContentError::Service(err) => Err(ContentError::Service(err)),
                err => Err(ContentError::Internal(format!(
                    "active identity does not have a JWT yet: {err}"
                ))),
            };
        }
    }
    Ok(session)
}

fn identity_summary_from_record(record: &StoredIdentity) -> IdentitySummary {
    IdentitySummary {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        handle: record.handle.clone(),
    }
}

fn update_changed_fields(params: &UpdatePageParams) -> Vec<String> {
    let mut changed_fields = Vec::new();
    if !params.title.trim().is_empty() {
        changed_fields.push("title".to_string());
    }
    if params.body.is_some() {
        changed_fields.push("body".to_string());
    }
    if params.visibility.is_some() {
        changed_fields.push("visibility".to_string());
    }
    changed_fields
}
