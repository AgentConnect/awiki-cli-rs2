use std::collections::BTreeSet;

use serde_json::{Map, Value};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::internal::local_state::old_admin_recovery_notices::{
    OldAdminRecoveryNoticeRecord, OldAdminRecoveryNoticeScope,
};

pub(crate) const RECOVERY_STARTED_EVENT_TYPE: &str = "identity.recovery_started";
const RECOVERY_AGGREGATE_KIND: &str = "identity_recovery";
const DURABLE_EVENT_PREFIX: &str = "identity-recovery-started:";
const MIN_COOLING_PERIOD_SECONDS: u64 = 3_600;
const MAX_COOLING_PERIOD_SECONDS: u64 = 604_800;
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

pub(crate) enum RealtimeRecoveryNoticeProjection {
    NotRecoveryControl,
    RecoveryNotice(crate::ImResult<OldAdminRecoveryNoticeRecord>),
    UnknownRecoveryControl,
}

pub(crate) fn scope_for_client(
    client: &crate::core::ImClient,
) -> crate::ImResult<OldAdminRecoveryNoticeScope> {
    let owner_device_id = client
        .current_identity()
        .device_id
        .as_deref()
        .ok_or_else(|| invalid_notice("current identity has no protocol device id"))?;
    let owner_device_id = crate::ids::ProtocolDeviceId::parse(owner_device_id)?;
    Ok(OldAdminRecoveryNoticeScope {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        owner_device_id: owner_device_id.as_str().to_owned(),
    })
}

