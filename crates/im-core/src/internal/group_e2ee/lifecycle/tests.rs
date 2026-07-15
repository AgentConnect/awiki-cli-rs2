use super::*;
use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};
use anp::group_e2ee::operations::{
    AbortCommitOutput, DecryptInput, DecryptOutput, EncryptInput, EncryptOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, ProcessNoticeInput, ProcessNoticeOutput,
    ProcessWelcomeInput, ProcessWelcomeOutput, RecoverMemberInput, StatusInput, StatusOutput,
    UpdateMemberInput,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[cfg(feature = "blocking")]
#[test]
fn lifecycle_create_prepares_delivers_finalizes_and_persists_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.create".to_owned(),
                json!({
                    "accepted": true,
                    "group_did": "did:example:groups:e2ee",
                    "group_state_version": "state-0",
                    "epoch": "0"
                }),
            )],
        },
        provider.clone(),
    )
    .create_secure_group(GroupE2eeCreateInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        group_state_ref: None,
    })
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(provider.created().as_slice(), ["did:example:groups:e2ee"]);
    assert_eq!(provider.finalized().as_slice(), ["pc-create"]);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.create");
    assert_eq!(
        calls[0].params["meta"]["target"],
        json!({"kind":"service","did":"did:example:service"})
    );
    assert_eq!(
        calls[0].params["body"]["group_did"],
        "did:example:groups:e2ee"
    );
    assert_eq!(calls[0].params["body"]["epoch"], "0");
    assert!(calls[0].params["body"].get("commit_b64u").is_none());
    assert!(
        stored_group_metadata(&fixture, &client, "did:example:groups:e2ee")
            .to_string()
            .contains("group-e2ee")
    );
}

#[tokio::test]
async fn lifecycle_create_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.create".to_owned(),
                json!({
                    "accepted": true,
                    "group_did": "did:example:groups:e2ee",
                    "group_state_version": "state-async-0",
                    "epoch": "0"
                }),
            )],
        },
        provider.clone(),
    )
    .create_secure_group_async(GroupE2eeCreateInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        group_state_ref: None,
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(provider.created().as_slice(), ["did:example:groups:e2ee"]);
    assert_eq!(provider.finalized().as_slice(), ["pc-create"]);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.create");
    assert_eq!(
        calls[0].params["meta"]["target"],
        json!({"kind":"service","did":"did:example:service"})
    );
    assert_eq!(
        calls[0].params["body"]["group_did"],
        "did:example:groups:e2ee"
    );

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-0"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn lifecycle_add_gets_key_package_prepares_commit_and_finalizes() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json("did:example:bob")}),
                ),
                (
                    "group.e2ee.add".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "state-2",
                        "epoch": "2"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .add_secure_member(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: None,
        leave_request_id: None,
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        group_state_ref: None,
        operation_id: None,
    })
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.added().as_slice(),
        ["did:example:groups:e2ee:did:example:bob"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-add"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.add"]
    );
    assert_eq!(calls[0].params["body"]["target_did"], "did:example:bob");
    assert_eq!(calls[1].params["body"]["member_did"], "did:example:bob");
    assert_eq!(
        calls[1].params["body"]["subject_key_package_id"],
        "kp-did:example:bob"
    );
    assert_eq!(calls[1].params["body"]["commit_b64u"], "commit-add");
    assert_eq!(calls[1].params["body"]["welcome_b64u"], "welcome-add");
}

