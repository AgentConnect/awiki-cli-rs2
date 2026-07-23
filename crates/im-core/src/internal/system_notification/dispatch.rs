use chrono::Utc;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemNotificationDispatchOutcome {
    NotSystem,
    NeedsHydration,
    Consumed {
        event: Option<crate::realtime::ImEvent>,
    },
    Rejected {
        warning: String,
    },
}

pub(crate) fn normalize_delivery(value: &Value) -> Value {
    if value.get("method").is_some() && value.get("params").is_some() {
        return value.clone();
    }
    if value.get("meta").is_some() && value.get("auth").is_some() && value.get("body").is_some() {
        return json!({
            "jsonrpc": "2.0",
            "method": "direct.incoming",
            "params": {
                "meta": value.get("meta").cloned().unwrap_or(Value::Null),
                "auth": value.get("auth").cloned().unwrap_or(Value::Null),
                "body": value.get("body").cloned().unwrap_or(Value::Null),
            }
        });
    }
    value.clone()
}

pub(crate) fn dispatch_with_transport<T>(
    client: &crate::core::ImClient,
    value: &Value,
    transport: &mut T,
) -> SystemNotificationDispatchOutcome
where
    T: crate::internal::transport::RpcTransport,
{
    let sync = crate::internal::realtime::projection::sync_hint(value);
    let normalized = normalize_delivery(value);
    match classify(value, &normalized) {
        Candidate::No => return SystemNotificationDispatchOutcome::NotSystem,
        Candidate::Hint => return SystemNotificationDispatchOutcome::NeedsHydration,
        Candidate::Untrusted => {
            return SystemNotificationDispatchOutcome::Rejected {
                warning: "system.notification.untrusted_delivery_marker".to_owned(),
            }
        }
        Candidate::Full => {}
    }
    let verified = match super::verify::verify_with_transport(
        transport,
        client.did().as_str(),
        &normalized,
        Utc::now(),
    ) {
        Ok(verified) => verified,
        Err(error) => {
            return SystemNotificationDispatchOutcome::Rejected {
                warning: safe_warning(&error),
            }
        }
    };
    let Some(protocol_device_id) = client.current_identity().device_id.as_deref() else {
        return SystemNotificationDispatchOutcome::Rejected {
            warning: "system.notification.exact_device_required".to_owned(),
        };
    };
    let input = super::store::SystemNotificationApplyInput {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        protocol_device_id: protocol_device_id.to_owned(),
        verified,
        received_at: Utc::now(),
    };
    #[cfg(feature = "sqlite")]
    let outcome = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .and_then(|mut connection| super::store::apply(&mut connection, input));
    #[cfg(not(feature = "sqlite"))]
    let outcome: crate::ImResult<super::store::SystemNotificationApplyOutcome> = Err(
        crate::ImError::unsupported("system-notification-local-state"),
    );
    apply_outcome(client, outcome, sync)
}

