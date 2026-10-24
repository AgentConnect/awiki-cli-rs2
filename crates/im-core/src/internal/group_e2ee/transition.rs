//! Home-local hints only schedule a reload. The authenticated owner view and
//! fresh DID manifests, never the hint, constrain the existing MLS repair.
use serde::Deserialize;
use serde_json::Value;

pub(crate) const HINT_METHOD: &str = "group_e2ee_reconcile_required";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransitionPlan {
    update_id: String,
    group_did: String,
    group_state_version: String,
    previous_did: String,
    current_did: String,
    phase: String,
    eligible_device_ids: Vec<String>,
}

impl TransitionPlan {
    pub(crate) fn from_group_response(
        raw: &Value,
        state: &anp::group_e2ee::V2GroupStateRef,
    ) -> crate::ImResult<Option<Self>> {
        let Some(value) = raw.pointer("/e2ee_maintenance/did_transition") else {
            return Ok(None);
        };
        let plan: Self =
            serde_json::from_value(value.clone()).map_err(|_| crate::ImError::PermissionDenied)?;
        if plan.group_did != state.group_did
            || plan.group_state_version != state.group_state_version
        {
            return Err(crate::ImError::CursorStale);
        }
        if plan.update_id.is_empty()
            || plan.previous_did == plan.current_did
            || !plan.previous_did.starts_with("did:")
            || !plan.current_did.starts_with("did:")
            || plan.eligible_device_ids.is_empty()
            || plan
                .eligible_device_ids
                .iter()
                .any(|id| id.is_empty() || id.trim() != id)
            || plan
                .eligible_device_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || !matches!(
                plan.phase.as_str(),
                "add_pending" | "adds_confirmed" | "remove_pending" | "action_required"
            )
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some(plan))
    }
}

pub(crate) fn validate_roster_members(
    plan: Option<&TransitionPlan>,
    members: &std::collections::BTreeSet<String>,
) -> crate::ImResult<()> {
    if let Some(plan) = plan {
        if !members.contains(&plan.current_did) || members.contains(&plan.previous_did) {
            return Err(crate::ImError::CursorStale);
        }
    }
    Ok(())
}

pub(crate) fn desired_devices(
    plan: Option<&TransitionPlan>,
    did: &str,
    current: Vec<String>,
) -> crate::ImResult<Vec<String>> {
    let Some(plan) = plan.filter(|plan| plan.current_did == did) else {
        return Ok(current);
    };
    // A frozen device that is no longer eligible cannot be silently replaced
    // by a later Join. The Host must resolve that transition explicitly.
    if plan
        .eligible_device_ids
        .iter()
        .any(|id| !current.contains(id))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(plan.eligible_device_ids.clone())
}

type Endpoint = (String, String);

pub(crate) fn restrict_delta(
    plan: Option<&TransitionPlan>,
    mut extra: Vec<Endpoint>,
    mut missing: Vec<Endpoint>,
) -> crate::ImResult<(Vec<Endpoint>, Vec<Endpoint>)> {
    let Some(plan) = plan else {
        return Ok((extra, missing));
    };
    if plan.phase == "action_required" {
        return Err(crate::ImError::PermissionDenied);
    }
    missing.retain(|(did, device)| {
        did == &plan.current_did && plan.eligible_device_ids.contains(device)
    });
    extra.retain(|(did, _)| did == &plan.previous_did);
    if plan.phase == "remove_pending" && !missing.is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }
    // Defer unrelated Adds/Removes (including devices joined after Recovery)
    // until the Host has closed the frozen Add-before-Remove transaction.
    if !missing.is_empty() {
        extra.clear();
    }
    Ok((extra, missing))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconcileHint {
    recipient_did: String,
    group_did: String,
}

fn hint_group(notification: &Value, recipient: &str) -> crate::ImResult<crate::ids::GroupRef> {
    let hint: ReconcileHint = serde_json::from_value(notification["params"].clone())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if hint.recipient_did != recipient {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::ids::GroupRef::parse(&hint.group_did)
}

pub(crate) fn consume_hint(
    client: &crate::core::ImClient,
    notification: &Value,
    warnings: &mut Vec<String>,
) -> bool {
    if notification["method"].as_str() != Some(HINT_METHOD) {
        return false;
    }
    if !client.core_inner().group_e2ee_v2_enabled() {
        return true;
    }
    let result = hint_group(notification, client.did().as_str()).and_then(|group| {
        crate::internal::group_runtime::recovery::record(
            client,
            &format!("recovery:repair:{}", group.as_str()),
            group.as_str(),
            None,
            false,
        )
        .map(|_| ())
    });
    if result.is_err() {
        warnings.push("Group reconciliation hint could not be scheduled".to_owned());
    } else if tokio::runtime::Handle::try_current().is_ok() {
        crate::internal::message_runtime::sync_dispatcher::wake_group_recovery(client);
    } else {
        warnings.push("Group reconciliation executor runtime was unavailable".to_owned());
    }
    // Never project control input as chat or treat it as a P6 Commit/ACK.
    true
}

#[cfg(test)]
#[path = "transition_tests.rs"]
mod tests;
