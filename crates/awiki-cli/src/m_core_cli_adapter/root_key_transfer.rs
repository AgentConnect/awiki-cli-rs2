//! Safe CLI adapter for AWiki-local management-device root-key transfer.
//!
//! The adapter accepts only public device/message identifiers. Root material,
//! encrypted-inner JSON, completion proofs, and internal checkpoints never
//! enter argv or CLI output.

use im_core::prelude::{
    IdentitySelector, MessageId, ProtocolDeviceId, RootKeyTransferListRequest,
    RootKeyTransferRetryRequest, RootKeyTransferSendRequest,
};
use serde::Serialize;

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub(crate) async fn send_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    recipient_device_id: &str,
    message_id: &str,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str, &str) -> Result<(), ExitError>,
{
    require_rollout_enabled()?;
    let recipient_device_id = ProtocolDeviceId::parse(recipient_device_id.trim())
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?;
    let message_id = MessageId::parse(message_id.trim())
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?;
    let core = super::build_im_core_async(resolved).await?;
    let did = core
        .client_async(selector.clone())
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?
        .did()
        .as_str()
        .to_owned();
    confirm(&did, recipient_device_id.as_str())?;
    let result = core
        .root_key_transfer()
        .send(RootKeyTransferSendRequest {
            identity: selector,
            recipient_device_id,
            message_id,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key send"))?;
    command_result(
        "root_key_transfer_send",
        &result,
        "Encrypted root-key control accepted for the target device".to_owned(),
        vec![
            "Delivery acceptance does not mean the recipient has imported the root key yet."
                .to_owned(),
        ],
    )
}

pub(crate) async fn list_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    include_completed: bool,
) -> Result<CommandResult, ExitError> {
    require_rollout_enabled()?;
    let core = super::build_im_core_async(resolved).await?;
    let result = core
        .root_key_transfer()
        .list(RootKeyTransferListRequest {
            identity: selector,
            include_completed,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key list"))?;
    command_result(
        "root_key_transfer_list",
        &result,
        format!("{} local root-key transfer operation(s)", result.len()),
        Vec::new(),
    )
}

pub(crate) async fn retry_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    message_id: &str,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str, &str) -> Result<(), ExitError>,
{
    require_rollout_enabled()?;
    let message_id = MessageId::parse(message_id.trim())
        .map_err(|err| super::map_im_error(err, "id device root-key retry"))?;
    let core = super::build_im_core_async(resolved).await?;
    let did = core
        .client_async(selector.clone())
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key retry"))?
        .did()
        .as_str()
        .to_owned();
    confirm(&did, message_id.as_str())?;
    let result = core
        .root_key_transfer()
        .retry(RootKeyTransferRetryRequest {
            identity: selector,
            message_id,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device root-key retry"))?;
    command_result(
        "root_key_transfer_retry",
        &result,
        "Retried the exact persisted root-key control operation".to_owned(),
        vec![
            "Retry never changes the recipient or plaintext and may still await recipient import."
                .to_owned(),
        ],
    )
}

pub(crate) fn require_rollout_enabled() -> Result<(), ExitError> {
    if super::vault::multi_device_root_transfer_enabled()? {
        return Ok(());
    }
    Err(ExitError::new(
        "unsupported_capability",
        2,
        "Management-device root-key transfer is disabled by the local rollout gate.",
        "Set AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED=1 only after P5 v2 device sessions and private root-control routing are deployed.",
    ))
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
            "Report this issue without including private key material or encrypted control payloads.",
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
