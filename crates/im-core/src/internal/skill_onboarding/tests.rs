use super::*;
use std::collections::VecDeque;

struct Fixture {
    root: tempfile::TempDir,
    core: crate::core::ImCore,
}

impl Fixture {
    fn file_compat() -> Self {
        Self::new(false)
    }

    fn vault_required() -> Self {
        Self::new(true)
    }

    fn new(vault_required: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir_all(workspace.join("identities")).unwrap();
        fs::create_dir_all(workspace.join("local")).unwrap();
        fs::create_dir_all(workspace.join("cache")).unwrap();
        fs::create_dir_all(workspace.join("tmp")).unwrap();
        let config = crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://awiki.info").unwrap(),
            did_domain: "awiki.info".to_owned(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        };
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: workspace.join("identities"),
                registry_path: workspace.join("identities/registry.json"),
                default_identity_path: Some(workspace.join("identities/default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: workspace.join("local/im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: workspace.join("cache"),
                temp_dir: workspace.join("tmp"),
            },
        };
        let core = if vault_required {
            crate::core::ImCore::new_with_options(
                config,
                paths,
                crate::core::ImCoreOpenOptions::default().with_identity_secret_vault(
                    crate::core::IdentitySecretStoragePolicy::VaultRequired,
                    crate::core::ImCoreSecretVaultOptions::new(
                        crate::vault::DeviceVaultRootKey::from_bytes([9_u8; 32]),
                        workspace.join("vault"),
                        "workspace-test",
                        "device-test",
                    ),
                ),
            )
            .unwrap()
        } else {
            crate::core::ImCore::new(config, paths).unwrap()
        };
        Self { root, core }
    }

    fn journal_text(&self) -> String {
        fs::read_to_string(journal_path(&self.core)).unwrap()
    }

    fn all_text(&self) -> String {
        collect_text(self.root.path())
    }
}

#[derive(Default)]
struct FakeRemote {
    verify_calls: usize,
    exchange_calls: usize,
    authenticate_calls: usize,
    greeting_calls: usize,
    exchanged_dids: Vec<crate::ids::Did>,
    greeting_ids: Vec<crate::ids::MessageId>,
    failures: VecDeque<FailurePoint>,
    response_did_override: Option<crate::ids::Did>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    Exchange,
    Authenticate,
    Greeting,
}

impl FakeRemote {
    fn with_failure(failure: FailurePoint) -> Self {
        Self {
            failures: VecDeque::from([failure]),
            ..Self::default()
        }
    }

    fn should_fail(&mut self, failure: FailurePoint) -> bool {
        if self.failures.front() == Some(&failure) {
            self.failures.pop_front();
            true
        } else {
            false
        }
    }
}

impl SkillOnboardingRemote for FakeRemote {
    async fn verify_token(
        &mut self,
        _token: &crate::onboarding::SkillOnboardingToken,
    ) -> crate::ImResult<SkillTokenMetadata> {
        self.verify_calls += 1;
        Ok(valid_metadata())
    }

    async fn exchange_token(
        &mut self,
        _token: &crate::onboarding::SkillOnboardingToken,
        metadata: &SkillTokenMetadata,
        pending: &PendingIdentityBundle,
    ) -> crate::ImResult<SkillExchangeResult> {
        self.exchange_calls += 1;
        self.exchanged_dids.push(pending.did.clone());
        if self.should_fail(FailurePoint::Exchange) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "exchange unavailable".to_owned(),
            });
        }
        Ok(SkillExchangeResult {
            token_id: metadata.token_id.clone(),
            did: self
                .response_did_override
                .clone()
                .unwrap_or_else(|| pending.did.clone()),
            user_id: "agent-user-skill".to_owned(),
            controller_did: metadata.controller_did.clone(),
            controller_handle: metadata.controller_handle.clone(),
            agent_handle: metadata.agent_handle.clone(),
            status: "registered".to_owned(),
        })
    }

    async fn authenticate(
        &mut self,
        _local_alias: &str,
        _metadata: &SkillTokenMetadata,
        _pending: &PendingIdentityBundle,
    ) -> crate::ImResult<String> {
        self.authenticate_calls += 1;
        if self.should_fail(FailurePoint::Authenticate) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "DID auth unavailable".to_owned(),
            });
        }
        Ok("jwt-agent-secret".to_owned())
    }

    async fn send_controller_greeting(
        &mut self,
        _local_alias: &str,
        _controller_did: &crate::ids::Did,
        message_id: &crate::ids::MessageId,
    ) -> crate::ImResult<()> {
        self.greeting_calls += 1;
        self.greeting_ids.push(message_id.clone());
        if self.should_fail(FailurePoint::Greeting) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "greeting unavailable".to_owned(),
            });
        }
        Ok(())
    }
}

