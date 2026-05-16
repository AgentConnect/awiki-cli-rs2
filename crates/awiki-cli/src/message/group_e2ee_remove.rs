use super::group_e2ee_create::{
    attach_group_state_ref, local_group_state_ref, persist_group_e2ee_summary,
};
use super::group_e2ee_provider::{MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::group_service::{
    cached_group_members, cached_group_snapshot, compact_warnings, sync_group_state,
};
use super::service::{require_active_identity, resolve_target, string_value, CommandResult};
use super::{GroupE2eeProcessLeaveRequest, GroupLeaveRequest, GroupMemberRequest, MessageError};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use serde_json::{json, Map, Value};

const DEFAULT_PROCESS_LEAVE_REASON: &str = "leave request processed by owner";

pub(crate) fn remove_group_member_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &GroupMemberRequest,
) -> Result<(Map<String, Value>, Vec<String>), MessageError> {
    let provider = MlsExecProvider::new(resolved);
    let device_id = "default";
    let group_state_ref = local_group_state_ref(resolved, record, &request.group);
    let request_body = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-remove-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": {
            "agent_did": record.did,
            "actor_did": record.did,
            "device_id": device_id,
            "group_did": request.group,
            "member_did": request.member,
            "subject_did": request.member,
            "operation_id": format!("op-{}", super::wire::generate_operation_id()),
            "group_state_ref": group_state_ref,
        },
    });
    let mut prepared = provider.remove_member(&request_body, &record.did, device_id)?;
    prepared = attach_group_state_ref(prepared, &request.group, group_state_ref);
    submit_prepared_group_e2ee_commit(
        resolved,
        manager,
        record,
        &request.group,
        &request.member,
        &request.reason_text,
        prepared,
        |transport, prepared_commit| {
            transport.remove_group_e2ee(
                &request.group,
                &request.member,
                prepared_commit,
                &request.reason_text,
                &request.leave_request_id,
            )
        },
    )
}

pub(crate) fn remove_group_member_e2ee_result(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &GroupMemberRequest,
    member_handle: &str,
) -> Result<CommandResult, MessageError> {
    let (e2ee_result, e2ee_warnings) =
        remove_group_member_e2ee(resolved, manager, record, request)?;
    let mut warnings = e2ee_warnings;
    warnings.extend(sync_group_state(
        resolved,
        manager,
        record,
        &request.group,
        true,
    ));
    let snapshot = cached_group_snapshot(resolved, record, &request.group)
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    let members = cached_group_members(resolved, record, &request.group, 100).unwrap_or_default();
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": e2ee_result.get("delivery").cloned().unwrap_or(Value::Null),
            "member": {
                "did": request.member,
                "handle": member_handle,
            },
            "e2ee": e2ee_result,
        }),
        summary: "Removed member from group with group E2EE".to_string(),
        warnings: compact_warnings(&mut warnings),
    })
}

pub(crate) fn leave_group_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &GroupLeaveRequest,
) -> Result<(Map<String, Value>, Vec<String>), MessageError> {
    let mut transport = GroupE2eeTransport::new(resolved, manager, record)?;
    let delivery =
        transport.create_group_e2ee_leave_request(&request.group, &request.reason_text)?;
    let leave_request_id = first_non_empty_string(&[
        string_value(delivery.get("leave_request_id")),
        string_value(delivery.get("request_id")),
    ]);
    let mut data = Map::new();
    data.insert("delivery".to_string(), Value::Object(delivery));
    data.insert(
        "group_did".to_string(),
        Value::String(request.group.clone()),
    );
    data.insert("subject_did".to_string(), Value::String(record.did.clone()));
    data.insert(
        "subject_status".to_string(),
        Value::String("leave_requested".to_string()),
    );
    data.insert(
        "leave_request_id".to_string(),
        Value::String(leave_request_id),
    );
    Ok((
        data,
        vec![
            "Group E2EE leave request created; the group owner must process it with `group e2ee process-leave-request` to advance the MLS epoch."
                .to_string(),
        ],
    ))
}

pub fn process_group_e2ee_leave_request(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eeProcessLeaveRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageError::MemberRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let member = resolve_target(resolved, &request.member)?;
    let mutation = GroupMemberRequest {
        identity_name: request.identity_name,
        group: request.group.clone(),
        member: member.did.clone(),
        role: String::new(),
        reason_text: first_non_empty_string(&[
            request.reason_text.trim().to_string(),
            DEFAULT_PROCESS_LEAVE_REASON.to_string(),
        ]),
        e2ee: true,
        leave_request_id: request.leave_request_id.trim().to_string(),
    };
    let (e2ee_result, e2ee_warnings) =
        remove_group_member_e2ee(resolved, manager, &record, &mutation)?;
    let mut warnings = e2ee_warnings;
    warnings.extend(sync_group_state(
        resolved,
        manager,
        &record,
        &request.group,
        true,
    ));
    let snapshot = cached_group_snapshot(resolved, &record, &request.group).unwrap_or(Value::Null);
    let members = cached_group_members(resolved, &record, &request.group, 100).unwrap_or_default();
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": e2ee_result.get("delivery").cloned().unwrap_or(Value::Null),
            "member": {
                "did": member.did,
                "handle": member.handle,
            },
            "leave_request_id": mutation.leave_request_id,
            "e2ee": e2ee_result,
        }),
        summary: "Processed group E2EE leave request with epoch-advancing remove".to_string(),
        warnings: compact_warnings(&mut warnings),
    })
}

