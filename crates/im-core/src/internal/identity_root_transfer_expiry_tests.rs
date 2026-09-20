use super::*;

#[test]
fn historical_delivery_preserves_acceptance_based_recovery_without_rewriting_ledger() {
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

#[test]
fn durable_delivery_checkpoint_detects_supersession_without_rewriting_accepted_message() {
    use crate::internal::identity_device_state::IdentityInternalCheckpoint;
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE identity_root_transfer_sender_v1 (
        owner_identity_id TEXT, owner_did TEXT, local_device_id TEXT, message_id TEXT,
        recipient_device_id TEXT, phase TEXT, created_at TEXT, updated_at TEXT, accepted_at TEXT,
        PRIMARY KEY(owner_identity_id,local_device_id,message_id));",
    )
    .unwrap();
    ensure_sender_envelope_format_column_with_connection(&db).unwrap();
    ensure_sender_envelope_format_column_with_connection(&db).unwrap();
    let original = IdentityInternalCheckpoint {
        document_version: 4,
        registry_version: 8,
        document_hash: "signed-document-hash".into(),
    };
    let tx = db.transaction().unwrap();
    persist_sender_delivery_pending_tx(
        &tx,
        "owner",
        "did",
        "sender",
        "root-message",
        "recipient",
        "2026-09-17T00:00:00Z",
        &original,
    )
    .unwrap();
    tx.commit().unwrap();
    let contract: String = db
        .query_row(
            "SELECT completion_contract FROM identity_root_transfer_sender_v1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(contract, "v1");
    let at =
        |s: &str| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap();
    db.execute("UPDATE identity_root_transfer_sender_v1 SET phase='sent',accepted_at='2026-09-17T00:00:01Z'", []).unwrap();
    assert!(!delivery_expired_with_connection(
        &db,
        "owner",
        "did",
        "sender",
        "recipient",
        at("2026-09-17T00:10:00Z")
    )
    .unwrap());
    assert!(delivery_expired_with_connection(
        &db,
        "owner",
        "did",
        "sender",
        "recipient",
        at("2026-09-17T00:10:00.001Z")
    )
    .unwrap());
    assert!(
        !delivery_checkpoint_changed(&db, "owner", "did", "sender", "recipient", &original)
            .unwrap()
    );
    for changed in [
        IdentityInternalCheckpoint {
            registry_version: 9,
            ..original.clone()
        },
        IdentityInternalCheckpoint {
            document_version: 5,
            document_hash: "new-signed-document".into(),
            ..original.clone()
        },
        IdentityInternalCheckpoint {
            document_hash: "conflicting-hash".into(),
            ..original.clone()
        },
    ] {
        assert!(
            delivery_checkpoint_changed(&db, "owner", "did", "sender", "recipient", &changed)
                .unwrap()
        );
        assert!(
            !delivery_checkpoint_changed(&db, "other", "did", "sender", "recipient", &changed)
                .unwrap()
        );
        let tx = db.transaction().unwrap();
        assert!(persist_sender_delivery_pending_tx(
            &tx,
            "owner",
            "did",
            "sender",
            "root-message",
            "recipient",
            "2026-09-17T00:00:00Z",
            &changed
        )
        .is_err());
        tx.rollback().unwrap();
    }
    let saved: (String,String,String) = db.query_row("SELECT phase,accepted_at,transfer_checkpoint_json FROM identity_root_transfer_sender_v1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(
        saved,
        (
            "sent".into(),
            "2026-09-17T00:00:01Z".into(),
            serde_json::to_string(&original).unwrap()
        )
    );
    db.execute(
        "UPDATE identity_root_transfer_sender_v1 SET transfer_checkpoint_json=NULL",
        [],
    )
    .unwrap();
    assert!(
        !delivery_checkpoint_changed(&db, "owner", "did", "sender", "recipient", &original)
            .unwrap()
    );
    db.execute(
        "UPDATE identity_root_transfer_sender_v1 SET transfer_checkpoint_json='invalid'",
        [],
    )
    .unwrap();
    assert!(
        delivery_checkpoint_changed(&db, "owner", "did", "sender", "recipient", &original).is_err()
    );
}

#[test]
fn sender_emits_the_closed_v1_envelope_without_a_completion_extension() {
    // An old strict V1 reader rejects added fields, even when system_type is V1.
    let original = serde_json::json!({
        "system_type": ROOT_KEY_ENVELOPE_V1,
        "message_id": "msg-root-key-compat", "did": "did:example:owner",
        "root_key_id": "root", "root_public_key_fingerprint": "fingerprint",
        "root_private_key_pkcs8_b64u": "test-only",
        "sender_device_id": "sender", "sender_e2ee_key_id": "sender-e2ee",
        "recipient_device_id": "recipient", "recipient_e2ee_key_id": "recipient-e2ee",
        "document_version": 1, "document_hash": "hash", "registry_version": 2,
        "issued_at": "2026-09-18T00:00:00Z", "expires_at": "2026-09-18T00:10:00Z"
    });
    let envelope: RootKeyEnvelopeV1 = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(&envelope).unwrap(), original);
    let mut extended = original;
    extended["completion_contract"] = serde_json::json!("awiki.device.root-key-import-complete.v2");
    assert!(serde_json::from_value::<RootKeyEnvelopeV1>(extended).is_err());
}
