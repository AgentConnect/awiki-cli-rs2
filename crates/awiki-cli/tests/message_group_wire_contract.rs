use anp::proof::{verify_rfc9421_origin_proof, Rfc9421OriginProofVerificationOptions};
use awiki_cli::identity::generate_identity;
use awiki_cli::identity::types::{GeneratedIdentity, StoredIdentity};
use awiki_cli::message::{
    build_group_add_rpc_params, build_group_create_rpc_params, build_group_get_info_rpc_params,
    build_group_get_rpc_params, build_group_join_rpc_params, build_group_leave_rpc_params,
    build_group_list_rpc_params, build_group_members_rpc_params, build_group_messages_rpc_params,
    build_group_remove_rpc_params, build_group_send_rpc_params,
    build_group_update_policy_rpc_params, build_group_update_profile_rpc_params,
    GroupCreateRequest, GroupGetRequest, GroupInfoRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberRequest, GroupMembersRequest, GroupMessagesRequest, MessageError,
    GROUP_E2EE_SECURITY_PROFILE, ORIGIN_PROOF_SCHEME,
};
use serde_json::{json, Map, Value};

#[test]
fn group_create_params_use_origin_proof_and_default_policy_contract() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);

    let params = build_group_create_rpc_params(
        &record,
        "did:wba:awiki.ai:services:message:e1_service",
        GroupCreateRequest {
            name: " Protocol Review ".to_string(),
            ..GroupCreateRequest::default()
        },
    )
    .expect("group create params");

    assert_eq!(params["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        params["meta"]["target"],
        json!({ "kind": "service", "did": "did:wba:awiki.ai:services:message:e1_service" })
    );
    assert_eq!(params["meta"]["content_type"], "application/json");
    assert_eq!(params["auth"]["scheme"], ORIGIN_PROOF_SCHEME);
    assert!(params["auth"]["origin_proof"].is_object());
    assert!(params["auth"].get("actor_proof").is_none());
    assert_origin_proof_verifies(&params, "group.create", &record);

    let profile = &params["body"]["group_profile"];
    assert_eq!(profile["display_name"], "Protocol Review");
    let policy = &params["body"]["group_policy"];
    assert_eq!(policy["admission_mode"], "open-join");
    assert_eq!(policy["attachments_allowed"], true);
    assert_eq!(policy["max_members"], "500");
    assert_eq!(policy["message_security_profile"], "transport-protected");
    assert_eq!(policy["bootstrap_security_profile"], "transport-protected");
    assert_eq!(policy["permissions"]["send"], "member");
    assert_eq!(policy["permissions"]["update_policy"], "owner");
}

#[test]
fn group_create_params_apply_patches_and_e2ee_security_profile() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);

    let params = build_group_create_rpc_params(
        &record,
        "did:wba:awiki.ai:services:message:e1_service",
        GroupCreateRequest {
            name: "Encrypted Group".to_string(),
            description: "description".to_string(),
            discoverability: "public".to_string(),
            admission_mode: "invite-only".to_string(),
            e2ee: true,
            slug: "encrypted-group".to_string(),
            goal: "ship".to_string(),
            rules: "be kind".to_string(),
            message_prompt: "answer clearly".to_string(),
            doc_url: "https://example.com/group".to_string(),
            attachments_allowed: Some(false),
            max_members: "25".to_string(),
            member_max_messages: Some(7),
            member_max_total_chars: Some(1024),
            ..GroupCreateRequest::default()
        },
    )
    .expect("group create params");

    let profile = &params["body"]["group_profile"];
    assert_eq!(profile["display_name"], "Encrypted Group");
    assert_eq!(profile["description"], "description");
    assert_eq!(profile["discoverability"], "public");
    assert_eq!(profile["slug"], "encrypted-group");
    assert_eq!(profile["goal"], "ship");
    assert_eq!(profile["rules"], "be kind");
    assert_eq!(profile["message_prompt"], "answer clearly");
    assert_eq!(profile["doc_url"], "https://example.com/group");

    let policy = &params["body"]["group_policy"];
    assert_eq!(policy["admission_mode"], "invite-only");
    assert_eq!(policy["attachments_allowed"], false);
    assert_eq!(policy["max_members"], "25");
    assert_eq!(policy["member_max_messages"], 7);
    assert_eq!(policy["member_max_total_chars"], 1024);
    assert_eq!(
        policy["message_security_profile"],
        GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        policy["bootstrap_security_profile"],
        GROUP_E2EE_SECURITY_PROFILE
    );
}

