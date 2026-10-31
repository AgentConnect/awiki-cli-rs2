use crate::dto::{error::DartImError, identity::DartIdentitySelector, pin_recovery::*};
use im_core::pin_recovery as p;
use std::sync::Arc;
fn presence(confirmed: bool) -> Result<p::UserPresence, DartImError> {
    if !confirmed {
        return Err(DartImError::invalid_input(
            Some("user_presence_confirmed".into()),
            "Explicit user confirmation is required",
        ));
    }
    Ok(p::UserPresence::confirmed())
}
pub async fn request_pin_recovery_phone_code(
    core: &Arc<crate::api::core::DartImCore>,
    tenant_id: String,
    phone: String,
    resume_operation_id: Option<String>,
) -> Result<DartPhoneChallengeSummary, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .request_phone_code(p::PhoneChallengeRequest {
            tenant_id,
            phone: phone.into(),
            resume_operation_id: resume_operation_id.map(p::PinRecoveryOperationId),
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn verify_pin_recovery_phone_code(
    core: &Arc<crate::api::core::DartImCore>,
    challenge_handle: String,
    code: String,
) -> Result<DartDiscoverySessionSummary, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .verify_phone_code(p::PhoneVerifyRequest {
            challenge: p::PhoneChallengeHandle(challenge_handle),
            code: p::SecretOtp::new(code.into_bytes()).map_err(DartImError::from)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn list_pin_recovery_targets(
    core: &Arc<crate::api::core::DartImCore>,
    discovery_handle: String,
) -> Result<Vec<DartPinRecoveryTarget>, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .list_targets(p::DiscoverySessionHandle(discovery_handle))
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}
pub async fn select_pin_recovery_target(
    core: &Arc<crate::api::core::DartImCore>,
    discovery_handle: String,
    target_id: String,
) -> Result<DartPhoneSessionSummary, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .select_target(
            p::DiscoverySessionHandle(discovery_handle),
            p::RecoveryTargetId(target_id),
        )
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn begin_enable_pin_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    session_handle: String,
    selector: DartIdentitySelector,
    user_presence_confirmed: bool,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .begin_enable(p::EnablePinRecoveryRequest {
            session: p::PhoneSessionHandle(session_handle),
            identity: selector.try_into()?,
            user_presence: presence(user_presence_confirmed)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn begin_pin_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    session_handle: String,
    device_label: String,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .begin_recover(p::BeginPinRecoveryRequest {
            session: p::PhoneSessionHandle(session_handle),
            device_label,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn submit_pin_recovery_pin(
    core: &Arc<crate::api::core::DartImCore>,
    operation_id: String,
    pin: String,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .submit_pin(
            p::PinRecoveryOperationId(operation_id),
            p::SecretPin::new(pin.into_bytes()).map_err(DartImError::from)?,
        )
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn confirm_pin_recovery_device(
    core: &Arc<crate::api::core::DartImCore>,
    operation_id: String,
    user_presence_confirmed: bool,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .confirm_device(p::ConfirmRecoveredDeviceRequest {
            operation_id: p::PinRecoveryOperationId(operation_id),
            user_presence: presence(user_presence_confirmed)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn list_pending_pin_recovery_operations(
    core: &Arc<crate::api::core::DartImCore>,
) -> Result<Vec<DartPinRecoveryProgress>, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .list_pending()
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}
pub async fn pin_recovery_status(
    core: &Arc<crate::api::core::DartImCore>,
    operation_id: String,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .status(p::PinRecoveryOperationId(operation_id))
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn resume_pin_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    operation_id: String,
    session_handle: Option<String>,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .resume(p::ResumePinRecoveryRequest {
            operation_id: p::PinRecoveryOperationId(operation_id),
            session: session_handle.map(p::PhoneSessionHandle),
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
pub async fn cancel_pin_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    operation_id: String,
) -> Result<DartPinRecoveryProgress, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .cancel(p::PinRecoveryOperationId(operation_id))
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn pin_recovery_backup_status(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartPinRecoveryBackupStatus, DartImError> {
    core.clone_inner()?
        .pin_recovery()
        .backup_status(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
