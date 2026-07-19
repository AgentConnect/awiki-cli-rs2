use super::*;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

#[test]
fn approval_handle_is_single_active_intent_and_redacts_handle_state() {
    let store = DeviceJoinApprovalHandleStore::default();
    let state = DeviceJoinApprovalHandleState {
        admin_identity: crate::identity::IdentitySelector::Default,
        join_session_id: "join-approval-handle".to_owned(),
        operation_id: "op-approval-handle".to_owned(),
        role: DeviceAuthorizationRole::Member,
        expires_at: "2026-07-19T12:00:00Z".to_owned(),
        user_presence_at: None,
    };
    let first = store.issue(state.clone()).unwrap();
    let second = store.issue(state).unwrap();

    assert_ne!(first, second);
    assert!(matches!(
        store.claim(
            &first,
            "2026-07-19T11:00:00Z",
            test_time("2026-07-19T11:00:00Z")
        ),
        Err(crate::ImError::PermissionDenied)
    ));
    let DeviceJoinApprovalHandleClaim::Claimed(bound) = store
        .claim(
            &second,
            "2026-07-19T11:00:00Z",
            test_time("2026-07-19T11:00:00Z"),
        )
        .unwrap()
    else {
        panic!("fresh approval handle must be claimable")
    };
    assert_eq!(
        bound.user_presence_at.as_deref(),
        Some("2026-07-19T11:00:00Z")
    );
    assert!(matches!(
        store.claim(
            &second,
            "2026-07-19T11:01:00Z",
            test_time("2026-07-19T11:01:00Z"),
        ),
        Err(crate::ImError::PermissionDenied)
    ));
    assert!(matches!(
        store.issue(bound.clone()),
        Err(crate::ImError::PermissionDenied)
    ));
    assert!(matches!(
        store.cancel_ready(&second),
        Err(crate::ImError::PermissionDenied)
    ));
    store.release(&second).unwrap();
    let DeviceJoinApprovalHandleClaim::Claimed(retried) = store
        .claim(
            &second,
            "2026-07-19T11:02:00Z",
            test_time("2026-07-19T11:02:00Z"),
        )
        .unwrap()
    else {
        panic!("released approval handle must remain claimable before expiry")
    };
    assert_eq!(
        retried.user_presence_at.as_deref(),
        Some("2026-07-19T11:00:00Z")
    );
    store.consume(&second).unwrap();
    assert!(matches!(
        store.claim(
            &second,
            "2026-07-19T11:03:00Z",
            test_time("2026-07-19T11:03:00Z"),
        ),
        Err(crate::ImError::PermissionDenied)
    ));
    let debug = format!("{store:?}");
    assert!(!debug.contains(&second));
}

#[test]
fn approval_handle_claim_is_atomic_across_concurrent_callers() {
    let store = std::sync::Arc::new(DeviceJoinApprovalHandleStore::default());
    let handle = store
        .issue(DeviceJoinApprovalHandleState {
            admin_identity: crate::identity::IdentitySelector::Default,
            join_session_id: "join-concurrent-handle".to_owned(),
            operation_id: "op-concurrent-handle".to_owned(),
            role: DeviceAuthorizationRole::Member,
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
            user_presence_at: None,
        })
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let callers: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let handle = handle.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.claim(
                    &handle,
                    "2029-01-01T00:00:00Z",
                    test_time("2029-01-01T00:00:00Z"),
                )
            })
        })
        .collect();
    let results: Vec<_> = callers
        .into_iter()
        .map(|caller| caller.join().unwrap())
        .collect();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(crate::ImError::PermissionDenied)))
            .count(),
        7
    );
}

