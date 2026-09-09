#[tokio::test]
async fn explicit_direct_retry_reuses_durable_wire_metadata() {
    let fixture = Fixture::new("plain-direct-durable-retry");
    let client = fixture.client();
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.awiki.test",
    )
    .unwrap();
    let conversation =
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(&scope);
    fixture.seed_route(&conversation, &scope, "did:example:bob-current");
    let resolved = || super::ResolvedSendRequest {
        request: crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
            ),
            body: crate::messages::MessageBody::Payload {
                payload: serde_json::json!({"event":"example"}),
            },
            security: crate::messages::MessageSecurityMode::DefaultPlain,
            client_message_id: Some(crate::ids::MessageId::parse("msg-durable-direct").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-durable-direct".into()),
                wait_for_final_acceptance: false,
            },
            delegated_signing: None,
        },
        target_did: Some("did:example:bob-current".into()),
        peer_scope: Some(scope.clone()),
    };
    let mut first = super::plain_direct_submission(&client, resolved()).unwrap();
    first.wire_created_at = "2026-07-05T00:00:00Z".into();
    crate::internal::message_runtime::local_projection::persist_send_projection_async(
        &client,
        &first.request.target,
        &first.request.body,
        first.request.client_message_id.as_ref().unwrap(),
        first.request.delivery.idempotency_key.as_deref(),
        crate::messages::DeliveryState::StoredLocally,
        first.wire_target_did.as_deref(),
        first.target_handle.as_deref(),
        first.peer_scope.as_ref(),
        Some(&first.wire_created_at),
    )
    .await
    .unwrap();
    // Simulate restarting the caller after an uncertain network outcome.
    let retry = super::plain_direct_submission(&fixture.client(), resolved()).unwrap();
    assert_eq!(retry.wire_created_at, first.wire_created_at);
    assert_eq!(retry.wire_target_did, first.wire_target_did);
    assert_eq!(
        retry.request.client_message_id,
        first.request.client_message_id
    );
    assert_eq!(
        retry.request.delivery.idempotency_key,
        first.request.delivery.idempotency_key
    );
}
