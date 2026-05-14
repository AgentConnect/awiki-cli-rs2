use anp::proof::{verify_rfc9421_origin_proof, Rfc9421OriginProofVerificationOptions};
use awiki_cli::identity::generate_identity;
use awiki_cli::identity::types::{GeneratedIdentity, StoredIdentity};
use awiki_cli::message::{
    build_group_e2ee_add_rpc_params, build_group_e2ee_create_rpc_params,
    build_group_e2ee_get_key_package_rpc_params,
    build_group_e2ee_get_recovery_key_package_rpc_params,
    build_group_e2ee_get_update_key_package_rpc_params, build_group_e2ee_head_rpc_params,
    build_group_e2ee_leave_request_rpc_params, build_group_e2ee_leave_rpc_params,
    build_group_e2ee_notice_rpc_params, build_group_e2ee_publish_key_package_rpc_params,
    build_group_e2ee_recover_member_rpc_params, build_group_e2ee_remove_rpc_params,
    build_group_e2ee_send_rpc_params, build_group_e2ee_update_member_rpc_params, MessageError,
    GROUP_E2EE_CIPHER_CONTENT_TYPE, GROUP_E2EE_PROFILE, GROUP_E2EE_SECURITY_PROFILE,
    GROUP_E2EE_TRANSPORT_PROFILE, ORIGIN_PROOF_SCHEME,
};
use serde_json::{json, Map, Value};

#[test]
fn group_e2ee_send_sanitizes_cipher_and_preserves_ids() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);

    let params = build_group_e2ee_send_rpc_params(
        &record,
        "did:wba:awiki.ai:groups:demo:e1_group",
        object_map(json!({
            "crypto_group_id_b64u": "Y3J5cHRv",
            "openmls_group_id_b64u": "provider-local",
            "epoch": "1",
            "private_message_b64u": "Y2lwaGVy",
            "epoch_authenticator": "YXV0aA",
            "group_state_ref": {
                "group_did": "did:wba:awiki.ai:groups:demo:e1_group",
                "group_state_version": "7"
            },
            "non_cryptographic": true,
            "artifact_mode": "contract-test",
            "application_plaintext": { "text": "secret" },
        })),
        "op-e2ee-send",
        "msg-e2ee-send",
    )
    .expect("group e2ee send params");

    assert_eq!(params["meta"]["profile"], GROUP_E2EE_PROFILE);
    assert_eq!(
        params["meta"]["security_profile"],
        GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        params["meta"]["content_type"],
        GROUP_E2EE_CIPHER_CONTENT_TYPE
    );
    assert_eq!(params["meta"]["operation_id"], "op-e2ee-send");
    assert_eq!(params["meta"]["message_id"], "msg-e2ee-send");
    assert_eq!(params["auth"]["scheme"], ORIGIN_PROOF_SCHEME);
    assert_origin_proof_verifies(&params, "group.e2ee.send", &record);

    let body = params["body"].as_object().expect("body object");
    for forbidden in [
        "openmls_group_id_b64u",
        "application_plaintext",
        "non_cryptographic",
        "artifact_mode",
    ] {
        assert!(
            body.get(forbidden).is_none(),
            "{forbidden} leaked: {body:?}"
        );
    }
    assert_eq!(body["crypto_group_id_b64u"], "Y3J5cHRv");
    assert_eq!(body["group_state_ref"]["group_state_version"], "7");
}

