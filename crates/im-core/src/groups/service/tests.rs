use super::*;
use crate::internal::group_e2ee::provider::GroupMlsProvider;
use crate::internal::transport::AsyncAuthenticatedRpcTransport;
use anp::group_e2ee::operations::{
    AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
    ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
    RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn publish_key_package_async_helper_uses_async_transport() {
    let fixture = Fixture::new();
    let owner_did = fixture.did_bundle.did().unwrap().to_owned();
    let client = fixture.client();
    let credentials = fixture.credentials();
    let provider = RecordingKeyPackageProvider::default();
    let generated = Arc::clone(&provider.generated);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let result = publish_group_key_package_with_components(
        &client,
        crate::groups::GroupKeyPackagePublishRequest {
            purpose: crate::groups::GroupKeyPackagePurpose::Recovery,
            group: Some(crate::ids::GroupRef::parse("did:example:groups:secure").unwrap()),
            device_id: Some(" device-a ".to_owned()),
            key_package_id: Some(" kp-explicit ".to_owned()),
        },
        credentials,
        crate::ids::Did::parse("did:example:service").unwrap(),
        provider,
        RecordingAsyncTransport {
            calls: Arc::clone(&calls),
            response: json!({"accepted": true}),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.owner_did.as_str(), owner_did);
    assert_eq!(result.device_id, "device-a");
    assert_eq!(result.key_package_id, "kp-explicit");
    assert_eq!(
        result.purpose,
        crate::groups::GroupKeyPackagePurpose::Recovery
    );
    assert_eq!(
        result.group.as_ref().map(|group| group.as_str()),
        Some("did:example:groups:secure")
    );
    assert_eq!(result.raw_response, json!({"accepted": true}));

    let generated = generated.lock().unwrap();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].owner_did, owner_did);
    assert_eq!(generated[0].device_id, "device-a");
    assert_eq!(generated[0].key_package_id.as_deref(), Some("kp-explicit"));
    assert_eq!(generated[0].purpose.as_deref(), Some("recovery"));
    assert_eq!(
        generated[0].group_did.as_deref(),
        Some("did:example:groups:secure")
    );

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].endpoint,
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT
    );
    assert_eq!(calls[0].method, "group.e2ee.publish_key_package");
    assert_eq!(
        calls[0].params["meta"]["target"],
        json!({"kind": "service", "did": "did:example:service"})
    );
    assert_eq!(
        calls[0].params["body"]["group_key_package"]["key_package_id"],
        "kp-explicit"
    );
    assert_eq!(
        calls[0].params["body"]["group_key_package"]["device_id"],
        "device-a"
    );
    assert_eq!(
        calls[0].params["body"]["group_key_package"]["purpose"],
        "recovery"
    );
    assert_eq!(
        calls[0].params["body"]["group_key_package"]["group_did"],
        "did:example:groups:secure"
    );
    assert!(calls[0].params["body"]["group_key_package"]["did_wba_binding"]["proof"].is_object());
    assert_group_key_package_binding_uses_base58btc_object_proof(
        &calls[0].params["body"]["group_key_package"]["did_wba_binding"],
    );
    verify_group_key_package_binding_contract(
        &calls[0].params["body"]["group_key_package"]["did_wba_binding"],
        &fixture.did_bundle.did_document,
    )
    .unwrap();
    assert!(calls[0].params["auth"]["origin_proof"].is_object());
}

fn assert_group_key_package_binding_uses_base58btc_object_proof(binding: &Value) {
    let proof_value = binding["proof"]["proofValue"]
        .as_str()
        .expect("binding proofValue should be a string");
    assert!(
        proof_value.starts_with('z'),
        "binding proofValue should use multibase base58btc"
    );
    let signature =
        decode_base58btc_signature(&proof_value[1..]).expect("proofValue should be base58btc");
    assert_eq!(
        signature.len(),
        64,
        "binding proofValue should encode one Ed25519 signature"
    );
}