#[tokio::test]
async fn lifecycle_add_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json("did:example:bob")}),
                ),
                (
                    "group.e2ee.add".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "state-async-2",
                        "epoch": "2"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .add_secure_member_async(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: None,
        leave_request_id: None,
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        group_state_ref: None,
        operation_id: None,
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.added().as_slice(),
        ["did:example:groups:e2ee:did:example:bob"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-add"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.add"]
    );
    assert_eq!(calls[0].params["body"]["target_did"], "did:example:bob");
    assert_eq!(calls[1].params["body"]["member_did"], "did:example:bob");
    assert_eq!(calls[1].params["body"]["commit_b64u"], "commit-add");
    assert_eq!(calls[1].params["body"]["welcome_b64u"], "welcome-add");

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-2"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn lifecycle_public_delivery_redacts_mls_artifacts_from_service_response() {
    let delivery = public_lifecycle_delivery(
        "secure_group_remove_member",
        "did:example:groups:e2ee",
        Some("did:example:bob"),
        Some("removed"),
        &json!({
            "accepted": true,
            "group_did": "did:example:groups:e2ee",
            "member_did": "did:example:bob",
            "subject_status": "removed",
            "epoch": "3",
            "group_state_ref": {
                "group_state_version": "state-3",
                "crypto_group_id_b64u": "secret-crypto-group"
            },
            "commit_b64u": "secret-commit",
            "welcome_b64u": "secret-welcome",
            "ratchet_tree_b64u": "secret-ratchet",
            "group_key_package": key_package_json("did:example:bob"),
            "e2ee_notice": {
                "notice_type": "commit-delivery",
                "commit_b64u": "secret-notice-commit"
            }
        }),
    );

    assert_eq!(delivery["action"], "secure_group_remove_member");
    assert_eq!(delivery["secure"], true);
    assert_eq!(delivery["group_did"], "did:example:groups:e2ee");
    assert_eq!(delivery["member_did"], "did:example:bob");
    assert_eq!(delivery["subject_status"], "removed");
    assert_eq!(delivery["group_state"]["epoch"], "3");
    assert_eq!(delivery["group_state"]["group_state_version"], "state-3");
    let encoded = delivery.to_string();
    for secret in [
        "secret-commit",
        "secret-welcome",
        "secret-ratchet",
        "secret-notice-commit",
        "secret-crypto-group",
        "mls_key_package_b64u",
        "group_key_package",
        "e2ee_notice",
    ] {
        assert!(!encoded.contains(secret), "{encoded}");
    }
}

#[test]
fn lifecycle_remove_aborts_pending_commit_on_deterministic_service_rejection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let err = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.remove".to_owned(),
                json!({"error": {"code": "invalid_argument", "message": "bad remove"}}),
            )],
        },
        provider.clone(),
    )
    .remove_secure_member(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: Some("cleanup".to_owned()),
        leave_request_id: Some("leave-1".to_owned()),
        credentials: Some(fixture.credentials()),
        service_did: None,
        group_state_ref: None,
        operation_id: None,
    })
    .unwrap_err();

    assert!(err.to_string().contains("was aborted"));
    assert_eq!(provider.removed().as_slice(), ["did:example:bob"]);
    assert_eq!(provider.aborted().as_slice(), ["pc-remove"]);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.remove");
    assert_eq!(calls[0].params["body"]["reason_text"], "cleanup");
    assert_eq!(calls[0].params["body"]["leave_request_id"], "leave-1");
}

#[test]
fn lifecycle_remove_returns_redacted_public_delivery() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.remove".to_owned(),
                json!({
                    "accepted": true,
                    "group_did": "did:example:groups:e2ee",
                    "member_did": "did:example:bob",
                    "subject_status": "removed",
                    "epoch": "3",
                    "group_state_ref": {
                        "group_state_version": "state-3",
                        "crypto_group_id_b64u": "secret-crypto-group"
                    },
                    "commit_b64u": "secret-service-commit",
                    "e2ee_notice": {
                        "notice_type": "commit-delivery",
                        "commit_b64u": "secret-notice-commit"
                    }
                }),
            )],
        },
        provider,
    )
    .remove_secure_member(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: None,
        leave_request_id: None,
        credentials: Some(fixture.credentials()),
        service_did: None,
        group_state_ref: None,
        operation_id: None,
    })
    .unwrap();

    assert_eq!(result.delivery["action"], "secure_group_remove_member");
    assert_eq!(result.delivery["accepted"], true);
    assert_eq!(result.delivery["member_did"], "did:example:bob");
    assert_eq!(result.delivery["group_state"]["epoch"], "3");
    assert_eq!(
        result.delivery["group_state"]["group_state_version"],
        "state-3"
    );
    let encoded = result.delivery.to_string();
    assert!(!encoded.contains("secret-service-commit"), "{encoded}");
    assert!(!encoded.contains("secret-notice-commit"), "{encoded}");
    assert!(!encoded.contains("secret-crypto-group"), "{encoded}");
    assert!(!encoded.contains("e2ee_notice"), "{encoded}");

    let calls = calls.borrow();
    assert_eq!(calls[0].method, "group.e2ee.remove");
    assert_eq!(calls[0].params["body"]["commit_b64u"], "commit-remove");
}

