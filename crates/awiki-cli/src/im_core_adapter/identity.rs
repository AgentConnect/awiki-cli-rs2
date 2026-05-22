// Temporary migration-only legacy bridge exception.
// Delete in PR C3/C7 when identity/profile/directory default handlers call
// im-core public APIs directly and this adapter no longer converts SDK DTOs
// back to legacy identity requests, stores, clients, or compat bridges.

use im_core::prelude::{
    ContactBindingMethod, ContactBindingMethodKind, ContactBindingRequest, ContactBindingResult,
    ContactBindingState, Did, DirectoryResolution, Handle, HandleRegistrationResult,
    HandleRegistrationState, IdentitySelector, IdentitySubject, InitialProfile, PeerRef,
    ProfilePatch, RecoverGeneratedIdentity, RecoverHandleRequest, RegisterHandleRequest,
    RegistrationMethod, VerificationInput,
};
use serde::Serialize;
use serde_json::json;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::cli::ParsedCommand;
use crate::config;
use crate::identity;
use crate::identity::types::{StoredIdentity, LEGACY_LAYOUT_HINT};
use crate::output::ExitError;
use crate::store;

pub use super::identity_replace_did_plan::{
    replace_did_plan_bridge_request, replace_did_plan_via_im_core, ReplaceDidPlanBridgeRequest,
};

fn identity_diagnostic_raw(result: &impl IdentityDiagnosticRaw) -> Value {
    result.diagnostic_raw().cloned().unwrap_or(Value::Null)
}

trait IdentityDiagnosticRaw {
    fn diagnostic_raw(&self) -> Option<&Value>;
}

impl IdentityDiagnosticRaw for ContactBindingResult {
    fn diagnostic_raw(&self) -> Option<&Value> {
        ContactBindingResult::diagnostic_raw(self)
    }
}

impl IdentityDiagnosticRaw for im_core::identity::RecoverHandleResult {
    fn diagnostic_raw(&self) -> Option<&Value> {
        im_core::identity::RecoverHandleResult::diagnostic_raw(self)
    }
}

#[derive(Debug, Clone)]
pub struct GetProfileBridgeRequest {
    pub self_profile: bool,
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct ResolveBridgeRequest {
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct SetProfileCommandRequest {
    pub patch: ProfilePatch,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct BindContactCommandRequest {
    pub sdk: ContactBindingRequest,
    pub verification_timeout: i64,
    pub poll_interval_seconds: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RecoverHandleCommandRequest {
    pub identity_name: String,
    pub handle: String,
    pub phone: String,
    pub otp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverLocalStateMergeResult {
    pub store_merge_counts: BTreeMap<String, i64>,
    pub e2ee_cleanup_counts: BTreeMap<String, i64>,
}

const RECOVER_REPAIR_HINT: &str =
    "Inspect the returned backup path and temporary identity, then repair the local workspace state before retrying.";

pub fn cli_identity_selector(identity_flag: &str) -> IdentitySelector {
    let value = identity_flag.trim();
    if value.is_empty() || value == "default" {
        return IdentitySelector::Default;
    }
    if value.starts_with("did:") {
        return Did::parse(value)
            .map(IdentitySelector::Did)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    if looks_like_handle(value) {
        return Handle::parse(value, "")
            .map(IdentitySelector::Handle)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    IdentitySelector::LocalAlias(value.to_string())
}

pub fn register_handle_request(
    command: &ParsedCommand,
) -> Result<RegisterHandleRequest, ExitError> {
    let handle = string_flag(command, "handle");
    let requested_handle = Handle::parse(&handle, "").map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --handle: {err}"),
            "Use a non-empty handle local part or full handle.",
        )
    })?;
    let local_alias = trimmed_optional(&command.globals.identity);
    let phone = trimmed_optional(&string_flag(command, "phone"));
    let email = trimmed_optional(&string_flag(command, "email"));
    let otp = string_flag(command, "otp");
    let otp = trimmed_optional(&otp);
    let wait_for_verification = command
        .flags
        .get("wait")
        .is_some_and(|value| value == "true");
    let verification = match (phone, email, otp) {
        (Some(phone), None, otp) => VerificationInput::Phone { phone, otp },
        (None, Some(email), None) => VerificationInput::Email {
            email,
            wait_for_verification,
        },
        (None, None, Some(code)) => VerificationInput::Otp { code },
        (None, None, None) => VerificationInput::AlreadyVerified,
        (Some(_), Some(_), _) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id register accepts either --phone or --email, but not both.",
                "Choose one verification method for handle registration.",
            ));
        }
        (None, Some(_), Some(_)) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id register --otp requires --phone when --email is not used.",
                "Use --phone with --otp for phone registration, or use --email without --otp.",
            ));
        }
    };
    Ok(RegisterHandleRequest {
        local_alias,
        requested_handle,
        verification,
        invite_code: trimmed_optional(&string_flag(command, "invite-code")),
        profile: InitialProfile {
            display_name: trimmed_optional(&string_flag(command, "display-name")),
            avatar_url: trimmed_optional(&string_flag(command, "avatar-url")),
        },
        make_default: !command
            .flags
            .get("no-default")
            .is_some_and(|value| value == "true"),
    })
}

pub fn register_handle_command_request(
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<RegisterHandleRequest, ExitError> {
    let mut sdk_command = command.clone();
    sdk_command.globals.identity = identity_flag.to_string();
    register_handle_request(&sdk_command)
}

pub fn register_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    register_handle_plan_command_result(manager, did_domain, request)
}

pub fn register_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    let core = super::build_im_core(resolved, manager)?;
    let result = core
        .identities()
        .register_handle(request.clone())
        .map_err(|err| super::map_im_error(err, "id register"))?;
    register_handle_command_result(result, manager, &request)
}

fn register_handle_command_result(
    result: HandleRegistrationResult,
    manager: &identity::Manager,
    request: &RegisterHandleRequest,
) -> Result<identity::CommandResult, ExitError> {
    let full_handle = result.handle.as_str().to_string();
    let handle = full_handle
        .split_once('.')
        .map(|(local, _)| local)
        .unwrap_or(full_handle.as_str())
        .to_string();
    let method = registration_method_label(result.method);
    let verification_state = registration_state_label(result.state);
    let identity_name = result
        .identity
        .as_ref()
        .map(sdk_identity_name)
        .unwrap_or_else(|| pending_registration_identity_name(request, &handle, &full_handle));
    let mut data = json!({
        "action": registration_action(result.state),
        "identity_name": identity_name,
        "handle": handle,
        "full_handle": full_handle,
        "method": method,
        "verification_state": verification_state,
    });
    if let Some(identity) = result.identity.as_ref() {
        data["identity"] = json!(cli_identity_summary_from_sdk_with_manager(
            identity, manager
        )?);
    }
    if let Some(phone) = registration_phone(&request.verification) {
        data["phone"] = json!(normalize_registration_phone(phone)?);
    }
    if let Some(email) = registration_email(&request.verification) {
        data["email"] = json!(normalize_registration_email(email));
    }
    Ok(identity::CommandResult {
        summary: registration_summary(result.state, data["full_handle"].as_str().unwrap_or("")),
        data,
        warnings: result.warnings,
    })
}