pub(crate) fn list_old_admin_notices(
    core: &crate::core::ImCore,
    old_identity: crate::identity::IdentitySelector,
) -> crate::ImResult<Vec<crate::identity::OldAdminRecoveryNotice>> {
    let client = core.client(old_identity)?;
    let scope = scope_for_client(&client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::old_admin_recovery_notices::list_active(
        &connection,
        &scope,
        OffsetDateTime::now_utc(),
    )?
    .into_iter()
    .map(public_notice)
    .collect()
}

pub(crate) fn get_old_admin_notice(
    core: &crate::core::ImCore,
    old_identity: crate::identity::IdentitySelector,
    event_id: &str,
) -> crate::ImResult<Option<crate::identity::OldAdminRecoveryNotice>> {
    let client = core.client(old_identity)?;
    let scope = scope_for_client(&client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::old_admin_recovery_notices::get_active(
        &connection,
        &scope,
        event_id,
        OffsetDateTime::now_utc(),
    )?
    .map(public_notice)
    .transpose()
}

pub(crate) fn dismiss_old_admin_notice(
    core: &crate::core::ImCore,
    request: crate::identity::OldAdminRecoveryNoticeDismissRequest,
) -> crate::ImResult<crate::identity::OldAdminRecoveryNoticeDismissResult> {
    let client = core.client(request.old_identity)?;
    let scope = scope_for_client(&client)?;
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let dismissed = crate::internal::local_state::old_admin_recovery_notices::dismiss_active(
        &mut connection,
        &scope,
        &request.event_id,
        OffsetDateTime::now_utc(),
    )?;
    Ok(crate::identity::OldAdminRecoveryNoticeDismissResult {
        event_id: request.event_id,
        dismissed,
    })
}

fn public_notice(
    record: OldAdminRecoveryNoticeRecord,
) -> crate::ImResult<crate::identity::OldAdminRecoveryNotice> {
    Ok(crate::identity::OldAdminRecoveryNotice {
        event_id: record.event_id,
        recovery_session_id: record.recovery_session_id,
        handle: crate::ids::Handle::parse(record.handle, "")?,
        old_did: crate::ids::Did::parse(record.owner_did)?,
        requested_at: record.requested_at,
        cancellable_until: record.cancellable_until,
    })
}

pub(crate) fn parse_sync_event(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync::SyncDeltaEvent,
    now: OffsetDateTime,
) -> crate::ImResult<OldAdminRecoveryNoticeRecord> {
    if event.event_type != RECOVERY_STARTED_EVENT_TYPE {
        return Err(invalid_notice("unexpected recovery notice event type"));
    }
    if event.aggregate_kind.as_deref() != Some(RECOVERY_AGGREGATE_KIND) {
        return Err(invalid_notice("recovery notice aggregate kind is invalid"));
    }
    if event.owner_subject_id.as_deref() != Some(client.did().as_str()) {
        return Err(invalid_notice(
            "recovery notice owner DID does not match current DID",
        ));
    }
    let payload = exact_object(
        &event.payload,
        &[
            "event_id",
            "recovery_session_id",
            "handle",
            "requested_at",
            "cooling_period_seconds",
        ],
        "recovery notice payload schema is invalid",
    )?;
    let source_event_id = required_identifier(payload, "event_id")?;
    let recovery_session_id = required_identifier(payload, "recovery_session_id")?;
    if event.aggregate_id.as_deref() != Some(recovery_session_id.as_str()) {
        return Err(invalid_notice(
            "recovery notice aggregate id does not match recovery session",
        ));
    }
    let expected_durable_event_id = format!("{DURABLE_EVENT_PREFIX}{source_event_id}");
    if event.event_id != expected_durable_event_id {
        return Err(invalid_notice(
            "durable recovery notice event id is invalid",
        ));
    }
    let requested_at = required_string(payload, "requested_at")?;
    let created_at = event
        .created_at
        .as_deref()
        .ok_or_else(|| invalid_notice("durable recovery notice has no creation time"))?;
    let created_at = parse_time("created_at", created_at)?;
    let requested_at_value = parse_time("requested_at", &requested_at)?;
    if created_at != requested_at_value {
        return Err(invalid_notice(
            "recovery notice request time does not match durable event time",
        ));
    }
    validated_record(
        client,
        event.event_id.clone(),
        source_event_id,
        recovery_session_id,
        required_string(payload, "handle")?,
        requested_at_value,
        cooling_period_seconds(payload)?,
        now,
    )
}

pub(crate) fn classify_realtime_notification(
    client: &crate::core::ImClient,
    notification: &Value,
    now: OffsetDateTime,
) -> RealtimeRecoveryNoticeProjection {
    let Some(method) = notification.get("method").and_then(Value::as_str) else {
        return RealtimeRecoveryNoticeProjection::NotRecoveryControl;
    };
    if method != RECOVERY_STARTED_EVENT_TYPE {
        return if method.starts_with("identity.recovery_") {
            RealtimeRecoveryNoticeProjection::UnknownRecoveryControl
        } else {
            RealtimeRecoveryNoticeProjection::NotRecoveryControl
        };
    }
    RealtimeRecoveryNoticeProjection::RecoveryNotice(parse_realtime_notification(
        client,
        notification,
        now,
    ))
}

fn parse_realtime_notification(
    client: &crate::core::ImClient,
    notification: &Value,
    now: OffsetDateTime,
) -> crate::ImResult<OldAdminRecoveryNoticeRecord> {
    let envelope = exact_object(
        notification,
        &["jsonrpc", "method", "params", "sync"],
        "recovery realtime envelope schema is invalid",
    )?;
    if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid_notice(
            "recovery realtime JSON-RPC version is invalid",
        ));
    }
    if envelope.get("method").and_then(Value::as_str) != Some(RECOVERY_STARTED_EVENT_TYPE) {
        return Err(invalid_notice("recovery realtime method is invalid"));
    }
    let params = exact_object(
        envelope
            .get("params")
            .ok_or_else(|| invalid_notice("recovery realtime params are missing"))?,
        &[
            "event_id",
            "recovery_session_id",
            "handle",
            "requested_at",
            "cooling_period_seconds",
        ],
        "recovery realtime params schema is invalid",
    )?;
    let sync = exact_object(
        envelope
            .get("sync")
            .ok_or_else(|| invalid_notice("recovery realtime sync hint is missing"))?,
        &["event_id", "event_seq", "event_type"],
        "recovery realtime sync schema is invalid",
    )?;
    if sync.get("event_type").and_then(Value::as_str) != Some(RECOVERY_STARTED_EVENT_TYPE) {
        return Err(invalid_notice(
            "recovery realtime sync event type is invalid",
        ));
    }
    let event_seq = required_string(sync, "event_seq")?;
    crate::internal::local_state::sync_state::normalize_decimal_seq(&event_seq)
        .map_err(|_| invalid_notice("recovery realtime event sequence is invalid"))?;
    let source_event_id = required_identifier(params, "event_id")?;
    let durable_event_id = required_identifier(sync, "event_id")?;
    if durable_event_id != format!("{DURABLE_EVENT_PREFIX}{source_event_id}") {
        return Err(invalid_notice(
            "recovery realtime durable event id is invalid",
        ));
    }
    validated_record(
        client,
        durable_event_id,
        source_event_id,
        required_identifier(params, "recovery_session_id")?,
        required_string(params, "handle")?,
        parse_time("requested_at", &required_string(params, "requested_at")?)?,
        cooling_period_seconds(params)?,
        now,
    )
}

