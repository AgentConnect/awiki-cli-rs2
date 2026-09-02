use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::common::{self, WireIdentity};

pub(crate) const SYNC_V2_PROFILE: &str = "anp.sync.local.v2";
pub(crate) const MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1: &str =
    "awiki.message-sync.explicit-negotiation.v1";
pub(crate) const SNAPSHOT_PAGING_V1: &str = "sync.snapshot_paging.v1";
pub(crate) const SYNC_CAPABILITY_P5_DEVICE_V1: &str = "lanes.p5_device.v1";
pub(crate) const SYNC_CAPABILITY_P6_GROUP_V1: &str = "lanes.p6_group.v1";
pub(crate) const P6_DELIVERY_CONTEXT_CAPABILITY_V1: &str = "p6.delivery_context.v1";
pub(crate) const MESSAGE_GET_BATCH_MAX_EVENT_IDS: usize = 100;
pub(crate) const MESSAGE_GET_BATCH_CLIENT_CHUNK_EVENT_IDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncCursorV2 {
    pub(crate) stream_epoch: String,
    pub(crate) scan_seq: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SyncLaneV3 {
    P5Device,
    P6Group,
}

impl SyncLaneV3 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::P5Device => "p5_device",
            Self::P6Group => "p6_group",
        }
    }

    pub(crate) fn capability(self) -> &'static str {
        match self {
            Self::P5Device => SYNC_CAPABILITY_P5_DEVICE_V1,
            Self::P6Group => SYNC_CAPABILITY_P6_GROUP_V1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncLaneCursorV3 {
    pub(crate) cursor: SyncCursorV2,
    pub(crate) committed_seq: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SyncLaneBootstrapV3 {
    pub(crate) capabilities: BTreeSet<SyncLaneV3>,
    pub(crate) lanes: BTreeMap<SyncLaneV3, SyncLaneCursorV3>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncBootstrapV2 {
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) server_time: String,
    pub(crate) cursor: SyncCursorV2,
    pub(crate) read_state_baseline: Vec<Value>,
    pub(crate) group_state_baseline: Vec<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) lane_bootstrap: SyncLaneBootstrapV3,
    pub(crate) p6_delivery_client_instance_id: Option<String>,
    pub(crate) snapshot_paging_v1: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncBootstrapResponseV2 {
    TailOnly(SyncBootstrapV2),
    RecoveryRequired {
        recovery: SyncRecoveryV2,
        lane_bootstrap: SyncLaneBootstrapV3,
        p6_delivery_client_instance_id: Option<String>,
        snapshot_paging_v1: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncLaneEventV3 {
    P5Delivery {
        delivery_id: String,
        seq: String,
        envelope: Value,
    },
    P6Delivery {
        delivery_id: String,
        seq: String,
        group_did: String,
        group_event_seq: String,
        envelope: Value,
    },
    P6ControlNotice {
        notice_id: String,
        seq: String,
        group_did: String,
        notice: Value,
    },
}

impl SyncLaneEventV3 {
    pub(crate) fn seq(&self) -> &str {
        match self {
            Self::P5Delivery { seq, .. }
            | Self::P6Delivery { seq, .. }
            | Self::P6ControlNotice { seq, .. } => seq,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncLaneErrorV3 {
    pub(crate) code: i64,
    pub(crate) anp_code: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncLaneDeltaSectionV3 {
    Page {
        events: Vec<SyncLaneEventV3>,
        next_cursor: SyncCursorV2,
        has_more: bool,
    },
    Error(SyncLaneErrorV3),
    TransportInvalid,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncDeltaPageV2 {
    pub(crate) server_time: String,
    pub(crate) events: Vec<SyncEventV2>,
    pub(crate) next_cursor: SyncCursorV2,
    pub(crate) has_more: bool,
    pub(crate) warnings: Vec<String>,
    pub(crate) lanes: BTreeMap<SyncLaneV3, SyncLaneDeltaSectionV3>,
    pub(crate) lane_transport_invalid: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncDeltaResponseV2 {
    Delta(SyncDeltaPageV2),
    RecoveryRequired(SyncRecoveryV2),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SyncRecoveryV2 {
    pub(crate) recovery_id: String,
    pub(crate) token: String,
    pub(crate) stream_epoch: String,
    pub(crate) snapshot_scan_seq: String,
    pub(crate) expires_at: String,
    pub(crate) snapshot_schema: u32,
    pub(crate) snapshot_delivery: String,
}

impl std::fmt::Debug for SyncRecoveryV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncRecoveryV2")
            .field("recovery_id", &"<redacted>")
            .field("token", &"<redacted>")
            .field("stream_epoch", &"<redacted>")
            .field("snapshot_scan_seq", &"<redacted>")
            .field("expires_at", &"<redacted>")
            .field("snapshot_schema", &self.snapshot_schema)
            .field("snapshot_delivery", &self.snapshot_delivery)
            .finish()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncSnapshotV2 {
    pub(crate) snapshot_schema: u32,
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) server_time: String,
    pub(crate) server_cutoff: String,
    pub(crate) snapshot_cursor: SyncCursorV2,
    pub(crate) read_states: Vec<Value>,
    pub(crate) groups: Vec<Value>,
    pub(crate) recent_plain_messages: Vec<SnapshotPlainMessageV2>,
    pub(crate) unexpired_system_notifications: Vec<SnapshotSystemNotificationV2>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncSnapshotV3 {
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) server_time: String,
    pub(crate) server_cutoff: String,
    pub(crate) snapshot_cursor: SyncCursorV2,
    pub(crate) read_states: Vec<Value>,
    pub(crate) groups: Vec<Value>,
    pub(crate) recent_plain_messages: Vec<SnapshotPlainMessageV2>,
    pub(crate) unexpired_system_notifications: Vec<SnapshotSystemNotificationV2>,
    pub(crate) older_history_excluded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SnapshotPlainMessageV2 {
    pub(crate) event: SyncEventV2,
    pub(crate) message: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SnapshotSystemNotificationV2 {
    pub(crate) event: SyncEventV2,
    pub(crate) message: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SyncEventV2 {
    pub(crate) event_id: String,
    pub(crate) stream_epoch: String,
    pub(crate) event_seq: String,
    pub(crate) event_type: String,
    pub(crate) schema_version: u32,
    pub(crate) ignore_safe: bool,
    pub(crate) account_id: String,
    pub(crate) recipient_device_id: Option<String>,
    pub(crate) origin_did: Option<String>,
    pub(crate) origin_device_id: Option<String>,
    pub(crate) aggregate_kind: String,
    pub(crate) aggregate_id: String,
    pub(crate) state_version: Option<String>,
    pub(crate) thread_key: Option<String>,
    pub(crate) occurred_at: String,
    pub(crate) payload: Value,
    pub(crate) source: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HydratedMessageV2 {
    pub(crate) event_id: String,
    pub(crate) message: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MessageBatchV2 {
    pub(crate) items: Vec<HydratedMessageV2>,
    pub(crate) unavailable: Vec<String>,
}

pub(crate) const SNAPSHOT_PAGE_MAX_ITEMS: u64 = 100;
pub(crate) const SNAPSHOT_PAGE_MAX_ENCODED_BYTES: u64 = 1_048_576;
pub(crate) const SNAPSHOT_PACKAGE_MAX_ITEMS: u64 = 10_000;
pub(crate) const SNAPSHOT_PACKAGE_MAX_ENCODED_BYTES: u64 = 67_108_864;
pub(crate) const SNAPSHOT_PACKAGE_MAX_PAGES: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SnapshotSectionV3 {
    ReadStates,
    Groups,
    RecentPlainMessages,
    UnexpiredSystemNotifications,
}

impl SnapshotSectionV3 {
    pub(crate) const ORDERED: [Self; 4] = [
        Self::ReadStates,
        Self::Groups,
        Self::RecentPlainMessages,
        Self::UnexpiredSystemNotifications,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ReadStates => "read_states",
            Self::Groups => "groups",
            Self::RecentPlainMessages => "recent_plain_messages",
            Self::UnexpiredSystemNotifications => "unexpired_system_notifications",
        }
    }

    fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "read_states" => Ok(Self::ReadStates),
            "groups" => Ok(Self::Groups),
            "recent_plain_messages" => Ok(Self::RecentPlainMessages),
            "unexpired_system_notifications" => Ok(Self::UnexpiredSystemNotifications),
            _ => Err(invalid_page("snapshot page section is unknown")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotSectionSummaryV3 {
    pub(crate) item_count: u64,
    pub(crate) digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotRecoveryBudgetV3 {
    pub(crate) required_state_items: u64,
    pub(crate) required_state_encoded_bytes: u64,
    pub(crate) required_state_pages: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotHistoryPolicyV3 {
    pub(crate) returned_items: u64,
    pub(crate) returned_encoded_bytes: u64,
    pub(crate) returned_pages: u64,
    pub(crate) oldest_included_event_seq: Option<String>,
    pub(crate) excluded_older_messages: u64,
    pub(crate) older_history_excluded: bool,
    pub(crate) truncation_reason: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct SyncSnapshotManifestV3 {
    pub(crate) frozen_at: String,
    pub(crate) snapshot_cursor: SyncCursorV2,
    pub(crate) sections: BTreeMap<SnapshotSectionV3, SnapshotSectionSummaryV3>,
    pub(crate) recovery_budget: SnapshotRecoveryBudgetV3,
    pub(crate) history_policy: SnapshotHistoryPolicyV3,
    pub(crate) server_cutoff: String,
    pub(crate) total_items: u64,
    pub(crate) total_encoded_bytes: u64,
    pub(crate) total_pages: u64,
    pub(crate) manifest_digest: String,
    pub(crate) raw: Value,
}

impl std::fmt::Debug for SyncSnapshotManifestV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncSnapshotManifestV3")
            .field("total_items", &self.total_items)
            .field("total_encoded_bytes", &self.total_encoded_bytes)
            .field("total_pages", &self.total_pages)
            .field(
                "older_history_excluded",
                &self.history_policy.older_history_excluded,
            )
            .field("private_fields", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct SnapshotPageV3 {
    pub(crate) section: SnapshotSectionV3,
    pub(crate) items: Vec<Value>,
    pub(crate) returned_items: u64,
    pub(crate) returned_encoded_bytes: u64,
    pub(crate) page_digest: String,
    pub(crate) has_more: bool,
    pub(crate) next_page_ref: Option<String>,
}

impl std::fmt::Debug for SnapshotPageV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotPageV3")
            .field("section", &self.section)
            .field("returned_items", &self.returned_items)
            .field("returned_encoded_bytes", &self.returned_encoded_bytes)
            .field("page_digest", &self.page_digest)
            .field("has_more", &self.has_more)
            .field("items", &"<redacted>")
            .field("next_page_ref", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct SyncSnapshotPageV3 {
    pub(crate) recovery_id: String,
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) device_auth_generation: String,
    pub(crate) client_instance_id: String,
    pub(crate) server_time: String,
    pub(crate) snapshot_cursor: SyncCursorV2,
    pub(crate) manifest: Option<SyncSnapshotManifestV3>,
    pub(crate) manifest_digest: String,
    pub(crate) page: SnapshotPageV3,
}

impl std::fmt::Debug for SyncSnapshotPageV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncSnapshotPageV3")
            .field("recovery_id", &"<redacted>")
            .field("account_id", &"<redacted>")
            .field("device_id", &"<redacted>")
            .field("device_auth_generation", &"<redacted>")
            .field("client_instance_id", &"<redacted>")
            .field("server_time", &self.server_time)
            .field("snapshot_cursor", &"<redacted>")
            .field("manifest", &self.manifest)
            .field("manifest_digest", &self.manifest_digest)
            .field("page", &self.page)
            .finish()
    }
}

pub(crate) fn build_capability_discovery_params(identity: &WireIdentity) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    Ok(json!({
        "meta": common::local_meta(&did, "anp.core.binding.v1"),
        "body": {}
    }))
}

pub(crate) fn require_explicit_sync_negotiation_capability(raw: &Value) -> crate::ImResult<()> {
    let response = object(raw, "anp.get_capabilities response")?;
    let profiles = response
        .get("supported_profiles")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("supported_profiles must be an array"))?;
    let has_profile = |expected: &str| {
        profiles
            .iter()
            .any(|value| value.as_str() == Some(expected))
    };
    if has_profile(MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1) && has_profile(SNAPSHOT_PAGING_V1) {
        return Ok(());
    }
    Err(crate::ImError::unsupported(SNAPSHOT_PAGING_V1))
}

pub(crate) fn build_bootstrap_params(
    identity: &WireIdentity,
    client_instance_id: &str,
) -> crate::ImResult<Value> {
    build_bootstrap_params_with_lanes(identity, client_instance_id, &BTreeSet::new())
}

pub(crate) fn build_bootstrap_params_with_lanes(
    identity: &WireIdentity,
    client_instance_id: &str,
    lanes: &BTreeSet<SyncLaneV3>,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    let client_instance_id = required_string("client_instance_id", client_instance_id)?;
    let mut requested = Vec::new();
    if lanes.contains(&SyncLaneV3::P5Device) {
        requested.push(SYNC_CAPABILITY_P5_DEVICE_V1);
    }
    if lanes.contains(&SyncLaneV3::P6Group) {
        requested.push(SYNC_CAPABILITY_P6_GROUP_V1);
        requested.push(P6_DELIVERY_CONTEXT_CAPABILITY_V1);
    }
    let mut params = json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "client_instance_id": client_instance_id,
            "capabilities": {
                "sync_profile": SYNC_V2_PROFILE,
                "event_schema_max": 1,
                "requested_sync_capabilities": requested,
                "requested_snapshot_capabilities": {
                    "schema_max": 3,
                    "deliveries": ["paged_v1"]
                }
            }
        }
    });
    if lanes.contains(&SyncLaneV3::P6Group) {
        params["body"]["capabilities"]["p6_delivery"] =
            Value::String(P6_DELIVERY_CONTEXT_CAPABILITY_V1.to_owned());
    }
    Ok(params)
}

pub(crate) fn build_delta_params(
    identity: &WireIdentity,
    cursor: &SyncCursorV2,
    limit: u32,
    reason: &str,
    client_instance_id: &str,
) -> crate::ImResult<Value> {
    build_delta_params_with_lanes(
        identity,
        cursor,
        limit,
        reason,
        &BTreeMap::new(),
        client_instance_id,
    )
}

pub(crate) fn build_delta_params_with_lanes(
    identity: &WireIdentity,
    cursor: &SyncCursorV2,
    limit: u32,
    reason: &str,
    lanes: &BTreeMap<SyncLaneV3, SyncLaneCursorV3>,
    client_instance_id: &str,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    validate_cursor(cursor)?;
    let limit = validate_limit(limit)?;
    let reason = validate_reason(reason)?;
    let client_instance_id = required_string("client_instance_id", client_instance_id)?;
    let mut params = json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "cursor": {
                "stream_epoch": cursor.stream_epoch,
                "scan_seq": cursor.scan_seq
            },
            "limit": limit,
            "reason": reason
        }
    });
    if !lanes.is_empty() {
        let mut lane_sections = Map::new();
        for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
            let Some(state) = lanes.get(&lane) else {
                continue;
            };
            validate_cursor(&state.cursor)?;
            crate::internal::local_state::sync_v2::validate_decimal(
                "committed_seq",
                &state.committed_seq,
            )?;
            if crate::internal::local_state::sync_v2::compare_decimal(
                &state.committed_seq,
                &state.cursor.scan_seq,
            )? == std::cmp::Ordering::Greater
            {
                return Err(crate::ImError::invalid_input(
                    Some("lanes.committed_seq".to_owned()),
                    "lane committed_seq must not exceed scan_seq",
                ));
            }
            lane_sections.insert(
                lane.as_str().to_owned(),
                json!({
                    "cursor": {
                        "stream_epoch": state.cursor.stream_epoch,
                        "scan_seq": state.cursor.scan_seq,
                    },
                    "committed_seq": state.committed_seq,
                }),
            );
        }
        params
            .get_mut("body")
            .and_then(Value::as_object_mut)
            .expect("sync.delta body is an object")
            .insert("lanes".to_owned(), Value::Object(lane_sections));
    }
    if lanes.contains_key(&SyncLaneV3::P6Group) {
        params
            .get_mut("body")
            .and_then(Value::as_object_mut)
            .expect("sync.delta body is an object")
            .insert(
                "p6_delivery".to_owned(),
                json!({
                    "profile": P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                    "client_instance_id": client_instance_id
                }),
            );
    }
    Ok(params)
}

pub(crate) fn build_message_get_batch_params(
    identity: &WireIdentity,
    event_ids: &[String],
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    validate_event_ids(event_ids)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "event_ids": event_ids
        }
    }))
}

pub(crate) fn build_snapshot_params(
    identity: &WireIdentity,
    recovery: &SyncRecoveryV2,
) -> crate::ImResult<Value> {
    build_snapshot_page_params(identity, recovery, None)
}

pub(crate) fn build_snapshot_page_params(
    identity: &WireIdentity,
    recovery: &SyncRecoveryV2,
    page_ref: Option<&str>,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    let recovery_id = required_string("recovery_id", &recovery.recovery_id)?;
    let token = required_string("token", &recovery.token)?;
    let mut params = json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "recovery_id": recovery_id,
            "token": token
        }
    });
    if let Some(page_ref) = page_ref {
        params["body"]["page_ref"] = Value::String(required_string("page_ref", page_ref)?);
    }
    Ok(params)
}

pub(crate) fn build_thread_after_params(
    identity: &WireIdentity,
    thread_key: &str,
    after_server_seq: &str,
    limit: u32,
) -> crate::ImResult<Value> {
    let did = required_string("identity.did", identity.did.as_str())?;
    let thread_key = required_string("thread_key", thread_key)?;
    crate::internal::local_state::sync_v2::validate_decimal("after_server_seq", after_server_seq)?;
    let limit = validate_limit(limit)?;
    Ok(json!({
        "meta": common::local_meta(&did, SYNC_V2_PROFILE),
        "body": {
            "thread_key": thread_key,
            "after_server_seq": after_server_seq,
            "limit": limit
        }
    }))
}

pub(crate) fn validate_thread_after_response(
    raw: &Value,
    expected_thread_kind: &str,
) -> crate::ImResult<()> {
    if !matches!(expected_thread_kind, "direct" | "group") {
        return Err(crate::ImError::invalid_input(
            Some("thread_kind".to_owned()),
            "thread_kind must be direct or group",
        ));
    }
    let response = object(raw, "sync.thread_after response")?;
    let expected_fields = ["messages", "next_after_server_seq", "has_more", "warnings"];
    if response.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !response.contains_key(*field))
    {
        return Err(invalid_page(
            "sync.thread_after response fields do not match the v2 contract",
        ));
    }
    let messages = response
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("messages must be an array"))?;
    for message in messages {
        let message = object(message, "sync.thread_after message")?;
        if canonical_string_field(message, "thread_kind")? != expected_thread_kind {
            return Err(invalid_page(
                "sync.thread_after message thread_kind does not match the requested thread",
            ));
        }
    }
    decimal_field(response, "next_after_server_seq")?;
    response
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_page("has_more must be a boolean"))?;
    warnings(response.get("warnings"))?;
    Ok(())
}

