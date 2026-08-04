use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use anp::authentication::{
    create_did_wba_document, validate_device_manifest, DidDocumentOptions, DidProfile,
};
use anp::group_e2ee::operations::v2::{
    V2AddMemberInput, V2CreateGroupInput, V2DidDocument, V2EncryptInput, V2GenerateKeyPackageInput,
    V2ProcessNoticeInput, V2RemoveMemberInput,
};
use anp::group_e2ee::storage::{GroupMlsOwnerScope, ImCoreSqliteGroupMlsStore};
use anp::group_e2ee::{
    V2E2eeNotice, V2GroupApplicationPlaintext, V2GroupControlMetadata, V2GroupIncomingBody,
    V2GroupIncomingMetadata, V2GroupNoticeMetadata, V2GroupSendMetadata, V2GroupStateRef,
    V2ServiceMetadata, V2Target, GROUP_CIPHER_CONTENT_TYPE_V2, GROUP_E2EE_PROFILE_V2,
    GROUP_E2EE_SECURITY_PROFILE_V2, GROUP_E2EE_TRANSPORT_PROFILE_V2,
};
use anp::proof::{
    generate_w3c_proof, ProofGenerationOptions, CRYPTOSUITE_EDDSA_JCS_2022,
    PROOF_TYPE_DATA_INTEGRITY,
};
use serde_json::{json, Value};

use super::*;

const NOW: &str = "2026-07-20T00:00:00Z";
const ISSUED_AT: &str = "2026-07-19T00:00:00Z";
const EXPIRES_AT: &str = "2026-08-19T00:00:00Z";
const GROUP_DID: &str = "did:wba:p6-core.example:groups:product";
const SERVICE_DID: &str = "did:wba:p6-core.example:services:message";

#[derive(Debug)]
struct DeviceFixture {
    device_id: String,
    signing_key_id: String,
    signing_private_pem: String,
}

