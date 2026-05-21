use im_core::prelude::{
    AuthScope, ContactBindingMethod, ContactBindingRequest, ContactBindingState, Did, Handle,
    IdentitySelector, InitialProfile, PeerRef, ProfilePatch, RecoverGeneratedIdentity,
    RecoverHandleRequest, RegisterHandleRequest, SessionBundle, VerificationInput,
};
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::cli::ParsedCommand;
use crate::identity;
use crate::output::ExitError;
use crate::store;
use crate::transportcfg::Profile;

pub use super::identity_replace_did_plan::{
    replace_did_plan_bridge_request, replace_did_plan_via_im_core, replace_did_via_im_core,
    ReplaceDidPlanBridgeRequest,
};

#[derive(Debug, Clone)]
pub struct RegisterHandleBridgeRequest {
    pub sdk: RegisterHandleRequest,
    pub legacy: identity::RegisterParams,
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
pub struct SetProfileBridgeRequest {
    pub patch: ProfilePatch,
    pub legacy: identity::SetProfileParams,
}

#[derive(Debug, Clone)]
pub struct BindContactBridgeRequest {
    pub sdk: ContactBindingRequest,
    pub legacy: identity::BindParams,
}

#[derive(Debug, Clone)]
pub struct RecoverHandleBridgeRequest {
    pub sdk: RecoverHandleRequest,
    pub legacy: identity::RecoverParams,
}

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
    let otp = string_flag(command, "otp");
    Ok(RegisterHandleRequest {
        local_alias,
        requested_handle,
        verification: if otp.trim().is_empty() {
            VerificationInput::AlreadyVerified
        } else {
            VerificationInput::Otp {
                code: otp.trim().to_string(),
            }
        },
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

pub fn register_handle_bridge_request(
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<RegisterHandleBridgeRequest, ExitError> {
    let mut sdk_command = command.clone();
    sdk_command.globals.identity = identity_flag.to_string();
    let sdk = register_handle_request(&sdk_command)?;
    let legacy = identity::RegisterParams {
        identity_name: identity_flag.to_string(),
        handle: string_flag(command, "handle"),
        phone: string_flag(command, "phone"),
        email: string_flag(command, "email"),
        otp: string_flag(command, "otp"),
        invite_code: string_flag(command, "invite-code"),
        wait: command
            .flags
            .get("wait")
            .is_some_and(|value| value == "true"),
        verification_timeout: 300,
        poll_interval_seconds: 5.0,
    };
    Ok(RegisterHandleBridgeRequest { sdk, legacy })
}

pub fn register_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let _sdk_request = bridge.sdk;
    identity::register_plan(manager, did_domain, &bridge.legacy).map_err(crate::app::identity_exit)
}

pub fn register_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let core = super::build_im_core(resolved, manager)?;
    core.identities()
        .register_handle(bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id register"))?;
    identity::register(resolved, manager, bridge.legacy).map_err(crate::app::identity_exit)
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

pub fn bind_contact_bridge_request(
    command: &ParsedCommand,
) -> Result<BindContactBridgeRequest, ExitError> {
    let sdk = bind_contact_request(command)?;
    let legacy = identity::BindParams {
        phone: string_flag(command, "phone"),
        email: string_flag(command, "email"),
        otp: string_flag(command, "otp"),
        wait: command
            .flags
            .get("wait")
            .is_some_and(|value| value == "true"),
        verification_timeout: 300,
        poll_interval_seconds: 5.0,
    };
    Ok(BindContactBridgeRequest { sdk, legacy })
}

pub fn bind_contact_plan_via_im_core(
    command: &ParsedCommand,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = bind_contact_bridge_request(command)?;
    let _sdk_request = bridge.sdk;
    Ok(identity::bind_plan(&bridge.legacy))
}

pub fn bind_contact_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    command: &ParsedCommand,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = bind_contact_bridge_request(command)?;
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let identity = identity::store::identity_summary_from_record(&record);
    let result =
        if bridge.legacy.wait && matches!(bridge.sdk.method, ContactBindingMethod::Email { .. }) {
            bind_email_wait_via_im_core(resolved, manager, &client, &record, &identity, bridge)?
        } else {
            let result = im_core::compat::identity::bind_contact_with_bridge(
                &client,
                bridge.sdk,
                IdentitySessionProvider {
                    subject: client.did().clone(),
                    resolved,
                    manager,
                    record: record.clone(),
                },
                IdentityLegacyRestTransport {
                    resolved,
                    manager,
                    record,
                },
            )
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

pub fn recover_handle_bridge_request(
    params: identity::RecoverParams,
    generated_identity: Option<RecoverGeneratedIdentity>,
    default_domain: &str,
) -> Result<RecoverHandleBridgeRequest, ExitError> {
    let sdk = recover_handle_request(
        params.handle.clone(),
        params.phone.clone(),
        trimmed_optional(&params.otp),
        generated_identity,
        default_domain,
    )?;
    Ok(RecoverHandleBridgeRequest {
        sdk,
        legacy: params,
    })
}

pub fn recover_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    params: identity::RecoverParams,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = recover_handle_bridge_request(params, None, did_domain)?;
    let _sdk_request = bridge.sdk;
    identity::recover_preview(manager, did_domain, bridge.legacy).map_err(crate::app::identity_exit)
}

pub fn recover_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    params: identity::RecoverParams,
) -> Result<identity::CommandResult, ExitError> {
    let phone = params.phone.trim().to_string();
    let otp = params.otp.trim().to_string();
    if params.handle.trim().is_empty() || phone.is_empty() {
        return Err(crate::app::identity_exit(
            identity::IdentityError::InvalidInput(
                "invalid input: handle and phone are required".to_string(),
            ),
        ));
    }

    let plan = identity::recover::build_recover_plan(manager, &resolved.did_domain, &params)
        .map_err(crate::app::identity_exit)?;

    if otp.is_empty() {
        let bridge = recover_handle_bridge_request(params, None, &resolved.did_domain)?;
        let result = im_core::compat::identity::recover_handle_with_bridge(
            bridge.sdk,
            IdentityRecoveryRpcTransport { resolved },
        )
        .map_err(|err| super::map_im_error(err, "id recover"))?;
        return identity::wire::recover_otp_result(
            &plan.final_identity_name,
            &plan.target_local_part,
            &plan.target_handle,
            &phone,
            result.raw,
        )
        .map_err(crate::app::identity_exit);
    }

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
    let bridge = recover_handle_bridge_request(params, Some(sdk_generated), &resolved.did_domain)?;
    let active_before = identity::recover::recover_active_before(&resolved.paths.config_file)
        .map_err(crate::app::identity_exit)?;
    let backup = manager
        .backup_identities_for_handle_recovery(identity::recover::RecoverBackupRequest {
            handle: &plan.target_handle,
            candidates: &plan.same_handle_candidates,
            planned_final_identity_name: &plan.final_identity_name,
            planned_temp_identity_name: &plan.temp_identity_name,
            active_before: &active_before,
            config_file: Some(&resolved.paths.config_file),
        })
        .map_err(crate::app::identity_exit)?;

    let bridge_result = im_core::compat::identity::recover_handle_with_bridge(
        bridge.sdk,
        IdentityRecoveryRpcTransport { resolved },
    )
    .map_err(|err| super::map_im_error(err, "id recover"))?;
    let raw = bridge_result.raw;
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

pub fn merge_recovered_handle_local_state_via_im_core(
    paths: &crate::config::Paths,
    old_owner_dids: Vec<String>,
    new_owner_did: String,
    final_identity_name: String,
) -> Result<im_core::compat::identity::RecoverLocalStateMergeResult, store::StoreError> {
    im_core::compat::identity::merge_recovered_handle_local_state_with_bridge(
        im_core::compat::identity::RecoverLocalStateMergeRequest {
            old_owner_dids,
            new_owner_did,
            final_identity_name,
        },
        RecoverLocalStateStore { paths },
    )
    .map_err(store_error_from_im_error)
}

fn bind_email_wait_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    client: &im_core::ImClient,
    record: &identity::types::StoredIdentity,
    identity: &identity::IdentitySummary,
    bridge: BindContactBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let email = match &bridge.sdk.method {
        ContactBindingMethod::Email { email } => email.clone(),
        _ => unreachable!("wait bridge is only used for email"),
    };
    let mut result = im_core::compat::identity::bind_contact_with_bridge(
        client,
        bridge.sdk.clone(),
        IdentitySessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        IdentityLegacyRestTransport {
            resolved,
            manager,
            record: record.clone(),
        },
    )
    .map_err(|err| super::map_im_error(err, "id bind"))?;
    if result.result.state == ContactBindingState::Completed {
        return bind_command_result(identity, result).map_err(crate::app::identity_exit);
    }
    if result.result.state != ContactBindingState::Pending {
        return bind_command_result(identity, result).map_err(crate::app::identity_exit);
    }

    let wait_result = wait_for_email_verification_via_im_core(
        resolved,
        manager,
        client,
        record,
        &email,
        bridge.legacy.verification_timeout,
        bridge.legacy.poll_interval_seconds,
    )?;
    result.result = wait_result.result;
    result.raw_status = wait_result.raw_status;
    result.raw_send = wait_result.raw_send;
    bind_command_result(identity, result).map_err(crate::app::identity_exit)
}

fn wait_for_email_verification_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    client: &im_core::ImClient,
    record: &identity::types::StoredIdentity,
    email: &str,
    timeout_secs: i64,
    poll_interval_secs: f64,
) -> Result<im_core::compat::identity::ContactBindingBridgeResult, ExitError> {
    let timeout_secs = if timeout_secs <= 0 { 300 } else { timeout_secs };
    let poll_interval_secs = if poll_interval_secs <= 0.0 {
        5.0
    } else {
        poll_interval_secs
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
    loop {
        let result = im_core::compat::identity::bind_email_status_with_bridge(
            client,
            email.to_string(),
            IdentitySessionProvider {
                subject: client.did().clone(),
                resolved,
                manager,
                record: record.clone(),
            },
            IdentityLegacyRestTransport {
                resolved,
                manager,
                record: record.clone(),
            },
        )
        .map_err(|err| super::map_im_error(err, "id bind"))?;
        if result.result.state == ContactBindingState::Completed
            || std::time::Instant::now() >= deadline
        {
            return Ok(result);
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(poll_interval_secs));
    }
}

fn bind_command_result(
    identity: &identity::IdentitySummary,
    result: im_core::compat::identity::ContactBindingBridgeResult,
) -> Result<identity::CommandResult, identity::IdentityError> {
    match result.result.state {
        ContactBindingState::OtpSent => identity::wire::bind_phone_otp_result(
            identity,
            &result.result.target,
            result.raw_send.unwrap_or(Value::Null),
        ),
        ContactBindingState::Completed
            if matches!(
                result.result.method,
                im_core::identity::ContactBindingMethodKind::Phone
            ) =>
        {
            identity::wire::bind_phone_completed_result(
                identity,
                &result.result.target,
                result.raw_send.unwrap_or(Value::Null),
            )
        }
        ContactBindingState::EmailSent => Ok(identity::wire::bind_email_sent_result(
            identity,
            &result.result.target,
            result.raw_send.unwrap_or(Value::Null),
        )),
        ContactBindingState::Pending => Ok(identity::wire::bind_email_pending_result(
            identity,
            &result.result.target,
        )),
        ContactBindingState::Completed => Ok(identity::wire::bind_email_completed_result(
            identity,
            &result.result.target,
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
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let result = im_core::compat::profile::read_self_profile_with_bridge(
        &client,
        ProfileSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ProfileLegacyTransport {
            resolved,
            manager,
            record,
        },
    )
    .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_self_result(result.raw))
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
        let peer = PeerRef::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let result = im_core::compat::directory::resolve_peer_with_bridge(
            &client,
            peer,
            DirectoryLegacyTransport { resolved },
        )
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let did = result.resolution.did.as_str().to_string();
        subject.insert("handle".to_string(), Value::String(target.local_part));
        subject.insert("full_handle".to_string(), Value::String(target.full_handle));
        subject.insert("domain".to_string(), Value::String(target.effective_domain));
        subject.insert("did".to_string(), Value::String(did));
        let profile = result.public_profile.unwrap_or(Value::Null);
        return Ok(identity::wire::profile_public_result(
            Value::Object(subject),
            profile,
        ));
    }
    if !profile_did.trim().is_empty() {
        subject.insert("did".to_string(), Value::String(profile_did.clone()));
    }
    let peer = PeerRef::parse(&profile_did, "")
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    let result = im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        peer,
        DirectoryLegacyTransport { resolved },
    )
    .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_public_result(
        Value::Object(subject),
        result.public_profile.unwrap_or(Value::Null),
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
    let result = im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        peer,
        DirectoryLegacyTransport { resolved },
    )
    .map_err(|err| super::map_im_error(err, "id resolve"))?;
    Ok(identity::wire::resolve_result(
        result.resolve,
        result.lookup,
        result.public_profile,
        result.resolution.warnings,
    ))
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
) -> Result<SetProfileBridgeRequest, ExitError> {
    let legacy = identity::SetProfileParams {
        display_name,
        bio,
        tags_csv,
        markdown,
        markdown_file,
    };
    let patch = profile_patch_from_legacy_params(&legacy)?;
    Ok(SetProfileBridgeRequest { patch, legacy })
}

pub fn set_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: SetProfileBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let identity = identity::store::identity_summary_from_record(&record);
    let result = im_core::compat::profile::update_profile_with_bridge(
        &client,
        request.patch,
        ProfileSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ProfileLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
    )
    .map_err(|err| super::map_im_error(err, "id profile set"))?;
    let display_name = request.legacy.display_name.trim();
    if !display_name.is_empty() {
        let _ = manager.update_display_name(&record.identity_name, display_name);
    }
    Ok(identity::wire::profile_update_result(
        &identity,
        result.changed_fields,
        result.raw,
    ))
}

fn profile_patch_from_legacy_params(
    params: &identity::SetProfileParams,
) -> Result<ProfilePatch, ExitError> {
    let markdown_file = params.markdown_file.trim();
    let markdown = if markdown_file.is_empty() {
        trimmed_optional(&params.markdown)
    } else {
        let raw = std::fs::read(&params.markdown_file).map_err(|err| {
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
        display_name: trimmed_optional(&params.display_name),
        bio: trimmed_optional(&params.bio),
        tags: tags_patch(&params.tags_csv),
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

struct ProfileSessionProvider<'a> {
    subject: Did,
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

struct IdentitySessionProvider<'a> {
    subject: Did,
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

impl im_core::compat::profile::BridgeProfileSessionProvider for ProfileSessionProvider<'_> {
    fn ensure_profile_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::UserProfile,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

impl im_core::compat::identity::BridgeIdentitySessionProvider for IdentitySessionProvider<'_> {
    fn ensure_identity_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::UserProfile,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct ProfileLegacyTransport<'a> {
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

struct IdentityLegacyRestTransport<'a> {
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

struct DirectoryLegacyTransport<'a> {
    resolved: &'a crate::config::Resolved,
}

struct IdentityRecoveryRpcTransport<'a> {
    resolved: &'a crate::config::Resolved,
}

struct RecoverLocalStateStore<'a> {
    paths: &'a crate::config::Paths,
}

impl im_core::compat::directory::BridgeDirectoryRpcTransport for DirectoryLegacyTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> im_core::ImResult<Value> {
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        let profile = match method {
            "get_public_profile" => Profile::RpcReadHeavy,
            _ => Profile::RpcDefault,
        };
        client
            .rpc_call_profile(profile, endpoint, method, params)
            .map_err(identity_error_to_im_error)
    }
}

impl im_core::compat::identity::BridgeIdentityRpcTransport for IdentityRecoveryRpcTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> im_core::ImResult<Value> {
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        client
            .rpc_call_profile(Profile::RpcDefault, endpoint, method, params)
            .map_err(identity_error_to_im_error)
    }
}

impl im_core::compat::identity::BridgeRecoverLocalStateStore for RecoverLocalStateStore<'_> {
    fn merge_recovered_handle_local_state(
        &mut self,
        request: im_core::compat::identity::RecoverLocalStateMergeRequest,
    ) -> im_core::ImResult<im_core::compat::identity::RecoverLocalStateMergeResult> {
        let (store_merge_counts, e2ee_cleanup_counts) = store::merge_recovered_handle_local_state(
            self.paths,
            &request.old_owner_dids,
            &request.new_owner_did,
            &request.final_identity_name,
        )
        .map_err(store_error_to_im_error)?;
        Ok(im_core::compat::identity::RecoverLocalStateMergeResult {
            store_merge_counts,
            e2ee_cleanup_counts,
        })
    }
}