pub(crate) fn parse_bootstrap(raw: &Value) -> crate::ImResult<SyncBootstrapV2> {
    match parse_bootstrap_response(raw)? {
        SyncBootstrapResponseV2::TailOnly(bootstrap) => Ok(bootstrap),
        SyncBootstrapResponseV2::RecoveryRequired { .. } => Err(sync_error(
            "SYNC_RECOVERY_REQUIRED",
            "sync.bootstrap requires compact recovery",
        )),
    }
}

pub(crate) fn parse_bootstrap_response(raw: &Value) -> crate::ImResult<SyncBootstrapResponseV2> {
    let object = object(raw, "sync.bootstrap response")?;
    let snapshot_paging_v1 = parse_snapshot_capability(object)?;
    let lane_bootstrap = parse_lane_bootstrap_v3(object)?;
    let p6_delivery_client_instance_id = parse_p6_delivery_bootstrap(object)?;
    if lane_bootstrap.capabilities.contains(&SyncLaneV3::P6Group)
        != p6_delivery_client_instance_id.is_some()
    {
        return Err(invalid_page(
            "P6 lane capability and p6_delivery activation must occur together",
        ));
    }
    if object.get("mode").and_then(Value::as_str) == Some("compact_recovery_required") {
        let recovery = self::object(
            object
                .get("recovery")
                .ok_or_else(|| invalid_page("bootstrap recovery is required"))?,
            "bootstrap recovery",
        )?;
        return Ok(SyncBootstrapResponseV2::RecoveryRequired {
            recovery: parse_recovery_descriptor_v3(recovery)?,
            lane_bootstrap,
            p6_delivery_client_instance_id,
            snapshot_paging_v1,
        });
    }
    exact_mode(object, "tail_only")?;
    let read_state_baseline = object
        .get("read_state_baseline")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("read_state_baseline must be an array"))?;
    for state in read_state_baseline {
        if !state.is_object() {
            return Err(invalid_page("read_state_baseline items must be objects"));
        }
        reject_e2ee_value(state)?;
    }
    let groups = object
        .get("group_state_baseline")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("group_state_baseline must be an array"))?
        .clone();
    for group in &groups {
        if !group.is_object() {
            return Err(invalid_page("group_state_baseline items must be objects"));
        }
        reject_e2ee_value(group)?;
    }
    Ok(SyncBootstrapResponseV2::TailOnly(SyncBootstrapV2 {
        account_id: canonical_string_field(object, "account_id")?,
        device_id: canonical_string_field(object, "device_id")?,
        server_time: canonical_string_field(object, "server_time")?,
        cursor: parse_cursor(
            object
                .get("cursor")
                .ok_or_else(|| invalid_page("cursor is required"))?,
        )?,
        read_state_baseline: read_state_baseline.clone(),
        group_state_baseline: groups,
        warnings: warnings(object.get("warnings"))?,
        lane_bootstrap,
        p6_delivery_client_instance_id,
        snapshot_paging_v1,
    }))
}

fn parse_snapshot_capability(response: &Map<String, Value>) -> crate::ImResult<bool> {
    let capability = response
        .get("snapshot_capability")
        .ok_or_else(|| invalid_page("snapshot_capability is required"))?;
    let capability = object(capability, "snapshot_capability")?;
    exact_fields(capability, &["schema", "delivery"], "snapshot_capability")?;
    if capability.get("schema").and_then(Value::as_u64) != Some(3)
        || capability.get("delivery").and_then(Value::as_str) != Some("paged_v1")
    {
        return Err(invalid_page(
            "snapshot_capability must confirm Schema 3 paged_v1",
        ));
    }
    Ok(true)
}

fn parse_p6_delivery_bootstrap(response: &Map<String, Value>) -> crate::ImResult<Option<String>> {
    let Some(value) = response.get("p6_delivery") else {
        return Ok(None);
    };
    let object = self::object(value, "p6_delivery")?;
    if object.len() != 3
        || object.get("profile").and_then(Value::as_str) != Some(P6_DELIVERY_CONTEXT_CAPABILITY_V1)
        || object.get("activated").and_then(Value::as_bool) != Some(true)
    {
        return Err(invalid_page(
            "p6_delivery must confirm p6.delivery_context.v1 activation",
        ));
    }
    canonical_string_field(object, "client_instance_id").map(Some)
}

fn parse_lane_bootstrap_v3(response: &Map<String, Value>) -> crate::ImResult<SyncLaneBootstrapV3> {
    let capabilities = match response.get("sync_capabilities") {
        None => BTreeSet::new(),
        Some(value) => {
            let values = value
                .as_array()
                .ok_or_else(|| invalid_page("sync_capabilities must be an array"))?;
            let mut raw = BTreeSet::new();
            let mut recognized = BTreeSet::new();
            for value in values {
                let value = value
                    .as_str()
                    .ok_or_else(|| invalid_page("sync_capabilities entries must be strings"))?;
                let value = required_string("sync_capabilities", value)
                    .map_err(|_| invalid_page("sync_capabilities entries must be canonical"))?;
                if !raw.insert(value.clone()) {
                    return Err(invalid_page("sync_capabilities contains a duplicate"));
                }
                for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
                    if value == lane.capability() {
                        recognized.insert(lane);
                    }
                }
            }
            recognized
        }
    };
    let lane_values = match response.get("lanes") {
        None if capabilities.is_empty() => return Ok(SyncLaneBootstrapV3::default()),
        None => {
            return Err(invalid_page(
                "advertised sync lanes require bootstrap cursors",
            ))
        }
        Some(value) => object(value, "bootstrap lanes")?,
    };
    if lane_values
        .keys()
        .any(|name| !matches!(name.as_str(), "p5_device" | "p6_group"))
    {
        return Err(invalid_page("bootstrap lanes contains an unknown lane"));
    }
    let mut lanes = BTreeMap::new();
    for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
        let Some(value) = lane_values.get(lane.as_str()) else {
            if capabilities.contains(&lane) {
                return Err(invalid_page(
                    "advertised sync lane is missing its bootstrap cursor",
                ));
            }
            continue;
        };
        if !capabilities.contains(&lane) {
            return Err(invalid_page(
                "bootstrap lane cursor was returned without its capability",
            ));
        }
        let section = object(value, "bootstrap lane")?;
        exact_fields(section, &["cursor", "committed_seq"], "bootstrap lane")?;
        let cursor = parse_cursor(
            section
                .get("cursor")
                .ok_or_else(|| invalid_page("bootstrap lane cursor is required"))?,
        )?;
        let committed_seq = decimal_field(section, "committed_seq")?;
        if crate::internal::local_state::sync_v2::compare_decimal(&committed_seq, &cursor.scan_seq)?
            == std::cmp::Ordering::Greater
        {
            return Err(invalid_page(
                "bootstrap lane committed_seq exceeds its scan cursor",
            ));
        }
        lanes.insert(
            lane,
            SyncLaneCursorV3 {
                cursor,
                committed_seq,
            },
        );
    }
    Ok(SyncLaneBootstrapV3 {
        capabilities,
        lanes,
    })
}

pub(crate) fn parse_delta(raw: &Value) -> crate::ImResult<SyncDeltaPageV2> {
    match parse_delta_response(raw)? {
        SyncDeltaResponseV2::Delta(page) => Ok(page),
        SyncDeltaResponseV2::RecoveryRequired(_) => Err(sync_error(
            "SYNC_RECOVERY_REQUIRED",
            "sync.delta requires compact recovery",
        )),
    }
}

pub(crate) fn parse_delta_response(raw: &Value) -> crate::ImResult<SyncDeltaResponseV2> {
    let object = object(raw, "sync.delta response")?;
    match object.get("mode").and_then(Value::as_str) {
        Some("compact_recovery_required") => {
            let events = object
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid_page("events must be an array"))?;
            if !events.is_empty()
                || object.get("next_cursor") != Some(&Value::Null)
                || object.get("has_more").and_then(Value::as_bool) != Some(false)
            {
                return Err(invalid_page(
                    "compact recovery response must not carry events or a next cursor",
                ));
            }
            let recovery = self::object(
                object
                    .get("recovery")
                    .ok_or_else(|| invalid_page("recovery is required"))?,
                "sync recovery",
            )?;
            return Ok(SyncDeltaResponseV2::RecoveryRequired(
                parse_recovery_descriptor_v3(recovery)?,
            ));
        }
        Some("delta") => {}
        _ => return Err(invalid_page("sync.delta response has an unsupported mode")),
    }
    if !matches!(object.get("recovery"), None | Some(Value::Null)) {
        return Err(invalid_page("delta response recovery must be null"));
    }
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("events must be an array"))?
        .iter()
        .map(parse_event)
        .collect::<crate::ImResult<Vec<_>>>()?;
    let mut event_ids = BTreeSet::new();
    let mut event_seqs = BTreeSet::new();
    for event in &events {
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(invalid_page("sync.delta contains a duplicate event_id"));
        }
        if !event_seqs.insert(event.event_seq.as_str()) {
            return Err(invalid_page("sync.delta contains a duplicate event_seq"));
        }
    }
    let (lanes, lane_transport_invalid) = parse_lane_delta_sections_v3(object.get("lanes"));
    Ok(SyncDeltaResponseV2::Delta(SyncDeltaPageV2 {
        server_time: canonical_string_field(object, "server_time")?,
        events,
        next_cursor: parse_cursor(
            object
                .get("next_cursor")
                .ok_or_else(|| invalid_page("next_cursor is required"))?,
        )?,
        has_more: object
            .get("has_more")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid_page("has_more must be a boolean"))?,
        warnings: warnings(object.get("warnings"))?,
        lanes,
        lane_transport_invalid,
    }))
}

