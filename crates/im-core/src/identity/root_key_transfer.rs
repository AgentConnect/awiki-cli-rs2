//! Secret-free public boundary for one exact-device root-key transfer.
//!
//! Root material, P5 state, PreKeys, checkpoints, proofs, nonces and
//! ciphertext never cross this boundary.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const AUTHORIZATION_HANDLE_BYTES: usize = 32;
const AUTHORIZATION_HANDLE_LEN: usize = 43;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RootKeyTransferAuthorizationHandle(String);

impl RootKeyTransferAuthorizationHandle {
    pub(crate) fn from_generated(value: String) -> crate::ImResult<Self> {
        validate_authorization_handle(&value)?;
        Ok(Self(value))
    }

    pub(crate) fn expose_to_core(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RootKeyTransferAuthorizationHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RootKeyTransferAuthorizationHandle(<redacted>)")
    }
}

impl Serialize for RootKeyTransferAuthorizationHandle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RootKeyTransferAuthorizationHandle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_authorization_handle(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

fn validate_authorization_handle(value: &str) -> crate::ImResult<()> {
    if value.len() != AUTHORIZATION_HANDLE_LEN
        || URL_SAFE_NO_PAD
            .decode(value)
            .ok()
            .is_none_or(|bytes| bytes.len() != AUTHORIZATION_HANDLE_BYTES)
    {
        return Err(crate::ImError::invalid_input(
            Some("authorization_handle".to_owned()),
            "root transfer authorization handle must be 32-byte Base64URL without padding",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferPrepareRequest {
    pub recipient_device_id: crate::ids::ProtocolDeviceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferPreparation {
    pub authorization_handle: RootKeyTransferAuthorizationHandle,
    pub recipient: RootKeyTransferRecipientSummary,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferRecipientSummary {
    pub did: crate::ids::Did,
    pub device_id: crate::ids::ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub registry_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferSendRequest {
    pub authorization_handle: RootKeyTransferAuthorizationHandle,
    pub user_presence_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferSendResult {
    pub did: crate::ids::Did,
    pub sender_device_id: crate::ids::ProtocolDeviceId,
    pub recipient_device_id: crate::ids::ProtocolDeviceId,
    pub message_id: crate::ids::MessageId,
    pub accepted_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKeyTransferErrorCode {
    #[serde(rename = "root_transfer.unsupported")]
    Unsupported,
    #[serde(rename = "root_transfer.invalid_request")]
    InvalidRequest,
    #[serde(rename = "root_transfer.sender_not_eligible")]
    SenderNotEligible,
    #[serde(rename = "root_transfer.recipient_not_eligible")]
    RecipientNotEligible,
    #[serde(rename = "root_transfer.prekey_unavailable")]
    PrekeyUnavailable,
    #[serde(rename = "root_transfer.prekey_invalid")]
    PrekeyInvalid,
    #[serde(rename = "root_transfer.root_vault_unavailable")]
    RootVaultUnavailable,
    #[serde(rename = "root_transfer.authorization_invalid")]
    AuthorizationInvalid,
    #[serde(rename = "root_transfer.authorization_expired")]
    AuthorizationExpired,
    #[serde(rename = "root_transfer.authorization_already_consumed")]
    AuthorizationAlreadyConsumed,
    #[serde(rename = "root_transfer.user_presence_denied")]
    UserPresenceDenied,
    #[serde(rename = "root_transfer.state_changed")]
    StateChanged,
    #[serde(rename = "root_transfer.transport_pending")]
    TransportPending,
    #[serde(rename = "root_transfer.transport_rejected")]
    TransportRejected,
    #[serde(rename = "root_transfer.temporarily_unavailable")]
    TemporarilyUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootKeyTransferError {
    pub code: RootKeyTransferErrorCode,
    pub retryable: bool,
}

impl RootKeyTransferError {
    pub(crate) const fn new(code: RootKeyTransferErrorCode) -> Self {
        let retryable = matches!(
            code,
            RootKeyTransferErrorCode::PrekeyUnavailable
                | RootKeyTransferErrorCode::RootVaultUnavailable
                | RootKeyTransferErrorCode::AuthorizationExpired
                | RootKeyTransferErrorCode::StateChanged
                | RootKeyTransferErrorCode::TransportRejected
                | RootKeyTransferErrorCode::TemporarilyUnavailable
        );
        Self { code, retryable }
    }
}

impl fmt::Display for RootKeyTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = serde_json::to_value(self.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "root_transfer.temporarily_unavailable".to_owned());
        formatter.write_str(&encoded)
    }
}

impl std::error::Error for RootKeyTransferError {}

pub type RootKeyTransferResult<T> = Result<T, RootKeyTransferError>;

pub struct RootKeyTransferService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> RootKeyTransferService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    #[cfg(feature = "sqlite")]
    pub async fn prepare(
        &self,
        request: RootKeyTransferPrepareRequest,
    ) -> RootKeyTransferResult<RootKeyTransferPreparation> {
        crate::internal::identity_root_transfer_runtime::prepare_root_key_transfer(
            self.client,
            request,
        )
        .await
    }

    #[cfg(not(feature = "sqlite"))]
    pub async fn prepare(
        &self,
        _request: RootKeyTransferPrepareRequest,
    ) -> RootKeyTransferResult<RootKeyTransferPreparation> {
        Err(RootKeyTransferError::new(
            RootKeyTransferErrorCode::Unsupported,
        ))
    }

    #[cfg(feature = "sqlite")]
    pub async fn confirm_and_send(
        &self,
        request: RootKeyTransferSendRequest,
    ) -> RootKeyTransferResult<RootKeyTransferSendResult> {
        crate::internal::identity_root_transfer_runtime::confirm_and_send_root_key_transfer(
            self.client,
            request,
        )
        .await
    }

    #[cfg(not(feature = "sqlite"))]
    pub async fn confirm_and_send(
        &self,
        _request: RootKeyTransferSendRequest,
    ) -> RootKeyTransferResult<RootKeyTransferSendResult> {
        Err(RootKeyTransferError::new(
            RootKeyTransferErrorCode::Unsupported,
        ))
    }
}
