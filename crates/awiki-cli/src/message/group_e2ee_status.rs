use super::group_e2ee_provider::{default_string, MlsExecProvider};
use super::service::{auth_session, i64_value, require_active_identity, string_value};
use super::{
    build_group_e2ee_head_rpc_params, build_group_e2ee_notice_rpc_params, Client, CommandResult,
    MessageError, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeStatusRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eePendingRequest {
    pub identity_name: String,
    pub group: String,
    pub limit: i64,
}

pub fn inspect_group_e2ee_status(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eeStatusRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let provider = MlsExecProvider::new(resolved);
    let mut warnings = Vec::new();
    let (local_status, local_device_id, local_error) =
        match group_e2ee_status_for_recovery(&provider, &record.did, &request.group, "") {
            Ok((status, device_id)) => (Some(status), device_id, None),
            Err(err) => {
                warnings.push(format!("Group E2EE local MLS status unavailable: {err}"));
                (None, String::new(), Some(err.to_string()))
            }
        };

    let mut service_head: Option<Map<String, Value>> = None;
    let mut pending: Option<Map<String, Value>> = None;
    match group_e2ee_transport(resolved, manager, &record) {
        Ok(mut transport) => {
            match transport.get_group_e2ee_head(&request.group) {
                Ok(head) => service_head = Some(head),
                Err(err) => warnings.push(format!("Group E2EE service head unavailable: {err}")),
            }
            match transport.pull_group_e2ee_notices(&request.group, limit, false) {
                Ok(result) => pending = Some(result),
                Err(err) => warnings.push(format!(
                    "Group E2EE pending notice status unavailable: {err}"
                )),
            }
        }
        Err(err) => warnings.push(format!("Group E2EE service status unavailable: {err}")),
    }

    let pending_notices = pending
        .as_ref()
        .map(|value| values_from_array(value.get("notices")))
        .unwrap_or_default();
    let pending_notice_count = pending
        .as_ref()
        .and_then(|value| i64_value(value.get("pending_count")))
        .unwrap_or(pending_notices.len() as i64);
    let local_status_value = local_status
        .as_ref()
        .map(|value| Value::Object(value.clone()))
        .unwrap_or(Value::Null);
    let service_head_value = service_head
        .as_ref()
        .map(|value| Value::Object(value.clone()))
        .unwrap_or(Value::Null);
    let diagnosis = group_e2ee_recovery_diagnosis(
        local_status.as_ref(),
        service_head.as_ref(),
        pending_notice_count,
        local_error.as_deref(),
    );
    let recovery_artifact = group_e2ee_recovery_artifact(
        &record,
        &request.group,
        &local_device_id,
        local_status.as_ref(),
        service_head.as_ref(),
        &diagnosis,
    );

    Ok(CommandResult {
        data: json!({
            "group": request.group,
            "available": local_error.is_none(),
            "mls": local_status_value,
            "local": local_status_value,
            "local_device_id": local_device_id,
            "service_head": service_head_value,
            "pending_notices": pending_notices,
            "pending_notice_count": pending_notice_count,
            "diagnosis": Value::Object(diagnosis),
            "recovery_artifact": recovery_artifact,
        }),
        summary: "Group E2EE recovery status inspected".to_string(),
        warnings: super::compact_warnings(warnings),
    })
}

pub fn pull_group_e2ee_notices(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eePendingRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let mut transport = group_e2ee_transport(resolved, manager, &record)?;
    let result = transport.pull_group_e2ee_notices(&request.group, limit, false)?;
    Ok(CommandResult {
        data: json!({
            "notices": values_from_array(result.get("notices")),
            "pending_count": result.get("pending_count").cloned().unwrap_or(Value::Null),
            "group": request.group,
        }),
        summary: "Pulled group E2EE pending notices".to_string(),
        warnings: Vec::new(),
    })
}

struct GroupE2eeTransport<'a> {
    client: Client,
    auth: crate::authsdk::Session,
    record: &'a StoredIdentity,
}

