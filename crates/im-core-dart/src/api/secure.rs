use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    secure::{
        DartDirectSecurePrepareResult, DartDirectSecureRepairResult, DartDirectSecureStatus,
        DartGroupSecurePrepareResult, DartGroupSecureRepairResult, DartGroupSecureStatus,
        DartSecureOutboxEntry, DartSecureOutboxResult,
    },
};

pub async fn secure_direct_status(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecureStatus, DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .secure()
        .direct(peer)
        .status_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_direct_prepare(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecurePrepareResult, DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .secure()
        .direct(peer)
        .prepare_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_direct_repair(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecureRepairResult, DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .secure()
        .direct(peer)
        .repair_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_group_status(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecureStatus, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
    inner
        .secure()
        .group(group)
        .status_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_group_prepare(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecurePrepareResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
    inner
        .secure()
        .group(group)
        .prepare_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_group_repair(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecureRepairResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
    inner
        .secure()
        .group(group)
        .repair_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_outbox_list_failed(
    client: Arc<crate::api::client::DartImClient>,
) -> Result<Vec<DartSecureOutboxEntry>, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .secure()
        .outbox()
        .list_failed_async()
        .await
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn secure_outbox_retry(
    client: Arc<crate::api::client::DartImClient>,
    outbox_id: String,
) -> Result<DartSecureOutboxResult, DartImError> {
    let inner = client.clone_inner()?;
    let outbox_id = im_core::secure::SecureOutboxId::parse(outbox_id).map_err(DartImError::from)?;
    inner
        .secure()
        .outbox()
        .retry_async(outbox_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn secure_outbox_drop(
    client: Arc<crate::api::client::DartImClient>,
    outbox_id: String,
) -> Result<DartSecureOutboxResult, DartImError> {
    let inner = client.clone_inner()?;
    let outbox_id = im_core::secure::SecureOutboxId::parse(outbox_id).map_err(DartImError::from)?;
    inner
        .secure()
        .outbox()
        .drop_async(outbox_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
