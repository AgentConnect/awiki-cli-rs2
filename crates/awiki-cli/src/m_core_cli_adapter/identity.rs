use im_core::identity::{ContactBindingMethodKind, ContactBindingResult};
use im_core::prelude::{
    ContactBindingMethod, ContactBindingRequest, ContactBindingState, Did, DirectoryResolution,
    Handle, HandleRegistrationResult, HandleRegistrationState, IdentitySelector, IdentitySubject,
    InitialProfile, PeerRef, ProfilePatch, RecoverHandleLocalFinalizeRequest,
    RecoverHandlePlanRequest, RecoverHandleRequest, RegisterHandleRequest, RegistrationMethod,
    VerificationInput,
};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub use super::identity_replace_did_plan::{
    replace_did_plan_command_request, replace_did_plan_via_im_core, ReplaceDidPlanCommandRequest,
};

fn identity_raw_response(result: &impl IdentityRawResponse) -> Value {
    result.raw_response().cloned().unwrap_or(Value::Null)
}

trait IdentityRawResponse {
    fn raw_response(&self) -> Option<&Value>;
}

impl IdentityRawResponse for ContactBindingResult {
    fn raw_response(&self) -> Option<&Value> {
        self.response_json()
    }
}

impl IdentityRawResponse for im_core::identity::RecoverHandleResult {
    fn raw_response(&self) -> Option<&Value> {
        self.response_json()
    }
}

#[derive(Debug, Clone)]
pub struct GetProfileCommandRequest {
    pub self_profile: bool,
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct ResolveCommandRequest {
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct SetProfileCommandRequest {
    pub patch: ProfilePatch,
    pub display_name: String,
    pub avatar_uri: String,
    pub avatar_url: String,
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

#[derive(Debug, Clone, Serialize, Default)]
pub struct CliIdentitySummary {
    pub identity_name: String,
    pub did: String,
    pub unique_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub full_handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    pub dir_name: String,
    pub is_default: bool,
    pub has_jwt: bool,
    pub has_did_document: bool,
    pub has_key1_private: bool,
    pub has_key1_public: bool,
    pub has_e2ee_signing_private: bool,
    pub has_e2ee_agreement_private: bool,
    pub user_state: CliUserState,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CliUserState {
    pub registration_state: String,
    pub ready_for_messaging: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedHandle {
    local_part: String,
    full_handle: String,
    effective_domain: String,
}

const RECOVER_IDENTITY_IGNORED_WARNING: &str = "The --identity flag is ignored by `awiki-cli id recover`; the recover target and final live identity are derived only from --handle.";

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
        (None, None, None) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "exactly one of phone or email is required",
                "Pass either --phone <number> or --email <address>.",
            ));
        }
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
            avatar_url: trimmed_optional(&standard_or_compat_flag(
                command,
                "avatar-uri",
                "avatar-url",
            )),
        },
        make_default: command
            .flags
            .get("no-default")
            .is_none_or(|value| value != "true"),
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
    resolved: &crate::workspace_config::Resolved,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    register_handle_plan_command_result(resolved, request)
}

pub fn register_handle_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    let core = super::build_im_core(resolved)?;
    let result = core
        .identities()
        .register_handle(request.clone())
        .map_err(|err| super::map_im_error(err, "id register"))?;
    register_handle_command_result(result, &request)
}

pub async fn register_handle_plan_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    register_handle_plan_command_result_async(resolved, request).await
}

pub async fn register_handle_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let request = register_handle_command_request(command, identity_flag)?;
    let core = super::build_im_core_async(resolved).await?;
    let result = core
        .identities()
        .register_handle_async(request.clone())
        .await
        .map_err(|err| super::map_im_error(err, "id register"))?;
    register_handle_command_result(result, &request)
}

fn register_handle_command_result(
    result: HandleRegistrationResult,
    request: &RegisterHandleRequest,
) -> Result<CommandResult, ExitError> {
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
        data["identity"] = json!(cli_identity_summary_from_sdk(identity, &[]));
    }
    if let Some(phone) = registration_phone(&request.verification) {
        data["phone"] = json!(normalize_registration_phone(phone)?);
    }
    if let Some(email) = registration_email(&request.verification) {
        data["email"] = json!(normalize_registration_email(email));
    }
    Ok(CommandResult {
        summary: registration_summary(result.state, data["full_handle"].as_str().unwrap_or("")),
        data,
        warnings: result.warnings,
    })
}

pub fn list_identities_via_im_core(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
    let identities = core
        .identities()
        .list()
        .map_err(|err| super::map_im_error(err, "id list"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities);
    let identity_count = summaries.len();
    let current = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let mut warnings = Vec::new();
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
            "identities": summaries,
            "default_identity": current,
        }),
        summary: format!("Found {identity_count} local identities"),
        warnings,
    })
}

pub async fn list_identities_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let identities = core
        .identities()
        .list_async()
        .await
        .map_err(|err| super::map_im_error(err, "id list"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities);
    let identity_count = summaries.len();
    let current = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let mut warnings = Vec::new();
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
            "identities": summaries,
            "default_identity": current,
        }),
        summary: format!("Found {identity_count} local identities"),
        warnings,
    })
}