#[derive(Debug)]
struct DidFixture {
    did: String,
    document: Value,
    devices: Vec<DeviceFixture>,
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(prefix: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextResponse {
    Exact,
    TransportFailure,
    WrongEpoch,
}

#[derive(Debug, Clone)]
struct RpcCall {
    method: String,
    params: Value,
}

#[derive(Debug, Default)]
struct LoopbackState {
    calls: Vec<RpcCall>,
    next: Option<NextResponse>,
}

#[derive(Clone, Default)]
struct LoopbackTransport {
    state: Rc<RefCell<LoopbackState>>,
}

impl LoopbackTransport {
    fn set_next(&self, next: NextResponse) {
        self.state.borrow_mut().next = Some(next);
    }

    fn calls(&self) -> Vec<RpcCall> {
        self.state.borrow().calls.clone()
    }
}

impl AuthenticatedRpcTransport for LoopbackTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        assert_eq!(endpoint, MESSAGE_RPC_ENDPOINT);
        self.state.borrow_mut().calls.push(RpcCall {
            method: method.to_owned(),
            params: params.clone(),
        });
        let next = self
            .state
            .borrow_mut()
            .next
            .take()
            .unwrap_or(NextResponse::Exact);
        if next == NextResponse::TransportFailure {
            return Err(crate::ImError::TransportUnavailable {
                detail: "simulated uncertain P6 transport failure".to_owned(),
            });
        }
        let meta = &params["meta"];
        let body = &params["body"];
        let mut response = match method {
            "group.e2ee.publish_key_package" => json!({
                "published": true,
                "owner_did": body["group_key_package"]["owner_did"],
                "owner_device_id": body["group_key_package"]["owner_device_id"],
                "key_package_id": body["group_key_package"]["key_package_id"],
                "published_at": NOW,
            }),
            "group.e2ee.create" => json!({
                "created": true,
                "group_did": body["group_did"],
                "group_state_ref": body["group_state_ref"],
                "crypto_group_id_b64u": body["crypto_group_id_b64u"],
                "epoch": body["epoch"],
                "accepted_at": NOW,
            }),
            "group.e2ee.add" | "group.e2ee.remove" => json!({
                "accepted": true,
                "group_did": body["group_state_ref"]["group_did"],
                "member_did": body["member_did"],
                "member_device_id": body["member_device_id"],
                "group_state_ref": body["group_state_ref"],
                "crypto_group_id_b64u": body["crypto_group_id_b64u"],
                "epoch": body["epoch"],
                "accepted_at": NOW,
            }),
            "group.e2ee.send" => json!({
                "accepted": true,
                "group_did": body["group_state_ref"]["group_did"],
                "message_id": meta["message_id"],
                "operation_id": meta["operation_id"],
                "group_event_seq": "1",
                "group_state_version": body["group_state_ref"]["group_state_version"],
                "accepted_at": NOW,
                "epoch": body["epoch"],
                "group_receipt": {"test": true},
            }),
            other => panic!("unexpected loopback method {other}"),
        };
        if next == NextResponse::WrongEpoch {
            response["epoch"] = json!("999");
        }
        Ok(response)
    }
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for LoopbackTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

#[test]
fn product_orchestrates_device_scoped_mls_and_filters_control_notices() {
    let directory = TestDirectory::new("im-core-p6-v2-product");
    let fixture = make_did_fixture("alice-product", &["alice-a1", "alice-a2"]);
    let a1 = &fixture.devices[0];
    let a2 = &fixture.devices[1];
    let transport = LoopbackTransport::default();
    let mut a1_product = product(&directory, &fixture, a1, transport.clone());
    let mut a2_product = product(&directory, &fixture, a2, transport.clone());

    let a1_key = signing_key(a1);
    let a1_publish = a1_product
        .prepare_current_key_package(
            key_service_meta(&fixture.did, &a1.device_id, "op-publish-a1"),
            key_package_input(&fixture, a1, "kp-a1", "req-kp-a1"),
            &fixture.document,
            &a1_key,
        )
        .expect("prepare A1 KeyPackage publish");
    a1_product
        .publish_current_key_package(&a1_publish)
        .expect("publish A1 KeyPackage");

    let a2_key = signing_key(a2);
    let a2_publish = a2_product
        .prepare_current_key_package(
            key_service_meta(&fixture.did, &a2.device_id, "op-publish-a2"),
            key_package_input(&fixture, a2, "kp-a2", "req-kp-a2"),
            &fixture.document,
            &a2_key,
        )
        .expect("prepare A2 KeyPackage publish");
    a2_product
        .publish_current_key_package(&a2_publish)
        .expect("publish A2 KeyPackage");

    let create = a1_product
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(&fixture.did, &a1.device_id, "op-create"),
            group_state_ref: state_ref(1),
            creator_key_package: a1_publish.body.group_key_package.clone(),
            creator_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-create".to_owned(),
            request_id: "req-create".to_owned(),
        })
        .expect("prepare create");
    let created = a1_product
        .submit_create(&create, "req-finalize-create")
        .expect("submit and finalize create");
    assert_eq!(created.finalized.status, "finalized");
    assert_eq!(created.finalized.epoch, "0");
    let a1_ready = status_runtime(&directory, &fixture, a1)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("A1 local P6 status");
    assert_eq!(a1_ready.state, crate::secure::GroupSecureState::Ready);
    assert!(a1_ready.can_send_secure);
    let a2_before_welcome = status_runtime(&directory, &fixture, a2)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("A2 pre-Welcome status");
    assert_eq!(
        a2_before_welcome.state,
        crate::secure::GroupSecureState::MissingLocalState
    );
    assert!(!a2_before_welcome.can_send_secure);
    let a2_repair_before_welcome = status_runtime(&directory, &fixture, a2)
        .repair(
            crate::ids::GroupRef::parse(GROUP_DID).unwrap(),
            "req-repair-a2-before-welcome",
        )
        .expect("A2 pre-Welcome repair is a secret-free no-op");
    assert_eq!(
        a2_repair_before_welcome.state,
        crate::secure::GroupSecureState::MissingLocalState
    );
    assert!(!a2_repair_before_welcome.repaired);
    assert_eq!(a2_repair_before_welcome.added_devices, 0);
    assert_eq!(a2_repair_before_welcome.removed_devices, 0);
    assert_eq!(a2_repair_before_welcome.remaining_devices, 0);

    let before_welcome = a1_product
        .prepare_product_application_send(
            send_meta(&fixture.did, &a1.device_id, "before-welcome"),
            state_ref(1),
            V2ProductApplication::text(GROUP_DID, "text/plain", "history before A2")
                .expect("prepare text body"),
            fixture.document.clone(),
            NOW.to_owned(),
            true,
            "req-encrypt-before-welcome".to_owned(),
        )
        .expect("A1 encrypts before A2 joins");
    a1_product
        .submit_product_application_send(&before_welcome)
        .expect("Host accepts pre-Welcome ciphertext");
    assert!(a2_product
        .decrypt_incoming_application(incoming_input(
            &fixture,
            a1,
            a2,
            &before_welcome.encrypted,
            "req-a2-cannot-decrypt-history",
        ))
        .is_err());

    let mut wrong_device_package = a2_publish.body.group_key_package.clone();
    wrong_device_package.owner_device_id = "alice-not-a2".to_owned();
    assert!(a1_product
        .prepare_add(V2AddMemberInput {
            meta: control_meta(&fixture.did, &a1.device_id, "op-add-wrong-device"),
            group_state_ref: state_ref(2),
            group_key_package: wrong_device_package,
            member_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-add-wrong-device".to_owned(),
            request_id: "req-add-wrong-device".to_owned(),
        })
        .is_err());

    let add = a1_product
        .prepare_add(V2AddMemberInput {
            meta: control_meta(&fixture.did, &a1.device_id, "op-add-a2"),
            group_state_ref: state_ref(2),
            group_key_package: a2_publish.body.group_key_package.clone(),
            member_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-add-a2".to_owned(),
            request_id: "req-add-a2".to_owned(),
        })
        .expect("prepare A2 Add");
    let added = a1_product
        .submit_add(&add, "req-finalize-add-a2")
        .expect("submit and finalize A2 Add");
    assert_eq!(added.finalized.epoch, "1");

    let welcome = welcome_notice(&fixture, a2, &add.prepared.body, "notice-welcome-a2");
    let output = consume_public_notice(&directory, &fixture, a2, welcome.clone())
        .expect("A2 consumes standard Welcome notice");
    assert_eq!(output.notice_type, "welcome-delivery");
    assert_eq!(output.epoch, "1");
    let a2_ready = status_runtime(&directory, &fixture, a2)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("A2 status after durable Welcome processing");
    assert_eq!(a2_ready.state, crate::secure::GroupSecureState::Ready);
    assert!(a2_ready.can_send_secure);
    let replay = consume_public_notice(
        &directory,
        &fixture,
        a2,
        V2ProcessNoticeInput {
            request_id: "req-welcome-replay-after-restart".to_owned(),
            ..welcome
        },
    )
    .expect("Welcome replay remains control-only and idempotent");
    assert_eq!(replay, output);
    let mut conflicting_replay =
        welcome_notice(&fixture, a2, &add.prepared.body, "notice-welcome-a2");
    conflicting_replay.notice.epoch = "2".to_owned();
    assert!(consume_public_notice(&directory, &fixture, a2, conflicting_replay).is_err());
    assert_eq!(
        status_runtime(&directory, &fixture, a2)
            .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
            .expect("conflicting operation replay preserves the accepted epoch")
            .state,
        crate::secure::GroupSecureState::Ready
    );

    let wrong_target = welcome_notice(&fixture, a2, &add.prepared.body, "notice-wrong-target");
    let mut wrong_target = wrong_target;
    wrong_target.meta.recipient_device_id = a1.device_id.clone();
    assert!(consume_public_notice(&directory, &fixture, a2, wrong_target).is_err());

    let malformed = welcome_notice(&fixture, a2, &add.prepared.body, "notice-malformed-secret");
    let mut malformed_wire =
        anp::group_e2ee::group_notice_notification_v2(malformed.meta, malformed.notice)
            .expect("build malformed notice base");
    malformed_wire["params"]["body"]["unexpected_private_material"] =
        json!("SECRET-WELCOME-CONTROL");
    let malformed_error = crate::internal::group_e2ee::v2_notice::parse_notice(&malformed_wire)
        .expect_err("unknown notice fields fail closed before MLS state");
    assert!(!malformed_error
        .to_string()
        .contains("SECRET-WELCOME-CONTROL"));

    let self_echo = commit_notice(
        &fixture,
        a1,
        &add.prepared.body,
        "notice-add-a2-self-echo",
        "active",
    );
    let self_echo_output = consume_public_notice(&directory, &fixture, a1, self_echo.clone())
        .expect("A1 records its exact finalized Commit echo without merging twice");
    assert_eq!(
        self_echo_output.source_operation_id.as_deref(),
        Some("op-add-a2")
    );
    let replayed_self_echo = consume_public_notice(
        &directory,
        &fixture,
        a1,
        V2ProcessNoticeInput {
            request_id: "req-add-a2-self-echo-replay-after-restart".to_owned(),
            ..self_echo
        },
    )
    .expect("A1 self-echo receipt replays after restart");
    assert_eq!(replayed_self_echo, self_echo_output);

    let mut wrong_group = commit_notice(
        &fixture,
        a1,
        &add.prepared.body,
        "notice-add-a2-wrong-group",
        "active",
    );
    wrong_group.meta.sender_did = "did:wba:p6-core.example:groups:other".to_owned();
    wrong_group.notice.group_did = wrong_group.meta.sender_did.clone();
    wrong_group.notice.group_state_ref.group_did = wrong_group.meta.sender_did.clone();
    assert!(consume_public_notice(&directory, &fixture, a1, wrong_group).is_err());
    assert_eq!(
        status_runtime(&directory, &fixture, a1)
            .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
            .expect("wrong-group notice preserves the real group")
            .state,
        crate::secure::GroupSecureState::Ready
    );

    let send = a1_product
        .prepare_application_send(V2EncryptInput {
            meta: send_meta(&fixture.did, &a1.device_id, "after-add"),
            group_state_ref: state_ref(2),
            application_plaintext: V2GroupApplicationPlaintext {
                application_content_type: "text/plain".to_owned(),
                thread_id: None,
                reply_to_message_id: None,
                annotations: None,
                text: Some("hello A2".to_owned()),
                payload: None,
                payload_b64u: None,
            },
            sender_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            request_id: "req-encrypt-after-add".to_owned(),
        })
        .expect("encrypt one MLS application message");
    let sent = a1_product
        .submit_application_send(&send)
        .expect("submit exact prepared MLS ciphertext");
    assert_eq!(sent.message_id, send.meta.message_id);
    let decrypted = a2_product
        .decrypt_incoming_application(incoming_input(&fixture, a1, a2, &send, "req-decrypt-a2"))
        .expect("A2 decrypts application delivery");
    assert_eq!(
        decrypted.application_plaintext.text.as_deref(),
        Some("hello A2")
    );

    let committed_attachment = committed_group_attachment();
    let object_key = committed_attachment.full_manifest["attachments"][0]["encryption_info"]
        ["object_key_b64u"]
        .as_str()
        .expect("full manifest object key")
        .to_owned();
    let attachment_application =
        V2ProductApplication::committed_attachment(GROUP_DID, &committed_attachment)
            .expect("one committed group object becomes one MLS application body");
    let projected = attachment_application
        .projection()
        .payload
        .as_ref()
        .expect("redacted attachment projection");
    assert!(!projected.to_string().contains(&object_key));
    let send_calls_before = transport
        .calls()
        .iter()
        .filter(|call| call.method == "group.e2ee.send")
        .count();
    let attachment_send = a1_product
        .prepare_product_application_send(
            send_meta(&fixture.did, &a1.device_id, "attachment"),
            state_ref(2),
            attachment_application,
            fixture.document.clone(),
            NOW.to_owned(),
            true,
            "req-encrypt-attachment".to_owned(),
        )
        .expect("encrypt one attachment manifest");
    assert!(!format!("{attachment_send:?}").contains(&object_key));
    a1_product
        .submit_product_application_send(&attachment_send)
        .expect("submit one attachment MLS ciphertext");
    let send_calls_after = transport
        .calls()
        .iter()
        .filter(|call| call.method == "group.e2ee.send")
        .count();
    assert_eq!(send_calls_after, send_calls_before + 1);
    let attachment_call = transport
        .calls()
        .last()
        .expect("attachment Host call")
        .clone();
    assert_eq!(
        attachment_call.params["client"]["attachment_grant_refs"],
        json!([committed_attachment.grant_ref])
    );
    assert!(!attachment_call.params.to_string().contains(&object_key));
    let attachment_decrypted = a2_product
        .decrypt_incoming_application(incoming_input(
            &fixture,
            a1,
            a2,
            &attachment_send.encrypted,
            "req-decrypt-attachment-a2",
        ))
        .expect("A2 decrypts the single MLS attachment manifest");
    let attachment_payload = attachment_decrypted
        .application_plaintext
        .payload
        .expect("full attachment manifest remains inside MLS");
    assert_eq!(
        attachment_payload["attachments"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        attachment_payload["attachments"][0]["object_uri"],
        committed_attachment.full_manifest["attachments"][0]["object_uri"]
    );
    assert_eq!(
        attachment_payload["attachments"][0]["encryption_info"]["object_key_b64u"],
        object_key
    );

    let remove = a1_product
        .prepare_remove(V2RemoveMemberInput {
            meta: control_meta(&fixture.did, &a1.device_id, "op-remove-a2"),
            group_state_ref: state_ref(3),
            member_did: fixture.did.clone(),
            member_device_id: a2.device_id.clone(),
            member_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-remove-a2".to_owned(),
            request_id: "req-remove-a2".to_owned(),
        })
        .expect("prepare exact A2 Remove");
    let removed = a1_product
        .submit_remove(&remove, "req-finalize-remove-a2")
        .expect("submit and finalize exact A2 Remove");
    assert_eq!(removed.finalized.epoch, "2");
    let remove_notice =
        remove_commit_notice(&fixture, a2, &remove.prepared.body, "notice-remove-a2");
    let remove_output = consume_public_notice(&directory, &fixture, a2, remove_notice)
        .expect("A2 consumes its exact Remove Commit notice");
    assert!(remove_output.self_removed);
    let a2_removed = status_runtime(&directory, &fixture, a2)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("A2 status after exact Remove Commit");
    assert!(!a2_removed.can_send_secure);
    assert!(!a2_removed.local_readiness.has_active_membership);

    let after_remove = a1_product
        .prepare_application_send(V2EncryptInput {
            meta: send_meta(&fixture.did, &a1.device_id, "after-remove"),
            group_state_ref: state_ref(3),
            application_plaintext: V2GroupApplicationPlaintext {
                application_content_type: "text/plain".to_owned(),
                thread_id: None,
                reply_to_message_id: None,
                annotations: None,
                text: Some("future ciphertext".to_owned()),
                payload: None,
                payload_b64u: None,
            },
            sender_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            request_id: "req-encrypt-after-remove".to_owned(),
        })
        .expect("remaining A1 encrypts after exact A2 removal");
    a1_product
        .submit_application_send(&after_remove)
        .expect("Host accepts post-removal ciphertext");
    assert!(a2_product
        .decrypt_incoming_application(incoming_input(
            &fixture,
            a1,
            a2,
            &after_remove,
            "req-decrypt-removed-a2",
        ))
        .is_err());

    let calls = transport.calls();
    assert!(calls.iter().any(|call| {
        call.method == "group.e2ee.add" && call.params["auth"]["origin_proof"].is_object()
    }));
    assert!(calls.iter().any(|call| {
        call.method == "group.e2ee.send" && call.params["auth"]["origin_proof"].is_object()
    }));
    // Adding A2 is a P6 Leaf mutation only. The product seam never invokes a
    // P4 business-member mutation, so Alice remains one business DID member.
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.method == "group.e2ee.add")
            .count(),
        1
    );
    assert!(calls
        .iter()
        .all(|call| call.method.starts_with("group.e2ee.")));
    assert_ne!(
        store(&directory, &fixture, a1).state_db_path(),
        store(&directory, &fixture, a2).state_db_path()
    );
}

