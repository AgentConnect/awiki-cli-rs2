//! Safe CLI adapter for one exact-device root-key transfer.
//!
//! The adapter accepts only the public recipient device identifier. Secret
//! material and Core-internal state never enter argv or CLI output.

use im_core::prelude::{
    IdentitySelector, ProtocolDeviceId, RootKeyTransferErrorCode, RootKeyTransferPrepareRequest,
    RootKeyTransferSendRequest,
};
use serde::Serialize;

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub(crate) async fn send_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    recipient_device_id: &str,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str, &str, &str, &str, &str) -> Result<(), ExitError>,
{
    let recipient_device_id = ProtocolDeviceId::parse(recipient_device_id.trim())
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?;
    let core = super::build_im_core_async(resolved).await?;
    let client = core
        .client_async(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?;
    let preparation = client
        .root_key_transfer()
        .prepare(RootKeyTransferPrepareRequest {
            recipient_device_id,
        })
        .await
        .map_err(map_root_transfer_error)?;
    confirm(
        preparation.recipient.did.as_str(),
        preparation.recipient.device_id.as_str(),
        &preparation.recipient.signing_key_id,
        &preparation.recipient.e2ee_key_id,
        &preparation.expires_at,
    )?;
    let result = client
        .root_key_transfer()
        .confirm_and_send(RootKeyTransferSendRequest {
            authorization_handle: preparation.authorization_handle,
            user_presence_confirmed: true,
        })
        .await
        .map_err(map_root_transfer_error)?;
    command_result(
        "root_key_transfer_send",
        &result,
        "根密钥已发送".to_owned(),
        Vec::new(),
    )
}

fn map_root_transfer_error(error: im_core::identity::RootKeyTransferError) -> ExitError {
    let code = error.to_string();
    let hint = match error.code {
        RootKeyTransferErrorCode::TransportPending => {
            "Core has accepted responsibility for recovery. Do not prepare or send another transfer."
        }
        _ if error.retryable => {
            "Retry this command after the temporary condition is resolved."
        }
        _ => "Verify that this device and the selected recipient are still eligible.",
    };
    ExitError::new(
        &code,
        if error.retryable { 3 } else { 4 },
        "Root-key transfer did not complete.",
        hint,
    )
}

fn command_result(
    action: &str,
    value: &impl Serialize,
    summary: String,
    warnings: Vec<String>,
) -> Result<CommandResult, ExitError> {
    let value = serde_json::to_value(value).map_err(|err| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize root-key transfer result: {err}"),
            "Report this issue without including private identity material.",
        )
    })?;
    Ok(CommandResult {
        data: serde_json::json!({
            "action": action,
            "result": value,
        }),
        summary,
        warnings,
    })
}
