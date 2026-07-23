//! Safe CLI projection for the AWiki-local device Join control plane.
//!
//! Verification and Join grants are write-only inputs. This adapter never
//! returns them, and the one-time approval handle is created and consumed in
//! one foreground invocation instead of crossing the CLI output boundary.

use im_core::prelude::{
    DeviceJoinAccountVerificationGrant, DeviceJoinBeginRequest, DeviceJoinConfirmApprovalRequest,
    DeviceJoinProgress, DeviceJoinRejectReason, Did, IdentitySelector,
};
use serde::Serialize;

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

const ACCOUNT_VERIFICATION_TOKEN_ENV: &str = "AWIKI_ACCOUNT_VERIFICATION_TOKEN";

pub async fn local_sessions_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CommandResult, ExitError> {
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

pub async fn poll_new_device_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    join_session_id: &str,
    display_sas: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str) -> Result<(), ExitError>,
{
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .poll_new_device_join(&join_session_id)
        .await
        .map_err(|err| super::map_im_error(err, "id device join poll"))?;
    poll_progress_result(progress, display_sas)
}

pub async fn registry_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
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

pub async fn local_requests_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
    let core = super::build_im_core_async(resolved).await?;
    let requests = core
        .device_join()
        .local_device_join_requests(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id device join requests"))?;
    command_result(
        "device_join_requests",
        &requests,
        format!("Loaded {} local device Join request(s)", requests.len()),
    )
}

pub async fn start_verification_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    operation_id: &str,
    challenge_ttl_seconds: u64,
) -> Result<CommandResult, ExitError> {
    let join_session_id = required_value(join_session_id, "--session")?;
    let operation_id = required_value(operation_id, "--operation-id")?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .start_device_join_verification(
            selector,
            &join_session_id,
            &operation_id,
            challenge_ttl_seconds,
        )
        .await
        .map_err(|err| super::map_im_error(err, "id device join verify"))?;
    progress_result(
        "device_join_verify",
        progress,
        "Device Join verification started",
    )
}

pub async fn reject_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    reason: &str,
) -> Result<CommandResult, ExitError> {
    let join_session_id = required_value(join_session_id, "--session")?;
    let reason = reject_reason_from_cli(reason)?;
    let core = super::build_im_core_async(resolved).await?;
    let progress = core
        .device_join()
        .reject_device_join(selector, &join_session_id, reason)
        .await
        .map_err(|err| super::map_im_error(err, "id device join reject"))?;
    progress_result("device_join_reject", progress, "Device Join rejected")
}

pub(crate) async fn approve_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    join_session_id: &str,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str) -> Result<(), ExitError>,
{
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let prompt = core
        .device_join()
        .prepare_device_join_approval(selector, &join_session_id, true)
        .map_err(|err| super::map_im_error(err, "id device join approve"))?;
    confirm(&prompt.sas)?;
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
    join_session_id: &str,
) -> Result<CommandResult, ExitError> {
    let join_session_id = required_value(join_session_id, "--session")?;
    let core = super::build_im_core_async(resolved).await?;
    let session = core
        .device_join()
        .cancel_new_device_join(&join_session_id)
        .await
        .map_err(|err| super::map_im_error(err, "id device join cancel"))?;
    command_result(
        "device_join_cancel",
        &session,
        "Device Join session cancelled".to_owned(),
    )
}

fn reject_reason_from_cli(value: &str) -> Result<DeviceJoinRejectReason, ExitError> {
    match value.trim() {
        "" | "user-rejected" => Ok(DeviceJoinRejectReason::UserRejected),
        "sas-mismatch" => Ok(DeviceJoinRejectReason::SasMismatch),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            "--reason must be user-rejected or sas-mismatch.",
            "Use user-rejected for an explicit refusal, or sas-mismatch when the displayed codes differ.",
        )),
    }
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
    let value = remove_sas(serde_json::to_value(progress).map_err(serialization_error)?);
    command_value_result(action, value, summary.to_owned())
}

fn poll_progress_result<F>(
    progress: DeviceJoinProgress,
    display_sas: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str) -> Result<(), ExitError>,
{
    if let Some(sas) = progress.sas.as_deref() {
        display_sas(sas)?;
    }
    progress_result("device_join_poll", progress, "Device Join state refreshed")
}

fn remove_sas(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("sas");
    }
    value
}

fn command_result(
    action: &str,
    value: &impl Serialize,
    summary: String,
) -> Result<CommandResult, ExitError> {
    let value = serde_json::to_value(value).map_err(serialization_error)?;
    command_value_result(action, value, summary)
}

fn command_value_result(
    action: &str,
    value: serde_json::Value,
    summary: String,
) -> Result<CommandResult, ExitError> {
    Ok(CommandResult {
        data: serde_json::json!({
            "action": action,
            "result": value,
        }),
        summary,
        warnings: Vec::new(),
    })
}

fn serialization_error(err: serde_json::Error) -> ExitError {
    ExitError::new(
        "serialization_error",
        1,
        format!("serialize device Join result: {err}"),
        "Report this issue without including verification grants or private key material.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress_with_sas() -> DeviceJoinProgress {
        serde_json::from_value(serde_json::json!({
            "session": {
                "join_session_id": "join-1",
                "did": "did:wba:example.test:alice",
                "protocol_device_id": "device-1",
                "side": "new_device",
                "phase": "response_prepared",
                "expires_at": "2026-07-24T00:00:00Z"
            },
            "remote_state": "response_verified",
            "sas": "012345",
            "authorized_device": null
        }))
        .unwrap()
    }

    #[test]
    fn progress_projection_never_emits_sas() {
        let value = remove_sas(serde_json::json!({
            "session": {"join_session_id": "join-1"},
            "remote_state": "response_verified",
            "sas": "012345",
            "authorized_device": null
        }));

        let result =
            command_value_result("device_join_poll", value, "refreshed".to_owned()).unwrap();
        let encoded = serde_json::to_string(&result.data).unwrap();
        assert!(!encoded.contains("012345"));
        assert!(result.data["result"].get("sas").is_none());
    }

    #[test]
    fn poll_displays_sas_through_callback_but_not_command_result() {
        let mut displayed = String::new();

        let result = poll_progress_result(progress_with_sas(), |sas| {
            displayed.push_str(sas);
            Ok(())
        })
        .unwrap();

        assert_eq!(displayed, "012345");
        let encoded = serde_json::to_string(&result.data).unwrap();
        assert!(!encoded.contains("012345"));
        assert!(result.data["result"].get("sas").is_none());
    }

    #[test]
    fn poll_without_sas_does_not_invoke_callback() {
        let mut progress = progress_with_sas();
        progress.sas = None;

        let result = poll_progress_result(progress, |_| {
            panic!("callback must not run when Core returned no SAS");
        })
        .unwrap();

        assert!(result.data["result"].get("sas").is_none());
    }
}
