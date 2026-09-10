use super::*;

fn identity() -> Value {
    json!({"did": "did:example:alice", "user_state": {"ready_for_messaging": true}})
}

fn listener() -> Value {
    json!({"installed": true, "running": true, "bridge_available": true,
        "sessions": [{"did": "did:example:alice", "connected": true}],
        "reliable_sync": {"v2_subprotocol_negotiated": true, "v2_bootstrap_completed": true, "legacy_sync_used": false}})
}

#[test]
fn realtime_readiness_requires_current_identity_connection_and_reliable_sync() {
    let ready = classify("websocket", true, Some(&identity()), &listener());
    assert_eq!(ready["identity_ready"], true);
    assert_eq!(ready["realtime"]["ready"], true);
    assert!(ready["realtime"]["next_command"].is_null());
    for (path, value, state) in [
        ("/installed", json!(false), "not_installed"),
        ("/running", json!(false), "stopped"),
        ("/bridge_available", json!(false), "disconnected"),
        (
            "/sessions/0/did",
            json!("did:example:other-tenant"),
            "disconnected",
        ),
        ("/sessions/0/connected", json!(false), "disconnected"),
        (
            "/reliable_sync/v2_subprotocol_negotiated",
            json!(false),
            "synchronizing",
        ),
        (
            "/reliable_sync/v2_bootstrap_completed",
            json!(false),
            "synchronizing",
        ),
        (
            "/reliable_sync/legacy_sync_used",
            json!(true),
            "synchronizing",
        ),
    ] {
        let mut probe = listener();
        *probe.pointer_mut(path).unwrap() = value;
        let view = classify("websocket", true, Some(&identity()), &probe);
        assert_eq!(view["identity_ready"], true);
        assert_eq!(view["realtime"]["state"], state, "{path}");
        assert_eq!(view["realtime"]["ready"], false);
        assert!(view["realtime"]["next_command"].is_string());
    }
}

#[test]
fn readiness_distinguishes_disabled_http_unknown_and_missing_identity() {
    for (mode, enabled, identity, probe, state) in [
        ("http", true, Some(identity()), listener(), "on_demand"),
        ("websocket", false, Some(identity()), listener(), "disabled"),
        (
            "websocket",
            true,
            Some(identity()),
            json!({"status_unavailable": true}),
            "unknown",
        ),
        ("websocket", true, None, listener(), "identity_required"),
    ] {
        let view = classify(mode, enabled, identity.as_ref(), &probe);
        assert_eq!(view["realtime"]["state"], state);
        assert_eq!(view["realtime"]["ready"], false);
        let mut warnings = vec![];
        append_warning(&view, &mut warnings);
        assert_eq!(warnings.is_empty(), state == "on_demand");
    }
}