#[allow(clippy::too_many_arguments)]
fn validated_record(
    client: &crate::core::ImClient,
    event_id: String,
    source_event_id: String,
    recovery_session_id: String,
    handle: String,
    requested_at: OffsetDateTime,
    cooling_period_seconds: u64,
    now: OffsetDateTime,
) -> crate::ImResult<OldAdminRecoveryNoticeRecord> {
    if requested_at > now + Duration::seconds(MAX_CLOCK_SKEW_SECONDS) {
        return Err(invalid_notice(
            "recovery notice request time is in the future",
        ));
    }
    let cooling_period_seconds = i64::try_from(cooling_period_seconds)
        .map_err(|_| invalid_notice("recovery notice cooling period is invalid"))?;
    let cancellable_until = requested_at
        .checked_add(Duration::seconds(cooling_period_seconds))
        .ok_or_else(|| invalid_notice("recovery notice cooling deadline overflowed"))?;
    let handle = crate::ids::Handle::parse(&handle, client.did_domain())?;
    if let Some(current_handle) = client.handle() {
        if current_handle != &handle {
            return Err(invalid_notice(
                "recovery notice handle does not match current identity",
            ));
        }
    }
    let scope = scope_for_client(client)?;
    Ok(OldAdminRecoveryNoticeRecord {
        owner_identity_id: scope.owner_identity_id,
        owner_did: scope.owner_did,
        owner_device_id: scope.owner_device_id,
        event_id,
        source_event_id,
        recovery_session_id,
        handle: handle.as_str().to_owned(),
        requested_at: format_time(requested_at)?,
        cancellable_until: format_time(cancellable_until)?,
    })
}

fn exact_object<'a>(
    value: &'a Value,
    expected_keys: &[&str],
    error: &'static str,
) -> crate::ImResult<&'a Map<String, Value>> {
    let object = value.as_object().ok_or_else(|| invalid_notice(error))?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected_keys.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid_notice(error));
    }
    Ok(object)
}

fn required_identifier(
    object: &Map<String, Value>,
    field: &'static str,
) -> crate::ImResult<String> {
    let value = required_string(object, field)?;
    if value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(invalid_notice("recovery notice identifier is invalid"));
    }
    Ok(value)
}

fn required_string(object: &Map<String, Value>, field: &'static str) -> crate::ImResult<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_notice(format!("recovery notice {field} is missing")))
}

fn cooling_period_seconds(object: &Map<String, Value>) -> crate::ImResult<u64> {
    let value = object
        .get("cooling_period_seconds")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_notice("recovery notice cooling period is invalid"))?;
    if !(MIN_COOLING_PERIOD_SECONDS..=MAX_COOLING_PERIOD_SECONDS).contains(&value) {
        return Err(invalid_notice(
            "recovery notice cooling period is out of range",
        ));
    }
    Ok(value)
}

fn parse_time(field: &'static str, value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value.trim(), &Rfc3339)
        .map_err(|_| invalid_notice(format!("recovery notice {field} is invalid")))
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| invalid_notice("recovery notice timestamp formatting failed"))
}