#[test]
fn group_e2ee_add_remove_and_leave_bodies_match_membership_contracts() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    let bob = "did:wba:awiki.ai:user:bob:e1_bob";

    let add = build_group_e2ee_add_rpc_params(
        &record,
        group,
        bob,
        object_map(json!({
            "crypto_group_id_b64u": "Y3J5cHRv",
            "epoch": "2",
            "epoch_authenticator": "YXV0aDI",
            "welcome_b64u": "d2VsY29tZQ",
            "commit_b64u": "Y29tbWl0",
            "ratchet_tree_b64u": "cmF0Y2hldA",
            "key_package_id": "kp-bob-1",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "12"
            },
            "group_key_package": {
                "owner_did": bob,
                "key_package_id": "kp-bob-1",
                "device_id": "phone"
            },
        })),
    )
    .expect("add params");
    assert_eq!(add["body"]["subject_did"], bob);
    assert_eq!(add["body"]["member_did"], bob);
    assert_eq!(add["body"]["key_package_id"], "kp-bob-1");
    assert_eq!(add["body"]["subject_key_package_id"], "kp-bob-1");
    assert_eq!(add["body"]["group_key_package"]["device_id"], "phone");
    assert_eq!(add["body"]["ratchet_tree_b64u"], "cmF0Y2hldA");
    assert_eq!(add["body"]["group_state_ref"]["group_state_version"], "12");
    assert_origin_proof_verifies(&add, "group.e2ee.add", &record);

    let remove = build_group_e2ee_remove_rpc_params(
        &record,
        group,
        bob,
        object_map(json!({
            "pending_commit_id": "pc-remove-1",
            "operation_id": "op-remove-1",
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": "4",
            "to_epoch": "5",
            "commit_b64u": "Y29tbWl0",
            "ratchet_tree_b64u": "cmF0Y2hldA",
            "group_info_b64u": "Z3JvdXBpbmZv",
            "epoch_authenticator_b64u": "YXV0aDU",
            "application_plaintext": "must-not-leak",
            "provider_private_material": "must-not-leak",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "8",
                "epoch": "4"
            },
        })),
        " cleanup ",
        " leave-req-1 ",
    )
    .expect("remove params");
    assert_eq!(remove["meta"]["operation_id"], "op-remove-1");
    assert_eq!(
        remove["meta"]["security_profile"],
        GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(remove["meta"]["target"]["kind"], "group");
    assert_eq!(remove["body"]["subject_did"], bob);
    assert_eq!(remove["body"]["subject_status"], "removed");
    assert_eq!(remove["body"]["commit_b64u"], "Y29tbWl0");
    assert_eq!(remove["body"]["reason_text"], "cleanup");
    assert_eq!(remove["body"]["leave_request_id"], "leave-req-1");
    assert!(remove["body"].get("application_plaintext").is_none());
    assert!(remove["body"].get("provider_private_material").is_none());
    assert_eq!(remove["body"]["group_state_ref"]["epoch"], "4");
    assert_eq!(
        remove["body"]["group_state_ref"]["group_state_version"],
        "8"
    );
    assert_origin_proof_verifies(&remove, "group.e2ee.remove", &record);

    let leave = build_group_e2ee_leave_rpc_params(
        &record,
        group,
        object_map(json!({
            "operation_id": "op-leave-1",
            "pending_commit_id": "pc-leave-1",
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": "5",
            "to_epoch": "6",
            "commit_b64u": "Y29tbWl0LWxlYXZl",
        })),
    )
    .expect("leave params");
    assert_eq!(leave["body"]["subject_did"], record.did);
    assert_eq!(leave["body"]["member_did"], record.did);
    assert_eq!(leave["body"]["subject_status"], "left");
    assert_eq!(leave["body"]["epoch"], "6");
    assert_origin_proof_verifies(&leave, "group.e2ee.leave", &record);
}

#[test]
fn group_e2ee_leave_request_uses_transport_protected_control_plane() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);

    let params = build_group_e2ee_leave_request_rpc_params(
        &record,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "done for now",
    )
    .expect("leave request params");

    assert_eq!(params["meta"]["profile"], GROUP_E2EE_PROFILE);
    assert_eq!(
        params["meta"]["security_profile"],
        GROUP_E2EE_TRANSPORT_PROFILE
    );
    assert!(params["meta"].get("message_id").is_none());
    assert_eq!(params["body"]["subject_did"], record.did);
    assert_eq!(params["body"]["member_did"], record.did);
    assert_eq!(params["body"]["subject_status"], "leave_requested");
    assert_eq!(params["body"]["reason_text"], "done for now");
    assert_origin_proof_verifies(&params, "group.e2ee.leave_request", &record);
}

