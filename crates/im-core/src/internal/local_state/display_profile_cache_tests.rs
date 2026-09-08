use super::*;

fn fixture() -> (Connection, Did) {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    (db, Did::parse("did:example:guest").unwrap())
}

fn profile(did: &Did, name: Option<&str>) -> Profile {
    serde_json::from_value(serde_json::json!({
        "subject": did.as_str(), "handle": "guest.example", "display_name": name,
        "tags": [], "metadata": [], "ttl": 60,
    }))
    .unwrap()
}

#[test]
fn display_cache_merges_requests_retries_failures_and_preserves_owner_isolation() {
    let (db, did) = fixture();
    let lease = claim(&db, "alice", &did, false, 100).unwrap().unwrap();
    assert!(claim(&db, "alice", &did, true, 101).unwrap().is_none());
    finish(
        &db,
        "alice",
        &did,
        &lease,
        Some(profile(&did, Some("Guest A"))),
        101,
    )
    .unwrap();
    assert!(claim(&db, "alice", &did, false, 110).unwrap().is_none());
    assert!(read(&db, "bob", &did, 110).unwrap().is_none());
    let lease = claim(&db, "alice", &did, false, 162).unwrap().unwrap();
    finish(&db, "alice", &did, &lease, None, 163).unwrap();
    let stale = read(&db, "alice", &did, 164).unwrap().unwrap();
    assert_eq!(stale.display_name.as_deref(), Some("Guest A"));
    assert!(stale.is_stale);
    assert!(claim(&db, "alice", &did, false, 164).unwrap().is_none());
    assert!(claim(&db, "alice", &did, false, 168).unwrap().is_some());
}

#[test]
fn display_cache_rejects_old_lease_and_does_not_resurrect_deleted_owner() {
    let (db, did) = fixture();
    let old = claim(&db, "alice", &did, false, 100).unwrap().unwrap();
    let new = claim(&db, "alice", &did, false, 131).unwrap().unwrap();
    finish(
        &db,
        "alice",
        &did,
        &new,
        Some(profile(&did, Some("New"))),
        132,
    )
    .unwrap();
    finish(
        &db,
        "alice",
        &did,
        &old,
        Some(profile(&did, Some("Old"))),
        133,
    )
    .unwrap();
    assert_eq!(
        read(&db, "alice", &did, 134)
            .unwrap()
            .unwrap()
            .display_name
            .as_deref(),
        Some("New")
    );
    db.execute(
        "DELETE FROM display_profile_cache WHERE owner_identity_id='alice'",
        [],
    )
    .unwrap();
    finish(
        &db,
        "alice",
        &did,
        &new,
        Some(profile(&did, Some("Late"))),
        135,
    )
    .unwrap();
    assert!(read(&db, "alice", &did, 136).unwrap().is_none());
    let recreated = claim(&db, "alice", &did, false, 137).unwrap().unwrap();
    finish(
        &db,
        "alice",
        &did,
        &old,
        Some(profile(&did, Some("Before deletion"))),
        138,
    )
    .unwrap();
    assert!(read(&db, "alice", &did, 139).unwrap().is_none());
    finish(
        &db,
        "alice",
        &did,
        &recreated,
        Some(profile(&did, Some("Recreated"))),
        139,
    )
    .unwrap();
    assert_eq!(
        read(&db, "alice", &did, 140)
            .unwrap()
            .unwrap()
            .display_name
            .as_deref(),
        Some("Recreated")
    );
}

#[test]
fn successful_empty_name_is_cached_and_clears_previous_name() {
    let (db, did) = fixture();
    let first = claim(&db, "alice", &did, false, 100).unwrap().unwrap();
    finish(
        &db,
        "alice",
        &did,
        &first,
        Some(profile(&did, Some("Old"))),
        101,
    )
    .unwrap();
    let next = claim(&db, "alice", &did, true, 102).unwrap().unwrap();
    finish(&db, "alice", &did, &next, Some(profile(&did, None)), 103).unwrap();
    let empty = read(&db, "alice", &did, 104).unwrap().unwrap();
    assert_eq!(empty.display_name, None);
    assert!(empty.cache_hit && !empty.is_stale);
    assert!(claim(&db, "alice", &did, false, 104).unwrap().is_none());
}

