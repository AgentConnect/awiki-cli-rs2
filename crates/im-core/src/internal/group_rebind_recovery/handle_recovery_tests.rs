use super::{handle_recovery_operation_id, is_handle_recovery_operation_id};

#[test]
fn handle_recovery_operation_id_matches_frozen_vectors() {
    let vectors = [
        (
            "alice.awiki.info",
            "did:wba:awiki.info:users:alice-old",
            "did:wba:awiki.info:users:alice-new",
            "8",
            "did:wba:awiki.info:groups:engineering",
            "op-rebind-v1-955013ec7adf5d7c0ce680e33d459bef9d1b3461b722e8164fe0cc8f0d6a45d9",
        ),
        (
            "bob.chat.example",
            "did:wba:chat.example:users:bob-v1",
            "did:wba:chat.example:users:bob-v2",
            "42",
            "did:wba:chat.example:groups:ops",
            "op-rebind-v1-97b78405d8d50c2ed956c50098b9c238cbb464e0e28120c100e622d3c153ecb6",
        ),
    ];

    for (handle, previous_did, current_did, generation, group_did, expected) in vectors {
        assert_eq!(
            handle_recovery_operation_id(handle, previous_did, current_did, generation, group_did,)
                .unwrap(),
            expected,
        );
    }
}

#[test]
fn handle_recovery_operation_id_rejects_noncanonical_inputs() {
    assert!(handle_recovery_operation_id(
        "Alice.awiki.info",
        "did:wba:awiki.info:users:alice-old",
        "did:wba:awiki.info:users:alice-new",
        "8",
        "did:wba:awiki.info:groups:engineering",
    )
    .is_err());
    assert!(handle_recovery_operation_id(
        "alice.awiki.info",
        "did:wba:awiki.info:users:alice-old",
        "did:wba:awiki.info:users:alice-new",
        "08",
        "did:wba:awiki.info:groups:engineering",
    )
    .is_err());
}

#[test]
fn handle_recovery_operation_id_classifier_is_closed() {
    let valid = handle_recovery_operation_id(
        "alice.awiki.info",
        "did:wba:awiki.info:users:alice-old",
        "did:wba:awiki.info:users:alice-new",
        "8",
        "did:wba:awiki.info:groups:engineering",
    )
    .unwrap();
    assert!(is_handle_recovery_operation_id(&valid));
    assert!(!is_handle_recovery_operation_id("op-rebind-v1-short"));
    assert!(!is_handle_recovery_operation_id(
        "op-rebind-v1-955013EC7ADF5D7C0CE680E33D459BEF9D1B3461B722E8164FE0CC8F0D6A45D9"
    ));
    assert!(!is_handle_recovery_operation_id(
        "legacy-rebind-955013ec7adf5d7c0ce680e33d459bef9d1b3461b722e8164fe0cc8f0d6a45d9"
    ));
}

#[test]
fn recovery_transport_eligibility_is_exact_and_fail_closed() {
    assert!(super::exact_handle_recovery_transport_profile(
        r#"{"required_security_profile":"transport-protected"}"#
    ));
    assert!(!super::exact_handle_recovery_transport_profile(
        r#"{"required_security_profile":"group-e2ee"}"#
    ));
    assert!(!super::exact_handle_recovery_transport_profile(
        r#"{"message_security_profile":"transport-protected"}"#
    ));
    assert!(!super::exact_handle_recovery_transport_profile(
        r#"{"required_security_profile":"transport-protected","message_security_profile":"group-e2ee"}"#
    ));
    assert!(!super::exact_handle_recovery_transport_profile("not-json"));
}

#[test]
fn recovery_job_counts_ignore_legacy_jobs_and_completed_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite3");
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
    let operation_id = handle_recovery_operation_id(
        "alice.awiki.info",
        "did:wba:awiki.info:users:alice-old",
        "did:wba:awiki.info:users:alice-new",
        "8",
        "did:wba:awiki.info:groups:engineering",
    )
    .unwrap();
    for (job_id, group_did, phase) in [
        (
            operation_id.as_str(),
            "did:wba:awiki.info:groups:engineering",
            "pending",
        ),
        ("legacy-job", "did:wba:awiki.info:groups:legacy", "blocked"),
    ] {
        connection
            .execute(
                r#"INSERT INTO group_rebind_outbox
(job_id, owner_identity_id, group_did, member_handle, previous_member_did,
 new_member_did, binding_generation, phase, created_at, updated_at)
VALUES (?1,'owner',?2,'alice.awiki.info','did:wba:awiki.info:users:alice-old',
        'did:wba:awiki.info:users:alice-new','8',?3,'now','now')"#,
                rusqlite::params![job_id, group_did, phase],
            )
            .unwrap();
    }
    drop(connection);

    assert_eq!(
        super::handle_recovery_job_counts(
            &path,
            "owner",
            "alice.awiki.info",
            "did:wba:awiki.info:users:alice-old",
            "did:wba:awiki.info:users:alice-new",
            "8",
        )
        .unwrap(),
        (1, 0),
    );

    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    connection
        .execute(
            "UPDATE group_rebind_outbox SET phase='complete' WHERE job_id=?1",
            [&operation_id],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        super::handle_recovery_job_counts(
            &path,
            "owner",
            "alice.awiki.info",
            "did:wba:awiki.info:users:alice-old",
            "did:wba:awiki.info:users:alice-new",
            "8",
        )
        .unwrap(),
        (0, 0),
    );
}

#[test]
fn recovery_impact_counts_known_e2ee_and_did_only_groups() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("impact.sqlite3");
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
    for (group, metadata, anchor_kind) in [
        (
            "did:wba:awiki.info:groups:e2ee",
            r#"{"required_security_profile":"group-e2ee"}"#,
            "handle",
        ),
        (
            "did:wba:awiki.info:groups:did-only",
            r#"{"required_security_profile":"transport-protected"}"#,
            "did",
        ),
    ] {
        connection
            .execute(
                "INSERT INTO groups(owner_identity_id,owner_did,group_id,group_did,stored_at,metadata) VALUES ('owner','did:owner',?1,?1,'now',?2)",
                rusqlite::params![group, metadata],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO group_members(owner_identity_id,owner_did,group_id,user_id,member_did,anchor_kind,anchor_value,last_synced_at) VALUES ('owner','did:owner',?1,?1,'did:wba:awiki.info:users:alice-old',?2,'alice.awiki.info','now')",
                rusqlite::params![group, anchor_kind],
            )
            .unwrap();
    }
    drop(connection);

    assert_eq!(
        super::recovery_impact_counts(&path, "owner", "did:wba:awiki.info:users:alice-old",)
            .unwrap(),
        (1, 1),
    );
}