pub fn current_identity_via_im_core(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
    let default_identity = core
        .identities()
        .default_identity()
        .map_err(|err| super::map_im_error(err, "id current"))?;
    let Some(default_identity) = default_identity else {
        return Ok(CommandResult {
            data: json!({ "identity": Value::Null }),
            summary: "No default identity is configured".to_string(),
            warnings: Vec::new(),
        });
    };
    let current = cli_identity_summary_from_sdk(&default_identity, &[]);
    let mut summary = format!("Current identity is {}", current.identity_name);
    let mut warnings = Vec::new();
    if !current.user_state.ready_for_messaging {
        summary = format!("Current identity {} is local-only", current.identity_name);
        warnings.push(
            "Register or recover a handle-backed user before using messaging commands.".to_string(),
        );
    }
    Ok(CommandResult {
        data: json!({ "identity": current }),
        summary,
        warnings,
    })
}

pub async fn current_identity_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let default_identity = core
        .identities()
        .default_identity_async()
        .await
        .map_err(|err| super::map_im_error(err, "id current"))?;
    let Some(default_identity) = default_identity else {
        return Ok(CommandResult {
            data: json!({ "identity": Value::Null }),
            summary: "No default identity is configured".to_string(),
            warnings: Vec::new(),
        });
    };
    let current = cli_identity_summary_from_sdk(&default_identity, &[]);
    let mut summary = format!("Current identity is {}", current.identity_name);
    let mut warnings = Vec::new();
    if !current.user_state.ready_for_messaging {
        summary = format!("Current identity {} is local-only", current.identity_name);
        warnings.push(
            "Register or recover a handle-backed user before using messaging commands.".to_string(),
        );
    }
    Ok(CommandResult {
        data: json!({ "identity": current }),
        summary,
        warnings,
    })
}

pub fn identity_status_via_im_core(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
    let identities = core
        .identities()
        .list()
        .map_err(|err| super::map_im_error(err, "id status"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities);
    let active_identity = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let mut warnings = Vec::new();
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
    Ok(CommandResult {
        data: json!({
            "active_identity": active_identity,
            "identity_count": summaries.len(),
        }),
        summary,
        warnings,
    })
}

pub async fn identity_status_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let identities = core
        .identities()
        .list_async()
        .await
        .map_err(|err| super::map_im_error(err, "id status"))?;
    let summaries = cli_identity_summaries_from_sdk(&identities);
    let active_identity = identities
        .iter()
        .find(|identity| identity.is_default)
        .map(|identity| cli_identity_summary_from_sdk(identity, &summaries));
    let mut warnings = Vec::new();
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
    Ok(CommandResult {
        data: json!({
            "active_identity": active_identity,
            "identity_count": summaries.len(),
        }),
        summary,
        warnings,
    })
}

pub fn use_identity_plan_via_im_core(identity_name: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "set_default_identity",
                "identity_name": identity_name.trim(),
                "config_updates": ["active_identity"],
            }
        }),
        summary: "Dry run: default identity switch planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn use_identity_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_name: &str,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
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
    write_default_identity_file(&resolved.paths.identity_dir, target_name)?;
    let summary = cli_identity_summary_from_sdk(&change.next, &[]);
    Ok(CommandResult {
        data: json!({
            "action": "set_default_identity",
            "identity": summary,
        }),
        summary: format!("Default identity switched to {target_name}"),
        warnings: Vec::new(),
    })
}

pub async fn use_identity_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_name: &str,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let change = core
        .identities()
        .plan_default_identity_change_async(IdentitySelector::LocalAlias(identity_name.to_string()))
        .await
        .map_err(|err| super::map_im_error(err, "id use"))?;
    let target_name = change
        .next
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(identity_name);
    write_default_identity_file(&resolved.paths.identity_dir, target_name)?;
    let summary = cli_identity_summary_from_sdk(&change.next, &[]);
    Ok(CommandResult {
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

pub fn bind_contact_plan_via_im_core(command: &ParsedCommand) -> Result<CommandResult, ExitError> {
    let request = bind_contact_command_request(command)?;
    Ok(bind_contact_plan_command_result(&request.sdk))
}

pub fn bind_contact_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    command: &ParsedCommand,
) -> Result<CommandResult, ExitError> {
    let request = bind_contact_command_request(command)?;
    let client = super::build_im_client(resolved, cli_identity_selector(identity_flag))?;
    require_messaging_ready(client.current_identity(), "id bind")?;
    let identity = cli_identity_summary_from_sdk(client.current_identity(), &[]);
    let result = if request.sdk.wait_for_email_verification
        && matches!(request.sdk.method, ContactBindingMethod::Email { .. })
    {
        bind_email_wait_via_im_core(&client, &identity, request)?
    } else {
        let result = client
            .identity()
            .bind_contact(request.sdk)
            .map_err(|err| super::map_im_error(err, "id bind"))?;
        bind_command_result(&identity, result)?
    };
    Ok(result)
}

pub async fn bind_contact_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    command: &ParsedCommand,
) -> Result<CommandResult, ExitError> {
    let request = bind_contact_command_request(command)?;
    let client =
        super::build_im_client_async(resolved, cli_identity_selector(identity_flag)).await?;
    require_messaging_ready(client.current_identity(), "id bind")?;
    let identity = cli_identity_summary_from_sdk(client.current_identity(), &[]);
    let result = if request.sdk.wait_for_email_verification
        && matches!(request.sdk.method, ContactBindingMethod::Email { .. })
    {
        bind_email_wait_via_im_core_async(&client, &identity, request).await?
    } else {
        let result = client
            .identity()
            .bind_contact_async(request.sdk)
            .await
            .map_err(|err| super::map_im_error(err, "id bind"))?;
        bind_command_result(&identity, result)?
    };
    Ok(result)
}

