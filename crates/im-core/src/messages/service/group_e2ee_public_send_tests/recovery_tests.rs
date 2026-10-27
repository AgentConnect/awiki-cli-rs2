use super::*;

#[test]
fn proven_expired_group_send_uses_a_new_frozen_attempt_and_keeps_original_bytes() {
    let fixture = Fixture::vnext();
    let open = |url| {
        crate::core::ImCore::new_with_options(
            fixture.config(url),
            fixture.paths(),
            fixture.vnext_open_options(),
        )
        .unwrap()
    };
    let selector = crate::identity::IdentitySelector::LocalAlias("alice".into());
    let core = open("http://127.0.0.1:9".into());
    let own = core
        .client(selector.clone())
        .unwrap()
        .did()
        .as_str()
        .to_owned();
    drop(core);
    let view = json!({"group_snapshot":{"group_did":fixture.group_did,"my_role":"member","membership_status":"active"}});
    let info = |version| {
        json!({"group_did":fixture.group_did,"group_state_version":version,
        "group_profile":{},"group_policy":{"message_security_profile":"group-e2ee"}})
    };
    let members = |version| {
        json!({"group_did":fixture.group_did,"group_state_version":version,
        "members":[{"agent_did":own,"status":"active","role":"member"}],"total":1,"has_more":false})
    };
    let first = RpcTestServer::spawn(vec![
        view.clone(),
        info("1"),
        view.clone(),
        info("1"),
        members("1"),
        view.clone(),
        info("1"),
        json!({"_test_drop_response":true}),
    ]);
    let core = open(first.base_url());
    let client = core.client(selector.clone()).unwrap();
    prepare_local_mls_group(&client, &fixture.group_did);
    let request = crate::messages::SendMessageRequest {
        target: crate::messages::MessageTarget::Group(
            crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
        ),
        body: crate::messages::MessageBody::Text {
            text: "same logical message".into(),
            kind: crate::messages::MessageKind::Text,
        },
        security: crate::messages::MessageSecurityMode::GroupE2ee,
        client_message_id: Some(crate::ids::MessageId::parse("msg-expired").unwrap()),
        delivery: crate::messages::MessageDeliveryOptions {
            idempotency_key: Some("op-expired".into()),
            wait_for_final_acceptance: true,
        },
        delegated_signing: None,
    };
    assert!(client.messages().send(request.clone()).is_err());
    let journal =
        crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &request).unwrap();
    let original = journal.load(&client).unwrap().unwrap();
    assert!(!journal.is_expired(&client, &original).unwrap());
    let unavailable = crate::ImError::Service {
        status_code: None,
        code: Some("group.e2ee.state_not_ready".into()),
        message: "in progress".into(),
        data: None,
    };
    assert!(!journal
        .record_expired(&client, &original, &unavailable)
        .unwrap());
    let proof = json!({"schema_version":1,"reason":"state_version_advanced","current_value":"2",
        "group_did":fixture.group_did,"operation_id":"op-expired","group_state_version":"1","epoch":"0",
        "crypto_group_id_b64u":original.encrypted.cipher.crypto_group_id_b64u});
    let mut wrong = proof.clone();
    wrong["operation_id"] = json!("someone-else");
    let error = |proof| crate::ImError::Service {
        status_code: None,
        code: Some("group.state_version_conflict".into()),
        message: "version advanced".into(),
        data: Some(json!({"expired_submission":proof})),
    };
    assert!(journal
        .record_expired(&client, &original, &error(wrong))
        .is_err());
    assert!(!journal.is_expired(&client, &original).unwrap());
    journal
        .record_expired(&client, &original, &error(proof))
        .unwrap();
    let next_operation = journal.retry_operation(&original).unwrap();
    let original_bytes = original.encrypted.clone();
    drop(client);
    drop(core);

    let second = RpcTestServer::spawn(vec![
        view.clone(),
        info("2"),
        members("2"),
        view,
        info("2"),
        json!({"accepted":true,"group_did":fixture.group_did,"message_id":"msg-expired","operation_id":next_operation,
            "group_event_seq":"91","group_state_version":"2","accepted_at":"2026-05-21T00:00:00Z","epoch":"0","group_receipt":{}}),
    ]);
    let core = open(second.base_url());
    let client = core.client(selector).unwrap();
    client.messages().send(request.clone()).unwrap();
    let count = second.requests().len();
    client.messages().send(request.clone()).unwrap();
    assert_eq!(
        second.requests().len(),
        count,
        "accepted child attempt replays locally"
    );
    let current = crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &request)
        .unwrap()
        .load(&client)
        .unwrap()
        .unwrap();
    assert_eq!(current.encrypted.meta.operation_id, next_operation);
    assert_eq!(
        current.encrypted.meta.message_id,
        original_bytes.meta.message_id
    );
    assert_ne!(
        current.encrypted.cipher.private_message_b64u,
        original_bytes.cipher.private_message_b64u
    );
    let db = rusqlite::Connection::open(fixture.paths().local_state.sqlite_path).unwrap();
    let frozen: String = db.query_row("SELECT intent_json FROM group_operation_journal WHERE json_extract(intent_json,'$.encrypted.meta.operation_id')='op-expired'",[],|row|row.get(0)).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&frozen).unwrap()["encrypted"],
        json!(original_bytes)
    );
    let messages: i64 = db
        .query_row(
            "SELECT count(*) FROM messages WHERE group_did=?1 AND server_seq=91",
            [fixture.group_did.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(messages, 1);
}

#[test]
fn public_group_send_reopens_unknown_submission_with_original_ciphertext() {
    let fixture = Fixture::vnext();
    let open = |url| {
        crate::core::ImCore::new_with_options(
            fixture.config(url),
            fixture.paths(),
            fixture.vnext_open_options(),
        )
        .unwrap()
    };
    let identity_core = open("http://127.0.0.1:9".into());
    let selector = crate::identity::IdentitySelector::LocalAlias("alice".into());
    let own = identity_core
        .client(selector.clone())
        .unwrap()
        .did()
        .as_str()
        .to_owned();
    drop(identity_core);
    let view = json!({"group_snapshot":{"group_did":fixture.group_did,
        "my_role":"member","membership_status":"active","required_security_profile":"transport-protected"}});
    let info = json!({"group_did":fixture.group_did,"group_state_version":"1",
        "group_profile":{},"group_policy":{"message_security_profile":"group-e2ee"}});
    let members = json!({"group_did":fixture.group_did,"group_state_version":"1",
        "members":[{"agent_did":own,"status":"active","role":"member"}],"total":1,"has_more":false});
    let reads = || {
        vec![
            view.clone(),
            info.clone(),
            view.clone(),
            info.clone(),
            members.clone(),
            view.clone(),
            info.clone(),
        ]
    };
    let accepted = json!({"accepted":true,"group_did":fixture.group_did,
        "message_id":"msg-recover-send","operation_id":"op-recover-send","group_event_seq":"91",
        "group_state_version":"1","accepted_at":"2026-05-21T00:00:00Z","epoch":"0","group_receipt":{"test":true}});
    let mut responses = reads();
    responses.push(json!({"_test_drop_response":true}));
    responses.push(accepted); // Frozen retry skips group preparation; accepted replay is local.
    let server = RpcTestServer::spawn(responses);
    let request = crate::messages::SendMessageRequest {
        target: crate::messages::MessageTarget::Group(
            crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
        ),
        body: crate::messages::MessageBody::Text {
            text: "must survive response loss".into(),
            kind: crate::messages::MessageKind::Text,
        },
        security: crate::messages::MessageSecurityMode::E2eeRequired,
        client_message_id: Some(crate::ids::MessageId::parse("msg-recover-send").unwrap()),
        delivery: crate::messages::MessageDeliveryOptions {
            idempotency_key: Some("op-recover-send".into()),
            wait_for_final_acceptance: true,
        },
        delegated_signing: None,
    };
    let core = open(server.base_url());
    let client = core.client(selector.clone()).unwrap();
    prepare_local_mls_group(&client, &fixture.group_did);
    assert!(client.messages().send(request.clone()).is_err());
    drop(client);
    drop(core);

    let db = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_projection BEFORE INSERT ON messages WHEN NEW.server_seq = 91 BEGIN SELECT RAISE(ABORT,'synthetic projection failure'); END;").unwrap();
    let core = open(server.base_url());
    let client = core.client(selector.clone()).unwrap();
    let accepted_without_projection = client.messages().send(request.clone()).unwrap();
    assert_eq!(
        accepted_without_projection.delivery,
        crate::messages::DeliveryState::Accepted
    );
    assert!(!accepted_without_projection.warnings.is_empty());
    drop(client);
    drop(core);
    db.execute_batch("DROP TRIGGER fail_projection;").unwrap();
    drop(db);

    let core = open(server.base_url());
    let client = core.client(selector).unwrap();
    let mut replay = request.clone();
    replay.delivery.wait_for_final_acceptance = false;
    replay.delivery.idempotency_key = Some(" op-recover-send ".into());
    let restored = client.messages().send(replay).unwrap();
    assert_eq!(restored.delivery, crate::messages::DeliveryState::Accepted);
    assert!(restored.warnings.is_empty());
    let mut conflicting = request.clone();
    conflicting.body = crate::messages::MessageBody::Text {
        text: "different content".into(),
        kind: crate::messages::MessageKind::Text,
    };
    assert!(client.messages().send(conflicting).is_err());
    let sends: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.rpc_method == "group.e2ee.send")
        .collect();
    assert_eq!(
        sends.len(),
        2,
        "unknown retry plus original; accepted replay must not submit"
    );
    assert_eq!(sends[0].params["meta"], sends[1].params["meta"]);
    assert_eq!(sends[0].params["body"], sends[1].params["body"]);
    assert_eq!(sends[0].params["client"], sends[1].params["client"]);
    let db = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM messages WHERE owner_identity_id=?1 AND group_did=?2 AND server_seq=91", [client.current_identity().id.as_str(), fixture.group_did.as_str()],|r|r.get(0)).unwrap();
    assert_eq!(count, 1);
    let journals: String = db.query_row("SELECT group_concat(intent_json) FROM group_operation_journal WHERE operation_key LIKE 'p6-application-send%'",[],|r|r.get(0)).unwrap();
    assert!(!journals.contains("must survive response loss"));

    // A renewed authorization cannot borrow either an old prepared ciphertext
    // or its cached acceptance. This path performs no network calls.
    let mut normalized = request;
    normalized.security = crate::messages::MessageSecurityMode::GroupE2ee;
    let journal =
        crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &normalized).unwrap();
    assert!(journal.load(&client).unwrap().is_some());
    let paths = fixture.paths();
    let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
    let mut state = store.load_index().unwrap().credentials["alice"]
        .device_state
        .clone()
        .unwrap();
    state.authorization.as_mut().unwrap().auth_generation += 1;
    store.save_device_state("alice", state).unwrap();
    assert!(journal.load(&client).is_err());
    let renewed =
        crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &normalized).unwrap();
    assert!(renewed.load(&client).is_err());
}

