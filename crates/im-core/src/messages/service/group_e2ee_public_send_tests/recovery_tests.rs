use super::*;

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
    responses.extend(reads());
    responses.push(accepted);
    responses.extend(reads()); // A reopened accepted intent only restores projection.
    responses.extend([view.clone(), info.clone(), view, info]); // Changed body fails closed.
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
    let journals: String = db.query_row("SELECT group_concat(intent_json) FROM group_operation_journal WHERE operation_key LIKE 'p6-application-send:%'",[],|r|r.get(0)).unwrap();
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
