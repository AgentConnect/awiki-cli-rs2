use std::time::Duration;

use super::{list_conversations_sync_request, sync_request, NodeSyncOptions};

#[test]
fn default_sync_uses_a_core_supported_manual_refresh_reason() {
    let (request, timeout) = sync_request(None, Duration::from_secs(15)).unwrap();

    assert_eq!(request.reason, "manual_refresh");
    assert_eq!(request.limit, None);
    assert_eq!(timeout, Duration::from_secs(15));
}

#[test]
fn conversation_listing_sync_uses_a_core_supported_manual_refresh_reason() {
    let request = list_conversations_sync_request();

    assert_eq!(request.reason, "manual_refresh");
    assert_eq!(request.limit, Some(100));
}

#[test]
fn explicit_supported_sync_reason_is_preserved() {
    let (request, _) = sync_request(
        Some(NodeSyncOptions {
            reason: Some("foreground_reconcile".to_owned()),
            limit: Some(25),
            timeout_ms: Some(500),
        }),
        Duration::from_secs(15),
    )
    .unwrap();

    assert_eq!(request.reason, "foreground_reconcile");
    assert_eq!(request.limit, Some(25));
}