fn require_messaging_ready(
    identity: &im_core::identity::IdentitySummary,
    context: &'static str,
) -> Result<(), ExitError> {
    if identity.readiness.ready_for_messaging {
        return Ok(());
    }
    Err(super::map_im_error(im_core::ImError::AuthRequired, context))
}

pub fn recover_handle_request(
    handle: String,
    phone: String,
    otp: Option<String>,
    local_finalize: Option<RecoverHandleLocalFinalizeRequest>,
    default_domain: &str,
) -> Result<RecoverHandleRequest, ExitError> {
    Ok(RecoverHandleRequest {
        handle: Handle::parse(&handle, default_domain)
            .map_err(|err| super::map_im_error(err, "id recover"))?,
        raw_handle: Some(handle),
        phone,
        otp,
        generated_identity: None,
        local_finalize,
    })
}

fn recover_handle_command_result(
    result: im_core::identity::RecoverHandleResult,
    plan: &im_core::identity::RecoverHandlePlan,
) -> Result<CommandResult, ExitError> {
    let raw = identity_raw_response(&result);
    if result.state == im_core::identity::RecoverHandleState::OtpSent {
        let full_handle = plan.target_handle.clone();
        let handle = full_handle
            .split_once('.')
            .map(|(local, _)| local.to_string())
            .unwrap_or_else(|| full_handle.clone());
        return Ok(recover_otp_command_result(
            &plan.final_identity_name,
            &handle,
            &full_handle,
            &result.phone,
            raw,
        )?);
    }
    let Some(local) = result.local_recovery.as_ref() else {
        return Err(ExitError::new(
            "internal_error",
            1,
            "recover_handle result is missing local recovery summary",
            "Run `awiki-cli doctor` to inspect configuration and storage paths.",
        ));
    };
    Ok(CommandResult {
        data: json!({
            "action": "recover_handle",
            "identity": local.identity,
            "backup_path": local.backup_path,
            "archived_identities": local.archived_identities,
            "archived_dids": local.archived_dids,
            "full_handle": local.full_handle,
            "final_identity_name": local.final_identity_name,
            "store_merge_counts": local.store_merge_counts,
            "e2ee_cleanup_counts": local.e2ee_cleanup_counts,
            "result": raw,
        }),
        summary: format!("Handle {} recovered successfully", local.full_handle),
        warnings: result.warnings,
    })
}

pub fn recover_handle_plan_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core(resolved)?;
    let plan = core
        .identities()
        .recover_handle_plan(RecoverHandlePlanRequest {
            handle: Handle::parse(request.handle.clone(), &resolved.did_domain)
                .map_err(|err| super::map_im_error(err, "id recover"))?,
            raw_handle: Some(request.handle.clone()),
            phone: request.phone.clone(),
            otp: trimmed_optional(&request.otp),
        })
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    Ok(CommandResult {
        data: json!({ "plan": plan }),
        summary: "Dry run: handle recovery planned".to_string(),
        warnings: Vec::new(),
    })
}

pub async fn recover_handle_plan_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let plan = core
        .identities()
        .recover_handle_plan_async(RecoverHandlePlanRequest {
            handle: Handle::parse(request.handle.clone(), &resolved.did_domain)
                .map_err(|err| super::map_im_error(err, "id recover"))?,
            raw_handle: Some(request.handle.clone()),
            phone: request.phone.clone(),
            otp: trimmed_optional(&request.otp),
        })
        .await
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    Ok(CommandResult {
        data: json!({ "plan": plan }),
        summary: "Dry run: handle recovery planned".to_string(),
        warnings: Vec::new(),
    })
}

pub fn recover_handle_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
) -> Result<CommandResult, ExitError> {
    let phone = request.phone.trim().to_string();
    let otp = request.otp.trim().to_string();
    if request.handle.trim().is_empty() || phone.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: handle and phone are required",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        ));
    }

    let core = super::build_im_core(resolved)?;
    let plan = core
        .identities()
        .recover_handle_plan(RecoverHandlePlanRequest {
            handle: Handle::parse(request.handle.clone(), &resolved.did_domain)
                .map_err(|err| super::map_im_error(err, "id recover"))?,
            raw_handle: Some(request.handle.clone()),
            phone: request.phone.clone(),
            otp: None,
        })
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    let sdk_request = recover_handle_request(
        request.handle.clone(),
        request.phone.clone(),
        trimmed_optional(&request.otp),
        (!otp.is_empty()).then(|| RecoverHandleLocalFinalizeRequest {
            raw_handle: Some(request.handle.clone()),
            active_identity_name: Some(resolved.active_identity.clone()),
            config_file_path: Some(std::path::PathBuf::from(&resolved.paths.config_file)),
        }),
        &resolved.did_domain,
    )?;
    let result = core
        .identities()
        .recover_handle(sdk_request)
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    recover_handle_command_result(result, &plan)
}