fn submit_prepared_group_e2ee_commit(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    group_did: &str,
    subject_did: &str,
    reason_text: &str,
    prepared: Map<String, Value>,
    submit: impl FnOnce(
        &mut GroupE2eeTransport<'_>,
        Map<String, Value>,
    ) -> Result<Map<String, Value>, MessageError>,
) -> Result<(Map<String, Value>, Vec<String>), MessageError> {
    let mut warnings = Vec::new();
    let mut transport = match GroupE2eeTransport::new(resolved, manager, record) {
        Ok(transport) => transport,
        Err(err) => {
            return Err(err);
        }
    };
    let delivery = match submit(&mut transport, prepared.clone()) {
        Ok(delivery) => delivery,
        Err(err) => {
            if should_abort_group_e2ee_pending_commit(&err) {
                match abort_prepared_group_e2ee_commit(resolved, record, group_did, &prepared) {
                    Ok(abort_result) => {
                        let _ = abort_result;
                        warnings.push(
                            "Group E2EE pending commit aborted after deterministic service rejection."
                                .to_string(),
                        );
                        return Err(MessageError::Internal(format!(
                            "{err}; local group E2EE pending commit aborted"
                        )));
                    }
                    Err(abort_err) => {
                        warnings.push(format!(
                            "Group E2EE pending commit abort failed after service rejection: {abort_err}"
                        ));
                    }
                }
            } else {
                warnings.push(
                    "Group E2EE pending commit left intact after retryable or unknown service failure; retry with the same operation_id or inspect group e2ee status before finalize/abort."
                        .to_string(),
                );
            }
            return Err(MessageError::Internal(format!(
                "{err}; local group E2EE pending commit retained for retry"
            )));
        }
    };
    let finalized =
        match finalize_prepared_group_e2ee_commit(resolved, record, group_did, &prepared) {
            Ok(finalized) => Some(finalized),
            Err(err) => {
                warnings.push(format!(
                    "Group E2EE service accepted commit but local finalize failed: {err}"
                ));
                None
            }
        };
    let summary_source = finalized.as_ref().unwrap_or(&prepared);
    warnings.extend(persist_group_e2ee_summary(
        resolved,
        record,
        group_did,
        summary_source,
        &delivery,
    ));
    let mut result = Map::new();
    result.insert("mls_prepare".to_string(), Value::Object(prepared));
    result.insert(
        "mls_finalize".to_string(),
        finalized.map(Value::Object).unwrap_or(Value::Null),
    );
    result.insert("delivery".to_string(), Value::Object(delivery));
    result.insert(
        "subject_did".to_string(),
        Value::String(subject_did.to_string()),
    );
    result.insert(
        "reason_text".to_string(),
        Value::String(reason_text.to_string()),
    );
    Ok((result, warnings))
}

fn finalize_prepared_group_e2ee_commit(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    prepared: &Map<String, Value>,
) -> Result<Map<String, Value>, MessageError> {
    let provider = MlsExecProvider::new(resolved);
    let device_id = "default";
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-commit-finalize-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": pending_commit_params(record, group_did, prepared),
    });
    provider.commit_finalize(&request, &record.did, device_id)
}

fn abort_prepared_group_e2ee_commit(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    prepared: &Map<String, Value>,
) -> Result<Map<String, Value>, MessageError> {
    let provider = MlsExecProvider::new(resolved);
    let device_id = "default";
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-commit-abort-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": pending_commit_params(record, group_did, prepared),
    });
    provider.commit_abort(&request, &record.did, device_id)
}

fn pending_commit_params(
    record: &StoredIdentity,
    group_did: &str,
    prepared: &Map<String, Value>,
) -> Map<String, Value> {
    let mut params = json_object(json!({
        "agent_did": record.did,
        "actor_did": record.did,
        "device_id": "default",
        "group_did": group_did,
        "commit_b64u": prepared.get("commit_b64u").cloned().unwrap_or(Value::Null),
    }));
    for key in [
        "pending_commit_id",
        "subject_did",
        "subject_status",
        "from_epoch",
        "to_epoch",
    ] {
        if let Some(value) = prepared.get(key) {
            params.insert(key.to_string(), value.clone());
        }
    }
    params
}

fn should_abort_group_e2ee_pending_commit(err: &MessageError) -> bool {
    let MessageError::Service(service_err) = err else {
        return false;
    };
    if service_err.status_code >= 500 {
        return false;
    }
    if service_err.status_code >= 400 {
        return true;
    }
    service_err.rpc_code >= 2000
}

fn first_non_empty_string(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn json_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