fn invalid_notice(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("identity.recovery_notice_invalid".to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse("2030-01-01T12:00:00Z", &Rfc3339).unwrap()
    }

    fn client() -> crate::core::ImClient {
        let root = tempfile::tempdir().unwrap().into_path();
        let core = crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: root.join("identities"),
                    registry_path: root.join("identities.json"),
                    default_identity_path: None,
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.join("local.sqlite3"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.join("cache"),
                    temp_dir: root.join("tmp"),
                },
            },
        )
        .unwrap();
        core.client_with_identity_material(crate::identity::HostedIdentityMaterial {
            identity_id: "identity-alice".to_owned(),
            did: "did:wba:awiki.test:user:alice".to_owned(),
            handle: Some("alice.awiki.test".to_owned()),
            display_name: None,
            did_document: json!({"id": "did:wba:awiki.test:user:alice"}),
            default_signing_private_key_pem: "test-signing-secret".to_owned(),
            e2ee_agreement_private_key_pem: None,
            auth_token: None,
        })
        .unwrap()
    }

    fn realtime() -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "identity.recovery_started",
            "params": {
                "event_id": "event-1",
                "recovery_session_id": "session-1",
                "handle": "alice.awiki.test",
                "requested_at": "2030-01-01T00:00:00Z",
                "cooling_period_seconds": 86400
            },
            "sync": {
                "event_id": "identity-recovery-started:event-1",
                "event_seq": "7",
                "event_type": "identity.recovery_started"
            }
        })
    }

    #[test]
    fn realtime_exact_schema_is_required_and_never_accepts_secret_fields() {
        let mut client = client();
        client.set_protocol_device_id_for_test("dev-old-admin");
        let projected = classify_realtime_notification(&client, &realtime(), now());
        let RealtimeRecoveryNoticeProjection::RecoveryNotice(Ok(record)) = projected else {
            panic!("expected valid recovery notice");
        };
        assert_eq!(record.event_id, "identity-recovery-started:event-1");
        assert_eq!(record.cancellable_until, "2030-01-02T00:00:00Z");

        let mut malformed = realtime();
        malformed["params"]["otp"] = json!("super-secret-otp");
        let RealtimeRecoveryNoticeProjection::RecoveryNotice(Err(error)) =
            classify_realtime_notification(&client, &malformed, now())
        else {
            panic!("expected malformed recovery notice");
        };
        assert!(!error.to_string().contains("super-secret-otp"));
    }

    #[test]
    fn unknown_recovery_control_is_fail_closed() {
        let client = client();
        assert!(matches!(
            classify_realtime_notification(
                &client,
                &json!({"method": "identity.recovery_future"}),
                now()
            ),
            RealtimeRecoveryNoticeProjection::UnknownRecoveryControl
        ));
    }

    #[test]
    fn durable_sync_notice_requires_exact_payload_and_current_did() {
        let mut client = client();
        client.set_protocol_device_id_for_test("dev-old-admin");
        let mut event = crate::internal::wire::sync::SyncDeltaEvent {
            event_id: "identity-recovery-started:event-1".to_owned(),
            event_seq: "7".to_owned(),
            event_type: "identity.recovery_started".to_owned(),
            aggregate_kind: Some("identity_recovery".to_owned()),
            aggregate_id: Some("session-1".to_owned()),
            owner_subject_id: Some("did:wba:awiki.test:user:alice".to_owned()),
            created_at: Some("2030-01-01T00:00:00Z".to_owned()),
            payload: json!({
                "event_id": "event-1",
                "recovery_session_id": "session-1",
                "handle": "alice.awiki.test",
                "requested_at": "2030-01-01T00:00:00Z",
                "cooling_period_seconds": 86400
            }),
        };

        let record = parse_sync_event(&client, &event, now()).unwrap();
        assert_eq!(record.recovery_session_id, "session-1");

        event.payload["token"] = json!("super-secret-token");
        let error = parse_sync_event(&client, &event, now()).unwrap_err();
        assert!(!error.to_string().contains("super-secret-token"));
        event.payload.as_object_mut().unwrap().remove("token");
        event.owner_subject_id = Some("did:wba:awiki.test:user:mallory".to_owned());
        assert!(parse_sync_event(&client, &event, now()).is_err());
    }
}