#[test]
fn expired_approval_handle_is_consumed_on_claim() {
    let store = DeviceJoinApprovalHandleStore::default();
    let state = DeviceJoinApprovalHandleState {
        admin_identity: crate::identity::IdentitySelector::Default,
        join_session_id: "join-expired-handle".to_owned(),
        operation_id: "op-expired-handle".to_owned(),
        role: DeviceAuthorizationRole::Member,
        expires_at: "2026-07-19T11:00:00Z".to_owned(),
        user_presence_at: None,
    };
    let before_boundary = store.issue(state.clone()).unwrap();
    assert!(matches!(
        store
            .claim(
                &before_boundary,
                "2026-07-19T10:59:59Z",
                test_time("2026-07-19T10:59:59Z"),
            )
            .unwrap(),
        DeviceJoinApprovalHandleClaim::Claimed(_)
    ));
    store.consume(&before_boundary).unwrap();

    let handle = store.issue(state).unwrap();

    assert!(matches!(
        store
            .claim(
                &handle,
                "2026-07-19T11:00:00Z",
                test_time("2026-07-19T11:00:00Z"),
            )
            .unwrap(),
        DeviceJoinApprovalHandleClaim::Expired(state)
            if state.operation_id == "op-expired-handle"
    ));
    assert!(matches!(
        store.claim(
            &handle,
            "2026-07-19T10:00:00Z",
            test_time("2026-07-19T10:00:00Z"),
        ),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn retryable_approval_lease_is_extended_to_the_exact_proof_expiry() {
    let store = DeviceJoinApprovalHandleStore::default();
    let handle = store
        .issue(DeviceJoinApprovalHandleState {
            admin_identity: crate::identity::IdentitySelector::Default,
            join_session_id: "join-proof-lease".to_owned(),
            operation_id: "op-proof-lease".to_owned(),
            role: DeviceAuthorizationRole::Member,
            expires_at: "2026-07-19T11:05:00Z".to_owned(),
            user_presence_at: None,
        })
        .unwrap();
    assert!(matches!(
        store
            .claim(
                &handle,
                "2026-07-19T11:04:59Z",
                test_time("2026-07-19T11:04:59Z"),
            )
            .unwrap(),
        DeviceJoinApprovalHandleClaim::Claimed(_)
    ));
    store
        .release_with_expiry(&handle, Some("2026-07-19T11:09:59Z"))
        .unwrap();

    let DeviceJoinApprovalHandleClaim::Claimed(retried) = store
        .claim(
            &handle,
            "2026-07-19T11:06:00Z",
            test_time("2026-07-19T11:06:00Z"),
        )
        .unwrap()
    else {
        panic!("the retry lease must follow the persisted proof expiry")
    };
    assert_eq!(retried.expires_at, "2026-07-19T11:09:59Z");
    assert_eq!(
        retried.user_presence_at.as_deref(),
        Some("2026-07-19T11:04:59Z")
    );
}

#[test]
fn new_device_remote_errors_cannot_echo_join_grants_or_tokens() {
    let secret = "join-token-must-never-appear";
    let redacted = redact_new_device_remote_error(crate::ImError::Service {
        status_code: Some(400),
        code: Some("bad_join".to_owned()),
        message: format!("bad token {secret}"),
        data: Some(serde_json::json!({"token": secret})),
    });
    let rendered = format!("{redacted:?} {redacted}");
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("\"token\""));
}

fn test_time(value: &str) -> time::OffsetDateTime {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
}

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.test".to_owned(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: crate::MessageTransportPolicy::HttpOnly,
    }
}

fn test_paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn open_vault_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([53_u8; 32]),
                root.join("vault"),
                "join-runtime-test-workspace",
                "join-runtime-test-vault-device",
            ),
        ),
    )
    .unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedCall {
    Create {
        operation_id: String,
        join_session_id: String,
        account_verification_token: &'static str,
    },
    StatusNew {
        join_session_token: &'static str,
    },
    Response {
        operation_id: String,
        join_session_id: String,
        join_session_token: &'static str,
    },
    TokenIssue {
        operation_id: String,
        device_id: String,
    },
}

enum QueuedResult {
    Create(DeviceJoinRemoteCreateResult),
    StatusNew(DeviceJoinRemoteNewDeviceStatus),
    Transition(DeviceJoinRemoteTransitionResult),
    TokenIssue(crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult),
}

#[derive(Default)]
struct RecordingRemote {
    calls: Vec<RecordedCall>,
    results: VecDeque<QueuedResult>,
}

impl RecordingRemote {
    fn next(&mut self) -> QueuedResult {
        self.results.pop_front().expect("queued remote result")
    }
}