#[tokio::test]
async fn key_package_publish_restarts_with_exact_bytes_and_skips_host_after_acceptance() {
    let directory = TestDirectory::new("im-core-p6-v2-key-package-wal");
    let fixture = make_did_fixture("alice-key-package-wal", &["alice-kp1"]);
    let device = &fixture.devices[0];
    let transport = LoopbackTransport::default();
    let key = signing_key(device);
    let mut meta = key_service_meta(&fixture.did, &device.device_id, "op-join-kp");
    meta.created_at = None;
    let first_input = key_package_input(&fixture, device, "kp-join", "req-join-kp-first");
    let initial = product(&directory, &fixture, device, transport.clone());
    let first = initial
        .prepare_current_key_package(meta.clone(), first_input.clone(), &fixture.document, &key)
        .expect("prepare durable public KeyPackage before Host call");
    transport.set_next(NextResponse::TransportFailure);
    let mut async_transport = transport.clone();
    assert!(matches!(
        initial
            .publish_current_key_package_async(&mut async_transport, &first)
            .await,
        Err(crate::ImError::TransportUnavailable { .. })
    ));
    drop(initial);

    let mut retry_input = first_input.clone();
    retry_input.issued_at = "2026-07-19T01:00:00Z".to_owned();
    retry_input.expires_at = "2026-08-20T00:00:00Z".to_owned();
    retry_input.now = "2026-07-20T00:01:00Z".to_owned();
    retry_input.request_id = "req-join-kp-restart".to_owned();
    let restarted = product(&directory, &fixture, device, transport.clone());
    let resumed = restarted
        .prepare_current_key_package(meta.clone(), retry_input.clone(), &fixture.document, &key)
        .expect("restart resumes exact public KeyPackage");
    assert_eq!(resumed, first);
    let accepted = restarted
        .publish_current_key_package_async(&mut async_transport, &resumed)
        .await
        .expect("typed Host result is persisted as accepted");
    drop(restarted);

    let calls = transport.calls();
    let publish_calls = calls
        .iter()
        .filter(|call| call.method == "group.e2ee.publish_key_package")
        .collect::<Vec<_>>();
    assert_eq!(publish_calls.len(), 2);
    assert_eq!(publish_calls[0].params, publish_calls[1].params);

    let successful_replay = product(&directory, &fixture, device, transport.clone());
    let cached = successful_replay
        .prepare_current_key_package(meta, retry_input, &fixture.document, &key)
        .expect("accepted publish remains resumable");
    assert_eq!(
        cached.status,
        anp::group_e2ee::operations::v2::V2KeyPackagePublishStatus::Accepted
    );
    assert_eq!(cached.meta, first.meta);
    assert_eq!(cached.body, first.body);
    assert_eq!(cached.accepted_result.as_ref(), Some(&accepted));
    assert_eq!(
        successful_replay
            .publish_current_key_package_async(&mut async_transport, &cached)
            .await
            .expect("accepted replay returns cached typed result"),
        accepted
    );
    assert_eq!(
        transport
            .calls()
            .iter()
            .filter(|call| call.method == "group.e2ee.publish_key_package")
            .count(),
        2,
        "repeated successful Join polling must not call the Host again"
    );
}

