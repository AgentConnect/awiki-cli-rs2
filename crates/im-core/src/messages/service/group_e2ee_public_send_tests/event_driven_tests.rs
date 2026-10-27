use super::*;
use crate::internal::group_e2ee::send_state;
use crate::internal::wire::sync_v2::group_e2ee_state::{
    GroupStateObservation, GroupStateObservations,
};

#[tokio::test]
async fn prepared_owner_and_member_submit_without_frontloaded_group_reads() {
    for role in ["owner", "member"] {
        let fixture = Fixture::vnext();
        let accepted = json!({"accepted":true,"group_did":fixture.group_did,
            "message_id":"msg-ready","operation_id":"op-ready","group_event_seq":"91",
            "group_state_version":"1","accepted_at":"2026-05-21T00:00:00Z","epoch":"0","group_receipt":{}});
        let server = RpcTestServer::spawn(vec![accepted]);
        let mut config = fixture.config(server.base_url());
        config.anp_service_did = Some(crate::ids::Did::parse("did:wba:awiki.test").unwrap());
        let core = crate::core::ImCore::new_with_options(
            config,
            fixture.paths(),
            fixture.vnext_open_options(),
        )
        .unwrap();
        let client = core
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".into(),
            ))
            .unwrap();
        prepare_local_mls_group(&client, &fixture.group_did);
        let authorization =
            crate::internal::identity_device_state::active_authorization_for_client(&client)
                .unwrap();
        let db =
            crate::internal::local_state::open_writable(&fixture.paths().local_state.sqlite_path)
                .unwrap();
        crate::internal::local_state::sync_v2::upsert_identity_account_binding(
            &db,
            &crate::internal::local_state::sync_v2::IdentityAccountBinding {
                owner_identity_id: client.current_identity().id.as_str().into(),
                account_id: "fixture-account".into(),
                handle_scope: None,
                current_did: client.did().as_str().into(),
                protocol_device_id: authorization.protocol_device_id.as_str().into(),
                identity_generation: "1".into(),
                device_auth_generation: authorization.auth_generation.to_string(),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        drop(db);
        let connection = send_state::ConnectionGuard::connected(&client).unwrap();
        let observation = GroupStateObservations {
            schema_version: 1,
            complete: true,
            groups: vec![GroupStateObservation {
                group_did: fixture.group_did.clone(),
                host_service_did: "did:wba:awiki.test".into(),
                group_state_version: "1".into(),
                epoch: Some("0".into()),
                revision: format!("sha256:{}", "a".repeat(43)),
                local_identity_coverage: true,
            }],
        };
        send_state::observe_async(
            &client,
            Some(&observation),
            send_state::connection_token(&client),
            true,
            false,
        )
        .await
        .unwrap();
        let group = crate::ids::GroupRef::parse(&fixture.group_did).unwrap();
        assert!(
            send_state::prepared_snapshot(&client, &group)
                .unwrap()
                .is_none(),
            "a connection plus observation does not prepare MLS"
        );
        let authoritative = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_did":fixture.group_did, "group_state_version":"1",
                "group_snapshot":{"group_did":fixture.group_did,"my_role":role,"membership_status":"active"},
                "group_policy":{"message_security_profile":"group-e2ee"}
            }),
            vec![],
        );
        assert!(send_state::confirm_members(
            &client,
            &group,
            &authoritative,
            std::collections::BTreeSet::from([client.did().as_str().to_owned()])
        )
        .unwrap());
        let ticket = send_state::preparation_ticket(&client, &group).unwrap();
        send_state::publish_prepared(&client, ticket.clone(), &authoritative).unwrap();
        for (key, progress) in crate::internal::group_runtime::journal::pending(&client).unwrap() {
            crate::internal::group_runtime::recovery::complete_observed(&client, &key, &progress)
                .unwrap();
        }
        assert!(send_state::prepared_snapshot(&client, &group)
            .unwrap()
            .is_some());
        let request = crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(group.clone()),
            body: crate::messages::MessageBody::Text {
                text: "one encrypted submission".into(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::E2eeRequired,
            client_message_id: Some(crate::ids::MessageId::parse("msg-ready").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-ready".into()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        };
        client.messages().send_async(request).await.unwrap();
        let business: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|request| request.rpc_method != "anp.get_capabilities")
            .collect();
        assert_eq!(
            business.len(),
            1,
            "{role} should issue only the ciphertext submission"
        );
        assert_eq!(business[0].rpc_method, "group.e2ee.send");
        assert!(!business[0]
            .params
            .to_string()
            .contains("one encrypted submission"));
        send_state::invalidate(&client, Some(group.as_str()));
        send_state::publish_prepared(&client, ticket, &authoritative).unwrap();
        assert!(
            send_state::prepared_snapshot(&client, &group)
                .unwrap()
                .is_none(),
            "older preparation must not override a later change"
        );
        let newer_connection = send_state::ConnectionGuard::connected(&client).unwrap();
        let newer_token = send_state::connection_token(&client);
        drop(connection);
        assert_eq!(
            send_state::connection_token(&client),
            newer_token,
            "old transport shutdown cannot disconnect a newer connection"
        );
        drop(newer_connection);
        assert!(
            send_state::preparation_ticket(&client, &group)
                .unwrap()
                .is_none(),
            "disconnect must require a new synchronized connection"
        );
        core.shutdown_local_state_for_reset().await.unwrap();
    }
}