fn parse_lane_delta_sections_v3(
    value: Option<&Value>,
) -> (BTreeMap<SyncLaneV3, SyncLaneDeltaSectionV3>, bool) {
    let Some(value) = value else {
        return (BTreeMap::new(), false);
    };
    let Some(sections) = value.as_object() else {
        return (BTreeMap::new(), true);
    };
    let transport_invalid = sections.is_empty()
        || sections
            .keys()
            .any(|name| !matches!(name.as_str(), "p5_device" | "p6_group"));
    let mut parsed = BTreeMap::new();
    for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
        let Some(value) = sections.get(lane.as_str()) else {
            continue;
        };
        let parsed_section = parse_lane_delta_section_v3(lane, value)
            .unwrap_or(SyncLaneDeltaSectionV3::TransportInvalid);
        parsed.insert(lane, parsed_section);
    }
    (parsed, transport_invalid)
}

fn parse_lane_delta_section_v3(
    lane: SyncLaneV3,
    value: &Value,
) -> crate::ImResult<SyncLaneDeltaSectionV3> {
    let section = object(value, "sync.delta lane section")?;
    let parsed_section = if let Some(error) = section.get("error") {
        exact_fields(section, &["error"], "sync.delta lane error section")?;
        let error = object(error, "sync.delta lane error")?;
        exact_fields(
            error,
            &["code", "anp_code", "message"],
            "sync.delta lane error",
        )?;
        SyncLaneDeltaSectionV3::Error(SyncLaneErrorV3 {
            code: error
                .get("code")
                .and_then(Value::as_i64)
                .ok_or_else(|| invalid_page("lane error code must be an integer"))?,
            anp_code: canonical_string_field(error, "anp_code")?,
            message: canonical_string_field(error, "message")?,
        })
    } else {
        exact_fields(
            section,
            &["events", "next_cursor", "has_more"],
            "sync.delta lane page",
        )?;
        let events = section
            .get("events")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_page("lane events must be an array"))?
            .iter()
            .map(|event| parse_lane_event_v3(lane, event))
            .collect::<crate::ImResult<Vec<_>>>()?;
        let next_cursor = parse_cursor(
            section
                .get("next_cursor")
                .ok_or_else(|| invalid_page("lane next_cursor is required"))?,
        )?;
        let mut previous_seq: Option<&str> = None;
        for event in &events {
            if previous_seq.is_some_and(|previous| {
                crate::internal::local_state::sync_v2::compare_decimal(previous, event.seq())
                    .map(|order| order != std::cmp::Ordering::Less)
                    .unwrap_or(true)
            }) {
                return Err(invalid_page(
                    "lane events must have unique increasing sequence values",
                ));
            }
            if crate::internal::local_state::sync_v2::compare_decimal(
                event.seq(),
                &next_cursor.scan_seq,
            )? == std::cmp::Ordering::Greater
            {
                return Err(invalid_page("lane event is ahead of its next cursor"));
            }
            previous_seq = Some(event.seq());
        }
        SyncLaneDeltaSectionV3::Page {
            events,
            next_cursor,
            has_more: section
                .get("has_more")
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid_page("lane has_more must be a boolean"))?,
        }
    };
    Ok(parsed_section)
}

fn parse_lane_event_v3(lane: SyncLaneV3, value: &Value) -> crate::ImResult<SyncLaneEventV3> {
    let event = object(value, "sync.delta lane event")?;
    let event_type = canonical_string_field(event, "event_type")?;
    match (lane, event_type.as_str()) {
        (SyncLaneV3::P5Device, "p5.delivery.created") => {
            exact_fields(
                event,
                &["event_type", "delivery_id", "seq", "envelope"],
                "p5 delivery event",
            )?;
            let envelope = event
                .get("envelope")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("p5 delivery envelope must be an object"))?;
            validate_lane_envelope_v3(&envelope, "anp.direct.e2ee.v2", "direct-e2ee")?;
            Ok(SyncLaneEventV3::P5Delivery {
                delivery_id: canonical_string_field(event, "delivery_id")?,
                seq: positive_decimal_field(event, "seq")?,
                envelope,
            })
        }
        (SyncLaneV3::P6Group, "p6.delivery.created") => {
            exact_fields(
                event,
                &[
                    "event_type",
                    "delivery_id",
                    "seq",
                    "group_did",
                    "group_event_seq",
                    "envelope",
                ],
                "p6 delivery event",
            )?;
            let envelope = event
                .get("envelope")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("p6 delivery envelope must be an object"))?;
            validate_lane_envelope_v3(&envelope, "anp.group.e2ee.v2", "group-e2ee")?;
            Ok(SyncLaneEventV3::P6Delivery {
                delivery_id: canonical_string_field(event, "delivery_id")?,
                seq: positive_decimal_field(event, "seq")?,
                group_did: canonical_string_field(event, "group_did")?,
                group_event_seq: positive_decimal_field(event, "group_event_seq")?,
                envelope,
            })
        }
        (SyncLaneV3::P6Group, "p6.control.notice") => {
            exact_fields(
                event,
                &["event_type", "notice_id", "seq", "group_did", "notice"],
                "p6 control notice",
            )?;
            let notice = event
                .get("notice")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("p6 control notice must be an object"))?;
            validate_lane_envelope_v3(
                &notice,
                "anp.group.e2ee.v2",
                anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2,
            )?;
            Ok(SyncLaneEventV3::P6ControlNotice {
                notice_id: canonical_string_field(event, "notice_id")?,
                seq: positive_decimal_field(event, "seq")?,
                group_did: canonical_string_field(event, "group_did")?,
                notice,
            })
        }
        _ => Err(invalid_page(
            "sync.delta lane event does not match its closed lane shape",
        )),
    }
}

pub(crate) fn validate_lane_envelope_v3(
    value: &Value,
    expected_profile: &str,
    expected_security_profile: &str,
) -> crate::ImResult<()> {
    let envelope = object(value, "E2EE lane envelope")?;
    let meta = envelope
        .get("meta")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_page("E2EE lane envelope meta must be an object"))?;
    if meta.get("profile").and_then(Value::as_str) != Some(expected_profile)
        || meta.get("security_profile").and_then(Value::as_str) != Some(expected_security_profile)
        || !envelope.get("body").is_some_and(Value::is_object)
    {
        return Err(invalid_page(
            "E2EE lane envelope profile does not match its lane",
        ));
    }
    Ok(())
}

pub(crate) fn parse_snapshot_page_v3(
    raw: &Value,
    first_page: bool,
) -> crate::ImResult<SyncSnapshotPageV3> {
    let response = object(raw, "Schema 3 sync.snapshot response")?;
    let mut expected = vec![
        "mode",
        "recovery_id",
        "account_id",
        "device_id",
        "device_auth_generation",
        "client_instance_id",
        "server_time",
        "snapshot_schema",
        "snapshot_delivery",
        "snapshot_cursor",
        "page",
    ];
    expected.push(if first_page {
        "manifest"
    } else {
        "manifest_digest"
    });
    exact_fields(response, &expected, "Schema 3 sync.snapshot response")?;
    exact_mode(response, "compact_recovery")?;
    if response.get("snapshot_schema").and_then(Value::as_u64) != Some(3)
        || response.get("snapshot_delivery").and_then(Value::as_str) != Some("paged_v1")
    {
        return Err(invalid_page(
            "sync.snapshot must use Schema 3 paged_v1 without fallback",
        ));
    }
    let manifest = if first_page {
        Some(parse_snapshot_manifest_v3(
            response
                .get("manifest")
                .ok_or_else(|| invalid_page("first snapshot page requires manifest"))?,
        )?)
    } else {
        None
    };
    let manifest_digest = match &manifest {
        Some(manifest) => manifest.manifest_digest.clone(),
        None => digest_field(response, "manifest_digest")?,
    };
    let page = parse_snapshot_page_envelope_v3(
        response
            .get("page")
            .ok_or_else(|| invalid_page("snapshot page is required"))?,
    )?;
    Ok(SyncSnapshotPageV3 {
        recovery_id: canonical_string_field(response, "recovery_id")?,
        account_id: canonical_string_field(response, "account_id")?,
        device_id: canonical_string_field(response, "device_id")?,
        device_auth_generation: positive_decimal_field(response, "device_auth_generation")?,
        client_instance_id: canonical_string_field(response, "client_instance_id")?,
        server_time: canonical_timestamp_field(response, "server_time")?,
        snapshot_cursor: parse_cursor(
            response
                .get("snapshot_cursor")
                .ok_or_else(|| invalid_page("snapshot_cursor is required"))?,
        )?,
        manifest,
        manifest_digest,
        page,
    })
}

fn parse_snapshot_page_envelope_v3(raw: &Value) -> crate::ImResult<SnapshotPageV3> {
    let page = object(raw, "snapshot page")?;
    exact_fields(
        page,
        &[
            "section",
            "items",
            "returned_items",
            "returned_encoded_bytes",
            "page_digest",
            "has_more",
            "next_page_ref",
        ],
        "snapshot page",
    )?;
    let section = SnapshotSectionV3::parse(&canonical_string_field(page, "section")?)?;
    let items = page
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| invalid_page("snapshot page items must be an array"))?;
    let returned_items = u64_field(page, "returned_items")?;
    if returned_items != items.len() as u64 || returned_items > SNAPSHOT_PAGE_MAX_ITEMS {
        return Err(invalid_page(
            "snapshot page returned_items does not match its bounded item array",
        ));
    }
    let actual_item_bytes = items.iter().try_fold(0_u64, |total, item| {
        let encoded = serde_json_canonicalizer::to_vec(item)
            .map_err(|_| invalid_page("snapshot page item is not canonicalizable"))?;
        Ok::<_, crate::ImError>(
            total.saturating_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX)),
        )
    })?;
    let returned_encoded_bytes = u64_field(page, "returned_encoded_bytes")?;
    if returned_encoded_bytes != actual_item_bytes
        || returned_encoded_bytes > SNAPSHOT_PAGE_MAX_ENCODED_BYTES
    {
        return Err(invalid_page(
            "snapshot page returned_encoded_bytes does not match item JCS",
        ));
    }
    let page_digest = digest_field(page, "page_digest")?;
    if page_digest != canonical_digest(&items)? {
        return Err(invalid_page("snapshot page digest mismatch"));
    }
    let has_more = page
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_page("snapshot page has_more must be a boolean"))?;
    let next_page_ref = match page.get("next_page_ref") {
        Some(Value::Null) => None,
        Some(Value::String(value)) => Some(required_string("next_page_ref", value)?),
        _ => {
            return Err(invalid_page(
                "snapshot page next_page_ref has invalid shape",
            ))
        }
    };
    if has_more != next_page_ref.is_some() || (has_more && items.is_empty()) {
        return Err(invalid_page(
            "snapshot page continuation fields are inconsistent",
        ));
    }
    Ok(SnapshotPageV3 {
        section,
        items,
        returned_items,
        returned_encoded_bytes,
        page_digest,
        has_more,
        next_page_ref,
    })
}