#[tokio::test]
async fn key_package_publish_rotates_an_expired_unaccepted_join_attempt() {
    let directory = TestDirectory::new("im-core-p6-v2-key-package-rotate");
    let fixture = make_did_fixture("alice-key-package-rotate", &["alice-kp-rotate"]);
    let device = &fixture.devices[0];
    let transport = LoopbackTransport::default();
    let key = signing_key(device);
    let mut meta = key_service_meta(&fixture.did, &device.device_id, "op-join-kp-stable");
    meta.created_at = None;
    let first_input = key_package_input(
        &fixture,
        device,
        "kp-join-stable",
        "req-join-kp-rotate-first",
    );
    let initial = product(&directory, &fixture, device, transport.clone());
    let first = initial
        .prepare_current_key_package(meta.clone(), first_input, &fixture.document, &key)
        .expect("prepare the stable Join KeyPackage family");
    transport.set_next(NextResponse::TransportFailure);
    let mut async_transport = transport.clone();
    assert!(matches!(
        initial
            .publish_current_key_package_async(&mut async_transport, &first)
            .await,
        Err(crate::ImError::TransportUnavailable { .. })
    ));
    drop(initial);

    let mut expired_retry = key_package_input(
        &fixture,
        device,
        "kp-join-stable",
        "req-join-kp-rotate-expired",
    );
    expired_retry.issued_at = "2026-08-20T00:00:00Z".to_owned();
    expired_retry.expires_at = "2026-09-20T00:00:00Z".to_owned();
    expired_retry.now = "2026-08-20T00:00:00Z".to_owned();
    let restarted = product(&directory, &fixture, device, transport.clone());
    let rotated = restarted
        .prepare_current_key_package(meta.clone(), expired_retry.clone(), &fixture.document, &key)
        .expect("expired unaccepted attempt rotates within the stable Join family");
    assert_ne!(rotated.meta.operation_id, first.meta.operation_id);
    assert_ne!(
        rotated.body.group_key_package.key_package_id,
        first.body.group_key_package.key_package_id
    );
    assert!(rotated.meta.operation_id.starts_with("kp-op-attempt-"));
    assert!(rotated
        .body
        .group_key_package
        .key_package_id
        .starts_with("kp-attempt-"));

    expired_retry.now = "2026-08-21T00:00:00Z".to_owned();
    expired_retry.request_id = "req-join-kp-rotate-exact-retry".to_owned();
    let exact_retry = restarted
        .prepare_current_key_package(meta.clone(), expired_retry.clone(), &fixture.document, &key)
        .expect("the rotated attempt resumes byte-for-byte");
    assert_eq!(exact_retry, rotated);
    let accepted = restarted
        .publish_current_key_package_async(&mut async_transport, &exact_retry)
        .await
        .expect("publish and accept the attempt-specific wire IDs");

    let calls = transport.calls();
    let publish_calls = calls
        .iter()
        .filter(|call| call.method == "group.e2ee.publish_key_package")
        .collect::<Vec<_>>();
    assert_eq!(publish_calls.len(), 2);
    assert_ne!(publish_calls[0].params, publish_calls[1].params);
    assert_eq!(
        publish_calls[1].params["meta"]["operation_id"],
        rotated.meta.operation_id
    );
    assert_eq!(
        publish_calls[1].params["body"]["group_key_package"]["key_package_id"],
        rotated.body.group_key_package.key_package_id
    );

    expired_retry.now = "2026-10-01T00:00:00Z".to_owned();
    expired_retry.request_id = "req-join-kp-accepted-after-ttl".to_owned();
    let cached = restarted
        .prepare_current_key_package(meta, expired_retry, &fixture.document, &key)
        .expect("accepted Join family remains terminal after TTL");
    assert_eq!(cached.meta, rotated.meta);
    assert_eq!(cached.accepted_result.as_ref(), Some(&accepted));
    assert_eq!(
        restarted
            .publish_current_key_package_async(&mut async_transport, &cached)
            .await
            .expect("terminal retry uses the cached Host result"),
        accepted
    );
    assert_eq!(
        transport
            .calls()
            .iter()
            .filter(|call| call.method == "group.e2ee.publish_key_package")
            .count(),
        2
    );
}