pub async fn recover_handle_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
) -> Result<CommandResult, ExitError> {
    let phone = request.phone.trim().to_string();
    let otp = request.otp.trim().to_string();
    if request.handle.trim().is_empty() || phone.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: handle and phone are required",
            "Usage: awiki-cli id recover --handle <handle> --phone <phone> [--otp <code>]",
        ));
    }

    let core = super::build_im_core_async(resolved).await?;
    let plan = core
        .identities()
        .recover_handle_plan_async(RecoverHandlePlanRequest {
            handle: Handle::parse(request.handle.clone(), &resolved.did_domain)
                .map_err(|err| super::map_im_error(err, "id recover"))?,
            raw_handle: Some(request.handle.clone()),
            phone: request.phone.clone(),
            otp: None,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    let sdk_request = recover_handle_request(
        request.handle.clone(),
        request.phone.clone(),
        trimmed_optional(&request.otp),
        (!otp.is_empty()).then(|| RecoverHandleLocalFinalizeRequest {
            raw_handle: Some(request.handle.clone()),
            active_identity_name: Some(resolved.active_identity.clone()),
            config_file_path: Some(std::path::PathBuf::from(&resolved.paths.config_file)),
        }),
        &resolved.did_domain,
    )?;
    let result = core
        .identities()
        .recover_handle_async(sdk_request)
        .await
        .map_err(|err| super::map_im_error(err, "id recover"))?;
    recover_handle_command_result(result, &plan)
}

pub fn recover_handle_command_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
    dry_run: bool,
    identity_changed: bool,
) -> Result<CommandResult, ExitError> {
    let mut result = if dry_run {
        recover_handle_plan_via_im_core(resolved, request)
    } else {
        recover_handle_via_im_core(resolved, request)
    }?;
    append_recover_identity_warning(&mut result, identity_changed);
    Ok(result)
}

pub async fn recover_handle_command_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    request: RecoverHandleCommandRequest,
    dry_run: bool,
    identity_changed: bool,
) -> Result<CommandResult, ExitError> {
    let mut result = if dry_run {
        recover_handle_plan_via_im_core_async(resolved, request).await
    } else {
        recover_handle_via_im_core_async(resolved, request).await
    }?;
    append_recover_identity_warning(&mut result, identity_changed);
    Ok(result)
}

fn append_recover_identity_warning(result: &mut CommandResult, identity_changed: bool) {
    if identity_changed {
        result
            .warnings
            .push(RECOVER_IDENTITY_IGNORED_WARNING.to_string());
    }
}

fn bind_email_wait_via_im_core(
    client: &im_core::ImClient,
    identity: &CliIdentitySummary,
    request: BindContactCommandRequest,
) -> Result<CommandResult, ExitError> {
    let email = match &request.sdk.method {
        ContactBindingMethod::Email { email } => email.clone(),
        _ => unreachable!("wait request is only used for email"),
    };
    let mut result = client
        .identity()
        .bind_contact(request.sdk.clone())
        .map_err(|err| super::map_im_error(err, "id bind"))?;
    if result.state == ContactBindingState::Completed {
        return bind_command_result(identity, result);
    }
    if result.state != ContactBindingState::Pending {
        return bind_command_result(identity, result);
    }

    let wait_result = wait_for_email_verification_via_im_core(
        client,
        &email,
        request.verification_timeout,
        request.poll_interval_seconds,
    )?;
    result = wait_result;
    bind_command_result(identity, result)
}

