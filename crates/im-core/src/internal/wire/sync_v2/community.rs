//! Explicit single-device sync negotiation for Community Homes.
//!
//! These parsers select wire contracts only. The runtime must independently bind
//! the discovery source, account, device and installation before using a result.

use serde_json::{json, Value};

use super::{
    canonical_string_field, common, exact_fields, exact_mode, invalid_page, object, parse_cursor,
    reject_e2ee_value, required_string, warnings, SyncBootstrapV2, SyncLaneBootstrapV3,
    WireIdentity, MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1, SNAPSHOT_PAGING_V1, SYNC_V2_PROFILE,
};

pub(crate) const COMMUNITY_SYNC_V1: &str = "awiki.open.single-device-sync.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncServiceMode {
    Commercial,
    Community,
}

/// A missing or invalid declaration never grants Community semantics.
pub(crate) fn discover_sync_service_mode(raw: &Value) -> crate::ImResult<SyncServiceMode> {
    let response = object(raw, "anp.get_capabilities response")?;
    let profiles = response
        .get("supported_profiles")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("supported_profiles must be an array"))?;
    let advertised = profiles
        .iter()
        .any(|p| p.as_str() == Some(COMMUNITY_SYNC_V1));
    let feature = response
        .get("features")
        .and_then(Value::as_object)
        .and_then(|features| features.get("community_sync"));

    if !advertised && feature.is_none() {
        // Preserve the existing commercial gate, including all failure cases.
        super::require_explicit_sync_negotiation_capability(raw)?;
        return Ok(SyncServiceMode::Commercial);
    }
    if !advertised {
        return Err(invalid_page(
            "Community feature requires its supported profile",
        ));
    }
    let feature = object(
        feature
            .ok_or_else(|| invalid_page("Community profile requires its feature declaration"))?,
        "features.community_sync",
    )?;
    exact_fields(
        feature,
        &[
            "profile",
            "wire_profile",
            "max_devices",
            "max_client_instances",
            "snapshot",
            "lanes",
            "history",
            "ws_subprotocol",
        ],
        "features.community_sync",
    )?;
    let expected = json!({
        "profile": COMMUNITY_SYNC_V1,
        "wire_profile": SYNC_V2_PROFILE,
        "max_devices": 1,
        "max_client_instances": 1,
        "snapshot": false,
        "lanes": [],
        "history": "retained_log",
        "ws_subprotocol": "awiki.sync.event.v3"
    });
    if Value::Object(feature.clone()) != expected {
        return Err(invalid_page(
            "Community sync declaration has unsupported values",
        ));
    }
    canonical_string_field(response, "service_did")?;
    let mut unique = std::collections::BTreeSet::new();
    for profile in profiles {
        let profile = profile
            .as_str()
            .filter(|p| !p.is_empty() && p.trim() == *p)
            .ok_or_else(|| {
                invalid_page("Community supported profiles must be canonical strings")
            })?;
        if !unique.insert(profile) {
            return Err(invalid_page("Community supported profiles must be unique"));
        }
        if matches!(
            profile,
            MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1
                | SNAPSHOT_PAGING_V1
                | super::SYNC_CAPABILITY_P5_DEVICE_V1
                | super::SYNC_CAPABILITY_P6_GROUP_V1
                | super::P6_DELIVERY_CONTEXT_CAPABILITY_V1
                | "anp.direct.e2ee.v1"
                | "anp.direct.e2ee.v2"
                | "anp.group.e2ee.v1"
                | "anp.group.e2ee.v2"
        ) {
            return Err(invalid_page(
                "Community declaration conflicts with commercial sync or E2EE",
            ));
        }
    }
    if !unique.contains(SYNC_V2_PROFILE) {
        return Err(invalid_page(
            "Community declaration requires its sync wire profile",
        ));
    }
    Ok(SyncServiceMode::Community)
}

pub(crate) fn build_community_bootstrap_params(
    identity: &WireIdentity,
    client_instance_id: &str,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    let client_instance_id = required_string("client_instance_id", client_instance_id)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "client_instance_id": client_instance_id,
            "capabilities": {"sync_profile": SYNC_V2_PROFILE, "event_schema_max": 1}
        }
    }))
}

pub(crate) fn parse_community_bootstrap(raw: &Value) -> crate::ImResult<SyncBootstrapV2> {
    let response = object(raw, "Community sync.bootstrap response")?;
    exact_fields(
        response,
        &[
            "mode",
            "account_id",
            "device_id",
            "server_time",
            "cursor",
            "read_state_baseline",
            "group_state_baseline",
            "warnings",
        ],
        "Community sync.bootstrap response",
    )?;
    exact_mode(response, "tail_only")?;
    let baseline = |field: &str| -> crate::ImResult<Vec<Value>> {
        let values = response
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_page("Community baseline must be an array"))?;
        for value in values {
            object(value, "Community baseline item")?;
            reject_e2ee_value(value)?;
        }
        Ok(values.clone())
    };
    Ok(SyncBootstrapV2 {
        account_id: canonical_string_field(response, "account_id")?,
        device_id: canonical_string_field(response, "device_id")?,
        server_time: canonical_string_field(response, "server_time")?,
        cursor: parse_cursor(&response["cursor"])?,
        read_state_baseline: baseline("read_state_baseline")?,
        group_state_baseline: baseline("group_state_baseline")?,
        warnings: warnings(response.get("warnings"))?,
        lane_bootstrap: SyncLaneBootstrapV3::default(),
        p6_delivery_client_instance_id: None,
        snapshot_paging_v1: false,
    })
}

#[cfg(test)]
mod tests;