#[test]
fn sequential_device_membership_reloads_finalized_epoch_for_each_leaf() {
    let directory = TestDirectory::new("im-core-p6-v2-membership-epochs");
    let alice = make_did_fixture("alice-membership-epochs", &["alice-e1"]);
    let bob = make_did_fixture("bob-membership-epochs", &["bob-e1", "bob-e2"]);
    let controller = &alice.devices[0];
    let bob_one = &bob.devices[0];
    let bob_two = &bob.devices[1];
    let transport = LoopbackTransport::default();
    let mut controller_product = product(&directory, &alice, controller, transport.clone());
    let bob_one_product = product(&directory, &bob, bob_one, transport.clone());
    let bob_two_product = product(&directory, &bob, bob_two, transport.clone());

    let controller_package = controller_product
        .prepare_current_key_package(
            key_service_meta(&alice.did, &controller.device_id, "op-epoch-kp-a1"),
            key_package_input(&alice, controller, "kp-epoch-a1", "req-epoch-kp-a1"),
            &alice.document,
            &signing_key(controller),
        )
        .unwrap();
    let bob_one_package = bob_one_product
        .prepare_current_key_package(
            key_service_meta(&bob.did, &bob_one.device_id, "op-epoch-kp-b1"),
            key_package_input(&bob, bob_one, "kp-epoch-b1", "req-epoch-kp-b1"),
            &bob.document,
            &signing_key(bob_one),
        )
        .unwrap();
    let bob_two_package = bob_two_product
        .prepare_current_key_package(
            key_service_meta(&bob.did, &bob_two.device_id, "op-epoch-kp-b2"),
            key_package_input(&bob, bob_two, "kp-epoch-b2", "req-epoch-kp-b2"),
            &bob.document,
            &signing_key(bob_two),
        )
        .unwrap();
    let create = controller_product
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(&alice.did, &controller.device_id, "op-epoch-create"),
            group_state_ref: state_ref(1),
            creator_key_package: controller_package.body.group_key_package,
            creator_did_document: alice.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-epoch-create".to_owned(),
            request_id: "req-epoch-create".to_owned(),
        })
        .unwrap();
    controller_product
        .submit_create(&create, "req-epoch-create-finalize")
        .unwrap();

    let add_one = controller_product
        .prepare_add(V2AddMemberInput {
            meta: control_meta(&alice.did, &controller.device_id, "op-epoch-add-b1"),
            group_state_ref: state_ref(2),
            group_key_package: bob_one_package.body.group_key_package,
            member_did_document: bob.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-epoch-add-b1".to_owned(),
            request_id: "req-epoch-add-b1".to_owned(),
        })
        .unwrap();
    assert_eq!(add_one.prepared.from_epoch, "0");
    assert_eq!(add_one.prepared.body.epoch, "1");
    controller_product
        .submit_add(&add_one, "req-epoch-add-b1-finalize")
        .unwrap();

    let add_two = controller_product
        .prepare_add(V2AddMemberInput {
            meta: control_meta(&alice.did, &controller.device_id, "op-epoch-add-b2"),
            // One P4 member-add state reference is stable across both device
            // leaves; the local MLS epoch must nevertheless advance.
            group_state_ref: state_ref(2),
            group_key_package: bob_two_package.body.group_key_package,
            member_did_document: bob.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-epoch-add-b2".to_owned(),
            request_id: "req-epoch-add-b2".to_owned(),
        })
        .unwrap();
    assert_eq!(add_two.prepared.from_epoch, "1");
    assert_eq!(add_two.prepared.body.epoch, "2");
    controller_product
        .submit_add(&add_two, "req-epoch-add-b2-finalize")
        .unwrap();

    let remove_one = controller_product
        .prepare_remove(V2RemoveMemberInput {
            meta: control_meta(&alice.did, &controller.device_id, "op-epoch-remove-b1"),
            group_state_ref: state_ref(3),
            member_did: bob.did.clone(),
            member_device_id: bob_one.device_id.clone(),
            member_did_document: bob.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-epoch-remove-b1".to_owned(),
            request_id: "req-epoch-remove-b1".to_owned(),
        })
        .unwrap();
    assert_eq!(remove_one.prepared.from_epoch, "2");
    assert_eq!(remove_one.prepared.body.epoch, "3");
    controller_product
        .submit_remove(&remove_one, "req-epoch-remove-b1-finalize")
        .unwrap();

    let remove_two = controller_product
        .prepare_remove(V2RemoveMemberInput {
            meta: control_meta(&alice.did, &controller.device_id, "op-epoch-remove-b2"),
            group_state_ref: state_ref(3),
            member_did: bob.did.clone(),
            member_device_id: bob_two.device_id.clone(),
            member_did_document: bob.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-epoch-remove-b2".to_owned(),
            request_id: "req-epoch-remove-b2".to_owned(),
        })
        .unwrap();
    assert_eq!(remove_two.prepared.from_epoch, "3");
    assert_eq!(remove_two.prepared.body.epoch, "4");
    controller_product
        .submit_remove(&remove_two, "req-epoch-remove-b2-finalize")
        .unwrap();

    let membership_calls = transport
        .calls()
        .into_iter()
        .filter(|call| matches!(call.method.as_str(), "group.e2ee.add" | "group.e2ee.remove"))
        .collect::<Vec<_>>();
    assert_eq!(membership_calls.len(), 4);
    assert_eq!(
        membership_calls[0].params["body"]["group_state_ref"]["group_state_version"],
        "2"
    );
    assert_eq!(
        membership_calls[1].params["body"]["group_state_ref"]["group_state_version"],
        "2"
    );
    assert_eq!(
        membership_calls[2].params["body"]["group_state_ref"]["group_state_version"],
        "3"
    );
    assert_eq!(
        membership_calls[3].params["body"]["group_state_ref"]["group_state_version"],
        "3"
    );
    assert_eq!(membership_calls[0].params["body"]["epoch"], "1");
    assert_eq!(membership_calls[1].params["body"]["epoch"], "2");
    assert_eq!(membership_calls[2].params["body"]["epoch"], "3");
    assert_eq!(membership_calls[3].params["body"]["epoch"], "4");
}

#[test]
fn uncertain_submit_survives_restart_and_requires_host_recheck() {
    let directory = TestDirectory::new("im-core-p6-v2-reconcile");
    let fixture = make_did_fixture("alice-reconcile", &["alice-r1"]);
    let device = &fixture.devices[0];
    let transport = LoopbackTransport::default();
    let mut initial = product(&directory, &fixture, device, transport.clone());
    let key = signing_key(device);
    let publish = initial
        .prepare_current_key_package(
            key_service_meta(&fixture.did, &device.device_id, "op-publish-r1"),
            key_package_input(&fixture, device, "kp-r1", "req-kp-r1"),
            &fixture.document,
            &key,
        )
        .expect("prepare package");
    let create = initial
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(&fixture.did, &device.device_id, "op-create-r1"),
            group_state_ref: state_ref(1),
            creator_key_package: publish.body.group_key_package,
            creator_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-create-r1".to_owned(),
            request_id: "req-create-r1".to_owned(),
        })
        .expect("prepare create");
    transport.set_next(NextResponse::TransportFailure);
    assert!(initial
        .submit_create(&create, "req-finalize-must-not-run")
        .is_err());
    drop(initial);

    let needs_repair = status_runtime(&directory, &fixture, device)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("read durable prepared WAL status");
    assert_eq!(
        needs_repair.state,
        crate::secure::GroupSecureState::NeedsRepair
    );
    assert_eq!(needs_repair.pending_work.pending_commits, 1);
    let local_repair = status_runtime(&directory, &fixture, device)
        .repair(
            crate::ids::GroupRef::parse(GROUP_DID).unwrap(),
            "req-local-wal-repair",
        )
        .expect("local WAL reconciliation");
    assert!(!local_repair.repaired);
    assert_eq!(
        local_repair.state,
        crate::secure::GroupSecureState::NeedsRepair
    );

    let mut restarted = product(&directory, &fixture, device, transport.clone());
    let reconciled = restarted
        .reconcile_pending("req-reconcile-network-uncertain")
        .expect("reconcile after restart");
    assert_eq!(reconciled.entries.len(), 1);
    assert_eq!(reconciled.entries[0].pending.status, "prepared");
    assert!(reconciled.entries[0].host_recheck_required);
    assert!(reconciled.entries[0].pending.prepared_response.is_some());

    transport.set_next(NextResponse::WrongEpoch);
    assert!(restarted
        .submit_create(&create, "req-finalize-wrong-response")
        .is_err());
    let still_pending = restarted
        .reconcile_pending("req-reconcile-wrong-response")
        .expect("mismatched accepted response does not finalize or abort");
    assert!(still_pending.entries[0].host_recheck_required);

    let committed = restarted
        .submit_create(&create, "req-finalize-after-recheck")
        .expect("exact replayed Host response finalizes");
    assert_eq!(committed.finalized.status, "finalized");
    assert!(restarted
        .reconcile_pending("req-reconcile-finalized")
        .expect("final reconciliation")
        .entries
        .is_empty());
    let ready = status_runtime(&directory, &fixture, device)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("status after exact Host result finalizes WAL");
    assert_eq!(ready.state, crate::secure::GroupSecureState::Ready);
    assert!(ready.can_send_secure);
}

