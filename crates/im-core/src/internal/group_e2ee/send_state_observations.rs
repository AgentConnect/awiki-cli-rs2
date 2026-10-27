use super::*;
use crate::internal::wire::sync_v2::group_e2ee_state::{
    GroupStateObservation, GroupStateObservations,
};
use serde_json::json;

#[derive(Clone)]
pub(super) struct ObservedGroup {
    observation: GroupStateObservation,
    connection: Option<String>,
    caught_up: bool,
    local_generation: u64,
    ready: Option<PreparedState>,
}

#[derive(Clone)]
struct PreparedState {
    authoritative: GroupReadResult,
    endpoints: BTreeSet<(String, String)>,
}

#[derive(Clone)]
pub(crate) struct PreparationTicket {
    scope: Scope,
    observation: GroupStateObservation,
    connection: String,
    local_generation: u64,
}

fn same_owner(left: &Scope, right: &Scope) -> bool {
    left.owner == right.owner
        && left.did == right.did
        && left.device == right.device
        && left.generation == right.generation
        && left.signing_key == right.signing_key
        && left.e2ee_key == right.e2ee_key
}

pub(crate) fn has_observation(client: &crate::ImClient, group: &GroupRef) -> bool {
    let Ok(scope) = scope(client, group) else {
        return false;
    };
    client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observed
        .contains_key(&scope)
}

/// Whether preparation can use the latest complete observation on this live
/// connection. Unlike a send ticket, bootstrap does not require an MLS epoch.
pub(crate) fn observation_is_current(client: &crate::ImClient, group: &GroupRef) -> bool {
    let Ok(key) = scope(client, group) else {
        return false;
    };
    let Some(connection) = connection_token(client) else {
        return false;
    };
    client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observed
        .get(&key)
        .is_some_and(|observed| {
            observed.caught_up
                && observed.connection.as_ref() == Some(&connection)
                && observed.local_generation != u64::MAX
        })
}

pub(crate) fn invalidate(client: &crate::ImClient, group: Option<&str>) {
    let Ok(owner) = scope_for(client, "") else {
        return;
    };
    let mut state = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (key, value) in &mut state.observed {
        if same_owner(key, &owner) && group.is_none_or(|group| key.group == group) {
            value.ready = None;
            value.local_generation = value.local_generation.saturating_add(1);
        }
    }
}

/// An explicit authenticated read can discover a change before its notification.
/// It invalidates old facts but cannot mint a replacement readiness certificate.
pub(crate) fn observe_group_read(client: &crate::ImClient, raw: &serde_json::Value) {
    use serde_json::Value;
    let group = raw
        .get("group_did")
        .or_else(|| raw.pointer("/group_snapshot/group_did"))
        .and_then(Value::as_str);
    let Some(group) = group else { return };
    let Ok(key) = scope_for(client, group) else {
        return;
    };
    let wake = {
        let mut state = client
            .core_inner()
            .group_send_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(observed) = state.observed.get_mut(&key) else {
            return;
        };
        let version = raw
            .get("group_state_version")
            .or_else(|| raw.pointer("/group_snapshot/group_state_version"));
        let ordering = version.and_then(Value::as_str).map(|version| {
            crate::internal::local_state::sync_v2::compare_decimal(
                version,
                &observed.observation.group_state_version,
            )
        });
        if matches!(&ordering, Some(Ok(std::cmp::Ordering::Less))) {
            return;
        }
        let changed_version =
            version.is_some() && !matches!(&ordering, Some(Ok(std::cmp::Ordering::Equal)));
        let membership = raw
            .get("membership_status")
            .or_else(|| raw.pointer("/group_snapshot/membership_status"))
            .and_then(Value::as_str);
        let role = raw
            .get("my_role")
            .or_else(|| raw.pointer("/group_snapshot/my_role"))
            .and_then(Value::as_str);
        let changed_role = role.is_some_and(|role| {
            observed
                .ready
                .as_ref()
                .and_then(|ready| ready.authoritative.group.as_ref())
                .and_then(|group| group.my_role.as_deref())
                .is_some_and(|previous| previous != role)
        });
        let changed = changed_version
            || changed_role
            || membership.is_some_and(|status| status != "active")
            || raw
                .get("e2ee_maintenance")
                .is_some_and(|value| !value.is_null());
        if !changed {
            return;
        }
        let wake = observed.caught_up || observed.ready.is_some();
        observed.ready = None;
        observed.caught_up = false;
        observed.local_generation = observed.local_generation.saturating_add(1);
        wake
    };
    if wake && tokio::runtime::Handle::try_current().is_ok() {
        let client = client.clone();
        tokio::spawn(async move {
            let _ = client
                .messages()
                .request_receive_async(crate::messages::MessageSyncRequest {
                    reason: "after_mutation".into(),
                    limit: None,
                })
                .await;
        });
    }
}