pub fn list_identities_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
) -> Result<identity::CommandResult, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    let identities = core
        .identities()
        .list()
        .map_err(|err| super::map_im_error(err, "id list"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities, manager)?;
    let identity_count = summaries.len();
    let current = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let legacy = manager.scan_legacy().map_err(crate::app::identity_exit)?;
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
    Ok(identity::CommandResult {
        data: json!({
            "identities": summaries,
            "default_identity": current,
            "legacy_scan": legacy,
        }),
        summary: format!("Found {identity_count} local identities"),
        warnings,
    })
}

pub fn current_identity_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
) -> Result<identity::CommandResult, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    let default_identity = core
        .identities()
        .default_identity()
        .map_err(|err| super::map_im_error(err, "id current"))?;
    let Some(default_identity) = default_identity else {
        return Ok(identity::CommandResult {
            data: json!({ "identity": Value::Null }),
            summary: "No default identity is configured".to_string(),
            warnings: Vec::new(),
        });
    };
    let current = cli_identity_summary_from_sdk_with_manager(&default_identity, manager)?;
    let mut summary = format!("Current identity is {}", current.identity_name);
    let mut warnings = Vec::new();
    if !current.user_state.ready_for_messaging {
        summary = format!("Current identity {} is local-only", current.identity_name);
        warnings.push(
            "Register or recover a handle-backed user before using messaging commands.".to_string(),
        );
    }
    Ok(identity::CommandResult {
        data: json!({ "identity": current }),
        summary,
        warnings,
    })
}

pub fn identity_status_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
) -> Result<identity::CommandResult, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    let identities = core
        .identities()
        .list()
        .map_err(|err| super::map_im_error(err, "id status"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities, manager)?;
    let legacy = manager.scan_legacy().map_err(crate::app::identity_exit)?;
    let active_identity = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let mut warnings = Vec::new();
    if legacy.has_legacy {
        warnings.push(LEGACY_LAYOUT_HINT.to_string());
    }
    let mut summary = "Identity store is ready".to_string();
    if active_identity.is_none() {
        summary = "No default identity is configured yet".to_string();
    } else if active_identity
        .as_ref()
        .is_some_and(|identity| !identity.user_state.ready_for_messaging)
    {
        summary = "Default identity exists but user setup is incomplete".to_string();
        warnings.push(
            "Current identity is local-only. Register or recover a handle-backed user before using messaging."
                .to_string(),
        );
    }
    Ok(identity::CommandResult {
        data: json!({
            "active_identity": active_identity,
            "identity_count": summaries.len(),
            "legacy_scan": legacy,
        }),
        summary,
        warnings,
    })
}

pub fn use_identity_plan_via_im_core(identity_name: &str) -> identity::CommandResult {
    identity::use_plan(identity_name)
}

pub fn use_identity_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
) -> Result<identity::CommandResult, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    let change = core
        .identities()
        .plan_default_identity_change(IdentitySelector::LocalAlias(identity_name.to_string()))
        .map_err(|err| super::map_im_error(err, "id use"))?;
    let target_name = change
        .next
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(identity_name);
    let summary = manager
        .set_default(target_name)
        .map_err(crate::app::identity_exit)?;
    Ok(identity::CommandResult {
        data: json!({
            "action": "set_default_identity",
            "identity": summary,
        }),
        summary: format!("Default identity switched to {target_name}"),
        warnings: Vec::new(),
    })
}

pub fn bind_contact_request(command: &ParsedCommand) -> Result<ContactBindingRequest, ExitError> {
    let phone = string_flag(command, "phone");
    let email = string_flag(command, "email");
    let otp = string_flag(command, "otp");
    let phone_set = !phone.trim().is_empty();
    let email_set = !email.trim().is_empty();
    if phone_set == email_set {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "exactly one of phone or email is required",
            "Pass either --phone <number> or --email <address>.",
        ));
    }
    Ok(ContactBindingRequest {
        method: if phone_set {
            ContactBindingMethod::Phone {
                phone,
                otp: trimmed_optional(&otp),
            }
        } else {
            ContactBindingMethod::Email { email }
        },
        wait_for_email_verification: command
            .flags
            .get("wait")
            .is_some_and(|value| value == "true"),
    })
}

pub fn bind_contact_command_request(
    command: &ParsedCommand,
) -> Result<BindContactCommandRequest, ExitError> {
    let sdk = bind_contact_request(command)?;
    Ok(BindContactCommandRequest {
        sdk,
        verification_timeout: 300,
        poll_interval_seconds: 5.0,
    })
}

pub fn bind_contact_plan_via_im_core(
    command: &ParsedCommand,
) -> Result<identity::CommandResult, ExitError> {
    let request = bind_contact_command_request(command)?;
    Ok(bind_contact_plan_command_result(&request.sdk))
}

pub fn bind_contact_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    command: &ParsedCommand,
) -> Result<identity::CommandResult, ExitError> {
    let request = bind_contact_command_request(command)?;
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let identity = identity::store::identity_summary_from_record(&record);
    let result = if request.sdk.wait_for_email_verification
        && matches!(request.sdk.method, ContactBindingMethod::Email { .. })
    {
        bind_email_wait_via_im_core(&client, &identity, request)?
    } else {
        let result = client
            .identity()
            .bind_contact(request.sdk)
            .map_err(|err| super::map_im_error(err, "id bind"))?;
        bind_command_result(&identity, result).map_err(crate::app::identity_exit)?
    };
    Ok(result)
}

pub fn recover_handle_request(
    handle: String,
    phone: String,
    otp: Option<String>,
    generated_identity: Option<RecoverGeneratedIdentity>,
    default_domain: &str,
) -> Result<RecoverHandleRequest, ExitError> {
    Ok(RecoverHandleRequest {
        handle: Handle::parse(handle, default_domain)
            .map_err(|err| super::map_im_error(err, "id recover"))?,
        phone,
        otp,
        generated_identity,
    })
}

pub fn recover_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    request: RecoverHandleCommandRequest,
) -> Result<identity::CommandResult, ExitError> {
    recover_handle_plan_command_result(manager, did_domain, &request)
}