#[test]
fn schema_41_upgrade_adds_only_disposable_display_state() {
    let (db, _) = fixture();
    db.execute_batch("DROP TABLE display_profile_cache; PRAGMA user_version=41;")
        .unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        43
    );
    assert!(claim(
        &db,
        "alice",
        &Did::parse("did:example:guest").unwrap(),
        false,
        100
    )
    .unwrap()
    .is_some());
    for table in [
        "contacts",
        "conversation_registry",
        "peer_personas",
        "direct_peer_routes",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn existing_persona_wins_over_display_only_cache_and_newer_explicit_refresh() {
    let (db, did) = fixture();
    let lease = claim(&db, "alice", &did, false, 100).unwrap().unwrap();
    db.execute_batch("INSERT INTO peer_personas(owner_identity_id,peer_persona_id,authority_namespace,authority_subject_id,full_handle,source,verified_at,created_at,updated_at) VALUES ('alice','persona','example','guest','guest.example','verified','100','100','100');
    INSERT INTO peer_identifiers(owner_identity_id,peer_persona_id,identifier_kind,identifier_value,source,verified_at,first_seen_at,last_seen_at) VALUES ('alice','persona','did','did:example:guest','verified','100','100','100');").unwrap();
    super::super::peer_profiles::refresh_existing_from_public_profile(
        &db,
        "alice",
        &did,
        &profile(&did, Some("Verified")),
    )
    .unwrap();
    finish(
        &db,
        "alice",
        &did,
        &lease,
        Some(profile(&did, Some("Late unknown"))),
        102,
    )
    .unwrap();
    assert!(read(&db, "alice", &did, 103).unwrap().is_none());
    let lease = claim(&db, "alice", &did, true, 131).unwrap().unwrap();
    super::super::peer_profiles::refresh_existing_from_public_profile(
        &db,
        "alice",
        &did,
        &profile(&did, Some("New explicit")),
    )
    .unwrap();
    finish(
        &db,
        "alice",
        &did,
        &lease,
        Some(profile(&did, Some("Late stale"))),
        132,
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT display_name FROM peer_profiles WHERE owner_identity_id='alice'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "New explicit"
    );
    let lease = claim(&db, "alice", &did, true, 162).unwrap().unwrap();
    finish(&db, "alice", &did, &lease, Some(profile(&did, None)), 163).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT display_name FROM peer_profiles WHERE owner_identity_id='alice'",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    assert!(read(&db, "alice", &did, 164).unwrap().is_none());
    for table in ["contacts", "direct_peer_routes", "conversation_registry"] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn schema_41_and_42_upgrade_preserves_recovery_rows_and_terminal_constraint() {
    for version in [41, 42] {
        let (db, _) = fixture();
        db.execute_batch(
            "DROP TABLE display_profile_cache; DROP TABLE identity_transition_pending;",
        )
        .unwrap();
        let sql = crate::internal::identity_transition_pending::IDENTITY_TRANSITION_SQL;
        db.execute_batch(&if version == 41 {
            sql.replace(",'superseded'", "")
        } else {
            sql.to_owned()
        })
        .unwrap();
        db.execute_batch("INSERT INTO identity_transition_pending
            (recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,
            account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at)
            VALUES ('recovery',1,'v1','hash','initiator','operation','root','account','owner','guest.example',
            'did:example:old','did:example:new','1','pending','100','100');").unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        assert_eq!(
            crate::internal::local_state::schema::current_schema_version(&db).unwrap(),
            43
        );
        assert_eq!(
            db.query_row(
                "SELECT source_id FROM identity_transition_pending WHERE recovery_id='recovery'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "operation"
        );
        db.execute("UPDATE identity_transition_pending SET phase='superseded' WHERE recovery_id='recovery'", []).unwrap();
        assert!(claim(
            &db,
            "owner",
            &Did::parse("did:example:guest").unwrap(),
            false,
            100
        )
        .unwrap()
        .is_some());
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    }
}

#[test]
fn schema_42_rejects_missing_terminal_constraint_without_advancing_version() {
    let (db, _) = fixture();
    db.execute_batch("DROP TABLE display_profile_cache; DROP TABLE identity_transition_pending;")
        .unwrap();
    db.execute_batch(
        &crate::internal::identity_transition_pending::IDENTITY_TRANSITION_SQL
            .replace(",'superseded'", ""),
    )
    .unwrap();
    db.pragma_update(None, "user_version", 42).unwrap();
    assert!(crate::internal::local_state::schema::ensure_schema(&db).is_err());
    assert_eq!(
        crate::internal::local_state::schema::current_schema_version(&db).unwrap(),
        42
    );
}

#[test]
fn schema_43_rejects_missing_cache_or_terminal_shape() {
    for table in ["display_profile_cache", "identity_transition_pending"] {
        let (db, _) = fixture();
        db.execute_batch(&format!("DROP TABLE {table}")).unwrap();
        assert!(crate::internal::local_state::schema::ensure_schema(&db).is_err());
        assert_eq!(
            crate::internal::local_state::schema::current_schema_version(&db).unwrap(),
            43
        );
    }
}
