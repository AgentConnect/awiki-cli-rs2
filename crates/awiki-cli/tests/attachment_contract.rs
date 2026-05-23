use awiki_cli::identity::types::StoredIdentity;
use awiki_cli::message::{
    attachment_manifest_content_type, build_attachment_commit_object_rpc_params,
    build_attachment_create_slot_rpc_params, build_attachment_download_ticket_rpc_params,
    build_attachment_manifest, find_attachment_selection, manifest_content_string,
    select_attachment_rpc_service_from_document, AttachmentCreateSlotResult, AttachmentSelection,
    MessageError, PreparedAttachment,
};
use serde_json::json;

#[test]
fn attachment_manifest_and_wire_params_match_transport_contract() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");
    let prepared = PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_bytes: 5,
        size_string: "5".to_string(),
        digest_b64u: "digest".to_string(),
        payload: b"hello".to_vec(),
        ..PreparedAttachment::default()
    };

    let create_slot = build_attachment_create_slot_rpc_params(
        &record,
        "did:wba:awiki.ai:services:message:e1",
        "agent",
        "did:wba:awiki.ai:user:bob:e1_bob",
        &prepared,
    )
    .expect("create-slot params");
    assert_eq!(create_slot["meta"]["profile"], "anp.attachment.v1");
    assert_eq!(
        create_slot["meta"]["target"],
        json!({ "kind": "service", "did": "did:wba:awiki.ai:services:message:e1" })
    );
    assert_eq!(create_slot["body"]["filename"], "hello.txt");
    assert_eq!(create_slot["body"]["expected_size"], "5");
    assert_eq!(
        create_slot["body"]["expected_digest"],
        json!({ "alg": "sha-256", "value_b64u": "digest" })
    );
    assert_eq!(
        create_slot["body"]["intended_target"],
        json!({ "kind": "agent", "did": "did:wba:awiki.ai:user:bob:e1_bob" })
    );
    assert_eq!(create_slot.get("auth"), None);

    let slot = AttachmentCreateSlotResult {
        attachment_id: "att-1".to_string(),
        slot_id: "slot-1".to_string(),
        object_uri: "https://objects.example/obj-1".to_string(),
        commit_token: "commit-token".to_string(),
        ..AttachmentCreateSlotResult::default()
    };
    let commit = build_attachment_commit_object_rpc_params(
        &record,
        "did:wba:awiki.ai:services:message:e1",
        &prepared,
        &slot,
    )
    .expect("commit params");
    assert_eq!(commit["meta"]["profile"], "anp.attachment.v1");
    assert_eq!(commit["body"]["attachment_id"], "att-1");
    assert_eq!(commit["body"]["slot_id"], "slot-1");
    assert_eq!(commit["body"]["commit_token"], "commit-token");
    assert_eq!(commit["body"]["object_encryption_mode"], "none");

    let manifest = build_attachment_manifest(&prepared, &slot, "hello");
    assert_eq!(
        attachment_manifest_content_type(),
        "application/anp-attachment-manifest+json"
    );
    assert_eq!(manifest["primary_attachment_id"], "att-1");
    assert_eq!(manifest["caption"], "hello");
    assert_eq!(
        manifest["attachments"][0],
        json!({
            "attachment_id": "att-1",
            "filename": "hello.txt",
            "mime_type": "text/plain",
            "size": "5",
            "digest": { "alg": "sha-256", "value_b64u": "digest" },
            "access_info": { "object_uri": "https://objects.example/obj-1" },
            "encryption_info": { "mode": "none" },
        })
    );
    assert!(manifest_content_string(&manifest).contains("\"primary_attachment_id\":\"att-1\""));
}

#[test]
fn attachment_download_ticket_params_distinguish_direct_and_group_targets() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");
    let selection = AttachmentSelection {
        attachment_id: "att-1".to_string(),
        object_uri: "https://objects.example/obj-1".to_string(),
        ..AttachmentSelection::default()
    };

    let direct = build_attachment_download_ticket_rpc_params(
        &record,
        "did:wba:awiki.ai:services:attachment:e1",
        "did:wba:awiki.ai:user:bob:e1_bob",
        "msg-1",
        "",
        &selection,
    )
    .expect("direct ticket params");
    assert_eq!(direct["meta"]["profile"], "anp.attachment.v1");
    assert_eq!(
        direct["meta"]["target"],
        json!({ "kind": "service", "did": "did:wba:awiki.ai:services:attachment:e1" })
    );
    assert_eq!(direct["body"]["attachment_id"], "att-1");
    assert_eq!(
        direct["body"]["object_uri"],
        "https://objects.example/obj-1"
    );
    assert_eq!(
        direct["body"]["sender_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(direct["body"]["requester_did"], record.did);
    assert_eq!(direct["body"]["message_target_did"], record.did);
    assert_eq!(direct["body"].get("group_did"), None);
    assert_eq!(direct["body"]["one_time"], true);
    assert_eq!(direct.get("auth"), None);

    let group = build_attachment_download_ticket_rpc_params(
        &record,
        "did:wba:awiki.ai:services:attachment:e1",
        "did:wba:awiki.ai:user:bob:e1_bob",
        "group-msg-1",
        "did:wba:awiki.ai:groups:demo:e1_group",
        &selection,
    )
    .expect("group ticket params");
    assert_eq!(
        group["body"]["group_did"],
        "did:wba:awiki.ai:groups:demo:e1_group"
    );
    assert_eq!(group["body"].get("message_target_did"), None);
}

#[test]
fn attachment_selection_errors_preserve_cli_message_contract() {
    let multiple = vec![json!({
        "id": "msg-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "content": {
            "attachments": [
                {"attachment_id": "att-1", "access_info": {"object_uri": "https://objects.example/1"}},
                {"attachment_id": "att-2", "access_info": {"object_uri": "https://objects.example/2"}}
            ],
            "primary_attachment_id": "att-1"
        }
    })];
    assert_eq!(
        find_attachment_selection(&multiple, "msg-1", "").unwrap_err(),
        MessageError::AttachmentIdRequired
    );
    assert_eq!(
        find_attachment_selection(&multiple, "msg-1", "missing").unwrap_err(),
        MessageError::AttachmentNotFound
    );

    let invalid_manifest = vec![json!({
        "id": "msg-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "content": {"text": "not an attachment manifest"}
    })];
    assert_eq!(
        find_attachment_selection(&invalid_manifest, "msg-1", "").unwrap_err(),
        MessageError::AttachmentMessageInvalid
    );
    assert_eq!(
        find_attachment_selection(&invalid_manifest, "missing", "").unwrap_err(),
        MessageError::MessageNotFound
    );
}