pub fn recover_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    request: RecoverHandleCommandRequest,
) -> Result<identity::CommandResult, ExitError> {
    let phone = request.phone.trim().to_string();
    let otp = request.otp.trim().to_string();
    if request.handle.trim().is_empty() || phone.is_empty() {
        return Err(crate::app::identity_exit(
            identity::IdentityError::InvalidInput(
                "invalid input: handle and phone are required".to_string(),
            ),
        ));
    }

    let plan = recover_handle_plan(manager, &resolved.did_domain, &request)?;

    if otp.is_empty() {
        let core = super::build_im_core(resolved, manager)?;
        let sdk_request = recover_handle_request(
            request.handle.clone(),
            request.phone.clone(),
            trimmed_optional(&request.otp),
            None,
            &resolved.did_domain,
        )?;
        let result = core
            .identities()
            .recover_handle(sdk_request)
            .map_err(|err| super::map_im_error(err, "id recover"))?;
        let raw = identity_diagnostic_raw(&result);
        return identity::wire::recover_otp_result(
            &plan.final_identity_name,
            &plan.target_local_part,
            &plan.target_handle,
            &phone,
            raw,
        )
        .map_err(crate::app::identity_exit);
    }

    let core = super::build_im_core(resolved, manager)?;
    let generated = identity::generate_identity_with_path_segments(
        &plan.effective_domain,
        [plan.target_local_part.as_str()],
        &resolved.anp_service_endpoint,
        &resolved.anp_service_did,
    )
    .map_err(crate::app::identity_exit)?;
    let sdk_generated = RecoverGeneratedIdentity {
        did: Did::parse(&generated.did).map_err(|err| super::map_im_error(err, "id recover"))?,
        unique_id: generated.unique_id.clone(),
        did_document: generated.did_document.clone(),
    };
    let sdk_request = recover_handle_request(
        request.handle.clone(),
        request.phone.clone(),
        trimmed_optional(&request.otp),
        Some(sdk_generated),
        &resolved.did_domain,
    )?;
    let active_before = recover_active_before(&resolved.paths.config_file)?;
    let backup =
        recover_handle_backup(manager, &plan, &active_before, &resolved.paths.config_file)?;

    let result = core
        .identities()
        .recover_handle(sdk_request)
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    let raw = identity_diagnostic_raw(&result);
    let record = manager
        .save(identity::types::SaveInput {
            identity_name: plan.temp_identity_name.clone(),
            did: string_value(&raw, "did", &generated.did),
            unique_id: generated.unique_id,
            user_id: string_value(&raw, "user_id", ""),
            display_name: plan.target_local_part.clone(),
            handle: default_string_value(&raw, "handle", &plan.target_local_part),
            full_handle: default_string_value(&raw, "full_handle", &plan.target_handle),
            jwt_token: string_value(&raw, "access_token", ""),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..identity::types::SaveInput::default()
        })
        .map_err(crate::app::identity_exit)?;
    let summary = identity::store::identity_summary_from_record(&record);
    Ok(identity::CommandResult {
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
            "result": raw,
        }),
        summary: format!("Handle {} recovered successfully", plan.target_handle),
        warnings: Vec::new(),
    })
}

pub fn recover_handle_command_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    request: RecoverHandleCommandRequest,
    dry_run: bool,
    identity_changed: bool,
) -> Result<identity::CommandResult, ExitError> {
    let mut result = if dry_run {
        recover_handle_plan_via_im_core(manager, &resolved.did_domain, request)
    } else {
        recover_handle_via_im_core(resolved, manager, request)
    }?;
    if result.data.get("action").and_then(Value::as_str) != Some("recover_handle") {
        append_recover_identity_warning(&mut result, identity_changed);
        return Ok(result);
    }
    finalize_recovered_handle_result_via_im_core(resolved, manager, result, identity_changed)
}

pub fn merge_recovered_handle_local_state_via_im_core(
    paths: &crate::config::Paths,
    old_owner_dids: Vec<String>,
    new_owner_did: String,
    final_identity_name: String,
) -> Result<RecoverLocalStateMergeResult, store::StoreError> {
    let (store_merge_counts, e2ee_cleanup_counts) = store::merge_recovered_handle_local_state(
        paths,
        &old_owner_dids,
        &new_owner_did,
        &final_identity_name,
    )?;
    Ok(RecoverLocalStateMergeResult {
        store_merge_counts,
        e2ee_cleanup_counts,
    })
}