#[test]
fn explicit_abort_is_idempotent() {
    let directory = TestDirectory::new("im-core-p6-v2-abort");
    let fixture = make_did_fixture("alice-abort", &["alice-x1"]);
    let device = &fixture.devices[0];
    let transport = LoopbackTransport::default();
    let product = product(&directory, &fixture, device, transport);
    let key = signing_key(device);
    let publish = product
        .prepare_current_key_package(
            key_service_meta(&fixture.did, &device.device_id, "op-publish-x1"),
            key_package_input(&fixture, device, "kp-x1", "req-kp-x1"),
            &fixture.document,
            &key,
        )
        .expect("prepare package");
    let create = product
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(&fixture.did, &device.device_id, "op-create-x1"),
            group_state_ref: state_ref(1),
            creator_key_package: publish.body.group_key_package,
            creator_did_document: fixture.document,
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-create-x1".to_owned(),
            request_id: "req-create-x1".to_owned(),
        })
        .expect("prepare create");
    assert_eq!(
        product
            .abort_pending(&create.prepared.pending_commit_id, "req-abort-x1")
            .expect("explicit deterministic abort")
            .status,
        "aborted"
    );
    assert_eq!(
        product
            .abort_pending(&create.prepared.pending_commit_id, "req-abort-x1-repeat")
            .expect("abort replay")
            .status,
        "aborted"
    );
}

#[test]
fn status_repair_finishes_accepted_wal_without_copying_another_device_state() {
    let directory = TestDirectory::new("im-core-p6-v2-accepted-repair");
    let fixture = make_did_fixture("alice-accepted", &["alice-w1", "alice-w2"]);
    let a1 = &fixture.devices[0];
    let a2 = &fixture.devices[1];
    let transport = LoopbackTransport::default();
    let product = product(&directory, &fixture, a1, transport);
    let key = signing_key(a1);
    let publish = product
        .prepare_current_key_package(
            key_service_meta(&fixture.did, &a1.device_id, "op-publish-w1"),
            key_package_input(&fixture, a1, "kp-w1", "req-kp-w1"),
            &fixture.document,
            &key,
        )
        .expect("prepare package");
    product
        .prepare_create(V2CreateGroupInput {
            meta: control_service_meta(&fixture.did, &a1.device_id, "op-create-w1"),
            group_state_ref: state_ref(1),
            creator_key_package: publish.body.group_key_package,
            creator_did_document: fixture.document.clone(),
            now: NOW.to_owned(),
            draft_extension_negotiated: true,
            pending_commit_id: "pending-create-w1".to_owned(),
            request_id: "req-create-w1".to_owned(),
        })
        .expect("prepare durable create WAL");
    let a1_store = store(&directory, &fixture, a1);
    let connection = rusqlite::Connection::open(a1_store.state_db_path()).expect("open A1 state");
    connection
        .execute(
            "UPDATE group_mls_pending_commits
             SET status = 'accepted'
             WHERE owner_identity_id = ?1
               AND device_id = ?2
               AND pending_commit_id = ?3",
            rusqlite::params![
                format!("identity-{}", a1.device_id),
                a1.device_id,
                "pending-create-w1"
            ],
        )
        .expect("simulate crash after Host acceptance was durably recorded");
    drop(connection);

    let syncing = status_runtime(&directory, &fixture, a1)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("accepted WAL status");
    assert_eq!(syncing.state, crate::secure::GroupSecureState::Syncing);
    let repaired = status_runtime(&directory, &fixture, a1)
        .repair(
            crate::ids::GroupRef::parse(GROUP_DID).unwrap(),
            "req-repair-accepted-wal",
        )
        .expect("finish accepted WAL from persisted local state");
    assert!(repaired.repaired);
    assert_eq!(repaired.state, crate::secure::GroupSecureState::Ready);

    let a2_status = status_runtime(&directory, &fixture, a2)
        .status(crate::ids::GroupRef::parse(GROUP_DID).unwrap())
        .expect("A2 remains independent");
    assert_eq!(
        a2_status.state,
        crate::secure::GroupSecureState::MissingLocalState
    );
    assert_ne!(
        a1_store.state_db_path(),
        store(&directory, &fixture, a2).state_db_path()
    );
}

fn product(
    directory: &TestDirectory,
    fixture: &DidFixture,
    device: &DeviceFixture,
    transport: LoopbackTransport,
) -> GroupE2eeV2Product<RpcGroupE2eeV2Host<LoopbackTransport>> {
    let runtime = GroupE2eeV2Runtime::new(store(directory, fixture, device));
    let host = RpcGroupE2eeV2Host::new(
        transport,
        OriginProofIdentity {
            identity_name: format!("identity-{}", device.device_id),
            did_document: Some(fixture.document.clone()),
            key1_private_pem: device.signing_private_pem.clone(),
            verification_method: Some(device.signing_key_id.clone()),
        },
    );
    GroupE2eeV2Product::new(runtime, host)
}

fn consume_public_notice(
    directory: &TestDirectory,
    fixture: &DidFixture,
    recipient: &DeviceFixture,
    input: V2ProcessNoticeInput,
) -> crate::ImResult<anp::group_e2ee::operations::v2::V2ProcessNoticeOutput> {
    let wire = anp::group_e2ee::group_notice_notification_v2(input.meta, input.notice)
        .map_err(map_v2_wire_error)?;
    let (meta, notice) = crate::internal::group_e2ee::v2_notice::parse_notice(&wire)?;
    crate::internal::group_e2ee::v2_notice::consume_with_runtime(
        &GroupE2eeV2Runtime::new(store(directory, fixture, recipient)),
        meta,
        notice,
        input.member_documents,
        NOW.to_owned(),
        input.request_id,
    )
}

fn status_runtime(
    directory: &TestDirectory,
    fixture: &DidFixture,
    device: &DeviceFixture,
) -> crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime {
    crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime::new(GroupE2eeV2Runtime::new(
        store(directory, fixture, device),
    ))
}

fn store(
    directory: &TestDirectory,
    fixture: &DidFixture,
    device: &DeviceFixture,
) -> ImCoreSqliteGroupMlsStore {
    ImCoreSqliteGroupMlsStore::new_scoped_state_db(
        directory
            .path()
            .join(format!("{}.sqlite", device.device_id)),
        GroupMlsOwnerScope::new(
            format!("identity-{}", device.device_id),
            fixture.did.clone(),
            device.device_id.clone(),
        )
        .expect("owner scope"),
    )
}

fn signing_key(device: &DeviceFixture) -> PrivateKeyMaterial {
    PrivateKeyMaterial::from_pem(&device.signing_private_pem).expect("device signing key")
}

fn committed_group_attachment(
) -> crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
    let e2ee = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
        "p6-v2.pdf",
        "application/pdf",
        b"one uploaded encrypted group object".to_vec(),
    )
    .expect("prepare encrypted attachment object");
    let slot = crate::internal::wire::attachment::AttachmentCreateSlotResult {
        attachment_id: "att-p6-v2".to_owned(),
        slot_id: "slot-p6-v2".to_owned(),
        upload_uri: "https://upload.example/slot-p6-v2".to_owned(),
        upload_headers: serde_json::Map::new(),
        object_uri: "https://objects.example/att-p6-v2".to_owned(),
        commit_token: "commit-p6-v2".to_owned(),
        expires_at: EXPIRES_AT.to_owned(),
        request_service_did: SERVICE_DID.to_owned(),
    };
    let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
        &e2ee.prepared,
        slot.attachment_id.clone(),
        slot.object_uri.clone(),
    );
    let full_manifest =
        crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
            &descriptor,
            "P6 v2 attachment",
            &e2ee.secrets,
        )
        .expect("full attachment manifest");
    let redacted_manifest =
        crate::attachments::manifest::build_attachment_manifest(&descriptor, "P6 v2 attachment")
            .expect("redacted attachment manifest");
    let grant_ref = crate::attachments::manifest::build_attachment_grant_ref(&descriptor)
        .expect("attachment grant ref");
    crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
        target_kind: "group",
        target_did: GROUP_DID.to_owned(),
        prepared: e2ee.prepared,
        slot,
        descriptor,
        redacted_manifest,
        full_manifest,
        grant_ref,
    }
}