#[test]
fn group_e2ee_key_package_publish_get_and_sanitize_contracts() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let service = "did:wba:awiki.ai:services:message:e1_service";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    let bob = "did:wba:awiki.ai:users:bob:e1_bob";

    let publish = build_group_e2ee_publish_key_package_rpc_params(
        &record,
        service,
        object_map(json!({
            "group_key_package": {
                "owner_did": record.did,
                "key_package_id": "kp-bob-main",
                "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
                "mls_key_package_b64u": "a3A",
                "did_wba_binding": { "agent_did": record.did },
                "device_id": "bob-main",
                "purpose": "recovery",
                "group_did": group,
                "private_key_package_b64u": "must-not-leak"
            },
        })),
    )
    .expect("publish params");
    let package = &publish["body"]["group_key_package"];
    assert_eq!(package["device_id"], "bob-main");
    assert_eq!(package["key_package_id"], "kp-bob-main");
    assert_eq!(package["purpose"], "recovery");
    assert_eq!(package["group_did"], group);
    assert!(package.get("private_key_package_b64u").is_none());
    assert_eq!(
        publish["meta"]["security_profile"],
        GROUP_E2EE_TRANSPORT_PROFILE
    );
    assert_origin_proof_verifies(&publish, "group.e2ee.publish_key_package", &record);

    let minimal_publish = build_group_e2ee_publish_key_package_rpc_params(
        &record,
        service,
        object_map(json!({
            "group_key_package": {
                "owner_did": bob,
                "device_id": "bob-main",
                "key_package_id": "kp-bob-main",
                "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
                "mls_key_package_b64u": "a3A",
                "did_wba_binding": { "agent_did": bob },
                "purpose": "",
                "group_did": ""
            },
        })),
    )
    .expect("minimal publish params");
    let minimal = &minimal_publish["body"]["group_key_package"];
    assert!(minimal.get("purpose").is_none());
    assert!(minimal.get("group_did").is_none());

    let get = build_group_e2ee_get_key_package_rpc_params(&record, service, group, bob)
        .expect("get params");
    assert_eq!(
        get["meta"]["security_profile"],
        GROUP_E2EE_TRANSPORT_PROFILE
    );
    assert_eq!(get["meta"]["target"]["kind"], "service");
    assert_eq!(get["body"]["group_did"], group);
    assert_eq!(get["body"]["target_did"], bob);
    assert_origin_proof_verifies(&get, "group.e2ee.get_key_package", &record);

    let recovery =
        build_group_e2ee_get_recovery_key_package_rpc_params(&record, service, group, bob, " ")
            .expect("recovery get params");
    assert_eq!(recovery["body"]["purpose"], "recovery");
    assert_eq!(recovery["body"]["device_id"], "default");

    let update =
        build_group_e2ee_get_update_key_package_rpc_params(&record, service, group, bob, "phone")
            .expect("update get params");
    assert_eq!(update["body"]["purpose"], "update");
    assert_eq!(update["body"]["device_id"], "phone");
}