fn finalize_recovered_handle_result_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    mut result: identity::CommandResult,
    identity_changed: bool,
) -> Result<identity::CommandResult, ExitError> {
    let final_identity_name = string_from_data(&result.data, "final_identity_name");
    let temp_identity_name = string_from_data(&result.data, "temp_identity_name");
    let backup_path = string_from_data(&result.data, "backup_path");
    let active_before = string_from_data(&result.data, "active_before");
    let old_dids = string_slice_from_data(&result.data, "old_dids");
    let archived_identities = string_slice_from_data(&result.data, "archived_identities");
    let new_did = result
        .data
        .get("identity")
        .and_then(|value| value.get("did"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let merge_result = merge_recovered_handle_local_state_via_im_core(
        &resolved.paths,
        old_dids,
        new_did.clone(),
        final_identity_name.clone(),
    )
    .map_err(|err| recover_store_exit(err, &backup_path, &temp_identity_name, &new_did))?;
    let (store_merge_counts, e2ee_cleanup_counts) = (
        merge_result.store_merge_counts,
        merge_result.e2ee_cleanup_counts,
    );

    let promoted = identity::finalize_recovered_handle(
        manager,
        identity::RecoverFinalizeRequest {
            final_identity_name: &final_identity_name,
            temp_identity_name: &temp_identity_name,
            archived_identity_names: &archived_identities,
            active_before: &active_before,
            backup_path: &backup_path,
            new_did: &new_did,
            config_paths: Some(&resolved.paths),
        },
    )
    .map_err(recover_finalize_exit)?;

    if let Some(object) = result.data.as_object_mut() {
        let identity = recovered_identity_value(manager, &promoted.identity);
        object.insert("identity".to_string(), identity);
        object.insert(
            "store_merge_counts".to_string(),
            serde_json::to_value(store_merge_counts).unwrap_or_else(|_| json!({})),
        );
        object.insert(
            "e2ee_cleanup_counts".to_string(),
            serde_json::to_value(e2ee_cleanup_counts).unwrap_or_else(|_| json!({})),
        );
        object.remove("temp_identity_name");
        object.remove("active_before");
        object.remove("old_dids");
    }
    if !archived_identities.is_empty() {
        result.warnings.push(format!(
            "Archived {} same-handle local identities; they were removed from the live index, while their original directories and the recover backup were kept.",
            archived_identities.len()
        ));
    }
    append_recover_identity_warning(&mut result, identity_changed);
    Ok(result)
}

fn recovered_identity_value(manager: &identity::Manager, promoted: &StoredIdentity) -> Value {
    manager
        .list()
        .ok()
        .and_then(|items| {
            items
                .into_iter()
                .find(|summary| summary.identity_name == promoted.identity_name)
        })
        .map(|summary| serde_json::to_value(summary).unwrap_or_else(|_| json!({})))
        .unwrap_or_else(|| {
            json!({
                "identity_name": promoted.identity_name,
                "did": promoted.did,
                "handle": promoted.handle,
                "full_handle": promoted.full_handle,
                "created_at": promoted.created_at,
            })
        })
}

fn append_recover_identity_warning(result: &mut identity::CommandResult, identity_changed: bool) {
    if identity_changed {
        result
            .warnings
            .push(identity::recover_identity_ignored_warning().to_string());
    }
}

fn recover_store_exit(
    err: store::StoreError,
    backup_path: &str,
    temp_identity_name: &str,
    new_did: &str,
) -> ExitError {
    let mut exit = recover_store_base_exit(err);
    exit.detail.code = "internal_error".to_string();
    exit.exit_code = 1;
    exit.detail.message = format!(
        "merge recovered handle local state: {}",
        exit.detail.message
    );
    exit.detail.details = recover_error_details(backup_path, temp_identity_name, new_did);
    exit
}

fn recover_store_base_exit(err: store::StoreError) -> ExitError {
    match err {
        store::StoreError::LegacyDatabaseNotFound | store::StoreError::NotFound(_) => {
            ExitError::new("not_found", 5, err.to_string(), RECOVER_REPAIR_HINT)
        }
        store::StoreError::UnsafeSql(_) | store::StoreError::UnsupportedLegacySchema(_) => {
            ExitError::new("invalid_argument", 2, err.to_string(), RECOVER_REPAIR_HINT)
        }
        store::StoreError::Invalid(_) | store::StoreError::Sqlite(_) | store::StoreError::Io(_) => {
            ExitError::new("internal_error", 1, err.to_string(), RECOVER_REPAIR_HINT)
        }
    }
}

fn recover_finalize_exit(err: identity::RecoverFinalizeError) -> ExitError {
    let details = recover_error_details(&err.backup_path, &err.temp_identity_name, &err.new_did);
    let mut exit = ExitError::new("internal_error", 1, err.to_string(), RECOVER_REPAIR_HINT);
    exit.detail.details = details;
    exit
}

fn recover_error_details(backup_path: &str, temp_identity_name: &str, new_did: &str) -> Value {
    json!({
        "backup_path": backup_path,
        "temp_identity_name": temp_identity_name,
        "new_did": new_did,
    })
}

fn string_from_data(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_slice_from_data(data: &Value, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn bind_email_wait_via_im_core(
    client: &im_core::ImClient,
    identity: &identity::IdentitySummary,
    request: BindContactCommandRequest,
) -> Result<identity::CommandResult, ExitError> {
    let email = match &request.sdk.method {
        ContactBindingMethod::Email { email } => email.clone(),
        _ => unreachable!("wait request is only used for email"),
    };
    let mut result = client
        .identity()
        .bind_contact(request.sdk.clone())
        .map_err(|err| super::map_im_error(err, "id bind"))?;
    if result.state == ContactBindingState::Completed {
        return bind_command_result(identity, result).map_err(crate::app::identity_exit);
    }
    if result.state != ContactBindingState::Pending {
        return bind_command_result(identity, result).map_err(crate::app::identity_exit);
    }

    let wait_result = wait_for_email_verification_via_im_core(
        client,
        &email,
        request.verification_timeout,
        request.poll_interval_seconds,
    )?;
    result = wait_result;
    bind_command_result(identity, result).map_err(crate::app::identity_exit)
}

fn wait_for_email_verification_via_im_core(
    client: &im_core::ImClient,
    email: &str,
    timeout_secs: i64,
    poll_interval_secs: f64,
) -> Result<ContactBindingResult, ExitError> {
    let timeout_secs = if timeout_secs <= 0 { 300 } else { timeout_secs };
    let poll_interval_secs = if poll_interval_secs <= 0.0 {
        5.0
    } else {
        poll_interval_secs
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
    loop {
        let result = client
            .identity()
            .bind_email_status(email.to_string())
            .map_err(|err| super::map_im_error(err, "id bind"))?;
        if result.state == ContactBindingState::Completed || std::time::Instant::now() >= deadline {
            return Ok(result);
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(poll_interval_secs));
    }
}

fn bind_command_result(
    identity: &identity::IdentitySummary,
    result: ContactBindingResult,
) -> Result<identity::CommandResult, identity::IdentityError> {
    match result.state {
        ContactBindingState::OtpSent => identity::wire::bind_phone_otp_result(
            identity,
            &result.target,
            identity_diagnostic_raw(&result),
        ),
        ContactBindingState::Completed
            if matches!(result.method, ContactBindingMethodKind::Phone) =>
        {
            identity::wire::bind_phone_completed_result(
                identity,
                &result.target,
                identity_diagnostic_raw(&result),
            )
        }
        ContactBindingState::EmailSent => Ok(identity::wire::bind_email_sent_result(
            identity,
            &result.target,
            identity_diagnostic_raw(&result),
        )),
        ContactBindingState::Pending => Ok(identity::wire::bind_email_pending_result(
            identity,
            &result.target,
        )),
        ContactBindingState::Completed => Ok(identity::wire::bind_email_completed_result(
            identity,
            &result.target,
        )),
    }
}

pub fn get_profile_request(command: &ParsedCommand) -> GetProfileBridgeRequest {
    GetProfileBridgeRequest {
        self_profile: command
            .flags
            .get("self")
            .is_some_and(|value| value == "true"),
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn get_self_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let profile = client
        .identity()
        .profile()
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_self_result(legacy_profile_value(
        &profile,
    )))
}

pub fn get_public_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: GetProfileBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let Some(client) =
        build_optional_directory_client(resolved, manager, identity_flag, "id profile get")?
    else {
        return identity::get_profile(
            resolved,
            manager,
            identity::GetProfileParams {
                self_profile: request.self_profile,
                handle: request.handle,
                did: request.did,
            },
        )
        .map_err(crate::app::identity_exit);
    };
    let mut subject = serde_json::Map::new();
    let profile_did = request.did.trim().to_string();
    if !request.handle.trim().is_empty() {
        let target = identity::normalize_handle_input(&request.handle, &resolved.did_domain)
            .map_err(crate::app::identity_exit)?;
        let handle = Handle::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let result = client
            .directory()
            .public_profile(IdentitySubject::Handle(handle))
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let did = result.did.as_str().to_string();
        subject.insert("handle".to_string(), Value::String(target.local_part));
        subject.insert("full_handle".to_string(), Value::String(target.full_handle));
        subject.insert("domain".to_string(), Value::String(target.effective_domain));
        subject.insert("did".to_string(), Value::String(did));
        return Ok(identity::wire::profile_public_result(
            Value::Object(subject),
            legacy_profile_value(&result.profile),
        ));
    }
    if !profile_did.trim().is_empty() {
        subject.insert("did".to_string(), Value::String(profile_did.clone()));
    }
    let did = Did::parse(&profile_did).map_err(|err| super::map_im_error(err, "id profile get"))?;
    let result = client
        .directory()
        .public_profile(IdentitySubject::Did(did))
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_public_result(
        Value::Object(subject),
        legacy_profile_value(&result.profile),
    ))
}

pub fn resolve_request(command: &ParsedCommand) -> ResolveBridgeRequest {
    ResolveBridgeRequest {
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn resolve_identity_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: ResolveBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let handle = request.handle.trim();
    let did = request.did.trim();
    if (handle.is_empty() && did.is_empty()) || (!handle.is_empty() && !did.is_empty()) {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: exactly one of handle or did is required",
            "Pass either --handle <handle> or --did <did>.",
        ));
    }
    let Some(client) =
        build_optional_directory_client(resolved, manager, identity_flag, "id resolve")?
    else {
        return identity::resolve_identity(
            resolved,
            identity::ResolveParams {
                handle: request.handle,
                did: request.did,
            },
        )
        .map_err(crate::app::identity_exit);
    };
    let peer = if !handle.is_empty() {
        let target = identity::normalize_handle_input(handle, &resolved.did_domain)
            .map_err(crate::app::identity_exit)?;
        PeerRef::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id resolve"))?
    } else {
        PeerRef::parse(did, "").map_err(|err| super::map_im_error(err, "id resolve"))?
    };
    let result = client
        .directory()
        .resolve_peer(peer)
        .map_err(|err| super::map_im_error(err, "id resolve"))?;
    Ok(resolve_command_result_from_sdk(result))
}

fn build_optional_directory_client(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    context: &'static str,
) -> Result<Option<im_core::ImClient>, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    match core.client(cli_identity_selector(identity_flag)) {
        Ok(client) => Ok(Some(client)),
        Err(im_core::ImError::DefaultIdentityMissing)
        | Err(im_core::ImError::IdentityRequired)
        | Err(im_core::ImError::IdentityNotFound { .. }) => Ok(None),
        Err(err) => Err(super::map_im_error(err, context)),
    }
}

pub fn set_profile_request(
    display_name: String,
    bio: String,
    tags_csv: String,
    markdown: String,
    markdown_file: String,
) -> Result<SetProfileCommandRequest, ExitError> {
    let patch =
        profile_patch_from_command(&display_name, &bio, &tags_csv, &markdown, &markdown_file)?;
    Ok(SetProfileCommandRequest {
        patch,
        display_name,
    })
}

pub fn set_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: SetProfileCommandRequest,
) -> Result<identity::CommandResult, ExitError> {
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let identity = identity::store::identity_summary_from_record(&record);
    let changed_fields = changed_fields_from_profile_patch(&request.patch);
    let profile = client
        .identity()
        .update_profile(request.patch)
        .map_err(|err| super::map_im_error(err, "id profile set"))?;
    let display_name = request.display_name.trim();
    if !display_name.is_empty() {
        let _ = manager.update_display_name(&record.identity_name, display_name);
    }
    Ok(identity::wire::profile_update_result(
        &identity,
        changed_fields,
        legacy_profile_value(&profile),
    ))
}