fn parse_snapshot_manifest_v3(raw: &Value) -> crate::ImResult<SyncSnapshotManifestV3> {
    let manifest = object(raw, "snapshot manifest")?;
    exact_fields(
        manifest,
        &[
            "manifest_schema",
            "frozen_at",
            "snapshot_cursor",
            "sections",
            "recovery_budget",
            "history_policy",
            "message_policy",
            "system_notification_policy",
            "excluded",
            "total_items",
            "total_encoded_bytes",
            "total_pages",
            "manifest_digest",
        ],
        "snapshot manifest",
    )?;
    if manifest.get("manifest_schema").and_then(Value::as_u64) != Some(1) {
        return Err(invalid_page("snapshot manifest_schema must equal 1"));
    }
    let sections_object = object(
        manifest
            .get("sections")
            .ok_or_else(|| invalid_page("snapshot manifest sections are required"))?,
        "snapshot manifest sections",
    )?;
    exact_fields(
        sections_object,
        &SnapshotSectionV3::ORDERED.map(SnapshotSectionV3::as_str),
        "snapshot manifest sections",
    )?;
    let mut sections = BTreeMap::new();
    for section in SnapshotSectionV3::ORDERED {
        let summary = object(
            sections_object
                .get(section.as_str())
                .ok_or_else(|| invalid_page("snapshot section summary is missing"))?,
            "snapshot section summary",
        )?;
        exact_fields(
            summary,
            &["item_count", "digest"],
            "snapshot section summary",
        )?;
        sections.insert(
            section,
            SnapshotSectionSummaryV3 {
                item_count: u64_field(summary, "item_count")?,
                digest: digest_field(summary, "digest")?,
            },
        );
    }
    let budget = object(
        manifest
            .get("recovery_budget")
            .ok_or_else(|| invalid_page("recovery_budget is required"))?,
        "snapshot recovery budget",
    )?;
    exact_fields(
        budget,
        &[
            "max_items",
            "max_encoded_bytes",
            "max_pages",
            "required_state_items",
            "required_state_encoded_bytes",
            "required_state_pages",
        ],
        "snapshot recovery budget",
    )?;
    if u64_field(budget, "max_items")? != SNAPSHOT_PACKAGE_MAX_ITEMS
        || u64_field(budget, "max_encoded_bytes")? != SNAPSHOT_PACKAGE_MAX_ENCODED_BYTES
        || u64_field(budget, "max_pages")? != SNAPSHOT_PACKAGE_MAX_PAGES
    {
        return Err(invalid_page("snapshot recovery budget constants mismatch"));
    }
    let recovery_budget = SnapshotRecoveryBudgetV3 {
        required_state_items: u64_field(budget, "required_state_items")?,
        required_state_encoded_bytes: u64_field(budget, "required_state_encoded_bytes")?,
        required_state_pages: u64_field(budget, "required_state_pages")?,
    };
    let history = object(
        manifest
            .get("history_policy")
            .ok_or_else(|| invalid_page("history_policy is required"))?,
        "snapshot history policy",
    )?;
    exact_fields(
        history,
        &[
            "selection",
            "returned_items",
            "returned_encoded_bytes",
            "returned_pages",
            "oldest_included_event_seq",
            "excluded_older_messages",
            "older_history_excluded",
            "truncation_reason",
            "complete_within_policy",
        ],
        "snapshot history policy",
    )?;
    if history.get("selection").and_then(Value::as_str) != Some("newest_complete_suffix")
        || history
            .get("complete_within_policy")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(invalid_page("snapshot history policy is not complete"));
    }
    let returned_items = u64_field(history, "returned_items")?;
    let oldest_included_event_seq = match history.get("oldest_included_event_seq") {
        Some(Value::Null) if returned_items == 0 => None,
        Some(Value::String(value)) if returned_items > 0 => {
            crate::internal::local_state::sync_v2::validate_decimal(
                "oldest_included_event_seq",
                value,
            )?;
            Some(value.clone())
        }
        _ => {
            return Err(invalid_page(
                "oldest_included_event_seq does not match returned history",
            ))
        }
    };
    let excluded_older_messages = u64_field(history, "excluded_older_messages")?;
    let older_history_excluded = history
        .get("older_history_excluded")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_page("older_history_excluded must be a boolean"))?;
    let truncation_reason = match history.get("truncation_reason") {
        Some(Value::Null) => None,
        Some(Value::String(value))
            if matches!(
                value.as_str(),
                "max_items" | "max_encoded_bytes" | "max_pages"
            ) =>
        {
            Some(value.clone())
        }
        _ => return Err(invalid_page("snapshot truncation_reason is invalid")),
    };
    if older_history_excluded != (excluded_older_messages > 0 && truncation_reason.is_some())
        || (!older_history_excluded
            && (excluded_older_messages != 0 || truncation_reason.is_some()))
    {
        return Err(invalid_page(
            "snapshot bounded-history declaration is inconsistent",
        ));
    }
    let history_policy = SnapshotHistoryPolicyV3 {
        returned_items,
        returned_encoded_bytes: u64_field(history, "returned_encoded_bytes")?,
        returned_pages: u64_field(history, "returned_pages")?,
        oldest_included_event_seq,
        excluded_older_messages,
        older_history_excluded,
        truncation_reason,
    };
    let message_policy = object(
        manifest
            .get("message_policy")
            .ok_or_else(|| invalid_page("message_policy is required"))?,
        "snapshot message policy",
    )?;
    exact_fields(
        message_policy,
        &["server_cutoff", "selection"],
        "snapshot message policy",
    )?;
    if message_policy.get("selection").and_then(Value::as_str) != Some("ordinary_plain_only") {
        return Err(invalid_page("snapshot message selection is invalid"));
    }
    let system_policy = object(
        manifest
            .get("system_notification_policy")
            .ok_or_else(|| invalid_page("system_notification_policy is required"))?,
        "snapshot system notification policy",
    )?;
    exact_fields(
        system_policy,
        &["scope", "complete_through_scan_seq", "complete"],
        "snapshot system notification policy",
    )?;
    if system_policy.get("scope").and_then(Value::as_str) != Some("exact_device_unexpired")
        || system_policy.get("complete").and_then(Value::as_bool) != Some(true)
    {
        return Err(invalid_page(
            "snapshot system notification policy is incomplete",
        ));
    }
    let excluded = object(
        manifest
            .get("excluded")
            .ok_or_else(|| invalid_page("snapshot excluded policy is required"))?,
        "snapshot excluded policy",
    )?;
    exact_fields(
        excluded,
        &["e2ee_messages", "plain_messages_before_cutoff"],
        "snapshot excluded policy",
    )?;
    if excluded.get("e2ee_messages").and_then(Value::as_bool) != Some(true)
        || excluded
            .get("plain_messages_before_cutoff")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(invalid_page("snapshot excluded policy is invalid"));
    }
    let total_items = u64_field(manifest, "total_items")?;
    let total_encoded_bytes = u64_field(manifest, "total_encoded_bytes")?;
    let total_pages = u64_field(manifest, "total_pages")?;
    if total_items > SNAPSHOT_PACKAGE_MAX_ITEMS
        || total_encoded_bytes > SNAPSHOT_PACKAGE_MAX_ENCODED_BYTES
        || !(1..=SNAPSHOT_PACKAGE_MAX_PAGES).contains(&total_pages)
        || sections
            .values()
            .map(|summary| summary.item_count)
            .sum::<u64>()
            != total_items
        || sections[&SnapshotSectionV3::RecentPlainMessages].item_count
            != history_policy.returned_items
        || sections[&SnapshotSectionV3::ReadStates]
            .item_count
            .saturating_add(sections[&SnapshotSectionV3::Groups].item_count)
            .saturating_add(sections[&SnapshotSectionV3::UnexpiredSystemNotifications].item_count)
            != recovery_budget.required_state_items
    {
        return Err(invalid_page("snapshot manifest totals are inconsistent"));
    }
    let page_sum = recovery_budget
        .required_state_pages
        .saturating_add(history_policy.returned_pages);
    if page_sum != total_pages && !(total_items == 0 && page_sum == 0 && total_pages == 1) {
        return Err(invalid_page(
            "snapshot manifest page totals are inconsistent",
        ));
    }
    let snapshot_cursor = parse_cursor(
        manifest
            .get("snapshot_cursor")
            .ok_or_else(|| invalid_page("manifest snapshot_cursor is required"))?,
    )?;
    if decimal_field(system_policy, "complete_through_scan_seq")? != snapshot_cursor.scan_seq {
        return Err(invalid_page(
            "notification policy does not match the snapshot anchor",
        ));
    }
    let manifest_digest = digest_field(manifest, "manifest_digest")?;
    let mut unsigned = raw.clone();
    unsigned
        .as_object_mut()
        .expect("manifest was validated as object")
        .remove("manifest_digest");
    if manifest_digest != canonical_digest(&unsigned)? {
        return Err(invalid_page("snapshot manifest digest mismatch"));
    }
    Ok(SyncSnapshotManifestV3 {
        frozen_at: canonical_timestamp_field(manifest, "frozen_at")?,
        snapshot_cursor,
        sections,
        recovery_budget,
        history_policy,
        server_cutoff: canonical_timestamp_field(message_policy, "server_cutoff")?,
        total_items,
        total_encoded_bytes,
        total_pages,
        manifest_digest,
        raw: raw.clone(),
    })
}

fn u64_field(object: &Map<String, Value>, field: &str) -> crate::ImResult<u64> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_page(format!("{field} must be a non-negative integer")))
}

