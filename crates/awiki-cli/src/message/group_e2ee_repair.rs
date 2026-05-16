use super::group_e2ee_create::persist_group_e2ee_summary;
use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_remove::finalize_prepared_group_e2ee_commit;
use super::group_e2ee_status::{
    group_e2ee_local_epoch_from_status, group_e2ee_recovery_artifact,
    group_e2ee_recovery_device_ids, group_e2ee_recovery_diagnosis, group_e2ee_status_for_recovery,
    values_from_array,
};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::group_service::compact_warnings;
use super::service::{i64_value, require_active_identity, string_value, CommandResult};
use super::MessageError;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use serde_json::{json, Map, Value};

pub fn repair_group_e2ee_notices(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
    group_did: &str,
    limit: i64,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, identity_name)?;
    let mut transport = GroupE2eeTransport::new(resolved, manager, &record)?;
    let mut warnings = Vec::new();
    let service_head = match transport.get_group_e2ee_head(group_did) {
        Ok(head) => Some(head),
        Err(err) => {
            warnings.push(format!(
                "Group E2EE service head unavailable during repair: {err}"
            ));
            None
        }
    };

    let finalized_pending = if let Some(service_head) = service_head.as_ref() {
        let (finalized, finalize_warnings) = finalize_accepted_pending_group_e2ee_commits(
            resolved,
            &record,
            group_did,
            service_head,
        );
        warnings.extend(finalize_warnings);
        finalized
    } else {
        Vec::new()
    };

    let limit = if limit <= 0 { 50 } else { limit };
    let pending = transport.pull_group_e2ee_notices(group_did, limit, false, Vec::new())?;
    let mut processed = Vec::new();
    let mut notice_ids = Vec::new();
    let provider = MlsExecProvider::new(resolved);
    for notice in notice_objects(pending.get("notices")) {
        let notice_type = string_value(notice.get("notice_type"));
        if !is_group_e2ee_welcome_notice_type(&notice_type) && notice_type != "commit-delivery" {
            continue;
        }
        let target_group_did = default_string(&string_value(notice.get("group_did")), group_did);
        if target_group_did.trim().is_empty() {
            warnings.push(format!(
                "Group E2EE repair skipped {notice_type} notice without group_did"
            ));
            continue;
        }
        let mut recipient =
            first_non_empty_string(&[notice.get("recipient_did"), notice.get("member_did")]);
        if is_group_e2ee_welcome_notice_type(&notice_type) && recipient.is_empty() {
            recipient = string_value(notice.get("subject_did"));
        }
        if !recipient.is_empty() && recipient != record.did {
            warnings.push(format!(
                "Group E2EE repair skipped notice for different recipient {recipient}"
            ));
            continue;
        }
        let (item, item_warnings) = if notice_type == "commit-delivery" {
            process_group_commit_notice(resolved, &provider, &record, &target_group_did, &notice)
        } else {
            process_group_welcome_notice(resolved, &provider, &record, &target_group_did, &notice)
        };
        if let Some(item) = item {
            if let Some(notice_id) = notice
                .get("notice_id")
                .map(|value| string_from_any(value))
                .filter(|value| !value.trim().is_empty())
            {
                notice_ids.push(notice_id);
            }
            processed.push(Value::Object(item));
        } else if is_group_e2ee_welcome_notice_type(&notice_type)
            && group_welcome_already_available(&provider, &record, &target_group_did, &notice)
        {
            if let Some(notice_id) = notice
                .get("notice_id")
                .map(|value| string_from_any(value))
                .filter(|value| !value.trim().is_empty())
            {
                notice_ids.push(notice_id);
            }
            processed.push(json!({
                "processed": true,
                "already_restored": true,
                "notice_id": notice.get("notice_id").cloned().unwrap_or(Value::Null),
                "group_did": target_group_did,
                "member_did": record.did,
                "device_id": default_string(&string_value(notice.get("device_id")), "default"),
            }));
        } else {
            warnings.extend(item_warnings);
        }
    }

    let delivered_result = if notice_ids.is_empty() {
        Value::Null
    } else {
        match transport.pull_group_e2ee_notices(
            group_did,
            notice_ids.len() as i64,
            true,
            notice_ids.clone(),
        ) {
            Ok(result) => Value::Object(result),
            Err(err) => {
                warnings.push(format!(
                    "Group E2EE repair processed notices but failed to mark delivered: {err}"
                ));
                Value::Null
            }
        }
    };

    let (local_status, local_device_id, local_error) =
        match group_e2ee_status_for_recovery(&provider, &record.did, group_did, "") {
            Ok((status, device_id)) => (Some(status), device_id, None),
            Err(err) => {
                warnings.push(format!(
                    "Group E2EE local MLS status unavailable after repair: {err}"
                ));
                (None, String::new(), Some(err.to_string()))
            }
        };
    let remaining_pending =
        (notice_objects(pending.get("notices")).len() as i64 - notice_ids.len() as i64).max(0);
    let diagnosis = group_e2ee_recovery_diagnosis(
        local_status.as_ref(),
        service_head.as_ref(),
        remaining_pending,
        local_error.as_deref(),
    );
    if string_value(diagnosis.get("next_action")) == "needs_snapshot_or_readd" {
        warnings.push(
            "Group E2EE repair could not prove epoch continuity; fail closed and ask the group owner to run group e2ee recover-member after this member publishes a --recovery --group KeyPackage."
                .to_string(),
        );
    }
    let recovery_artifact = group_e2ee_recovery_artifact(
        &record,
        group_did,
        &local_device_id,
        local_status.as_ref(),
        service_head.as_ref(),
        &diagnosis,
    );

    Ok(CommandResult {
        data: json!({
            "processed": processed,
            "processed_count": processed.len(),
            "finalized_pending_commits": finalized_pending,
            "finalized_pending_count": finalized_pending.len(),
            "pending_count": pending.get("pending_count").cloned().unwrap_or(Value::Null),
            "delivered_result": delivered_result,
            "group": group_did,
            "local": local_status.map(Value::Object).unwrap_or(Value::Null),
            "local_device_id": local_device_id,
            "service_head": service_head.map(Value::Object).unwrap_or(Value::Null),
            "diagnosis": Value::Object(diagnosis),
            "recovery_artifact": recovery_artifact,
        }),
        summary: "Replayed group E2EE pending notices".to_string(),
        warnings: compact_warnings(&mut warnings),
    })
}