fn profile_patch_from_command(
    display_name: &str,
    bio: &str,
    tags_csv: &str,
    markdown: &str,
    markdown_file: &str,
) -> Result<ProfilePatch, ExitError> {
    let markdown_file = markdown_file.trim();
    let markdown = if markdown_file.is_empty() {
        trimmed_optional(markdown)
    } else {
        let raw = std::fs::read(markdown_file).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("read markdown file {markdown_file:?}: {err}"),
                "Check the --markdown-file path and permissions.",
            )
        })?;
        let markdown = String::from_utf8_lossy(&raw).into_owned();
        (!markdown.trim().is_empty()).then_some(markdown)
    };
    Ok(ProfilePatch {
        display_name: trimmed_optional(display_name),
        bio: trimmed_optional(bio),
        tags: tags_patch(tags_csv),
        markdown,
    })
}

fn tags_patch(tags_csv: &str) -> Option<Vec<String>> {
    let tags = tags_csv
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!tags.is_empty()).then_some(tags)
}

fn changed_fields_from_profile_patch(patch: &ProfilePatch) -> Vec<String> {
    let mut fields = Vec::new();
    if patch.display_name.is_some() {
        fields.push("display_name".to_string());
    }
    if patch.bio.is_some() {
        fields.push("bio".to_string());
    }
    if patch.tags.is_some() {
        fields.push("tags".to_string());
    }
    if patch.markdown.is_some() {
        fields.push("profile_md".to_string());
    }
    fields
}

fn legacy_profile_value(profile: &im_core::identity::Profile) -> Value {
    let mut value = Map::new();
    value.insert(
        "did".to_string(),
        Value::String(profile.subject.as_str().to_string()),
    );
    if let Some(handle) = profile.handle.as_ref() {
        value.insert(
            "handle".to_string(),
            Value::String(handle.as_str().to_string()),
        );
    }
    if let Some(display_name) = profile.display_name.as_ref() {
        value.insert("nick_name".to_string(), Value::String(display_name.clone()));
    }
    if let Some(bio) = profile.bio.as_ref() {
        value.insert("bio".to_string(), Value::String(bio.clone()));
    }
    if !profile.tags.is_empty() {
        value.insert("tags".to_string(), json!(profile.tags));
    }
    if let Some(markdown) = profile.markdown.as_ref() {
        value.insert("profile_md".to_string(), Value::String(markdown.clone()));
    }
    if let Some(avatar_url) = profile.avatar_url.as_ref() {
        value.insert("avatar_url".to_string(), Value::String(avatar_url.clone()));
    }
    if let Some(updated_at) = profile.updated_at.as_ref() {
        value.insert("updated_at".to_string(), Value::String(updated_at.clone()));
    }
    if !profile.metadata.is_empty() {
        value.insert(
            "metadata".to_string(),
            Value::Object(
                profile
                    .metadata
                    .iter()
                    .map(|attribute| {
                        (
                            attribute.key.clone(),
                            Value::String(attribute.value.clone()),
                        )
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(value)
}

fn resolve_command_result_from_sdk(resolution: DirectoryResolution) -> identity::CommandResult {
    let resolve = Some(json!({ "did": resolution.did.as_str() }));
    let lookup = resolution.handle.as_ref().map(|handle| {
        json!({
            "did": resolution.did.as_str(),
            "handle": handle.as_str(),
            "full_handle": handle.as_str(),
        })
    });
    let public_profile = resolution.profile.as_ref().map(legacy_profile_value);
    identity::wire::resolve_result(resolve, lookup, public_profile, resolution.warnings)
}

fn cli_identity_summaries_from_sdk(
    identities: &[im_core::IdentitySummary],
    manager: &identity::Manager,
) -> Result<Vec<identity::IdentitySummary>, ExitError> {
    identities
        .iter()
        .map(|summary| cli_identity_summary_from_sdk_with_manager(summary, manager))
        .collect()
}

fn cli_identity_summary_from_sdk_with_manager(
    summary: &im_core::IdentitySummary,
    manager: &identity::Manager,
) -> Result<identity::IdentitySummary, ExitError> {
    let identity_name = sdk_identity_name(summary);
    match manager.load(&identity_name) {
        Ok(record) => {
            let mut cli_summary = identity::store::identity_summary_from_record(&record);
            cli_summary.is_default = summary.is_default;
            Ok(cli_summary)
        }
        Err(identity::IdentityError::NotFound(_)) => Ok(cli_identity_summary_from_sdk(
            summary,
            &manager.list().unwrap_or_default(),
        )),
        Err(err) => Err(crate::app::identity_exit(err)),
    }
}

fn cli_identity_summary_from_sdk(
    summary: &im_core::IdentitySummary,
    known: &[identity::IdentitySummary],
) -> identity::IdentitySummary {
    let identity_name = sdk_identity_name(summary);
    if let Some(existing) = known
        .iter()
        .find(|identity| identity.identity_name == identity_name)
    {
        let mut existing = existing.clone();
        existing.is_default = summary.is_default;
        return existing;
    }
    let full_handle = summary
        .handle
        .as_ref()
        .map(|handle| handle.as_str().to_string())
        .unwrap_or_default();
    let handle = full_handle
        .split_once('.')
        .map(|(local, _)| local)
        .unwrap_or(full_handle.as_str())
        .to_string();
    let user_state = sdk_user_state(summary);
    identity::IdentitySummary {
        identity_name,
        did: summary.did.as_str().to_string(),
        unique_id: summary.id.as_str().to_string(),
        display_name: summary.display_name.clone().unwrap_or_default(),
        handle,
        full_handle,
        created_at: String::new(),
        dir_name: summary.id.as_str().to_string(),
        is_default: summary.is_default,
        has_jwt: summary
            .readiness
            .missing
            .iter()
            .all(|item| !matches!(item, im_core::identity::IdentityMissingItem::AuthState)),
        has_did_document: summary
            .readiness
            .missing
            .iter()
            .all(|item| !matches!(item, im_core::identity::IdentityMissingItem::DidDocument)),
        has_key1_private: summary
            .readiness
            .missing
            .iter()
            .all(|item| !matches!(item, im_core::identity::IdentityMissingItem::PrivateKey)),
        has_key1_public: summary.readiness.ready_for_auth,
        has_e2ee_signing_private: summary.readiness.ready_for_messaging,
        has_e2ee_agreement_private: summary.readiness.ready_for_messaging,
        user_state,
        user_id: String::new(),
    }
}

fn sdk_identity_name(summary: &im_core::IdentitySummary) -> String {
    summary
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.id.as_str())
        .to_string()
}

struct RegisterPlanTarget {
    local_part: String,
    full_handle: Handle,
    effective_domain: String,
    explicit_domain: bool,
}

fn register_handle_plan_command_result(
    manager: &identity::Manager,
    did_domain: &str,
    request: RegisterHandleRequest,
) -> Result<identity::CommandResult, ExitError> {
    let target = register_plan_target(request.requested_handle.as_str(), did_domain)?;
    let existing = manager.list().unwrap_or_default();
    let alias_base = if target.explicit_domain {
        target.full_handle.as_str()
    } else {
        target.local_part.as_str()
    };
    let identity_name = identity::store::choose_named_identity(
        &request.local_alias.unwrap_or_default(),
        &existing,
        alias_base,
    );
    let (action, remote_calls) = register_plan_action_and_calls(&request.verification);
    Ok(identity::CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "identity_name": identity_name,
                "handle": target.local_part,
                "full_handle": target.full_handle.as_str(),
                "did_domain": target.effective_domain,
                "phone": registration_phone(&request.verification).unwrap_or_default(),
                "email": registration_email(&request.verification).unwrap_or_default(),
                "remote_calls": remote_calls,
            }
        }),
        summary: "Dry run: handle registration flow planned".to_string(),
        warnings: Vec::new(),
    })
}

