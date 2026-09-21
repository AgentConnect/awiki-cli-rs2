use super::*;

fn lookup(subject: &str, did: &str, generation: &str) -> crate::directory::HandleLookupResult {
    crate::directory::HandleLookupResult {
        handle: crate::ids::Handle::parse("peer.remote.test", "").unwrap(),
        did: crate::ids::Did::parse(did).unwrap(),
        user_id: subject.to_owned(),
        domain: Some("remote.test".to_owned()),
        status: Some("active".to_owned()),
        binding_generation: Some(generation.to_owned()),
        profile: None,
        warnings: vec![],
    }
}
fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    db
}
fn project(
    db: &mut Connection,
    owner: &str,
    lookup: &crate::directory::HandleLookupResult,
    home: Option<&str>,
) -> crate::ImResult<String> {
    super::super::peer_personas::project_verified_handle_in_domain(
        db,
        owner,
        "did:wba:home.test:owner",
        lookup,
        home,
    )
}
const DID: &str = "did:wba:remote.test:user:peer";

#[test]
fn foreign_persona_repair_preserves_read_state_aliases_and_other_owner() {
    let mut db = db();
    let old = lookup("private-user-id", DID, "7");
    let new = lookup("peer.remote.test", DID, "7");
    let old_id = project(&mut db, "owner", &old, None).unwrap();
    project(&mut db, "other", &old, None).unwrap();
    db.execute("INSERT INTO thread_read_state(owner_identity_id, thread_scope, thread_id, conversation_id, read_watermark_seq, pending_remote_ack, updated_at) VALUES('owner','direct',?1,?1,'19',1,'t')", [&old_id]).unwrap();
    let message = super::super::messages::MessageRecord {
        msg_id: "ciphertext-message".to_owned(),
        owner_identity_id: "owner".to_owned(),
        owner_did: "did:wba:home.test:owner".to_owned(),
        conversation_id: old_id.clone(),
        thread_id: old_id.clone(),
        sender_did: DID.to_owned(),
        receiver_did: "did:wba:home.test:owner".to_owned(),
        content: "historical ciphertext bytes".to_owned(),
        is_e2ee: true,
        is_read: true,
        metadata: r#"{"opaque":"preserve exactly"}"#.to_owned(),
        sent_at: "2026-09-21T00:00:00Z".to_owned(),
        stored_at: "2026-09-21T00:00:00Z".to_owned(),
        ..Default::default()
    }
    .with_resolved_wire_thread("direct", DID);
    super::super::messages::upsert_message(&db, &message).unwrap();
    let next_id = project(&mut db, "owner", &new, Some("home.test")).unwrap();
    assert_ne!(old_id, next_id);
    let bytes: (String,String,String,bool) = db.query_row("SELECT content, metadata, wire_thread_ref, is_read FROM messages WHERE msg_id='ciphertext-message'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        bytes,
        (
            message.content.clone(),
            message.metadata.clone(),
            DID.to_owned(),
            true
        )
    );
    assert_eq!(
        super::super::direct_peer_routes::get(&db, "owner", &old_id)
            .unwrap()
            .unwrap()
            .conversation_id,
        next_id
    );
    super::super::conversation_registry::ensure_validated(
        &db,
        "owner",
        "did:wba:home.test:owner",
        &old_id,
    )
    .unwrap();

    assert_eq!(
        project(&mut db, "owner", &new, Some("home.test")).unwrap(),
        next_id
    );
    let state: (String, String, String, bool) = db.query_row("SELECT conversation_id, thread_id, read_watermark_seq, pending_remote_ack FROM thread_read_state WHERE owner_identity_id='owner'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        state,
        (next_id.clone(), next_id.clone(), "19".to_owned(), true)
    );
    assert_eq!(
        super::super::conversation_aliases::resolve(
            &db,
            "owner",
            "verified_foreign_persona",
            &old_id
        )
        .unwrap(),
        Some(next_id.clone())
    );
    assert_eq!(
        super::super::peer_personas::resolve_by_did(&db, "other", DID)
            .unwrap()
            .unwrap()
            .conversation_id,
        old_id
    );
    assert!(super::super::canonical_invariants::check(&db, "owner")
        .unwrap()
        .is_empty());
}

#[test]
fn foreign_persona_repair_requires_continuity_and_rolls_back_all_writes() {
    let mut db = db();
    let old = lookup("private-user-id", DID, "7");
    let old_id = project(&mut db, "owner", &old, None).unwrap();
    let new = lookup("peer.remote.test", "did:wba:remote.test:user:new", "8");
    assert!(matches!(
        project(&mut db, "owner", &new, Some("home.test")),
        Err(crate::ImError::IdentityBindingConflict { .. })
    ));
    assert_eq!(
        super::super::peer_personas::resolve_by_did(&db, "owner", DID)
            .unwrap()
            .unwrap()
            .conversation_id,
        old_id
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM peer_personas", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    super::super::did_transition_edges::compare_and_set_verified(
        &db,
        "owner",
        &super::super::did_transition_edges::VerifiedDidTransitionEdge {
            predecessor_did: DID.to_owned(),
            successor_did: new.did.as_str().to_owned(),
            assurance: anp::authentication::TransitionAssurance::Verified,
        },
    )
    .unwrap();
    let next_id = project(&mut db, "owner", &new, Some("home.test")).unwrap();
    assert_ne!(next_id, old_id);
}