fn process_group_commit_notice(
    resolved: &Resolved,
    provider: &MlsExecProvider,
    record: &StoredIdentity,
    group_did: &str,
    notice: &Map<String, Value>,
) -> (Option<Map<String, Value>>, Vec<String>) {
    let commit_b64u = string_value(notice.get("commit_b64u"));
    if commit_b64u.is_empty() {
        return (
            None,
            vec!["Group E2EE repair skipped commit notice missing commit_b64u".to_string()],
        );
    }
    let group_state_ref = group_state_ref_from_notice(group_did, notice);
    let mut warnings = Vec::new();
    for device_id in group_e2ee_recovery_device_ids(
        provider,
        &record.did,
        &string_value(notice.get("device_id")),
    ) {
        let request = json!({
            "api_version": ANP_MLS_API_VERSION,
            "request_id": format!("group-e2ee-commit-repair-{}", super::wire::generate_operation_id()),
            "agent_did": record.did,
            "device_id": device_id,
            "params": {
                "agent_did": record.did,
                "device_id": device_id,
                "group_did": group_did,
                "group_state_ref": Value::Object(group_state_ref.clone()),
                "commit_b64u": commit_b64u,
                "ratchet_tree_b64u": notice.get("ratchet_tree_b64u").cloned().unwrap_or(Value::Null),
                "group_info_b64u": notice.get("group_info_b64u").cloned().unwrap_or(Value::Null),
                "operation_id": notice.get("operation_id").cloned().unwrap_or(Value::Null),
                "notice_id": notice.get("notice_id").cloned().unwrap_or(Value::Null),
                "actor_did": notice.get("actor_did").cloned().unwrap_or(Value::Null),
                "subject_did": notice.get("subject_did").cloned().unwrap_or(Value::Null),
                "subject_status": notice.get("subject_status").cloned().unwrap_or(Value::Null),
                "from_epoch": notice.get("from_epoch").cloned().unwrap_or(Value::Null),
                "to_epoch": notice.get("to_epoch").cloned().unwrap_or(Value::Null),
                "crypto_group_id_b64u": notice.get("crypto_group_id_b64u").cloned().unwrap_or(Value::Null),
                "epoch_authenticator": first_present_value(&[
                    notice.get("epoch_authenticator"),
                    notice.get("epoch_authenticator_b64u"),
                ]).cloned().unwrap_or(Value::Null),
            },
        });
        match provider.process_commit(&request, &record.did, &device_id) {
            Ok(commit_result) => {
                warnings.extend(persist_group_e2ee_summary(
                    resolved,
                    record,
                    group_did,
                    &commit_result,
                    notice,
                ));
                return (
                    Some(json_object(json!({
                        "processed": true,
                        "notice_type": "commit-delivery",
                        "notice_id": notice.get("notice_id").cloned().unwrap_or(Value::Null),
                        "group_did": group_did,
                        "member_did": record.did,
                        "device_id": device_id,
                        "epoch": commit_result.get("epoch").cloned().unwrap_or(Value::Null),
                        "subject_did": notice.get("subject_did").cloned().unwrap_or(Value::Null),
                        "subject_status": notice.get("subject_status").cloned().unwrap_or(Value::Null),
                    }))),
                    warnings,
                );
            }
            Err(err) => {
                if let (true, status_warnings) = group_commit_notice_already_applied(
                    provider, record, group_did, &device_id, notice,
                ) {
                    return (
                        Some(json_object(json!({
                            "processed": true,
                            "already_applied": true,
                            "notice_type": "commit-delivery",
                            "notice_id": notice.get("notice_id").cloned().unwrap_or(Value::Null),
                            "group_did": group_did,
                            "member_did": record.did,
                            "device_id": device_id,
                            "epoch": notice.get("to_epoch").cloned().unwrap_or(Value::Null),
                            "subject_did": notice.get("subject_did").cloned().unwrap_or(Value::Null),
                            "subject_status": notice.get("subject_status").cloned().unwrap_or(Value::Null),
                        }))),
                        status_warnings,
                    );
                }
                warnings.push(format!(
                    "Group E2EE repair commit processing failed on device {device_id}: {err}"
                ));
            }
        }
    }
    (None, compact_warnings(&mut warnings))
}

