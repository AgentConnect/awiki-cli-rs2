use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    group::{DartCreateGroupRequest, DartGroupReadResult},
};

fn page_limit(limit: u32) -> Result<im_core::ids::PageLimit, DartImError> {
    im_core::ids::PageLimit::new(limit).map_err(DartImError::from)
}

pub fn create_group(
    client: Arc<crate::api::client::DartImClient>,
    request: DartCreateGroupRequest,
) -> Result<DartGroupReadResult, DartImError> {
    let default_service_did = client.default_service_did()?;
    client.with_inner(|inner| {
        inner
            .groups()
            .create(request.into_core(default_service_did)?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn join_group(
    client: Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
        inner
            .groups()
            .join(im_core::groups::GroupJoinRequest {
                group,
                reason_text: None,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn get_group(
    client: Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
        inner
            .groups()
            .get(group)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn list_groups(
    client: Arc<crate::api::client::DartImClient>,
    limit: u32,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        inner
            .groups()
            .list(im_core::groups::GroupListRequest {
                limit: page_limit(limit)?,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn list_group_members(
    client: Arc<crate::api::client::DartImClient>,
    group_did: String,
    limit: u32,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
        inner
            .groups()
            .members(im_core::groups::GroupMembersRequest {
                group,
                limit: page_limit(limit)?,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn list_group_messages(
    client: Arc<crate::api::client::DartImClient>,
    group_did: String,
    limit: u32,
    cursor: Option<String>,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
        let cursor = cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?;
        inner
            .groups()
            .messages(im_core::groups::GroupMessagesRequest {
                group,
                limit: page_limit(limit)?,
                cursor,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn leave_group(
    client: Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    client.with_inner(|inner| {
        let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
        inner
            .groups()
            .leave(im_core::groups::GroupLeaveRequest { group })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn get_group_join_code(
    _client: Arc<crate::api::client::DartImClient>,
    _group_did: String,
) -> Result<Option<String>, DartImError> {
    Ok(None)
}

pub fn refresh_group_join_code(
    _client: Arc<crate::api::client::DartImClient>,
    _group_did: String,
) -> Result<Option<String>, DartImError> {
    Ok(None)
}