async fn bind_email_wait_via_im_core_async(
    client: &im_core::ImClient,
    identity: &CliIdentitySummary,
    request: BindContactCommandRequest,
) -> Result<CommandResult, ExitError> {
    let email = match &request.sdk.method {
        ContactBindingMethod::Email { email } => email.clone(),
        _ => unreachable!("wait request is only used for email"),
    };
    let mut result = client
        .identity()
        .bind_contact_async(request.sdk.clone())
        .await
        .map_err(|err| super::map_im_error(err, "id bind"))?;
    if result.state == ContactBindingState::Completed {
        return bind_command_result(identity, result);
    }
    if result.state != ContactBindingState::Pending {
        return bind_command_result(identity, result);
    }

    let wait_result = wait_for_email_verification_via_im_core_async(
        client,
        &email,
        request.verification_timeout,
        request.poll_interval_seconds,
    )
    .await?;
    result = wait_result;
    bind_command_result(identity, result)
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

async fn wait_for_email_verification_via_im_core_async(
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
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
    loop {
        let result = client
            .identity()
            .bind_email_status_async(email.to_string())
            .await
            .map_err(|err| super::map_im_error(err, "id bind"))?;
        if result.state == ContactBindingState::Completed || tokio::time::Instant::now() >= deadline
        {
            return Ok(result);
        }
        tokio::time::sleep(std::time::Duration::from_secs_f64(poll_interval_secs)).await;
    }
}

fn bind_command_result(
    identity: &CliIdentitySummary,
    result: ContactBindingResult,
) -> Result<CommandResult, ExitError> {
    match result.state {
        ContactBindingState::OtpSent => {
            bind_phone_otp_command_result(identity, &result.target, identity_raw_response(&result))
        }
        ContactBindingState::Completed
            if matches!(result.method, ContactBindingMethodKind::Phone) =>
        {
            bind_phone_completed_command_result(
                identity,
                &result.target,
                identity_raw_response(&result),
            )
        }
        ContactBindingState::EmailSent => Ok(bind_email_sent_command_result(
            identity,
            &result.target,
            identity_raw_response(&result),
        )),
        ContactBindingState::Pending => {
            Ok(bind_email_pending_command_result(identity, &result.target))
        }
        ContactBindingState::Completed => Ok(bind_email_completed_command_result(
            identity,
            &result.target,
        )),
    }
}

pub fn get_profile_request(command: &ParsedCommand) -> GetProfileCommandRequest {
    GetProfileCommandRequest {
        self_profile: command
            .flags
            .get("self")
            .is_some_and(|value| value == "true"),
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn get_self_profile_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let client = super::build_im_client(resolved, cli_identity_selector(identity_flag))?;
    let profile = client
        .identity()
        .profile()
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(profile_self_command_result(profile.to_wire_profile_value()))
}

pub async fn get_self_profile_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
) -> Result<CommandResult, ExitError> {
    let client =
        super::build_im_client_async(resolved, cli_identity_selector(identity_flag)).await?;
    let profile = client
        .identity()
        .profile_async()
        .await
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(profile_self_command_result(profile.to_wire_profile_value()))
}

pub fn get_public_profile_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: GetProfileCommandRequest,
) -> Result<CommandResult, ExitError> {
    let Some(client) = build_optional_directory_client(resolved, identity_flag, "id profile get")?
    else {
        return Err(super::unsupported_cutover_command(
            "id.profile.get",
            "unauthenticated public profile lookup",
            "anonymous directory client support",
        ));
    };
    let mut subject = serde_json::Map::new();
    let profile_did = request.did.trim().to_string();
    if !request.handle.trim().is_empty() {
        let target = normalize_handle_input(&request.handle, &resolved.did_domain)?;
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
        return Ok(profile_public_command_result(
            Value::Object(subject),
            result.profile.to_wire_profile_value(),
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
    Ok(profile_public_command_result(
        Value::Object(subject),
        result.profile.to_wire_profile_value(),
    ))
}

pub async fn get_public_profile_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: GetProfileCommandRequest,
) -> Result<CommandResult, ExitError> {
    let Some(client) =
        build_optional_directory_client_async(resolved, identity_flag, "id profile get").await?
    else {
        return Err(super::unsupported_cutover_command(
            "id.profile.get",
            "unauthenticated public profile lookup",
            "anonymous directory client support",
        ));
    };
    let mut subject = serde_json::Map::new();
    let profile_did = request.did.trim().to_string();
    if !request.handle.trim().is_empty() {
        let target = normalize_handle_input(&request.handle, &resolved.did_domain)?;
        let handle = Handle::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let result = client
            .directory()
            .public_profile_async(IdentitySubject::Handle(handle))
            .await
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let did = result.did.as_str().to_string();
        subject.insert("handle".to_string(), Value::String(target.local_part));
        subject.insert("full_handle".to_string(), Value::String(target.full_handle));
        subject.insert("domain".to_string(), Value::String(target.effective_domain));
        subject.insert("did".to_string(), Value::String(did));
        return Ok(profile_public_command_result(
            Value::Object(subject),
            result.profile.to_wire_profile_value(),
        ));
    }
    if !profile_did.trim().is_empty() {
        subject.insert("did".to_string(), Value::String(profile_did.clone()));
    }
    let did = Did::parse(&profile_did).map_err(|err| super::map_im_error(err, "id profile get"))?;
    let result = client
        .directory()
        .public_profile_async(IdentitySubject::Did(did))
        .await
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(profile_public_command_result(
        Value::Object(subject),
        result.profile.to_wire_profile_value(),
    ))
}

pub fn resolve_request(command: &ParsedCommand) -> ResolveCommandRequest {
    ResolveCommandRequest {
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn resolve_identity_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: ResolveCommandRequest,
) -> Result<CommandResult, ExitError> {
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
    let Some(client) = build_optional_directory_client(resolved, identity_flag, "id resolve")?
    else {
        return Err(super::unsupported_cutover_command(
            "id.resolve",
            "unauthenticated directory resolve",
            "anonymous directory client support",
        ));
    };
    let peer = if !handle.is_empty() {
        let target = normalize_handle_input(handle, &resolved.did_domain)?;
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

pub async fn resolve_identity_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: ResolveCommandRequest,
) -> Result<CommandResult, ExitError> {
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
        build_optional_directory_client_async(resolved, identity_flag, "id resolve").await?
    else {
        return Err(super::unsupported_cutover_command(
            "id.resolve",
            "unauthenticated directory resolve",
            "anonymous directory client support",
        ));
    };
    let peer = if !handle.is_empty() {
        let target = normalize_handle_input(handle, &resolved.did_domain)?;
        PeerRef::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id resolve"))?
    } else {
        PeerRef::parse(did, "").map_err(|err| super::map_im_error(err, "id resolve"))?
    };
    let result = client
        .directory()
        .resolve_peer_async(peer)
        .await
        .map_err(|err| super::map_im_error(err, "id resolve"))?;
    Ok(resolve_command_result_from_sdk(result))
}

fn build_optional_directory_client(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    context: &'static str,
) -> Result<Option<im_core::ImClient>, ExitError> {
    let core = super::build_im_core(resolved)?;
    match core.client(cli_identity_selector(identity_flag)) {
        Ok(client) => Ok(Some(client)),
        Err(im_core::ImError::DefaultIdentityMissing)
        | Err(im_core::ImError::IdentityRequired)
        | Err(im_core::ImError::IdentityNotFound { .. }) => Ok(None),
        Err(err) => Err(super::map_im_error(err, context)),
    }
}

async fn build_optional_directory_client_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    context: &'static str,
) -> Result<Option<im_core::ImClient>, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    match core
        .client_async(cli_identity_selector(identity_flag))
        .await
    {
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
    avatar_uri: String,
    avatar_url: String,
) -> Result<SetProfileCommandRequest, ExitError> {
    let patch = profile_patch_from_command(
        &display_name,
        &bio,
        &tags_csv,
        &markdown,
        &markdown_file,
        &avatar_uri,
        &avatar_url,
    )?;
    Ok(SetProfileCommandRequest {
        patch,
        display_name,
        avatar_uri,
        avatar_url,
    })
}

pub fn set_profile_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: SetProfileCommandRequest,
) -> Result<CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, selector)?;
    let identity = cli_identity_summary_from_sdk(client.current_identity(), &[]);
    let changed_fields = changed_fields_from_profile_patch(&request.patch);
    let profile = client
        .identity()
        .update_profile(request.patch)
        .map_err(|err| super::map_im_error(err, "id profile set"))?;
    Ok(profile_update_command_result(
        &identity,
        changed_fields,
        profile.to_wire_profile_value(),
    ))
}