fn decode_base58btc_signature(value: &str) -> Option<Vec<u8>> {
    const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut decoded = Vec::new();
    for byte in value.bytes() {
        let mut carry = ALPHABET
            .as_bytes()
            .iter()
            .position(|candidate| *candidate == byte)?;
        for current in decoded.iter_mut().rev() {
            let next = (*current as usize * 58) + carry;
            *current = (next & 0xff) as u8;
            carry = next >> 8;
        }
        while carry > 0 {
            decoded.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let leading_zeroes = value
        .bytes()
        .take_while(|byte| *byte == ALPHABET.as_bytes()[0])
        .count();
    let first_non_zero = decoded
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(decoded.len());
    let mut output = vec![0; leading_zeroes];
    output.extend_from_slice(&decoded[first_non_zero..]);
    Some(output)
}

#[derive(Default)]
struct RecordingKeyPackageProvider {
    generated: Arc<Mutex<Vec<GenerateKeyPackageInput>>>,
}

impl GroupMlsProvider for RecordingKeyPackageProvider {
    fn generate_key_package(
        &self,
        input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        self.generated.lock().unwrap().push(input.clone());
        let owner_did = input.owner_did.clone();
        Ok(GroupKeyPackageOutput {
            group_key_package: anp::group_e2ee::GroupKeyPackage {
                key_package_id: input
                    .key_package_id
                    .clone()
                    .unwrap_or_else(|| "kp-generated".to_owned()),
                owner_did: owner_did.clone(),
                device_id: Some(input.device_id),
                purpose: input.purpose,
                group_did: input.group_did,
                suite: anp::group_e2ee::MTI_SUITE.to_owned(),
                mls_key_package_b64u: "mls-key-package".to_owned(),
                did_wba_binding: json!({
                    "agent_did": owner_did,
                    "verification_method": format!("{owner_did}#key-1"),
                    "leaf_signature_key_b64u": "leaf-key",
                    "issued_at": "2026-01-01T00:00:00Z",
                    "expires_at": "2099-01-01T00:00:00Z"
                }),
                expires_at: None,
                non_cryptographic: false,
                artifact_mode: None,
            },
        })
    }

    fn create_group_prepare(
        &self,
        _input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not create groups")
    }

    fn add_member_prepare(
        &self,
        _input: AddMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not add members")
    }

    fn remove_member_prepare(
        &self,
        _input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not remove members")
    }

    fn leave_prepare(&self, _input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not leave groups")
    }

    fn update_member_prepare(
        &self,
        _input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not update members")
    }

    fn recover_member_prepare(
        &self,
        _input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("publish key package should not recover members")
    }

    fn finalize_commit(
        &self,
        _input: FinalizeCommitInput,
    ) -> crate::ImResult<FinalizeCommitOutput> {
        unreachable!("publish key package should not finalize commits")
    }

    fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        unreachable!("publish key package should not abort commits")
    }

    fn process_welcome(
        &self,
        _input: ProcessWelcomeInput,
    ) -> crate::ImResult<ProcessWelcomeOutput> {
        unreachable!("publish key package should not process welcomes")
    }

    fn process_notice(&self, _input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        unreachable!("publish key package should not process notices")
    }

    fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        unreachable!("publish key package should not encrypt")
    }

    fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        unreachable!("publish key package should not decrypt")
    }

    fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
        unreachable!("publish key package should not read status")
    }
}

struct RecordingAsyncTransport {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
    response: Value,
}

impl AsyncAuthenticatedRpcTransport for RecordingAsyncTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.lock().unwrap().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        Ok(self.response.clone())
    }
}

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

