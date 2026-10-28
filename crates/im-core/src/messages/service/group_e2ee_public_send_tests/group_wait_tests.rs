use super::*;
use crate::internal::group_e2ee::send_state;
use crate::internal::wire::sync_v2::group_e2ee_state::{
    GroupStateObservation, GroupStateObservations,
};

// Exercise the public send with real local MLS state. B's deferred input must
// survive; completing A's background preparation must release only A's send.
#[tokio::test]
async fn group_send_does_not_wait_for_another_groups_retry_backoff() {
    use std::time::Duration;
    for (role, lane, shared_baseline) in
        [("owner", "p6_group", false), ("member", "ordinary", true)]
    {
        let fixture = Fixture::vnext();
        let server = RpcTestServer::spawn(vec![json!({
            "accepted":true,"group_did":fixture.group_did,
            "message_id":"msg-scoped-wait","operation_id":"op-scoped-wait",
            "group_event_seq":"91","group_state_version":"1",
            "accepted_at":"2026-05-21T00:00:00Z","epoch":"0","group_receipt":{}
        })]);
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
        let auth = crate::internal::identity_device_state::active_authorization_for_client(&client)
            .unwrap();
        let owner = client.current_identity().id.as_str();
        let db =
            crate::internal::local_state::open_writable(&fixture.paths().local_state.sqlite_path)
                .unwrap();
        crate::internal::local_state::sync_v2::upsert_identity_account_binding(
            &db,
            &crate::internal::local_state::sync_v2::IdentityAccountBinding {
                owner_identity_id: owner.into(),
                account_id: "fixture-account".into(),
                handle_scope: None,
                current_did: client.did().as_str().into(),
                protocol_device_id: auth.protocol_device_id.as_str().into(),
                identity_generation: "1".into(),
                device_auth_generation: auth.auth_generation.to_string(),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let connection = send_state::ConnectionGuard::connected(&client).unwrap();
        let group = crate::ids::GroupRef::parse(&fixture.group_did).unwrap();
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
        let now = chrono::Utc::now().timestamp();
        let insert = |id: &str, input_lane: &str, group_did: Option<&str>| {
            db.execute("INSERT INTO sync_lane_inbox(
                input_id,owner_identity_id,lane,lane_epoch,position,event_id,event_type,
                raw_payload_json,payload_bytes,account_id_snapshot,device_id_snapshot,
                auth_generation_snapshot,client_instance_id_snapshot,group_did,processing_scope,
                received_at,created_at,next_attempt_at,attempt_count,processing_error_code)
                VALUES(?1,?2,?3,'1','1',?1,?4,'{}',2,'fixture-account',?5,?6,'installation',?7,COALESCE(?7,'baseline'),
                    '2026-10-28T00:00:00Z',?8,?8+60,6,'group.e2ee.state_not_ready')",
                rusqlite::params![id,owner,input_lane,
                    if input_lane=="p6_group" {"p6.control.notice"} else {"group.member_changed"},
                    auth.protocol_device_id.as_str(),auth.auth_generation.to_string(),group_did,now]).unwrap();
        };
        insert(
            "other-group-backoff",
            lane,
            Some("did:wba:awiki.test:groups:other"),
        );
        if shared_baseline {
            insert("shared-baseline", "baseline", None);
        }
        let authoritative = crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_did":fixture.group_did,"group_state_version":"1",
                "group_snapshot":{"group_did":fixture.group_did,"my_role":role,"membership_status":"active"},
                "group_policy":{"message_security_profile":"group-e2ee"}
            }),
            vec![],
        );
        assert!(send_state::confirm_members(
            &client,
            &group,
            &authoritative,
            std::collections::BTreeSet::from([client.did().as_str().into()])
        )
        .unwrap());
        let ticket = send_state::preparation_ticket(&client, &group).unwrap();
        let lock = client
            .core_inner()
            .group_lifecycle_lock(owner, group.as_str());
        let preparation_guard = lock.lock().await;
        let messages = client.messages();
        let mut send = Box::pin(messages.send_async(crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(group.clone()),
            body: crate::messages::MessageBody::Text {
                text: "target group only".into(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::E2eeRequired,
            client_message_id: Some(crate::ids::MessageId::parse("msg-scoped-wait").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-scoped-wait".into()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        }));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), send.as_mut())
                .await
                .is_err()
        );
        // Complete preparation while the foreground send is already waiting.
        send_state::publish_prepared(&client, ticket, &authoritative).unwrap();
        for (key, progress) in crate::internal::group_runtime::journal::pending(&client).unwrap() {
            crate::internal::group_runtime::recovery::complete_observed(&client, &key, &progress)
                .unwrap();
        }
        if shared_baseline {
            // Check the scoped waiter directly while holding A's preparation
            // lock; background recovery must not consume the send-only RPC
            // fixture while the deliberately retained baseline is unfinished.
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(100),
                    crate::messages::processing::wait_for_group_inputs_async(&client, &group),
                )
                .await
                .is_err(),
                "shared baseline must block the scoped waiter itself"
            );
            assert!(send_state::prepared_snapshot(&client, &group)
                .unwrap()
                .is_none());
            assert!(
                tokio::time::timeout(Duration::from_millis(100), send.as_mut())
                    .await
                    .is_err(),
                "an owner-wide baseline must still block A"
            );
            assert!(server
                .requests()
                .iter()
                .all(|r| r.rpc_method == "anp.get_capabilities"));
            db.execute(
                "DELETE FROM sync_lane_inbox WHERE input_id='shared-baseline'",
                [],
            )
            .unwrap();
        }
        drop(preparation_guard);
        assert!(send_state::prepared_snapshot(&client, &group)
            .unwrap()
            .is_some());
        let result = tokio::time::timeout(Duration::from_secs(2), send.as_mut()).await;
        let deferred: (i64,i64) = db.query_row(
            "SELECT COUNT(*),MIN(next_attempt_at) FROM sync_lane_inbox WHERE input_id='other-group-backoff' AND closed_at IS NULL",
            [], |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(deferred.0, 1);
        assert!(
            deferred.1 > chrono::Utc::now().timestamp() + 30,
            "B must remain in its original long backoff"
        );
        assert!(
            result.is_ok(),
            "A became Ready but its send waited for unrelated B's 60-second backoff"
        );
        result.unwrap().unwrap();
        let requests: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|r| r.rpc_method != "anp.get_capabilities")
            .collect();
        assert_eq!(
            requests.len(),
            1,
            "no new group preparation RPCs after background Ready"
        );
        assert_eq!(requests[0].rpc_method, "group.e2ee.send");
        assert!(!requests[0].params.to_string().contains("target group only"));
        drop(send);
        drop(connection);
        core.shutdown_local_state_for_reset().await.unwrap();
    }
}
