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
    let disposition = a2_product
        .consume_notice(welcome.clone())
        .expect("A2 consumes standard Welcome notice");
    let V2ControlDisposition::ConsumedControl(output) = disposition;
    assert_eq!(output.notice_type, "welcome-delivery");
    assert_eq!(output.epoch, "1");
    let replay = product(&directory, &fixture, a2, transport.clone())
        .consume_notice(V2ProcessNoticeInput {
            request_id: "req-welcome-replay-after-restart".to_owned(),
            ..welcome
        })
        .expect("Welcome replay remains control-only and idempotent");
    assert!(matches!(replay, V2ControlDisposition::ConsumedControl(_)));

    let wrong_target = welcome_notice(&fixture, a2, &add.prepared.body, "notice-wrong-target");
    let mut wrong_target = wrong_target;
    wrong_target.meta.recipient_device_id = a1.device_id.clone();
    assert!(a2_product.consume_notice(wrong_target).is_err());

    let self_echo = commit_notice(
        &fixture,
        a1,
        &add.prepared.body,
        "notice-add-a2-self-echo",
        "active",
    );
    let V2ControlDisposition::ConsumedControl(self_echo_output) = a1_product
        .consume_notice(self_echo.clone())
        .expect("A1 records its exact finalized Commit echo without merging twice");
    assert_eq!(
        self_echo_output.source_operation_id.as_deref(),
        Some("op-add-a2")
    );
    let replayed_self_echo = product(&directory, &fixture, a1, transport.clone())
        .consume_notice(V2ProcessNoticeInput {
            request_id: "req-add-a2-self-echo-replay-after-restart".to_owned(),
            ..self_echo
        })
        .expect("A1 self-echo receipt replays after restart");
    assert_eq!(
        replayed_self_echo,
        V2ControlDisposition::ConsumedControl(self_echo_output)
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
    let V2ControlDisposition::ConsumedControl(remove_output) = a2_product
        .consume_notice(remove_notice)
        .expect("A2 consumes its exact Remove Commit notice");
    assert!(remove_output.self_removed);

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
    assert_ne!(
        store(&directory, &fixture, a1).state_db_path(),
        store(&directory, &fixture, a2).state_db_path()
    );
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
        "anp.core.binding.v2",
        "anp.identity.discovery.v2",
        "anp.group.base.v2",
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
