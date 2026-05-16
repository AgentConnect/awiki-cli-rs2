use super::group_e2ee_add::{process_local_group_welcome, redacted_key_package_summary};
use super::group_e2ee_create::{
    attach_group_state_ref, local_group_state_ref, persist_group_e2ee_summary,
};
use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_remove::{
    abort_prepared_group_e2ee_commit, finalize_prepared_group_e2ee_commit,
    should_abort_group_e2ee_pending_commit,
};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::group_service::compact_warnings;
use super::service::{
    bool_value, require_active_identity, resolve_target, string_value, CommandResult,
};
use super::{GroupE2eeRecoverMemberRequest, MessageError};
use crate::config::Resolved;
use crate::identity::Manager;
use serde_json::{json, Map, Value};

pub fn recover_group_e2ee_member(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eeRecoverMemberRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageError::MemberRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let member = resolve_target(resolved, &request.member)?;
    let device_id = default_string(request.device_id.trim(), "default");
    let mut transport = GroupE2eeTransport::new(resolved, manager, &record)?;
    let mut warnings = Vec::new();
    match transport.get_group_e2ee_head(&request.group) {
        Ok(service_head) => {
            if !bool_value(service_head.get("actor_recovery_eligible")) {
                return Err(MessageError::Internal(format!(
                    "group E2EE recovery requires the actor to be the active owner before public discovery; role={} status={}",
                    string_value(service_head.get("actor_membership_role")),
                    string_value(service_head.get("actor_membership_status"))
                )));
            }
        }
        Err(err) => warnings.push(format!(
            "Group E2EE service head unavailable before recovery: {err}"
        )),
    }

    let leased_package =
        transport.get_group_e2ee_recovery_key_package(&request.group, &member.did, &device_id)?;
    let group_state_ref = local_group_state_ref(resolved, &record, &request.group);
    let operation_id = format!("op-{}", super::wire::generate_operation_id());
    let prepare_request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-recover-member-prepare-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": "default",
        "params": {
            "agent_did": record.did,
            "actor_did": record.did,
            "device_id": "default",
            "group_did": request.group,
            "target": {
                "agent_did": member.did,
                "device_id": device_id,
            },
            "target_did": member.did,
            "target_device_id": device_id,
            "recovery_key_package_id": leased_package.get("key_package_id").cloned().unwrap_or(Value::Null),
            "group_key_package": leased_package.get("group_key_package").cloned().unwrap_or(Value::Null),
            "target_key_package": Value::Object(leased_package.clone()),
            "operation_id": operation_id,
            "group_state_ref": group_state_ref,
            "p4_membership_mutate": false,
            "recovery_operation_purpose": "same-device-crypto-recovery",
        },
    });
    let provider = MlsExecProvider::new(resolved);
    let mut prepared = provider.recover_member_prepare(&prepare_request, &record.did, "default")?;
    prepared = attach_group_state_ref(prepared, &request.group, group_state_ref);

    let delivery = match transport.recover_group_e2ee_member(
        &request.group,
        &member.did,
        &device_id,
        prepared.clone(),
        leased_package.clone(),
    ) {
        Ok(delivery) => delivery,
        Err(err) => {
            if should_abort_group_e2ee_pending_commit(&err) {
                match abort_prepared_group_e2ee_commit(resolved, &record, &request.group, &prepared)
                {
                    Ok(abort_result) => {
                        let _ = abort_result;
                        warnings.push(
                            "Group E2EE recovery pending commit aborted after deterministic service rejection."
                                .to_string(),
                        );
                        return Err(MessageError::Internal(format!(
                            "{err}; local group E2EE recovery pending commit aborted"
                        )));
                    }
                    Err(abort_err) => warnings.push(format!(
                        "Group E2EE recovery pending commit abort failed after service rejection: {abort_err}"
                    )),
                }
            }
            return Err(err);
        }
    };

    let finalized =
        match finalize_prepared_group_e2ee_commit(resolved, &record, &request.group, &prepared) {
            Ok(finalized) => Some(finalized),
            Err(err) => {
                warnings.push(format!(
                    "Group E2EE recovery accepted by service but local finalize failed: {err}"
                ));
                None
            }
        };
    let summary_source = finalized.as_ref().unwrap_or(&prepared);
    warnings.extend(persist_group_e2ee_summary(
        resolved,
        &record,
        &request.group,
        summary_source,
        &delivery,
    ));
    let (local_welcome, local_welcome_warnings) = process_local_group_welcome(
        resolved,
        manager,
        &member.did,
        &request.group,
        &delivery,
        &leased_package,
    );
    warnings.extend(local_welcome_warnings);

    let mut data = json_object(json!({
        "group": request.group,
        "member": {
            "did": member.did,
            "handle": member.handle,
        },
        "target": {
            "agent_did": member.did,
            "device_id": device_id,
        },
        "recovery_key_package": Value::Object(redacted_key_package_summary(&leased_package)),
        "mls_prepare": Value::Object(prepared),
        "mls_finalize": finalized.map(Value::Object).unwrap_or(Value::Null),
        "delivery": Value::Object(delivery),
        "p4_membership_mutate": false,
        "argv_sensitive_fields": "stdin-json-only",
    }));
    if let Some(local_welcome) = local_welcome {
        data.insert("local_welcome".to_string(), Value::Object(local_welcome));
    }
    Ok(CommandResult {
        data: Value::Object(data),
        summary: "Recovered active group E2EE member without P4 membership mutation".to_string(),
        warnings: compact_warnings(&mut warnings),
    })
}

fn json_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