#[tokio::test]
async fn lifecycle_remove_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.remove".to_owned(),
                json!({
                    "accepted": true,
                    "group_did": "did:example:groups:e2ee",
                    "member_did": "did:example:bob",
                    "subject_status": "removed",
                    "group_state_version": "state-async-3",
                    "epoch": "3"
                }),
            )],
        },
        provider.clone(),
    )
    .remove_secure_member_async(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: Some("remove".to_owned()),
        leave_request_id: Some("leave-1".to_owned()),
        credentials: Some(fixture.credentials()),
        service_did: None,
        group_state_ref: None,
        operation_id: None,
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(provider.removed().as_slice(), ["did:example:bob"]);
    assert_eq!(provider.finalized().as_slice(), ["pc-remove"]);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.remove");
    assert_eq!(calls[0].params["body"]["member_did"], "did:example:bob");
    assert_eq!(calls[0].params["body"]["reason_text"], "remove");
    assert_eq!(calls[0].params["body"]["leave_request_id"], "leave-1");
    assert_eq!(result.delivery["action"], "secure_group_remove_member");
    assert_eq!(result.delivery["subject_status"], "removed");

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-3"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn lifecycle_leave_request_uses_high_level_request_without_local_finalize() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.leave_request".to_owned(),
                json!({"accepted": true, "leave_request_id": "leave-request-1"}),
            )],
        },
        provider.clone(),
    )
    .leave_secure_group(GroupE2eeLeaveInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        reason_text: Some("bye".to_owned()),
        owner_leave_commit: false,
        credentials: Some(fixture.credentials()),
    })
    .unwrap();

    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("owner must process")));
    assert!(provider.finalized().is_empty());
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.leave_request");
    assert_eq!(calls[0].params["body"]["subject_status"], "leave_requested");
    assert_eq!(calls[0].params["body"]["reason_text"], "bye");
}

#[test]
fn lifecycle_leave_request_returns_redacted_public_delivery() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.leave_request".to_owned(),
                json!({
                    "accepted": true,
                    "leave_request_id": "leave-request-1",
                    "e2ee_notice": {
                        "notice_type": "leave-request",
                        "welcome_b64u": "secret-welcome"
                    }
                }),
            )],
        },
        provider,
    )
    .leave_secure_group(GroupE2eeLeaveInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        reason_text: Some("bye".to_owned()),
        owner_leave_commit: false,
        credentials: Some(fixture.credentials()),
    })
    .unwrap();

    assert_eq!(result.delivery["action"], "secure_group_leave_request");
    assert_eq!(result.delivery["accepted"], true);
    assert_eq!(result.delivery["leave_request_id"], "leave-request-1");
    assert_eq!(result.delivery["subject_status"], "leave_requested");
    let encoded = result.delivery.to_string();
    assert!(!encoded.contains("secret-welcome"), "{encoded}");
    assert!(!encoded.contains("e2ee_notice"), "{encoded}");

    let calls = calls.borrow();
    assert_eq!(calls[0].method, "group.e2ee.leave_request");
}

#[tokio::test]
async fn lifecycle_leave_request_async_uses_async_transport_without_local_mls() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut transport = RecordingTransport {
        calls: Rc::clone(&calls),
        responses: vec![(
            "group.e2ee.leave_request".to_owned(),
            json!({"accepted": true, "leave_request_id": "leave-request-async-1"}),
        )],
    };
    let result = leave_secure_group_request_async(
        &client,
        &ReadySessionProvider,
        &mut transport,
        GroupE2eeLeaveInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            reason_text: Some("bye async".to_owned()),
            owner_leave_commit: false,
            credentials: Some(fixture.credentials()),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.delivery["action"], "secure_group_leave_request");
    assert_eq!(result.delivery["accepted"], true);
    assert_eq!(result.delivery["leave_request_id"], "leave-request-async-1");
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("owner must process")));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.leave_request");
    assert_eq!(calls[0].params["body"]["reason_text"], "bye async");
    assert_eq!(calls[0].params["body"]["subject_status"], "leave_requested");
}

