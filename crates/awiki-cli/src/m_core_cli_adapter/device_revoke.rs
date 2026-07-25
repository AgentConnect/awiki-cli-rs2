//! Safe CLI adapter for permanent AWiki-local device revocation.
//!
//! The adapter accepts only the selected identity and an opaque device ID.
//! Internal checkpoints, proofs, documents, generations and key material never
//! enter argv or CLI output.

use im_core::prelude::{DeviceRevokeRequest, IdentitySelector, ProtocolDeviceId};

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub(crate) async fn revoke_via_im_core_async<F>(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    target_device_id: &str,
    confirm: F,
) -> Result<CommandResult, ExitError>
where
    F: FnOnce(&str, &str) -> Result<(), ExitError>,
{
    require_rollout_enabled()?;
    let target_device_id = ProtocolDeviceId::parse(target_device_id.trim())
        .map_err(|err| super::map_im_error(err, "id device revoke"))?;
    let core = super::build_im_core_async(resolved).await?;
    let did = core
        .client_async(selector.clone())
        .await
        .map_err(|err| super::map_im_error(err, "id device revoke"))?
        .did()
        .as_str()
        .to_owned();
    confirm(&did, target_device_id.as_str())?;
    let result = core
        .device_revoke()
        .revoke(DeviceRevokeRequest {
            identity: selector,
            target_device_id,
            user_presence_confirmed: true,
        })
        .await
        .map_err(|err| super::map_im_error(err, "id device revoke"))?;
    let value = serde_json::to_value(&result).map_err(|err| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize device revoke result: {err}"),
            "Report this issue without including identity proofs or private key material.",
        )
    })?;
    Ok(CommandResult {
        data: serde_json::json!({
            "action": "device_revoke",
            "result": value,
        }),
        summary: format!("Revoked device {}", result.target_device_id.as_str()),
        warnings: vec![
            "Revocation protects future access; it cannot erase data already obtained by the device."
                .to_owned(),
            "Affected encrypted groups converge independently; group sending may remain paused until an owner device repairs each group."
                .to_owned(),
        ],
    })
}

pub(crate) fn require_rollout_enabled() -> Result<(), ExitError> {
    if super::vault::multi_device_device_revoke_enabled()? {
        return Ok(());
    }
    Err(ExitError::new(
        "unsupported_capability",
        2,
        "Permanent device revocation is disabled by the local rollout gate.",
        "Set AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED=1 only after Identity and Message revocation convergence is deployed.",
    ))
}
