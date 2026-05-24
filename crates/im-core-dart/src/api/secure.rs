use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    secure::{
        DartDirectSecurePrepareResult, DartDirectSecureRepairResult, DartDirectSecureStatus,
        DartGroupSecurePrepareResult, DartGroupSecureRepairResult, DartGroupSecureStatus,
        DartSecureOutboxEntry, DartSecureOutboxResult,
    },
};

pub fn secure_direct_status(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecureStatus, DartImError> {
    client.with_inner(|inner| {
        let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
        inner
            .secure()
            .direct(peer)
            .status()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_direct_prepare(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecurePrepareResult, DartImError> {
    client.with_inner(|inner| {
        let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
        inner
            .secure()
            .direct(peer)
            .prepare()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_direct_repair(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectSecureRepairResult, DartImError> {
    client.with_inner(|inner| {
        let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
        inner
            .secure()
            .direct(peer)
            .repair()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_group_status(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecureStatus, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
        inner
            .secure()
            .group(group)
            .status()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_group_prepare(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecurePrepareResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
        inner
            .secure()
            .group(group)
            .prepare()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_group_repair(
    client: Arc<crate::api::client::DartImClient>,
    group: String,
) -> Result<DartGroupSecureRepairResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group).map_err(DartImError::from)?;
        inner
            .secure()
            .group(group)
            .repair()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_outbox_list_failed(
    client: Arc<crate::api::client::DartImClient>,
) -> Result<Vec<DartSecureOutboxEntry>, DartImError> {
    client.with_inner(|inner| {
        inner
            .secure()
            .outbox()
            .list_failed()
            .map(|entries| entries.into_iter().map(Into::into).collect())
            .map_err(DartImError::from)
    })
}

pub fn secure_outbox_retry(
    client: Arc<crate::api::client::DartImClient>,
    outbox_id: String,
) -> Result<DartSecureOutboxResult, DartImError> {
    client.with_inner(|inner| {
        let outbox_id = im_core::secure::SecureOutboxId::parse(outbox_id)
            .map_err(DartImError::from)?;
        inner
            .secure()
            .outbox()
            .retry(outbox_id)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn secure_outbox_drop(
    client: Arc<crate::api::client::DartImClient>,
    outbox_id: String,
) -> Result<DartSecureOutboxResult, DartImError> {
    client.with_inner(|inner| {
        let outbox_id = im_core::secure::SecureOutboxId::parse(outbox_id)
            .map_err(DartImError::from)?;
        inner
            .secure()
            .outbox()
            .drop(outbox_id)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}
