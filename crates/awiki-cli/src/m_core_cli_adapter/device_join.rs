//! Safe CLI projection for the AWiki-local device Join control plane.
//!
//! Verification and Join grants are write-only inputs. This adapter never
//! returns them, and the one-time approval handle is created and consumed in
//! one foreground invocation instead of crossing the CLI output boundary.

use im_core::prelude::{
    DeviceJoinAccountVerificationGrant, DeviceJoinBeginRequest, DeviceJoinConfirmApprovalRequest,
    DeviceJoinProgress, DeviceJoinRole, Did, IdentitySelector,
};
use im_core::ImCore;
use serde::Serialize;

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

const ACCOUNT_VERIFICATION_TOKEN_ENV: &str = "AWIKI_ACCOUNT_VERIFICATION_TOKEN";

pub async fn local_sessions_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let core = super::build_im_core_async(resolved).await?;
    let sessions = core
        .device_join()
        .local_sessions()
        .map_err(|err| super::map_im_error(err, "id device join sessions"))?;
    command_result(
        "device_join_sessions",
        &sessions,
        format!("Loaded {} local device Join session(s)", sessions.len()),
    )
}

pub async fn begin_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    did: &str,
    operation_id: &str,
    ttl_seconds: u64,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let did = parse_did(did)?;
    let operation_id = required_value(operation_id, "--operation-id")?;
    let grant = account_verification_grant()?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .begin_new_device_join(DeviceJoinBeginRequest {
            operation_id,
            did,
            ttl_seconds,
            account_verification_grant: grant,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device join start"))?;
    progress_result("device_join_start", progress, "Device Join request created")
}

pub async fn poll_new_device_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    join_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .poll_new_device_join(&join_session_id)
        .await
        .map_err(|err| super::map_im_error(err, "id device join poll"))?;
    progress_result("device_join_poll", progress, "Device Join state refreshed")
}

pub async fn registry_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let core = super::build_im_core_async(resolved).await?;
    let registry = core
        .device_join()
        .registry(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id device list"))?;
    let count = registry.devices.len();
    command_result(
        "device_registry",
        &registry,
        format!("Loaded {count} authorized device(s)"),
    )
}

pub async fn claim_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    operation_id: &str,
    challenge_ttl_seconds: u64,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let join_session_id = required_value(join_session_id, "--session")?;
    let operation_id = required_value(operation_id, "--operation-id")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .claim_device_join(
            selector,
            &join_session_id,
            &operation_id,
            challenge_ttl_seconds,
        )
        .await
        .map_err(|err| super::map_im_error(err, "id device join claim"))?;
    progress_result(
        "device_join_claim",
        progress,
        "Device Join claimed and local challenge prepared",
    )
}

pub async fn poll_admin_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = poll_admin(&core, selector, &join_session_id).await?;
    progress_result(
        "device_join_admin_poll",
        progress,
        "Administrative Device Join state refreshed",
    )
}

pub(crate) async fn approve_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    role: DeviceJoinRole,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str) -> Result<(), ExitError>,
{
    require_rollout_enabled()?;
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = poll_admin(&core, selector.clone(), &join_session_id).await?;
    let sas = progress.sas.as_deref().ok_or_else(|| {
        ExitError::new(
            "join_not_ready",
            3,
            "The new device response is not ready for SAS confirmation.",
            "Run `awiki-cli id device join poll --session <SESSION> --admin` and retry after both devices show a SAS.",
        )
    })?;
    confirm(sas)?;

    let prompt = core
        .device_join()
        .prepare_device_join_approval(selector, &join_session_id, role, true)
        .map_err(|err| super::map_im_error(err, "id device join approve"))?;
    let approved = core
        .device_join()
        .confirm_device_join_approval(DeviceJoinConfirmApprovalRequest {
            approval_handle: prompt.approval_handle,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device join approve"))?;
    progress_result("device_join_approve", approved, "Device Join approved")
}

pub async fn cancel_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    admin_side: bool,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let session = if admin_side {
        core.device_join()
            .cancel_admin_device_join(selector, &join_session_id)
    } else {
        core.device_join().cancel_new_device_join(&join_session_id)
    }
    .map_err(|err| super::map_im_error(err, "id device join cancel"))?;
    command_result(
        "device_join_cancel",
        &session,
        "Device Join session cancelled".to_owned(),
    )
}

pub fn role_from_cli(value: &str) -> Result<DeviceJoinRole, ExitError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "member" => Ok(DeviceJoinRole::Member),
        "admin" => Ok(DeviceJoinRole::Admin),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            "--role must be member or admin.",
            "Use member for the default communication-only role, or admin to authorize device management.",
        )),
    }
}

pub(crate) fn require_rollout_enabled() -> Result<(), ExitError> {
    if super::vault::multi_device_join_enabled()? {
        return Ok(());
    }
    Err(ExitError::new(
        "unsupported_capability",
        2,
        "Multi-device Join is disabled by the local rollout gate.",
        "Set AWIKI_MULTI_DEVICE_JOIN_ENABLED=1 only in an environment prepared for the multi-device rollout.",
    ))
}

fn account_verification_grant() -> Result<DeviceJoinAccountVerificationGrant, ExitError> {
    let token = std::env::var(ACCOUNT_VERIFICATION_TOKEN_ENV).map_err(|_| {
        ExitError::new(
            "account_verification_required",
            3,
            "A short-lived account verification grant is required.",
            format!(
                "Set {ACCOUNT_VERIFICATION_TOKEN_ENV} for this process; never pass the grant on the command line."
            ),
        )
    })?;
    DeviceJoinAccountVerificationGrant::from_token(token)
        .map_err(|err| super::map_im_error(err, "id device join start"))
}

async fn poll_admin(
    core: &ImCore,
    selector: IdentitySelector,
    join_session_id: &str,
) -> Result<DeviceJoinProgress, ExitError> {
    core.device_join()
        .poll_admin_device_join(selector, join_session_id)
        .await
        .map_err(|err| super::map_im_error(err, "id device join poll"))
}

fn parse_did(value: &str) -> Result<Did, ExitError> {
    Did::parse(value.trim()).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --did: {err}"),
            "Pass the existing account DID returned by account verification.",
        )
    })
}

fn required_value(value: &str, flag: &str) -> Result<String, ExitError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{flag} is required."),
            format!("Pass a non-empty value for {flag}."),
        ));
    }
    Ok(value.to_owned())
}

fn progress_result(
    action: &str,
    progress: DeviceJoinProgress,
    summary: &str,
) -> Result<CommandResult, ExitError> {
    command_result(action, &progress, summary.to_owned())
}

fn command_result(
    action: &str,
    value: &impl Serialize,
    summary: String,
) -> Result<CommandResult, ExitError> {
    let value = serde_json::to_value(value).map_err(|err| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize device Join result: {err}"),
            "Report this issue without including verification grants or private key material.",
        )
    })?;
    Ok(CommandResult {
        data: serde_json::json!({
            "action": action,
            "result": value,
        }),
        summary,
        warnings: Vec::new(),
    })
}