#[tokio::test]
async fn claim_completes_and_persists_no_raw_token_in_workspace() {
    let fixture = Fixture::file_compat();
    let mut remote = FakeRemote::default();
    let request = valid_request();
    let token_secret = request.token.expose().to_owned();

    let result = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap();

    assert_eq!(
        result.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(result.phase, crate::onboarding::SkillClaimPhase::Completed);
    assert!(!result.retryable);
    assert_eq!(remote.verify_calls, 1);
    assert_eq!(remote.exchange_calls, 1);
    assert_eq!(remote.authenticate_calls, 1);
    assert_eq!(remote.greeting_calls, 1);
    let identities = fixture.core.identities().list_async().await.unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].did, result.agent_did);
    assert!(identities[0].is_default);
    assert!(!fixture.all_text().contains(&token_secret));
    assert!(!fixture.journal_text().contains("jwt-agent-secret"));
    assert!(!fixture.journal_text().contains("PRIVATE KEY"));
    assert!(fixture.journal_text().contains("completed"));
}

#[tokio::test]
async fn exchange_failure_resumes_same_pending_did_without_reverify() {
    let fixture = Fixture::file_compat();
    let request = valid_request();
    let mut remote = FakeRemote::with_failure(FailurePoint::Exchange);

    let first = claim_with_remote(&fixture.core, request.clone(), &mut remote)
        .await
        .unwrap_err();
    assert_skill_error(&first, "skill_onboarding_transport_unavailable", true);
    let result = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap();

    assert_eq!(
        result.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(remote.verify_calls, 1);
    assert_eq!(remote.exchange_calls, 2);
    assert_eq!(remote.exchanged_dids[0], remote.exchanged_dids[1]);
}

#[tokio::test]
async fn auth_failure_replays_exchange_with_same_did() {
    let fixture = Fixture::file_compat();
    let request = valid_request();
    let mut remote = FakeRemote::with_failure(FailurePoint::Authenticate);

    let first = claim_with_remote(&fixture.core, request.clone(), &mut remote)
        .await
        .unwrap_err();
    assert_skill_error(&first, "skill_onboarding_transport_unavailable", true);
    let result = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap();

    assert_eq!(
        result.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(remote.exchange_calls, 2);
    assert_eq!(remote.authenticate_calls, 2);
    assert_eq!(remote.exchanged_dids[0], remote.exchanged_dids[1]);
}

#[tokio::test]
async fn greeting_failure_returns_pending_then_reuses_same_message_id() {
    let fixture = Fixture::file_compat();
    let request = valid_request();
    let mut remote = FakeRemote::with_failure(FailurePoint::Greeting);

    let pending = claim_with_remote(&fixture.core, request.clone(), &mut remote)
        .await
        .unwrap();
    assert_eq!(
        pending.status,
        crate::onboarding::SkillClaimStatus::GreetingPending
    );
    assert!(pending.retryable);
    assert_eq!(
        pending.error_code.as_deref(),
        Some("skill_onboarding_greeting_pending")
    );
    let completed = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap();

    assert_eq!(
        completed.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(remote.verify_calls, 1);
    assert_eq!(remote.exchange_calls, 1);
    assert_eq!(remote.authenticate_calls, 1);
    assert_eq!(remote.greeting_calls, 2);
    assert_eq!(remote.greeting_ids[0], remote.greeting_ids[1]);
}

#[tokio::test]
async fn nonempty_workspace_fails_before_remote_call() {
    let fixture = Fixture::file_compat();
    let request = valid_request();
    let mut first_remote = FakeRemote::default();
    claim_with_remote(&fixture.core, request.clone(), &mut first_remote)
        .await
        .unwrap();
    fs::remove_file(journal_path(&fixture.core)).unwrap();
    let mut remote = FakeRemote::default();

    let error = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap_err();

    assert_skill_error(&error, "skill_onboarding_workspace_conflict", false);
    assert_eq!(remote.verify_calls, 0);
    assert_eq!(remote.exchange_calls, 0);
}

#[tokio::test]
async fn response_mismatch_does_not_commit_identity() {
    let fixture = Fixture::file_compat();
    let mut remote = FakeRemote {
        response_did_override: Some(
            crate::ids::Did::parse("did:wba:awiki.info:agent:skill:other:e1_x").unwrap(),
        ),
        ..FakeRemote::default()
    };

    let error = claim_with_remote(&fixture.core, valid_request(), &mut remote)
        .await
        .unwrap_err();

    assert_skill_error(&error, "skill_onboarding_response_mismatch", false);
    assert!(fixture
        .core
        .identities()
        .list_async()
        .await
        .unwrap()
        .is_empty());
    assert!(fixture.journal_text().contains("identity_pending"));
}

#[tokio::test]
async fn cross_origin_and_expected_handle_mismatch_fail_closed() {
    let fixture = Fixture::file_compat();
    let mut remote = FakeRemote::default();
    let mut wrong_origin = valid_request();
    wrong_origin.service_base_url = "https://awiki.ai".to_owned();

    let origin_error = claim_with_remote(&fixture.core, wrong_origin, &mut remote)
        .await
        .unwrap_err();
    assert_skill_error(&origin_error, "skill_onboarding_scope_mismatch", false);
    assert_eq!(remote.verify_calls, 0);

    let mut wrong_handle = valid_request();
    wrong_handle.expected_agent_handle = "skill-other.awiki.info".to_owned();
    let handle_error = claim_with_remote(&fixture.core, wrong_handle, &mut remote)
        .await
        .unwrap_err();
    assert_skill_error(&handle_error, "skill_onboarding_scope_mismatch", false);
    assert_eq!(remote.verify_calls, 1);
    assert!(!journal_path(&fixture.core).exists());
}

#[tokio::test]
async fn vault_required_keeps_identity_secrets_out_of_plaintext_files() {
    let fixture = Fixture::vault_required();
    let mut remote = FakeRemote::default();

    let result = claim_with_remote(&fixture.core, valid_request(), &mut remote)
        .await
        .unwrap();

    assert_eq!(
        result.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    let text = fixture.all_text();
    assert!(!text.contains("jwt-agent-secret"));
    assert!(!text.contains("BEGIN PRIVATE KEY"));
    assert!(!pending_dir(&fixture.core).exists());
}

#[test]
fn token_and_pending_debug_are_redacted() {
    let token = crate::onboarding::SkillOnboardingToken::new("awsk1_super-secret-value").unwrap();
    let token_debug = format!("{token:?}");
    assert!(!token_debug.contains("super-secret-value"));
    assert!(token_debug.contains("redacted"));

    let generated = crate::internal::identity_generation::generate_skill_handle_identity(
        "awiki.info",
        "skill-test",
        None,
        None,
    )
    .unwrap();
    let private_key = generated.key1_private_pem.clone();
    let pending = PendingIdentityBundle::from(generated);
    let debug = format!("{pending:?}");
    assert!(!debug.contains(&private_key));
    assert!(debug.contains("redacted-private-key"));
}

#[test]
fn document_digest_is_stable_across_object_key_order() {
    let left = json!({"b": 2, "a": {"d": 4, "c": 3}});
    let right: Value = serde_json::from_str(r#"{"a":{"c":3,"d":4},"b":2}"#).unwrap();

    assert_eq!(
        document_digest(&left).unwrap(),
        document_digest(&right).unwrap()
    );
}

fn valid_request() -> crate::onboarding::SkillClaimRequest {
    crate::onboarding::SkillClaimRequest {
        token: crate::onboarding::SkillOnboardingToken::new("awsk1_test-secret-value").unwrap(),
        service_base_url: "https://awiki.info".to_owned(),
        expected_controller_handle: "alice.awiki.info".to_owned(),
        expected_agent_handle: "skill-test.awiki.info".to_owned(),
    }
}

fn valid_metadata() -> SkillTokenMetadata {
    SkillTokenMetadata {
        token_id: "agtok_skill_test".to_owned(),
        service_origin: "https://awiki.info".to_owned(),
        controller_did: crate::ids::Did::parse("did:wba:awiki.info:user:alice").unwrap(),
        controller_handle: crate::ids::Handle::parse("alice.awiki.info", "awiki.info").unwrap(),
        agent_handle: crate::ids::Handle::parse("skill-test.awiki.info", "awiki.info").unwrap(),
        expires_at: (OffsetDateTime::now_utc() + time::Duration::minutes(30))
            .format(&Rfc3339)
            .unwrap(),
    }
}

fn assert_skill_error(error: &crate::ImError, expected_code: &str, retryable: bool) {
    match error {
        crate::ImError::SkillOnboarding {
            code,
            retryable: actual_retryable,
            ..
        } => {
            assert_eq!(code, expected_code);
            assert_eq!(*actual_retryable, retryable);
        }
        other => panic!("expected Skill onboarding error, got {other:?}"),
    }
}

fn collect_text(root: &Path) -> String {
    let mut output = String::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap().flatten() {
                stack.push(entry.path());
            }
        } else if let Ok(raw) = fs::read(&path) {
            output.push_str(&String::from_utf8_lossy(&raw));
        }
    }
    output
}
