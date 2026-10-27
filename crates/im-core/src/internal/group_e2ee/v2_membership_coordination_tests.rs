use super::*;
fn runtime(root: &std::path::Path, owner: &str) -> GroupE2eeV2Runtime {
    GroupE2eeV2Runtime::new(
        ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
            root.join("im.sqlite"),
            owner,
            "did:example:owner",
            "device",
        )
        .unwrap(),
    )
    .with_control_inbox(root.join("im.sqlite"))
}
#[test]
fn group_membership_coordination_excludes_independent_handles_and_releases_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let first = runtime(root.path(), "owner");
    let second = runtime(root.path(), "owner");
    let guard = first.membership_guard().unwrap();
    assert!(second.membership_guard().is_err());
    let independent = runtime(root.path(), "other-owner")
        .membership_guard()
        .unwrap();
    drop(independent);
    drop(guard);
    assert!(second.membership_guard().is_ok());
}
#[test]
fn group_membership_prepare_waits_for_its_unclosed_control_inputs_only() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path(), "owner");
    let db = rusqlite::Connection::open(root.path().join("im.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE sync_lane_inbox(owner_identity_id TEXT,group_did TEXT,event_type TEXT,closed_at INTEGER);").unwrap();
    for (owner, group, kind, closed) in [
        ("other", "g", "p6.control.notice", None),
        ("owner", "other-group", "p6.control.notice", None),
        ("owner", "g", "p6.delivery.created", None),
        ("owner", "g", "p6.control.notice", Some(1)),
    ] {
        db.execute(
            "INSERT INTO sync_lane_inbox VALUES (?1,?2,?3,?4)",
            (owner, group, kind, closed),
        )
        .unwrap();
    }
    runtime.require_control_inbox_drained("g").unwrap();
    db.execute(
        "INSERT INTO sync_lane_inbox VALUES ('owner','g','p6.control.notice',NULL)",
        [],
    )
    .unwrap();
    assert!(runtime.require_control_inbox_drained("g").is_err());
    db.execute("UPDATE sync_lane_inbox SET closed_at=1", [])
        .unwrap();
    runtime.require_control_inbox_drained("g").unwrap();
}