#[tokio::test]
async fn lifecycle_leave_request_async_rejects_owner_commit_fallback() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut transport = RecordingTransport {
        calls: Rc::clone(&calls),
        responses: Vec::new(),
    };
    let err = leave_secure_group_request_async(
        &client,
        &ReadySessionProvider,
        &mut transport,
        GroupE2eeLeaveInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            reason_text: None,
            owner_leave_commit: true,
            credentials: Some(fixture.credentials()),
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        crate::ImError::UnsupportedCapability { ref capability }
            if capability == "group-e2ee-owner-leave-commit-async"
    ));
    assert!(calls.borrow().is_empty());
}

#[test]
fn lifecycle_update_key_leases_update_package_and_finalizes() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json_with_purpose(
                        "did:example:bob",
                        "update"
                    )}),
                ),
                (
                    "group.e2ee.update".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "target_did": "did:example:bob",
                        "device_id": "default",
                        "update_key_package_id": "kp-did:example:bob",
                        "group_state_version": "state-5",
                        "epoch": "5"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .update_member_key(GroupE2eeKeyReplacementInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        device_id: "default".to_owned(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
    })
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.updated().as_slice(),
        ["did:example:groups:e2ee:did:example:bob:default"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-update"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.update"]
    );
    assert_eq!(calls[0].params["body"]["purpose"], "update");
    assert_eq!(calls[0].params["body"]["device_id"], "default");
    assert_eq!(
        calls[1].params["body"]["target"]["agent_did"],
        "did:example:bob"
    );
    assert_eq!(calls[1].params["body"]["target"]["device_id"], "default");
    assert_eq!(
        calls[1].params["body"]["update_key_package_id"],
        "kp-did:example:bob"
    );
    assert_eq!(
        calls[1].params["body"]["group_key_package"]["purpose"],
        "update"
    );
    assert_eq!(result.delivery["action"], "secure_group_update_key");
    assert_eq!(result.delivery["group_state"]["epoch"], "5");
}

#[tokio::test]
async fn lifecycle_update_key_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json_with_purpose(
                        "did:example:bob",
                        "update"
                    )}),
                ),
                (
                    "group.e2ee.update".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "target_did": "did:example:bob",
                        "device_id": "default",
                        "update_key_package_id": "kp-did:example:bob",
                        "group_state_version": "state-async-5",
                        "epoch": "5"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .update_member_key_async(GroupE2eeKeyReplacementInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        device_id: "default".to_owned(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.updated().as_slice(),
        ["did:example:groups:e2ee:did:example:bob:default"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-update"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.update"]
    );
    assert_eq!(calls[0].params["body"]["purpose"], "update");
    assert_eq!(calls[0].params["body"]["device_id"], "default");
    assert_eq!(
        calls[1].params["body"]["update_key_package_id"],
        "kp-did:example:bob"
    );
    assert_eq!(
        calls[1].params["body"]["group_key_package"]["purpose"],
        "update"
    );
    assert_eq!(result.delivery["action"], "secure_group_update_key");
    assert_eq!(result.delivery["group_state"]["epoch"], "5");

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-5"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn lifecycle_recover_member_leases_recovery_package_and_finalizes() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json_with_purpose(
                        "did:example:bob",
                        "recovery"
                    )}),
                ),
                (
                    "group.e2ee.recover_member".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "target_did": "did:example:bob",
                        "device_id": "default",
                        "recovery_key_package_id": "kp-did:example:bob",
                        "group_state_version": "state-6",
                        "epoch": "6"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .recover_member(GroupE2eeKeyReplacementInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        device_id: "default".to_owned(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
    })
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.recovered().as_slice(),
        ["did:example:groups:e2ee:did:example:bob:default"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-recover"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.recover_member"]
    );
    assert_eq!(calls[0].params["body"]["purpose"], "recovery");
    assert_eq!(
        calls[1].params["body"]["recovery_key_package_id"],
        "kp-did:example:bob"
    );
    assert_eq!(
        calls[1].params["body"]["group_key_package"]["purpose"],
        "recovery"
    );
    assert_eq!(result.delivery["action"], "secure_group_recover_member");
    assert_eq!(result.delivery["group_state"]["epoch"], "6");
}

