use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::group_service::group_storage_key;
use super::service::{metadata_string, string_value};
use super::{build_group_e2ee_create_rpc_params, GROUP_E2EE_SECURITY_PROFILE};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::store;
use serde_json::{json, Map, Value};

pub(crate) fn create_group_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    group_did: &str,
) -> (Option<Map<String, Value>>, Vec<String>) {
    let provider = MlsExecProvider::new(resolved);
    let device_id = "default";
    let request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("group-e2ee-create-{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": {
            "agent_did": record.did,
            "device_id": device_id,
            "group_did": group_did,
        },
    });
    let mut mls_head = match provider.create_group(&request, &record.did, device_id) {
        Ok(result) => result,
        Err(err) => return (None, vec![format!("Group E2EE MLS create failed: {err}")]),
    };
    mls_head = attach_group_state_ref(
        mls_head,
        group_did,
        local_group_state_ref(resolved, record, group_did),
    );
    let mut transport = match GroupE2eeTransport::new(resolved, manager, record) {
        Ok(transport) => transport,
        Err(err) => {
            let mut result = Map::new();
            result.insert("mls".to_string(), Value::Object(mls_head));
            return (
                Some(result),
                vec![format!("Group E2EE service transport unavailable: {err}")],
            );
        }
    };
    let service_did = match transport.message_service_did() {
        Ok(service_did) => service_did,
        Err(err) => {
            let mut result = Map::new();
            result.insert("mls".to_string(), Value::Object(mls_head));
            return (
                Some(result),
                vec![format!("Group E2EE create delivery failed: {err}")],
            );
        }
    };
    let params =
        match build_group_e2ee_create_rpc_params(record, &service_did, group_did, mls_head.clone())
        {
            Ok(params) => params,
            Err(err) => {
                let mut result = Map::new();
                result.insert("mls".to_string(), Value::Object(mls_head));
                return (
                    Some(result),
                    vec![format!("Group E2EE create delivery failed: {err}")],
                );
            }
        };
    let delivery = match transport.rpc_call("group.e2ee.create", params) {
        Ok(delivery) => delivery,
        Err(err) => {
            let mut result = Map::new();
            result.insert("mls".to_string(), Value::Object(mls_head));
            return (
                Some(result),
                vec![format!("Group E2EE create delivery failed: {err}")],
            );
        }
    };
    let warnings = persist_group_e2ee_summary(resolved, record, group_did, &mls_head, &delivery);
    let mut result = Map::new();
    result.insert("mls".to_string(), Value::Object(mls_head));
    result.insert("delivery".to_string(), Value::Object(delivery));
    (Some(result), warnings)
}

