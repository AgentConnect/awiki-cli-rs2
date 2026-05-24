use super::client::Client;
use super::did::{generate_identity, generate_identity_with_path_segments};
use super::handle_input::normalize_handle_input;
use super::recover::{build_recover_plan, recover_active_before, RecoverBackupRequest};
use super::store::{
    choose_default_identity_name, choose_named_identity, identity_summary_from_record,
};
use super::types::{
    BindParams, IdentityError, RecoverParams, RegisterParams, SaveInput, LEGACY_LAYOUT_HINT,
};
use super::wire::{
    bind_email_completed_result, bind_email_pending_result, bind_email_sent_result,
    bind_phone_completed_result, bind_phone_otp_result, build_email_send_rest_call,
    build_email_status_rest_call, build_get_me_profile_rpc_call,
    build_handle_lookup_by_did_rpc_call, build_handle_lookup_by_handle_rpc_call,
    build_phone_bind_send_rest_call, build_phone_bind_verify_rest_call,
    build_profile_resolve_rpc_call, build_public_profile_rpc_call, build_recover_handle_rpc_call,
    build_register_rpc_call, build_send_otp_rpc_call, build_update_me_profile_rpc_call,
    normalize_email, profile_public_result, profile_self_result, profile_update_result,
    recover_otp_result, refresh_token_result, register_completed_result, register_phone_otp_result,
    registration_email_pending_result, registration_email_sent_result, resolve_result,
    HandleLookupResult, RecoverHandleRpcParams, RegisterRpcParams, ServiceError,
    UpdateProfileParams, DID_AUTH_RPC_ENDPOINT,
};
use super::Manager;
use crate::authsdk::Session;
use crate::config::Resolved;
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

const DEFAULT_EMAIL_VERIFICATION_SECS: i64 = 300;
const DEFAULT_EMAIL_POLL_INTERVAL_SECS: f64 = 5.0;

pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SetProfileParams {
    pub display_name: String,
    pub bio: String,
    pub tags_csv: String,
    pub markdown: String,
    pub markdown_file: String,
}

