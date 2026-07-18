use super::*;

use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedCall {
    Registry {
        did: String,
        include_pending_join_requests: bool,
    },
    Create {
        operation_id: String,
        join_session_id: String,
        account_verification_token: &'static str,
    },
    StatusNew {
        join_session_token: &'static str,
    },
    StatusAdmin {
        join_session_id: String,
    },
    Claim {
        operation_id: String,
        join_session_id: String,
    },
    Challenge {
        operation_id: String,
        join_session_id: String,
    },
    Response {
        operation_id: String,
        join_session_id: String,
        join_session_token: &'static str,
    },
    Approve {
        operation_id: String,
        join_session_id: String,
    },
}

enum QueuedResult {
    Registry(DeviceJoinRemoteRegistry),
    Create(DeviceJoinRemoteCreateResult),
    StatusNew(DeviceJoinRemoteNewDeviceStatus),
    StatusAdmin(DeviceJoinRemoteAdminStatus),
    Claim(DeviceJoinRemoteClaimResult),
    Challenge(DeviceJoinRemoteChallengeResult),
    Transition(DeviceJoinRemoteTransitionResult),
    Approve(DeviceJoinRemoteApproveResult),
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

impl DeviceJoinRemote for RecordingRemote {
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
        include_pending_join_requests: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        self.calls.push(RecordedCall::Registry {
            did: did.as_str().to_owned(),
            include_pending_join_requests,
        });
        let QueuedResult::Registry(result) = self.next() else {
            panic!("expected registry result")
        };
        Ok(result)
    }

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

    async fn status_as_new_device(
        &mut self,
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

    async fn status_as_admin(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinRemoteAdminStatus> {
        self.calls.push(RecordedCall::StatusAdmin {
            join_session_id: join_session_id.to_owned(),
        });
        let QueuedResult::StatusAdmin(result) = self.next() else {
            panic!("expected admin status result")
        };
        Ok(result)
    }

    async fn claim(
        &mut self,
        request: DeviceJoinRemoteClaimRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteClaimResult> {
        self.calls.push(RecordedCall::Claim {
            operation_id: request.operation_id.to_owned(),
            join_session_id: request.join_session_id.to_owned(),
        });
        let QueuedResult::Claim(result) = self.next() else {
            panic!("expected claim result")
        };
        Ok(result)
    }

    async fn submit_challenge(
        &mut self,
        challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
        self.calls.push(RecordedCall::Challenge {
            operation_id: challenge.operation_id.clone(),
            join_session_id: challenge.join_session_id.clone(),
        });
        let QueuedResult::Challenge(result) = self.next() else {
            panic!("expected challenge result")
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

    async fn approve(
        &mut self,
        request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult> {
        self.calls.push(RecordedCall::Approve {
            operation_id: request.operation_id.to_owned(),
            join_session_id: request.join_session_id.to_owned(),
        });
        let QueuedResult::Approve(result) = self.next() else {
            panic!("expected approve result")
        };
        Ok(result)
    }
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
    remote.status_as_new_device(&join_token).await.unwrap();

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
