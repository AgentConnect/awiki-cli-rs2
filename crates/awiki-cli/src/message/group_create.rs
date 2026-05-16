use super::group_e2ee_create::create_group_e2ee;
use super::group_service::{
    cached_group_members, cached_group_snapshot, compact_warnings, group_control_source,
    group_did_from_result, normalize_group_snapshot, sync_group_state,
};
use super::service::{auth_session, require_active_identity, CommandResult};
use super::{
    build_group_create_rpc_params, Client, GroupCreateRequest, MessageError,
    GROUP_E2EE_SECURITY_PROFILE, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::Manager;
use crate::transportcfg::Profile;
use serde_json::{json, Value};

pub fn create_group(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupCreateRequest,
) -> Result<CommandResult, MessageError> {
    if request.name.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params =
        build_group_create_rpc_params(&record, &resolved.anp_service_did, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "group.create",
        params,
        &mut auth,
    )?;
    let group_did = group_did_from_result(&raw);
    let mut warnings = sync_group_state(resolved, manager, &record, &group_did, true);
    let mut e2ee_result = None;
    if group_request_uses_e2ee(&request) {
        let (candidate, e2ee_warnings) = create_group_e2ee(resolved, manager, &record, &group_did);
        warnings.extend(e2ee_warnings);
        e2ee_result = candidate;
    }
    let snapshot = cached_group_snapshot(resolved, &record, &group_did)
        .or_else(|| normalize_group_snapshot(&raw))
        .unwrap_or(Value::Null);
    let members = cached_group_members(resolved, &record, &group_did, 100).unwrap_or_default();
    let mut data = json!({
        "group": snapshot,
        "members": members,
        "delivery": raw,
        "source": group_control_source(&raw),
    });
    if let (Some(e2ee), Some(object)) = (e2ee_result, data.as_object_mut()) {
        object.insert("e2ee".to_string(), Value::Object(e2ee));
    }
    Ok(CommandResult {
        data,
        summary: format!("Created group {group_did}"),
        warnings: compact_warnings(&mut warnings),
    })
}

fn group_request_uses_e2ee(request: &GroupCreateRequest) -> bool {
    request.e2ee || request.message_security_profile.trim() == GROUP_E2EE_SECURITY_PROFILE
}