#[derive(Debug, Clone, Default)]
pub struct GetProfileParams {
    pub self_profile: bool,
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveParams {
    pub handle: String,
    pub did: String,
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

pub fn create_migration_identity(
    resolved: &Resolved,
    manager: &Manager,
    display_name: &str,
    identity_name: &str,
) -> Result<CommandResult, IdentityError> {
    create_identity(resolved, manager, display_name, identity_name)
}

pub fn register_plan(
    manager: &Manager,
    did_domain: &str,
    params: &RegisterParams,
) -> Result<CommandResult, IdentityError> {
    let target = normalize_handle_input(&params.handle, did_domain)?;
    let existing = manager.list().unwrap_or_default();
    let alias_base = if target.explicit_domain {
        target.full_handle.as_str()
    } else {
        target.local_part.as_str()
    };
    let alias = choose_named_identity(&params.identity_name, &existing, alias_base);
    let phone = params.phone.as_str();
    let email = params.email.as_str();
    let mut action = "register_handle";
    let mut remote_calls = vec!["did-auth.register"];
    if !phone.is_empty() && params.otp.trim().is_empty() {
        action = "send_handle_otp";
        remote_calls = vec!["handle.send_otp"];
    }
    if !email.is_empty() && !params.wait {
        action = "send_registration_email";
        remote_calls = vec!["POST /user-service/auth/email-send"];
    }
    if !email.is_empty() && params.wait {
        remote_calls = vec![
            "GET /user-service/auth/email-status",
            "POST /user-service/auth/email-send",
            "did-auth.register",
        ];
    }
    Ok(CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "identity_name": alias,
                "handle": target.local_part,
                "full_handle": target.full_handle,
                "did_domain": target.effective_domain,
                "phone": phone,
                "email": email,
                "remote_calls": remote_calls,
            }
        }),
        summary: "Dry run: handle registration flow planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn register(
    resolved: &Resolved,
    manager: &Manager,
    params: RegisterParams,
) -> Result<CommandResult, IdentityError> {
    let target = normalize_handle_input(&params.handle, &resolved.did_domain)?;
    let phone = params.phone.trim().to_string();
    let email = normalize_email(&params.email);
    if (phone.is_empty() && email.is_empty()) || (!phone.is_empty() && !email.is_empty()) {
        return Err(IdentityError::InvalidInput(
            "exactly one of phone or email is required".to_string(),
        ));
    }

    let existing = manager.list()?;
    let alias_base = if target.explicit_domain {
        target.full_handle.as_str()
    } else {
        target.local_part.as_str()
    };
    let alias = choose_named_identity(&params.identity_name, &existing, alias_base);
    let client = Client::new(resolved)?;

    if !phone.is_empty() && params.otp.trim().is_empty() {
        let call = build_send_otp_rpc_call(&phone)?;
        let result: Value =
            client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)?;
        return register_phone_otp_result(
            &alias,
            &target.local_part,
            &target.full_handle,
            &phone,
            result,
        );
    }

    if !email.is_empty() {
        let (mut verified, _) =
            check_email_verified(&client, &email, Some(&target.full_handle), false, "")?;
        if !verified {
            let call = build_email_send_rest_call(&email, Some(&target.full_handle), false)?;
            let send_result: Value = client.rest_post(call)?;
            if !params.wait {
                return Ok(registration_email_sent_result(
                    &alias,
                    &target.local_part,
                    &target.full_handle,
                    &email,
                    send_result,
                ));
            }
            verified = wait_for_email_verification(
                &client,
                &email,
                Some(&target.full_handle),
                false,
                "",
                params.verification_timeout,
                params.poll_interval_seconds,
            )?
            .0;
            if !verified {
                return Ok(registration_email_pending_result(
                    &alias,
                    &target.local_part,
                    &target.full_handle,
                    &email,
                ));
            }
        }
    }

    let generated = generate_identity_with_path_segments(
        &target.effective_domain,
        [target.local_part.as_str()],
        &resolved.anp_service_endpoint,
        &resolved.anp_service_did,
    )?;
    let call = build_register_rpc_call(RegisterRpcParams {
        did_document: generated.did_document.clone(),
        handle: target.local_part.clone(),
        phone: (!phone.is_empty()).then(|| phone.clone()),
        otp_code: (!phone.is_empty()).then(|| params.otp.clone()),
        email: (!email.is_empty()).then(|| email.clone()),
        invite_code: params.invite_code,
        ..RegisterRpcParams::default()
    })?;
    let result: Value =
        client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)?;
    let record = manager.save(SaveInput {
        identity_name: alias,
        did: string_value(&result, "did", &generated.did),
        unique_id: generated.unique_id,
        user_id: string_value(&result, "user_id", ""),
        display_name: target.local_part.clone(),
        handle: default_string_value(&result, "handle", &target.local_part),
        full_handle: default_string_value(&result, "full_handle", &target.full_handle),
        jwt_token: string_value(&result, "access_token", ""),
        did_document: Some(generated.did_document),
        key1_private_pem: generated.key1_private_pem,
        key1_public_pem: generated.key1_public_pem,
        e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
        e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
        ..SaveInput::default()
    })?;
    let summary = identity_summary_from_record(&record);
    Ok(register_completed_result(
        &summary,
        &target.full_handle,
        if phone.is_empty() { "email" } else { "phone" },
        result,
    ))
}

