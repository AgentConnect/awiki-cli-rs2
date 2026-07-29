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

pub(crate) const SYNC_CHANGED_V2_SUBPROTOCOL: &str = "awiki.sync.changed.v2";

pub(crate) fn accepts_negotiated_notification(
    sync_changed_v2: bool,
    message: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    sync_changed_v2
        || message.get("method").and_then(serde_json::Value::as_str) != Some("sync.changed")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn v1_fallback_ignores_v2_notifications_but_keeps_v1_notifications() {
        let v2 = json!({"method": "sync.changed"})
            .as_object()
            .unwrap()
            .clone();
        let v1 = json!({"method": "direct.incoming"})
            .as_object()
            .unwrap()
            .clone();

        assert!(!super::accepts_negotiated_notification(false, &v2));
        assert!(super::accepts_negotiated_notification(false, &v1));
        assert!(super::accepts_negotiated_notification(true, &v2));
    }
}