fn register_plan_target(raw: &str, did_domain: &str) -> Result<RegisterPlanTarget, ExitError> {
    let trimmed = raw.trim().trim_start_matches('@').to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "id register requires --handle.",
            "Use a non-empty handle local part or full handle.",
        ));
    }
    let handle = trimmed.strip_prefix("wba://").unwrap_or(&trimmed);
    let (local_part, effective_domain, explicit_domain) = if let Some(dot) = handle.find('.') {
        (
            handle[..dot].trim().to_string(),
            handle[dot + 1..].trim().trim_end_matches('.').to_string(),
            true,
        )
    } else {
        (
            handle.to_string(),
            did_domain.trim().trim_end_matches('.').to_ascii_lowercase(),
            false,
        )
    };
    if local_part.is_empty() || effective_domain.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "id register requires a handle local part and domain.",
            "Use --handle <local> with configured did_domain, or --handle <local.domain>.",
        ));
    }
    let full_handle = Handle::parse(format!("{local_part}.{effective_domain}"), "")
        .map_err(|err| super::map_im_error(err, "id register"))?;
    Ok(RegisterPlanTarget {
        local_part,
        full_handle,
        effective_domain,
        explicit_domain,
    })
}

fn register_plan_action_and_calls(
    verification: &VerificationInput,
) -> (&'static str, Vec<&'static str>) {
    match verification {
        VerificationInput::Phone { otp, .. } if otp.as_deref().unwrap_or_default().is_empty() => {
            ("send_handle_otp", vec!["handle.send_otp"])
        }
        VerificationInput::Email {
            wait_for_verification: false,
            ..
        } => (
            "send_registration_email",
            vec!["POST /user-service/auth/email-send"],
        ),
        VerificationInput::Email {
            wait_for_verification: true,
            ..
        } => (
            "register_handle",
            vec![
                "GET /user-service/auth/email-status",
                "POST /user-service/auth/email-send",
                "did-auth.register",
            ],
        ),
        _ => ("register_handle", vec!["did-auth.register"]),
    }
}

