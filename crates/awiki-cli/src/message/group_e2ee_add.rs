use super::group_e2ee_create::{
    attach_group_state_ref, local_group_state_ref, persist_group_e2ee_summary,
};
use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::{GroupMemberRequest, GROUP_E2EE_SECURITY_PROFILE};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use serde_json::{json, Map, Value};

pub(crate) fn add_group_member_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    group_did: &str,
    member_did: &str,
) -> (Option<Map<String, Value>>, Vec<String>) {
    let mut transport = match GroupE2eeTransport::new(resolved, manager, record) {
        Ok(transport) => transport,
        Err(err) => {
            return (
                None,
                vec![format!("Group E2EE service transport unavailable: {err}")],
            )
        }
    };
    let leased_package = match transport.get_group_e2ee_key_package(group_did, member_did) {
        Ok(leased_package) => leased_package,
        Err(err) => {
            return (
                None,
                vec![format!("Group E2EE member KeyPackage lookup failed: {err}")],
            )
        }
    };

    let provider = MlsExecProvider::new(resolved);
    let device_id = "default";
    let group_state_ref = local_group_state_ref(resolved, record, group_did);
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-add-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": {
            "agent_did": record.did,
            "device_id": device_id,
            "group_did": group_did,
            "member_did": member_did,
            "group_state_ref": group_state_ref,
            "group_key_package": leased_package.get("group_key_package").cloned().unwrap_or(Value::Null),
            "key_package_id": leased_package.get("key_package_id").cloned().unwrap_or(Value::Null),
            "target_key_package": Value::Object(leased_package.clone()),
        },
    });
    let mut mls_head = match provider.add_member(&request, &record.did, device_id) {
        Ok(result) => result,
        Err(err) => {
            let mut result = Map::new();
            result.insert(
                "leased_key_package".to_string(),
                Value::Object(redacted_key_package_summary(&leased_package)),
            );
            return (
                Some(result),
                vec![format!("Group E2EE MLS add-member failed: {err}")],
            );
        }
    };
    if let Some(key_package_id) = leased_package.get("key_package_id") {
        if !value_string(Some(key_package_id)).trim().is_empty() {
            mls_head.insert("key_package_id".to_string(), key_package_id.clone());
        }
    }
    if let Some(group_key_package) = leased_package.get("group_key_package") {
        mls_head.insert("group_key_package".to_string(), group_key_package.clone());
    }
    mls_head = attach_group_state_ref(mls_head, group_did, group_state_ref);

    let delivery = match transport.add_group_e2ee(group_did, member_did, mls_head.clone()) {
        Ok(delivery) => delivery,
        Err(err) => {
            let mut result = Map::new();
            result.insert("mls".to_string(), Value::Object(mls_head));
            result.insert(
                "leased_key_package".to_string(),
                Value::Object(redacted_key_package_summary(&leased_package)),
            );
            return (
                Some(result),
                vec![format!("Group E2EE add delivery failed: {err}")],
            );
        }
    };

    let mut warnings =
        persist_group_e2ee_summary(resolved, record, group_did, &mls_head, &delivery);
    let (local_welcome, local_welcome_warnings) = process_local_group_welcome(
        resolved,
        manager,
        member_did,
        group_did,
        &delivery,
        &leased_package,
    );
    warnings.extend(local_welcome_warnings);

    let mut result = Map::new();
    result.insert("mls".to_string(), Value::Object(mls_head));
    result.insert("delivery".to_string(), Value::Object(delivery));
    result.insert(
        "leased_key_package".to_string(),
        Value::Object(redacted_key_package_summary(&leased_package)),
    );
    if let Some(local_welcome) = local_welcome {
        result.insert("local_welcome".to_string(), Value::Object(local_welcome));
    }
    (Some(result), warnings)
}

pub(crate) fn group_member_mutation_uses_e2ee(
    request: &GroupMemberRequest,
    pre_mutation_snapshot: Option<&Value>,
    post_mutation_snapshot: Option<&Value>,
) -> bool {
    request.e2ee
        || pre_mutation_snapshot.is_some_and(group_snapshot_uses_e2ee)
        || post_mutation_snapshot.is_some_and(group_snapshot_uses_e2ee)
}