pub async fn set_profile_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    identity_flag: &str,
    request: SetProfileCommandRequest,
) -> Result<CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client_async(resolved, selector).await?;
    let identity = cli_identity_summary_from_sdk(client.current_identity(), &[]);
    let changed_fields = changed_fields_from_profile_patch(&request.patch);
    let profile = client
        .identity()
        .update_profile_async(request.patch)
        .await
        .map_err(|err| super::map_im_error(err, "id profile set"))?;
    Ok(profile_update_command_result(
        &identity,
        changed_fields,
        profile.to_wire_profile_value(),
    ))
}

fn profile_patch_from_command(
    display_name: &str,
    bio: &str,
    tags_csv: &str,
    markdown: &str,
    markdown_file: &str,
    avatar_uri: &str,
    avatar_url: &str,
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
        avatar_uri: trimmed_optional(avatar_uri),
        avatar_url: trimmed_optional(avatar_url),
        ..ProfilePatch::default()
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
    if patch.avatar_uri.is_some() {
        fields.push("avatar_uri".to_string());
    }
    if patch.avatar_url.is_some() {
        fields.push("avatar_url".to_string());
    }
    fields
}

fn resolve_command_result_from_sdk(resolution: DirectoryResolution) -> CommandResult {
    let resolve = Some(json!({ "did": resolution.did.as_str() }));
    let lookup = resolution.handle.as_ref().map(|handle| {
        json!({
            "did": resolution.did.as_str(),
            "handle": handle.as_str(),
            "full_handle": handle.as_str(),
        })
    });
    let public_profile = resolution
        .profile
        .as_ref()
        .map(im_core::identity::Profile::to_wire_profile_value);
    resolve_command_result(resolve, lookup, public_profile, resolution.warnings)
}

fn recover_otp_command_result(
    identity_name: &str,
    handle: &str,
    full_handle: &str,
    phone: &str,
    result: Value,
) -> Result<CommandResult, ExitError> {
    let phone = normalize_registration_phone(phone)?;
    let full_handle = full_handle.trim();
    Ok(CommandResult {
        data: json!({
            "action": "send_recover_otp",
            "identity_name": identity_name,
            "handle": handle.trim(),
            "full_handle": full_handle,
            "method": "phone",
            "phone": phone,
            "verification_state": "otp_sent",
            "result": result,
        }),
        summary: format!("OTP sent for handle {full_handle} recovery"),
        warnings: Vec::new(),
    })
}

fn bind_phone_otp_command_result(
    identity: &CliIdentitySummary,
    phone: &str,
    result: Value,
) -> Result<CommandResult, ExitError> {
    Ok(CommandResult {
        data: json!({
            "action": "send_bind_phone_otp",
            "identity": identity,
            "phone": normalize_registration_phone(phone)?,
            "verification_state": "otp_sent",
            "result": result,
        }),
        summary: "Phone binding OTP sent".to_string(),
        warnings: Vec::new(),
    })
}

