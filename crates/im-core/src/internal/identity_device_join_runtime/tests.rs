use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

struct JoinedMarkerAssertingRemote {
    sqlite_path: std::path::PathBuf,
}

impl DeviceJoinNewDeviceRemote for JoinedMarkerAssertingRemote {
    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
        let marker = crate::internal::identity_transition_pending::load_joined_device(
            &self.sqlite_path,
            &request.join_request.join_session_id,
        )?;
        if marker.is_none() {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "joined transition marker was not durable before remote create".to_owned(),
            });
        }
        Ok(DeviceJoinRemoteCreateResult {
            join_session_id: request.join_request.join_session_id.clone(),
            join_session_token: SecretBytes::from_vec(b"join-token".to_vec()),
            state: DeviceJoinRemoteState::Pending,
            session_revision: 1,
            expires_at: request.join_request.expires_at.clone(),
        })
    }

    async fn status(
        &mut self,
        _expected_join_session_id: &str,
        _join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
        Err(crate::ImError::unsupported("joined-marker-test-status"))
    }

    async fn submit_response(
        &mut self,
        _request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        Err(crate::ImError::unsupported("joined-marker-test-response"))
    }

    async fn cancel(
        &mut self,
        _request: DeviceJoinRemoteCancelRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        Err(crate::ImError::unsupported("joined-marker-test-cancel"))
    }

    async fn refresh_device_access(
        &mut self,
        _pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAccessResult> {
        Err(crate::ImError::unsupported("joined-marker-test-refresh"))
    }
}

struct UnusedResolver;

impl crate::internal::transport::AsyncRawJsonTransport for UnusedResolver {
    async fn get_json_url(
        &mut self,
        _url: &str,
        _headers: std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<serde_json::Value> {
        Err(crate::ImError::unsupported("joined-marker-test-resolve"))
    }
}

const JOIN_ACCESS_SECRET_DID: &str = "did:wba:private.example:users:secret";
const JOIN_ACCESS_SECRET_TOKEN: &str = "bearer-secret-token";

struct JoinAccessFailingRemote {
    status_code: u16,
    code: &'static str,
}

impl DeviceJoinNewDeviceRemote for JoinAccessFailingRemote {
    async fn create(
        &mut self,
        _request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
        Err(crate::ImError::unsupported("join-access-test-create"))
    }

    async fn status(
        &mut self,
        _expected_join_session_id: &str,
        _join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
        Err(crate::ImError::unsupported("join-access-test-status"))
    }

    async fn submit_response(
        &mut self,
        _request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        Err(crate::ImError::unsupported("join-access-test-response"))
    }

    async fn cancel(
        &mut self,
        _request: DeviceJoinRemoteCancelRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        Err(crate::ImError::unsupported("join-access-test-cancel"))
    }

    async fn refresh_device_access(
        &mut self,
        _pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAccessResult> {
        Err(crate::ImError::Service {
            status_code: Some(self.status_code),
            code: Some(self.code.to_owned()),
            message: JOIN_ACCESS_SECRET_DID.to_owned(),
            data: Some(json!({
                "did": JOIN_ACCESS_SECRET_DID,
                "token": JOIN_ACCESS_SECRET_TOKEN,
            })),
        })
    }
}

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.test".to_owned(),
        client_version_info: None,
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

fn open_ready_admin_core(root: &Path) -> (crate::ImCore, serde_json::Value, crate::ids::Did) {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };

    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap();
    let document_hash =
        crate::internal::identity_wire::document::document_hash(&generated.did_document).unwrap();
    let paths = test_paths(root);
    let vault = Arc::new(crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.join("vault")),
    ));
    crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .save_identity_with_secret_storage(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: generated.did.clone(),
                unique_id: generated.unique_id.clone(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                binding_generation: None,
                jwt_token: "access-token".to_owned(),
                did_document: Some(generated.did_document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: generated.root_key_id.clone(),
                    device_signing_key_id: generated.device_signing_key_id.clone(),
                    device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id: generated.protocol_device_id.clone(),
                        signing_key_id: generated.device_signing_key_id.clone(),
                        e2ee_key_id: generated.device_e2ee_key_id.clone(),
                        status: DeviceAuthorizationStatus::Active,
                        role: DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(IdentityInternalCheckpoint {
                        document_version: 7,
                        document_hash,
                        registry_version: 3,
                    }),
                }),
                key1_private_pem: generated.root_private_pem,
                key1_public_pem: generated.root_public_pem,
                e2ee_signing_private_pem: generated.device_signing_private_pem,
                e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
                daemon_subkey_package: None,
                make_default: true,
            },
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                workspace_id: "join-runtime-test-workspace".to_owned(),
                device_id: "join-runtime-test-vault-device".to_owned(),
                vault,
            },
        )
        .unwrap();
    let did = generated.did;
    let document = generated.did_document;
    let core = crate::ImCore::new_with_options(
        test_config(),
        paths,
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                root.join("vault"),
                "join-runtime-test-workspace",
                "join-runtime-test-vault-device",
            ),
        ),
    )
    .unwrap();
    (core, document, did)
}

