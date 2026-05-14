use awiki_cli::store;

#[test]
fn make_thread_id_matches_go_direct_group_and_unknown_contracts() {
    assert_eq!(
        store::make_thread_id(" did:z ", " did:a ", ""),
        "dm:did:a:did:z"
    );
    assert_eq!(
        store::make_thread_id("did:a", "did:z", ""),
        "dm:did:a:did:z"
    );
    assert_eq!(
        store::make_thread_id("did:owner", "", " group-1 "),
        "group:group-1"
    );
    assert_eq!(
        store::make_thread_id(" did:owner ", "", ""),
        "dm:did:owner:unknown"
    );
}

#[test]
fn now_utc_emits_rfc3339_utc_timestamp_shape() {
    let timestamp = store::now_utc();
    assert!(
        timestamp.ends_with('Z'),
        "timestamp should be UTC RFC3339: {timestamp}"
    );
    assert!(
        time::OffsetDateTime::parse(&timestamp, &time::format_description::well_known::Rfc3339,)
            .is_ok(),
        "timestamp should parse as RFC3339: {timestamp}"
    );
}