fn process_local_group_welcome(
    resolved: &Resolved,
    manager: &Manager,
    member_did: &str,
    group_did: &str,
    delivery: &Map<String, Value>,
    leased_package: &Map<String, Value>,
) -> (Option<Map<String, Value>>, Vec<String>) {
    let Some(notice) = e2ee_notice_object(delivery) else {
        return (None, Vec::new());
    };
    let welcome_b64u = value_string(notice.get("welcome_b64u"));
    if welcome_b64u.trim().is_empty() {
        return (None, Vec::new());
    }
    let ratchet_tree_b64u = value_string(notice.get("ratchet_tree_b64u"));
    if ratchet_tree_b64u.trim().is_empty() {
        return (
            None,
            vec![
                "Group E2EE local welcome processing skipped: notice missing ratchet_tree_b64u"
                    .to_string(),
            ],
        );
    }
    let Some(member_record) = local_identity_by_did(manager, member_did) else {
        return (None, Vec::new());
    };
    let device_id = group_e2ee_welcome_device_id(leased_package);
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-welcome-{}", super::wire::generate_operation_id()),
        "agent_did": member_record.did,
        "device_id": device_id,
        "params": {
            "agent_did": member_record.did,
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
    let provider = MlsExecProvider::new(resolved);
    let welcome_result = match provider.process_welcome(&request, &member_record.did, &device_id) {
        Ok(result) => result,
        Err(err) => {
            return (
                None,
                vec![format!(
                    "Group E2EE local welcome processing failed for member {member_did}: {err}"
                )],
            )
        }
    };
    let warnings = persist_group_e2ee_summary(
        resolved,
        &member_record,
        group_did,
        &welcome_result,
        delivery,
    );
    let mut result = Map::new();
    result.insert("processed".to_string(), Value::Bool(true));
    result.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    result.insert(
        "member_did".to_string(),
        Value::String(member_record.did.clone()),
    );
    result.insert("device_id".to_string(), Value::String(device_id));
    result.insert(
        "epoch".to_string(),
        welcome_result.get("epoch").cloned().unwrap_or(Value::Null),
    );
    (Some(result), warnings)
}

fn e2ee_notice_object(delivery: &Map<String, Value>) -> Option<&Map<String, Value>> {
    delivery.get("e2ee_notice").and_then(Value::as_object)
}

fn group_snapshot_uses_e2ee(snapshot: &Value) -> bool {
    if snapshot.is_null() {
        return false;
    }
    if value_string(snapshot.get("message_security_profile")) == GROUP_E2EE_SECURITY_PROFILE {
        return true;
    }
    if snapshot
        .get("group_policy")
        .and_then(Value::as_object)
        .map(|policy| value_string(policy.get("message_security_profile")))
        .is_some_and(|profile| profile == GROUP_E2EE_SECURITY_PROFILE)
    {
        return true;
    }
    decoded_metadata(snapshot)
        .as_ref()
        .map(|metadata| value_string(metadata.get("message_security_profile")))
        .is_some_and(|profile| profile == GROUP_E2EE_SECURITY_PROFILE)
}

fn local_identity_by_did(manager: &Manager, did: &str) -> Option<StoredIdentity> {
    for summary in manager.list().ok()? {
        if summary.did == did {
            return manager.load(&summary.identity_name).ok();
        }
    }
    None
}

fn group_e2ee_welcome_device_id(leased_package: &Map<String, Value>) -> String {
    if let Some(device_id) = leased_package
        .get("group_key_package")
        .and_then(Value::as_object)
        .and_then(|package| package.get("device_id"))
        .map(|value| value_string(Some(value)))
        .filter(|value| !value.trim().is_empty())
    {
        return device_id;
    }
    default_string(&value_string(leased_package.get("device_id")), "default")
}

fn redacted_key_package_summary(raw: &Map<String, Value>) -> Map<String, Value> {
    let mut summary = Map::new();
    summary.insert(
        "target_did".to_string(),
        raw.get("target_did").cloned().unwrap_or(Value::Null),
    );
    summary.insert(
        "key_package_id".to_string(),
        raw.get("key_package_id").cloned().unwrap_or(Value::Null),
    );
    summary.insert("leased".to_string(), Value::Bool(true));
    summary.insert("private_material".to_string(), Value::Bool(false));
    summary
}

fn first_present_value<'a>(values: &[Option<&'a Value>]) -> Option<&'a Value> {
    values.iter().find_map(|value| *value)
}

fn decoded_metadata(snapshot: &Value) -> Option<Map<String, Value>> {
    match snapshot.get("metadata") {
        Some(Value::Object(metadata)) => Some(metadata.clone()),
        Some(Value::String(value)) => serde_json::from_str::<Value>(value)
            .ok()
            .and_then(|value| value.as_object().cloned()),
        _ => None,
    }
}

fn value_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}