fn open_empty_vault_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([53_u8; 32]),
                root.join("vault"),
                "join-runtime-candidate-workspace",
                "join-runtime-candidate-vault-device",
            ),
        ),
    )
    .unwrap()
}

fn admin_join_state_bytes(root: &Path) -> Vec<u8> {
    let mut paths = std::fs::read_dir(root.join("identities").join(".device-join"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 1);
    std::fs::read(&paths[0]).unwrap()
}

fn join_access_service_error(
    status_code: Option<u16>,
    code: Option<&str>,
    message: &str,
    data: Option<serde_json::Value>,
) -> crate::ImError {
    crate::ImError::Service {
        status_code,
        code: code.map(str::to_owned),
        message: message.to_owned(),
        data,
    }
}

#[test]
fn join_access_error_maps_strict_numeric_rpc_codes_to_public_codes() {
    for (remote_code, expected) in [
        ("-32000", "device.join.access.rpc.n32000"),
        ("1401", "device.join.access.rpc.p1401"),
    ] {
        let error = redact_device_join_access_error(join_access_service_error(
            Some(500),
            Some(remote_code),
            "remote detail",
            Some(json!({"retryable": true})),
        ));

        assert_eq!(
            error,
            crate::ImError::Service {
                status_code: Some(500),
                code: Some(expected.to_owned()),
                message: "device Join access request failed".to_owned(),
                data: None,
            }
        );
    }
}

#[test]
fn join_access_error_preserves_stable_public_service_codes() {
    for stable_code in ["device.join.expired", "anp.device_state_changed"] {
        let error = redact_device_join_access_error(join_access_service_error(
            None,
            Some(stable_code),
            "remote detail",
            Some(json!({"retryable": false})),
        ));

        assert!(matches!(
            error,
            crate::ImError::Service {
                status_code: None,
                code: Some(ref code),
                ref message,
                data: None,
            } if code == stable_code && message == "device Join access request failed"
        ));
    }
}

#[test]
fn join_access_error_collapses_did_auth_codes_to_a_fixed_public_code() {
    let private_marker = "private-upstream-auth-detail";
    for did_auth_code in [
        "did_auth.invalid_signature",
        "did_auth.凭据",
        private_marker,
    ] {
        let remote_code = if did_auth_code == private_marker {
            format!("did_auth.{private_marker}")
        } else {
            did_auth_code.to_owned()
        };
        let error = redact_device_join_access_error(join_access_service_error(
            Some(503),
            Some(&remote_code),
            private_marker,
            Some(json!({"remote_code": remote_code})),
        ));

        assert_eq!(
            error,
            crate::ImError::Service {
                status_code: Some(503),
                code: Some("device.join.access.did_auth".to_owned()),
                message: "device Join access request failed".to_owned(),
                data: None,
            }
        );
        assert!(!format!("{error:?} {error}").contains(private_marker));
    }
}

#[test]
fn join_access_error_uses_safe_http_status_only_without_a_service_code() {
    let error = redact_device_join_access_error(join_access_service_error(
        Some(503),
        None,
        "upstream unavailable",
        None,
    ));

    assert!(matches!(
        error,
        crate::ImError::Service {
            status_code: Some(503),
            code: Some(ref code),
            ref message,
            data: None,
        } if code == "device.join.access.http.503"
            && message == "device Join access request failed"
    ));

    let invalid_status = redact_device_join_access_error(join_access_service_error(
        Some(99),
        None,
        "not an HTTP status",
        None,
    ));
    assert!(matches!(
        invalid_status,
        crate::ImError::Service {
            status_code: Some(99),
            code: None,
            data: None,
            ..
        }
    ));
}

#[test]
fn join_access_error_collapses_noncanonical_or_untrusted_codes() {
    let overlong = format!("device.{}", "a".repeat(97));
    for untrusted_code in [
        "+32000",
        "032000",
        "-0",
        "0",
        "9223372036854775808",
        "device.join.INVALID",
        "device.join.凭据",
        "untrusted.secret",
        overlong.as_str(),
    ] {
        let error = redact_device_join_access_error(join_access_service_error(
            None,
            Some(untrusted_code),
            "remote detail",
            None,
        ));

        assert!(matches!(
            error,
            crate::ImError::Service {
                status_code: None,
                code: Some(ref code),
                ref message,
                data: None,
            } if code == "device.join.access.rpc.unclassified"
                && message == "device Join access request failed"
        ));
    }

    let no_status_fallback = redact_device_join_access_error(join_access_service_error(
        Some(503),
        Some("untrusted.secret"),
        "remote detail",
        None,
    ));
    assert!(matches!(
        no_status_fallback,
        crate::ImError::Service {
            status_code: Some(503),
            code: Some(ref code),
            data: None,
            ..
        } if code == "device.join.access.rpc.unclassified"
    ));
}

#[test]
fn join_access_error_never_exposes_remote_message_or_data() {
    let secret_did = "did:wba:private.example:users:secret";
    let secret_token = "bearer-secret-token";
    let error = redact_device_join_access_error(join_access_service_error(
        Some(500),
        Some("-32000"),
        secret_did,
        Some(json!({"token": secret_token, "did": secret_did})),
    ));

    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(secret_did));
    assert!(!rendered.contains(secret_token));
    assert!(matches!(
        error,
        crate::ImError::Service {
            message,
            data: None,
            ..
        } if message == "device Join access request failed"
    ));
}

