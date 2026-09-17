use super::*;

fn fixture(case: &str) -> Value {
    let fixtures: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-sync-v1.json"
    )))
    .unwrap();
    fixtures["cases"][case].clone()
}

#[test]
fn community_sync_requires_explicit_consistent_capability() {
    let raw = fixture("C01")["response"]["result"].clone();
    assert_eq!(
        discover_sync_service_mode(&raw).unwrap(),
        SyncServiceMode::Community
    );
    assert!(super::super::require_explicit_sync_negotiation_capability(&raw).is_err());
    for field in ["features", "supported_profiles", "service_did"] {
        let mut invalid = raw.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(
            discover_sync_service_mode(&invalid).is_err(),
            "missing {field}"
        );
    }
    for (field, value) in [
        ("snapshot", json!(true)),
        ("max_devices", json!(2)),
        ("max_client_instances", json!(2)),
        ("lanes", json!(["p5_device"])),
        ("ws_subprotocol", json!("awiki.sync.changed.v2")),
        ("extra", json!(false)),
    ] {
        let mut invalid = raw.clone();
        invalid["features"]["community_sync"][field] = value;
        assert!(
            discover_sync_service_mode(&invalid).is_err(),
            "invalid {field}"
        );
    }
    for profile in [
        SNAPSHOT_PAGING_V1,
        MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1,
        "anp.direct.e2ee.v2",
    ] {
        let mut invalid = raw.clone();
        invalid["supported_profiles"]
            .as_array_mut()
            .unwrap()
            .push(json!(profile));
        assert!(
            discover_sync_service_mode(&invalid).is_err(),
            "conflicting {profile}"
        );
    }
}

#[test]
fn community_sync_does_not_weaken_commercial_discovery() {
    let commercial =
        json!({"supported_profiles": [MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1, SNAPSHOT_PAGING_V1]});
    assert_eq!(
        discover_sync_service_mode(&commercial).unwrap(),
        SyncServiceMode::Commercial
    );
    for raw in [
        json!(null),
        json!({}),
        json!({"supported_profiles": []}),
        json!({"supported_profiles": [MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1]}),
    ] {
        assert!(discover_sync_service_mode(&raw).is_err());
    }
    let mut conflicting = commercial;
    conflicting["features"] = json!({"community_sync": null});
    assert!(discover_sync_service_mode(&conflicting).is_err());
}

#[test]
fn community_sync_bootstrap_request_matches_server_fixture_without_commercial_fields() {
    let fixture = fixture("C02");
    let expected = &fixture["request"]["params"];
    let identity = WireIdentity {
        did: expected["meta"]["sender_did"].as_str().unwrap().to_owned(),
    };
    let built = build_community_bootstrap_params(&identity, "installation-alice-1").unwrap();
    assert_eq!(built["body"], expected["body"]);
    assert_eq!(built["meta"]["sender_did"], expected["meta"]["sender_did"]);
    assert_eq!(built["meta"]["profile"], expected["meta"]["profile"]);
    assert!(build_community_bootstrap_params(&identity, "").is_err());
    assert!(build_community_bootstrap_params(
        &WireIdentity { did: String::new() },
        "installation-alice-1"
    )
    .is_err());
}

#[test]
fn community_sync_bootstrap_preserves_plain_binding_without_snapshot() {
    let raw = fixture("C02")["response"]["result"].clone();
    let bootstrap = parse_community_bootstrap(&raw).unwrap();
    assert_eq!(bootstrap.account_id, "account-alice-1");
    assert_eq!(bootstrap.device_id, "device-alice-1");
    assert_eq!(bootstrap.cursor.scan_seq, "0");
    assert!(!bootstrap.snapshot_paging_v1);
    assert!(bootstrap.lane_bootstrap.capabilities.is_empty());
    assert!(bootstrap.p6_delivery_client_instance_id.is_none());
    assert!(super::super::parse_bootstrap_response(&raw).is_err());
    for (field, value) in [
        (
            "snapshot_capability",
            json!({"schema": 3, "delivery": "paged_v1"}),
        ),
        ("lanes", json!({})),
        ("mode", json!("compact_recovery_required")),
        ("cursor", json!({"stream_epoch": "1", "scan_seq": "-1"})),
        ("account_id", json!("")),
        ("device_id", json!(null)),
        ("read_state_baseline", json!([null])),
    ] {
        let mut invalid = raw.clone();
        invalid[field] = value;
        assert!(
            parse_community_bootstrap(&invalid).is_err(),
            "invalid {field}"
        );
    }
}