#[test]
fn foreign_persona_repair_never_changes_same_domain_or_unknown_source() {
    for (home, corrupt) in [("remote.test", false), ("home.test", true)] {
        let mut db = db();
        let old = lookup("private-user-id", DID, "7");
        let old_id = project(&mut db, "owner", &old, None).unwrap();
        if corrupt {
            db.execute("UPDATE peer_personas SET source='unverified_import'", [])
                .unwrap();
        }
        let new = lookup("peer.remote.test", DID, "7");
        assert!(project(&mut db, "owner", &new, Some(home)).is_err());
        assert_eq!(
            super::super::peer_personas::resolve_by_did(&db, "owner", DID)
                .unwrap()
                .unwrap()
                .conversation_id,
            old_id
        );
    }
}

#[test]
fn foreign_persona_repair_transaction_rolls_back_on_late_write_failure() {
    let mut db = db();
    let old = lookup("private-user-id", DID, "7");
    let old_id = project(&mut db, "owner", &old, None).unwrap();
    db.execute_batch("CREATE TRIGGER reject_repair BEFORE DELETE ON direct_peer_routes BEGIN SELECT RAISE(ABORT,'test interruption'); END;").unwrap();
    assert!(project(
        &mut db,
        "owner",
        &lookup("peer.remote.test", DID, "7"),
        Some("home.test")
    )
    .is_err());
    assert_eq!(
        super::super::peer_personas::resolve_by_did(&db, "owner", DID)
            .unwrap()
            .unwrap()
            .conversation_id,
        old_id
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM peer_personas", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM conversation_aliases WHERE alias_kind='verified_foreign_persona'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn foreign_persona_repair_rejects_cross_owner_proofs_regression_and_independent_target() {
    for mode in [
        "cross-owner-proof",
        "regression",
        "independent-target",
        "corrupt-identity",
    ] {
        let mut db = db();
        let old = lookup("private-user-id", DID, "7");
        project(&mut db, "owner", &old, None).unwrap();
        let mut new = lookup("peer.remote.test", DID, "7");
        match mode {
            "cross-owner-proof" => {
                new.did = crate::ids::Did::parse("did:wba:remote.test:user:new").unwrap();
                new.binding_generation = Some("8".to_owned());
                super::super::did_transition_edges::compare_and_set_verified(
                    &db,
                    "other",
                    &super::super::did_transition_edges::VerifiedDidTransitionEdge {
                        predecessor_did: DID.to_owned(),
                        successor_did: new.did.as_str().to_owned(),
                        assurance: anp::authentication::TransitionAssurance::Verified,
                    },
                )
                .unwrap();
            }
            "regression" => new.binding_generation = Some("6".to_owned()),
            "independent-target" => {
                let p = new.peer_persona().unwrap();
                super::super::peer_personas::upsert(
                    &db,
                    &super::super::peer_personas::PeerPersonaRecord {
                        owner_identity_id: "owner".to_owned(),
                        persona: p,
                        binding_generation: Some("7".to_owned()),
                        subject_type: "human".to_owned(),
                        source: "handle_authority".to_owned(),
                        authority_revision: None,
                        verified_at: "t".to_owned(),
                    },
                )
                .unwrap();
            }
            _ => {
                db.execute(
                    "UPDATE peer_personas SET authority_subject_id='tampered'",
                    [],
                )
                .unwrap();
            }
        }
        assert!(
            matches!(
                project(&mut db, "owner", &new, Some("home.test")),
                Err(crate::ImError::IdentityBindingConflict { .. })
            ),
            "{mode}"
        );
        assert_eq!(db.query_row("SELECT COUNT(*) FROM conversation_aliases WHERE alias_kind='verified_foreign_persona'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}

#[test]
fn foreign_persona_repair_survives_reopen_and_late_old_conversation_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite");
    let mut db = Connection::open(&path).unwrap();
    super::super::schema::ensure_schema(&db).unwrap();
    let old_id = project(&mut db, "owner", &lookup("private-user-id", DID, "7"), None).unwrap();
    let new_id = project(
        &mut db,
        "owner",
        &lookup("peer.remote.test", DID, "7"),
        Some("home.test"),
    )
    .unwrap();
    drop(db);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        super::super::direct_peer_routes::get(&db, "owner", &old_id)
            .unwrap()
            .unwrap()
            .conversation_id,
        new_id
    );
    let message = super::super::messages::MessageRecord {
        msg_id: "late-old-id".to_owned(),
        owner_identity_id: "owner".to_owned(),
        owner_did: "did:wba:home.test:owner".to_owned(),
        conversation_id: old_id.clone(),
        thread_id: old_id.clone(),
        sender_did: DID.to_owned(),
        receiver_did: "did:wba:home.test:owner".to_owned(),
        content: "late reply".to_owned(),
        sent_at: "2026-09-21T00:00:00Z".to_owned(),
        stored_at: "2026-09-21T00:00:00Z".to_owned(),
        ..Default::default()
    }
    .with_resolved_wire_thread("direct", DID);
    super::super::messages::upsert_message(&db, &message).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT conversation_id FROM messages WHERE msg_id='late-old-id'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        new_id
    );
    let old_ref = crate::messages::ThreadRef::Thread(crate::ids::ThreadId::parse(&old_id).unwrap());
    let history = super::super::messages::list_messages_for_thread_ref_for_owner_identity(
        &db,
        "owner",
        "did:wba:home.test:owner",
        &old_ref,
        10,
        None,
    )
    .unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].content, "late reply");
    assert!(super::super::canonical_upgrade::list_alias_mappings(&path)
        .unwrap()
        .iter()
        .any(|alias| alias.legacy_conversation_id == old_id
            && alias.canonical_conversation_id == new_id));
}