pub fn bind_plan(params: &BindParams) -> CommandResult {
    let phone = params.phone.as_str();
    let email = params.email.as_str();
    let mut action = "bind_contact";
    let mut remote_calls: Vec<&str> = Vec::new();
    if !phone.is_empty() && params.otp.trim().is_empty() {
        action = "send_bind_phone_otp";
        remote_calls = vec!["POST /user-service/auth/phone-bind-send"];
    } else if !phone.is_empty() {
        action = "bind_phone";
        remote_calls = vec!["POST /user-service/auth/phone-bind-verify"];
    } else if !email.is_empty() && !params.wait {
        action = "send_bind_email";
        remote_calls = vec!["POST /user-service/auth/email-send"];
    } else if !email.is_empty() {
        action = "bind_email";
        remote_calls = vec![
            "GET /user-service/auth/email-status",
            "POST /user-service/auth/email-send",
        ];
    }
    CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "phone": phone,
                "email": email,
                "remote_calls": remote_calls,
            }
        }),
        summary: "Dry run: contact binding flow planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn bind(
    resolved: &Resolved,
    manager: &Manager,
    params: BindParams,
) -> Result<CommandResult, IdentityError> {
    let record = load_identity_for_mutation(resolved, manager, "")?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let phone = params.phone.trim().to_string();
    let email = normalize_email(&params.email);
    if (phone.is_empty() && email.is_empty()) || (!phone.is_empty() && !email.is_empty()) {
        return Err(IdentityError::InvalidInput(
            "exactly one of phone or email is required".to_string(),
        ));
    }
    let client = Client::new(resolved)?;

    if !phone.is_empty() {
        if params.otp.trim().is_empty() {
            let call = build_phone_bind_send_rest_call(&phone)?;
            let result: Value = client.authenticated_rest_post(call, &mut auth)?;
            return bind_phone_otp_result(&identity, &phone, result);
        }
        let call = build_phone_bind_verify_rest_call(&phone, &params.otp)?;
        let result: Value = client.authenticated_rest_post(call, &mut auth)?;
        return bind_phone_completed_result(&identity, &phone, result);
    }

    let (mut verified, _) = check_email_verified(&client, &email, None, true, auth.current_jwt())?;
    if !verified {
        let call = build_email_send_rest_call(&email, None, true)?;
        let send_result: Value = client.authenticated_rest_post(call, &mut auth)?;
        if !params.wait {
            return Ok(bind_email_sent_result(&identity, &email, send_result));
        }
        verified = wait_for_email_verification(
            &client,
            &email,
            None,
            true,
            auth.current_jwt(),
            params.verification_timeout,
            params.poll_interval_seconds,
        )?
        .0;
        if !verified {
            return Ok(bind_email_pending_result(&identity, &email));
        }
    }
    Ok(bind_email_completed_result(&identity, &email))
}

