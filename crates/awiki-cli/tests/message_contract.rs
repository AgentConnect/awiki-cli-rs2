use anp::proof::{
    build_im_content_digest, build_signed_request_object, canonicalize_signed_request_object,
    verify_rfc9421_origin_proof, Rfc9421OriginProofVerificationOptions,
};
use awiki_cli::identity::generate_identity;
use awiki_cli::identity::types::{GeneratedIdentity, StoredIdentity};
use awiki_cli::message::{
    attachment_manifest_content_type, build_attachment_create_slot_rpc_params,
    build_attachment_download_ticket_rpc_params, build_attachment_manifest,
    build_direct_text_payload, build_history_rpc_params, build_inbox_rpc_params,
    build_mark_read_rpc_params, build_origin_proof, content_type_for_message_type,
    find_attachment_selection, manifest_content_string, origin_auth_value,
    select_attachment_rpc_service_from_document, verification_method_id_from_document,
    websocket_cache_fallback_warning, websocket_http_fallback_warning, AttachmentCreateSlotResult,
    AttachmentSelection, HistoryRequest, InboxRequest, MarkReadRequest, MessageError,
    PreparedAttachment, ORIGIN_PROOF_SCHEME,
};
use serde_json::{json, Value};

#[test]
fn message_inbox_history_and_mark_read_params_match_go_contracts() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");

    let inbox = build_inbox_rpc_params(&record, InboxRequest::default());
    assert_eq!(inbox["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(inbox["meta"]["security_profile"], "transport-protected");
    assert_eq!(inbox["meta"]["sender_did"], record.did);
    assert_has_generated_meta(&inbox["meta"]);
    assert_eq!(inbox["body"]["user_did"], record.did);
    assert_eq!(inbox["body"]["limit"], 20);

    assert_eq!(
        build_history_rpc_params(&record, HistoryRequest::default()).unwrap_err(),
        MessageError::TargetRequired
    );
    let history = build_history_rpc_params(
        &record,
        HistoryRequest {
            with: "did:wba:awiki.ai:user:bob:e1_bob".to_string(),
            limit: 0,
            cursor: "42".to_string(),
            skip: 3,
            ..HistoryRequest::default()
        },
    )
    .expect("history params");
    assert_eq!(history["meta"]["profile"], "anp.direct.local.v1");
    assert_eq!(
        history["body"]["peer_did"],
        "did:wba:awiki.ai:user:bob:e1_bob"
    );
    assert_eq!(history["body"]["limit"], 50);
    assert_eq!(history["body"]["since_seq"], "42");
    assert_eq!(history["body"]["skip"], 3);

    assert!(build_mark_read_rpc_params(&record, MarkReadRequest::default()).is_err());
    let mark_read = build_mark_read_rpc_params(
        &record,
        MarkReadRequest {
            message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
            ..MarkReadRequest::default()
        },
    )
    .expect("mark-read params");
    assert_eq!(mark_read["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(mark_read["body"]["message_ids"], json!(["msg-1", "msg-2"]));
}

#[test]
fn direct_text_payload_and_message_type_content_match_go_contracts() {
    assert_eq!(
        content_type_for_message_type("attachment_manifest"),
        attachment_manifest_content_type()
    );
    assert_eq!(content_type_for_message_type("event"), "application/json");
    assert_eq!(content_type_for_message_type("unknown"), "text/plain");

    let payload = build_direct_text_payload(
        "did:wba:awiki.ai:user:alice:e1",
        "did:wba:awiki.ai:user:bob:e1",
        "hello",
        "text/plain",
    )
    .expect("direct payload");
    assert_eq!(payload.method, "direct.send");
    assert_eq!(payload.meta["profile"], "anp.direct.base.v1");
    assert_eq!(
        payload.meta["target"],
        json!({
            "kind": "agent",
            "did": "did:wba:awiki.ai:user:bob:e1",
        })
    );
    assert_eq!(payload.body, json!({ "text": "hello" }));
    assert_has_generated_meta(&payload.meta);
    assert!(payload.meta["message_id"]
        .as_str()
        .unwrap()
        .starts_with("msg-"));
}

#[test]
fn origin_proof_matches_rfc9421_contract_and_uses_auth_verification_method() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let record = generated_record("default", &generated);
    let payload = build_direct_text_payload(
        &generated.did,
        "did:wba:awiki.ai:user:bob:e1_bob",
        "hello",
        "text/plain",
    )
    .expect("direct payload");

    let key_id =
        verification_method_id_from_document(record.did_document.as_ref().expect("did document"))
            .expect("verification method id");
    assert_eq!(
        key_id,
        generated.did_document["authentication"][0]
            .as_str()
            .expect("authentication method")
    );

    let origin_proof = build_origin_proof(&record, &payload).expect("origin proof");
    let auth = origin_auth_value(&origin_proof);
    assert_eq!(auth["scheme"], ORIGIN_PROOF_SCHEME);
    assert!(auth["origin_proof"]["contentDigest"]
        .as_str()
        .unwrap()
        .starts_with("sha-256=:"));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .unwrap()
        .contains("\"@method\""));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .unwrap()
        .contains("\"@target-uri\""));
    assert!(auth["origin_proof"]["signatureInput"]
        .as_str()
        .unwrap()
        .contains("\"content-digest\""));
    assert!(auth["origin_proof"]["signature"]
        .as_str()
        .unwrap()
        .starts_with("sig1=:"));

    let signed_request = build_signed_request_object(&payload.method, &payload.meta, &payload.body)
        .expect("signed request");
    let canonical = canonicalize_signed_request_object(&signed_request).expect("canonical");
    assert_eq!(
        origin_proof.content_digest,
        build_im_content_digest(&canonical)
    );
    verify_rfc9421_origin_proof(
        &origin_proof,
        &payload.method,
        &payload.meta,
        &payload.body,
        Rfc9421OriginProofVerificationOptions {
            did_document: record.did_document.clone(),
            expected_signer_did: Some(generated.did.clone()),
            ..Rfc9421OriginProofVerificationOptions::default()
        },
    )
    .expect("origin proof verifies against did document");
}

