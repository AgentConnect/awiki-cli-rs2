use std::sync::Arc;

use crate::dto::{
    directory::{DartDirectoryResolution, DartRelationStatus},
    error::DartImError,
};

pub fn resolve_peer(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectoryResolution, DartImError> {
    client.with_inner(|inner| {
        let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
        inner
            .directory()
            .resolve_peer(peer)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn lookup_handle(
    client: Arc<crate::api::client::DartImClient>,
    handle: String,
) -> Result<DartDirectoryResolution, DartImError> {
    client.with_inner(|inner| {
        let handle = im_core::ids::Handle::parse(handle, "").map_err(DartImError::from)?;
        let result = inner
            .directory()
            .lookup_handle(handle)
            .map_err(DartImError::from)?;
        Ok(DartDirectoryResolution {
            input: result.handle.as_str().to_string(),
            did: result.did.as_str().to_string(),
            handle: Some(result.handle.as_str().to_string()),
            profile: None,
            warnings: Vec::new(),
        })
    })
}

pub fn relation_status(
    client: Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartRelationStatus, DartImError> {
    client.with_inner(|inner| {
        let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
        inner
            .directory()
            .relation_status(peer)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn follow(
    _client: Arc<crate::api::client::DartImClient>,
    _peer: String,
) -> Result<(), DartImError> {
    Err(DartImError::unsupported("relationship-remote-mutation"))
}

pub fn unfollow(
    _client: Arc<crate::api::client::DartImClient>,
    _peer: String,
) -> Result<(), DartImError> {
    Err(DartImError::unsupported("relationship-remote-mutation"))
}