impl DeviceJoinNewDeviceRemote for RecordingRemote {
    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
        self.calls.push(RecordedCall::Create {
            operation_id: request.operation_id.to_owned(),
            join_session_id: request.join_request.join_session_id.clone(),
            account_verification_token: "<redacted>",
        });
        let QueuedResult::Create(result) = self.next() else {
            panic!("expected create result")
        };
        Ok(result)
    }

    async fn status(
        &mut self,
        _expected_join_session_id: &str,
        _join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
        self.calls.push(RecordedCall::StatusNew {
            join_session_token: "<redacted>",
        });
        let QueuedResult::StatusNew(result) = self.next() else {
            panic!("expected new-device status result")
        };
        Ok(result)
    }

    async fn submit_response(
        &mut self,
        request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        self.calls.push(RecordedCall::Response {
            operation_id: request.response.operation_id.clone(),
            join_session_id: request.response.join_session_id.clone(),
            join_session_token: "<redacted>",
        });
        let QueuedResult::Transition(result) = self.next() else {
            panic!("expected transition result")
        };
        Ok(result)
    }

    async fn issue_device_token(
        &mut self,
        prepared: &crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
        _expected_auth_generation: u64,
    ) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult>
    {
        self.calls.push(RecordedCall::TokenIssue {
            operation_id: prepared.operation_id.clone(),
            device_id: prepared.device_id.clone(),
        });
        let QueuedResult::TokenIssue(result) = self.next() else {
            panic!("expected device token result")
        };
        Ok(result)
    }
}

