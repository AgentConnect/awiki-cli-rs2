//! Safe CLI adapter for the AWiki-local Handle Recovery lifecycle.
//!
//! Verification grants are consumed only from the process environment. CLI
//! arguments and results contain no OTP, token, proof, DID document, private
//! key, internal version/hash, or local checkpoint.

use im_core::identity::{
    HandleRecoveryBeginGrant, HandleRecoveryBeginRequest, HandleRecoveryCancelRequest,
    HandleRecoveryFinalizeRequest, HandleRecoveryReconfirmationGrant, IdentitySelector,
};
use im_core::ids::Handle;
use serde::Serialize;

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

const BEGIN_GRANT_ENV: &str = "AWIKI_HANDLE_RECOVERY_BEGIN_VERIFICATION_TOKEN";
const FINALIZE_GRANT_ENV: &str = "AWIKI_HANDLE_RECOVERY_FINALIZE_VERIFICATION_TOKEN";

pub(crate) async fn local_sessions_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let core = super::build_im_core_async(resolved).await?;
    let sessions = core
        .handle_recovery()
        .local_sessions()
        .map_err(|error| super::map_im_error(error, "id recovery sessions"))?;
    command_result(
        "handle_recovery_sessions",
        &sessions,
        format!("Loaded {} local Handle Recovery session(s)", sessions.len()),
        Vec::new(),
    )
}

pub(crate) async fn begin_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    handle: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let handle = Handle::parse(handle.trim(), &resolved.did_domain)
        .map_err(|error| super::map_im_error(error, "id recovery begin"))?;
    let grant = begin_grant()?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .handle_recovery()
        .begin(HandleRecoveryBeginRequest {
            handle,
            account_verification_grant: grant,
        })
        .await
        .map_err(|error| super::map_im_error(error, "id recovery begin"))?;
    command_result(
        "handle_recovery_begin",
        &progress,
        "Recovery entered its notification and cooling lifecycle".to_owned(),
        vec![
            "The Handle still points to the old DID; OTP verification did not recover any root key."
                .to_owned(),
        ],
    )
}

pub(crate) async fn status_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    recovery_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let recovery_session_id = required(recovery_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .handle_recovery()
        .status(&recovery_session_id)
        .await
        .map_err(|error| super::map_im_error(error, "id recovery status"))?;
    command_result(
        "handle_recovery_status",
        &progress,
        "Recovery state refreshed".to_owned(),
        Vec::new(),
    )
}

pub(crate) async fn cancel_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    old_identity: IdentitySelector,
    recovery_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let recovery_session_id = required(recovery_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let result = core
        .handle_recovery()
        .cancel(HandleRecoveryCancelRequest {
            old_identity,
            recovery_session_id,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|error| super::map_im_error(error, "id recovery cancel"))?;
    command_result(
        "handle_recovery_cancel",
        &result,
        "Server-authoritative Handle Recovery cancellation accepted".to_owned(),
        Vec::new(),
    )
}

pub(crate) async fn finalize_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    recovery_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let recovery_session_id = required(recovery_session_id, "--session")?;
    let grant = finalize_grant()?;
    let core = super::build_im_core_async(resolved).await?;
    let result = core
        .handle_recovery()
        .finalize(HandleRecoveryFinalizeRequest {
            recovery_session_id,
            reconfirmation_grant: grant,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|error| super::map_im_error(error, "id recovery finalize"))?;
    command_result(
        "handle_recovery_finalize",
        &result,
        "Handle cutover completed with a newly generated DID".to_owned(),
        vec![
            "Direct sessions, MLS state and historical decryption are not inherited from the old DID."
                .to_owned(),
            "Run `awiki-cli id recovery activate --session <SESSION>` after verifying the new local identity."
                .to_owned(),
        ],
    )
}

pub(crate) async fn activate_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    recovery_session_id: &str,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let recovery_session_id = required(recovery_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let identity = core
        .handle_recovery()
        .resume_activation_async(&recovery_session_id)
        .await
        .map_err(|error| super::map_im_error(error, "id recovery activate"))?;
    core.handle_recovery()
        .mark_activation_complete(&recovery_session_id)
        .map_err(|error| super::map_im_error(error, "id recovery activate"))?;
    command_result(
        "handle_recovery_activate",
        &identity,
        "Replacement DID is active locally and the restart marker was cleared".to_owned(),
        Vec::new(),
    )
}

pub(crate) fn require_rollout_enabled() -> Result<(), ExitError> {
    if super::vault::multi_device_handle_recovery_enabled()? {
        return Ok(());
    }
    Err(ExitError::new(
        "unsupported_capability",
        2,
        "Handle Recovery is disabled by the local rollout gate.",
        "Set AWIKI_MULTI_DEVICE_HANDLE_RECOVERY_ENABLED=1 only after notification, cooling, cutover, and rollback services are deployed.",
    ))
}

fn begin_grant() -> Result<HandleRecoveryBeginGrant, ExitError> {
    let token = verification_grant(BEGIN_GRANT_ENV, "begin")?;
    HandleRecoveryBeginGrant::from_bytes(token.into_bytes())
        .map_err(|error| super::map_im_error(error, "id recovery begin"))
}

fn finalize_grant() -> Result<HandleRecoveryReconfirmationGrant, ExitError> {
    let token = verification_grant(FINALIZE_GRANT_ENV, "finalize")?;
    HandleRecoveryReconfirmationGrant::from_bytes(token.into_bytes())
        .map_err(|error| super::map_im_error(error, "id recovery finalize"))
}

fn verification_grant(name: &'static str, phase: &'static str) -> Result<String, ExitError> {
    std::env::var(name).map_err(|_| {
        ExitError::new(
            "account_verification_required",
            3,
            format!("A short-lived Recovery {phase} verification grant is required."),
            format!("Set {name} for this process; never pass the grant on the command line."),
        )
    })
}

fn required(value: &str, flag: &'static str) -> Result<String, ExitError> {
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

fn command_result(
    action: &str,
    value: &impl Serialize,
    summary: String,
    warnings: Vec<String>,
) -> Result<CommandResult, ExitError> {
    let value = serde_json::to_value(value).map_err(|error| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize secret-free Handle Recovery result: {error}"),
            "Report this issue without including OTPs, grants, tokens, proofs, or private material.",
        )
    })?;
    Ok(CommandResult {
        data: serde_json::json!({"action": action, "result": value}),
        summary,
        warnings,
    })
}