struct Fixture {
    root: PathBuf,
    did_bundle: anp::authentication::DidDocumentBundle,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        std::fs::create_dir_all(root.join("identities").join("alice")).unwrap();
        Self {
            root,
            did_bundle: test_did_bundle(),
        }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "example.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
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
        .client(crate::identity::IdentitySelector::Did(
            crate::ids::Did::parse(self.did_bundle.did().unwrap()).unwrap(),
        ))
        .unwrap()
    }

    fn credentials(&self) -> crate::internal::message_runtime::group::GroupTextCredentials {
        let key1_private_pem = self.did_bundle.private_key_pem("key-1").unwrap().to_owned();
        crate::internal::message_runtime::group::GroupTextCredentials {
            identity_name: "alice".to_owned(),
            did_document: Some(self.did_bundle.did_document.clone()),
            key1_private_pem,
        }
    }
}

fn test_did_bundle() -> anp::authentication::DidDocumentBundle {
    anp::authentication::create_did_wba_document(
        "example.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["alice".to_owned()],
            domain: Some("example.test".to_owned()),
            challenge: Some("group-key-package-publish-test".to_owned()),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap()
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-group-key-package-publish-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn rebind_terminal_errors_use_exact_service_codes_not_messages() {
    let terminal = crate::ImError::Service {
        status_code: Some(409),
        code: Some("3012".to_owned()),
        message: "localized message".to_owned(),
        data: None,
    };
    assert!(super::rebind_error_is_terminal(&terminal));

    let misleading_transient = crate::ImError::Service {
        status_code: Some(503),
        code: Some("temporarily_unavailable".to_owned()),
        message: "upstream group not found while retrying".to_owned(),
        data: None,
    };
    assert!(!super::rebind_error_is_terminal(&misleading_transient));
    assert!(!super::rebind_error_is_terminal(
        &crate::ImError::TransportUnavailable {
            detail: "not member cache lookup failed".to_owned(),
        }
    ));
}

#[test]
fn rebind_p6_phase_uses_structured_finalize_outcome() {
    use crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome::*;
    assert_eq!(super::p6_phase_after_add(Finalized), "awaiting_remove");
    assert_eq!(super::p6_phase_after_add(AcceptedNeedsRepair), "add_repair");
    assert_eq!(super::p6_phase_after_remove(Finalized), "complete");
    assert_eq!(
        super::p6_phase_after_remove(AcceptedNeedsRepair),
        "remove_repair"
    );
}

#[test]
fn rebind_repair_requires_operation_gone_and_expected_roster() {
    use anp::group_e2ee::operations::{PendingCommitStatus, StatusOutput};
    let mut status = StatusOutput {
        status: "active".to_owned(),
        epoch: Some("9".to_owned()),
        local_epoch: Some("9".to_owned()),
        pending_commits: Vec::new(),
        epoch_authenticator: None,
        member_dids: vec!["did:old".to_owned(), "did:new".to_owned()],
    };
    assert!(super::rebind_repair_status_is_finalized(
        &status, "op-add", "did:old", "did:new", true
    )
    .is_ok());
    assert!(super::rebind_repair_status_is_finalized(
        &status,
        "op-remove",
        "did:old",
        "did:new",
        false
    )
    .is_err());
    status.pending_commits.push(PendingCommitStatus {
        pending_commit_id: "pc".to_owned(),
        operation_id: "op-add".to_owned(),
        agent_did: "did:owner".to_owned(),
        device_id: "default".to_owned(),
        group_did: "did:group".to_owned(),
        subject_did: "did:new".to_owned(),
        subject_status: "active".to_owned(),
        from_epoch: "8".to_owned(),
        to_epoch: "9".to_owned(),
        status: "pending".to_owned(),
    });
    assert!(super::rebind_repair_status_is_finalized(
        &status, "op-add", "did:old", "did:new", true
    )
    .is_err());
    status.pending_commits.clear();
    status.member_dids = vec!["did:new".to_owned()];
    assert!(super::rebind_repair_status_is_finalized(
        &status, "op-add", "did:old", "did:new", true
    )
    .is_err());
    assert!(super::rebind_repair_status_is_finalized(
        &status,
        "op-remove",
        "did:old",
        "did:new",
        false
    )
    .is_ok());
}