pub(crate) fn forget_observations(client: &crate::ImClient) {
    let Ok(owner) = scope_for(client, "") else {
        return;
    };
    client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observed
        .retain(|key, _| !same_owner(key, &owner));
}

/// Persist invalidation work before publishing the new in-memory observation.
/// Snapshot absence is not an empty inventory and never revives prior Ready.
pub(crate) async fn observe_async(
    client: &crate::ImClient,
    observations: Option<&GroupStateObservations>,
    expected_connection: Option<String>,
    caught_up: bool,
    wake_processing: bool,
) -> crate::ImResult<()> {
    let owner = scope_for(client, "")?;
    let Some(observations) = observations.filter(|snapshot| snapshot.complete) else {
        let mut state = client
            .core_inner()
            .group_send_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.observed.retain(|key, _| !same_owner(key, &owner));
        return Ok(());
    };
    let home = client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .as_ref()
        .map(|did| did.as_str());
    let observations = GroupStateObservations {
        schema_version: 1,
        complete: true,
        groups: observations
            .groups
            .iter()
            .filter(|group| {
                Some(group.host_service_did.as_str()) == home && group.local_identity_coverage
            })
            .cloned()
            .collect(),
    };
    let changed = {
        let mut state = client
            .core_inner()
            .group_send_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut changed = Vec::new();
        for observation in &observations.groups {
            let mut key = owner.clone();
            key.group = observation.group_did.clone();
            if !state.observed.get(&key).is_some_and(|previous| {
                previous.observation == *observation
                    && previous.connection == expected_connection
                    && previous.caught_up == caught_up
                    && !(caught_up && expected_connection.is_some() && previous.ready.is_none())
            }) {
                if let Some(previous) = state.observed.get_mut(&key) {
                    if previous.observation != *observation {
                        previous.ready = None;
                    }
                    if previous.observation != *observation
                        || previous.connection != expected_connection
                        || previous.caught_up != caught_up
                    {
                        previous.local_generation = previous.local_generation.saturating_add(1);
                        // Invalidate before awaiting the SQLite actor. Neither
                        // an old nor a new preparation may borrow old facts in
                        // the interval before the durable replacement commits.
                        previous.caught_up = false;
                    }
                }
                let generation = state
                    .observed
                    .get(&key)
                    .map_or(0, |old| old.local_generation);
                if state
                    .observed
                    .get(&key)
                    .is_none_or(|previous| previous.ready.is_none())
                {
                    changed.push((observation.clone(), generation));
                }
            }
        }
        state.observed.retain(|key, _| {
            !same_owner(key, &owner)
                || observations
                    .groups
                    .iter()
                    .any(|group| group.group_did == key.group)
        });
        changed
    };
    let stored = observations.clone();
    let stored_owner = owner.clone();
    let stored_connection = expected_connection.clone();
    let db = client.core_inner().local_state_db().await?;
    db.run_local(move |connection| {
        let transaction = rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let binding = crate::internal::local_state::sync_v2::load_identity_account_binding(&transaction, &stored_owner.owner)?;
        if !binding.is_some_and(|binding| binding.current_did == stored_owner.did
            && binding.protocol_device_id == stored_owner.device
            && binding.device_auth_generation == stored_owner.generation.to_string()) {
            return Err(crate::ImError::CursorStale);
        }
        for (group, generation) in changed {
            let trigger = format!("observation:{}:{}:{generation}:{caught_up}", group.revision, stored_connection.as_deref().unwrap_or("offline"));
            crate::internal::group_runtime::journal::schedule_preparation_on(
                &transaction, &stored_owner.owner, &stored_owner.device, &group.group_did, &trigger,
            )?;
        }
        crate::internal::group_runtime::journal::save_on(
            &transaction, &stored_owner.owner, &stored_owner.device, "group-observations-v1",
            json!({"did":stored_owner.did,"auth_generation":stored_owner.generation,"observations":stored}),
        )?;
        transaction.commit().map_err(crate::internal::local_state::local_state_unavailable)
    }).await?;
    {
        let mut state = client
            .core_inner()
            .group_send_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if connection_token(client) != expected_connection {
            return Ok(());
        }
        for observation in observations.groups {
            let mut key = owner.clone();
            key.group = observation.group_did.clone();
            let ready = state
                .observed
                .get(&key)
                .filter(|old| old.observation == observation && caught_up)
                .and_then(|old| old.ready.clone());
            let local_generation = state
                .observed
                .get(&key)
                .map_or(0, |old| old.local_generation);
            if state.observed.len() >= 4096 && !state.observed.contains_key(&key) {
                state.observed.pop_first();
            }
            state.observed.insert(
                key,
                ObservedGroup {
                    observation,
                    connection: expected_connection.clone(),
                    caught_up,
                    local_generation,
                    ready,
                },
            );
        }
    }
    if wake_processing {
        crate::internal::message_runtime::sync_dispatcher::wake_group_recovery(client);
    }
    Ok(())
}