impl<'a> GroupE2eeTransport<'a> {
    fn get_group_e2ee_head(&mut self, group_did: &str) -> Result<Map<String, Value>, MessageError> {
        let params = build_group_e2ee_head_rpc_params(self.record, group_did)?;
        self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.e2ee.head",
            params,
            &mut self.auth,
        )
    }

    fn pull_group_e2ee_notices(
        &mut self,
        group_did: &str,
        limit: i64,
        mark_delivered: bool,
    ) -> Result<Map<String, Value>, MessageError> {
        let params = build_group_e2ee_notice_rpc_params(
            self.record,
            group_did,
            limit,
            mark_delivered,
            Vec::new(),
        )?;
        self.client.authenticated_rpc_call_profile(
            Profile::RpcDefault,
            MESSAGE_RPC_ENDPOINT,
            "group.e2ee.notice",
            params,
            &mut self.auth,
        )
    }
}

fn group_e2ee_transport<'a>(
    resolved: &Resolved,
    manager: &Manager,
    record: &'a StoredIdentity,
) -> Result<GroupE2eeTransport<'a>, MessageError> {
    Ok(GroupE2eeTransport {
        client: Client::new(resolved)?,
        auth: auth_session(resolved, manager, record)?,
        record,
    })
}

pub(crate) fn group_e2ee_status_for_recovery(
    provider: &MlsExecProvider,
    agent_did: &str,
    group_did: &str,
    preferred_device_id: &str,
) -> Result<(Map<String, Value>, String), MessageError> {
    let device_ids = group_e2ee_recovery_device_ids(provider, agent_did, preferred_device_id);
    let mut best: Option<Map<String, Value>> = None;
    let mut best_device_id = "default".to_string();
    let mut best_rank = -1;
    let mut best_epoch = -1;
    let mut last_error: Option<MessageError> = None;
    for device_id in device_ids {
        let status = match provider.status(agent_did, &device_id, group_did) {
            Ok(status) => status,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        let rank = group_e2ee_status_rank(&status);
        let epoch = group_e2ee_local_epoch_from_status(&status).unwrap_or(-1);
        if best.is_none() || rank > best_rank || (rank == best_rank && epoch > best_epoch) {
            best = Some(status);
            best_device_id = device_id;
            best_rank = rank;
            best_epoch = epoch;
        }
    }
    if let Some(mut best) = best {
        best.insert(
            "device_id".to_string(),
            Value::String(best_device_id.clone()),
        );
        return Ok((best, best_device_id));
    }
    if let Some(err) = last_error {
        return Err(err);
    }
    let mut empty = Map::new();
    empty.insert("status".to_string(), Value::String("empty".to_string()));
    empty.insert(
        "device_id".to_string(),
        Value::String(best_device_id.clone()),
    );
    Ok((empty, best_device_id))
}

pub(crate) fn group_e2ee_recovery_device_ids(
    provider: &MlsExecProvider,
    agent_did: &str,
    preferred_device_id: &str,
) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    push_device_id(&mut ordered, &mut seen, preferred_device_id);
    for device_id in provider.candidate_device_ids(agent_did) {
        push_device_id(&mut ordered, &mut seen, &device_id);
    }
    if ordered.is_empty() {
        push_device_id(&mut ordered, &mut seen, "default");
    }
    ordered
}

fn push_device_id(ordered: &mut Vec<String>, seen: &mut HashSet<String>, device_id: &str) {
    let device_id = default_string(device_id.trim(), "default");
    if seen.insert(device_id.clone()) {
        ordered.push(device_id);
    }
}

fn group_e2ee_status_rank(status: &Map<String, Value>) -> i32 {
    match string_value(status.get("status"))
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "active" => 3,
        "left" | "removed" | "inactive" => 2,
        "empty" | "" => {
            if group_e2ee_local_epoch_from_status(status).is_some() {
                1
            } else {
                0
            }
        }
        _ => {
            if group_e2ee_local_epoch_from_status(status).is_some() {
                1
            } else {
                0
            }
        }
    }
}