#[tokio::test]
async fn lifecycle_recover_member_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"group_key_package": key_package_json_with_purpose(
                        "did:example:bob",
                        "recovery"
                    )}),
                ),
                (
                    "group.e2ee.recover_member".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "target_did": "did:example:bob",
                        "device_id": "default",
                        "recovery_key_package_id": "kp-did:example:bob",
                        "group_state_version": "state-async-6",
                        "epoch": "6"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .recover_member_async(GroupE2eeKeyReplacementInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        device_id: "default".to_owned(),
        credentials: Some(fixture.credentials()),
        service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(
        provider.recovered().as_slice(),
        ["did:example:groups:e2ee:did:example:bob:default"]
    );
    assert_eq!(provider.finalized().as_slice(), ["pc-recover"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.get_key_package", "group.e2ee.recover_member"]
    );
    assert_eq!(calls[0].params["body"]["purpose"], "recovery");
    assert_eq!(
        calls[1].params["body"]["recovery_key_package_id"],
        "kp-did:example:bob"
    );
    assert_eq!(
        calls[1].params["body"]["group_key_package"]["purpose"],
        "recovery"
    );
    assert_eq!(result.delivery["action"], "secure_group_recover_member");
    assert_eq!(result.delivery["group_state"]["epoch"], "6");

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-6"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn lifecycle_process_leave_request_marks_processing_then_removes() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.process_leave_request".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "leave_request_id": "leave-1",
                        "pending_leave_request_count": 0
                    }),
                ),
                (
                    "group.e2ee.remove".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "subject_did": "did:example:bob",
                        "subject_status": "removed",
                        "leave_request_id": "leave-1",
                        "group_state_version": "state-3",
                        "epoch": "3"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .process_leave_request(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: Some("approved leave".to_owned()),
        leave_request_id: Some("leave-1".to_owned()),
        credentials: Some(fixture.credentials()),
        service_did: None,
        group_state_ref: None,
        operation_id: None,
    })
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(provider.removed().as_slice(), ["did:example:bob"]);
    assert_eq!(provider.finalized().as_slice(), ["pc-remove"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.process_leave_request", "group.e2ee.remove"]
    );
    assert_eq!(calls[0].params["body"]["leave_request_id"], "leave-1");
    assert_eq!(calls[1].params["body"]["leave_request_id"], "leave-1");
    assert_eq!(calls[1].params["body"]["reason_text"], "approved leave");
    assert_eq!(
        result.delivery["action"],
        "secure_group_process_leave_request"
    );
    assert_eq!(result.delivery["subject_status"], "removed");
    assert_eq!(
        result.delivery["process_leave_request"]["leave_request_id"],
        "leave-1"
    );
}

#[tokio::test]
async fn lifecycle_process_leave_request_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::new();
    let result = GroupE2eeLifecycleRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.process_leave_request".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "leave_request_id": "leave-async-1",
                        "pending_leave_request_count": 0
                    }),
                ),
                (
                    "group.e2ee.remove".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "subject_did": "did:example:bob",
                        "subject_status": "removed",
                        "leave_request_id": "leave-async-1",
                        "group_state_version": "state-async-4",
                        "epoch": "4"
                    }),
                ),
            ],
        },
        provider.clone(),
    )
    .process_leave_request_async(GroupE2eeMemberMutationInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        member: crate::ids::Did::parse("did:example:bob").unwrap(),
        reason_text: Some("approved async leave".to_owned()),
        leave_request_id: Some("leave-async-1".to_owned()),
        credentials: Some(fixture.credentials()),
        service_did: None,
        group_state_ref: None,
        operation_id: None,
    })
    .await
    .unwrap();

    assert!(result.warnings.is_empty());
    assert_eq!(provider.removed().as_slice(), ["did:example:bob"]);
    assert_eq!(provider.finalized().as_slice(), ["pc-remove"]);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.process_leave_request", "group.e2ee.remove"]
    );
    assert_eq!(calls[0].params["body"]["leave_request_id"], "leave-async-1");
    assert_eq!(calls[1].params["body"]["leave_request_id"], "leave-async-1");
    assert_eq!(
        calls[1].params["body"]["reason_text"],
        "approved async leave"
    );
    assert_eq!(
        result.delivery["action"],
        "secure_group_process_leave_request"
    );
    assert_eq!(result.delivery["subject_status"], "removed");
    assert_eq!(
        result.delivery["process_leave_request"]["leave_request_id"],
        "leave-async-1"
    );

    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(
        metadata["message_security_profile"],
        crate::internal::group_e2ee::wire::GROUP_E2EE_SECURITY_PROFILE
    );
    assert_eq!(
        metadata["group_e2ee"]["group_state_version"],
        "state-async-4"
    );
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
}