fn key_package_input(
    fixture: &DidFixture,
    device: &DeviceFixture,
    key_package_id: &str,
    request_id: &str,
) -> V2GenerateKeyPackageInput {
    V2GenerateKeyPackageInput {
        owner_did: fixture.did.clone(),
        owner_device_id: device.device_id.clone(),
        verification_method: device.signing_key_id.clone(),
        key_package_id: key_package_id.to_owned(),
        issued_at: ISSUED_AT.to_owned(),
        expires_at: EXPIRES_AT.to_owned(),
        now: NOW.to_owned(),
        draft_extension_negotiated: true,
        request_id: request_id.to_owned(),
    }
}

fn state_ref(version: u64) -> V2GroupStateRef {
    V2GroupStateRef {
        group_did: GROUP_DID.to_owned(),
        group_state_version: version.to_string(),
        policy_hash: None,
        roster_hash: None,
    }
}

fn key_service_meta(did: &str, device_id: &str, operation_id: &str) -> V2ServiceMetadata {
    V2ServiceMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_TRANSPORT_PROFILE_V2.to_owned(),
        sender_did: did.to_owned(),
        sender_device_id: device_id.to_owned(),
        target: V2Target {
            kind: "service".to_owned(),
            did: SERVICE_DID.to_owned(),
        },
        operation_id: operation_id.to_owned(),
        created_at: Some(NOW.to_owned()),
    }
}

fn control_service_meta(did: &str, device_id: &str, operation_id: &str) -> V2ServiceMetadata {
    V2ServiceMetadata {
        security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        ..key_service_meta(did, device_id, operation_id)
    }
}

fn control_meta(did: &str, device_id: &str, operation_id: &str) -> V2GroupControlMetadata {
    V2GroupControlMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        sender_did: did.to_owned(),
        sender_device_id: device_id.to_owned(),
        target: V2Target {
            kind: "group".to_owned(),
            did: GROUP_DID.to_owned(),
        },
        operation_id: operation_id.to_owned(),
        created_at: Some(NOW.to_owned()),
    }
}

fn send_meta(did: &str, device_id: &str, suffix: &str) -> V2GroupSendMetadata {
    V2GroupSendMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        sender_did: did.to_owned(),
        sender_device_id: device_id.to_owned(),
        target: V2Target {
            kind: "group".to_owned(),
            did: GROUP_DID.to_owned(),
        },
        operation_id: format!("op-send-{suffix}"),
        message_id: format!("msg-{suffix}"),
        content_type: GROUP_CIPHER_CONTENT_TYPE_V2.to_owned(),
        created_at: Some(NOW.to_owned()),
    }
}

fn notice_meta(did: &str, device_id: &str, operation_id: &str) -> V2GroupNoticeMetadata {
    V2GroupNoticeMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_TRANSPORT_PROFILE_V2.to_owned(),
        sender_did: GROUP_DID.to_owned(),
        target: V2Target {
            kind: "agent".to_owned(),
            did: did.to_owned(),
        },
        recipient_device_id: device_id.to_owned(),
        operation_id: operation_id.to_owned(),
        created_at: Some(NOW.to_owned()),
    }
}

fn welcome_notice(
    fixture: &DidFixture,
    recipient: &DeviceFixture,
    add: &V2GroupAddBody,
    notice_id: &str,
) -> V2ProcessNoticeInput {
    V2ProcessNoticeInput {
        recipient_did: fixture.did.clone(),
        recipient_device_id: recipient.device_id.clone(),
        meta: notice_meta(&fixture.did, &recipient.device_id, notice_id),
        notice: V2E2eeNotice {
            notice_id: Some(notice_id.to_owned()),
            notice_type: "welcome-delivery".to_owned(),
            group_did: GROUP_DID.to_owned(),
            group_state_ref: add.group_state_ref.clone(),
            crypto_group_id_b64u: add.crypto_group_id_b64u.clone(),
            epoch: add.epoch.clone(),
            subject_did: fixture.did.clone(),
            subject_device_id: recipient.device_id.clone(),
            subject_status: "active".to_owned(),
            commit_b64u: None,
            welcome_b64u: Some(add.welcome_b64u.clone()),
            ratchet_tree_b64u: Some(add.ratchet_tree_b64u.clone()),
            epoch_authenticator: None,
            group_receipt: None,
        },
        member_documents: documents(fixture),
        now: NOW.to_owned(),
        draft_extension_negotiated: true,
        request_id: format!("req-{notice_id}"),
    }
}

fn commit_notice(
    fixture: &DidFixture,
    recipient: &DeviceFixture,
    add: &V2GroupAddBody,
    notice_id: &str,
    status: &str,
) -> V2ProcessNoticeInput {
    V2ProcessNoticeInput {
        recipient_did: fixture.did.clone(),
        recipient_device_id: recipient.device_id.clone(),
        meta: notice_meta(&fixture.did, &recipient.device_id, notice_id),
        notice: V2E2eeNotice {
            notice_id: Some(notice_id.to_owned()),
            notice_type: "commit-delivery".to_owned(),
            group_did: GROUP_DID.to_owned(),
            group_state_ref: add.group_state_ref.clone(),
            crypto_group_id_b64u: add.crypto_group_id_b64u.clone(),
            epoch: add.epoch.clone(),
            subject_did: add.member_did.clone(),
            subject_device_id: add.member_device_id.clone(),
            subject_status: status.to_owned(),
            commit_b64u: Some(add.commit_b64u.clone()),
            welcome_b64u: None,
            ratchet_tree_b64u: None,
            epoch_authenticator: None,
            group_receipt: None,
        },
        member_documents: documents(fixture),
        now: NOW.to_owned(),
        draft_extension_negotiated: true,
        request_id: format!("req-{notice_id}"),
    }
}

fn remove_commit_notice(
    fixture: &DidFixture,
    recipient: &DeviceFixture,
    remove: &V2GroupRemoveBody,
    notice_id: &str,
) -> V2ProcessNoticeInput {
    V2ProcessNoticeInput {
        recipient_did: fixture.did.clone(),
        recipient_device_id: recipient.device_id.clone(),
        meta: notice_meta(&fixture.did, &recipient.device_id, notice_id),
        notice: V2E2eeNotice {
            notice_id: Some(notice_id.to_owned()),
            notice_type: "commit-delivery".to_owned(),
            group_did: GROUP_DID.to_owned(),
            group_state_ref: remove.group_state_ref.clone(),
            crypto_group_id_b64u: remove.crypto_group_id_b64u.clone(),
            epoch: remove.epoch.clone(),
            subject_did: remove.member_did.clone(),
            subject_device_id: remove.member_device_id.clone(),
            subject_status: "removed".to_owned(),
            commit_b64u: Some(remove.commit_b64u.clone()),
            welcome_b64u: None,
            ratchet_tree_b64u: None,
            epoch_authenticator: None,
            group_receipt: None,
        },
        member_documents: documents(fixture),
        now: NOW.to_owned(),
        draft_extension_negotiated: true,
        request_id: format!("req-{notice_id}"),
    }
}

