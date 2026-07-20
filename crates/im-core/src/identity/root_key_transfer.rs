//! Safe public boundary for AWiki management-device root-key transfer.
//!
//! The encrypted control envelope, root private key, Direct binding, private
//! transport metadata, and internal identity checkpoints are intentionally not
//! represented in these DTOs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootKeyTransferSendRequest {
    pub identity: super::IdentitySelector,
    pub recipient_device_id: crate::ids::ProtocolDeviceId,
    /// Standard Direct message ID and idempotency key. There is no separate
    /// root-key `transfer_id`.
    pub message_id: crate::ids::MessageId,
    /// The host sets this only after foreground OS/user-presence confirmation.
    /// Core records the confirmation time internally.
    pub user_presence_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootKeyTransferSendResult {
    pub did: crate::ids::Did,
    pub sender_device_id: crate::ids::ProtocolDeviceId,
    pub recipient_device_id: crate::ids::ProtocolDeviceId,
    pub message_id: crate::ids::MessageId,
    pub accepted_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKeyTransferStatus {
    PendingDelivery,
    AwaitingImport,
    Importing,
    Failed,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootKeyTransferSummary {
    pub did: crate::ids::Did,
    pub message_id: crate::ids::MessageId,
    pub sender_device_id: crate::ids::ProtocolDeviceId,
    pub recipient_device_id: crate::ids::ProtocolDeviceId,
    pub status: RootKeyTransferStatus,
    pub created_at: String,
    pub accepted_at: Option<String>,
    pub completed_at: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootKeyTransferListRequest {
    pub identity: super::IdentitySelector,
    pub include_completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootKeyTransferRetryRequest {
    pub identity: super::IdentitySelector,
    /// Selects the exact persisted ciphertext/sidecar. The recipient is never
    /// accepted from the host on a retry.
    pub message_id: crate::ids::MessageId,
    pub user_presence_confirmed: bool,
}

pub struct RootKeyTransferService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> RootKeyTransferService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    #[cfg(feature = "sqlite")]
    pub async fn send(
        &self,
        request: RootKeyTransferSendRequest,
    ) -> crate::ImResult<RootKeyTransferSendResult> {
        if !self.core.inner().root_key_transfer_enabled() {
            return Err(crate::ImError::unsupported(
                "awiki-root-key-transfer-disabled",
            ));
        }
        if !request.user_presence_confirmed {
            return Err(crate::ImError::PermissionDenied);
        }
        let client = self.core.client_async(request.identity).await?;
        let delivery = crate::internal::identity_root_transfer_runtime::send_root_key(
            self.core,
            &client,
            request.recipient_device_id.as_str(),
            request.message_id.as_str(),
            true,
        )
        .await?;
        Ok(RootKeyTransferSendResult {
            did: crate::ids::Did::parse(delivery.did)?,
            sender_device_id: crate::ids::ProtocolDeviceId::parse(delivery.sender_device_id)?,
            recipient_device_id: crate::ids::ProtocolDeviceId::parse(delivery.recipient_device_id)?,
            message_id: crate::ids::MessageId::parse(delivery.message_id)?,
            accepted_at: delivery.accepted_at,
        })
    }

    #[cfg(feature = "sqlite")]
    pub async fn list(
        &self,
        request: RootKeyTransferListRequest,
    ) -> crate::ImResult<Vec<RootKeyTransferSummary>> {
        if !self.core.inner().root_key_transfer_enabled() {
            return Err(crate::ImError::unsupported(
                "awiki-root-key-transfer-disabled",
            ));
        }
        let client = self.core.client_async(request.identity).await?;
        let mut summaries =
            crate::internal::identity_root_transfer_runtime::list_root_key_transfers(
                self.core, &client,
            )?
            .into_iter()
            .map(|status| map_status(client.did(), status))
            .collect::<crate::ImResult<Vec<_>>>()?;
        if !request.include_completed {
            summaries.retain(|summary| summary.status != RootKeyTransferStatus::Completed);
        }
        Ok(summaries)
    }

    #[cfg(not(feature = "sqlite"))]
    pub async fn list(
        &self,
        _request: RootKeyTransferListRequest,
    ) -> crate::ImResult<Vec<RootKeyTransferSummary>> {
        Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-requires-sqlite",
        ))
    }

    #[cfg(feature = "sqlite")]
    pub async fn retry(
        &self,
        request: RootKeyTransferRetryRequest,
    ) -> crate::ImResult<RootKeyTransferSummary> {
        if !self.core.inner().root_key_transfer_enabled() {
            return Err(crate::ImError::unsupported(
                "awiki-root-key-transfer-disabled",
            ));
        }
        if !request.user_presence_confirmed {
            return Err(crate::ImError::PermissionDenied);
        }
        let client = self.core.client_async(request.identity).await?;
        let status = crate::internal::identity_root_transfer_runtime::retry_root_key_transfer(
            self.core,
            &client,
            request.message_id.as_str(),
            true,
        )
        .await?;
        map_status(client.did(), status)
    }

    #[cfg(not(feature = "sqlite"))]
    pub async fn retry(
        &self,
        _request: RootKeyTransferRetryRequest,
    ) -> crate::ImResult<RootKeyTransferSummary> {
        Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-requires-sqlite",
        ))
    }

    #[cfg(not(feature = "sqlite"))]
    pub async fn send(
        &self,
        _request: RootKeyTransferSendRequest,
    ) -> crate::ImResult<RootKeyTransferSendResult> {
        Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-requires-sqlite",
        ))
    }
}

#[cfg(feature = "sqlite")]
fn map_status(
    did: &crate::ids::Did,
    status: crate::internal::secure_direct::v2_store::V2PrivateOutboundStatus,
) -> crate::ImResult<RootKeyTransferSummary> {
    use crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase;
    Ok(RootKeyTransferSummary {
        did: did.clone(),
        message_id: crate::ids::MessageId::parse(status.operation_id)?,
        sender_device_id: crate::ids::ProtocolDeviceId::parse(status.sender_device_id)?,
        recipient_device_id: crate::ids::ProtocolDeviceId::parse(status.recipient_device_id)?,
        status: match status.phase {
            V2PrivateOutboundPhase::PendingDelivery => RootKeyTransferStatus::PendingDelivery,
            V2PrivateOutboundPhase::AwaitingImport => RootKeyTransferStatus::AwaitingImport,
            V2PrivateOutboundPhase::Importing => RootKeyTransferStatus::Importing,
            V2PrivateOutboundPhase::Failed => RootKeyTransferStatus::Failed,
            V2PrivateOutboundPhase::Completed => RootKeyTransferStatus::Completed,
        },
        created_at: status.created_at,
        accepted_at: status.accepted_at,
        completed_at: status.completed_at,
        retryable: status.retryable,
    })
}