#[test]
fn attachment_service_discovery_filters_profile_and_endpoint_contract() {
    let document = json!({
        "service": [
            {
                "id": "#direct",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/direct/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.direct.base.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 1
            },
            {
                "id": "#primary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/attachment/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": "2"
            },
            {
                "id": "#secondary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/secondary/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 9
            }
        ]
    });

    let service =
        select_attachment_rpc_service_from_document("did:wba:example.com:user:alice:e1", &document)
            .expect("attachment service");
    assert_eq!(service.sender_did, "did:wba:example.com:user:alice:e1");
    assert_eq!(service.service_did, "did:wba:example.com");
    assert_eq!(service.rpc_endpoint, "https://example.com/attachment/rpc");

    let invalid_endpoint = json!({
        "service": [{
            "type": "ANPMessageService",
            "serviceEndpoint": "example.com/attachment/rpc",
            "serviceDid": "did:wba:example.com",
            "profiles": ["anp.attachment.v1"],
            "securityProfiles": ["transport-protected"]
        }]
    });
    assert_eq!(
        select_attachment_rpc_service_from_document(
            "did:wba:example.com:user:alice:e1",
            &invalid_endpoint,
        )
        .unwrap_err(),
        MessageError::InvalidAttachmentServiceEndpoint("missing protocol scheme".to_string())
    );
}

#[test]
fn attachment_wire_builder_errors_preserve_legacy_adapter_variants() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");
    let prepared = PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_string: "5".to_string(),
        digest_b64u: "digest".to_string(),
        ..PreparedAttachment::default()
    };
    let slot = AttachmentCreateSlotResult {
        attachment_id: "att-1".to_string(),
        slot_id: "slot-1".to_string(),
        object_uri: "https://objects.example/obj-1".to_string(),
        commit_token: "commit-token".to_string(),
        ..AttachmentCreateSlotResult::default()
    };

    assert_eq!(
        build_attachment_create_slot_rpc_params(&record, "", "agent", "did:wba:target", &prepared)
            .unwrap_err(),
        MessageError::MissingMessageServiceDid
    );
    assert_eq!(
        build_attachment_create_slot_rpc_params(
            &record,
            "did:wba:service",
            "",
            "did:wba:target",
            &prepared,
        )
        .unwrap_err(),
        MessageError::TargetRequired
    );
    assert_eq!(
        build_attachment_download_ticket_rpc_params(
            &record,
            "",
            "did:wba:sender",
            "msg-1",
            "",
            &AttachmentSelection {
                attachment_id: "att-1".to_string(),
                object_uri: "https://objects.example/obj-1".to_string(),
                ..AttachmentSelection::default()
            },
        )
        .unwrap_err(),
        MessageError::MissingAttachmentServiceDid
    );
    assert_eq!(
        build_attachment_download_ticket_rpc_params(
            &record,
            "did:wba:service",
            "",
            "msg-1",
            "",
            &AttachmentSelection {
                attachment_id: "att-1".to_string(),
                object_uri: "https://objects.example/obj-1".to_string(),
                ..AttachmentSelection::default()
            },
        )
        .unwrap_err(),
        MessageError::AttachmentSenderRequired
    );
    assert_eq!(
        build_attachment_download_ticket_rpc_params(
            &record,
            "did:wba:service",
            "did:wba:sender",
            "msg-1",
            "",
            &AttachmentSelection::default(),
        )
        .unwrap_err(),
        MessageError::AttachmentNotFound
    );
    assert_eq!(
        build_attachment_commit_object_rpc_params(
            &record,
            "did:wba:service",
            &PreparedAttachment::default(),
            &slot,
        )
        .unwrap_err(),
        MessageError::FilePathRequired
    );
}

fn record(did: &str) -> StoredIdentity {
    StoredIdentity {
        did: did.to_string(),
        ..StoredIdentity::default()
    }
}