fn digest_field(object: &Map<String, Value>, field: &'static str) -> crate::ImResult<String> {
    let digest = canonical_string_field(object, field)?;
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(invalid_page(format!(
            "{field} must use sha256 lowercase hex"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(invalid_page(format!(
            "{field} must use sha256 lowercase hex"
        )));
    }
    Ok(digest)
}

pub(crate) fn canonical_digest<T: serde::Serialize>(value: &T) -> crate::ImResult<String> {
    let encoded = serde_json_canonicalizer::to_vec(value)
        .map_err(|_| invalid_page("snapshot value is not canonicalizable"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

pub(crate) fn finalize_snapshot_package_v3(
    manifest: &SyncSnapshotManifestV3,
    account_id: String,
    device_id: String,
    server_time: String,
    sections: &BTreeMap<SnapshotSectionV3, Vec<Value>>,
    page_counts: &BTreeMap<SnapshotSectionV3, u64>,
) -> crate::ImResult<SyncSnapshotV3> {
    let empty = Vec::new();
    for section in SnapshotSectionV3::ORDERED {
        let items = sections.get(&section).unwrap_or(&empty);
        let expected = manifest
            .sections
            .get(&section)
            .ok_or_else(|| invalid_page("snapshot manifest section is missing"))?;
        if expected.item_count != items.len() as u64 || expected.digest != canonical_digest(items)?
        {
            return Err(invalid_page(
                "snapshot section count or digest does not match the manifest",
            ));
        }
    }
    let read_values = sections
        .get(&SnapshotSectionV3::ReadStates)
        .cloned()
        .unwrap_or_default();
    let group_values = sections
        .get(&SnapshotSectionV3::Groups)
        .cloned()
        .unwrap_or_default();
    let message_values = sections
        .get(&SnapshotSectionV3::RecentPlainMessages)
        .cloned()
        .unwrap_or_default();
    let notification_values = sections
        .get(&SnapshotSectionV3::UnexpiredSystemNotifications)
        .cloned()
        .unwrap_or_default();
    let logical_package = json!({
        "read_states": read_values,
        "groups": group_values,
        "recent_plain_messages": message_values,
        "unexpired_system_notifications": notification_values,
    });
    let total_encoded_bytes = serde_json_canonicalizer::to_vec(&logical_package)
        .map_err(|_| invalid_page("snapshot package is not canonicalizable"))?
        .len() as u64;
    if total_encoded_bytes != manifest.total_encoded_bytes {
        return Err(invalid_page(
            "snapshot package byte count does not match the manifest",
        ));
    }
    let item_bytes = |section: SnapshotSectionV3| -> crate::ImResult<u64> {
        sections
            .get(&section)
            .into_iter()
            .flatten()
            .try_fold(0_u64, |total, item| {
                let bytes = serde_json_canonicalizer::to_vec(item)
                    .map_err(|_| invalid_page("snapshot item is not canonicalizable"))?
                    .len() as u64;
                Ok(total.saturating_add(bytes))
            })
    };
    let required_bytes = item_bytes(SnapshotSectionV3::ReadStates)?
        .saturating_add(item_bytes(SnapshotSectionV3::Groups)?)
        .saturating_add(item_bytes(SnapshotSectionV3::UnexpiredSystemNotifications)?);
    if required_bytes != manifest.recovery_budget.required_state_encoded_bytes
        || item_bytes(SnapshotSectionV3::RecentPlainMessages)?
            != manifest.history_policy.returned_encoded_bytes
    {
        return Err(invalid_page(
            "snapshot item byte totals do not match the manifest",
        ));
    }
    let pages = |section| page_counts.get(&section).copied().unwrap_or_default();
    let actual_required_pages = pages(SnapshotSectionV3::ReadStates)
        .saturating_add(pages(SnapshotSectionV3::Groups))
        .saturating_add(pages(SnapshotSectionV3::UnexpiredSystemNotifications));
    let empty_terminal_page = manifest.total_items == 0
        && actual_required_pages == 1
        && manifest.recovery_budget.required_state_pages == 0;
    if pages(SnapshotSectionV3::RecentPlainMessages) != manifest.history_policy.returned_pages
        || (actual_required_pages != manifest.recovery_budget.required_state_pages
            && !empty_terminal_page)
        || page_counts.values().copied().sum::<u64>() != manifest.total_pages
    {
        return Err(invalid_page(
            "snapshot section page counts do not match the manifest",
        ));
    }

    let parsed_cutoff = chrono::DateTime::parse_from_rfc3339(&manifest.server_cutoff)
        .map_err(|_| invalid_page("snapshot server cutoff is invalid"))?;
    let mut event_ids = BTreeSet::new();
    let mut event_seqs = BTreeSet::new();
    let recent_plain_messages = sections
        .get(&SnapshotSectionV3::RecentPlainMessages)
        .into_iter()
        .flatten()
        .map(|item| {
            let item = object(item, "snapshot plain message")?;
            exact_fields(item, &["event", "message"], "snapshot plain message")?;
            let event = parse_event(
                item.get("event")
                    .ok_or_else(|| invalid_page("snapshot message event is required"))?,
            )?;
            if event.event_type != "message.created"
                || event.account_id != account_id
                || event.stream_epoch != manifest.snapshot_cursor.stream_epoch
                || event.recipient_device_id.is_some()
                || crate::internal::local_state::sync_v2::compare_decimal(
                    &event.event_seq,
                    &manifest.snapshot_cursor.scan_seq,
                )? == std::cmp::Ordering::Greater
                || !event_ids.insert(event.event_id.clone())
                || !event_seqs.insert(event.event_seq.clone())
            {
                return Err(invalid_page(
                    "snapshot message event is outside its unique account anchor",
                ));
            }
            let message = item
                .get("message")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("snapshot message must be an object"))?;
            let accepted_at = message
                .get("accepted_at")
                .or_else(|| message.get("created_at"))
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_page("snapshot message timestamp is required"))?;
            let accepted_at = chrono::DateTime::parse_from_rfc3339(accepted_at)
                .map_err(|_| invalid_page("snapshot message timestamp must be RFC3339"))?;
            if accepted_at < parsed_cutoff {
                return Err(invalid_page(
                    "snapshot message timestamp is before the server cutoff",
                ));
            }
            reject_e2ee_value(&message)?;
            validate_message_kind_matches_hydration(&event, &message)?;
            Ok(SnapshotPlainMessageV2 { event, message })
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    if let Some(oldest) = manifest.history_policy.oldest_included_event_seq.as_deref() {
        if recent_plain_messages
            .first()
            .map(|item| item.event.event_seq.as_str())
            != Some(oldest)
        {
            return Err(invalid_page(
                "snapshot oldest included event does not match history",
            ));
        }
    }

    let unexpired_system_notifications = sections
        .get(&SnapshotSectionV3::UnexpiredSystemNotifications)
        .into_iter()
        .flatten()
        .map(|item| {
            let item = object(item, "snapshot system notification")?;
            exact_fields(item, &["event", "message"], "snapshot system notification")?;
            let event = parse_event(
                item.get("event")
                    .ok_or_else(|| invalid_page("snapshot notification event is required"))?,
            )?;
            if event.event_type != "system.notification"
                || event.account_id != account_id
                || event.stream_epoch != manifest.snapshot_cursor.stream_epoch
                || event.recipient_device_id.as_deref() != Some(device_id.as_str())
                || crate::internal::local_state::sync_v2::compare_decimal(
                    &event.event_seq,
                    &manifest.snapshot_cursor.scan_seq,
                )? == std::cmp::Ordering::Greater
                || !event_ids.insert(event.event_id.clone())
                || !event_seqs.insert(event.event_seq.clone())
            {
                return Err(invalid_page(
                    "snapshot notification is outside its unique exact-device anchor",
                ));
            }
            let message = item
                .get("message")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_page("snapshot notification must be an object"))?;
            exact_fields(
                message,
                &["projection_kind", "meta", "auth", "body"],
                "snapshot notification message",
            )?;
            if !crate::internal::system_notification::wire::is_trusted_delivery_marker(
                item.get("message").expect("message field was validated"),
            ) {
                return Err(invalid_page(
                    "snapshot notification must use its trusted projection marker",
                ));
            }
            Ok(SnapshotSystemNotificationV2 {
                event,
                message: item
                    .get("message")
                    .expect("message field was validated")
                    .clone(),
            })
        })
        .collect::<crate::ImResult<Vec<_>>>()?;

    let read_states = sections
        .get(&SnapshotSectionV3::ReadStates)
        .cloned()
        .unwrap_or_default();
    for value in &read_states {
        let state = object(value, "snapshot read state")?;
        exact_fields(
            state,
            &[
                "thread_kind",
                "thread_key",
                "read_up_to_thread_seq",
                "read_up_to_message_id",
                "state_version",
                "updated_by_device_id",
                "updated_at",
            ],
            "snapshot read state",
        )?;
        canonical_timestamp_field(state, "updated_at")?;
        optional_canonical_string_field(state, "read_up_to_message_id")?;
        optional_canonical_string_field(state, "updated_by_device_id")?;
        reject_e2ee_value(value)?;
    }
    let groups = sections
        .get(&SnapshotSectionV3::Groups)
        .cloned()
        .unwrap_or_default();
    for value in &groups {
        let state = object(value, "snapshot group state")?;
        exact_fields(
            state,
            &[
                "group_did",
                "host_service_did",
                "creator_did",
                "group_state_version",
                "group_event_seq",
                "required_security_profile",
                "group_profile",
                "member_role",
                "membership_status",
                "member_count",
                "updated_at",
            ],
            "snapshot group state",
        )?;
        canonical_timestamp_field(state, "updated_at")?;
        reject_e2ee_value(value)?;
    }
    Ok(SyncSnapshotV3 {
        account_id,
        device_id,
        server_time,
        server_cutoff: manifest.server_cutoff.clone(),
        snapshot_cursor: manifest.snapshot_cursor.clone(),
        read_states,
        groups,
        recent_plain_messages,
        unexpired_system_notifications,
        older_history_excluded: manifest.history_policy.older_history_excluded,
    })
}

#[cfg(test)]
pub(crate) fn parse_snapshot(raw: &Value) -> crate::ImResult<SyncSnapshotV2> {
    let object = object(raw, "sync.snapshot response")?;
    let snapshot_schema = match object.get("snapshot_schema") {
        None => 1,
        Some(Value::Number(value)) if value.as_u64() == Some(2) => 2,
        Some(_) => return Err(invalid_page("snapshot_schema must be absent or equal 2")),
    };
    let schema_one_fields = [
        "mode",
        "account_id",
        "device_id",
        "server_time",
        "snapshot_cursor",
        "read_states",
        "groups",
        "recent_plain_messages",
        "message_policy",
        "excluded",
    ];
    let schema_two_fields = [
        "mode",
        "snapshot_schema",
        "account_id",
        "device_id",
        "server_time",
        "snapshot_cursor",
        "read_states",
        "groups",
        "recent_plain_messages",
        "message_policy",
        "excluded",
        "unexpired_system_notifications",
        "system_notification_policy",
    ];
    exact_fields(
        object,
        if snapshot_schema == 2 {
            &schema_two_fields
        } else {
            &schema_one_fields
        },
        "sync.snapshot response",
    )?;
    exact_mode(object, "compact_recovery")?;
    let account_id = canonical_string_field(object, "account_id")?;
    let device_id = canonical_string_field(object, "device_id")?;
    let server_time = canonical_timestamp_field(object, "server_time")?;
    let snapshot_cursor = parse_cursor(
        object
            .get("snapshot_cursor")
            .ok_or_else(|| invalid_page("snapshot_cursor is required"))?,
    )?;
    let excluded = self::object(
        object
            .get("excluded")
            .ok_or_else(|| invalid_page("excluded is required"))?,
        "snapshot excluded",
    )?;
    exact_fields(
        excluded,
        &["e2ee_messages", "plain_messages_before_cutoff"],
        "snapshot excluded",
    )?;
    if excluded.get("e2ee_messages").and_then(Value::as_bool) != Some(true)
        || excluded
            .get("plain_messages_before_cutoff")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(invalid_page(
            "snapshot must explicitly exclude E2EE and pre-cutoff messages",
        ));
    }
    let policy = self::object(
        object
            .get("message_policy")
            .ok_or_else(|| invalid_page("message_policy is required"))?,
        "snapshot message policy",
    )?;
    exact_fields(
        policy,
        &[
            "server_cutoff",
            "max_logical_messages",
            "returned_logical_messages",
        ],
        "snapshot message policy",
    )?;
    let server_cutoff = canonical_timestamp_field(policy, "server_cutoff")?;
    let parsed_server_cutoff = chrono::DateTime::parse_from_rfc3339(&server_cutoff)
        .map_err(|_| invalid_page("server_cutoff must be an RFC3339 timestamp"))?;
    let max_logical_messages = policy
        .get("max_logical_messages")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_page("max_logical_messages must be an integer"))?;
    if max_logical_messages != 500 {
        return Err(invalid_page(
            "max_logical_messages must equal the frozen value 500",
        ));
    }
    let returned_logical_messages = policy
        .get("returned_logical_messages")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_page("returned_logical_messages must be an integer"))?;
    if returned_logical_messages > 500 {
        return Err(invalid_page(
            "returned_logical_messages must not exceed 500",
        ));
    }
    let mut event_ids = BTreeSet::new();
    let mut event_seqs = BTreeSet::new();
    let recent_plain_messages = object
        .get("recent_plain_messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("recent_plain_messages must be an array"))?
        .iter()
        .map(|item| {
            let item = self::object(item, "snapshot plain message")?;
            if item.len() != 2 || !item.contains_key("event") || !item.contains_key("message") {
                return Err(invalid_page(
                    "snapshot plain message must contain exactly event and message",
                ));
            }
            let event = parse_event(
                item.get("event")
                    .ok_or_else(|| invalid_page("snapshot message event is required"))?,
            )?;
            if event.event_type != "message.created" {
                return Err(invalid_page(
                    "snapshot message event must be message.created",
                ));
            }
            if !event_ids.insert(event.event_id.clone())
                || !event_seqs.insert(event.event_seq.clone())
            {
                return Err(invalid_page(
                    "snapshot message event_id and event_seq must be unique",
                ));
            }
            if event.account_id != account_id
                || event.stream_epoch != snapshot_cursor.stream_epoch
                || event.recipient_device_id.is_some()
                || crate::internal::local_state::sync_v2::compare_decimal(
                    &event.event_seq,
                    &snapshot_cursor.scan_seq,
                )? == std::cmp::Ordering::Greater
            {
                return Err(invalid_page(
                    "snapshot message event is outside the account/epoch/anchor boundary",
                ));
            }
            let message = item
                .get("message")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("snapshot message must be an object"))?;
            let accepted_at = message
                .get("accepted_at")
                .or_else(|| message.get("created_at"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid_page("snapshot message requires accepted_at or created_at")
                })?;
            let accepted_at = chrono::DateTime::parse_from_rfc3339(accepted_at)
                .map_err(|_| invalid_page("snapshot message timestamp must be RFC3339"))?;
            if accepted_at < parsed_server_cutoff {
                return Err(invalid_page(
                    "snapshot message timestamp is before message_policy.server_cutoff",
                ));
            }
            reject_e2ee_value(&message)?;
            validate_message_kind_matches_hydration(&event, &message)?;
            Ok(SnapshotPlainMessageV2 { event, message })
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    if recent_plain_messages.len() > 500 {
        return Err(invalid_page(
            "snapshot contains more than 500 ordinary messages",
        ));
    }
    if returned_logical_messages != recent_plain_messages.len() as u64 {
        return Err(invalid_page(
            "returned_logical_messages does not match recent_plain_messages",
        ));
    }
    let unexpired_system_notifications = if snapshot_schema == 2 {
        let items = object
            .get("unexpired_system_notifications")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_page("unexpired_system_notifications must be an array"))?
            .iter()
            .map(|item| {
                let item = self::object(item, "snapshot system notification")?;
                exact_fields(
                    item,
                    &["event", "message"],
                    "snapshot system notification",
                )?;
                let event = parse_event(
                    item.get("event")
                        .ok_or_else(|| invalid_page("snapshot notification event is required"))?,
                )?;
                if event.event_type != "system.notification"
                    || event.account_id != account_id
                    || event.stream_epoch != snapshot_cursor.stream_epoch
                    || event.recipient_device_id.as_deref() != Some(device_id.as_str())
                    || crate::internal::local_state::sync_v2::compare_decimal(
                        &event.event_seq,
                        &snapshot_cursor.scan_seq,
                    )? == std::cmp::Ordering::Greater
                {
                    return Err(invalid_page(
                        "snapshot system notification is outside the account/epoch/device/anchor boundary",
                    ));
                }
                if !event_ids.insert(event.event_id.clone())
                    || !event_seqs.insert(event.event_seq.clone())
                {
                    return Err(invalid_page(
                        "snapshot event_id and event_seq must be unique across all projections",
                    ));
                }
                let message = item
                    .get("message")
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid_page("snapshot notification message must be an object"))?;
                exact_fields(
                    message,
                    &["projection_kind", "meta", "auth", "body"],
                    "snapshot notification message",
                )?;
                if !crate::internal::system_notification::wire::is_trusted_delivery_marker(
                    item.get("message").unwrap(),
                ) {
                    return Err(invalid_page(
                        "snapshot notification message must use the trusted projection marker",
                    ));
                }
                Ok(SnapshotSystemNotificationV2 {
                    event,
                    message: item.get("message").unwrap().clone(),
                })
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        if items.len() > 100 {
            return Err(invalid_page(
                "snapshot contains more than 100 unexpired system notifications",
            ));
        }
        for pair in items.windows(2) {
            let ordering = crate::internal::local_state::sync_v2::compare_decimal(
                &pair[0].event.event_seq,
                &pair[1].event.event_seq,
            )?
            .then_with(|| pair[0].event.event_id.cmp(&pair[1].event.event_id));
            if ordering != std::cmp::Ordering::Less {
                return Err(invalid_page(
                    "snapshot system notifications must be ordered by event_seq and event_id",
                ));
            }
        }
        let policy = self::object(
            object
                .get("system_notification_policy")
                .ok_or_else(|| invalid_page("system_notification_policy is required"))?,
            "snapshot system notification policy",
        )?;
        exact_fields(
            policy,
            &[
                "scope",
                "complete_through_scan_seq",
                "returned_events",
                "complete",
            ],
            "snapshot system notification policy",
        )?;
        if policy.get("scope").and_then(Value::as_str) != Some("exact_device_unexpired")
            || decimal_field(policy, "complete_through_scan_seq")? != snapshot_cursor.scan_seq
            || policy.get("returned_events").and_then(Value::as_u64) != Some(items.len() as u64)
            || policy.get("complete").and_then(Value::as_bool) != Some(true)
        {
            return Err(invalid_page(
                "system_notification_policy does not prove a complete exact-device snapshot",
            ));
        }
        items
    } else {
        Vec::new()
    };
    let read_states = object
        .get("read_states")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("read_states must be an array"))?
        .iter()
        .map(|value| {
            let state = self::object(value, "snapshot read state")?;
            exact_fields(
                state,
                &[
                    "thread_kind",
                    "thread_key",
                    "read_up_to_thread_seq",
                    "read_up_to_message_id",
                    "state_version",
                    "updated_by_device_id",
                    "updated_at",
                ],
                "snapshot read state",
            )?;
            canonical_timestamp_field(state, "updated_at")?;
            optional_canonical_string_field(state, "read_up_to_message_id")?;
            optional_canonical_string_field(state, "updated_by_device_id")?;
            Ok(value.clone())
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    let groups = object
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("groups must be an array"))?
        .iter()
        .map(|value| {
            let state = self::object(value, "snapshot group state")?;
            exact_fields(
                state,
                &[
                    "group_did",
                    "host_service_did",
                    "creator_did",
                    "group_state_version",
                    "group_event_seq",
                    "required_security_profile",
                    "group_profile",
                    "member_role",
                    "membership_status",
                    "member_count",
                    "updated_at",
                ],
                "snapshot group state",
            )?;
            canonical_timestamp_field(state, "updated_at")?;
            Ok(value.clone())
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    for value in read_states.iter().chain(groups.iter()) {
        if !value.is_object() {
            return Err(invalid_page("snapshot state items must be objects"));
        }
        reject_e2ee_value(value)?;
    }
    Ok(SyncSnapshotV2 {
        snapshot_schema,
        account_id,
        device_id,
        server_time,
        server_cutoff,
        snapshot_cursor,
        read_states,
        groups,
        recent_plain_messages,
        unexpired_system_notifications,
    })
}

fn exact_fields(
    object: &Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> crate::ImResult<()> {
    if object.len() != expected.len() || expected.iter().any(|field| !object.contains_key(*field)) {
        return Err(invalid_page(format!(
            "{label} must contain exactly the frozen fields"
        )));
    }
    Ok(())
}

fn parse_recovery_descriptor_v3(recovery: &Map<String, Value>) -> crate::ImResult<SyncRecoveryV2> {
    exact_fields(
        recovery,
        &[
            "recovery_id",
            "token",
            "snapshot_schema",
            "snapshot_delivery",
            "stream_epoch",
            "snapshot_scan_seq",
            "message_cutoff",
            "expires_at",
        ],
        "Schema 3 recovery descriptor",
    )?;
    if recovery.get("snapshot_schema").and_then(Value::as_u64) != Some(3)
        || recovery.get("snapshot_delivery").and_then(Value::as_str) != Some("paged_v1")
    {
        return Err(invalid_page(
            "compact recovery must use Schema 3 paged_v1 without fallback",
        ));
    }
    canonical_timestamp_field(recovery, "message_cutoff")?;
    Ok(SyncRecoveryV2 {
        recovery_id: canonical_string_field(recovery, "recovery_id")?,
        token: canonical_string_field(recovery, "token")?,
        stream_epoch: positive_decimal_field(recovery, "stream_epoch")?,
        snapshot_scan_seq: decimal_field(recovery, "snapshot_scan_seq")?,
        expires_at: canonical_timestamp_field(recovery, "expires_at")?,
        snapshot_schema: 3,
        snapshot_delivery: "paged_v1".to_owned(),
    })
}

#[cfg(test)]
fn optional_snapshot_schema(object: &Map<String, Value>) -> crate::ImResult<u32> {
    match object.get("snapshot_schema") {
        None => Ok(1),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| matches!(value, 1 | 2))
            .ok_or_else(|| invalid_page("snapshot_schema must be 1 or 2")),
    }
}

pub(crate) fn parse_message_batch(
    raw: &Value,
    requested_event_ids: &[String],
) -> crate::ImResult<MessageBatchV2> {
    validate_event_ids(requested_event_ids)?;
    let request_order = requested_event_ids
        .iter()
        .enumerate()
        .map(|(index, event_id)| (event_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let response = object(raw, "message.get_batch response")?;
    let items = response
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("items must be an array"))?
        .iter()
        .map(|item| {
            let item = object(item, "message.get_batch item")?;
            let event_id = canonical_string_field(item, "event_id")?;
            let message = item
                .get("message")
                .filter(|message| message.is_object())
                .cloned()
                .ok_or_else(|| invalid_page("hydrated message must be an object"))?;
            reject_e2ee_value(&message)?;
            Ok(HydratedMessageV2 { event_id, message })
        })
        .collect::<crate::ImResult<Vec<_>>>()?;
    let unavailable = response
        .get("unavailable")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_page("unavailable must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid_page("unavailable entries must be strings"))
                .and_then(|value| required_string("unavailable.event_id", value))
        })
        .collect::<crate::ImResult<Vec<_>>>()?;

    let mut seen = BTreeSet::new();
    validate_response_order(
        items.iter().map(|item| item.event_id.as_str()),
        &request_order,
        "items",
        &mut seen,
    )?;
    validate_response_order(
        unavailable.iter().map(String::as_str),
        &request_order,
        "unavailable",
        &mut seen,
    )?;
    if seen.len() != requested_event_ids.len() {
        return Err(invalid_page(
            "message.get_batch response does not cover every requested event_id",
        ));
    }
    Ok(MessageBatchV2 { items, unavailable })
}

fn parse_event(value: &Value) -> crate::ImResult<SyncEventV2> {
    let object = object(value, "sync event")?;
    let payload = object
        .get("payload")
        .filter(|payload| payload.is_object())
        .cloned()
        .ok_or_else(|| invalid_page("event payload must be an object"))?;
    let schema_version = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid_page("schema_version must be a positive integer"))?;
    if schema_version == 0 {
        return Err(invalid_page("schema_version must be a positive integer"));
    }
    let state_version = optional_decimal_field(object, "state_version")?;
    let source = match object.get("source") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(value.clone()),
        Some(_) => return Err(invalid_page("source must be an object or null")),
    };
    Ok(SyncEventV2 {
        event_id: canonical_string_field(object, "event_id")?,
        stream_epoch: positive_decimal_field(object, "stream_epoch")?,
        event_seq: positive_decimal_field(object, "event_seq")?,
        event_type: canonical_string_field(object, "event_type")?,
        schema_version,
        ignore_safe: object
            .get("ignore_safe")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid_page("ignore_safe must be a boolean"))?,
        account_id: canonical_string_field(object, "account_id")?,
        recipient_device_id: optional_canonical_string_field(object, "recipient_device_id")?,
        origin_did: optional_canonical_string_field(object, "origin_did")?,
        origin_device_id: optional_canonical_string_field(object, "origin_device_id")?,
        aggregate_kind: canonical_string_field(object, "aggregate_kind")?,
        aggregate_id: canonical_string_field(object, "aggregate_id")?,
        state_version,
        thread_key: optional_canonical_string_field(object, "thread_key")?,
        occurred_at: canonical_string_field(object, "occurred_at")?,
        payload,
        source,
    })
}

pub(crate) fn parse_inline_event(value: &Value) -> crate::ImResult<SyncEventV2> {
    let object = object(value, "inline sync event")?;
    exact_fields(
        object,
        &[
            "event_id",
            "stream_epoch",
            "event_seq",
            "event_type",
            "schema_version",
            "ignore_safe",
            "account_id",
            "recipient_device_id",
            "origin_did",
            "origin_device_id",
            "aggregate_kind",
            "aggregate_id",
            "state_version",
            "thread_key",
            "occurred_at",
            "payload",
            "source",
        ],
        "inline sync event",
    )?;
    parse_event(value)
}

pub(crate) fn parse_inline_message(event_id: &str, message: &Value) -> crate::ImResult<Value> {
    let batch = parse_message_batch(
        &json!({
            "items": [{"event_id": event_id, "message": message}],
            "unavailable": [],
        }),
        &[event_id.to_owned()],
    )?;
    batch
        .items
        .into_iter()
        .next()
        .map(|item| item.message)
        .ok_or_else(|| invalid_page("inline message projection is missing"))
}

fn parse_cursor(value: &Value) -> crate::ImResult<SyncCursorV2> {
    let object = object(value, "cursor")?;
    let cursor = SyncCursorV2 {
        stream_epoch: positive_decimal_field(object, "stream_epoch")?,
        scan_seq: decimal_field(object, "scan_seq")?,
    };
    validate_cursor(&cursor)?;
    Ok(cursor)
}

fn validate_cursor(cursor: &SyncCursorV2) -> crate::ImResult<()> {
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "stream_epoch",
        &cursor.stream_epoch,
    )?;
    crate::internal::local_state::sync_v2::validate_decimal("scan_seq", &cursor.scan_seq)
}

fn validate_event_ids(event_ids: &[String]) -> crate::ImResult<()> {
    if event_ids.is_empty() || event_ids.len() > MESSAGE_GET_BATCH_MAX_EVENT_IDS {
        return Err(crate::ImError::invalid_input(
            Some("event_ids".to_owned()),
            format!(
                "event_ids must contain between 1 and {MESSAGE_GET_BATCH_MAX_EVENT_IDS} entries"
            ),
        ));
    }
    let mut unique = BTreeSet::new();
    for event_id in event_ids {
        required_string("event_ids", event_id)?;
        if !unique.insert(event_id.as_str()) {
            return Err(crate::ImError::invalid_input(
                Some("event_ids".to_owned()),
                "event_ids must not contain duplicates",
            ));
        }
    }
    Ok(())
}

fn validate_response_order<'a>(
    event_ids: impl Iterator<Item = &'a str>,
    request_order: &BTreeMap<&str, usize>,
    field: &str,
    seen: &mut BTreeSet<String>,
) -> crate::ImResult<()> {
    let mut previous = None;
    for event_id in event_ids {
        let Some(index) = request_order.get(event_id).copied() else {
            return Err(invalid_page(format!(
                "{field} contains an event_id that was not requested"
            )));
        };
        if previous.is_some_and(|previous| index <= previous) {
            return Err(invalid_page(format!(
                "{field} does not preserve request order"
            )));
        }
        if !seen.insert(event_id.to_owned()) {
            return Err(invalid_page(
                "message.get_batch repeats an event_id across response fields",
            ));
        }
        previous = Some(index);
    }
    Ok(())
}

