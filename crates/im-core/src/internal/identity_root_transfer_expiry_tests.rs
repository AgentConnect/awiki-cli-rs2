use super::*;

#[test]
fn expiry_uses_service_acceptance_not_receiver_or_wall_clock_and_preserves_ledger() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE identity_root_transfer_sender_v1 (
        owner_identity_id TEXT, owner_did TEXT, local_device_id TEXT,
        recipient_device_id TEXT, phase TEXT, created_at TEXT, accepted_at TEXT);
        INSERT INTO identity_root_transfer_sender_v1 VALUES
        ('owner','did','sender','recipient','pending_delivery','2026-09-16T00:00:00Z',NULL);",
    )
    .unwrap();
    let at =
        |s: &str| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap();
    let expired = |now| {
        delivery_expired_with_connection(&db, "owner", "did", "sender", "recipient", now).unwrap()
    };
    assert!(!expired(at("2026-09-16T00:10:00Z")));
    assert!(expired(at("2026-09-16T00:10:00.001Z")));
    assert!(!delivery_expired_with_connection(
        &db,
        "other",
        "did",
        "sender",
        "recipient",
        at("2026-09-17T00:00:00Z")
    )
    .unwrap());
    db.execute("UPDATE identity_root_transfer_sender_v1 SET phase='sent', accepted_at='2026-09-16T00:10:00Z'", []).unwrap();
    assert!(!expired(at("2027-09-16T00:00:00Z")));
    db.execute(
        "UPDATE identity_root_transfer_sender_v1 SET accepted_at='2026-09-16T00:10:00.001Z'",
        [],
    )
    .unwrap();
    assert!(expired(at("2026-09-16T00:11:00Z")));
    let phase: String = db
        .query_row(
            "SELECT phase FROM identity_root_transfer_sender_v1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(phase, "sent"); // Expiry must not erase acceptance or enable a fresh message.
    db.execute(
        "UPDATE identity_root_transfer_sender_v1 SET accepted_at=NULL",
        [],
    )
    .unwrap();
    assert!(delivery_expired_with_connection(
        &db,
        "owner",
        "did",
        "sender",
        "recipient",
        at("2026-09-16T00:11:00Z")
    )
    .is_err());
}