fn bind_phone_completed_command_result(
    identity: &CliIdentitySummary,
    phone: &str,
    result: Value,
) -> Result<CommandResult, ExitError> {
    Ok(CommandResult {
        data: json!({
            "action": "bind_phone",
            "identity": identity,
            "phone": normalize_registration_phone(phone)?,
            "verification_state": "completed",
            "result": result,
        }),
        summary: "Phone bound successfully".to_string(),
        warnings: Vec::new(),
    })
}

fn bind_email_sent_command_result(
    identity: &CliIdentitySummary,
    email: &str,
    result: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "send_bind_email",
            "identity": identity,
            "email": normalize_registration_email(email),
            "verification_state": "email_sent",
            "result": result,
        }),
        summary: "Binding email sent".to_string(),
        warnings: Vec::new(),
    }
}

fn bind_email_pending_command_result(identity: &CliIdentitySummary, email: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "wait_for_bind_email",
            "identity": identity,
            "email": normalize_registration_email(email),
            "verification_state": "pending",
        }),
        summary: "Email verification is still pending".to_string(),
        warnings: Vec::new(),
    }
}

fn bind_email_completed_command_result(
    identity: &CliIdentitySummary,
    email: &str,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "bind_email",
            "identity": identity,
            "email": normalize_registration_email(email),
            "verification_state": "completed",
        }),
        summary: "Email binding verified successfully".to_string(),
        warnings: Vec::new(),
    }
}

fn resolve_command_result(
    resolve: Option<Value>,
    lookup: Option<Value>,
    public_profile: Option<Value>,
    warnings: Vec<String>,
) -> CommandResult {
    let mut data = Map::new();
    if let Some(resolve) = resolve {
        data.insert("resolve".to_string(), resolve);
    }
    if let Some(lookup) = lookup {
        data.insert("lookup".to_string(), lookup);
    }
    if let Some(public_profile) = public_profile {
        data.insert("public_profile".to_string(), public_profile);
    }
    CommandResult {
        data: Value::Object(data),
        summary: "Identity resolved successfully".to_string(),
        warnings,
    }
}

fn profile_self_command_result(profile: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "subject": "self",
            "profile": profile,
        }),
        summary: "Fetched current identity profile".to_string(),
        warnings: Vec::new(),
    }
}

fn profile_public_command_result(subject: Value, profile: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "subject": subject,
            "profile": profile,
        }),
        summary: "Fetched public profile".to_string(),
        warnings: Vec::new(),
    }
}

fn profile_update_command_result(
    identity: &CliIdentitySummary,
    changed_fields: Vec<String>,
    profile: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "update_profile",
            "identity": identity,
            "changed_fields": changed_fields,
            "profile": profile,
        }),
        summary: "Profile updated successfully".to_string(),
        warnings: Vec::new(),
    }
}

fn cli_identity_summaries_from_sdk(
    identities: &[im_core::IdentitySummary],
) -> Vec<CliIdentitySummary> {
    identities
        .iter()
        .map(|summary| cli_identity_summary_from_sdk(summary, &[]))
        .collect()
}

pub(crate) fn cli_identity_summary_from_sdk(
    summary: &im_core::IdentitySummary,
    known: &[CliIdentitySummary],
) -> CliIdentitySummary {
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
    let has_private_key = summary
        .readiness
        .missing
        .iter()
        .all(|item| !matches!(item, im_core::identity::IdentityMissingItem::PrivateKey));
    let user_state = sdk_user_state(summary);
    CliIdentitySummary {
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
        has_key1_private: has_private_key,
        has_key1_public: summary.readiness.ready_for_auth,
        has_e2ee_signing_private: has_private_key,
        has_e2ee_agreement_private: has_private_key,
        user_state,
    }
}

pub(crate) fn sdk_identity_name(summary: &im_core::IdentitySummary) -> String {
    summary
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.id.as_str())
        .to_string()
}

pub(crate) fn cli_identity_summary_from_sdk_with_status(
    summary: &im_core::IdentitySummary,
    status: &im_core::auth::AuthStatus,
) -> CliIdentitySummary {
    let mut value = cli_identity_summary_from_sdk(summary, &[]);
    value.has_jwt = status.has_session || !status.needs_refresh;
    value
}

fn write_default_identity_file(
    identity_root_dir: &str,
    identity_name: &str,
) -> Result<(), ExitError> {
    let identity_name = identity_name.trim();
    if identity_name.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "default identity name must not be empty.",
            "Run `awiki-cli id list` to inspect available identities.",
        ));
    }
    let root = std::path::Path::new(identity_root_dir);
    std::fs::create_dir_all(root).map_err(|err| {
        ExitError::new(
            "internal_error",
            1,
            format!("create identity root {}: {err}", root.display()),
            "Check workspace identity directory permissions.",
        )
    })?;
    set_private_dir_mode(root)?;
    let path = root.join("default");
    std::fs::write(&path, format!("{identity_name}\n")).map_err(|err| {
        ExitError::new(
            "internal_error",
            1,
            format!("write default identity file {}: {err}", path.display()),
            "Check workspace identity directory permissions.",
        )
    })?;
    set_private_file_mode(&path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_mode(path: &std::path::Path) -> Result<(), ExitError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|err| {
        ExitError::new(
            "internal_error",
            1,
            format!("set permissions on {}: {err}", path.display()),
            "Check workspace identity directory permissions.",
        )
    })
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &std::path::Path) -> Result<(), ExitError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &std::path::Path) -> Result<(), ExitError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|err| {
        ExitError::new(
            "internal_error",
            1,
            format!("set permissions on {}: {err}", path.display()),
            "Check workspace identity directory permissions.",
        )
    })
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &std::path::Path) -> Result<(), ExitError> {
    Ok(())
}

