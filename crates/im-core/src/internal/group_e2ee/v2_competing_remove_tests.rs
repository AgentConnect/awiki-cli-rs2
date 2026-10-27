use super::*;
use sha2::{Digest, Sha256};

#[test]
fn group_competing_remove_waits_for_host_decision_before_applying_winner_notice() {
    let directory = TestDirectory::new("im-core-competing-remove");
    let fixture = make_did_fixture("competing", &["a1", "a2", "a3"]);
    let transport = LoopbackTransport::default();
    let mut products: Vec<_> = fixture
        .devices
        .iter()
        .map(|device| product(&directory, &fixture, device, transport.clone()))
        .collect();
    let mut packages = Vec::new();
    for (index, device) in fixture.devices.iter().enumerate() {
        packages.push(
            products[index]
                .prepare_current_key_package(
                    key_service_meta(&fixture.did, &device.device_id, &format!("publish-{index}")),
                    key_package_input(
                        &fixture,
                        device,
                        &format!("kp-{index}"),
                        &format!("kp-request-{index}"),
                    ),
                    &fixture.document,
                    &signing_key(device),
                )
                .unwrap()
                .body
                .group_key_package,
        );
    }
    let create = products[0]
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(
                &fixture.did,
                &fixture.devices[0].device_id,
                "create-competing",
            ),
            group_state_ref: state_ref(1),
            creator_key_package: packages[0].clone(),
            creator_did_document: fixture.document.clone(),
            now: NOW.into(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-create".into(),
            request_id: "prepare-create".into(),
        })
        .unwrap();
    products[0]
        .submit_create(&create, "finalize-create")
        .unwrap();
    for index in 1..3 {
        let add = products[0]
            .prepare_add(V2AddMemberInput {
                meta: control_meta(
                    &fixture.did,
                    &fixture.devices[0].device_id,
                    &format!("add-{index}"),
                ),
                group_state_ref: state_ref(2),
                group_key_package: packages[index].clone(),
                member_did_document: fixture.document.clone(),
                now: NOW.into(),
                draft_extension_negotiated: true,
                pending_commit_id: format!("pending-add-{index}"),
                request_id: format!("prepare-add-{index}"),
            })
            .unwrap();
        products[0]
            .submit_add(&add, format!("finalize-add-{index}"))
            .unwrap();
        for recipient in 0..index {
            consume_public_notice(
                &directory,
                &fixture,
                &fixture.devices[recipient],
                commit_notice(
                    &fixture,
                    &fixture.devices[recipient],
                    &add.prepared.body,
                    &format!("add-notice-{index}-{recipient}"),
                    "active",
                ),
            )
            .unwrap();
        }
        consume_public_notice(
            &directory,
            &fixture,
            &fixture.devices[index],
            welcome_notice(
                &fixture,
                &fixture.devices[index],
                &add.prepared.body,
                &format!("welcome-{index}"),
            ),
        )
        .unwrap();
    }
    let mut removals = Vec::new();
    for index in [0, 2] {
        removals.push(
            products[index]
                .prepare_remove(V2RemoveMemberInput {
                    meta: control_meta(
                        &fixture.did,
                        &fixture.devices[index].device_id,
                        &format!("remove-{index}"),
                    ),
                    group_state_ref: state_ref(3),
                    member_did: fixture.did.clone(),
                    member_device_id: fixture.devices[1].device_id.clone(),
                    member_did_document: fixture.document.clone(),
                    now: NOW.into(),
                    draft_extension_negotiated: true,
                    pending_commit_id: format!("pending-remove-{index}"),
                    request_id: format!("prepare-remove-{index}"),
                })
                .unwrap(),
        );
    }
    assert_ne!(
        removals[0].prepared.body.commit_b64u,
        removals[1].prepared.body.commit_b64u
    );
    products[0]
        .submit_remove(&removals[0], "finalize-winner")
        .unwrap();
    let notice = remove_commit_notice(
        &fixture,
        &fixture.devices[2],
        &removals[0].prepared.body,
        "winning-remove-notice",
    );
    // Open an independent runtime against the persisted store: process-local
    // mutexes alone cannot protect a surviving Core after another one restarts.
    let blocked = consume_public_notice(&directory, &fixture, &fixture.devices[2], notice.clone())
        .unwrap_err();
    assert!(
        matches!(blocked,crate::ImError::Service{code:Some(code),..} if code=="group.e2ee.state_not_ready")
    );
    let loser = &removals[1];
    let body = &loser.prepared.body;
    let rejection = crate::ImError::Service {
        status_code: Some(200),
        code: Some("anp.target_not_found".into()),
        message: "competing remove accepted".into(),
        data: Some(json!({"rejected_membership_submission":{
            "schema_version":1,"reason":"competing_commit_accepted","method":"group.e2ee.remove",
            "operation_id":loser.meta.operation_id,"sender_did":loser.meta.sender_did,"sender_device_id":loser.meta.sender_device_id,
            "group_state_ref":body.group_state_ref,"crypto_group_id_b64u":body.crypto_group_id_b64u,"epoch":body.epoch,
            "member_did":body.member_did,"member_device_id":body.member_device_id,
            "commit_b64u_sha256":URL_SAFE_NO_PAD.encode(Sha256::digest(body.commit_b64u.as_bytes())),
        }})),
    };
    assert!(
        crate::internal::group_e2ee::v2_remove_recovery::proven_competing_remove(&rejection, loser)
    );
    let reopened = product(&directory, &fixture, &fixture.devices[2], transport);
    assert_eq!(
        reopened
            .abort_pending(&loser.prepared.pending_commit_id, "abort-proven-loser")
            .unwrap()
            .status,
        "aborted"
    );
    let applied =
        consume_public_notice(&directory, &fixture, &fixture.devices[2], notice.clone()).unwrap();
    assert_eq!(applied.epoch, "3");
    assert!(!applied.self_removed);
    assert_eq!(
        consume_public_notice(&directory, &fixture, &fixture.devices[2], notice).unwrap(),
        applied
    );
    for index in [0, 2] {
        assert!(
            status_runtime(&directory, &fixture, &fixture.devices[index])
                .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
                .unwrap()
                .can_send_secure
        );
    }
}