fn reject_e2ee_value(value: &Value) -> crate::ImResult<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_page("ordinary sync v2 value must be an object"))?;
    reject_e2ee_discriminators(object)?;
    if let Some(metadata) = object.get("metadata").and_then(Value::as_object) {
        reject_e2ee_discriminators(metadata)?;
    }
    Ok(())
}

fn reject_e2ee_discriminators(object: &Map<String, Value>) -> crate::ImResult<()> {
    let forbidden = [
        "anp.direct.e2ee",
        "anp.group.e2ee",
        "group.e2ee.",
        "e2ee",
        "mls",
        "secure_direct",
    ];
    let has_forbidden_discriminator = ["profile", "security_profile", "thread_kind"]
        .into_iter()
        .filter_map(|field| object.get(field).and_then(Value::as_str))
        .map(str::to_ascii_lowercase)
        .any(|value| forbidden.iter().any(|token| value.contains(token)));
    if has_forbidden_discriminator
        || object
            .get("secure")
            .and_then(Value::as_bool)
            .is_some_and(|secure| secure)
    {
        return Err(invalid_page(
            "ordinary sync v2 response contains an E2EE/MLS value",
        ));
    }
    Ok(())
}

fn validate_reason(value: &str) -> crate::ImResult<String> {
    let value = required_string("reason", value)?;
    if matches!(
        value.as_str(),
        "session_start"
            | "app_resume"
            | "websocket_hint"
            | "websocket_reconnect"
            | "foreground_reconcile"
            | "manual_refresh"
            | "after_mutation"
    ) {
        Ok(value)
    } else {
        Err(crate::ImError::invalid_input(
            Some("reason".to_owned()),
            "unsupported message sync reason",
        ))
    }
}

fn validate_limit(limit: u32) -> crate::ImResult<u32> {
    if (1..=500).contains(&limit) {
        Ok(limit)
    } else {
        Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "message sync limit must be between 1 and 500",
        ))
    }
}

fn exact_mode(object: &Map<String, Value>, expected: &str) -> crate::ImResult<()> {
    let mode = canonical_string_field(object, "mode")?;
    if mode == expected {
        Ok(())
    } else {
        Err(invalid_page(format!(
            "response mode must be {expected:?}, got {mode:?}"
        )))
    }
}

fn warnings(value: Option<&Value>) -> crate::ImResult<Vec<String>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Err(invalid_page("warnings must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid_page("warning entries must be strings"))
                .and_then(|value| required_string("warning", value))
        })
        .collect()
}

fn object<'a>(value: &'a Value, label: &str) -> crate::ImResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid_page(format!("{label} must be an object")))
}

fn canonical_string_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_page(format!("{field} must be a string")))
        .and_then(|value| required_string(field, value))
}

fn canonical_timestamp_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    let value = canonical_string_field(object, field)?;
    time::OffsetDateTime::parse(&value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid_page(format!("{field} must be an RFC 3339 timestamp")))?;
    Ok(value)
}

fn validate_message_kind_matches_hydration(
    event: &SyncEventV2,
    message: &Value,
) -> crate::ImResult<()> {
    let event_kind = event
        .payload
        .get("message_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_page("message.created payload is missing message_kind"))?;
    let expected_thread_kind = match event_kind {
        "direct_plain" => "direct",
        "group_plain" => "group",
        _ => {
            return Err(invalid_page(
                "snapshot message event is not an ordinary Direct/Group message",
            ))
        }
    };
    if message.get("thread_kind").and_then(Value::as_str) != Some(expected_thread_kind) {
        return Err(invalid_page(
            "snapshot message thread_kind conflicts with its event message_kind",
        ));
    }
    Ok(())
}

fn optional_canonical_string_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => required_string(field, value).map(Some),
        Some(_) => Err(invalid_page(format!("{field} must be a string or null"))),
    }
}

fn decimal_field(object: &Map<String, Value>, field: &'static str) -> crate::ImResult<String> {
    let value = canonical_string_field(object, field)?;
    crate::internal::local_state::sync_v2::validate_decimal(field, &value)
        .map_err(|_| invalid_page(format!("{field} must be a canonical decimal string")))?;
    Ok(value)
}

fn positive_decimal_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    let value = canonical_string_field(object, field)?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(field, &value).map_err(
        |_| {
            invalid_page(format!(
                "{field} must be a canonical positive decimal string"
            ))
        },
    )?;
    Ok(value)
}

fn optional_decimal_field(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            crate::internal::local_state::sync_v2::validate_decimal(field, value).map_err(
                |_| {
                    invalid_page(format!(
                        "{field} must be a canonical decimal string or null"
                    ))
                },
            )?;
            Ok(Some(value.clone()))
        }
        Some(_) => Err(invalid_page(format!(
            "{field} must be a decimal string or null"
        ))),
    }
}

fn required_string(field: &'static str, value: &str) -> crate::ImResult<String> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a non-empty canonical string"),
        ));
    }
    Ok(value.to_owned())
}

fn invalid_page(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("SYNC_INVALID_PAGE".to_owned()),
        message: message.into(),
        data: None,
    }
}