#[test]
fn origin_proof_reports_missing_verification_method_like_go_contract() {
    let generated =
        generate_identity("awiki.ai", "", "").expect("generated identity should be valid");
    let mut record = generated_record("broken", &generated);
    record.did_document = Some(json!({ "id": generated.did }));
    let payload = build_direct_text_payload(
        &generated.did,
        "did:wba:awiki.ai:user:bob:e1_bob",
        "hello",
        "text/plain",
    )
    .expect("direct payload");

    let error = build_origin_proof(&record, &payload).unwrap_err();
    assert_eq!(
        error.to_string(),
        "identity broken is missing an authentication verification method"
    );

    let fallback = json!({
        "verificationMethod": [{
            "id": "did:wba:awiki.ai:user:alice:e1#key-1"
        }]
    });
    assert_eq!(
        verification_method_id_from_document(&fallback).as_deref(),
        Some("did:wba:awiki.ai:user:alice:e1#key-1")
    );

    let empty_auth_takes_precedence = json!({
        "authentication": [""],
        "verificationMethod": [{
            "id": "did:wba:awiki.ai:user:alice:e1#key-1"
        }]
    });
    assert_eq!(
        verification_method_id_from_document(&empty_auth_takes_precedence).as_deref(),
        Some("")
    );
    let mut empty_auth_record = generated_record("empty-auth", &generated);
    empty_auth_record.did_document = Some(empty_auth_takes_precedence);
    let error = build_origin_proof(&empty_auth_record, &payload).unwrap_err();
    assert_eq!(
        error.to_string(),
        "identity empty-auth is missing an authentication verification method"
    );
}