pub(crate) fn preparation_ticket(
    client: &crate::ImClient,
    group: &GroupRef,
) -> crate::ImResult<Option<PreparationTicket>> {
    if !client.core_inner().group_e2ee_v2_enabled() {
        return Ok(None);
    }
    let key = match scope(client, group) {
        Ok(key) => key,
        // Plain/legacy callers have no P6 state to reuse. Their normal policy
        // resolver still decides whether the requested operation is allowed.
        Err(crate::ImError::PermissionDenied) => return Ok(None),
        Err(error) => return Err(error),
    };
    let current_connection = connection_token(client);
    let state = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(observed) = state.observed.get(&key) else {
        return Ok(None);
    };
    let Some(connection) = current_connection.as_ref() else {
        return Ok(None);
    };
    if !observed.caught_up
        || observed.connection.as_ref() != Some(connection)
        || observed.observation.epoch.is_none()
        || observed.local_generation == u64::MAX
    {
        return Ok(None);
    }
    Ok(Some(PreparationTicket {
        scope: key,
        observation: observed.observation.clone(),
        connection: connection.clone(),
        local_generation: observed.local_generation,
    }))
}

/// Bounded local-only test evidence. No identities, keys, endpoint identifiers,
/// tokens, or ciphertext are exposed, and no synchronization is triggered.
#[cfg(feature = "internal-test-helpers")]
pub(crate) fn diagnostics_for_test(
    client: &crate::ImClient,
    group: &GroupRef,
) -> crate::ImResult<serde_json::Value> {
    let key = match scope(client, group) {
        Ok(key) => key,
        Err(_) => return Ok(json!({"scope_valid":false})),
    };
    let token = connection_token(client);
    let observed = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observed
        .get(&key)
        .cloned();
    let mut value = json!({"scope_valid":true,"connection_present":token.is_some(),"observed":observed.is_some(),
        "group_enabled":client.core_inner().group_e2ee_v2_enabled(),
        "connection_scope":crate::internal::message_runtime::send_lifecycle::diagnostics_for_test(client)});
    if let Some(observed) = observed {
        value["observation_connected"] = json!(observed.connection.is_some());
        value["connection_matches"] = json!(token.is_some() && observed.connection == token);
        value["caught_up"] = json!(observed.caught_up);
        value["epoch_present"] = json!(observed.observation.epoch.is_some());
        value["prepared"] = json!(observed.ready.is_some());
        value["generation"] = json!(observed.local_generation);
        let current = endpoints(client, group);
        value["sdk_ready"] = json!(current.is_ok());
        if let (Ok(current), Some(ready)) = (current, observed.ready) {
            value["endpoints_match"] = json!(current == ready.endpoints);
        }
    }
    Ok(value)
}

fn endpoints(
    client: &crate::ImClient,
    group: &GroupRef,
) -> crate::ImResult<BTreeSet<(String, String)>> {
    let runtime = super::super::v2_runtime::runtime_for_client(client)?;
    let status = super::super::v2_status::GroupE2eeV2StatusRuntime::new(runtime.clone())
        .status(group.clone())?;
    if !status.can_send_secure {
        return Err(crate::ImError::CursorStale);
    }
    let device = super::super::v2_lifecycle::current_v2_device(client)?;
    Ok(runtime
        .list_local_group_member_endpoints(super::super::v2_lifecycle::endpoint_inventory_input(
            client,
            &device,
            group.as_str(),
            "send-state",
        ))?
        .member_endpoints
        .into_iter()
        .map(|endpoint| (endpoint.member_did, endpoint.member_device_id))
        .collect())
}