pub(crate) fn local_group_state_ref(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> Map<String, Value> {
    let Some(snapshot) = cached_group_snapshot_raw(resolved, record, group_did) else {
        return minimal_group_state_ref(group_did);
    };
    group_state_ref_from_snapshot(group_did, &snapshot)
}

pub(crate) fn attach_group_state_ref(
    mut input: Map<String, Value>,
    group_did: &str,
    group_state_ref: Map<String, Value>,
) -> Map<String, Value> {
    let mut reference = if group_state_ref.is_empty() {
        minimal_group_state_ref(group_did)
    } else {
        group_state_ref
    };
    reference.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    input.insert("group_state_ref".to_string(), Value::Object(reference));
    input
}

fn group_state_ref_from_snapshot(group_did: &str, snapshot: &Value) -> Map<String, Value> {
    let mut reference = minimal_group_state_ref(group_did);
    let metadata = decoded_metadata(snapshot);
    let group_state_version = first_non_empty_string(&[
        snapshot.get("group_state_version"),
        metadata
            .as_ref()
            .and_then(|value| value.get("group_state_version")),
        metadata
            .as_ref()
            .and_then(|value| value.get("group_e2ee"))
            .and_then(|value| value.get("group_state_version")),
    ]);
    if !group_state_version.is_empty() {
        reference.insert(
            "group_state_version".to_string(),
            Value::String(group_state_version),
        );
    }
    let crypto_group_id = first_non_empty_string(&[metadata
        .as_ref()
        .and_then(|value| value.get("group_e2ee"))
        .and_then(|value| value.get("crypto_group_id_b64u"))]);
    if !crypto_group_id.is_empty() {
        reference.insert(
            "crypto_group_id_b64u".to_string(),
            Value::String(crypto_group_id),
        );
    }
    reference
}

pub(crate) fn persist_group_e2ee_summary(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    mls: &Map<String, Value>,
    delivery: &Map<String, Value>,
) -> Vec<String> {
    let existing_snapshot = cached_group_snapshot_raw(resolved, record, group_did);
    let existing_ref = existing_snapshot
        .as_ref()
        .map(|snapshot| group_state_ref_from_snapshot(group_did, snapshot))
        .unwrap_or_else(|| minimal_group_state_ref(group_did));
    let group_state_version = first_non_empty_string(&[
        mls.get("group_state_ref")
            .and_then(|value| value.get("group_state_version")),
        delivery
            .get("group_state_ref")
            .and_then(|value| value.get("group_state_version")),
        delivery.get("group_state_version"),
        existing_ref.get("group_state_version"),
    ]);
    let connection = match store::open(&resolved.paths) {
        Ok(connection) => connection,
        Err(err) => {
            return vec![format!(
                "Failed to open local store for group E2EE summary: {err}"
            )]
        }
    };
    if let Err(err) = store::ensure_schema(&connection) {
        return vec![format!(
            "Failed to ensure local schema for group E2EE summary: {err}"
        )];
    }
    let mut group_e2ee = Map::new();
    insert_string(
        &mut group_e2ee,
        "crypto_group_id_b64u",
        &string_value(first_present_value(&[
            mls.get("crypto_group_id_b64u"),
            delivery.get("crypto_group_id_b64u"),
        ])),
    );
    insert_string(
        &mut group_e2ee,
        "epoch",
        &string_value(first_present_value(&[
            mls.get("epoch"),
            delivery.get("epoch"),
        ])),
    );
    insert_string(
        &mut group_e2ee,
        "epoch_authenticator",
        &string_value(first_present_value(&[
            mls.get("epoch_authenticator"),
            mls.get("epoch_authenticator_b64u"),
            delivery.get("epoch_authenticator"),
        ])),
    );
    insert_string(
        &mut group_e2ee,
        "suite",
        &string_value(first_present_value(&[
            mls.get("suite"),
            delivery.get("suite"),
        ])),
    );
    insert_string(
        &mut group_e2ee,
        "updated_at",
        &string_value(delivery.get("updated_at")),
    );
    insert_string(
        &mut group_e2ee,
        "operation_id",
        &string_value(delivery.get("operation_id")),
    );
    insert_string(&mut group_e2ee, "group_state_version", &group_state_version);
    let mut metadata = Map::new();
    metadata.insert(
        "message_security_profile".to_string(),
        Value::String(GROUP_E2EE_SECURITY_PROFILE.to_string()),
    );
    metadata.insert("group_e2ee".to_string(), Value::Object(group_e2ee));
    if !group_state_version.is_empty() {
        metadata.insert(
            "group_state_version".to_string(),
            Value::String(group_state_version),
        );
    }
    if let Err(err) = store::upsert_group(
        &connection,
        store::GroupRecord {
            owner_did: record.did.clone(),
            group_id: group_storage_key(group_did),
            group_did: group_did.to_string(),
            membership_status: "active".to_string(),
            metadata: metadata_string(Value::Object(metadata)),
            credential_name: record.identity_name.clone(),
            ..store::GroupRecord::default()
        },
    ) {
        return vec![format!("Failed to persist group E2EE summary: {err}")];
    }
    Vec::new()
}

fn cached_group_snapshot_raw(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> Option<Value> {
    let connection = store::open(&resolved.paths).ok()?;
    store::ensure_schema(&connection).ok()?;
    store::get_group_snapshot(&connection, &record.did, &group_storage_key(group_did)).ok()
}

fn minimal_group_state_ref(group_did: &str) -> Map<String, Value> {
    let mut reference = Map::new();
    reference.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    reference
}

fn decoded_metadata(snapshot: &Value) -> Option<Value> {
    snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    for value in values.iter().flatten() {
        let text = string_value(Some(value));
        if !text.trim().is_empty() {
            return text;
        }
    }
    String::new()
}

fn first_present_value<'a>(values: &[Option<&'a Value>]) -> Option<&'a Value> {
    values.iter().find_map(|value| *value)
}

fn insert_string(target: &mut Map<String, Value>, key: &str, value: &str) {
    target.insert(key.to_string(), Value::String(default_string(value, "")));
}
