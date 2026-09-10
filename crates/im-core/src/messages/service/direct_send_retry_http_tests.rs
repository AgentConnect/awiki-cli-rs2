// Included in the existing bounded, loopback-only Direct HTTP fixture module.
fn target_first_request() -> crate::messages::SendMessageRequest {
    crate::messages::SendMessageRequest {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse(OLD_DID, "").unwrap(),
        ),
        body: crate::messages::MessageBody::Text {
            text: TEXT.to_owned(),
            kind: crate::messages::MessageKind::Text,
        },
        security: crate::messages::MessageSecurityMode::DefaultPlain,
        client_message_id: Some(crate::ids::MessageId::parse(MESSAGE_ID).unwrap()),
        delivery: crate::messages::MessageDeliveryOptions {
            idempotency_key: Some(OPERATION_ID.to_owned()),
            ..Default::default()
        },
        delegated_signing: None,
    }
}

#[tokio::test]
async fn target_first_direct_retry_after_reopen_preserves_wire_payload() {
    for blocking in [false, true] {
        let fixture = Fixture::new("target-first-reopen");
        let server = HttpTestServer::spawn(vec![
            ExpectedHttp::rpc_result(accepted(OLD_DID)),
            ExpectedHttp::rpc_result(accepted(OLD_DID)),
        ]);
        for attempt in 0..2 {
            let client = fixture.client(server.base_url()).await;
            let result = if blocking {
                tokio::task::spawn_blocking(move || client.messages().send(target_first_request()))
                    .await
                    .unwrap()
            } else {
                client.messages().send_async(target_first_request()).await
            }
            .unwrap();
            assert_eq!(result.message.id.as_str(), MESSAGE_ID);
            assert!(result.warnings.is_empty(), "{:?}", result.warnings);
            if attempt == 0 {
                tokio::time::sleep(Duration::from_millis(1100)).await;
            }
        }
        let requests = server.join();
        assert_request_sequence(&requests, &["direct.send", "direct.send"]);
        assert_eq!(requests[0].params()["meta"], requests[1].params()["meta"]);
        assert_eq!(requests[0].params()["body"], requests[1].params()["body"]);
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM messages WHERE msg_id = ?1",
                [MESSAGE_ID],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(db);
        fs::remove_dir_all(&fixture.root).unwrap();
    }
}

#[tokio::test]
async fn target_first_direct_changed_body_is_rejected_before_network() {
    let fixture = Fixture::new("target-first-conflict");
    let server = HttpTestServer::spawn(vec![ExpectedHttp::rpc_result(accepted(OLD_DID))]);
    let client = fixture.client(server.base_url()).await;
    client
        .messages()
        .send_async(target_first_request())
        .await
        .unwrap();
    let mut changed = target_first_request();
    changed.body = crate::messages::MessageBody::Text {
        text: "different message".to_owned(),
        kind: crate::messages::MessageKind::Text,
    };
    assert!(matches!(
        client.messages().send_async(changed).await,
        Err(crate::ImError::MessageWireIdentityConflict { .. })
    ));
    assert_eq!(server.join().len(), 1);
    drop(client);
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[tokio::test]
async fn target_first_direct_handle_retry_keeps_accepted_wire_target_after_recovery() {
    let fixture = Fixture::new("target-first-handle-recovery");
    let mut old_binding = directory_lookup(OLD_DID);
    old_binding["binding_generation"] = json!("1");
    let mut new_binding = directory_lookup(NEW_DID);
    new_binding["binding_generation"] = json!("2");
    let server = HttpTestServer::spawn(vec![
        ExpectedHttp::rpc_result(old_binding),
        ExpectedHttp::rpc_result(accepted(OLD_DID)),
        ExpectedHttp::rpc_result(new_binding),
        ExpectedHttp::rpc_result(accepted(OLD_DID)),
    ]);
    let client = fixture.client(server.base_url()).await;
    let mut request = target_first_request();
    request.target =
        crate::messages::MessageTarget::Direct(crate::ids::PeerRef::parse(HANDLE, "").unwrap());
    client.messages().send_async(request.clone()).await.unwrap();
    let replay = client.messages().send_async(request).await.unwrap();
    assert!(replay.warnings.is_empty(), "{:?}", replay.warnings);
    let requests = server.join();
    let sends = requests
        .iter()
        .filter(|request| request.rpc_method().as_deref() == Some("direct.send"))
        .collect::<Vec<_>>();
    assert_eq!(sends.len(), 2);
    assert_eq!(sends[0].params()["meta"], sends[1].params()["meta"]);
    assert_eq!(sends[1].params()["meta"]["target"]["did"], OLD_DID);
    assert_eq!(
        message_projection(&fixture.sqlite_path(), MESSAGE_ID).current_target_did,
        NEW_DID
    );
    drop(client);
    fs::remove_dir_all(&fixture.root).unwrap();
}