#[test]
fn service_availability_preflight_returns_disabled_gate_error() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut transport = RecordingTransport {
        calls: Rc::clone(&calls),
        responses: vec![(
            "group.e2ee.head".to_owned(),
            json!({"error": {"code": "1405", "message": "group E2EE contract-test APIs are disabled"}}),
        )],
    };

    let err = ensure_group_e2ee_service_available(
        &client,
        &ReadySessionProvider,
        &mut transport,
        GroupE2eeServiceAvailabilityInput {
            credentials: Some(fixture.credentials()),
            service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
            check_key_package: true,
        },
    )
    .unwrap_err();

    assert!(is_group_e2ee_service_disabled(&err));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.e2ee.head");
}

#[test]
fn service_availability_preflight_checks_key_package_gate_when_requested() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut transport = RecordingTransport {
        calls: Rc::clone(&calls),
        responses: vec![
            (
                "group.e2ee.head".to_owned(),
                json!({"error": {"code": "1404", "message": "group E2EE crypto head not found"}}),
            ),
            (
                "group.e2ee.get_key_package".to_owned(),
                json!({"error": {"code": "1405", "message": "group E2EE P6 APIs are disabled"}}),
            ),
        ],
    };

    let err = ensure_group_e2ee_service_available(
        &client,
        &ReadySessionProvider,
        &mut transport,
        GroupE2eeServiceAvailabilityInput {
            credentials: Some(fixture.credentials()),
            service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
            check_key_package: true,
        },
    )
    .unwrap_err();

    assert!(is_group_e2ee_service_disabled(&err));
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.head", "group.e2ee.get_key_package"]
    );
}

#[test]
fn service_availability_preflight_ignores_non_gate_errors() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut transport = RecordingTransport {
        calls: Rc::clone(&calls),
        responses: vec![
            (
                "group.e2ee.head".to_owned(),
                json!({"error": {"code": "1404", "message": "group E2EE crypto head not found"}}),
            ),
            (
                "group.e2ee.get_key_package".to_owned(),
                json!({"error": {"code": "1403", "message": "group.e2ee.get_key_package purpose=normal requires active owner role"}}),
            ),
        ],
    };

    ensure_group_e2ee_service_available(
        &client,
        &ReadySessionProvider,
        &mut transport,
        GroupE2eeServiceAvailabilityInput {
            credentials: Some(fixture.credentials()),
            service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
            check_key_package: true,
        },
    )
    .unwrap();

    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["group.e2ee.head", "group.e2ee.get_key_package"]
    );
}

#[derive(Clone)]
struct ReadySessionProvider;

impl SessionProvider for ReadySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("group E2EE lifecycle should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("group E2EE lifecycle should not read auth status")
    }
}

impl AsyncSessionProvider for ReadySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("group E2EE lifecycle should not refresh through the session provider")
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("group E2EE lifecycle should not read auth status")
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    responses: Vec<(String, Value)>,
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        if let Some((index, _)) = self
            .responses
            .iter()
            .enumerate()
            .find(|(_, (candidate, _))| candidate == method)
        {
            let value = self.responses.remove(index).1;
            if let Some(error) = value.get("error").and_then(Value::as_object) {
                return Err(crate::ImError::Service {
                    status_code: None,
                    code: error.get("code").and_then(Value::as_str).map(str::to_owned),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("service error")
                        .to_owned(),
                    data: None,
                });
            }
            return Ok(value);
        }
        Err(crate::ImError::Service {
            status_code: None,
            code: Some("missing_test_response".to_owned()),
            message: format!("missing response for {method}"),
            data: None,
        })
    }
}

impl AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

#[derive(Clone)]
struct RecordingMlsProvider {
    created: Arc<Mutex<Vec<String>>>,
    added: Arc<Mutex<Vec<String>>>,
    removed: Arc<Mutex<Vec<String>>>,
    updated: Arc<Mutex<Vec<String>>>,
    recovered: Arc<Mutex<Vec<String>>>,
    finalized: Arc<Mutex<Vec<String>>>,
    aborted: Arc<Mutex<Vec<String>>>,
}

