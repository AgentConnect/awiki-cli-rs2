use super::*;

fn edge(predecessor: &str, successor: &str) -> VerifiedDidTransitionEdge {
    VerifiedDidTransitionEdge {
        predecessor_did: predecessor.to_owned(),
        successor_did: successor.to_owned(),
        assurance: TransitionAssurance::Verified,
    }
}

#[test]
fn schema_37_verified_edge_cache_is_owner_scoped_and_cas() {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    let old = "did:wba:example.com:users:alice:e1_old";
    let new = "did:wba:example.com:users:alice:e1_new";

    compare_and_set_verified(&db, "owner-a", &edge(old, new)).unwrap();
    compare_and_set_verified(&db, "owner-a", &edge(old, new)).unwrap();
    assert_eq!(
        get_successor(&db, "owner-a", old).unwrap().as_deref(),
        Some(new)
    );
    assert_eq!(get_successor(&db, "owner-b", old).unwrap(), None);

    let conflict = compare_and_set_verified(
        &db,
        "owner-a",
        &edge(old, "did:wba:example.com:users:alice:e1_other"),
    )
    .unwrap_err();
    assert!(matches!(
        conflict,
        crate::ImError::IdentityBindingConflict { .. }
    ));
}

#[test]
fn schema_37_rejects_weak_edges_without_touching_other_projections() {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    let before: i64 = db
        .query_row("SELECT COUNT(*) FROM direct_peer_routes", [], |row| {
            row.get(0)
        })
        .unwrap();
    let weak = VerifiedDidTransitionEdge {
        predecessor_did: "did:wba:example.com:users:alice:e1_old".to_owned(),
        successor_did: "did:wba:example.com:users:alice:e1_new".to_owned(),
        assurance: TransitionAssurance::Unverified,
    };

    assert!(compare_and_set_verified(&db, "owner-a", &weak).is_err());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM did_transition_edges", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM direct_peer_routes", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        before
    );
}
