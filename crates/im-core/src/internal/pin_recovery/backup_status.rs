//! Read-only current backup metadata for an already authenticated local device.
//! This never exchanges factors or grants, creates an operation, or guesses a PIN.
use super::{failure, PinRecoveryBackupState, PinRecoveryBackupStatus, PinRecoveryError};
use crate::internal::transport::{AsyncAuthenticatedRestTransport, CoreHttpTransport};
use serde::Deserialize;
use zeroize::Zeroizing;

const ENDPOINT: &str = "/user-service/v1/key-recovery/device-status";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceStatus {
    did: String,
    full_handle: String,
    binding_generation: String,
    state: String,
    active_epoch: Option<u64>,
}
pub(crate) async fn read(
    core: &crate::ImCore,
    selector: crate::IdentitySelector,
) -> crate::ImResult<PinRecoveryBackupStatus> {
    let identity = core.identities().resolve(selector.clone())?;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let local_generation = index
        .credentials
        .values()
        .find(|entry| entry.unique_id == identity.id.as_str() && entry.did == identity.did.as_str())
        .and_then(|entry| entry.binding_generation.clone());
    let mut status = PinRecoveryBackupStatus {
        did: identity.did.as_str().into(),
        full_handle: identity.handle.as_ref().map(|h| h.as_str().into()),
        binding_generation: local_generation.clone(),
        state: PinRecoveryBackupState::Unknown,
        active_epoch: None,
        error: None,
    };
    let result = async {
        let client = core.client_async(selector).await?;
        let value = fetch(&client).await?;
        project(value, &status.did, local_generation.as_deref())
    }
    .await;
    match result {
        Ok(current) => Ok(current),
        Err(error) => {
            status.error = Some(project_error(&error));
            Ok(status)
        }
    }
}
async fn fresh_token(client: &crate::ImClient) -> crate::ImResult<Zeroizing<String>> {
    CoreHttpTransport::new_signature_only(client)
        .refresh_jwt_async()
        .await
        .map(Zeroizing::new)
}
async fn fetch(client: &crate::ImClient) -> crate::ImResult<serde_json::Value> {
    let mut token = match client.runtime().key_provider.valid_auth_token()? {
        Some(token) => Zeroizing::new(token),
        None => fresh_token(client).await?,
    };
    for attempt in 0..2 {
        // Ephemeral bearer mode suppresses HTTP-signature fallback at this
        // Bearer-only endpoint. Renewal uses the existing authenticated get_me.
        let mut transport = CoreHttpTransport::new_with_ephemeral_bearer(client, &token)?;
        match transport
            .authenticated_rest_get(ENDPOINT, "GET", &Default::default())
            .await
        {
            Err(crate::ImError::Service {
                status_code: Some(401),
                ..
            }) if attempt == 0 => {
                token = fresh_token(client).await?;
            }
            result => return result,
        }
    }
    Err(crate::ImError::AuthRequired)
}
fn project(
    value: serde_json::Value,
    expected_did: &str,
    local_generation: Option<&str>,
) -> crate::ImResult<PinRecoveryBackupStatus> {
    let response: DeviceStatus =
        serde_json::from_value(value).map_err(|_| failure("invalid_wire", false))?;
    if response.did != expected_did || !canonical_generation(&response.binding_generation) {
        return Err(failure("binding_changed", false));
    }
    let full =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&response.full_handle)?;
    if full.full != response.full_handle {
        return Err(failure("invalid_wire", false));
    }
    if let Some(local) = local_generation {
        if !canonical_generation(local)
            || response.binding_generation.len() < local.len()
            || (response.binding_generation.len() == local.len()
                && response.binding_generation.as_str() < local)
        {
            return Err(failure("binding_changed", false));
        }
    }
    let state = match response.state.as_str() {
        "not_enabled" => PinRecoveryBackupState::NotEnabled,
        "pending" => PinRecoveryBackupState::Pending,
        "active" => PinRecoveryBackupState::Active,
        _ => return Err(failure("invalid_wire", false)),
    };
    if (state == PinRecoveryBackupState::Active) != response.active_epoch.is_some()
        || response.active_epoch == Some(0)
    {
        return Err(failure("invalid_wire", false));
    }
    Ok(PinRecoveryBackupStatus {
        did: response.did,
        full_handle: Some(response.full_handle),
        binding_generation: Some(response.binding_generation),
        state,
        active_epoch: response.active_epoch,
        error: None,
    })
}
fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
}
fn project_error(error: &crate::ImError) -> PinRecoveryError {
    let (code, retryable) = match error {
        crate::ImError::Service {
            code: Some(code),
            status_code,
            ..
        } if code.len() <= 80 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => (
            code.clone(),
            status_code.is_some_and(|s| s == 429 || s >= 500),
        ),
        crate::ImError::TransportUnavailable { .. } => ("network_unavailable".into(), true),
        crate::ImError::UnsupportedCapability { .. } => ("unsupported_identity".into(), false),
        crate::ImError::AuthRequired
        | crate::ImError::SessionExpired
        | crate::ImError::PermissionDenied => ("invalid_device_session".into(), false),
        _ => ("backup_status_unavailable".into(), true),
    };
    PinRecoveryError {
        code,
        retryable,
        retry_after_seconds: None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn value(state: &str, epoch: Option<u64>) -> serde_json::Value {
        json!({"did":"did:wba:example.com:user:alice","full_handle":"alice.example.com","binding_generation":"184467440737095516160","state":state,"active_epoch":epoch})
    }
    #[test]
    fn current_status_is_exact_and_never_infers_not_enabled_from_absence() {
        let did = "did:wba:example.com:user:alice";
        assert_eq!(
            project(value("active", Some(2)), did, Some("9"))
                .unwrap()
                .state,
            PinRecoveryBackupState::Active
        );
        assert_eq!(
            project(value("not_enabled", None), did, None)
                .unwrap()
                .state,
            PinRecoveryBackupState::NotEnabled
        );
        assert!(project(value("active", None), did, None).is_err());
        assert!(project(value("pending", Some(2)), did, None).is_err());
        assert!(project(
            value("not_enabled", None),
            "did:wba:example.com:user:bob",
            None
        )
        .is_err());
        assert!(project(value("active", Some(2)), did, Some("184467440737095516161")).is_err());
    }
    #[test]
    fn offline_and_unsupported_are_explicit_errors_for_unknown_state() {
        assert_eq!(
            project_error(&crate::ImError::TransportUnavailable {
                detail: "bounded network error".into()
            })
            .code,
            "network_unavailable"
        );
        assert_eq!(
            project_error(&crate::ImError::unsupported("rootless_web")).code,
            "unsupported_identity"
        );
    }
}