impl RecordingMlsProvider {
    fn new() -> Self {
        Self {
            created: Arc::new(Mutex::new(Vec::new())),
            added: Arc::new(Mutex::new(Vec::new())),
            removed: Arc::new(Mutex::new(Vec::new())),
            updated: Arc::new(Mutex::new(Vec::new())),
            recovered: Arc::new(Mutex::new(Vec::new())),
            finalized: Arc::new(Mutex::new(Vec::new())),
            aborted: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn created(&self) -> Vec<String> {
        self.created.lock().unwrap().clone()
    }

    fn added(&self) -> Vec<String> {
        self.added.lock().unwrap().clone()
    }

    fn removed(&self) -> Vec<String> {
        self.removed.lock().unwrap().clone()
    }

    fn updated(&self) -> Vec<String> {
        self.updated.lock().unwrap().clone()
    }

    fn recovered(&self) -> Vec<String> {
        self.recovered.lock().unwrap().clone()
    }

    fn finalized(&self) -> Vec<String> {
        self.finalized.lock().unwrap().clone()
    }

    fn aborted(&self) -> Vec<String> {
        self.aborted.lock().unwrap().clone()
    }
}

impl GroupMlsProvider for RecordingMlsProvider {
    fn create_group_prepare(
        &self,
        input: CreateGroupInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        self.created.lock().unwrap().push(input.group_did.clone());
        Ok(prepared(
            "pc-create",
            input.operation_id,
            "0",
            "0",
            "active",
        ))
    }

    fn add_member_prepare(
        &self,
        input: AddMemberInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        self.added
            .lock()
            .unwrap()
            .push(format!("{}:{}", input.group_did, input.member_did));
        let mut output = prepared("pc-add", input.operation_id, "1", "2", "active");
        output.subject_did = input.member_did;
        output.member_did = Some(output.subject_did.clone());
        output.commit_b64u = "commit-add".to_owned();
        output.welcome_b64u = Some("welcome-add".to_owned());
        output.ratchet_tree_b64u = Some("ratchet-tree-add".to_owned());
        Ok(output)
    }

    fn remove_member_prepare(
        &self,
        input: RemoveMemberInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        self.removed.lock().unwrap().push(input.member_did.clone());
        let mut output = prepared("pc-remove", input.operation_id, "2", "3", "removed");
        output.subject_did = input.member_did;
        output.commit_b64u = "commit-remove".to_owned();
        Ok(output)
    }

    fn leave_prepare(
        &self,
        input: LeaveGroupInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        let mut output = prepared("pc-leave", input.operation_id, "3", "4", "left");
        output.subject_did = input.actor_did;
        output.commit_b64u = "commit-leave".to_owned();
        Ok(output)
    }

    fn finalize_commit(
        &self,
        input: FinalizeCommitInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::FinalizeCommitOutput> {
        self.finalized
            .lock()
            .unwrap()
            .push(input.pending_commit_id.clone());
        Ok(anp::group_e2ee::operations::FinalizeCommitOutput {
            pending_commit_id: input.pending_commit_id,
            operation_id: "op-finalized".to_owned(),
            group_did: "did:example:groups:e2ee".to_owned(),
            crypto_group_id_b64u: "crypto".to_owned(),
            status: "finalized".to_owned(),
            from_epoch: "0".to_owned(),
            epoch: "2".to_owned(),
            local_epoch: "2".to_owned(),
            subject_did: "did:example:bob".to_owned(),
            subject_status: "active".to_owned(),
            epoch_authenticator: Some("auth".to_owned()),
        })
    }

    fn abort_commit(&self, input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        self.aborted
            .lock()
            .unwrap()
            .push(input.pending_commit_id.clone());
        Ok(AbortCommitOutput {
            pending_commit_id: input.pending_commit_id,
            operation_id: "op-abort".to_owned(),
            group_did: "did:example:groups:e2ee".to_owned(),
            crypto_group_id_b64u: "crypto".to_owned(),
            status: "aborted".to_owned(),
            local_epoch: "2".to_owned(),
            subject_did: "did:example:bob".to_owned(),
            subject_status: "removed".to_owned(),
        })
    }

    fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
        Err(crate::ImError::LocalStateUnavailable {
            detail: "test has no local MLS state".to_owned(),
        })
    }

    fn generate_key_package(
        &self,
        _input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        unreachable!("lifecycle should lease member key packages from service")
    }

    fn update_member_prepare(
        &self,
        input: UpdateMemberInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        self.updated.lock().unwrap().push(format!(
            "{}:{}:{}",
            input.group_did, input.member_did, input.target_device_id
        ));
        let mut output = prepared("pc-update", input.operation_id, "4", "5", "active");
        output.subject_did = input.member_did;
        output.commit_b64u = "commit-update".to_owned();
        output.welcome_b64u = Some("welcome-update".to_owned());
        output.ratchet_tree_b64u = Some("ratchet-tree-update".to_owned());
        Ok(output)
    }

    fn recover_member_prepare(
        &self,
        input: RecoverMemberInput,
    ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
        self.recovered.lock().unwrap().push(format!(
            "{}:{}:{}",
            input.group_did, input.member_did, input.target_device_id
        ));
        let mut output = prepared("pc-recover", input.operation_id, "5", "6", "active");
        output.subject_did = input.member_did;
        output.commit_b64u = "commit-recover".to_owned();
        output.welcome_b64u = Some("welcome-recover".to_owned());
        output.ratchet_tree_b64u = Some("ratchet-tree-recover".to_owned());
        Ok(output)
    }

    fn process_welcome(
        &self,
        _input: ProcessWelcomeInput,
    ) -> crate::ImResult<ProcessWelcomeOutput> {
        unreachable!("lifecycle should not process welcomes")
    }

    fn process_notice(&self, _input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        unreachable!("lifecycle should not process notices")
    }

    fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        unreachable!("lifecycle should not encrypt")
    }

    fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        unreachable!("lifecycle should not decrypt")
    }
}

