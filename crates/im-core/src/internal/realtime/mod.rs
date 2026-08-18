pub(crate) mod async_ws_transport;
pub(crate) mod attachment_projection;
pub(crate) mod dispatch;
pub(crate) mod frame;
pub(crate) mod heartbeat;
pub(crate) mod local_projection;
pub(crate) mod notification;
pub(crate) mod projection;
pub(crate) mod reconnect;
pub(crate) mod session_loop;
pub(crate) mod shutdown;
pub(crate) mod transport;
#[cfg(feature = "blocking")]
pub(crate) mod ws_transport;

pub(crate) const SYNC_EVENT_V3_SUBPROTOCOL: &str = "awiki.sync.event.v3";
pub(crate) const SYNC_CHANGED_V2_SUBPROTOCOL: &str = "awiki.sync.changed.v2";
pub(crate) const P6_DELIVERY_CONTEXT_V1_SUBPROTOCOL: &str =
    "awiki.sync.event.v3.p6-delivery-context.v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SyncNotificationSubprotocol {
    #[default]
    Legacy,
    V2,
    V3,
}

pub(crate) fn accepts_negotiated_notification(
    subprotocol: SyncNotificationSubprotocol,
    message: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    if message.get("method").and_then(serde_json::Value::as_str) != Some("sync.changed") {
        return true;
    }
    let schema_version = message
        .get("sync")
        .and_then(serde_json::Value::as_object)
        .and_then(|sync| sync.get("schema_version"))
        .and_then(serde_json::Value::as_u64);
    match subprotocol {
        SyncNotificationSubprotocol::Legacy => false,
        SyncNotificationSubprotocol::V2 => schema_version == Some(2),
        SyncNotificationSubprotocol::V3 => matches!(schema_version, Some(2 | 3)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn v1_fallback_ignores_v2_notifications_but_keeps_v1_notifications() {
        let v2 = json!({"method": "sync.changed", "sync": {"schema_version": 2}})
            .as_object()
            .unwrap()
            .clone();
        let v1 = json!({"method": "direct.incoming"})
            .as_object()
            .unwrap()
            .clone();

        assert!(!super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::Legacy,
            &v2
        ));
        assert!(super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::Legacy,
            &v1
        ));
        assert!(super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::V2,
            &v2
        ));
    }

    #[test]
    fn v3_negotiation_accepts_inline_and_v2_fallback_notifications() {
        let v3 = json!({"method": "sync.changed", "sync": {"schema_version": 3}})
            .as_object()
            .unwrap()
            .clone();
        let v2 = json!({"method": "sync.changed", "sync": {"schema_version": 2}})
            .as_object()
            .unwrap()
            .clone();

        assert!(!super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::V2,
            &v3
        ));
        assert!(super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::V3,
            &v3
        ));
        assert!(super::accepts_negotiated_notification(
            super::SyncNotificationSubprotocol::V3,
            &v2
        ));
    }
}
