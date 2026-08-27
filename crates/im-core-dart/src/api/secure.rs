use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    secure::{
        DartDirectSecureStatus, DartGroupSecurePrepareResult, DartGroupSecureRepairResult,
        DartGroupSecureStatus,
    },
};

pub async fn secure_direct_status(
    client: &Arc<crate::api::client::DartImClient>,
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

pub async fn secure_group_status(
    client: &Arc<crate::api::client::DartImClient>,
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
    client: &Arc<crate::api::client::DartImClient>,
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
    client: &Arc<crate::api::client::DartImClient>,
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