#[test]
fn group_base_lifecycle_params_match_go_contracts() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let info = build_group_get_info_rpc_params(
        &record,
        GroupInfoRequest {
            group: format!(" {group} "),
            include_policy: true,
            include_member_list: true,
            ..GroupInfoRequest::default()
        },
    )
    .expect("group info params");
    assert_eq!(info["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        info["meta"]["target"],
        json!({ "kind": "group", "did": group })
    );
    assert_eq!(info["body"]["include_policy"], true);
    assert_eq!(info["body"]["include_member_list"], true);
    assert!(info.get("auth").is_none());

    let join = build_group_join_rpc_params(
        &record,
        GroupJoinRequest {
            group: group.to_string(),
            reason_text: " please ".to_string(),
            ..GroupJoinRequest::default()
        },
    )
    .expect("join params");
    assert_eq!(join["body"], json!({ "reason_text": "please" }));
    assert_origin_proof_verifies(&join, "group.join", &record);

    let add = build_group_add_rpc_params(
        &record,
        GroupMemberRequest {
            group: group.to_string(),
            member: " did:wba:awiki.ai:user:bob:e1_bob ".to_string(),
            role: " admin ".to_string(),
            reason_text: " invite ".to_string(),
            ..GroupMemberRequest::default()
        },
    )
    .expect("add params");
    assert_eq!(
        add["body"]["member_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(add["body"]["role"], "admin");
    assert_eq!(add["body"]["reason_text"], "invite");
    assert_origin_proof_verifies(&add, "group.add", &record);

    let remove = build_group_remove_rpc_params(
        &record,
        GroupMemberRequest {
            group: group.to_string(),
            member: "did:wba:awiki.ai:user:bob:e1_bob".to_string(),
            reason_text: "cleanup".to_string(),
            ..GroupMemberRequest::default()
        },
    )
    .expect("remove params");
    assert_eq!(
        remove["body"]["member_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(remove["body"]["reason_text"], "cleanup");
    assert!(remove["body"].get("role").is_none());
    assert_origin_proof_verifies(&remove, "group.remove", &record);

    let leave = build_group_leave_rpc_params(
        &record,
        GroupLeaveRequest {
            group: group.to_string(),
            reason_text: "ignored by base leave".to_string(),
            ..GroupLeaveRequest::default()
        },
    )
    .expect("leave params");
    assert_eq!(leave["body"], json!({}));
    assert_origin_proof_verifies(&leave, "group.leave", &record);
}

#[test]
fn group_update_and_send_params_match_go_contracts() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("alice", &generated);
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let profile_patch = object_map(json!({ "display_name": "Renamed" }));
    let profile = build_group_update_profile_rpc_params(&record, group, profile_patch)
        .expect("profile update params");
    assert_eq!(
        profile["body"]["group_profile_patch"],
        json!({ "display_name": "Renamed" })
    );
    assert_origin_proof_verifies(&profile, "group.update_profile", &record);

    let policy_patch = object_map(json!({ "admission_mode": "invite-only" }));
    let policy =
        build_group_update_policy_rpc_params(&record, group, policy_patch).expect("policy update");
    assert_eq!(
        policy["body"]["group_policy_patch"],
        json!({ "admission_mode": "invite-only" })
    );
    assert_origin_proof_verifies(&policy, "group.update_policy", &record);

    let send = build_group_send_rpc_params(&record, group, " hello group ", " Event ")
        .expect("group send params");
    assert_eq!(send["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        send["meta"]["target"],
        json!({ "kind": "group", "did": group })
    );
    assert_eq!(send["meta"]["content_type"], "application/json");
    assert!(send["meta"]["message_id"]
        .as_str()
        .expect("message id")
        .starts_with("msg-"));
    assert_eq!(send["body"], json!({ "text": " hello group " }));
    assert_origin_proof_verifies(&send, "group.send", &record);
}

#[test]
fn group_local_params_match_go_contracts_and_defaults() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let get = build_group_get_rpc_params(
        &record,
        GroupGetRequest {
            group: group.to_string(),
            ..GroupGetRequest::default()
        },
    )
    .expect("group get params");
    assert_eq!(get["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(
        get["meta"]["target"],
        json!({ "kind": "group", "did": group })
    );
    assert_eq!(get["body"]["group_did"], group);
    assert!(get["meta"].get("operation_id").is_none());

    let list = build_group_list_rpc_params(&record, GroupListRequest::default());
    assert_eq!(list["meta"]["profile"], "anp.group.local.v1");
    assert!(list["meta"].get("target").is_none());
    assert_eq!(list["body"]["limit"], 50);

    let members = build_group_members_rpc_params(
        &record,
        GroupMembersRequest {
            group: group.to_string(),
            ..GroupMembersRequest::default()
        },
    )
    .expect("members params");
    assert_eq!(members["body"]["limit"], 100);
    assert_eq!(members["body"]["group_did"], group);

    let messages = build_group_messages_rpc_params(
        &record,
        GroupMessagesRequest {
            group: group.to_string(),
            limit: 25,
            cursor: " 12 ".to_string(),
            skip: 50,
            ..GroupMessagesRequest::default()
        },
    )
    .expect("messages params");
    assert_eq!(messages["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(messages["body"]["limit"], 25);
    assert_eq!(messages["body"]["since_seq"], "12");
    assert_eq!(messages["body"]["skip"], 50);

    let default_messages = build_group_messages_rpc_params(
        &record,
        GroupMessagesRequest {
            group: group.to_string(),
            ..GroupMessagesRequest::default()
        },
    )
    .expect("default messages params");
    assert_eq!(default_messages["body"]["limit"], 50);
    assert!(default_messages["body"].get("since_seq").is_none());
    assert!(default_messages["body"].get("skip").is_none());
}

#[test]
fn group_wire_validation_errors_match_go_contracts() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");

    assert_eq!(
        build_group_get_info_rpc_params(&record, GroupInfoRequest::default()).unwrap_err(),
        MessageError::GroupRequired
    );
    assert_eq!(
        build_group_add_rpc_params(&record, GroupMemberRequest::default()).unwrap_err(),
        MessageError::MemberRequired
    );
    assert_eq!(
        build_group_send_rpc_params(&record, "", "hello", "text").unwrap_err(),
        MessageError::GroupRequired
    );
    assert_eq!(
        build_group_send_rpc_params(
            &record,
            "did:wba:awiki.ai:groups:demo:e1_group",
            " ",
            "text"
        )
        .unwrap_err(),
        MessageError::TextRequired
    );
    assert_eq!(
        build_group_create_rpc_params(&record, "", GroupCreateRequest::default()).unwrap_err(),
        MessageError::MissingMessageServiceDid
    );

    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let signed_record = generated_record("alice", &generated);
    let missing_name = build_group_create_rpc_params(
        &signed_record,
        "did:wba:awiki.ai:services:message:e1_service",
        GroupCreateRequest::default(),
    )
    .unwrap_err();
    assert_eq!(missing_name.to_string(), "group display name is required");
    assert_eq!(
        build_group_update_profile_rpc_params(&signed_record, "group", Map::new())
            .unwrap_err()
            .to_string(),
        "group profile patch is required"
    );
    assert_eq!(
        build_group_update_policy_rpc_params(&signed_record, "group", Map::new())
            .unwrap_err()
            .to_string(),
        "group policy patch is required"
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
    assert_service_meta_timestamp_compatible(&params["meta"]);
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

fn assert_service_meta_timestamp_compatible(meta: &Value) {
    let created_at = meta["created_at"].as_str().expect("created_at");
    assert_eq!(created_at.len(), "2026-05-15T09:33:18Z".len());
    assert!(created_at.ends_with('Z'));
    assert!(!created_at.contains('.'));
}