pub(crate) fn publish_prepared(
    client: &crate::ImClient,
    ticket: Option<PreparationTicket>,
    authoritative: &GroupReadResult,
) -> crate::ImResult<()> {
    let Some(ticket) = ticket else {
        // A live observer must finish synchronization before its recovery task
        // can complete. Legacy/HTTP-only preparation has no reusable ticket.
        if connection_token(client).is_some()
            && authoritative.group.as_ref().is_some_and(|group| {
                has_observation(client, &group.did)
            })
        {
            return Err(crate::ImError::CursorStale);
        }
        return Ok(());
    };
    let group = GroupRef::parse(&ticket.scope.group)?;
    if authoritative
        .raw_response()
        .is_none_or(|raw| raw.get("e2ee_maintenance").is_some())
        || !crate::groups::authoritative_group_e2ee_classification(group.as_str(), authoritative)?
    {
        return Err(crate::ImError::CursorStale);
    }
    if scope(client, &group)? != ticket.scope
        || inputs(&group, authoritative)?.0 != ticket.observation.group_state_version
    {
        return Err(crate::ImError::CursorStale);
    }
    let endpoints = endpoints(client, &group)?;
    let mut state = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(observed) = state.observed.get_mut(&ticket.scope) {
        if observed.observation == ticket.observation
            && observed.connection.as_ref() == Some(&ticket.connection)
            && observed.caught_up
            && observed.local_generation == ticket.local_generation
        {
            observed.ready = Some(PreparedState {
                authoritative: authoritative.clone(),
                endpoints,
            });
            return Ok(());
        }
    }
    Err(crate::ImError::CursorStale)
}

pub(crate) fn prepared_snapshot(
    client: &crate::ImClient,
    group: &GroupRef,
) -> crate::ImResult<Option<GroupReadResult>> {
    let Some(ticket) = preparation_ticket(client, group)? else {
        return Ok(None);
    };
    let ready = {
        let state = client
            .core_inner()
            .group_send_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .observed
            .get(&ticket.scope)
            .and_then(|observed| observed.ready.clone())
    };
    let Some(ready) = ready else { return Ok(None) };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let pending: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_lane_inbox WHERE owner_identity_id=?1 AND closed_at IS NULL AND (lane='baseline' OR group_did=?2 OR processing_scope=?2)) OR EXISTS(SELECT 1 FROM p6_lane_blockers WHERE owner_identity_id=?1 AND group_did=?2) OR EXISTS(SELECT 1 FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?3 AND operation_key LIKE 'recovery:%' AND json_extract(intent_json,'$.group_did')=?2 AND json_extract(intent_json,'$.phase')<>'completed')",
        (&ticket.scope.owner, group.as_str(), &ticket.scope.device), |row| row.get(0),
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    if pending || endpoints(client, group).ok().as_ref() != Some(&ready.endpoints) {
        return Ok(None);
    }
    let state = client
        .core_inner()
        .group_send_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(state
        .observed
        .get(&ticket.scope)
        .filter(|observed| {
            observed.ready.is_some()
                && connection_token(client).as_ref() == Some(&ticket.connection)
                && observed.observation == ticket.observation
                && observed.connection.as_ref() == Some(&ticket.connection)
                && observed.caught_up
                && observed.local_generation == ticket.local_generation
        })
        .map(|_| ready.authoritative))
}

/// Recheck after asynchronous attachment preparation and immediately before a
/// new encryption. Unknown-result retries never pass through this function.
pub(crate) fn validate_preparation_ticket(
    client: &crate::ImClient,
    ticket: &PreparationTicket,
) -> crate::ImResult<()> {
    let group = GroupRef::parse(&ticket.scope.group)?;
    if prepared_snapshot(client, &group)?.is_none() {
        return Err(crate::ImError::CursorStale);
    }
    let Some(current) = preparation_ticket(client, &group)? else {
        return Err(crate::ImError::CursorStale);
    };
    if current.scope != ticket.scope
        || current.connection != ticket.connection
        || current.observation != ticket.observation
        || current.local_generation != ticket.local_generation
    {
        return Err(crate::ImError::CursorStale);
    }
    Ok(())
}