#[tokio::test]
async fn advance_redacts_join_access_error_at_remote_trait_boundary() {
    let admin_directory = tempfile::tempdir().unwrap();
    let candidate_directory = tempfile::tempdir().unwrap();
    let (admin, current_document, did) = open_ready_admin_core(admin_directory.path());
    let candidate = open_empty_vault_core(candidate_directory.path());
    let current_document_hash =
        crate::internal::identity_wire::document::document_hash(&current_document).unwrap();
    let started = candidate
        .device_join()
        .start(crate::identity::DeviceJoinStartRequest {
            operation_id: "join-access-runtime-start".to_owned(),
            did,
            ttl_seconds: 300,
        })
        .unwrap();
    let join_session_id = started.session.join_session_id.clone();
    let join_request = started.join_request;
    let challenge = admin
        .device_join()
        .prepare_admin_challenge(crate::identity::DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "join-access-runtime-challenge".to_owned(),
            join_request: join_request.clone(),
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: current_document_hash.clone(),
        })
        .unwrap();
    let response = candidate
        .device_join()
        .respond_as_new_device(crate::identity::DeviceJoinNewDeviceRespondRequest {
            operation_id: "join-access-runtime-response".to_owned(),
            challenge: challenge.challenge,
            admin_did_document: current_document,
            document_version: 7,
            document_hash: current_document_hash.clone(),
        })
        .unwrap();
    admin
        .device_join()
        .verify_response_as_admin(crate::identity::DeviceJoinAdminVerifyRequest {
            operation_id: "join-access-runtime-verify".to_owned(),
            join_session_id: join_session_id.clone(),
            response: response.response,
        })
        .unwrap();
    let current_checkpoint = IdentityInternalCheckpoint {
        document_version: 7,
        document_hash: current_document_hash,
        registry_version: 3,
    };
    let approval = crate::internal::identity_device_join::prepare_admin_approval(
        &admin,
        "join-access-runtime-approve",
        &join_session_id,
        &current_checkpoint,
        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        true,
    )
    .unwrap();
    let authorized_document_hash =
        crate::internal::identity_wire::document::document_hash(&approval.new_document).unwrap();
    let authorization = DeviceJoinRemoteAuthorization {
        checkpoint: IdentityInternalCheckpoint {
            document_version: 8,
            document_hash: authorized_document_hash,
            registry_version: 4,
        },
        device: DeviceJoinRemoteDeviceSummary {
            device_id: join_request.device_id,
            signing_key_id: join_request
                .signing_public_key
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .to_owned(),
            e2ee_key_id: join_request
                .e2ee_public_key
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .to_owned(),
            status: DeviceAuthorizationStatus::Active,
            role: DeviceAuthorizationRole::Member,
            management_ready: false,
            auth_generation: 1,
        },
    };
    crate::internal::identity_device_join::prepare_new_device_activation(
        &candidate,
        &join_session_id,
        &authorization,
        &approval.new_document,
    )
    .unwrap();

    let mut runtime = DeviceJoinNewDeviceRuntime::new(
        &candidate,
        JoinAccessFailingRemote {
            status_code: 500,
            code: "-32000",
        },
        DeviceJoinDidResolver::new(UnusedResolver),
    );
    let error = runtime.advance(&join_session_id).await.unwrap_err();

    assert_eq!(
        error,
        crate::ImError::Service {
            status_code: Some(500),
            code: Some("device.join.access.rpc.n32000".to_owned()),
            message: "device Join access request failed".to_owned(),
            data: None,
        }
    );
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(JOIN_ACCESS_SECRET_DID));
    assert!(!rendered.contains(JOIN_ACCESS_SECRET_TOKEN));
    let session = candidate
        .device_join()
        .session(&join_session_id, crate::identity::DeviceJoinSide::NewDevice)
        .unwrap();
    assert_eq!(
        session.phase,
        crate::identity::DeviceJoinLocalPhase::ResponsePrepared
    );
    let pending = crate::internal::identity_device_join::load_pending_new_device_activation(
        &candidate,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert!(pending.access_result.is_none());

    let mut malicious_runtime = DeviceJoinNewDeviceRuntime::new(
        &candidate,
        JoinAccessFailingRemote {
            status_code: 503,
            code: "untrusted.secret",
        },
        DeviceJoinDidResolver::new(UnusedResolver),
    );
    let malicious_error = malicious_runtime
        .advance(&join_session_id)
        .await
        .unwrap_err();
    assert_eq!(
        malicious_error,
        crate::ImError::Service {
            status_code: Some(503),
            code: Some("device.join.access.rpc.unclassified".to_owned()),
            message: "device Join access request failed".to_owned(),
            data: None,
        }
    );
    let rendered = format!("{malicious_error:?} {malicious_error}");
    assert!(!rendered.contains(JOIN_ACCESS_SECRET_DID));
    assert!(!rendered.contains(JOIN_ACCESS_SECRET_TOKEN));
    let session = candidate
        .device_join()
        .session(&join_session_id, crate::identity::DeviceJoinSide::NewDevice)
        .unwrap();
    assert_eq!(
        session.phase,
        crate::identity::DeviceJoinLocalPhase::ResponsePrepared
    );
    let pending = crate::internal::identity_device_join::load_pending_new_device_activation(
        &candidate,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert!(pending.access_result.is_none());
}

#[test]
fn recovery_join_post_activation_policy_never_publishes_p6() {
    assert_eq!(
        post_activation_publish_policy(true),
        PostActivationPublishPolicy::PrekeysOnly
    );
    assert_eq!(
        post_activation_publish_policy(false),
        PostActivationPublishPolicy::PrekeysAndGroupKeyPackage
    );
}

#[test]
fn join_approval_handles_are_single_use() {
    let store = DeviceJoinApprovalHandleStore::default();
    let handle = store
        .issue(DeviceJoinApprovalHandleState {
            admin_identity: crate::identity::IdentitySelector::Id(
                crate::ids::IdentityId::parse("identity-admin").unwrap(),
            ),
            join_session_id: "join-1".to_owned(),
            operation_id: "approve-1".to_owned(),
            user_presence_at: None,
            expires_at: "2099-07-18T12:05:00Z".to_owned(),
        })
        .unwrap();
    let now = time::OffsetDateTime::now_utc();
    assert!(matches!(
        store.claim(&handle, "2026-07-18T12:00:00Z", now),
        Ok(DeviceJoinApprovalHandleClaim::Claimed(_))
    ));
    assert!(store.claim(&handle, "2026-07-18T12:00:00Z", now).is_err());
    assert!(store.consume(&handle).is_ok());
}

#[test]
fn public_join_states_have_no_claimed_compatibility_state() {
    let states = [
        DeviceJoinRemoteState::Pending,
        DeviceJoinRemoteState::ChallengeSent,
        DeviceJoinRemoteState::ResponseVerified,
        DeviceJoinRemoteState::Consumed,
        DeviceJoinRemoteState::Cancelled,
        DeviceJoinRemoteState::Rejected,
        DeviceJoinRemoteState::Expired,
    ];
    assert_eq!(states.len(), 7);
}

#[test]
fn response_notification_only_advances_a_waiting_admin_session() {
    assert!(should_verify_response_from_notification(Some(
        crate::identity::DeviceJoinLocalPhase::ChallengePrepared,
    )));
    for phase in [
        None,
        Some(crate::identity::DeviceJoinLocalPhase::ResponseVerified),
        Some(crate::identity::DeviceJoinLocalPhase::ApprovalPrepared),
        Some(crate::identity::DeviceJoinLocalPhase::Authorized),
    ] {
        assert!(!should_verify_response_from_notification(phase));
    }
}

#[tokio::test]
async fn registration_recovery_join_marker_is_durable_before_remote_join_create() {
    let directory = tempfile::tempdir().unwrap();
    let core = open_empty_vault_core(directory.path());
    let sqlite_path = core.inner().sdk_paths().local_state.sqlite_path.clone();
    let mut runtime = DeviceJoinNewDeviceRuntime::new(
        &core,
        JoinedMarkerAssertingRemote {
            sqlite_path: sqlite_path.clone(),
        },
        DeviceJoinDidResolver::new(UnusedResolver),
    );
    let session = runtime
        .begin_with_local_hook(
            crate::identity::DeviceJoinStartRequest {
                operation_id: "authorized-join-operation-1".to_owned(),
                did: crate::ids::Did::parse("did:wba:awiki.info:users:alice-new").unwrap(),
                ttl_seconds: 300,
            },
            &SecretBytes::from_vec(b"account-verification-token".to_vec()),
            |session| {
                let marker = crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
                    &sqlite_path,
                    &session.join_session_id,
                    "user-1",
                    "owner-1",
                    "alice.awiki.info",
                    "did:wba:awiki.info:users:alice-old",
                    "did:wba:awiki.info:users:alice-new",
                    "8",
                )?;
                crate::internal::identity_transition_pending::persist(&sqlite_path, &marker)
            },
        )
        .await
        .unwrap();

    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &sqlite_path,
            &session.join_session_id,
        )
        .unwrap()
        .is_some()
    );
}