pub(crate) fn group_e2ee_recovery_diagnosis(
    local_status: Option<&Map<String, Value>>,
    service_head: Option<&Map<String, Value>>,
    pending_notice_count: i64,
    local_error: Option<&str>,
) -> Map<String, Value> {
    let mut diagnosis = Map::new();
    diagnosis.insert("state".to_string(), Value::String("unknown".to_string()));
    diagnosis.insert(
        "next_action".to_string(),
        Value::String("inspect".to_string()),
    );
    diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
    diagnosis.insert(
        "pending_notice_count".to_string(),
        json!(pending_notice_count),
    );
    if let Some(local_error) = local_error.filter(|value| !value.trim().is_empty()) {
        diagnosis.insert(
            "local_error".to_string(),
            Value::String(local_error.to_string()),
        );
    }
    if let Some(local_text) = local_status
        .map(|status| string_value(status.get("status")))
        .filter(|value| !value.trim().is_empty())
    {
        diagnosis.insert("local_status".to_string(), Value::String(local_text));
    }
    if let Some(service_head) = service_head {
        if let Some(value) = service_head.get("actor_membership_status") {
            diagnosis.insert("actor_membership_status".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("actor_recovery_eligible") {
            diagnosis.insert("actor_recovery_eligible".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("epoch") {
            diagnosis.insert("service_epoch".to_string(), value.clone());
        }
    }
    if let Some(local_epoch) = local_status.and_then(group_e2ee_local_epoch_from_status) {
        diagnosis.insert("local_epoch".to_string(), json!(local_epoch.to_string()));
    }
    let pending_commit_count = local_status
        .map(|status| values_from_array(status.get("pending_commits")).len() as i64)
        .unwrap_or_default();
    diagnosis.insert(
        "pending_commit_count".to_string(),
        json!(pending_commit_count),
    );

    let actor_status = service_head
        .map(|head| string_value(head.get("actor_membership_status")))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if matches!(
        actor_status.as_str(),
        "removed" | "left" | "non_member" | "inactive"
    ) {
        diagnosis.insert("state".to_string(), Value::String("inactive".to_string()));
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("fresh_normal_keypackage_then_group_add_e2ee".to_string()),
        );
        diagnosis.insert(
            "rejoin_command".to_string(),
            Value::String("group add --e2ee".to_string()),
        );
        diagnosis.insert("recover_member_allowed".to_string(), Value::Bool(false));
        diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
        return diagnosis;
    }
    if pending_commit_count > 0 {
        diagnosis.insert(
            "state".to_string(),
            Value::String("pending_commit".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("run_group_e2ee_repair".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
        return diagnosis;
    }
    if pending_notice_count > 0 {
        diagnosis.insert(
            "state".to_string(),
            Value::String("pending_notices".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("run_group_e2ee_repair".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
        return diagnosis;
    }

    let local_epoch = local_status.and_then(group_e2ee_local_epoch_from_status);
    let service_epoch = service_head.and_then(|head| i64_value(head.get("epoch")));
    let local_state = local_status
        .map(|status| string_value(status.get("status")))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if local_error.is_some()
        || local_state.is_empty()
        || local_state == "empty"
        || local_epoch.is_none()
    {
        diagnosis.insert(
            "state".to_string(),
            Value::String("missing_state".to_string()),
        );
        diagnosis.insert(
            "next_action".to_string(),
            Value::String("needs_snapshot_or_readd".to_string()),
        );
        diagnosis.insert(
            "active_recovery_command".to_string(),
            Value::String("group e2ee recover-member".to_string()),
        );
        diagnosis.insert(
            "removed_left_rejoin_command".to_string(),
            Value::String("group add --e2ee".to_string()),
        );
        diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
        return diagnosis;
    }
    if let (Some(local_epoch), Some(service_epoch)) = (local_epoch, service_epoch) {
        match local_epoch.cmp(&service_epoch) {
            std::cmp::Ordering::Equal => {
                diagnosis.insert("state".to_string(), Value::String("in_sync".to_string()));
                diagnosis.insert("next_action".to_string(), Value::String("none".to_string()));
                diagnosis.insert("fail_closed".to_string(), Value::Bool(false));
            }
            std::cmp::Ordering::Less => {
                diagnosis.insert("state".to_string(), Value::String("epoch_lag".to_string()));
                diagnosis.insert("epoch_gap".to_string(), json!(service_epoch - local_epoch));
                diagnosis.insert(
                    "next_action".to_string(),
                    Value::String("needs_snapshot_or_readd".to_string()),
                );
                diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
            }
            std::cmp::Ordering::Greater => {
                diagnosis.insert(
                    "state".to_string(),
                    Value::String("local_ahead".to_string()),
                );
                diagnosis.insert("epoch_gap".to_string(), json!(local_epoch - service_epoch));
                diagnosis.insert(
                    "next_action".to_string(),
                    Value::String("stop_and_inspect".to_string()),
                );
                diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
            }
        }
        return diagnosis;
    }
    diagnosis.insert("state".to_string(), Value::String("local_only".to_string()));
    diagnosis.insert(
        "next_action".to_string(),
        Value::String("inspect_service_head".to_string()),
    );
    diagnosis.insert("fail_closed".to_string(), Value::Bool(true));
    diagnosis
}

pub(crate) fn group_e2ee_recovery_artifact(
    record: &StoredIdentity,
    group_did: &str,
    device_id: &str,
    local_status: Option<&Map<String, Value>>,
    service_head: Option<&Map<String, Value>>,
    diagnosis: &Map<String, Value>,
) -> Value {
    if string_value(diagnosis.get("next_action")) != "needs_snapshot_or_readd" {
        return Value::Null;
    }
    let device_id = default_string(device_id.trim(), "default");
    let mut artifact = Map::new();
    artifact.insert(
        "recovery_type".to_string(),
        Value::String("same-device-owner-assisted".to_string()),
    );
    artifact.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    artifact.insert(
        "member_did".to_string(),
        Value::String(record.did.to_string()),
    );
    artifact.insert("device_id".to_string(), Value::String(device_id.clone()));
    artifact.insert(
        "diagnosis_state".to_string(),
        Value::String(string_value(diagnosis.get("state"))),
    );
    artifact.insert(
        "fail_closed".to_string(),
        diagnosis
            .get("fail_closed")
            .cloned()
            .unwrap_or(Value::Bool(true)),
    );
    artifact.insert("p4_membership_mutate".to_string(), Value::Bool(false));
    artifact.insert(
        "publish_command".to_string(),
        Value::String(format!(
            "group e2ee publish-key-package --recovery --group {group_did} --device {device_id}"
        )),
    );
    artifact.insert(
        "owner_command".to_string(),
        Value::String(format!(
            "group e2ee recover-member --group {group_did} --member {} --device {device_id}",
            record.did
        )),
    );
    if let Some(local_epoch) = local_status.and_then(group_e2ee_local_epoch_from_status) {
        artifact.insert("local_epoch".to_string(), json!(local_epoch.to_string()));
    }
    if let Some(service_head) = service_head {
        if let Some(value) = service_head.get("epoch") {
            artifact.insert("service_epoch".to_string(), value.clone());
        }
        if let Some(value) = service_head.get("actor_membership_status") {
            artifact.insert("member_status".to_string(), value.clone());
        }
    }
    Value::Object(artifact)
}

pub(crate) fn group_e2ee_local_epoch_from_status(status: &Map<String, Value>) -> Option<i64> {
    for key in ["epoch", "local_epoch"] {
        if let Some(epoch) = i64_value(status.get(key)) {
            return Some(epoch);
        }
    }
    for binding in values_from_array(status.get("bindings")) {
        if let Some(epoch) = i64_value(binding.get("epoch")) {
            return Some(epoch);
        }
    }
    None
}

pub(crate) fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}
