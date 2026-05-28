use std::sync::Arc;

use crate::dto::{
    directory::{DartDirectoryResolution, DartRelationStatus, DartRelationshipPage},
    error::DartImError,
};

fn page_limit(limit: u32) -> Result<im_core::ids::PageLimit, DartImError> {
    im_core::ids::PageLimit::new(limit).map_err(DartImError::from)
}

pub async fn resolve_peer(
    client: &Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartDirectoryResolution, DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .directory()
        .resolve_peer_async(peer)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn lookup_handle(
    client: &Arc<crate::api::client::DartImClient>,
    handle: String,
) -> Result<DartDirectoryResolution, DartImError> {
    let inner = client.clone_inner()?;
    let handle = im_core::ids::Handle::parse(handle, "").map_err(DartImError::from)?;
    let result = inner
        .directory()
        .lookup_handle_async(handle)
        .await
        .map_err(DartImError::from)?;
    Ok(DartDirectoryResolution {
        input: result.handle.as_str().to_string(),
        did: result.did.as_str().to_string(),
        handle: Some(result.handle.as_str().to_string()),
        profile: None,
        warnings: Vec::new(),
    })
}

pub async fn relation_status(
    client: &Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<DartRelationStatus, DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .directory()
        .relation_status_async(peer)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn follow(
    client: &Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<(), DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .directory()
        .follow_async(im_core::directory::FollowRequest { peer })
        .await
        .map(|_| ())
        .map_err(DartImError::from)
}

pub async fn unfollow(
    client: &Arc<crate::api::client::DartImClient>,
    peer: String,
) -> Result<(), DartImError> {
    let inner = client.clone_inner()?;
    let peer = im_core::ids::PeerRef::parse(peer, "").map_err(DartImError::from)?;
    inner
        .directory()
        .unfollow_async(im_core::directory::UnfollowRequest { peer })
        .await
        .map(|_| ())
        .map_err(DartImError::from)
}

pub async fn list_followers(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    offset: u32,
    hydrate_profiles: bool,
) -> Result<DartRelationshipPage, DartImError> {
    relationship_list(client, "follower", limit, offset, hydrate_profiles).await
}

pub async fn list_following(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    offset: u32,
    hydrate_profiles: bool,
) -> Result<DartRelationshipPage, DartImError> {
    relationship_list(client, "following", limit, offset, hydrate_profiles).await
}

async fn relationship_list(
    client: &Arc<crate::api::client::DartImClient>,
    relationship: &'static str,
    limit: u32,
    offset: u32,
    hydrate_profiles: bool,
) -> Result<DartRelationshipPage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::directory::RelationshipListQuery {
        limit: Some(page_limit(limit)?),
        offset: Some(offset),
        hydrate_profiles,
    };
    let page = match relationship {
        "follower" => inner.directory().followers_async(query).await,
        "following" => inner.directory().following_async(query).await,
        _ => unreachable!("unsupported relationship list kind"),
    }
    .map_err(DartImError::from)?;
    Ok(DartRelationshipPage {
        items: page
            .items
            .into_iter()
            .filter_map(|item| relationship_item_to_dart(item, relationship))
            .collect(),
        next_cursor: page.next_cursor.map(|cursor| cursor.as_str().to_string()),
        has_more: page.has_more,
    })
}

fn relationship_item_to_dart(
    item: im_core::directory::RelationshipListItem,
    relationship: &'static str,
) -> Option<crate::dto::directory::DartRelationshipListItem> {
    let display_name = item
        .profile
        .as_ref()
        .and_then(|profile| profile.display_name.clone());
    let handle = item.handle.map(|handle| handle.as_str().to_string());
    let did = item.did.map(|did| did.as_str().to_string())?;
    Some(crate::dto::directory::DartRelationshipListItem {
        did,
        handle,
        display_name,
        relationship: relationship.to_string(),
        created_at: item.created_at,
        warnings: item.warnings,
    })
}
