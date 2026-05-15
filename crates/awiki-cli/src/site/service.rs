use super::types::{
    CommandResult, CreatePageParams, IdentitySummary, RenamePageParams, SetRootParams, SiteError,
    UpdatePageParams, DID_AUTH_RPC_ENDPOINT,
};
use super::wire::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_get_root_rpc_call, build_list_pages_rpc_call, build_rename_page_rpc_call,
    build_set_root_rpc_call, build_update_page_rpc_call, page_create_result, page_delete_result,
    page_get_result, page_list_result, page_rename_result, page_update_result, root_get_result,
    root_set_result,
};
use super::Client;
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use serde_json::Value;

pub fn get_root(
    resolved: &Resolved,
    manager: &Manager,
    domain: &str,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_get_root_rpc_call(domain)?;
    let domain = call_domain(&call.params).to_string();
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let root: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(root_get_result(&identity, &domain, root))
}

pub fn set_root(
    resolved: &Resolved,
    manager: &Manager,
    params: SetRootParams,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_set_root_rpc_call(params)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let root: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(root_set_result(&identity, call_domain(&call.params), root))
}

pub fn list_pages(
    resolved: &Resolved,
    manager: &Manager,
    domain: &str,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_list_pages_rpc_call(domain)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_list_result(
        &identity,
        call_domain(&call.params),
        &result,
    ))
}

pub fn get_page(
    resolved: &Resolved,
    manager: &Manager,
    domain: &str,
    slug: &str,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_get_page_rpc_call(domain, slug)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_get_result(
        &identity,
        call_domain(&call.params),
        call_slug(&call.params, "slug"),
        page,
    ))
}

pub fn create_page(
    resolved: &Resolved,
    manager: &Manager,
    params: CreatePageParams,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_create_page_rpc_call(params)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_create_result(
        &identity,
        call_domain(&call.params),
        call_slug(&call.params, "slug"),
        page,
    ))
}

pub fn update_page(
    resolved: &Resolved,
    manager: &Manager,
    params: UpdatePageParams,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_update_page_rpc_call(params)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_update_result(
        &identity,
        call_domain(&call.params),
        call_slug(&call.params, "slug"),
        page,
    ))
}

pub fn rename_page(
    resolved: &Resolved,
    manager: &Manager,
    params: RenamePageParams,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_rename_page_rpc_call(params)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let page: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_rename_result(
        &identity,
        call_domain(&call.params),
        call_slug(&call.params, "old_slug"),
        call_slug(&call.params, "new_slug"),
        page,
    ))
}

pub fn delete_page(
    resolved: &Resolved,
    manager: &Manager,
    domain: &str,
    slug: &str,
) -> Result<CommandResult, SiteError> {
    let (record, mut auth) = require_auth(resolved, manager)?;
    let call = build_delete_page_rpc_call(domain, slug)?;
    let identity = identity_summary_from_record(&record);
    let client = Client::new(resolved)?;
    let delete_result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params.clone(),
        &mut auth,
    )?;
    Ok(page_delete_result(
        &identity,
        call_domain(&call.params),
        call_slug(&call.params, "slug"),
        delete_result,
    ))
}

fn require_auth(
    resolved: &Resolved,
    manager: &Manager,
) -> Result<(StoredIdentity, Session), SiteError> {
    let record = require_active_identity(resolved, manager)?;
    let session = auth_session(resolved, manager, &record)?;
    Ok((record, session))
}

fn require_active_identity(
    resolved: &Resolved,
    manager: &Manager,
) -> Result<StoredIdentity, SiteError> {
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
) -> Result<Session, SiteError> {
    if record.identity_name.trim().is_empty() {
        return Err(SiteError::AuthIdentityRequired);
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
        if let Err(err) = client.ensure_jwt(&mut session, &did_auth_url, "site_bootstrap") {
            return match err {
                SiteError::Service(err) => Err(SiteError::Service(err)),
                err => Err(SiteError::Internal(format!(
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

fn call_domain(params: &Value) -> &str {
    params
        .get("domain")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn call_slug<'a>(params: &'a Value, key: &str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or_default()
}
