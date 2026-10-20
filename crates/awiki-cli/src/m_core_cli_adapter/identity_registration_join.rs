//! Consume registration Join preparations before their owning Core process exits.

use super::*;
use im_core::identity::{BeginPreparedRegistrationDeviceJoinRequest, HandleRegistrationJoinMode};
use rand::RngCore as _;

pub(super) async fn begin(
    core: &im_core::ImCore,
    result: &HandleRegistrationResult,
) -> Result<CommandResult, ExitError> {
    let request = begin_request(result)?;
    let progress = core
        .handle_recovery()
        .begin_prepared_registration_device_join(request)
        .await
        .map_err(|error| super::super::map_im_error(error, "id register device Join"))?;
    command_result(result, progress.join)
}

fn command_result(
    result: &HandleRegistrationResult,
    progress: im_core::identity::DeviceJoinProgress,
) -> Result<CommandResult, ExitError> {
    let authorized = progress.session.phase == im_core::identity::DeviceJoinLocalPhase::Authorized;
    let mut output = super::super::device_join::progress_result(
        "device_join_start",
        progress,
        if authorized {
            "Device Join authorized. Run id device join poll --session <join_session_id> to reconcile this device."
        } else {
            "Existing account verified; device Join started. Approve the request on an existing management device, then run id device join poll --session <join_session_id>."
        },
    )?;
    output.data["verification_state"] = json!(if authorized {
        "join_authorized"
    } else {
        "join_pending"
    });
    output.data["full_handle"] = json!(result.handle.as_str());
    output.warnings.extend(result.warnings.clone());
    Ok(output)
}

pub(super) fn begin_blocking(
    core: &im_core::ImCore,
    result: &HandleRegistrationResult,
) -> Result<CommandResult, ExitError> {
    // Registration and continuation share this Core, including its opaque store.
    // A scoped worker also keeps callers with an active Tokio runtime supported.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("cli-registration-join".into())
            .spawn_scoped(scope, || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| continuation_unavailable())?
                    .block_on(begin(core, result))
            })
            .map_err(|_| continuation_unavailable())?
            .join()
            .map_err(|_| continuation_unavailable())?
    })
}

fn begin_request(
    result: &HandleRegistrationResult,
) -> Result<BeginPreparedRegistrationDeviceJoinRequest, ExitError> {
    let preparation = result
        .join_required
        .as_ref()
        .filter(|preparation| {
            result.state == HandleRegistrationState::JoinRequired
                && !preparation.preparation_id.trim().is_empty()
                && preparation.full_handle == result.handle
        })
        .ok_or_else(continuation_unavailable)?;
    if preparation.mode != HandleRegistrationJoinMode::Ordinary
        || preparation.requires_user_presence
    {
        return Err(ExitError::new(
            "registration_join_confirmation_required",
            3,
            "This account requires explicit Recovery confirmation before device Join.",
            "Use the App recovery flow to review and confirm this account transition. No device Join was started.",
        ));
    }
    let mut nonce = [0_u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| continuation_unavailable())?;
    let operation_id = format!(
        "cli-register-join-{}",
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    Ok(BeginPreparedRegistrationDeviceJoinRequest {
        preparation_id: preparation.preparation_id.clone(),
        operation_id,
        ttl_seconds: 600,
        user_presence_confirmed: false,
    })
}

fn continuation_unavailable() -> ExitError {
    ExitError::new(
        "registration_join_unavailable",
        5,
        "Unable to continue the verified account into device Join.",
        "Keep this workspace. Inspect id device join sessions before retrying phone verification; do not export account verification material.",
    )
}

#[cfg(test)]
#[path = "identity_registration_join_tests.rs"]
mod tests;