impl im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport
    for ProfileLegacyTransport<'_>
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        read_authenticated_profile_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(identity_error_to_im_error)
    }
}

impl im_core::compat::identity::BridgeIdentityAuthenticatedRestTransport
    for IdentityLegacyRestTransport<'_>
{
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: Value,
    ) -> im_core::ImResult<Value> {
        let call = legacy_rest_call(endpoint, method, body, BTreeMap::new(), true)?;
        let mut auth = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        client
            .authenticated_rest_post(call, &mut auth)
            .map_err(identity_error_to_im_error)
    }

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &BTreeMap<String, String>,
    ) -> im_core::ImResult<Value> {
        let call = legacy_rest_call(endpoint, method, Value::Null, query.clone(), true)?;
        let auth = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        client
            .rest_get_with_bearer(call, auth.current_jwt())
            .map_err(identity_error_to_im_error)
    }
}

fn legacy_rest_call(
    endpoint: &str,
    method: &str,
    body: Value,
    query: BTreeMap<String, String>,
    authenticated: bool,
) -> im_core::ImResult<identity::wire::RestCall> {
    Ok(identity::wire::RestCall {
        endpoint: legacy_endpoint(endpoint)?,
        method: legacy_method(method)?,
        profile: Profile::RpcDefault,
        query,
        body,
        authenticated,
    })
}