fn incoming_input(
    fixture: &DidFixture,
    sender: &DeviceFixture,
    recipient: &DeviceFixture,
    send: &V2PreparedApplicationSend,
    request_id: &str,
) -> V2IncomingApplicationInput {
    let auth = origin_auth(
        fixture,
        sender,
        METHOD_GROUP_SEND_V2,
        &send.meta,
        &send.cipher,
    );
    V2IncomingApplicationInput {
        recipient_did: fixture.did.clone(),
        recipient_device_id: recipient.device_id.clone(),
        meta: V2GroupIncomingMetadata {
            anp_version: send.meta.anp_version.clone(),
            profile: send.meta.profile.clone(),
            security_profile: send.meta.security_profile.clone(),
            sender_did: send.meta.sender_did.clone(),
            sender_device_id: send.meta.sender_device_id.clone(),
            target: V2Target {
                kind: "agent".to_owned(),
                did: fixture.did.clone(),
            },
            recipient_device_id: recipient.device_id.clone(),
            operation_id: send.meta.operation_id.clone(),
            message_id: send.meta.message_id.clone(),
            content_type: send.meta.content_type.clone(),
            created_at: send.meta.created_at.clone(),
        },
        body: V2GroupIncomingBody {
            group_did: GROUP_DID.to_owned(),
            group_state_version: send.cipher.group_state_ref.group_state_version.clone(),
            group_event_seq: "1".to_owned(),
            accepted_at: NOW.to_owned(),
            group_receipt: json!({"test": true}),
            group_cipher_object: send.cipher.clone(),
        },
        auth,
        sender_did_document: fixture.document.clone(),
        now: NOW.to_owned(),
        draft_extension_negotiated: true,
        request_id: request_id.to_owned(),
    }
}

fn origin_auth<M: serde::Serialize, B: serde::Serialize>(
    fixture: &DidFixture,
    device: &DeviceFixture,
    method: &str,
    meta: &M,
    body: &B,
) -> V2OriginAuth {
    let proof = crate::internal::proof::origin::build_origin_proof(
        &OriginProofIdentity {
            identity_name: format!("identity-{}", device.device_id),
            did_document: Some(fixture.document.clone()),
            key1_private_pem: device.signing_private_pem.clone(),
            verification_method: Some(device.signing_key_id.clone()),
        },
        &DirectPayload {
            method: method.to_owned(),
            meta: serde_json::to_value(meta).expect("serialize meta"),
            body: serde_json::to_value(body).expect("serialize body"),
        },
    )
    .expect("origin proof");
    serde_json::from_value(crate::internal::proof::origin::origin_auth_value(&proof))
        .expect("typed origin auth")
}

fn documents(fixture: &DidFixture) -> Vec<V2DidDocument> {
    vec![V2DidDocument {
        did: fixture.did.clone(),
        document: fixture.document.clone(),
    }]
}

fn p6_profiles() -> Value {
    json!([
        "anp.core.binding.v1",
        "anp.identity.discovery.v1",
        "anp.group.base.v1",
        "anp.group.e2ee.v2"
    ])
}

fn make_did_fixture(label: &str, device_ids: &[&str]) -> DidFixture {
    assert!(!device_ids.is_empty());
    let primary = create_did_wba_document(
        "p6-core.example",
        DidDocumentOptions {
            path_segments: vec!["agents".to_owned(), label.to_owned()],
            did_profile: DidProfile::E1,
            created: Some(ISSUED_AT.to_owned()),
            ..Default::default()
        },
    )
    .expect("primary DID document");
    let did = primary.did().expect("primary DID").to_owned();
    let root_key = PrivateKeyMaterial::from_pem(&primary.keys["key-1"].private_key_pem)
        .expect("primary signing key");
    let mut document = primary.did_document.clone();
    document
        .as_object_mut()
        .expect("DID object")
        .remove("proof");

    let mut devices = vec![DeviceFixture {
        device_id: device_ids[0].to_owned(),
        signing_key_id: format!("{did}#key-1"),
        signing_private_pem: primary.keys["key-1"].private_key_pem.clone(),
    }];
    for (index, device_id) in device_ids.iter().enumerate().skip(1) {
        let scratch = create_did_wba_document(
            "p6-core.example",
            DidDocumentOptions {
                path_segments: vec!["scratch".to_owned(), label.to_owned(), index.to_string()],
                did_profile: DidProfile::E1,
                created: Some(ISSUED_AT.to_owned()),
                ..Default::default()
            },
        )
        .expect("additional device keys");
        let signing_key_id = format!("{did}#device-{index}-sign");
        let e2ee_key_id = format!("{did}#device-{index}-e2ee");
        let mut signing_method = scratch.did_document["verificationMethod"]
            .as_array()
            .expect("scratch methods")
            .iter()
            .find(|method| {
                method["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with("#key-1"))
            })
            .expect("scratch signing method")
            .clone();
        signing_method["id"] = json!(signing_key_id);
        signing_method["controller"] = json!(did);
        let mut e2ee_method = scratch.did_document["verificationMethod"]
            .as_array()
            .expect("scratch methods")
            .iter()
            .find(|method| {
                method["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with("#key-3"))
            })
            .expect("scratch E2EE method")
            .clone();
        e2ee_method["id"] = json!(e2ee_key_id);
        e2ee_method["controller"] = json!(did);
        document["verificationMethod"]
            .as_array_mut()
            .expect("verification methods")
            .extend([signing_method, e2ee_method]);
        document["authentication"]
            .as_array_mut()
            .expect("authentication")
            .push(json!(signing_key_id));
        document["assertionMethod"]
            .as_array_mut()
            .expect("assertionMethod")
            .push(json!(signing_key_id));
        document["keyAgreement"]
            .as_array_mut()
            .expect("keyAgreement")
            .push(json!(e2ee_key_id));
        devices.push(DeviceFixture {
            device_id: (*device_id).to_owned(),
            signing_key_id,
            signing_private_pem: scratch.keys["key-1"].private_key_pem.clone(),
        });
    }

    document["deviceManifest"] = json!({
        "type": "ANPDeviceManifest",
        "devices": devices.iter().enumerate().map(|(index, device)| {
            json!({
                "device_id": device.device_id,
                "signing_key_id": device.signing_key_id,
                "e2ee_key_id": if index == 0 {
                    format!("{did}#key-3")
                } else {
                    format!("{did}#device-{index}-e2ee")
                },
                "profiles": p6_profiles()
            })
        }).collect::<Vec<_>>()
    });
    document = generate_w3c_proof(
        &document,
        &root_key,
        &format!("{did}#key-1"),
        ProofGenerationOptions {
            proof_purpose: Some("assertionMethod".to_owned()),
            proof_type: Some(PROOF_TYPE_DATA_INTEGRITY.to_owned()),
            cryptosuite: Some(CRYPTOSUITE_EDDSA_JCS_2022.to_owned()),
            created: Some(ISSUED_AT.to_owned()),
            ..Default::default()
        },
    )
    .expect("signed DID document");
    validate_device_manifest(&document).expect("valid device Manifest");
    DidFixture {
        did,
        document,
        devices,
    }
}
