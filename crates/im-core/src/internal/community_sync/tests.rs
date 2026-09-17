use super::*;

fn fixture() -> (Connection, ModeBinding) {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    db.execute("INSERT INTO identity_account_bindings VALUES ('owner','account',NULL,'did:wba:home:user','device','1','1',0,0)", []).unwrap();
    let context = ModeBinding {
        home: "https://home".into(),
        account: "account".into(),
        did: "did:wba:home:user".into(),
        device: "device".into(),
        key: "did:wba:home:user#device".into(),
        generation: "1".into(),
        installation: "installation".into(),
    };
    (db, context)
}

fn saved(db: &Connection) -> String {
    db.query_row(
        "SELECT binding_json FROM sync_service_modes WHERE owner_identity_id='owner'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn community_mode_fences_identity_changes_and_generation_rollback_atomically() {
    let (db, context) = fixture();
    let confirm = |value: &ModeBinding| {
        save_mode(
            &db,
            "owner",
            SyncServiceMode::Community,
            value,
            "did:wba:home",
            "{}",
        )
    };
    confirm(&context).unwrap();
    let original = saved(&db);
    for field in ["home", "account", "did", "device", "key", "installation"] {
        let mut invalid = serde_json::to_value(&context).unwrap();
        invalid[field] = Value::String("replacement".into());
        assert!(confirm(&serde_json::from_value(invalid).unwrap()).is_err());
        assert_eq!(saved(&db), original);
    }
    assert!(save_mode(
        &db,
        "owner",
        SyncServiceMode::Commercial,
        &context,
        "did:wba:home",
        "{}"
    )
    .is_err());
    assert!(save_mode(
        &db,
        "owner",
        SyncServiceMode::Community,
        &context,
        "did:wba:other",
        "{}"
    )
    .is_err());
    let mut renewed = context.clone();
    renewed.generation = "2".into();
    confirm(&renewed).unwrap();
    let latest = saved(&db);
    assert!(confirm(&context).is_err());
    assert_eq!(saved(&db), latest);
}

#[test]
fn community_mode_does_not_adopt_existing_commercial_lane_state() {
    let (db, context) = fixture();
    db.execute(
        "INSERT INTO lane_sync_state VALUES ('owner','p6_group','1','4','3')",
        [],
    )
    .unwrap();
    assert!(save_mode(
        &db,
        "owner",
        SyncServiceMode::Community,
        &context,
        "did:wba:home",
        "{}"
    )
    .is_err());
    let count: i64 = db
        .query_row("SELECT count(*) FROM sync_service_modes", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    let position: String = db
        .query_row("SELECT scan_seq FROM lane_sync_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(position, "4");
}

#[test]
fn legacy_community_reads_require_complete_declaration_and_exact_home() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-sync-v1.json"
    )))
    .unwrap();
    let mut raw = fixture["cases"]["C01"]["response"]["result"].clone();
    let config = crate::ImCoreConfig::new(
        crate::ServiceEndpoint::parse("https://home.test").unwrap(),
        "home.test",
    )
    .unwrap();
    raw["service_did"] = serde_json::json!("did:wba:home.test");
    assert!(legacy_reads_declaration(&raw, &config).unwrap());
    raw["service_did"] = serde_json::json!("did:wba:other.test");
    assert!(legacy_reads_declaration(&raw, &config).is_err());
    raw["service_did"] = serde_json::json!("did:wba:home.test");
    raw["features"]["community_sync"]["max_devices"] = serde_json::json!("2");
    assert!(legacy_reads_declaration(&raw, &config).is_err());
    assert!(
        legacy_reads_declaration(&serde_json::json!({"supported_profiles":[]}), &config).is_err()
    );
    assert!(!legacy_reads_declaration(
        &serde_json::json!({"supported_profiles":[
            "awiki.message-sync.explicit-negotiation.v1", "sync.snapshot_paging.v1"
        ]}),
        &config
    )
    .unwrap());
}

#[test]
fn legacy_community_reads_never_adopt_persisted_vnext_owner_state() {
    let (db, _) = fixture();
    require_unbound_legacy_owner(&db, "other-owner").unwrap();
    assert!(require_unbound_legacy_owner(&db, "owner").is_err());
    db.execute(
        "DELETE FROM identity_account_bindings WHERE owner_identity_id='owner'",
        [],
    )
    .unwrap();
    require_unbound_legacy_owner(&db, "owner").unwrap();
    let old = Connection::open_in_memory().unwrap();
    old.execute(
        "CREATE TABLE legacy_messages(owner_did TEXT, body TEXT)",
        [],
    )
    .unwrap();
    require_unbound_legacy_owner(&old, "owner").unwrap();
    let tables: i64 = old
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1);
    // An incomplete historical database can contain a lane marker without the
    // current binding table. The read guard must still reject its exact owner.
    old.execute("CREATE TABLE lane_sync_state(owner_identity_id TEXT)", []).unwrap();
    old.execute("INSERT INTO lane_sync_state VALUES ('owner')", []).unwrap();
    assert!(require_unbound_legacy_owner(&old, "owner").is_err());
    require_unbound_legacy_owner(&old, "other-owner").unwrap();
    let retained: i64 = old.query_row("SELECT COUNT(*) FROM lane_sync_state", [], |row| row.get(0)).unwrap();
    assert_eq!(retained, 1);
}