fn legacy_endpoint(endpoint: &str) -> im_core::ImResult<&'static str> {
    match endpoint {
        identity::wire::EMAIL_SEND_ENDPOINT => Ok(identity::wire::EMAIL_SEND_ENDPOINT),
        identity::wire::EMAIL_STATUS_ENDPOINT => Ok(identity::wire::EMAIL_STATUS_ENDPOINT),
        identity::wire::PHONE_BIND_SEND_ENDPOINT => Ok(identity::wire::PHONE_BIND_SEND_ENDPOINT),
        identity::wire::PHONE_BIND_VERIFY_ENDPOINT => {
            Ok(identity::wire::PHONE_BIND_VERIFY_ENDPOINT)
        }
        _ => Err(im_core::ImError::TransportUnavailable {
            detail: format!("unsupported bridge REST endpoint {endpoint}"),
        }),
    }
}

fn legacy_method(method: &str) -> im_core::ImResult<&'static str> {
    match method {
        "GET" => Ok("GET"),
        "POST" => Ok("POST"),
        _ => Err(im_core::ImError::TransportUnavailable {
            detail: format!("unsupported bridge REST method {method}"),
        }),
    }
}

fn read_authenticated_profile_with_fallback(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, identity::IdentityError> {
    match read_authenticated_profile(resolved, manager, record, endpoint, method, params.clone()) {
        Ok(result) => Ok(result),
        Err(err) if identity_error_is_unauthorized(&err) => {
            let refreshed = identity::refresh_token(resolved, manager, &record.identity_name).ok();
            let record = refreshed
                .as_ref()
                .and_then(|_| manager.load(&record.identity_name).ok())
                .unwrap_or_else(|| record.clone());
            match read_authenticated_profile(resolved, manager, &record, endpoint, method, params) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn read_authenticated_profile(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, identity::IdentityError> {
    let mut auth = identity::service::auth_session(resolved, manager, record)?;
    let client = identity::client::Client::new(resolved)?;
    let profile = match method {
        "get_me" => Profile::RpcReadHeavy,
        _ => Profile::RpcDefault,
    };
    client.authenticated_rpc_call_profile(profile, endpoint, method, params, &mut auth)
}

fn identity_error_is_unauthorized(err: &identity::IdentityError) -> bool {
    matches!(
        err,
        identity::IdentityError::Service(service)
            if service.status_code == 401 || service.rpc_code == -32001
    )
}

fn identity_error_to_im_error(err: identity::IdentityError) -> im_core::ImError {
    match err {
        identity::IdentityError::InvalidInput(message) => {
            im_core::ImError::invalid_input(None, message)
        }
        identity::IdentityError::NotFound(message)
        | identity::IdentityError::NoDefaultIdentity(message) => {
            im_core::ImError::IdentityNotFound { selector: message }
        }
        identity::IdentityError::AuthRequired(_) => im_core::ImError::AuthRequired,
        identity::IdentityError::Service(service) => im_core::ImError::Service {
            status_code: (service.status_code != 0).then_some(service.status_code),
            code: (service.rpc_code != 0).then(|| service.rpc_code.to_string()),
            message: service.message,
        },
        identity::IdentityError::Io(err) => im_core::ImError::Io {
            detail: err.to_string(),
        },
        identity::IdentityError::Json(err) => im_core::ImError::Serialization {
            detail: err.to_string(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}

fn store_error_to_im_error(err: store::StoreError) -> im_core::ImError {
    match err {
        store::StoreError::Invalid(message) => im_core::ImError::invalid_input(None, message),
        store::StoreError::NotFound(message) => {
            im_core::ImError::LocalStateUnavailable { detail: message }
        }
        err => im_core::ImError::LocalStateUnavailable {
            detail: err.to_string(),
        },
    }
}

fn store_error_from_im_error(err: im_core::ImError) -> store::StoreError {
    match err {
        im_core::ImError::InvalidInput { message, .. } => store::StoreError::invalid(message),
        im_core::ImError::LocalStateUnavailable { detail } => store::StoreError::Invalid(detail),
        err => store::StoreError::Invalid(err.to_string()),
    }
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