fn process_group_welcome_notice(
    resolved: &Resolved,
    provider: &MlsExecProvider,
    record: &StoredIdentity,
    group_did: &str,
    notice: &Map<String, Value>,
) -> (Option<Map<String, Value>>, Vec<String>) {
    let welcome_b64u = string_value(notice.get("welcome_b64u"));
    if welcome_b64u.is_empty() {
        return (
            None,
            vec!["Group E2EE repair skipped welcome notice missing welcome_b64u".to_string()],
        );
    }
    let ratchet_tree_b64u = string_value(notice.get("ratchet_tree_b64u"));
    if ratchet_tree_b64u.is_empty() {
        return (
            None,
            vec!["Group E2EE repair skipped welcome notice missing ratchet_tree_b64u".to_string()],
        );
    }
    let device_id = default_string(&string_value(notice.get("device_id")), "default");
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-welcome-repair-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": {
            "agent_did": record.did,
            "device_id": device_id,
            "group_did": group_did,
            "welcome_b64u": welcome_b64u,
            "ratchet_tree_b64u": ratchet_tree_b64u,
            "group_state_ref": notice.get("group_state_ref").cloned().unwrap_or(Value::Null),
            "crypto_group_id_b64u": notice.get("crypto_group_id_b64u").cloned().unwrap_or(Value::Null),
            "epoch": first_present_value(&[notice.get("to_epoch"), notice.get("epoch")]).cloned().unwrap_or(Value::Null),
            "to_epoch": notice.get("to_epoch").cloned().unwrap_or(Value::Null),
            "from_epoch": notice.get("from_epoch").cloned().unwrap_or(Value::Null),
        },
    });
    let welcome_result = match provider.process_welcome(&request, &record.did, &device_id) {
        Ok(result) => result,
        Err(err) => {
            return (
                None,
                vec![format!(
                    "Group E2EE repair welcome processing failed: {err}"
                )],
            )
        }
    };
    let warnings = persist_group_e2ee_summary(resolved, record, group_did, &welcome_result, notice);
    (
        Some(json_object(json!({
            "processed": true,
            "notice_id": notice.get("notice_id").cloned().unwrap_or(Value::Null),
            "group_did": group_did,
            "member_did": record.did,
            "device_id": device_id,
            "epoch": welcome_result.get("epoch").cloned().unwrap_or(Value::Null),
        }))),
        warnings,
    )
}

fn finalize_accepted_pending_group_e2ee_commits(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    service_head: &Map<String, Value>,
) -> (Vec<Value>, Vec<String>) {
    let provider = MlsExecProvider::new(resolved);
    let status = match provider.status(&record.did, "default", group_did) {
        Ok(status) => status,
        Err(err) => {
            return (
                Vec::new(),
                vec![format!(
                    "Group E2EE pending commit status unavailable during repair: {err}"
                )],
            )
        }
    };
    let mut finalized = Vec::new();
    let mut warnings = Vec::new();
    for pending in notice_objects(status.get("pending_commits")) {
        let pending_id = string_value(pending.get("pending_commit_id"));
        if pending_id.is_empty() {
            continue;
        }
        if !group_e2ee_pending_commit_accepted_by_service(&pending, service_head) {
            warnings.push(format!(
                "Group E2EE pending commit {pending_id} retained: service head has not accepted its target epoch."
            ));
            continue;
        }
        let mut prepared = Map::new();
        prepared.insert(
            "pending_commit_id".to_string(),
            Value::String(pending_id.clone()),
        );
        match finalize_prepared_group_e2ee_commit(resolved, record, group_did, &prepared) {
            Ok(result) => finalized.push(Value::Object(result)),
            Err(err) => warnings.push(format!(
                "Group E2EE pending commit {pending_id} matched service head but local finalize failed: {err}"
            )),
        }
    }
    (finalized, warnings)
}