#[test]
fn group_e2ee_create_notice_and_head_targets_match_go() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let service = "did:wba:awiki.ai:services:message:e1_service";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let create = build_group_e2ee_create_rpc_params(
        &record,
        service,
        group,
        object_map(json!({
            "crypto_group_id_b64u": "Y3J5cHRv",
            "epoch": "0",
            "epoch_authenticator": "YXV0aA",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "3"
            },
        })),
    )
    .expect("create params");
    assert_eq!(create["meta"]["target"]["kind"], "service");
    assert_eq!(create["meta"]["target"]["did"], service);
    assert_eq!(
        create["meta"]["security_profile"],
        GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(create["body"]["group_did"], group);
    assert_eq!(create["body"]["group_state_ref"]["group_did"], group);
    assert_eq!(
        create["body"]["group_state_ref"]["group_state_version"],
        "3"
    );
    assert_origin_proof_verifies(&create, "group.e2ee.create", &record);

    let notice = build_group_e2ee_notice_rpc_params(
        &record,
        group,
        500,
        true,
        vec![" notice-1 ".to_string(), " ".to_string()],
    )
    .expect("notice params");
    assert_eq!(
        notice["meta"]["security_profile"],
        GROUP_E2EE_TRANSPORT_PROFILE
    );
    assert_eq!(notice["meta"]["target"]["kind"], "agent");
    assert_eq!(notice["meta"]["target"]["did"], record.did);
    assert_eq!(notice["body"]["limit"], 100);
    assert_eq!(notice["body"]["group_did"], group);
    assert_eq!(notice["body"]["notice_ids"], json!(["notice-1"]));
    assert_eq!(notice["body"]["mark_delivered"], true);
    assert_origin_proof_verifies(&notice, "group.e2ee.notice", &record);

    let head = build_group_e2ee_head_rpc_params(&record, group).expect("head params");
    assert_eq!(head["meta"]["profile"], GROUP_E2EE_PROFILE);
    assert_eq!(
        head["meta"]["security_profile"],
        GROUP_E2EE_TRANSPORT_PROFILE
    );
    assert_eq!(head["meta"]["target"]["kind"], "group");
    assert_eq!(head["meta"]["target"]["did"], group);
    assert_eq!(head["body"]["group_did"], group);
    assert_eq!(head["body"]["group_state_ref"]["group_did"], group);
    assert_origin_proof_verifies(&head, "group.e2ee.head", &record);
}

#[test]
fn group_e2ee_recover_and_update_avoid_p4_membership_fields() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    let bob = "did:wba:awiki.ai:user:bob:e1_bob";

    let recover = build_group_e2ee_recover_member_rpc_params(
        &record,
        group,
        bob,
        "bob-main",
        object_map(json!({
            "operation_id": "op-recover-1",
            "pending_commit_id": "pc-recover-1",
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": "5",
            "to_epoch": "6",
            "commit_b64u": "Y29tbWl0",
            "welcome_b64u": "d2VsY29tZQ",
            "ratchet_tree_b64u": "cmF0Y2hldA",
            "epoch_authenticator_b64u": "YXV0aDY",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "9"
            },
            "application_plaintext": "must-not-leak",
            "member_did": "must-not-be-forwarded",
        })),
        object_map(json!({
            "key_package_id": "kp-recovery-1",
            "group_key_package": {
                "owner_did": bob,
                "purpose": "recovery",
                "device_id": "bob-main",
                "private_key_package_b64u": "must-not-leak"
            },
        })),
    )
    .expect("recover params");
    assert_eq!(recover["meta"]["operation_id"], "op-recover-1");
    assert!(recover["body"].get("member_did").is_none());
    assert!(recover["body"].get("role").is_none());
    assert_eq!(recover["body"]["target"]["agent_did"], bob);
    assert_eq!(recover["body"]["target"]["device_id"], "bob-main");
    assert_eq!(recover["body"]["recovery_key_package_id"], "kp-recovery-1");
    assert!(recover["body"].get("application_plaintext").is_none());
    assert!(recover["body"]["group_key_package"]
        .get("private_key_package_b64u")
        .is_none());
    assert_eq!(recover["body"]["group_key_package"]["purpose"], "recovery");
    assert_eq!(
        recover["body"]["group_key_package"]["device_id"],
        "bob-main"
    );
    assert_eq!(
        recover["body"]["group_state_ref"]["group_state_version"],
        "9"
    );
    assert_origin_proof_verifies(&recover, "group.e2ee.recover_member", &record);

    let update = build_group_e2ee_update_member_rpc_params(
        &record,
        group,
        bob,
        "",
        object_map(json!({
            "operation_id": "op-update-1",
            "pending_commit_id": "pc-update-1",
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": "6",
            "to_epoch": "7",
            "commit_b64u": "Y29tbWl0LXVwZGF0ZQ",
            "update_key_package_id": "kp-update-1",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "10"
            },
        })),
        object_map(json!({
            "key_package_id": "kp-fallback",
            "group_key_package": {
                "owner_did": bob,
                "device_id": "default",
                "private_key_package_b64u": "must-not-leak"
            },
        })),
    )
    .expect("update params");
    assert_eq!(update["meta"]["operation_id"], "op-update-1");
    assert_eq!(update["body"]["target"]["agent_did"], bob);
    assert_eq!(update["body"]["target"]["device_id"], "default");
    assert_eq!(update["body"]["update_key_package_id"], "kp-update-1");
    assert!(update["body"].get("recovery_key_package_id").is_none());
    assert_eq!(update["body"]["group_key_package"]["purpose"], "update");
    assert!(update["body"]["group_key_package"]
        .get("private_key_package_b64u")
        .is_none());
    assert_eq!(update["body"]["group_state_ref"]["epoch"], "6");
    assert_origin_proof_verifies(&update, "group.e2ee.update", &record);
}