fn bind_contact_plan_command_result(request: &ContactBindingRequest) -> identity::CommandResult {
    let (action, phone, email, remote_calls) = match &request.method {
        ContactBindingMethod::Phone { phone, otp }
            if otp.as_deref().unwrap_or_default().is_empty() =>
        {
            (
                "send_bind_phone_otp",
                phone.as_str(),
                "",
                vec!["POST /user-service/auth/phone-bind-send"],
            )
        }
        ContactBindingMethod::Phone { phone, .. } => (
            "bind_phone",
            phone.as_str(),
            "",
            vec!["POST /user-service/auth/phone-bind-verify"],
        ),
        ContactBindingMethod::Email { email } if !request.wait_for_email_verification => (
            "send_bind_email",
            "",
            email.as_str(),
            vec!["POST /user-service/auth/email-send"],
        ),
        ContactBindingMethod::Email { email } => (
            "bind_email",
            "",
            email.as_str(),
            vec![
                "GET /user-service/auth/email-status",
                "POST /user-service/auth/email-send",
            ],
        ),
    };
    identity::CommandResult {
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

fn registration_phone(verification: &VerificationInput) -> Option<&str> {
    match verification {
        VerificationInput::Phone { phone, .. } => Some(phone.as_str()),
        _ => None,
    }
}

fn registration_email(verification: &VerificationInput) -> Option<&str> {
    match verification {
        VerificationInput::Email { email, .. } => Some(email.as_str()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct RecoverHandlePlan {
    target_handle: String,
    target_local_part: String,
    effective_domain: String,
    final_identity_name: String,
    temp_identity_name: String,
    backup_path_preview: String,
    same_handle_candidates: Vec<identity::IdentitySummary>,
    excluded_identities: Vec<identity::IdentitySummary>,
}

#[derive(Debug, Clone)]
struct RecoverHandleBackup {
    backup_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct RecoverHandleBackupManifest {
    reason: String,
    created_at: String,
    handle: String,
    archived_identity_names: Vec<String>,
    archived_dids: Vec<String>,
    archived_dir_names: Vec<String>,
    default_before: String,
    active_before: String,
    planned_final_identity: String,
    planned_temp_identity: String,
}

impl RecoverHandlePlan {
    fn archived_identity_names(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .map(|summary| summary.identity_name.clone())
            .collect()
    }

    fn archived_dids(&self) -> Vec<String> {
        self.same_handle_candidates
            .iter()
            .filter_map(|summary| {
                let did = summary.did.trim();
                (!did.is_empty()).then(|| did.to_string())
            })
            .collect()
    }

    fn old_owner_dids_in_merge_order(&self) -> Vec<String> {
        let mut dids = Vec::with_capacity(self.same_handle_candidates.len());
        for summary in &self.same_handle_candidates {
            let did = summary.did.trim();
            if did.is_empty() || dids.iter().any(|seen| seen == did) {
                continue;
            }
            dids.push(did.to_string());
        }
        dids
    }
}

fn recover_handle_plan_command_result(
    manager: &identity::Manager,
    did_domain: &str,
    request: &RecoverHandleCommandRequest,
) -> Result<identity::CommandResult, ExitError> {
    let plan = recover_handle_plan(manager, did_domain, request)?;
    let (action, remote_calls, local_writes, backup_path) = if request.otp.trim().is_empty() {
        (
            "send_recover_otp",
            json!(["handle.send_otp"]),
            Value::Null,
            String::new(),
        )
    } else {
        (
            "recover_handle",
            json!(["did-auth.recover_handle"]),
            json!([
                ".legacy-backup/recover-handle",
                "index.json",
                "config.yaml",
                "identity.json",
                "auth.json",
                "did_document.json",
                "key-1-private.pem",
                "key-1-public.pem",
                "e2ee-signing-private.pem",
                "e2ee-agreement-private.pem",
                "sqlite.recover_handle_merge",
                "sqlite.e2ee_cleanup",
            ]),
            plan.backup_path_preview.clone(),
        )
    };
    Ok(identity::CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "target_handle": plan.target_handle,
                "identity_name": plan.final_identity_name,
                "final_identity_name": plan.final_identity_name,
                "temp_identity_name": plan.temp_identity_name,
                "same_handle_candidates": plan.same_handle_candidates,
                "excluded_identities": plan.excluded_identities,
                "backup_path": backup_path,
                "phone": request.phone,
                "remote_calls": remote_calls,
                "local_writes": local_writes,
            }
        }),
        summary: "Dry run: handle recovery planned".to_string(),
        warnings: Vec::new(),
    })
}

fn recover_handle_plan(
    manager: &identity::Manager,
    did_domain: &str,
    request: &RecoverHandleCommandRequest,
) -> Result<RecoverHandlePlan, ExitError> {
    let target = identity::normalize_handle_input(&request.handle, did_domain)
        .map_err(crate::app::identity_exit)?;
    let existing = manager.list().map_err(crate::app::identity_exit)?;
    let identity_base = if target.explicit_domain {
        target.full_handle.clone()
    } else {
        target.local_part.clone()
    };
    let final_identity_name = identity::layout::sanitize_identity_name(&identity_base);
    if final_identity_name.is_empty() {
        return Err(crate::app::identity_exit(
            identity::IdentityError::InvalidInput(format!(
                "invalid input: handle {:?} cannot be used as an identity name",
                request.handle
            )),
        ));
    }

    let handle_key = canonical_handle(&target.full_handle);
    let mut same_handle_candidates = Vec::new();
    let mut excluded_identities = Vec::new();
    for summary in existing {
        let full_handle = identity::default_handle_string(
            &summary.full_handle,
            &identity::derive_full_handle_from_did(&summary.handle, &summary.did),
        );
        if canonical_handle(&full_handle) == handle_key {
            same_handle_candidates.push(summary);
        } else {
            excluded_identities.push(summary);
        }
    }
    same_handle_candidates.sort_by(|left, right| {
        match compare_rfc3339(&left.created_at, &right.created_at) {
            Ordering::Equal => left.identity_name.cmp(&right.identity_name),
            ordering => ordering,
        }
    });
    for summary in &excluded_identities {
        if summary.identity_name == final_identity_name {
            return Err(crate::app::identity_exit(
                identity::IdentityError::Conflict(format!(
                    "identity conflict: identity name {final_identity_name} is already used by another handle"
                )),
            ));
        }
    }

    let temp_base = format!("{final_identity_name}-recover-tmp");
    let mut all_existing =
        Vec::with_capacity(same_handle_candidates.len() + excluded_identities.len());
    all_existing.extend_from_slice(&same_handle_candidates);
    all_existing.extend_from_slice(&excluded_identities);
    let temp_identity_name =
        identity::store::choose_named_identity(&temp_base, &all_existing, &temp_base);
    let backup_path_preview = recover_backup_path_preview(manager, &target.full_handle);

    Ok(RecoverHandlePlan {
        target_handle: target.full_handle,
        target_local_part: target.local_part,
        effective_domain: target.effective_domain,
        final_identity_name,
        temp_identity_name,
        backup_path_preview,
        same_handle_candidates,
        excluded_identities,
    })
}

fn recover_backup_path_preview(manager: &identity::Manager, handle: &str) -> String {
    recover_backup_root(manager)
        .join("recover-handle")
        .join(recover_backup_preview_name(handle))
        .to_string_lossy()
        .into_owned()
}

fn recover_active_before(config_file: &str) -> Result<String, ExitError> {
    if config_file.trim().is_empty() {
        return Ok(String::new());
    }
    let (file_config, _, error) = config::read_file_config(config_file);
    if !error.is_empty() {
        return Err(crate::app::identity_exit(
            identity::IdentityError::Internal(format!(
                "read config before handle recover: {error}"
            )),
        ));
    }
    Ok(file_config.identity.active.trim().to_string())
}

fn recover_handle_backup(
    manager: &identity::Manager,
    plan: &RecoverHandlePlan,
    active_before: &str,
    config_file: &str,
) -> Result<RecoverHandleBackup, ExitError> {
    if plan.target_handle.trim().is_empty() {
        return Err(crate::app::identity_exit(
            identity::IdentityError::InvalidInput("invalid input: handle is required".to_string()),
        ));
    }
    manager.ensure_root().map_err(crate::app::identity_exit)?;
    let index = manager.load_index().map_err(crate::app::identity_exit)?;
    let created_at = OffsetDateTime::now_utc();
    let backup_root = recover_backup_root(manager).join("recover-handle");
    let backup_dir = recover_unique_backup_dir(
        backup_root.join(recover_backup_dir_name(created_at, &plan.target_handle)),
    );
    identity::layout::ensure_dir(&backup_dir).map_err(|err| {
        crate::app::identity_exit(identity::IdentityError::Internal(format!(
            "create recover backup directory: {err}"
        )))
    })?;

    identity::layout::write_secure_json(
        &backup_dir.join("index.before.json").to_string_lossy(),
        &index,
    )
    .map_err(|err| {
        crate::app::identity_exit(identity::IdentityError::Internal(format!(
            "write recover backup index snapshot: {err}"
        )))
    })?;
    let config_file = config_file.trim();
    if !config_file.is_empty() {
        match fs::read(config_file) {
            Ok(raw) => {
                identity::layout::write_secure_text(
                    &backup_dir.join("config.before.yaml").to_string_lossy(),
                    &String::from_utf8_lossy(&raw),
                )
                .map_err(|err| {
                    crate::app::identity_exit(identity::IdentityError::Internal(format!(
                        "write recover backup config snapshot: {err}"
                    )))
                })?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(crate::app::identity_exit(
                    identity::IdentityError::Internal(format!(
                        "read config before recover backup: {err}"
                    )),
                ));
            }
        }
    }

    let mut archived_identity_names = Vec::with_capacity(plan.same_handle_candidates.len());
    let mut archived_dids = Vec::with_capacity(plan.same_handle_candidates.len());
    let mut archived_dir_names = Vec::with_capacity(plan.same_handle_candidates.len());
    for (idx, summary) in plan.same_handle_candidates.iter().enumerate() {
        archived_identity_names.push(summary.identity_name.clone());
        archived_dids.push(summary.did.clone());
        archived_dir_names.push(summary.dir_name.clone());
        let source = PathBuf::from(manager.build_paths(&summary.dir_name).identity_dir);
        let metadata = match fs::metadata(&source) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(crate::app::identity_exit(
                    identity::IdentityError::Internal(format!(
                        "stat identity directory before recover backup: {err}"
                    )),
                ));
            }
        };
        if !metadata.is_dir() {
            return Err(crate::app::identity_exit(
                identity::IdentityError::InvalidInput(format!(
                    "invalid input: identity path is not a directory: {}",
                    source.to_string_lossy()
                )),
            ));
        }
        let mut target_name = format!(
            "{:02}-{}",
            idx + 1,
            identity::layout::sanitize_component(&summary.identity_name)
        );
        if target_name.trim().is_empty() {
            target_name = format!(
                "{:02}-{}",
                idx + 1,
                identity::layout::sanitize_component(&summary.dir_name)
            );
        }
        identity::layout::copy_dir(&source, &backup_dir.join("identities").join(target_name))
            .map_err(|err| {
                crate::app::identity_exit(identity::IdentityError::Internal(format!(
                    "backup identity directory before recover: {err}"
                )))
            })?;
    }

    let manifest = RecoverHandleBackupManifest {
        reason: "recover_handle".to_string(),
        created_at: created_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        handle: plan.target_handle.clone(),
        archived_identity_names,
        archived_dids,
        archived_dir_names,
        default_before: index.default_credential_name,
        active_before: active_before.trim().to_string(),
        planned_final_identity: plan.final_identity_name.clone(),
        planned_temp_identity: plan.temp_identity_name.clone(),
    };
    identity::layout::write_secure_json(
        &backup_dir.join("backup_manifest.json").to_string_lossy(),
        &manifest,
    )
    .map_err(|err| {
        crate::app::identity_exit(identity::IdentityError::Internal(format!(
            "write recover backup manifest: {err}"
        )))
    })?;
    Ok(RecoverHandleBackup {
        backup_path: backup_dir.to_string_lossy().into_owned(),
    })
}