#[test]
fn attachment_rpc_params_and_manifest_match_go_contracts() {
    let record = record("did:wba:awiki.ai:user:alice:e1_alice");
    let prepared = PreparedAttachment {
        filename: "hello.txt".to_string(),
        mime_type: "text/plain".to_string(),
        size_string: "5".to_string(),
        digest_b64u: "digest".to_string(),
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
    assert_eq!(create_slot["body"]["expected_size"], "5");
    assert_eq!(
        create_slot["body"]["expected_digest"]["value_b64u"],
        "digest"
    );
    assert_eq!(
        create_slot["body"]["intended_target"],
        json!({ "kind": "agent", "did": "did:wba:awiki.ai:user:bob:e1_bob" })
    );
    assert_eq!(create_slot.get("auth"), None);

    let slot = AttachmentCreateSlotResult {
        attachment_id: "att-1".to_string(),
        object_uri: "http://127.0.0.1:8080/objects/obj-1".to_string(),
        ..AttachmentCreateSlotResult::default()
    };
    let manifest = build_attachment_manifest(&prepared, &slot, "hello");
    assert_eq!(manifest["primary_attachment_id"], "att-1");
    assert_eq!(manifest["caption"], "hello");
    assert_eq!(
        manifest["attachments"][0]["digest"],
        json!({ "alg": "sha-256", "value_b64u": "digest" })
    );
    assert!(manifest_content_string(&manifest).contains("\"primary_attachment_id\":\"att-1\""));

    let ticket = build_attachment_download_ticket_rpc_params(
        &record,
        "did:wba:awiki.ai",
        "did:wba:awiki.ai:user:alice:e1",
        "msg-1",
        "",
        &AttachmentSelection {
            attachment_id: "att-1".to_string(),
            object_uri: "http://127.0.0.1:8080/objects/obj-1".to_string(),
            ..AttachmentSelection::default()
        },
    )
    .expect("download ticket params");
    assert_eq!(
        ticket["body"]["sender_did"],
        "did:wba:awiki.ai:user:alice:e1"
    );
    assert_eq!(ticket["body"]["requester_did"], record.did);
    assert_eq!(ticket["body"]["message_target_did"], record.did);
    assert_eq!(ticket.get("auth"), None);
}

#[test]
fn attachment_selection_matches_visible_or_raw_message_id() {
    let messages = vec![json!({
        "id": "did:wba:awiki.ai:groups:test:e1_group:7",
        "message_id": "msg-raw-1",
        "sender_did": "did:wba:awiki.ai:user:alice:e1",
        "content": {
            "attachments": [{
                "attachment_id": "att-1",
                "filename": "hello.txt",
                "mime_type": "text/plain",
                "size": "5",
                "digest": { "alg": "sha-256", "value_b64u": "digest" },
                "access_info": { "object_uri": "http://127.0.0.1:8080/objects/obj-1" }
            }],
            "primary_attachment_id": "att-1",
            "caption": "hello"
        }
    })];

    let selection =
        find_attachment_selection(&messages, "did:wba:awiki.ai:groups:test:e1_group:7", "")
            .expect("selection");
    assert_eq!(selection.message_id, "msg-raw-1");
    assert_eq!(
        selection.requested_id,
        "did:wba:awiki.ai:groups:test:e1_group:7"
    );
    assert_eq!(selection.attachment_id, "att-1");
    assert_eq!(selection.sender_did, "did:wba:awiki.ai:user:alice:e1");
    assert_eq!(selection.caption, "hello");
}

#[test]
fn attachment_service_discovery_matches_go_priority_and_filtering() {
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
                "id": "#secondary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/secondary/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": 9
            },
            {
                "id": "#primary",
                "type": "ANPMessageService",
                "serviceEndpoint": "https://example.com/primary/rpc",
                "serviceDid": "did:wba:example.com",
                "profiles": ["anp.attachment.v1"],
                "securityProfiles": ["transport-protected"],
                "priority": "2"
            }
        ]
    });
    let service =
        select_attachment_rpc_service_from_document("did:wba:example.com:user:alice:e1", &document)
            .expect("service");
    assert_eq!(service.rpc_endpoint, "https://example.com/primary/rpc");
    assert_eq!(service.service_did, "did:wba:example.com");
    assert_eq!(service.sender_did, "did:wba:example.com:user:alice:e1");
}

#[test]
fn websocket_fallback_warnings_use_readable_transport_details() {
    let err = MessageError::transport_unavailable(
        "local websocket bridge request failed: websocket session is not connected for identity zhuocheng",
    );

    assert_eq!(
        websocket_http_fallback_warning(Some(&err)),
        "WebSocket listener was unavailable for this identity; used HTTP fallback. Details: local websocket bridge request failed: websocket session is not connected for identity zhuocheng"
    );
    assert_eq!(
        websocket_cache_fallback_warning(Some(&err)),
        "WebSocket listener was unavailable for this identity; loaded data from local cache. Details: local websocket bridge request failed: websocket session is not connected for identity zhuocheng"
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

fn assert_has_generated_meta(meta: &Value) {
    assert_eq!(meta["anp_version"], "1.0");
    assert!(meta["operation_id"].as_str().unwrap().starts_with("op-"));
    assert!(!meta["created_at"].as_str().unwrap().is_empty());
}
