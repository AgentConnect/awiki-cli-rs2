use super::*;
use serde_json::json;
fn message(enabled: bool) -> Value {
    json!({"awiki_local": {"capabilities_version": 1, "capabilities": {"direct_e2ee": {"enabled": enabled, "protocol_version": 1}}}})
}
#[test]
fn missing_disabled_and_unknown_versions_are_distinct() {
    let raw = message(false);
    let caps = CapabilitySnapshot::from_message_discovery(&raw).unwrap();
    assert_eq!(caps.support("direct_e2ee"), Support::Unsupported);
    assert_eq!(caps.support("group_e2ee"), Support::Unknown);
    assert!(caps.require("direct_e2ee").is_err());
    assert!(caps.require("group_e2ee").is_err());
    assert_eq!(
        CapabilitySnapshot::from_message_discovery(&json!({}))
            .unwrap()
            .status,
        DiscoveryStatus::Unavailable
    );
    let mut raw = message(true);
    raw["awiki_local"]["capabilities_version"] = json!(2);
    assert_eq!(
        CapabilitySnapshot::from_message_discovery(&raw)
            .unwrap()
            .status,
        DiscoveryStatus::Incompatible
    );
}
#[test]
fn malformed_values_are_not_interpreted_as_disabled() {
    for enabled in [json!("false"), json!(null), json!(1)] {
        let mut raw = message(false);
        raw["awiki_local"]["capabilities"]["direct_e2ee"]["enabled"] = enabled;
        assert!(CapabilitySnapshot::from_message_discovery(&raw).is_err());
    }
}
#[test]
fn projection_discards_server_text_and_unknown_secret_fields() {
    let mut raw = message(false);
    raw["awiki_local"]["capabilities"]["direct_e2ee"]["reason_code"] = json!("PRIVATE_TOKEN");
    raw["awiki_local"]["capabilities"]["direct_e2ee"]["limits"] =
        json!({"token": "PRIVATE_TOKEN", "max_devices": 1});
    let caps = CapabilitySnapshot::from_message_discovery(&raw).unwrap();
    assert!(!serde_json::to_string(&caps)
        .unwrap()
        .contains("PRIVATE_TOKEN"));
    assert_eq!(caps.capabilities["direct_e2ee"].limits["max_devices"], 1);
}
#[tokio::test]
async fn refresh_retains_confirmed_snapshot_on_failure_but_applies_explicit_off() {
    let cache = CapabilityCache::default();
    cache
        .discover(false, || async {
            CapabilitySnapshot::from_message_discovery(&message(true))
        })
        .await
        .unwrap();
    let stale = cache
        .discover(true, || async { Err(invalid_contract()) })
        .await
        .unwrap();
    assert_eq!(stale.status, DiscoveryStatus::Stale);
    assert_eq!(stale.support("direct_e2ee"), Support::Supported);
    let suppressed = cache
        .discover(false, || async { panic!("must honor backoff") })
        .await
        .unwrap();
    assert_eq!(suppressed.status, DiscoveryStatus::Stale);
    cache.state.lock().await.retry_at = None;
    let off = cache
        .discover(true, || async {
            CapabilitySnapshot::from_message_discovery(&message(false))
        })
        .await
        .unwrap();
    assert_eq!(off.support("direct_e2ee"), Support::Unsupported);
}
#[tokio::test]
async fn concurrent_reads_coalesce_and_contexts_are_isolated() {
    let cache = CapabilityCache::default();
    let (a, b) = tokio::join!(
        cache.discover(false, || async {
            tokio::task::yield_now().await;
            CapabilitySnapshot::from_message_discovery(&message(true))
        }),
        cache.discover(false, || async { panic!("duplicate discovery") })
    );
    assert_eq!(a.unwrap(), b.unwrap());
    let other = CapabilityCache::default()
        .discover(false, || async { Err(invalid_contract()) })
        .await
        .unwrap();
    assert_eq!(other.status, DiscoveryStatus::Unavailable);
    assert_eq!(other.support("direct_e2ee"), Support::Unknown);
}
#[tokio::test]
async fn authentication_failure_is_not_a_negative_capability() {
    let cache = CapabilityCache::default();
    cache
        .discover(false, || async {
            CapabilitySnapshot::from_message_discovery(&message(true))
        })
        .await
        .unwrap();
    assert!(matches!(
        cache
            .discover(true, || async { Err(crate::ImError::PermissionDenied) })
            .await,
        Err(crate::ImError::PermissionDenied)
    ));
    assert_eq!(
        cache
            .state
            .lock()
            .await
            .snapshot
            .as_ref()
            .unwrap()
            .support("direct_e2ee"),
        Support::Supported
    );
}

