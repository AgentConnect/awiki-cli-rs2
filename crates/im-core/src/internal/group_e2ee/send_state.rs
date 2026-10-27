//! One owner/device-scoped source for prepared group sends and roster evidence.
//! Old servers retain explicit preparation; observed current state can reuse a
//! completed preparation while the same Core maintains its receive lifecycle.

use std::collections::{BTreeMap, BTreeSet};

use crate::groups::GroupReadResult;
use crate::ids::GroupRef;
use crate::internal::group_runtime::read::GroupReadRuntime;

#[path = "send_state_observations.rs"]
mod observations;
pub(crate) use crate::internal::message_runtime::send_lifecycle::connection_token;
#[cfg(test)]
pub(crate) use crate::internal::message_runtime::send_lifecycle::ConnectionGuard;
pub(crate) use observations::{
    forget_observations, has_observation, invalidate, observe_async, observe_group_read,
    preparation_ticket, prepared_snapshot, publish_prepared, validate_preparation_ticket,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Scope {
    owner: String,
    did: String,
    device: String,
    generation: u64,
    signing_key: String,
    e2ee_key: String,
    group: String,
}

#[derive(Default)]
pub(crate) struct GroupSendState {
    entries: BTreeMap<Scope, (String, BTreeSet<String>)>,
    observed: BTreeMap<Scope, observations::ObservedGroup>,
}

impl GroupSendState {
    fn contains(&self, scope: &Scope, version: &str, local: &BTreeSet<String>) -> bool {
        self.entries
            .get(scope)
            .is_some_and(|(saved_version, saved_members)| {
                saved_version == version && saved_members == local
            })
    }

    fn confirm(&mut self, scope: Scope, version: String, members: BTreeSet<String>) {
        if self.entries.len() >= 128 && !self.entries.contains_key(&scope) {
            self.entries.clear();
        }
        self.entries.insert(scope, (version, members));
    }
}

fn scope(client: &crate::ImClient, group: &GroupRef) -> crate::ImResult<Scope> {
    scope_for(client, group.as_str())
}

fn scope_for(client: &crate::ImClient, group: &str) -> crate::ImResult<Scope> {
    let authorization =
        crate::internal::identity_device_state::active_authorization_for_client(client)?;
    Ok(Scope {
        owner: client.current_identity().id.as_str().to_owned(),
        did: client.did().as_str().to_owned(),
        device: authorization.protocol_device_id.as_str().to_owned(),
        generation: authorization.auth_generation,
        signing_key: authorization.signing_key_id,
        e2ee_key: authorization.e2ee_key_id,
        group: group.to_owned(),
    })
}

fn inputs(group: &GroupRef, authoritative: &GroupReadResult) -> crate::ImResult<(String, usize)> {
    let snapshot = authoritative
        .group
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if snapshot.did.as_str() != group.as_str()
        || snapshot.membership_status.as_deref() != Some("active")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw = authoritative
        .raw_response()
        .ok_or(crate::ImError::InventoryIncomplete)?;
    let reference = super::state_ref::group_state_ref_from_group_response(group.as_str(), raw)
        .ok_or(crate::ImError::InventoryIncomplete)?;
    Ok((
        reference.group_state_version,
        super::member_collector::product_max_members(raw)?,
    ))
}

fn local_members(client: &crate::ImClient, group: &GroupRef) -> crate::ImResult<BTreeSet<String>> {
    let runtime = super::v2_runtime::runtime_for_client(client)?;
    let device = super::v2_lifecycle::current_v2_device(client)?;
    let endpoints = runtime.list_local_group_member_endpoints(
        super::v2_lifecycle::endpoint_inventory_input(client, &device, group.as_str(), "readiness"),
    )?;
    Ok(endpoints
        .member_endpoints
        .into_iter()
        .map(|endpoint| endpoint.member_did)
        .collect())
}

fn cache_hit(
    client: &crate::ImClient,
    group: &GroupRef,
    initial_scope: &Scope,
    version: &str,
) -> crate::ImResult<bool> {
    let local = local_members(client, group)?;
    if &scope(client, group)? != initial_scope {
        return Err(crate::ImError::CursorStale);
    }
    Ok(client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(initial_scope, version, &local))
}

/// Called only after a complete, version-fenced authoritative roster read.
pub(crate) fn confirm_members(
    client: &crate::ImClient,
    group: &GroupRef,
    authoritative: &GroupReadResult,
    expected: BTreeSet<String>,
) -> crate::ImResult<bool> {
    let (version, max_members) = inputs(group, authoritative)?;
    let initial_scope = scope(client, group)?;
    let matches = expected.len() <= max_members && expected == local_members(client, group)?;
    if scope(client, group)? != initial_scope {
        return Err(crate::ImError::CursorStale);
    }
    let mut cache = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if matches {
        cache.confirm(initial_scope, version, expected);
    } else {
        cache.entries.remove(&initial_scope);
    }
    Ok(matches)
}

fn finish(
    client: &crate::ImClient,
    group: &GroupRef,
    initial_scope: &Scope,
    version: &str,
    members: super::member_collector::CompleteGroupMembers,
    refreshed: GroupReadResult,
) -> crate::ImResult<Option<GroupReadResult>> {
    if &scope(client, group)? != initial_scope
        || inputs(group, &refreshed)?.0 != version
        || members.group_state_version != version
    {
        return Err(crate::ImError::CursorStale);
    }
    let expected = members
        .members
        .into_iter()
        .map(|member| {
            member
                .did
                .map(|did| did.as_str().to_owned())
                .ok_or(crate::ImError::InventoryIncomplete)
        })
        .collect::<crate::ImResult<BTreeSet<_>>>()?;
    Ok(confirm_members(client, group, &refreshed, expected)?.then_some(refreshed))
}

pub(crate) async fn validate_async(
    client: &crate::ImClient,
    group: GroupRef,
    authoritative: &GroupReadResult,
) -> crate::ImResult<Option<GroupReadResult>> {
    let (version, max_members) = inputs(&group, authoritative)?;
    let initial_scope = scope(client, &group)?;
    if cache_hit(client, &group, &initial_scope, &version)? {
        return Ok(Some(authoritative.clone()));
    }
    let members = super::member_collector::collect_complete_group_members_async(
        client,
        group.clone(),
        Some(&version),
        max_members,
    )
    .await?;
    let refreshed =
        super::v2_lifecycle::read_authoritative_group_async(client, group.clone()).await?;
    finish(client, &group, &initial_scope, &version, members, refreshed)
}

pub(crate) fn validate(
    client: &crate::ImClient,
    group: GroupRef,
    authoritative: &GroupReadResult,
) -> crate::ImResult<Option<GroupReadResult>> {
    let (version, max_members) = inputs(&group, authoritative)?;
    let initial_scope = scope(client, &group)?;
    if cache_hit(client, &group, &initial_scope, &version)? {
        return Ok(Some(authoritative.clone()));
    }
    let members = super::member_collector::collect_complete_group_members(
        client,
        group.clone(),
        Some(&version),
        max_members,
    )?;
    let refreshed = GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy(group.clone())?;
    finish(client, &group, &initial_scope, &version, members, refreshed)
}

#[cfg(test)]
#[path = "send_state_tests.rs"]
mod tests;