fn sync_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_id: &str, event_seq: &str) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": "1",
            "event_seq": event_seq,
            "event_type": "message.created",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": "account-1",
            "recipient_device_id": null,
            "origin_did": "did:example:alice",
            "origin_device_id": "device-1",
            "aggregate_kind": "direct_message",
            "aggregate_id": "message-1",
            "state_version": null,
            "thread_key": "conversation-1",
            "occurred_at": "2026-07-28T10:00:00Z",
            "payload": {},
            "source": {}
        })
    }

    fn empty_schema3_snapshot_response() -> Value {
        let empty_digest = canonical_digest(&Vec::<Value>::new()).unwrap();
        let logical_package = json!({
            "read_states": [],
            "groups": [],
            "recent_plain_messages": [],
            "unexpired_system_notifications": [],
        });
        let total_encoded_bytes = serde_json_canonicalizer::to_vec(&logical_package)
            .unwrap()
            .len();
        let mut manifest = json!({
            "manifest_schema": 1,
            "frozen_at": "2026-08-31T10:00:00Z",
            "snapshot_cursor": {"stream_epoch": "3", "scan_seq": "20"},
            "sections": {
                "read_states": {"item_count": 0, "digest": empty_digest},
                "groups": {"item_count": 0, "digest": empty_digest},
                "recent_plain_messages": {"item_count": 0, "digest": empty_digest},
                "unexpired_system_notifications": {"item_count": 0, "digest": empty_digest},
            },
            "recovery_budget": {
                "max_items": SNAPSHOT_PACKAGE_MAX_ITEMS,
                "max_encoded_bytes": SNAPSHOT_PACKAGE_MAX_ENCODED_BYTES,
                "max_pages": SNAPSHOT_PACKAGE_MAX_PAGES,
                "required_state_items": 0,
                "required_state_encoded_bytes": 0,
                "required_state_pages": 0,
            },
            "history_policy": {
                "selection": "newest_complete_suffix",
                "returned_items": 0,
                "returned_encoded_bytes": 0,
                "returned_pages": 0,
                "oldest_included_event_seq": null,
                "excluded_older_messages": 0,
                "older_history_excluded": false,
                "truncation_reason": null,
                "complete_within_policy": true,
            },
            "message_policy": {
                "server_cutoff": "2026-08-29T10:00:00Z",
                "selection": "ordinary_plain_only",
            },
            "system_notification_policy": {
                "scope": "exact_device_unexpired",
                "complete_through_scan_seq": "20",
                "complete": true,
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true,
            },
            "total_items": 0,
            "total_encoded_bytes": total_encoded_bytes,
            "total_pages": 1,
        });
        let manifest_digest = canonical_digest(&manifest).unwrap();
        manifest["manifest_digest"] = json!(manifest_digest);
        json!({
            "mode": "compact_recovery",
            "recovery_id": "recovery-fixture",
            "account_id": "account-1",
            "device_id": "device-1",
            "device_auth_generation": "3",
            "client_instance_id": "installation-1",
            "server_time": "2026-08-31T10:00:01Z",
            "snapshot_schema": 3,
            "snapshot_delivery": "paged_v1",
            "snapshot_cursor": {"stream_epoch": "3", "scan_seq": "20"},
            "manifest": manifest,
            "page": {
                "section": "read_states",
                "items": [],
                "returned_items": 0,
                "returned_encoded_bytes": 0,
                "page_digest": empty_digest,
                "has_more": false,
                "next_page_ref": null,
            }
        })
    }

    #[test]
    fn schema3_snapshot_decoder_accepts_empty_terminal_package_and_rejects_tamper() {
        let response = empty_schema3_snapshot_response();
        let page = parse_snapshot_page_v3(&response, true).unwrap();
        assert_eq!(page.page.section, SnapshotSectionV3::ReadStates);
        let debug = format!("{page:?}");
        for private in [
            "recovery-fixture",
            "account-1",
            "device-1",
            "installation-1",
        ] {
            assert!(!debug.contains(private));
        }
        let manifest = page.manifest.as_ref().unwrap();
        let snapshot = finalize_snapshot_package_v3(
            manifest,
            page.account_id.clone(),
            page.device_id.clone(),
            page.server_time.clone(),
            &BTreeMap::from([(SnapshotSectionV3::ReadStates, Vec::new())]),
            &BTreeMap::from([(SnapshotSectionV3::ReadStates, 1)]),
        )
        .unwrap();
        assert!(!snapshot.older_history_excluded);

        let mut tampered_page = response.clone();
        tampered_page["page"]["page_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        assert!(parse_snapshot_page_v3(&tampered_page, true).is_err());
        let mut extra = response.clone();
        extra["page"]["offset"] = json!(0);
        assert!(parse_snapshot_page_v3(&extra, true).is_err());
        let mut tampered_manifest = response;
        tampered_manifest["manifest"]["total_items"] = json!(1);
        assert!(parse_snapshot_page_v3(&tampered_manifest, true).is_err());
    }

    #[test]
    fn schema3_recovery_and_bootstrap_requests_have_no_legacy_fallback_shape() {
        let identity = WireIdentity {
            did: "did:example:alice".to_owned(),
        };
        require_explicit_sync_negotiation_capability(&json!({
            "supported_profiles": [
                MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1,
                SNAPSHOT_PAGING_V1
            ]
        }))
        .unwrap();
        assert!(require_explicit_sync_negotiation_capability(&json!({
            "supported_profiles": [MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1]
        }))
        .is_err());
        let bootstrap = build_bootstrap_params(&identity, "installation-1").unwrap();
        assert_eq!(
            bootstrap["body"]["capabilities"]["requested_snapshot_capabilities"],
            json!({"schema_max": 3, "deliveries": ["paged_v1"]})
        );
        let recovery = parse_delta_response(&json!({
            "mode": "compact_recovery_required",
            "server_time": "2026-08-31T10:00:00Z",
            "events": [],
            "next_cursor": null,
            "has_more": false,
            "recovery": {
                "recovery_id": "recovery-1",
                "token": "opaque-token",
                "snapshot_schema": 3,
                "snapshot_delivery": "paged_v1",
                "stream_epoch": "3",
                "snapshot_scan_seq": "20",
                "message_cutoff": "2026-08-29T10:00:00Z",
                "expires_at": "2026-08-31T10:10:00Z"
            },
            "warnings": []
        }))
        .unwrap();
        assert!(matches!(
            recovery,
            SyncDeltaResponseV2::RecoveryRequired(SyncRecoveryV2 {
                snapshot_schema: 3,
                ref snapshot_delivery,
                ..
            }) if snapshot_delivery == "paged_v1"
        ));
    }

    #[test]
    fn frozen_delta_fixture_decodes_decimal_strings_and_sparse_scan() {
        let raw = json!({
            "mode": "delta",
            "server_time": "2026-07-28T10:00:00Z",
            "events": [event("event-1", "100"), event("event-2", "102")],
            "next_cursor": {"stream_epoch": "1", "scan_seq": "105"},
            "has_more": false,
            "recovery": null,
            "warnings": []
        });
        let page = parse_delta(&raw).unwrap();
        assert_eq!(page.events[1].event_seq, "102");
        assert_eq!(page.next_cursor.scan_seq, "105");
    }

    #[test]
    fn unknown_required_fields_remain_available_to_the_reducer() {
        let mut raw = event("event-1", "1");
        raw["event_type"] = json!("future.required");
        raw["ignore_safe"] = json!(false);
        raw["schema_version"] = json!(2);
        let page = parse_delta(&json!({
            "mode": "delta",
            "server_time": "2026-07-28T10:00:00Z",
            "events": [raw],
            "next_cursor": {"stream_epoch": "1", "scan_seq": "1"},
            "has_more": false,
            "recovery": null,
            "warnings": []
        }))
        .unwrap();
        assert!(!page.events[0].ignore_safe);
        assert_eq!(page.events[0].schema_version, 2);
    }

    #[test]
    fn message_batch_rejects_duplicates_unavailable_and_order_drift() {
        let request = vec!["event-1".to_owned(), "event-2".to_owned()];
        assert!(parse_message_batch(
            &json!({
                "items": [
                    {"event_id": "event-2", "message": {}},
                    {"event_id": "event-1", "message": {}}
                ],
                "unavailable": []
            }),
            &request
        )
        .is_err());
        assert!(build_message_get_batch_params(
            &WireIdentity {
                did: "did:example:alice".to_owned()
            },
            &["event-1".to_owned(), "event-1".to_owned()]
        )
        .is_err());
    }

    #[test]
    fn ordinary_content_cannot_trigger_e2ee_discriminator_rejection() {
        let request = vec!["event-1".to_owned()];
        assert!(parse_message_batch(
            &json!({
                "items": [{
                    "event_id": "event-1",
                    "message": {
                        "thread_kind": "direct",
                        "content": "anp.direct.e2ee and mls are ordinary user text"
                    }
                }],
                "unavailable": []
            }),
            &request
        )
        .is_ok());
        assert!(parse_message_batch(
            &json!({
                "items": [{
                    "event_id": "event-1",
                    "message": {
                        "thread_kind": "direct",
                        "security_profile": "anp.direct.e2ee.v1"
                    }
                }],
                "unavailable": []
            }),
            &request
        )
        .is_err());
    }

    #[test]
    fn bootstrap_accepts_plain_current_read_state_baseline_in_stage_three() {
        let parsed = parse_bootstrap(&json!({
            "mode": "tail_only",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T10:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [{
                "thread_kind": "direct",
                "thread_key": "direct-1",
                "read_up_to_thread_seq": "9",
                "state_version": "2"
            }],
            "group_state_baseline": [],
            "warnings": [],
            "snapshot_capability": {"schema": 3, "delivery": "paged_v1"}
        }))
        .unwrap();
        assert_eq!(parsed.read_state_baseline.len(), 1);
    }

    #[test]
    fn thread_after_v2_has_exact_private_body_and_strict_response_shape() {
        let params = build_thread_after_params(
            &WireIdentity {
                did: "did:example:alice".to_owned(),
            },
            "conversation-ref-1",
            "42",
            100,
        )
        .unwrap();
        assert_eq!(
            params["body"],
            json!({
                "thread_key": "conversation-ref-1",
                "after_server_seq": "42",
                "limit": 100
            })
        );
        assert!(validate_thread_after_response(
            &json!({
                "messages": [{"thread_kind": "direct"}],
                "next_after_server_seq": "43",
                "has_more": false,
                "warnings": []
            }),
            "direct"
        )
        .is_ok());
        assert!(validate_thread_after_response(
            &json!({
                "messages": [{"thread_kind": "group"}],
                "next_after_server_seq": "43",
                "has_more": false,
                "warnings": []
            }),
            "direct"
        )
        .is_err());
    }

    #[test]
    fn recovery_and_snapshot_keep_policy_and_token_inside_private_wire_types() {
        let recovery = match parse_delta_response(&json!({
            "mode": "compact_recovery_required",
            "server_time": "2026-08-31T10:00:00Z",
            "events": [],
            "next_cursor": null,
            "has_more": false,
            "recovery": {
                "recovery_id": "recovery-123",
                "token": "opaque-secret",
                "snapshot_schema": 3,
                "snapshot_delivery": "paged_v1",
                "stream_epoch": "2",
                "snapshot_scan_seq": "15020",
                "message_cutoff": "2026-07-26T12:00:03Z",
                "expires_at": "2026-07-28T12:10:03Z"
            },
            "warnings": []
        }))
        .unwrap()
        {
            SyncDeltaResponseV2::RecoveryRequired(recovery) => recovery,
            SyncDeltaResponseV2::Delta(_) => panic!("expected recovery"),
        };
        assert_eq!(recovery.snapshot_schema, 3);
        assert_eq!(recovery.snapshot_delivery, "paged_v1");
        let recovery_debug = format!("{recovery:?}");
        assert!(!recovery_debug.contains("recovery-123"));
        assert!(!recovery_debug.contains("opaque-secret"));
        let params = build_snapshot_params(
            &WireIdentity {
                did: "did:example:alice".to_owned(),
            },
            &recovery,
        )
        .unwrap();
        assert_eq!(params.pointer("/body/token"), Some(&json!("opaque-secret")));

        let page = parse_snapshot_page_v3(&empty_schema3_snapshot_response(), true).unwrap();
        let manifest = page.manifest.as_ref().unwrap();
        let snapshot = finalize_snapshot_package_v3(
            manifest,
            page.account_id.clone(),
            page.device_id.clone(),
            page.server_time.clone(),
            &BTreeMap::from([(SnapshotSectionV3::ReadStates, Vec::new())]),
            &BTreeMap::from([(SnapshotSectionV3::ReadStates, 1)]),
        )
        .unwrap();
        assert_eq!(snapshot.snapshot_cursor.scan_seq, "20");
        assert_eq!(snapshot.server_cutoff, "2026-08-29T10:00:00Z");
        assert!(snapshot.recent_plain_messages.is_empty());
        assert!(!snapshot.older_history_excluded);
    }

    #[test]
    fn schema_two_snapshot_requires_complete_exact_device_notification_proof() {
        let mut notification_event = event("system.notification:evt-1:device-1", "10");
        notification_event["stream_epoch"] = json!("2");
        notification_event["account_id"] = json!("account-1");
        notification_event["event_type"] = json!("system.notification");
        notification_event["recipient_device_id"] = json!("device-1");
        notification_event["origin_device_id"] = Value::Null;
        notification_event["aggregate_kind"] = json!("system_notification");
        notification_event["aggregate_id"] = json!("evt-1");
        notification_event["thread_key"] = Value::Null;
        notification_event["payload"] = json!({
            "projection_kind": "system_notification",
            "event_id": "evt-1",
            "message_id": "evt-1"
        });
        let base = json!({
            "mode": "compact_recovery",
            "snapshot_schema": 2,
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {"stream_epoch": "2", "scan_seq": "10"},
            "read_states": [],
            "groups": [],
            "recent_plain_messages": [],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 0
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            },
            "unexpired_system_notifications": [{
                "event": notification_event,
                "message": {
                    "projection_kind": "system_notification",
                    "meta": {},
                    "auth": {},
                    "body": {}
                }
            }],
            "system_notification_policy": {
                "scope": "exact_device_unexpired",
                "complete_through_scan_seq": "10",
                "returned_events": 1,
                "complete": true
            }
        });
        let parsed = parse_snapshot(&base).unwrap();
        assert_eq!(parsed.snapshot_schema, 2);
        assert_eq!(parsed.unexpired_system_notifications.len(), 1);

        for mutation in ["complete", "anchor", "count", "wrapper", "target", "order"] {
            let mut invalid = base.clone();
            match mutation {
                "complete" => invalid["system_notification_policy"]["complete"] = json!(false),
                "anchor" => {
                    invalid["system_notification_policy"]["complete_through_scan_seq"] = json!("9")
                }
                "count" => invalid["system_notification_policy"]["returned_events"] = json!(0),
                "wrapper" => {
                    invalid["unexpired_system_notifications"][0]["message"]["id"] =
                        json!("forbidden")
                }
                "target" => {
                    invalid["unexpired_system_notifications"][0]["event"]["recipient_device_id"] =
                        json!("device-2")
                }
                "order" => {
                    let mut earlier = invalid["unexpired_system_notifications"][0].clone();
                    earlier["event"]["event_id"] = json!("system.notification:evt-2:device-1");
                    earlier["event"]["event_seq"] = json!("9");
                    earlier["event"]["aggregate_id"] = json!("evt-2");
                    earlier["event"]["payload"]["event_id"] = json!("evt-2");
                    earlier["event"]["payload"]["message_id"] = json!("evt-2");
                    invalid["unexpired_system_notifications"]
                        .as_array_mut()
                        .unwrap()
                        .push(earlier);
                    invalid["system_notification_policy"]["returned_events"] = json!(2);
                }
                _ => unreachable!(),
            }
            assert!(parse_snapshot(&invalid).is_err(), "mutation {mutation}");
        }
        let mut too_many = base;
        too_many["snapshot_cursor"]["scan_seq"] = json!("101");
        too_many["system_notification_policy"]["complete_through_scan_seq"] = json!("101");
        too_many["system_notification_policy"]["returned_events"] = json!(101);
        let template = too_many["unexpired_system_notifications"][0].clone();
        let items = too_many["unexpired_system_notifications"]
            .as_array_mut()
            .unwrap();
        items.clear();
        for seq in 1..=101 {
            let mut item = template.clone();
            item["event"]["event_id"] = json!(format!("system.notification:evt-{seq}:device-1"));
            item["event"]["event_seq"] = json!(seq.to_string());
            items.push(item);
        }
        assert!(parse_snapshot(&too_many).is_err());
    }

    #[test]
    fn snapshot_rejects_non_frozen_limit_and_inconsistent_returned_count() {
        let base = json!({
            "mode": "compact_recovery",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {"stream_epoch": "2", "scan_seq": "10"},
            "read_states": [],
            "groups": [],
            "recent_plain_messages": [],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 0
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            }
        });
        let mut wrong_limit = base.clone();
        wrong_limit["message_policy"]["max_logical_messages"] = json!(499);
        assert!(parse_snapshot(&wrong_limit).is_err());

        let mut wrong_count = base.clone();
        wrong_count["message_policy"]["returned_logical_messages"] = json!(1);
        assert!(parse_snapshot(&wrong_count).is_err());

        let mut invalid_cutoff = base;
        invalid_cutoff["message_policy"]["server_cutoff"] = json!("not-a-timestamp");
        assert!(parse_snapshot(&invalid_cutoff).is_err());
    }

    #[test]
    fn snapshot_rejects_device_targeted_or_kind_mismatched_message_envelopes() {
        let mut snapshot_event = event("event-snapshot-1", "10");
        snapshot_event["stream_epoch"] = json!("2");
        snapshot_event["account_id"] = json!("account-1");
        snapshot_event["payload"]["message_kind"] = json!("direct_plain");
        let base = json!({
            "mode": "compact_recovery",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {"stream_epoch": "2", "scan_seq": "10"},
            "read_states": [],
            "groups": [],
            "recent_plain_messages": [{
                "event": snapshot_event,
                "message": {
                    "thread_kind": "direct",
                    "created_at": "2026-07-28T12:00:03Z"
                }
            }],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 1
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            }
        });
        assert!(parse_snapshot(&base).is_ok());

        let mut targeted = base.clone();
        targeted["recent_plain_messages"][0]["event"]["recipient_device_id"] = json!("device-1");
        assert!(parse_snapshot(&targeted).is_err());

        let mut kind_mismatch = base;
        kind_mismatch["recent_plain_messages"][0]["message"]["thread_kind"] = json!("group");
        assert!(parse_snapshot(&kind_mismatch).is_err());
    }

    #[test]
    fn snapshot_rejects_pre_cutoff_duplicates_and_unknown_fields() {
        let mut snapshot_event = event("event-snapshot-1", "10");
        snapshot_event["stream_epoch"] = json!("2");
        snapshot_event["account_id"] = json!("account-1");
        snapshot_event["payload"]["message_kind"] = json!("direct_plain");
        let base = json!({
            "mode": "compact_recovery",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {"stream_epoch": "2", "scan_seq": "10"},
            "read_states": [],
            "groups": [],
            "recent_plain_messages": [{
                "event": snapshot_event,
                "message": {
                    "thread_kind": "direct",
                    "created_at": "2026-07-26T12:00:03Z"
                }
            }],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 1
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            }
        });
        assert!(parse_snapshot(&base).is_ok(), "cutoff is inclusive");

        let mut before_cutoff = base.clone();
        before_cutoff["recent_plain_messages"][0]["message"]["created_at"] =
            json!("2026-07-26T12:00:02Z");
        assert!(parse_snapshot(&before_cutoff).is_err());

        let mut duplicate = base.clone();
        let duplicate_item = duplicate["recent_plain_messages"][0].clone();
        duplicate["recent_plain_messages"]
            .as_array_mut()
            .unwrap()
            .push(duplicate_item);
        duplicate["message_policy"]["returned_logical_messages"] = json!(2);
        assert!(parse_snapshot(&duplicate).is_err());

        let mut unknown = base;
        unknown["message_policy"]["client_override"] = json!(true);
        assert!(parse_snapshot(&unknown).is_err());
    }

    #[test]
    fn snapshot_read_state_is_closed_and_requires_timestamp() {
        let base = json!({
            "mode": "compact_recovery",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {"stream_epoch": "2", "scan_seq": "10"},
            "read_states": [{
                "thread_kind": "direct",
                "thread_key": "dconv-read-1",
                "read_up_to_thread_seq": "9",
                "read_up_to_message_id": null,
                "state_version": "2",
                "updated_by_device_id": "device-1",
                "updated_at": "2026-07-28T12:00:00Z"
            }],
            "groups": [],
            "recent_plain_messages": [],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 0
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            }
        });
        assert!(parse_snapshot(&base).is_ok());

        let mut malformed = base.clone();
        malformed["read_states"][0]["updated_at"] = json!("not-a-timestamp");
        assert!(parse_snapshot(&malformed).is_err());

        let mut invalid_message_id = base.clone();
        invalid_message_id["read_states"][0]["read_up_to_message_id"] = json!(42);
        assert!(parse_snapshot(&invalid_message_id).is_err());

        let mut invalid_device_id = base.clone();
        invalid_device_id["read_states"][0]["updated_by_device_id"] = json!({"id": "device-1"});
        assert!(parse_snapshot(&invalid_device_id).is_err());

        let mut nullable = base.clone();
        nullable["read_states"][0]["updated_by_device_id"] = Value::Null;
        assert!(parse_snapshot(&nullable).is_ok());

        let mut unknown = base;
        unknown["read_states"][0]["recovery_token"] = json!("forbidden");
        assert!(parse_snapshot(&unknown).is_err());
    }

    #[test]
    fn v1a_lane_bootstrap_and_delta_round_trip_e2ee_lane_cursors() {
        let bootstrap = parse_bootstrap(&json!({
            "mode": "tail_only",
            "account_id": "account-1",
            "device_id": "device-1",
            "server_time": "2026-08-15T00:00:00Z",
            "cursor": {"stream_epoch": "3", "scan_seq": "9"},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
            "snapshot_capability": {"schema": 3, "delivery": "paged_v1"},
            "p6_delivery": {
                "profile": P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": "client-installation-1",
                "activated": true
            },
            "sync_capabilities": [
                SYNC_CAPABILITY_P5_DEVICE_V1,
                SYNC_CAPABILITY_P6_GROUP_V1
            ],
            "lanes": {
                "p5_device": {
                    "cursor": {"stream_epoch": "41", "scan_seq": "36"},
                    "committed_seq": "36"
                },
                "p6_group": {
                    "cursor": {"stream_epoch": "42", "scan_seq": "58"},
                    "committed_seq": "57"
                }
            }
        }))
        .unwrap();
        assert_eq!(
            bootstrap.lane_bootstrap.capabilities,
            BTreeSet::from([SyncLaneV3::P5Device, SyncLaneV3::P6Group])
        );
        assert_eq!(
            bootstrap.lane_bootstrap.lanes[&SyncLaneV3::P6Group].committed_seq,
            "57"
        );

        let identity = WireIdentity {
            did: "did:example:alice".to_owned(),
        };
        let ordinary = SyncCursorV2 {
            stream_epoch: "3".to_owned(),
            scan_seq: "9".to_owned(),
        };
        let ordinary_only = build_delta_params(
            &identity,
            &ordinary,
            100,
            "app_resume",
            "client-installation-1",
        )
        .unwrap();
        assert_eq!(
            ordinary_only["body"],
            json!({
                "cursor": {"stream_epoch": "3", "scan_seq": "9"},
                "limit": 100,
                "reason": "app_resume"
            })
        );
        let params = build_delta_params_with_lanes(
            &identity,
            &ordinary,
            100,
            "app_resume",
            &bootstrap.lane_bootstrap.lanes,
            "client-installation-1",
        )
        .unwrap();
        assert_eq!(params["body"]["lanes"]["p5_device"]["committed_seq"], "36");
        assert_eq!(
            params["body"]["lanes"]["p6_group"]["cursor"]["scan_seq"],
            "58"
        );
    }

    #[test]
    fn v1a_explicit_negotiation_discovery_and_default_requests_zero_secure_lanes() {
        let identity = WireIdentity {
            did: "did:example:alice".to_owned(),
        };
        let discovery = build_capability_discovery_params(&identity).unwrap();
        assert_eq!(discovery["body"], json!({}));
        assert_eq!(discovery["meta"]["profile"], "anp.core.binding.v1");
        require_explicit_sync_negotiation_capability(&json!({
            "supported_profiles": [
                MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1,
                SNAPSHOT_PAGING_V1
            ]
        }))
        .unwrap();
        assert!(require_explicit_sync_negotiation_capability(&json!({
            "supported_profiles": []
        }))
        .is_err());

        let bootstrap = build_bootstrap_params(&identity, "core-installation-v1a").unwrap();
        assert_eq!(
            bootstrap["body"]["capabilities"]["requested_sync_capabilities"],
            json!([])
        );
        assert_eq!(
            bootstrap["body"]["capabilities"]["requested_snapshot_capabilities"],
            json!({"schema_max": 3, "deliveries": ["paged_v1"]})
        );
        assert!(bootstrap["body"]["capabilities"]
            .get("p6_delivery")
            .is_none());
        let delta = build_delta_params(
            &identity,
            &SyncCursorV2 {
                stream_epoch: "1".to_owned(),
                scan_seq: "0".to_owned(),
            },
            100,
            "app_resume",
            "core-installation-v1a",
        )
        .unwrap();
        assert!(delta["body"].get("lanes").is_none());
        assert!(delta["body"].get("p6_delivery").is_none());
    }

    #[test]
    fn v1b_handoff_activation_requests_each_ready_lane_without_a_second_shape() {
        let identity = WireIdentity {
            did: "did:wba:example.test:users:alice:e1_owner".to_owned(),
        };
        let p5 = build_bootstrap_params_with_lanes(
            &identity,
            "core-installation-v1b",
            &BTreeSet::from([SyncLaneV3::P5Device]),
        )
        .unwrap();
        assert_eq!(
            p5["body"]["capabilities"]["requested_sync_capabilities"],
            json!([SYNC_CAPABILITY_P5_DEVICE_V1])
        );
        assert!(p5["body"]["capabilities"].get("p6_delivery").is_none());

        let all = build_bootstrap_params_with_lanes(
            &identity,
            "core-installation-v1b",
            &BTreeSet::from([SyncLaneV3::P5Device, SyncLaneV3::P6Group]),
        )
        .unwrap();
        assert_eq!(
            all["body"]["capabilities"]["requested_sync_capabilities"],
            json!([
                SYNC_CAPABILITY_P5_DEVICE_V1,
                SYNC_CAPABILITY_P6_GROUP_V1,
                P6_DELIVERY_CONTEXT_CAPABILITY_V1
            ])
        );
        assert_eq!(
            all["body"]["capabilities"]["p6_delivery"],
            P6_DELIVERY_CONTEXT_CAPABILITY_V1
        );
    }

    #[test]
    fn v1a_delta_lane_parser_enforces_closed_e2ee_profiles_and_isolates_errors() {
        let base = json!({
            "mode": "delta",
            "server_time": "2026-08-15T00:00:01Z",
            "events": [],
            "next_cursor": {"stream_epoch": "3", "scan_seq": "9"},
            "has_more": false,
            "recovery": null,
            "warnings": [],
            "lanes": {
                "p5_device": {
                    "events": [{
                        "event_type": "p5.delivery.created",
                        "delivery_id": "p5-37",
                        "seq": "37",
                        "envelope": {
                            "meta": {
                                "profile": "anp.direct.e2ee.v2",
                                "security_profile": "direct-e2ee"
                            },
                            "body": {},
                            "server_seq": 7
                        }
                    }],
                    "next_cursor": {"stream_epoch": "41", "scan_seq": "37"},
                    "has_more": false
                },
                "p6_group": {
                    "error": {
                        "code": 4602,
                        "anp_code": "p6_group_recovery_required",
                        "message": "lane cursor is fenced"
                    }
                }
            }
        });
        let page = parse_delta(&base).unwrap();
        assert!(matches!(
            page.lanes[&SyncLaneV3::P5Device],
            SyncLaneDeltaSectionV3::Page { ref events, .. }
                if matches!(events.as_slice(), [SyncLaneEventV3::P5Delivery { delivery_id, .. }] if delivery_id == "p5-37")
        ));
        assert!(matches!(
            page.lanes[&SyncLaneV3::P6Group],
            SyncLaneDeltaSectionV3::Error(ref error)
                if error.anp_code == "p6_group_recovery_required"
        ));

        let mut cross_lane = base;
        cross_lane["lanes"]["p5_device"]["events"][0]["envelope"]["meta"]["profile"] =
            json!("anp.group.e2ee.v2");
        cross_lane["lanes"]["p5_device"]["events"][0]["envelope"]["meta"]["security_profile"] =
            json!("group-e2ee");
        cross_lane["lanes"]["future_lane"] = json!({
            "events": [],
            "next_cursor": {"stream_epoch": "99", "scan_seq": "0"},
            "has_more": false
        });
        let page = parse_delta(&cross_lane).unwrap();
        assert!(page.lane_transport_invalid);
        assert!(matches!(
            page.lanes[&SyncLaneV3::P5Device],
            SyncLaneDeltaSectionV3::TransportInvalid
        ));
        assert!(matches!(
            page.lanes[&SyncLaneV3::P6Group],
            SyncLaneDeltaSectionV3::Error(_)
        ));
        assert_eq!(
            page.next_cursor,
            SyncCursorV2 {
                stream_epoch: "3".to_owned(),
                scan_seq: "9".to_owned(),
            }
        );
    }

    #[test]
    fn p6_control_notice_lane_requires_the_transport_security_profile() {
        let mut event = json!({
            "event_type": "p6.control.notice",
            "notice_id": "notice-1",
            "seq": "1",
            "group_did": "did:example:group",
            "notice": {
                "meta": {
                    "profile": "anp.group.e2ee.v2",
                    "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
                },
                "body": {"notice_id": "notice-1"}
            }
        });

        assert!(matches!(
            parse_lane_event_v3(SyncLaneV3::P6Group, &event).unwrap(),
            SyncLaneEventV3::P6ControlNotice { notice_id, .. } if notice_id == "notice-1"
        ));

        event["notice"]["meta"]["security_profile"] = json!("group-e2ee");
        assert!(parse_lane_event_v3(SyncLaneV3::P6Group, &event).is_err());
    }
}