struct RegisterPlanTarget {
    local_part: String,
    full_handle: Handle,
    effective_domain: String,
    explicit_domain: bool,
}

fn register_handle_plan_command_result(
    resolved: &crate::workspace_config::Resolved,
    request: RegisterHandleRequest,
) -> Result<CommandResult, ExitError> {
    let target = register_plan_target(request.requested_handle.as_str(), &resolved.did_domain)?;
    let core = super::build_im_core(resolved)?;
    let existing = core
        .identities()
        .list()
        .map(|items| cli_identity_summaries_from_sdk(&items))
        .unwrap_or_default();
    let alias_base = if target.explicit_domain {
        target.full_handle.as_str()
    } else {
        target.local_part.as_str()
    };
    let identity_name = choose_named_identity(
        &request.local_alias.unwrap_or_default(),
        &existing,
        alias_base,
    );
    let (action, remote_calls) = register_plan_action_and_calls(&request.verification);
    Ok(CommandResult {
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

async fn register_handle_plan_command_result_async(
    resolved: &crate::workspace_config::Resolved,
    request: RegisterHandleRequest,
) -> Result<CommandResult, ExitError> {
    let target = register_plan_target(request.requested_handle.as_str(), &resolved.did_domain)?;
    let core = super::build_im_core_async(resolved).await?;
    let existing = core
        .identities()
        .list_async()
        .await
        .map(|items| cli_identity_summaries_from_sdk(&items))
        .unwrap_or_default();
    let alias_base = if target.explicit_domain {
        target.full_handle.as_str()
    } else {
        target.local_part.as_str()
    };
    let identity_name = choose_named_identity(
        &request.local_alias.unwrap_or_default(),
        &existing,
        alias_base,
    );
    let (action, remote_calls) = register_plan_action_and_calls(&request.verification);
    Ok(CommandResult {
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

fn bind_contact_plan_command_result(request: &ContactBindingRequest) -> CommandResult {
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

fn sdk_user_state(summary: &im_core::IdentitySummary) -> CliUserState {
    if !summary.readiness.ready_for_messaging {
        let missing = summary
            .readiness
            .missing
            .iter()
            .map(sdk_missing_item_label)
            .collect::<Vec<_>>();
        return CliUserState {
            registration_state: if missing.len() <= 1 {
                "partial_user".to_string()
            } else {
                "local_identity".to_string()
            },
            ready_for_messaging: false,
            missing,
        };
    }
    CliUserState {
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

fn normalize_handle_input(raw: &str, did_domain: &str) -> Result<NormalizedHandle, ExitError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_identity_argument("handle is required"));
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("did:") {
        return Err(invalid_identity_argument(format!(
            "did values are not supported in handle input {raw:?}"
        )));
    }

    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    if handle.is_empty() {
        return Err(invalid_identity_argument("handle is required"));
    }
    if let Some(dot) = handle.find('.') {
        let local_part = handle[..dot].trim().to_string();
        let domain = normalize_handle_domain(&handle[dot + 1..]);
        if local_part.is_empty() || domain.is_empty() {
            return Err(invalid_identity_argument(format!("invalid handle {raw:?}")));
        }
        return Ok(NormalizedHandle {
            full_handle: format!("{local_part}.{domain}"),
            local_part,
            effective_domain: domain,
        });
    }

    let domain = normalize_handle_domain(did_domain);
    if domain.is_empty() {
        return Err(invalid_identity_argument(format!(
            "did_domain is required to complete bare handle {raw:?}"
        )));
    }
    Ok(NormalizedHandle {
        full_handle: format!("{handle}.{domain}"),
        local_part: handle.to_string(),
        effective_domain: domain,
    })
}

fn normalize_handle_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn sanitize_component(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .to_string()
}

fn choose_named_identity(
    requested: &str,
    existing: &[CliIdentitySummary],
    fallback: &str,
) -> String {
    let requested = sanitize_component(requested);
    if !requested.is_empty() {
        return requested;
    }
    let mut base = sanitize_component(fallback);
    if base.is_empty() {
        base = "identity".to_string();
    }
    if !existing.iter().any(|summary| summary.identity_name == base) {
        return base;
    }
    for idx in 2..1000 {
        let candidate = format!("{base}-{idx}");
        if !existing
            .iter()
            .any(|summary| summary.identity_name == candidate)
        {
            return candidate;
        }
    }
    format!("{base}-collision")
}

fn invalid_identity_argument(message: impl Into<String>) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        message.into(),
        "Check the identity command arguments and try again.",
    )
}

fn looks_like_handle(value: &str) -> bool {
    value.starts_with('@') || value.contains('.')
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn standard_or_compat_flag(command: &ParsedCommand, standard: &str, compat: &str) -> String {
    let standard_value = string_flag(command, standard);
    if !standard_value.trim().is_empty() {
        return standard_value;
    }
    string_flag(command, compat)
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