#[tokio::test]
async fn simultaneous_forced_refreshes_share_one_request() {
    let cache = CapabilityCache::default();
    let (a, b) = tokio::join!(
        cache.discover(true, || async {
            tokio::task::yield_now().await;
            CapabilitySnapshot::from_message_discovery(&message(true))
        }),
        cache.discover(true, || async { panic!("duplicate forced refresh") })
    );
    assert_eq!(a.unwrap(), b.unwrap());
}
#[test]
fn handle_only_requires_explicit_versioned_method() {
    let mut raw = json!({"schema_version":1,"capabilities_version":1,"identity":{"handle_registration":{"enabled":true,"methods":[{"id":"handle_only","enabled":true,"verification":{"required":false,"type":"none"}}]}}});
    assert_eq!(
        CapabilitySnapshot::from_server_info(&raw)
            .unwrap()
            .support("registration.handle_only"),
        Support::Supported
    );
    raw["identity"]["handle_registration"]["methods"][0]["verification"]["required"] = json!(true);
    assert_eq!(
        CapabilitySnapshot::from_server_info(&raw)
            .unwrap()
            .support("registration.handle_only"),
        Support::Unsupported
    );
    raw.as_object_mut().unwrap().remove("capabilities_version");
    assert_eq!(
        CapabilitySnapshot::from_server_info(&raw)
            .unwrap()
            .support("registration.handle_only"),
        Support::Unknown
    );
}

#[tokio::test]
async fn stable_not_supported_stops_future_work_but_auth_and_old_generation_do_not() {
    let cache = CapabilityCache::default();
    cache
        .discover(false, || async {
            CapabilitySnapshot::from_message_discovery(&message(true))
        })
        .await
        .unwrap();
    let error = |status_code| {
        Err(crate::ImError::Service {
            status_code,
            code: Some("not_supported".into()),
            message: "private server text".into(),
            data: None,
        })
    };
    cache
        .observe_rpc_result(
            cache.generation(),
            "direct.e2ee.publish_prekey_bundle",
            &error(Some(403)),
        )
        .await;
    assert_eq!(
        cache
            .state
            .lock()
            .await
            .snapshot
            .as_ref()
            .unwrap()
            .support("direct_e2ee"),
        Support::Supported
    );
    cache
        .observe_rpc_result(
            cache.generation() + 1,
            "direct.e2ee.publish_prekey_bundle",
            &error(Some(200)),
        )
        .await;
    assert_eq!(
        cache
            .state
            .lock()
            .await
            .snapshot
            .as_ref()
            .unwrap()
            .support("direct_e2ee"),
        Support::Supported
    );
    cache
        .observe_rpc_result(
            cache.generation(),
            "direct.e2ee.publish_prekey_bundle",
            &error(Some(200)),
        )
        .await;
    assert_eq!(
        cache
            .discover(false, || async {
                panic!("must stop retrying unsupported operations")
            })
            .await
            .unwrap()
            .support("direct_e2ee"),
        Support::Unsupported
    );
}

#[tokio::test]
async fn expired_authentication_is_not_hidden_as_discovery_unavailable() {
    for error in [
        crate::ImError::AuthRequired,
        crate::ImError::SessionExpired,
        crate::ImError::PermissionDenied,
    ] {
        let cache = CapabilityCache::default();
        assert!(cache
            .discover(false, || async { Err(error) })
            .await
            .is_err());
        assert!(cache.state.lock().await.snapshot.is_none());
    }
}

#[tokio::test]
async fn rpc_authentication_codes_are_propagated_without_stale_fallback() {
    for code in ["1401", "1403"] {
        let cache = CapabilityCache::default();
        cache
            .discover(false, || async {
                CapabilitySnapshot::from_message_discovery(&message(true))
            })
            .await
            .unwrap();
        let result = cache
            .discover(true, || async {
                Err(crate::ImError::Service {
                    status_code: Some(200),
                    code: Some(code.into()),
                    message: "authentication required".into(),
                    data: None,
                })
            })
            .await;
        assert!(matches!(result, Err(crate::ImError::Service { .. })));
        let state = cache.state.lock().await;
        assert_eq!(state.failures, 0);
        assert!(state.retry_at.is_none());
        assert_eq!(
            state.snapshot.as_ref().unwrap().support("direct_e2ee"),
            Support::Supported
        );
    }
}