fn group_e2ee_pending_commit_accepted_by_service(
    pending: &Map<String, Value>,
    service_head: &Map<String, Value>,
) -> bool {
    if pending.is_empty() || service_head.is_empty() {
        return false;
    }
    let group_did = string_value(pending.get("group_did"));
    let service_group_did = string_value(service_head.get("group_did"));
    if !group_did.is_empty() && !service_group_did.is_empty() && group_did != service_group_did {
        return false;
    }
    let crypto_group_id = string_value(pending.get("crypto_group_id_b64u"));
    let service_crypto_group_id = string_value(service_head.get("crypto_group_id_b64u"));
    if !crypto_group_id.is_empty()
        && !service_crypto_group_id.is_empty()
        && crypto_group_id != service_crypto_group_id
    {
        return false;
    }
    let to_epoch = i64_value(pending.get("to_epoch"));
    let service_epoch = i64_value(service_head.get("epoch"));
    matches!((to_epoch, service_epoch), (Some(to), Some(service)) if service >= to)
}

fn group_commit_notice_already_applied(
    provider: &MlsExecProvider,
    record: &StoredIdentity,
    group_did: &str,
    device_id: &str,
    notice: &Map<String, Value>,
) -> (bool, Vec<String>) {
    let Some(to_epoch) = i64_value(notice.get("to_epoch")) else {
        return (false, Vec::new());
    };
    let status = match provider.status(&record.did, device_id, group_did) {
        Ok(status) => status,
        Err(err) => {
            return (
                false,
                vec![format!(
                    "Group E2EE repair could not inspect local status after commit failure: {err}"
                )],
            )
        }
    };
    let Some(local_epoch) = group_e2ee_local_epoch_from_status(&status) else {
        return (false, Vec::new());
    };
    if local_epoch < to_epoch {
        return (false, Vec::new());
    }
    let notice_crypto_group_id = string_value(notice.get("crypto_group_id_b64u"));
    let local_crypto_group_id = string_value(status.get("crypto_group_id_b64u"));
    if !notice_crypto_group_id.is_empty()
        && !local_crypto_group_id.is_empty()
        && notice_crypto_group_id != local_crypto_group_id
    {
        return (false, Vec::new());
    }
    (
        true,
        vec![
            "Group E2EE repair treated duplicate/already-applied commit notice as delivered."
                .to_string(),
        ],
    )
}

fn group_welcome_already_available(
    provider: &MlsExecProvider,
    record: &StoredIdentity,
    group_did: &str,
    notice: &Map<String, Value>,
) -> bool {
    let device_id = default_string(&string_value(notice.get("device_id")), "default");
    let Ok(status) = provider.status(&record.did, &device_id, group_did) else {
        return false;
    };
    if string_value(status.get("status")) != "active" {
        return false;
    }
    let Some(target_epoch) = group_e2ee_welcome_notice_target_epoch(notice) else {
        return true;
    };
    group_e2ee_local_epoch_from_status(&status)
        .map(|local_epoch| local_epoch >= target_epoch)
        .unwrap_or(false)
}

fn group_e2ee_welcome_notice_target_epoch(notice: &Map<String, Value>) -> Option<i64> {
    for key in ["to_epoch", "epoch", "local_epoch"] {
        if let Some(epoch) = i64_value(notice.get(key)) {
            return Some(epoch);
        }
    }
    None
}

fn group_state_ref_from_notice(group_did: &str, notice: &Map<String, Value>) -> Map<String, Value> {
    if let Some(group_state_ref) = notice
        .get("group_state_ref")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
    {
        return group_state_ref.clone();
    }
    let mut group_state_ref = Map::new();
    group_state_ref.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    if let Some(crypto_group_id) = notice.get("crypto_group_id_b64u") {
        group_state_ref.insert("crypto_group_id_b64u".to_string(), crypto_group_id.clone());
    }
    if let Some(from_epoch) = notice.get("from_epoch") {
        group_state_ref.insert("epoch".to_string(), from_epoch.clone());
    }
    group_state_ref
}

fn notice_objects(value: Option<&Value>) -> Vec<Map<String, Value>> {
    values_from_array(value)
        .into_iter()
        .filter_map(|value| value.as_object().cloned())
        .collect()
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .map(|value| value.map(string_from_any).unwrap_or_default())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
}

fn first_present_value<'a>(values: &[Option<&'a Value>]) -> Option<&'a Value> {
    values.iter().find_map(|value| *value)
}

fn is_group_e2ee_welcome_notice_type(notice_type: &str) -> bool {
    matches!(
        notice_type.trim(),
        "welcome-delivery" | "recovery-welcome-delivery" | "update-welcome-delivery"
    )
}

fn string_from_any(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => String::new(),
    }
}

fn json_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