pub fn recover(
    resolved: &Resolved,
    manager: &Manager,
    params: RecoverParams,
) -> Result<CommandResult, IdentityError> {
    let phone = params.phone.trim().to_string();
    let otp = params.otp.trim().to_string();
    if params.handle.trim().is_empty() || phone.is_empty() {
        return Err(IdentityError::InvalidInput(
            "invalid input: handle and phone are required".to_string(),
        ));
    }
    let plan = build_recover_plan(manager, &resolved.did_domain, &params)?;
    let client = Client::new(resolved)?;

    if otp.is_empty() {
        let call = build_send_otp_rpc_call(&phone)?;
        let result: Value =
            client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)?;
        return recover_otp_result(
            &plan.final_identity_name,
            &plan.target_local_part,
            &plan.target_handle,
            &phone,
            result,
        );
    }

    let generated = generate_identity_with_path_segments(
        &plan.effective_domain,
        [plan.target_local_part.as_str()],
        &resolved.anp_service_endpoint,
        &resolved.anp_service_did,
    )?;
    let active_before = recover_active_before(&resolved.paths.config_file)?;
    let backup = manager.backup_identities_for_handle_recovery(RecoverBackupRequest {
        handle: &plan.target_handle,
        candidates: &plan.same_handle_candidates,
        planned_final_identity_name: &plan.final_identity_name,
        planned_temp_identity_name: &plan.temp_identity_name,
        active_before: &active_before,
        config_file: Some(&resolved.paths.config_file),
    })?;
    let call = build_recover_handle_rpc_call(RecoverHandleRpcParams {
        did_document: generated.did_document.clone(),
        handle: plan.target_handle.clone(),
        phone: phone.clone(),
        otp_code: otp,
    })?;
    let result: Value =
        client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)?;
    let record = manager.save(SaveInput {
        identity_name: plan.temp_identity_name.clone(),
        did: string_value(&result, "did", &generated.did),
        unique_id: generated.unique_id,
        user_id: string_value(&result, "user_id", ""),
        display_name: plan.target_local_part.clone(),
        handle: default_string_value(&result, "handle", &plan.target_local_part),
        full_handle: default_string_value(&result, "full_handle", &plan.target_handle),
        jwt_token: string_value(&result, "access_token", ""),
        did_document: Some(generated.did_document),
        key1_private_pem: generated.key1_private_pem,
        key1_public_pem: generated.key1_public_pem,
        e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
        e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
        ..SaveInput::default()
    })?;
    let summary = identity_summary_from_record(&record);
    Ok(CommandResult {
        data: json!({
            "action": "recover_handle",
            "identity": summary,
            "backup_path": backup.backup_path,
            "archived_identities": plan.archived_identity_names(),
            "archived_dids": plan.archived_dids(),
            "old_dids": plan.old_owner_dids_in_merge_order(),
            "full_handle": plan.target_handle,
            "final_identity_name": plan.final_identity_name,
            "temp_identity_name": plan.temp_identity_name,
            "active_before": active_before,
            "result": result,
        }),
        summary: format!("Handle {} recovered successfully", plan.target_handle),
        warnings: Vec::new(),
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

pub fn refresh_token(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
) -> Result<CommandResult, IdentityError> {
    let record = load_identity_for_mutation(resolved, manager, identity_name)?;
    let previous_token_present = !record.jwt_token.trim().is_empty();
    let mut auth = auth_session_without_stored_bearer(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let request_url =
        crate::config::join_base_url(&resolved.service_base_url, DID_AUTH_RPC_ENDPOINT);
    if client
        .ensure_jwt(&mut auth, &request_url, "identity_refresh_token")
        .is_err()
    {
        return Err(refresh_token_auth_required(&record.identity_name));
    }
    let jwt = auth.current_jwt().trim().to_string();
    if jwt.is_empty() {
        return Err(refresh_token_auth_required(&record.identity_name));
    }
    let mut updated = manager.load(&record.identity_name)?;
    updated.jwt_token = jwt;
    Ok(refresh_token_result(
        &identity_summary_from_record(&updated),
        previous_token_present,
    ))
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

pub fn set_profile(
    resolved: &Resolved,
    manager: &Manager,
    params: SetProfileParams,
) -> Result<CommandResult, IdentityError> {
    let record = load_identity_for_mutation(resolved, manager, "")?;
    let identity = identity_summary_from_record(&record);
    let mut auth = auth_session(resolved, manager, &record)?;
    let call = build_update_me_profile_rpc_call(update_profile_wire_params(&params)?)?;
    let client = Client::new(resolved)?;
    let profile: Value = client.authenticated_rpc_call_profile(
        call.call.profile,
        call.call.endpoint,
        call.call.method,
        call.call.params,
        &mut auth,
    )?;
    if !params.display_name.trim().is_empty() {
        let _ = manager.update_display_name(&record.identity_name, params.display_name.trim());
    }
    Ok(profile_update_result(
        &identity,
        call.changed_fields,
        profile,
    ))
}

pub fn get_profile(
    resolved: &Resolved,
    manager: &Manager,
    params: GetProfileParams,
) -> Result<CommandResult, IdentityError> {
    let self_profile =
        params.self_profile || (params.handle.trim().is_empty() && params.did.trim().is_empty());
    let client = Client::new(resolved)?;
    if self_profile {
        let record = load_identity_for_mutation(resolved, manager, "")?;
        let mut auth = auth_session(resolved, manager, &record)?;
        let call = build_get_me_profile_rpc_call();
        let profile: Value = client.authenticated_rpc_call_profile(
            call.profile,
            call.endpoint,
            call.method,
            call.params,
            &mut auth,
        )?;
        return Ok(profile_self_result(profile));
    }

    let mut subject = Map::new();
    let mut profile_did = params.did.trim().to_string();
    if !params.handle.trim().is_empty() {
        let target = normalize_handle_input(&params.handle, &resolved.did_domain)?;
        let lookup_call = build_handle_lookup_by_handle_rpc_call(&target.full_handle)?;
        let lookup: HandleLookupResult = client.rpc_call_profile(
            lookup_call.profile,
            lookup_call.endpoint,
            lookup_call.method,
            lookup_call.params,
        )?;
        if lookup.did.trim().is_empty() {
            return Err(IdentityError::NotFound(format!(
                "identity not found: handle {} did not resolve to a did",
                target.full_handle
            )));
        }
        profile_did = lookup.did.clone();
        subject.insert("handle".to_string(), Value::String(target.local_part));
        subject.insert("full_handle".to_string(), Value::String(target.full_handle));
        subject.insert("domain".to_string(), Value::String(target.effective_domain));
        subject.insert("did".to_string(), Value::String(lookup.did));
    }
    if !params.did.trim().is_empty() && !subject.contains_key("did") {
        subject.insert(
            "did".to_string(),
            Value::String(params.did.trim().to_string()),
        );
    }
    let profile_call = build_public_profile_rpc_call(&profile_did)?;
    let profile: Value = client.rpc_call_profile(
        profile_call.profile,
        profile_call.endpoint,
        profile_call.method,
        profile_call.params,
    )?;
    Ok(profile_public_result(Value::Object(subject), profile))
}

pub fn resolve_identity(
    resolved: &Resolved,
    params: ResolveParams,
) -> Result<CommandResult, IdentityError> {
    let handle = params.handle.trim();
    let mut did = params.did.trim().to_string();
    if (handle.is_empty() && did.is_empty()) || (!handle.is_empty() && !did.is_empty()) {
        return Err(IdentityError::InvalidInput(
            "invalid input: exactly one of handle or did is required".to_string(),
        ));
    }
    let client = Client::new(resolved)?;
    let mut lookup = None;
    let mut public_profile = None;
    let mut warnings = Vec::new();

    if !handle.is_empty() {
        let target = normalize_handle_input(handle, &resolved.did_domain)?;
        let lookup_call = build_handle_lookup_by_handle_rpc_call(&target.full_handle)?;
        let lookup_value: Value = client.rpc_call_profile(
            lookup_call.profile,
            lookup_call.endpoint,
            lookup_call.method,
            lookup_call.params,
        )?;
        did = lookup_value
            .get("did")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if did.trim().is_empty() {
            return Err(IdentityError::NotFound(format!(
                "identity not found: handle {} did not resolve to a did",
                target.full_handle
            )));
        }
        lookup = Some(lookup_value);
        match public_profile_by_did(&client, &did) {
            Ok(profile) => public_profile = Some(profile),
            Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
        }
    }

    let resolve_call = build_profile_resolve_rpc_call(&did)?;
    let resolve = Some(client.rpc_call_profile(
        resolve_call.profile,
        resolve_call.endpoint,
        resolve_call.method,
        resolve_call.params,
    )?);
    if handle.is_empty() {
        match handle_lookup_by_did(&client, &did) {
            Ok(value) => lookup = Some(value),
            Err(err) => warnings.push(format!("Handle lookup failed: {err}")),
        }
        match public_profile_by_did(&client, &did) {
            Ok(profile) => public_profile = Some(profile),
            Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
        }
    }

    Ok(resolve_result(resolve, lookup, public_profile, warnings))
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
    let did_auth_url = crate::config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
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

fn auth_session_without_stored_bearer(
    resolved: &Resolved,
    manager: &Manager,
    record: &super::types::StoredIdentity,
) -> Result<Session, IdentityError> {
    new_auth_session(resolved, manager, record, "")
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
    let persist_token: crate::authsdk::PersistToken = Box::new(move |token| {
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
    let did_auth_url = crate::config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
    }
    Ok(session)
}

fn update_profile_wire_params(
    params: &SetProfileParams,
) -> Result<UpdateProfileParams, IdentityError> {
    let markdown_file = params.markdown_file.trim();
    let (markdown, preserve_markdown) = if markdown_file.is_empty() {
        (params.markdown.trim().to_string(), false)
    } else {
        let raw = std::fs::read(&params.markdown_file)?;
        (String::from_utf8_lossy(&raw).into_owned(), true)
    };
    Ok(UpdateProfileParams {
        display_name: params.display_name.clone(),
        bio: params.bio.clone(),
        tags_csv: params.tags_csv.clone(),
        markdown,
        preserve_markdown,
    })
}

fn handle_lookup_by_did(client: &Client, did: &str) -> Result<Value, IdentityError> {
    let call = build_handle_lookup_by_did_rpc_call(did)?;
    client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)
}

fn public_profile_by_did(client: &Client, did: &str) -> Result<Value, IdentityError> {
    let call = build_public_profile_rpc_call(did)?;
    client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params)
}

fn check_email_verified(
    client: &Client,
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
    bearer: &str,
) -> Result<(bool, String), IdentityError> {
    let call = build_email_status_rest_call(email, handle, authenticated)?;
    let result: Value = match client.rest_get_with_bearer(call, bearer) {
        Ok(result) => result,
        Err(IdentityError::Service(err)) if service_error_is_not_found(&err) => {
            return Ok((false, String::new()));
        }
        Err(err) => return Err(err),
    };
    let verified = result
        .get("verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verified_at = result
        .get("verified_at")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((verified, verified_at))
}

fn wait_for_email_verification(
    client: &Client,
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
    bearer: &str,
    timeout_secs: i64,
    poll_interval_secs: f64,
) -> Result<(bool, String), IdentityError> {
    let timeout_secs = if timeout_secs <= 0 {
        DEFAULT_EMAIL_VERIFICATION_SECS
    } else {
        timeout_secs
    };
    let poll_interval_secs = if poll_interval_secs <= 0.0 {
        DEFAULT_EMAIL_POLL_INTERVAL_SECS
    } else {
        poll_interval_secs
    };
    let deadline = Instant::now() + Duration::from_secs(timeout_secs as u64);
    loop {
        let (verified, verified_at) =
            check_email_verified(client, email, handle, authenticated, bearer)?;
        if verified {
            return Ok((true, verified_at));
        }
        if Instant::now() >= deadline {
            return Ok((false, String::new()));
        }
        std::thread::sleep(Duration::from_secs_f64(poll_interval_secs));
    }
}

fn service_error_is_not_found(err: &ServiceError) -> bool {
    err.status_code == 404 || err.rpc_code == -32002
}

fn refresh_token_auth_required(identity_name: &str) -> IdentityError {
    IdentityError::AuthRequired(format!(
        "authentication required: failed to refresh jwt for identity {identity_name}"
    ))
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

pub fn import_v1_migration(
    manager: &Manager,
    name: &str,
    all: bool,
) -> Result<CommandResult, IdentityError> {
    import_v1(manager, name, all)
}

fn string_value(result: &Value, key: &str, fallback: &str) -> String {
    result
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn default_string_value(result: &Value, key: &str, fallback: &str) -> String {
    let value = string_value(result, key, "");
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
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