#[tokio::test]
async fn response_verified_notification_replay_is_idempotent_and_side_effect_free() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = crate::internal::identity_wire::document::document_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(crate::identity::DeviceJoinStartRequest {
            operation_id: "start-runtime-replay".to_owned(),
            did,
            ttl_seconds: 300,
        })
        .unwrap();
    let join_session_id = started.session.join_session_id.clone();
    let prepared = core
        .device_join()
        .prepare_admin_challenge(crate::identity::DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "prepare-runtime-replay".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();
    let responded = candidate
        .device_join()
        .respond_as_new_device(crate::identity::DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-runtime-replay".to_owned(),
            challenge: prepared.challenge,
            admin_did_document: document,
            document_version: 7,
            document_hash,
        })
        .unwrap();
    assert_eq!(
        core.device_join()
            .session(&join_session_id, crate::identity::DeviceJoinSide::Admin)
            .unwrap()
            .phase,
        crate::identity::DeviceJoinLocalPhase::ChallengePrepared
    );

    let client = core
        .client(crate::identity::IdentitySelector::Default)
        .unwrap();
    let current_device_id = client.exact_protocol_device_id().unwrap();
    let issued_at = chrono::Utc::now();
    let expires_at = issued_at + chrono::Duration::minutes(5);
    let notification_value = json!({
        "type": "awiki.device.join-response-verified.v1",
        "event_id": "event-runtime-response-replay",
        "did": client.did().as_str(),
        "join_session_id": join_session_id,
        "state": "response_verified",
        "session_revision": 3,
        "issued_at": issued_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "expires_at": expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "payload": {
            "claimed_by_device_id": current_device_id,
            "challenge_response": responded.response,
        },
    });
    let notification = crate::internal::system_notification::wire::parse_verified_notification(
        notification_value.clone(),
    )
    .unwrap();
    let meta = crate::internal::system_notification::wire::DirectMeta {
        anp_version: Some("2.0".to_owned()),
        profile: crate::internal::system_notification::wire::DIRECT_PROFILE.to_owned(),
        security_profile: crate::internal::system_notification::wire::TRANSPORT_SECURITY.to_owned(),
        sender_did: "did:wba:example.test:service".to_owned(),
        target: crate::internal::system_notification::wire::DirectTarget {
            kind: "did".to_owned(),
            did: client.did().as_str().to_owned(),
        },
        operation_id: "operation-runtime-response-replay".to_owned(),
        message_id: "message-runtime-response-replay".to_owned(),
        created_at: issued_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        content_type: crate::internal::system_notification::wire::JSON_CONTENT_TYPE.to_owned(),
    };
    let body = crate::internal::system_notification::wire::DirectBody {
        payload: notification_value,
    };
    let verified = crate::internal::system_notification::verify::VerifiedSystemNotification {
        envelope: crate::internal::system_notification::wire::SystemNotificationEnvelope {
            signed_meta: serde_json::to_value(&meta).unwrap(),
            signed_body: serde_json::to_value(&body).unwrap(),
            meta,
            auth: crate::internal::system_notification::wire::DirectAuth {
                scheme: "rfc9421-origin-proof".to_owned(),
                origin_proof: anp::proof::Rfc9421OriginProof {
                    content_digest: "sha-256=:test:".to_owned(),
                    signature_input: "sig1=()".to_owned(),
                    signature: "sig1=:test:".to_owned(),
                },
            },
            body,
            notification,
        },
        payload_hash: "sha256:runtime-response-replay".to_owned(),
        proof_hash: "sha256:runtime-response-proof".to_owned(),
    };
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .apply_system_notification(
            crate::internal::system_notification::store::SystemNotificationApplyInput {
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                protocol_device_id: client.exact_protocol_device_id().unwrap(),
                verified,
                received_at: issued_at,
            },
        )
        .await
        .unwrap();

    let mut runtime = DeviceJoinAdminRuntime::production(&core, &client);
    let first_notices = runtime.local_device_join_requests().await.unwrap();
    let first_session = core
        .device_join()
        .session(&join_session_id, crate::identity::DeviceJoinSide::Admin)
        .unwrap();
    assert_eq!(
        first_session.phase,
        crate::identity::DeviceJoinLocalPhase::ResponseVerified
    );
    assert_eq!(first_notices.len(), 1);
    assert!(first_notices[0].claimed_by_current_device);
    let state_after_first = admin_join_state_bytes(admin_root.path());

    let second_notices = runtime.local_device_join_requests().await.unwrap();
    let second_session = core
        .device_join()
        .session(&join_session_id, crate::identity::DeviceJoinSide::Admin)
        .unwrap();
    let state_after_second = admin_join_state_bytes(admin_root.path());

    assert_eq!(second_notices, first_notices);
    assert_eq!(second_session, first_session);
    assert_eq!(state_after_second, state_after_first);
}

#[tokio::test]
async fn cancelled_new_device_runtime_does_not_reopen_deleted_remote_token() {
    let root = tempfile::tempdir().unwrap();
    let core = open_empty_vault_core(root.path());
    let started = core
        .device_join()
        .start(crate::identity::DeviceJoinStartRequest {
            operation_id: "join-cancel-idempotent".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 600,
        })
        .unwrap();
    crate::internal::identity_device_join::cancel_join(
        &core,
        &started.session.join_session_id,
        crate::identity::DeviceJoinSide::NewDevice,
    )
    .unwrap();

    let mut runtime = DeviceJoinNewDeviceRuntime::new(
        &core,
        JoinAccessFailingRemote {
            status_code: 500,
            code: "must-not-call-remote",
        },
        DeviceJoinDidResolver::new(UnusedResolver),
    );
    let cancelled = runtime
        .cancel(&started.session.join_session_id)
        .await
        .unwrap();
    assert_eq!(
        cancelled.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
    );
}