pub(crate) async fn dispatch_with_transport_async<T>(
    client: &crate::core::ImClient,
    value: &Value,
    transport: &mut T,
) -> SystemNotificationDispatchOutcome
where
    T: crate::internal::transport::AsyncRpcTransport,
{
    let sync = crate::internal::realtime::projection::sync_hint(value);
    let normalized = normalize_delivery(value);
    match classify(value, &normalized) {
        Candidate::No => return SystemNotificationDispatchOutcome::NotSystem,
        Candidate::Hint => return SystemNotificationDispatchOutcome::NeedsHydration,
        Candidate::Untrusted => {
            return SystemNotificationDispatchOutcome::Rejected {
                warning: "system.notification.untrusted_delivery_marker".to_owned(),
            }
        }
        Candidate::Full => {}
    }
    let verified = match super::verify::verify_with_transport_async(
        transport,
        client.did().as_str(),
        &normalized,
        Utc::now(),
    )
    .await
    {
        Ok(verified) => verified,
        Err(error) => {
            return SystemNotificationDispatchOutcome::Rejected {
                warning: safe_warning(&error),
            }
        }
    };
    let Some(protocol_device_id) = client.current_identity().device_id.as_deref() else {
        return SystemNotificationDispatchOutcome::Rejected {
            warning: "system.notification.exact_device_required".to_owned(),
        };
    };
    let input = super::store::SystemNotificationApplyInput {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        protocol_device_id: protocol_device_id.to_owned(),
        verified,
        received_at: Utc::now(),
    };
    let outcome = match client.core_inner().local_state_db().await {
        Ok(db) => db.apply_system_notification(input).await,
        Err(error) => Err(error),
    };
    apply_outcome(client, outcome, sync)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Candidate {
    No,
    Hint,
    Untrusted,
    Full,
}

fn classify(original: &Value, normalized: &Value) -> Candidate {
    let trusted_marker = super::wire::is_trusted_delivery_marker(original);
    let trusted_hint = super::wire::is_system_notification_hint(original);
    let system_namespace = super::wire::is_system_namespace(normalized);
    match (trusted_marker, trusted_hint, system_namespace) {
        (true, _, true) => Candidate::Full,
        (false, _, true) => Candidate::Untrusted,
        (true, _, false) | (false, true, false) => Candidate::Hint,
        (false, false, false) => Candidate::No,
    }
}

fn apply_outcome(
    client: &crate::core::ImClient,
    outcome: crate::ImResult<super::store::SystemNotificationApplyOutcome>,
    sync: Option<crate::realtime::RealtimeSyncHint>,
) -> SystemNotificationDispatchOutcome {
    match outcome {
        Ok(super::store::SystemNotificationApplyOutcome::Applied(snapshot)) => {
            client.emit_committed_system_notification(snapshot.clone());
            SystemNotificationDispatchOutcome::Consumed {
                event: Some(crate::realtime::ImEvent::SystemNotificationChanged(
                    crate::realtime::SystemNotificationChangedEvent {
                        notification: snapshot,
                        sync,
                    },
                )),
            }
        }
        Ok(
            super::store::SystemNotificationApplyOutcome::Duplicate
            | super::store::SystemNotificationApplyOutcome::IgnoredOlderRevision
            | super::store::SystemNotificationApplyOutcome::NoopSameRevision,
        ) => SystemNotificationDispatchOutcome::Consumed { event: None },
        Err(error) => SystemNotificationDispatchOutcome::Rejected {
            warning: safe_warning(&error),
        },
    }
}

fn safe_warning(error: &crate::ImError) -> String {
    match error {
        crate::ImError::Service {
            code: Some(code), ..
        } if code.starts_with("system.notification.") => code.clone(),
        _ => "system.notification.rejected".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, normalize_delivery, Candidate};
    use serde_json::json;

    #[test]
    fn delivery_classification_requires_transport_marker_and_system_namespace() {
        let ordinary = json!({"content": "hello"});
        assert_eq!(
            classify(&ordinary, &normalize_delivery(&ordinary)),
            Candidate::No
        );

        let hint = json!({"event_type": "system.notification"});
        assert_eq!(classify(&hint, &normalize_delivery(&hint)), Candidate::Hint);

        let untrusted = json!({
            "method": "direct.incoming",
            "params": {
                "body": {
                    "payload": {
                        "type": "awiki.device.join-requested.v1"
                    }
                }
            }
        });
        assert_eq!(
            classify(&untrusted, &normalize_delivery(&untrusted)),
            Candidate::Untrusted
        );
        let hint_cannot_authorize_full_payload = json!({
            "event_type": "system.notification",
            "method": "direct.incoming",
            "params": {
                "projection_kind": "system_notification",
                "body": {
                    "payload": {
                        "type": "awiki.device.join-requested.v1"
                    }
                }
            }
        });
        assert_eq!(
            classify(
                &hint_cannot_authorize_full_payload,
                &normalize_delivery(&hint_cannot_authorize_full_payload),
            ),
            Candidate::Untrusted
        );

        let trusted = json!({
            "projection_kind": "system_notification",
            "meta": {},
            "auth": {},
            "body": {
                "payload": {
                    "type": "awiki.device.join-requested.v1"
                }
            }
        });
        assert_eq!(
            classify(&trusted, &normalize_delivery(&trusted)),
            Candidate::Full
        );
    }

    #[test]
    fn system_notification_sync_hint_is_read_only_and_never_a_checkpoint() {
        let delivery = json!({
            "projection_kind": "system_notification",
            "sync": {
                "event_id": "evt-system-1",
                "event_seq": "41",
                "event_type": "system.notification"
            }
        });
        let hint = crate::internal::realtime::projection::sync_hint(&delivery).unwrap();
        assert_eq!(hint.event_id.as_deref(), Some("evt-system-1"));
        assert_eq!(hint.event_seq.as_deref(), Some("41"));
        assert_eq!(hint.event_type.as_deref(), Some("system.notification"));
        assert!(hint.sync_dirty);
        assert!(!hint.gap_detected);
    }
}