#[test]
fn group_e2ee_wire_validation_errors_match_go_contracts() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");

    assert_eq!(
        build_group_e2ee_create_rpc_params(&record, "", "group", Map::new()).unwrap_err(),
        MessageError::GroupRequired
    );
    assert_eq!(
        build_group_e2ee_publish_key_package_rpc_params(&record, "", Map::new()).unwrap_err(),
        MessageError::MissingMessageServiceDid
    );
    assert_eq!(
        build_group_e2ee_publish_key_package_rpc_params(
            &record,
            "did:wba:awiki.ai:services:message:e1_service",
            Map::new(),
        )
        .unwrap_err()
        .to_string(),
        "group_key_package is required"
    );
    assert_eq!(
        build_group_e2ee_get_key_package_rpc_params(
            &record,
            "did:wba:awiki.ai:services:message:e1_service",
            "group",
            "",
        )
        .unwrap_err(),
        MessageError::MemberRequired
    );
    assert_eq!(
        build_group_e2ee_get_key_package_rpc_params(
            &record,
            "did:wba:awiki.ai:services:message:e1_service",
            "",
            "member",
        )
        .unwrap_err(),
        MessageError::GroupRequired
    );
    assert_eq!(
        build_group_e2ee_head_rpc_params(&record, " ").unwrap_err(),
        MessageError::GroupRequired
    );
}

fn record(did: &str) -> StoredIdentity {
    StoredIdentity {
        did: did.to_string(),
        ..StoredIdentity::default()
    }
}

fn generated_record(identity_name: &str, generated: &GeneratedIdentity) -> StoredIdentity {
    StoredIdentity {
        identity_name: identity_name.to_string(),
        did: generated.did.clone(),
        unique_id: generated.unique_id.clone(),
        did_document: Some(generated.did_document.clone()),
        key1_private_pem: generated.key1_private_pem.clone(),
        key1_public_pem: generated.key1_public_pem.clone(),
        ..StoredIdentity::default()
    }
}

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn assert_origin_proof_verifies(params: &Value, method: &str, record: &StoredIdentity) {
    let origin_proof: anp::proof::Rfc9421OriginProof =
        serde_json::from_value(params["auth"]["origin_proof"].clone())
            .expect("origin proof should deserialize");
    verify_rfc9421_origin_proof(
        &origin_proof,
        method,
        &params["meta"],
        &params["body"],
        Rfc9421OriginProofVerificationOptions {
            did_document: record.did_document.clone(),
            expected_signer_did: Some(record.did.clone()),
            ..Rfc9421OriginProofVerificationOptions::default()
        },
    )
    .expect("origin proof verifies");
}