fn recover_backup_root(manager: &identity::Manager) -> PathBuf {
    Path::new(manager.root_dir())
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(manager.root_dir()))
        .join(identity::types::LEGACY_BACKUP_DIR_NAME)
}

fn recover_backup_preview_name(handle: &str) -> String {
    let mut handle_part = identity::layout::sanitize_identity_name(handle);
    if handle_part.is_empty() {
        handle_part = "handle".to_string();
    }
    format!("<timestamp>-{handle_part}")
}

fn recover_backup_dir_name(created_at: OffsetDateTime, handle: &str) -> String {
    let mut handle_part = identity::layout::sanitize_identity_name(handle);
    if handle_part.is_empty() {
        handle_part = "handle".to_string();
    }
    let timestamp = created_at
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
        .replace(':', "-");
    format!("{timestamp}-{handle_part}")
}

fn recover_unique_backup_dir(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    for idx in 2..1000 {
        let candidate = PathBuf::from(format!("{}-{idx}", base.to_string_lossy()));
        if !candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(format!(
        "{}-{}",
        base.to_string_lossy(),
        OffsetDateTime::now_utc().unix_timestamp()
    ))
}

fn canonical_handle(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn compare_rfc3339(left: &str, right: &str) -> Ordering {
    let left = left.trim();
    let right = right.trim();
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    match (
        OffsetDateTime::parse(left, &Rfc3339),
        OffsetDateTime::parse(right, &Rfc3339),
    ) {
        (Ok(left_time), Ok(right_time)) => return left_time.cmp(&right_time),
        _ => {}
    }
    left.cmp(right)
}

fn pending_registration_identity_name(
    request: &RegisterHandleRequest,
    handle: &str,
    full_handle: &str,
) -> String {
    if let Some(identity_name) = request
        .local_alias
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return identity_name.to_string();
    }
    if request.requested_handle.as_str().trim().contains('.') {
        full_handle.to_string()
    } else {
        handle.to_string()
    }
}

fn normalize_registration_phone(phone: &str) -> Result<String, ExitError> {
    let phone = phone.trim();
    if is_international_phone(phone) {
        return Ok(phone.to_string());
    }
    if is_china_local_phone(phone) {
        return Ok(format!("+86{phone}"));
    }
    Err(super::map_im_error(
        im_core::ImError::invalid_input(
            Some("phone".to_string()),
            format!("invalid phone number {phone:?}"),
        ),
        "id register",
    ))
}

fn normalize_registration_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn is_international_phone(phone: &str) -> bool {
    let Some(rest) = phone.strip_prefix('+') else {
        return false;
    };
    let len = rest.len();
    (7..=17).contains(&len) && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_china_local_phone(phone: &str) -> bool {
    let bytes = phone.as_bytes();
    bytes.len() == 11 && bytes[0] == b'1' && bytes[1..].iter().all(|byte| byte.is_ascii_digit())
}

fn registration_action(state: HandleRegistrationState) -> &'static str {
    match state {
        HandleRegistrationState::OtpSent => "send_handle_otp",
        HandleRegistrationState::EmailSent => "send_registration_email",
        HandleRegistrationState::EmailPending => "wait_for_registration_email",
        HandleRegistrationState::Registered => "register_handle",
    }
}

fn registration_state_label(state: HandleRegistrationState) -> &'static str {
    match state {
        HandleRegistrationState::OtpSent => "otp_sent",
        HandleRegistrationState::EmailSent => "email_sent",
        HandleRegistrationState::EmailPending => "pending",
        HandleRegistrationState::Registered => "completed",
    }
}

fn registration_method_label(method: RegistrationMethod) -> &'static str {
    match method {
        RegistrationMethod::Phone => "phone",
        RegistrationMethod::Email => "email",
        RegistrationMethod::AlreadyVerified => "already_verified",
    }
}

fn registration_summary(state: HandleRegistrationState, full_handle: &str) -> String {
    match state {
        HandleRegistrationState::OtpSent => format!("OTP sent for handle {full_handle}"),
        HandleRegistrationState::EmailSent => {
            format!("Activation email sent for handle {full_handle}")
        }
        HandleRegistrationState::EmailPending => "Email verification is still pending".to_string(),
        HandleRegistrationState::Registered => {
            format!("Handle {full_handle} registered successfully")
        }
    }
}

fn sdk_user_state(summary: &im_core::IdentitySummary) -> identity::UserState {
    if !summary.readiness.ready_for_messaging {
        let missing = summary
            .readiness
            .missing
            .iter()
            .map(sdk_missing_item_label)
            .collect::<Vec<_>>();
        return identity::UserState {
            registration_state: if missing.len() <= 1 {
                "partial_user".to_string()
            } else {
                "local_identity".to_string()
            },
            ready_for_messaging: false,
            missing,
        };
    }
    identity::UserState {
        registration_state: "registered_user".to_string(),
        ready_for_messaging: true,
        missing: Vec::new(),
    }
}

fn sdk_missing_item_label(item: &im_core::identity::IdentityMissingItem) -> String {
    match item {
        im_core::identity::IdentityMissingItem::DidDocument => "did_document",
        im_core::identity::IdentityMissingItem::PrivateKey => "private_key",
        im_core::identity::IdentityMissingItem::AuthState => "auth",
        im_core::identity::IdentityMissingItem::Handle => "handle",
        im_core::identity::IdentityMissingItem::MessageEndpoint => "message_endpoint",
        im_core::identity::IdentityMissingItem::Other(value) => value.as_str(),
    }
    .to_string()
}

fn looks_like_handle(value: &str) -> bool {
    value.starts_with('@') || value.contains('.')
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn string_value(result: &Value, key: &str, fallback: &str) -> String {
    result
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
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