struct StatusOnlyAdminRemote {
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl DeviceJoinAdminRemote for StatusOnlyAdminRemote {
    async fn registry(
        &mut self,
        _did: &crate::ids::Did,
        _include_pending_join_requests: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        self.calls.borrow_mut().push("registry");
        panic!("expired approval reconciliation must not read the Registry")
    }

    async fn status(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinRemoteAdminStatus> {
        self.calls.borrow_mut().push("status");
        Ok(DeviceJoinRemoteAdminStatus {
            join_session_id: join_session_id.to_owned(),
            state: DeviceJoinRemoteState::ResponseVerified,
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
            challenge: None,
            challenge_response: None,
            authorization: None,
        })
    }

    async fn claim(
        &mut self,
        _request: DeviceJoinRemoteClaimRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteClaimResult> {
        self.calls.borrow_mut().push("claim");
        panic!("expired approval reconciliation must not claim")
    }

    async fn submit_challenge(
        &mut self,
        _challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
        self.calls.borrow_mut().push("challenge");
        panic!("expired approval reconciliation must not submit a challenge")
    }

    async fn approve(
        &mut self,
        _request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult> {
        self.calls.borrow_mut().push("approve");
        panic!("expired approval reconciliation must not approve")
    }
}

#[tokio::test]
async fn expired_approval_reconciliation_only_reads_remote_status() {
    let root = tempfile::tempdir().unwrap();
    let core = open_vault_core(root.path());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let remote = StatusOnlyAdminRemote {
        calls: calls.clone(),
    };
    let mut runtime = DeviceJoinAdminRuntime::new(
        &core,
        crate::identity::IdentitySelector::Default,
        remote,
        DeviceJoinRuntimeGate::from_rollout_flag(true),
    );

    let error = runtime
        .reconcile_expired_approval("join-expired-reconcile")
        .await
        .unwrap_err();

    assert!(matches!(error, crate::ImError::IdentityNotFound { .. }));
    assert_eq!(*calls.borrow(), vec!["status"]);
}

#[tokio::test]
async fn production_join_gate_defaults_disabled_before_local_or_remote_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let core = open_vault_core(root.path());
    let mut runtime = DeviceJoinNewDeviceRuntime::production(&core);
    let error = runtime
        .begin(
            crate::identity::DeviceJoinStartRequest {
                operation_id: "disabled-start".to_owned(),
                did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
                ttl_seconds: 300,
            },
            &SecretBytes::from_vec(b"account-token".to_vec()),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::UnsupportedCapability { capability }
            if capability == "awiki-multi-device-join-disabled"
    ));
    assert!(!root.path().join("identities").join(".device-join").exists());
}

#[test]
fn advance_result_debug_redacts_sas() {
    let result = DeviceJoinAdvanceResult {
        session: crate::identity::DeviceJoinSessionSummary {
            join_session_id: "join-1".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-new").unwrap(),
            side: crate::identity::DeviceJoinSide::NewDevice,
            phase: crate::identity::DeviceJoinLocalPhase::ResponsePrepared,
            join_request_hash: "sha256:join".to_owned(),
            challenge_id: Some("challenge-1".to_owned()),
            expires_at: "2026-07-19T00:10:00Z".to_owned(),
        },
        remote_state: DeviceJoinRemoteState::ResponseVerified,
        authorization: None,
        sas: Some("482917".to_owned()),
    };

    let debug = format!("{result:?}");
    assert!(debug.contains("<redacted-sas>"));
    assert!(!debug.contains("482917"));
}

#[tokio::test]
async fn recording_remote_never_retains_account_or_join_session_tokens() {
    let account_token = SecretBytes::from_vec(b"account-token-must-not-be-recorded".to_vec());
    let join_token = SecretBytes::from_vec(b"join-token-must-not-be-recorded".to_vec());
    let join_request = sample_join_request();
    let mut remote = RecordingRemote {
        calls: Vec::new(),
        results: VecDeque::from([
            QueuedResult::Create(DeviceJoinRemoteCreateResult {
                join_session_id: "join-1".to_owned(),
                join_session_token: SecretBytes::from_vec(b"server-session-secret".to_vec()),
                state: DeviceJoinRemoteState::Pending,
                expires_at: "2026-07-19T00:10:00Z".to_owned(),
            }),
            QueuedResult::StatusNew(DeviceJoinRemoteNewDeviceStatus {
                join_session_id: "join-1".to_owned(),
                state: DeviceJoinRemoteState::Pending,
                expires_at: "2026-07-19T00:10:00Z".to_owned(),
                challenge: None,
                authorization: None,
            }),
            QueuedResult::Transition(DeviceJoinRemoteTransitionResult {
                join_session_id: "join-1".to_owned(),
                state: DeviceJoinRemoteState::ResponseVerified,
            }),
        ]),
    };

    let create_request = DeviceJoinRemoteCreateRequest {
        operation_id: "op-create-1",
        account_verification_token: &account_token,
        join_request: &join_request,
    };
    let request_debug = format!("{create_request:?}");
    assert!(!request_debug.contains("account-token-must-not-be-recorded"));
    let created = remote.create(create_request).await.unwrap();
    let result_debug = format!("{created:?}");
    assert!(!result_debug.contains("server-session-secret"));
    remote.status("join-1", &join_token).await.unwrap();
    remote
        .submit_response(DeviceJoinRemoteResponseRequest {
            join_session_token: &join_token,
            response: &sample_response(),
        })
        .await
        .unwrap();

    let recorded = format!("{:?}", remote.calls);
    assert!(!recorded.contains("account-token-must-not-be-recorded"));
    assert!(!recorded.contains("join-token-must-not-be-recorded"));
    assert!(!recorded.contains("server-session-secret"));
    assert_eq!(
        remote.calls,
        vec![
            RecordedCall::Create {
                operation_id: "op-create-1".to_owned(),
                join_session_id: "join-1".to_owned(),
                account_verification_token: "<redacted>",
            },
            RecordedCall::StatusNew {
                join_session_token: "<redacted>",
            },
            RecordedCall::Response {
                operation_id: "op-response-1".to_owned(),
                join_session_id: "join-1".to_owned(),
                join_session_token: "<redacted>",
            },
        ]
    );
}

#[test]
fn new_device_and_admin_status_have_separate_response_types() {
    let new_status = DeviceJoinRemoteNewDeviceStatus {
        join_session_id: "join-1".to_owned(),
        state: DeviceJoinRemoteState::Pending,
        expires_at: "2026-07-19T00:10:00Z".to_owned(),
        challenge: None,
        authorization: None,
    };
    let admin_status = DeviceJoinRemoteAdminStatus {
        join_session_id: "join-1".to_owned(),
        state: DeviceJoinRemoteState::ResponseVerified,
        expires_at: "2026-07-19T00:10:00Z".to_owned(),
        challenge: Some(sample_challenge()),
        challenge_response: Some(sample_response()),
        authorization: None,
    };

    assert_eq!(new_status.state, DeviceJoinRemoteState::Pending);
    assert!(admin_status.challenge_response.is_some());
}

fn sample_join_request() -> DeviceJoinRequest {
    DeviceJoinRequest {
        request_type: crate::identity::DEVICE_JOIN_REQUEST_TYPE.to_owned(),
        did: "did:wba:awiki.test:alice".to_owned(),
        join_session_id: "join-1".to_owned(),
        device_id: "dev-new".to_owned(),
        signing_public_key: serde_json::json!({}),
        e2ee_public_key: serde_json::json!({}),
        pairing_public_key: "pairing".to_owned(),
        profiles: Vec::new(),
        requested_role: "member".to_owned(),
        issued_at: "2026-07-19T00:00:00Z".to_owned(),
        expires_at: "2026-07-19T00:10:00Z".to_owned(),
        signature: "signature".to_owned(),
    }
}

fn sample_response() -> DeviceJoinChallengeResponse {
    DeviceJoinChallengeResponse {
        operation_id: "op-response-1".to_owned(),
        join_session_id: "join-1".to_owned(),
        challenge_id: "challenge-1".to_owned(),
        challenge_hash: "sha256:challenge".to_owned(),
        join_request_hash: "sha256:join".to_owned(),
        pairing_transcript_hash: "sha256:transcript".to_owned(),
        new_device_proof: DeviceProof {
            proof_type: crate::identity::DEVICE_PROOF_TYPE.to_owned(),
            key_id: "did:wba:awiki.test:alice#dev-new-sign".to_owned(),
            created_at: "2026-07-19T00:00:00Z".to_owned(),
            expires_at: "2026-07-19T00:05:00Z".to_owned(),
            nonce: "nonce".to_owned(),
            signature: "signature".to_owned(),
        },
    }
}

fn sample_challenge() -> DeviceJoinChallenge {
    DeviceJoinChallenge {
        operation_id: "op-challenge-1".to_owned(),
        join_session_id: "join-1".to_owned(),
        challenge_id: "challenge-1".to_owned(),
        admin_device_id: "dev-admin".to_owned(),
        admin_pairing_public_key: "admin-pairing".to_owned(),
        ciphertext: crate::identity::EncryptedJoinChallenge {
            algorithm: crate::identity::DEVICE_JOIN_CHALLENGE_ALGORITHM.to_owned(),
            nonce_b64u: "nonce".to_owned(),
            ciphertext_b64u: "ciphertext".to_owned(),
        },
        challenge_expires_at: "2026-07-19T00:05:00Z".to_owned(),
        authorizing_device_proof: DeviceProof {
            proof_type: crate::identity::DEVICE_PROOF_TYPE.to_owned(),
            key_id: "did:wba:awiki.test:alice#dev-admin-sign".to_owned(),
            created_at: "2026-07-19T00:00:00Z".to_owned(),
            expires_at: "2026-07-19T00:05:00Z".to_owned(),
            nonce: "nonce".to_owned(),
            signature: "signature".to_owned(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedWireCall {
    authenticated: bool,
    endpoint: String,
    method: String,
    param_keys: Vec<String>,
}

#[derive(Clone)]
struct QueuedWireTransport {
    calls: Rc<RefCell<Vec<RecordedWireCall>>>,
    responses: Rc<RefCell<VecDeque<crate::ImResult<Value>>>>,
}

impl QueuedWireTransport {
    fn new(responses: Vec<Value>) -> Self {
        Self {
            calls: Rc::new(RefCell::new(Vec::new())),
            responses: Rc::new(RefCell::new(
                responses.into_iter().map(Ok).collect::<VecDeque<_>>(),
            )),
        }
    }

    fn record(&self, authenticated: bool, endpoint: &str, method: &str, params: &Value) {
        let mut param_keys = params
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        param_keys.sort();
        self.calls.borrow_mut().push(RecordedWireCall {
            authenticated,
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            param_keys,
        });
    }

    fn respond(&self) -> crate::ImResult<Value> {
        self.responses
            .borrow_mut()
            .pop_front()
            .expect("queued wire response")
    }
}

impl AsyncRpcTransport for QueuedWireTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.record(false, endpoint, method, &params);
        self.respond()
    }
}

impl AsyncAuthenticatedRpcTransport for QueuedWireTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.record(true, endpoint, method, &params);
        self.respond()
    }
}

#[tokio::test]
async fn http_adapter_routes_only_frozen_join_methods_to_the_correct_auth_view() {
    const DOCUMENT_HASH: &str = "sha256:UD5TmycQ6gS539AFNjM5cGoQUmeq2fQGPpwD00lMPlg";
    let checkpoint_json = serde_json::json!({
        "document_version": 7,
        "document_hash": DOCUMENT_HASH,
        "registry_version": 3,
    });
    let device_json = serde_json::json!({
        "device_id": "dev-new",
        "signing_key_id": "did:wba:awiki.test:alice#dev-new-sign",
        "e2ee_key_id": "did:wba:awiki.test:alice#dev-new-e2ee",
        "status": "active",
        "role": "member",
        "management_ready": false,
        "auth_generation": 1,
    });
    let join_request = sample_join_request();
    let challenge = sample_challenge();
    let response = sample_response();
    let plain = QueuedWireTransport::new(vec![
        serde_json::json!({
            "join_session_id": "join-1",
            "join_session_token": "join-token-issued",
            "state": "pending",
            "expires_at": "2026-07-19T00:10:00Z",
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "pending",
            "expires_at": "2026-07-19T00:10:00Z",
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "response_verified",
        }),
    ]);
    let authenticated = QueuedWireTransport::new(vec![
        serde_json::json!({
            "did": "did:wba:awiki.test:alice",
            "checkpoint": checkpoint_json.clone(),
            "devices": [],
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "claimed",
            "expires_at": "2026-07-19T00:10:00Z",
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "claimed",
            "claimed_by_device_id": "dev-admin",
            "claim_expires_at": "2026-07-19T00:05:00Z",
            "join_request": join_request.clone(),
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "challenge_sent",
            "challenge_id": "challenge-1",
        }),
        serde_json::json!({
            "join_session_id": "join-1",
            "state": "consumed",
            "checkpoint": checkpoint_json.clone(),
            "device": device_json,
        }),
    ]);
    let plain_calls = plain.calls.clone();
    let authenticated_calls = authenticated.calls.clone();
    let mut new_device_adapter = DeviceJoinNewDeviceHttpAdapter::new(plain);
    let mut admin_adapter = DeviceJoinAdminHttpAdapter::new(authenticated);
    let did = crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap();
    let account_token = SecretBytes::from_vec(b"account-token".to_vec());
    let join_token = SecretBytes::from_vec(b"join-token-issued".to_vec());
    let admin_proof = challenge.authorizing_device_proof.clone();
    let checkpoint = IdentityInternalCheckpoint {
        document_version: 7,
        document_hash: DOCUMENT_HASH.to_owned(),
        registry_version: 3,
    };
    let pairing_confirmation = DeviceJoinRemotePairingConfirmation {
        join_request_hash: "sha256:join".to_owned(),
        pairing_transcript_hash: "sha256:transcript".to_owned(),
        sas_confirmed: true,
        user_presence_at: "2026-07-19T00:04:00Z".to_owned(),
    };
    let new_document = serde_json::json!({"id": did.as_str()});

    admin_adapter.registry(&did, false).await.unwrap();
    new_device_adapter
        .create(DeviceJoinRemoteCreateRequest {
            operation_id: "op-create-1",
            account_verification_token: &account_token,
            join_request: &join_request,
        })
        .await
        .unwrap();
    new_device_adapter
        .status("join-1", &join_token)
        .await
        .unwrap();
    admin_adapter.status("join-1").await.unwrap();
    admin_adapter
        .claim(DeviceJoinRemoteClaimRequest {
            operation_id: "op-claim-1",
            join_session_id: "join-1",
            authorizing_device_id: "dev-admin",
            authorizing_device_proof: &admin_proof,
        })
        .await
        .unwrap();
    admin_adapter.submit_challenge(&challenge).await.unwrap();
    new_device_adapter
        .submit_response(DeviceJoinRemoteResponseRequest {
            join_session_token: &join_token,
            response: &response,
        })
        .await
        .unwrap();
    admin_adapter
        .approve(DeviceJoinRemoteApproveRequest {
            operation_id: "op-approve-1",
            join_session_id: "join-1",
            expected_checkpoint: &checkpoint,
            role: DeviceAuthorizationRole::Member,
            new_document: &new_document,
            pairing_confirmation: &pairing_confirmation,
            authorizing_device_id: "dev-admin",
            authorizing_device_proof: &admin_proof,
        })
        .await
        .unwrap();

    assert_eq!(
        plain_calls
            .borrow()
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec![
            "device_join_create",
            "device_join_status",
            "device_join_challenge_response",
        ]
    );
    assert!(plain_calls.borrow().iter().all(|call| !call.authenticated));
    assert_eq!(
        authenticated_calls
            .borrow()
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec![
            "device_registry_get",
            "device_join_status",
            "device_join_claim",
            "device_join_challenge",
            "device_join_approve",
        ]
    );
    assert!(authenticated_calls
        .borrow()
        .iter()
        .all(|call| call.authenticated));
    let recorded = format!(
        "{:?}{:?}",
        plain_calls.borrow(),
        authenticated_calls.borrow()
    );
    assert!(!recorded.contains("account-token"));
    assert!(!recorded.contains("join-token-issued"));
}
