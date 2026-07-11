use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    group::{DartCreateGroupRequest, DartGroupReadResult},
};

fn page_limit(limit: u32) -> Result<im_core::ids::PageLimit, DartImError> {
    im_core::ids::PageLimit::new(limit).map_err(DartImError::from)
}

pub async fn create_group(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartCreateGroupRequest,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .groups()
        .create_async(request.into_core()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn join_group(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    inner
        .groups()
        .join_async(im_core::groups::GroupJoinRequest {
            group,
            member_handle: None,
            reason_text: None,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn get_group(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    inner
        .groups()
        .get_async(group)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn list_groups(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .groups()
        .list_async(im_core::groups::GroupListRequest {
            limit: page_limit(limit)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn list_group_members(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
    limit: u32,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    inner
        .groups()
        .members_async(im_core::groups::GroupMembersRequest {
            group,
            limit: page_limit(limit)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn add_group_member(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
    member_ref: String,
    role: Option<String>,
) -> Result<DartGroupReadResult, DartImError> {
    mutate_group_member(client, group_did, member_ref, role, true).await
}

pub async fn remove_group_member(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
    member_ref: String,
) -> Result<DartGroupReadResult, DartImError> {
    mutate_group_member(client, group_did, member_ref, None, false).await
}

pub async fn list_group_messages(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
    limit: u32,
    cursor: Option<String>,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    let cursor = cursor
        .map(im_core::ids::Cursor::parse)
        .transpose()
        .map_err(DartImError::from)?;
    inner
        .groups()
        .messages_async(im_core::groups::GroupMessagesRequest {
            group,
            limit: page_limit(limit)?,
            cursor,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn leave_group(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    inner
        .groups()
        .leave_async(im_core::groups::GroupLeaveRequest {
            group,
            reason_text: None,
            security: Default::default(),
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub fn get_group_join_code(
    _client: &Arc<crate::api::client::DartImClient>,
    _group_did: String,
) -> Result<Option<String>, DartImError> {
    Ok(None)
}

pub fn refresh_group_join_code(
    _client: &Arc<crate::api::client::DartImClient>,
    _group_did: String,
) -> Result<Option<String>, DartImError> {
    Ok(None)
}

async fn mutate_group_member(
    client: &Arc<crate::api::client::DartImClient>,
    group_did: String,
    member_ref: String,
    role: Option<String>,
    add: bool,
) -> Result<DartGroupReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let group = im_core::ids::GroupRef::parse(group_did).map_err(DartImError::from)?;
    let did_domain = did_domain_for_client(&inner);
    let member = im_core::groups::GroupMemberRef::parse(member_ref, &did_domain)
        .map_err(DartImError::from)?;
    let role = match role {
        Some(value) => {
            im_core::groups::GroupMemberRole::parse_optional(value).map_err(DartImError::from)?
        }
        None => None,
    };
    let request = im_core::groups::GroupMemberMutationRequest {
        group,
        member,
        role,
        reason_text: None,
        leave_request_id: None,
        security: im_core::groups::GroupSecurityRequirement::Default,
    };
    let groups = inner.groups();
    let result = if add {
        groups.add_member_async(request).await
    } else {
        groups.remove_member_async(request).await
    };
    result.map(Into::into).map_err(DartImError::from)
}

fn did_domain_for_client(client: &im_core::ImClient) -> String {
    client.did_domain().to_owned()
}