fn prepared(
    pending_commit_id: &str,
    operation_id: String,
    from_epoch: &str,
    epoch: &str,
    subject_status: &str,
) -> anp::group_e2ee::operations::PreparedMlsCommitOutput {
    anp::group_e2ee::operations::PreparedMlsCommitOutput {
        pending_commit_id: pending_commit_id.to_owned(),
        operation_id,
        status: "pending".to_owned(),
        actor_did: "did:example:alice".to_owned(),
        subject_did: "did:example:alice".to_owned(),
        subject_status: subject_status.to_owned(),
        group_did: "did:example:groups:e2ee".to_owned(),
        crypto_group_id_b64u: "crypto".to_owned(),
        from_epoch: from_epoch.to_owned(),
        epoch: epoch.to_owned(),
        to_epoch: epoch.to_owned(),
        local_epoch: from_epoch.to_owned(),
        commit_b64u: "commit".to_owned(),
        welcome_b64u: None,
        ratchet_tree_b64u: None,
        group_info_b64u: Some("group-info".to_owned()),
        epoch_authenticator: Some("auth".to_owned()),
        epoch_authenticator_b64u: None,
        suite: "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519".to_owned(),
        member_did: None,
    }
}

fn key_package_json(owner: &str) -> Value {
    key_package_json_with_purpose(owner, "normal")
}

fn key_package_json_with_purpose(owner: &str, purpose: &str) -> Value {
    json!({
        "key_package_id": format!("kp-{owner}"),
        "owner_did": owner,
        "device_id": "default",
        "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
        "mls_key_package_b64u": "a2V5LXBhY2thZ2U",
        "did_wba_binding": {"did": owner},
        "expires_at": "2026-05-25T00:00:00Z",
        "purpose": purpose,
        "group_did": "did:example:groups:e2ee",
        "non_cryptographic": true
    })
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        fs::create_dir_all(identities.join("alice")).unwrap();
        let local = root.join("local");
        fs::create_dir_all(&local).unwrap();
        let connection = rusqlite::Connection::open(local.join("im.sqlite")).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        Self { root }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                mail_service_endpoint: None,
                message_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    fn credentials(&self) -> GroupTextCredentials {
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_owned()],
                domain: Some("awiki.test".to_owned()),
                challenge: Some("group-e2ee-lifecycle-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_owned();
        GroupTextCredentials {
            identity_name: "alice".to_owned(),
            did_document: Some(bundle.did_document),
            key1_private_pem,
        }
    }
}

fn stored_group_metadata(
    fixture: &Fixture,
    client: &crate::core::ImClient,
    group_did: &str,
) -> Value {
    let connection =
        rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite")).unwrap();
    let raw: String = connection
        .query_row(
            "SELECT metadata FROM groups WHERE owner_did = ?1 AND group_did = ?2",
            rusqlite::params![client.did().as_str(), group_did],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-group-e2ee-lifecycle-{}-{nanos}",
        std::process::id()
    ))
}