#[test]
fn distinct_group_operation_ids_cannot_alias_an_acceptance_record() {
    check_operation_key_domains(false);
}

#[test]
fn legacy_group_intents_and_receipts_remain_distinct_and_recoverable() {
    check_operation_key_domains(true);
}

fn check_operation_key_domains(legacy: bool) {
    let fixture = Fixture::vnext();
    let open = |url| {
        crate::core::ImCore::new_with_options(
            fixture.config(url),
            fixture.paths(),
            fixture.vnext_open_options(),
        )
        .unwrap()
    };
    let selector = crate::identity::IdentitySelector::LocalAlias("alice".into());
    let core = open("http://127.0.0.1:9".into());
    let own = core
        .client(selector.clone())
        .unwrap()
        .did()
        .as_str()
        .to_owned();
    drop(core);
    let view = json!({"group_snapshot":{"group_did":fixture.group_did,
        "my_role":"member","membership_status":"active","required_security_profile":"transport-protected"}});
    let info = json!({"group_did":fixture.group_did,"group_state_version":"1",
        "group_profile":{},"group_policy":{"message_security_profile":"group-e2ee"}});
    let members = json!({"group_did":fixture.group_did,"group_state_version":"1",
        "members":[{"agent_did":own,"status":"active","role":"member"}],"total":1,"has_more":false});
    let mut responses = Vec::new();
    let deliveries = if legacy {
        vec![
            ("msg-key-one", "op-key", "91"),
            ("msg-key-two", "op-key:accepted", "92"),
            ("msg-key-one", "op-key", "91"),
        ]
    } else {
        vec![
            ("msg-key-one", "op-key", "91"),
            ("msg-key-two", "op-key:accepted", "92"),
        ]
    };
    for (index, (message, operation, seq)) in deliveries.into_iter().enumerate() {
        if index < 2 {
            responses.extend([
                view.clone(),
                info.clone(),
                view.clone(),
                info.clone(),
                members.clone(),
                view.clone(),
                info.clone(),
            ]);
        }
        responses.push(json!({"accepted":true,"group_did":fixture.group_did,
            "message_id":message,"operation_id":operation,"group_event_seq":seq,
            "group_state_version":"1","accepted_at":"2026-05-21T00:00:00Z","epoch":"0","group_receipt":{}}));
    }
    let server = RpcTestServer::spawn(responses);
    let steps = if legacy { vec![0, 1, 0, 1] } else { vec![0, 1] };
    let mut parent_request = None;
    for (step, index) in steps.into_iter().enumerate() {
        let operation = if index == 0 {
            "op-key"
        } else {
            "op-key:accepted"
        };
        let core = open(server.base_url());
        let client = core.client(selector.clone()).unwrap();
        if step == 0 {
            prepare_local_mls_group(&client, &fixture.group_did);
        }
        let request = crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(&fixture.group_did).unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: format!("distinct operation {index}"),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::E2eeRequired,
            client_message_id: Some(
                crate::ids::MessageId::parse(if index == 0 {
                    "msg-key-one"
                } else {
                    "msg-key-two"
                })
                .unwrap(),
            ),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some(operation.into()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        };
        if step == 0 {
            parent_request = Some(request.clone());
        }
        let accepted = client.messages().send(request.clone()).unwrap();
        assert_eq!(accepted.delivery, crate::messages::DeliveryState::Accepted);
        if legacy {
            let db = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
            if step == 0 {
                // Model the exact v1 prepared/accepted layout without changing its
                // binding, ciphertext, acceptance, or any SDK-owned state.
                assert_eq!(db.execute("UPDATE group_operation_journal SET operation_key='p6-application-send:op-key' WHERE json_extract(intent_json,'$.encrypted.meta.operation_id')='op-key'", []).unwrap(), 1);
                assert_eq!(db.execute("UPDATE group_operation_journal SET operation_key='p6-application-send:op-key:accepted' WHERE json_extract(intent_json,'$.result.operation_id')='op-key'", []).unwrap(), 1);
                let mut child = request.clone();
                child.delivery.idempotency_key = Some("op-key:accepted".into());
                child.security = crate::messages::MessageSecurityMode::GroupE2ee;
                let journal =
                    crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &child)
                        .unwrap();
                assert!(journal.load(&client).unwrap().is_none());
                let receipt: String = db.query_row("SELECT intent_json FROM group_operation_journal WHERE operation_key='p6-application-send:op-key:accepted'", [], |r| r.get(0)).unwrap();
                db.execute("UPDATE group_operation_journal SET intent_json=json_set(intent_json,'$.request_binding','invalid') WHERE operation_key='p6-application-send:op-key:accepted'", []).unwrap();
                assert!(
                    journal.load(&client).is_err(),
                    "a corrupt legacy row must not be ignored"
                );
                db.execute("UPDATE group_operation_journal SET intent_json=?1 WHERE operation_key='p6-application-send:op-key:accepted'", [&receipt]).unwrap();
            } else if step == 1 {
                // Legacy first-op acceptance may be absent while a distinct
                // second-op prepared intent occupies that same raw key.
                assert_eq!(db.execute("DELETE FROM group_operation_journal WHERE operation_key='p6-application-send:op-key:accepted'", []).unwrap(), 1);
                assert_eq!(db.execute("UPDATE group_operation_journal SET operation_key='p6-application-send:op-key:accepted' WHERE json_extract(intent_json,'$.encrypted.meta.operation_id')='op-key:accepted'", []).unwrap(), 1);
                let mut parent = parent_request.as_ref().unwrap().clone();
                parent.security = crate::messages::MessageSecurityMode::GroupE2ee;
                let journal =
                    crate::internal::group_e2ee::send_journal::SendJournal::new(&client, &parent)
                        .unwrap();
                let prepared = journal.load(&client).unwrap().unwrap();
                assert!(journal.accepted(&client, &prepared).unwrap().is_none());
                let other: String = db.query_row("SELECT intent_json FROM group_operation_journal WHERE operation_key='p6-application-send:op-key:accepted'", [], |r| r.get(0)).unwrap();
                db.execute("UPDATE group_operation_journal SET intent_json=json_set(intent_json,'$.schema_version',99) WHERE operation_key='p6-application-send:op-key:accepted'", []).unwrap();
                assert!(
                    journal.accepted(&client, &prepared).is_err(),
                    "an unknown legacy schema must fail closed"
                );
                db.execute("UPDATE group_operation_journal SET intent_json=?1 WHERE operation_key='p6-application-send:op-key:accepted'", [&other]).unwrap();
            }
        }
    }
    let sends: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.rpc_method == "group.e2ee.send")
        .collect();
    assert_eq!(sends.len(), if legacy { 3 } else { 2 });
    if legacy {
        assert_eq!(sends[0].params["meta"], sends[2].params["meta"]);
        assert_eq!(sends[0].params["body"], sends[2].params["body"]);
        assert_eq!(sends[0].params["client"], sends[2].params["client"]);
    }
    let db = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE group_did=?1 AND server_seq IN (91,92)",
            [&fixture.group_did],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}
