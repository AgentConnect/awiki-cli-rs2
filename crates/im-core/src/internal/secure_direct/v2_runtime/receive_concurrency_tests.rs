use super::tests::{established_pair, scope, vault};
use super::*;
use crate::internal::secure_direct::v2_store::SqliteV2DirectStateStore;

#[test]
fn valid_cipher_losing_a_session_cas_remains_retryable_and_commits_exactly_once() {
    let root = tempfile::tempdir().unwrap();
    let sender_db =
        crate::internal::local_state::open_writable(&root.path().join("sender.sqlite")).unwrap();
    let receiver_path = root.path().join("receiver.sqlite");
    let receiver_db = crate::internal::local_state::open_writable(&receiver_path).unwrap();
    receiver_db
        .execute_batch("CREATE TABLE projected(message_id TEXT PRIMARY KEY);")
        .unwrap();
    let other_db = crate::internal::local_state::open_writable(&receiver_path).unwrap();
    let (sender_state, receiver_state) = established_pair();
    let sender_store = SqliteV2DirectStateStore::new_with_secret_vault(
        &sender_db,
        vault(&root.path().join("sender-vault"), 91),
        scope(
            "sender",
            "did:example:alice",
            "alice-phone",
            "did:example:alice#phone-e2ee",
        ),
    )
    .unwrap();
    let receiver_scope = scope(
        "receiver",
        "did:example:alice",
        "alice-laptop",
        "did:example:alice#laptop-e2ee",
    );
    let receiver_vault = vault(&root.path().join("receiver-vault"), 92);
    let receiver_store = SqliteV2DirectStateStore::new_with_secret_vault(
        &receiver_db,
        receiver_vault.clone(),
        receiver_scope.clone(),
    )
    .unwrap();
    let other_store =
        SqliteV2DirectStateStore::new_with_secret_vault(&other_db, receiver_vault, receiver_scope)
            .unwrap();
    for (store, state) in [
        (&sender_store, &sender_state),
        (&receiver_store, &receiver_state),
    ] {
        store
            .commit_inbound(
                state,
                "setup",
                "sha256:setup",
                None,
                V2SessionExpectation::Absent,
                "2026-09-11T00:00:00Z",
            )
            .unwrap();
    }
    let sender = V2EstablishedDirectRuntime::new(&sender_store);
    let receiver = V2EstablishedDirectRuntime::new(&receiver_store);
    let other = V2EstablishedDirectRuntime::new(&other_store);
    let plaintext = |id: &str| V2ApplicationPlaintext {
        application_content_type: "text/plain".to_owned(),
        logical_message_id: Some(id.to_owned()),
        conversation_id: None,
        reply_to_message_id: None,
        annotations: None,
        text: Some("valid reply".to_owned()),
        payload: None,
        payload_b64u: None,
    };
    let first = sender
        .prepare_outbound(
            &sender_state.binding,
            "first",
            &plaintext("first"),
            "2026-09-11T00:00:01Z",
        )
        .unwrap();
    sender.mark_outbound_accepted(&first).unwrap();
    let second = sender
        .prepare_outbound(
            &sender_state.binding,
            "second",
            &plaintext("second"),
            "2026-09-11T00:00:02Z",
        )
        .unwrap();
    let commit = |tx: &rusqlite::Transaction<'_>, id: &String| {
        tx.execute("INSERT INTO projected(message_id) VALUES(?1)", [id])
            .map(|_| ())
            .map_err(crate::internal::local_state::local_state_unavailable)
    };
    let conflict = receiver
        .decrypt_inbound_validated_with_commit(
            &receiver_state.binding,
            &first.metadata,
            first.cipher_body().unwrap(),
            "2026-09-11T00:00:03Z",
            |decoded, _, _| {
                // Commit a second valid message after the first task has decrypted
                // its snapshot but before it can atomically commit that snapshot.
                other.decrypt_inbound_validated_with_commit(
                    &receiver_state.binding,
                    &second.metadata,
                    second.cipher_body().unwrap(),
                    "2026-09-11T00:00:03Z",
                    |decoded, _, _| Ok(decoded.logical_message_id.clone().unwrap()),
                    commit,
                )?;
                Ok(decoded.logical_message_id.clone().unwrap())
            },
            commit,
        )
        .err()
        .expect("first message must lose the forced revision race");
    assert!(
        crate::internal::message_runtime::sync_processing::retry_at(&conflict, 1).is_some(),
        "a valid message losing CAS must not become a permanent security rejection: {conflict:?}"
    );
    assert_eq!(
        receiver_db
            .query_row("SELECT COUNT(*) FROM projected", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );

    for attempt in 0..2 {
        let result = receiver
            .decrypt_inbound_validated_with_commit(
                &receiver_state.binding,
                &first.metadata,
                first.cipher_body().unwrap(),
                "2026-09-11T00:00:04Z",
                |decoded, _, _| Ok(decoded.logical_message_id.clone().unwrap()),
                commit,
            )
            .unwrap();
        assert_eq!(
            matches!(result, V2ValidatedInboundOutcome::Replay { .. }),
            attempt == 1
        );
    }
    assert_eq!(
        receiver_db
            .query_row("SELECT COUNT(*) FROM projected", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let stored = receiver_store
        .load_session(&receiver_state.binding, &receiver_state.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.revision, 2);

    let mut tampered = first.cipher_body().unwrap().clone();
    let mut ciphertext = URL_SAFE_NO_PAD.decode(&tampered.ciphertext_b64u).unwrap();
    ciphertext[0] ^= 1;
    tampered.ciphertext_b64u = URL_SAFE_NO_PAD.encode(ciphertext);
    let rejected = receiver
        .decrypt_inbound_validated_with_commit(
            &receiver_state.binding,
            &first.metadata,
            &tampered,
            "2026-09-11T00:00:05Z",
            |decoded, _, _| Ok(decoded.logical_message_id.clone().unwrap()),
            commit,
        )
        .err()
        .expect("a modified replay must still fail");
    assert!(matches!(rejected, crate::ImError::PermissionDenied));
    assert!(crate::internal::message_runtime::sync_processing::retry_at(&rejected, 1).is_none());
    assert_eq!(
        receiver_store
            .load_session(&receiver_state.binding, &receiver_state.session_id)
            .unwrap()
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn non_racing_or_ineligible_session_cas_failures_remain_terminal() {
    for case in ["future_revision", "disabled", "binding_changed", "removed"] {
        let root = tempfile::tempdir().unwrap();
        let db = crate::internal::local_state::open_writable(&root.path().join("receiver.sqlite"))
            .unwrap();
        let (_, state) = established_pair();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &db,
            vault(&root.path().join("vault"), 93),
            scope(
                "receiver",
                "did:example:alice",
                "alice-laptop",
                "did:example:alice#laptop-e2ee",
            ),
        )
        .unwrap();
        store
            .commit_inbound(
                &state,
                "setup",
                "sha256:setup",
                None,
                V2SessionExpectation::Absent,
                "2026-09-11T00:00:00Z",
            )
            .unwrap();
        let expected_revision = match case {
            "future_revision" => 5,
            "disabled" => {
                db.execute(
                    "UPDATE direct_e2ee_v2_sessions SET disabled=1, revision=1",
                    [],
                )
                .unwrap();
                0
            }
            "binding_changed" => {
                db.execute(
                    "UPDATE direct_e2ee_v2_sessions SET owner_did='did:example:other', revision=1",
                    [],
                )
                .unwrap();
                0
            }
            "removed" => {
                db.execute("DELETE FROM direct_e2ee_v2_sessions", [])
                    .unwrap();
                0
            }
            _ => unreachable!(),
        };
        let rejected = store
            .commit_inbound(
                &state,
                "rejected",
                "sha256:rejected",
                None,
                V2SessionExpectation::Revision(expected_revision),
                "2026-09-11T00:00:01Z",
            )
            .err()
            .expect("invalid state must not commit");
        assert!(
            matches!(rejected, crate::ImError::PermissionDenied),
            "{case}"
        );
        assert!(
            crate::internal::message_runtime::sync_processing::retry_at(&rejected, 1).is_none(),
            "{case}"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM direct_e2ee_v2_replay WHERE message_id='rejected'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
