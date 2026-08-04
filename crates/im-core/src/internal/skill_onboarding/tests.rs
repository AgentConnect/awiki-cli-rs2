use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::collections::VecDeque;

struct Fixture {
    root: tempfile::TempDir,
    core: crate::core::ImCore,
    vault_required: bool,
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
        let core = Self::open_core(root.path(), vault_required);
        Self {
            root,
            core,
            vault_required,
        }
    }

    fn reopen_core(&self) -> crate::core::ImCore {
        Self::open_core(self.root.path(), self.vault_required)
    }

    fn open_core(root: &Path, vault_required: bool) -> crate::core::ImCore {
        let workspace = root.join("workspace");
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
        core
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
    prekey_calls: usize,
    greeting_calls: usize,
    exchanged_dids: Vec<crate::ids::Did>,
    exchanged_device_ids: Vec<crate::ids::ProtocolDeviceId>,
    exchanged_document_hashes: Vec<String>,
    exchanged_key_ids: Vec<(String, String, String)>,
    greeting_ids: Vec<crate::ids::MessageId>,
    failures: VecDeque<FailurePoint>,
    response_did_override: Option<crate::ids::Did>,
    controller_user_id_override: Option<String>,
    token_device_id_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    Exchange,
    Prekey,
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
        self.exchanged_dids.push(pending.generated.did.clone());
        self.exchanged_device_ids
            .push(pending.generated.protocol_device_id.clone());
        self.exchanged_document_hashes
            .push(pending.document_hash.clone());
        self.exchanged_key_ids.push((
            pending.generated.root_key_id.clone(),
            pending.generated.device_signing_key_id.clone(),
            pending.generated.device_e2ee_key_id.clone(),
        ));
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
                .unwrap_or_else(|| pending.generated.did.clone()),
            user_id: "agent-user-skill".to_owned(),
            controller_user_id: self
                .controller_user_id_override
                .clone()
                .unwrap_or_else(|| "controller-user".to_owned()),
            controller_did: metadata.controller_did.clone(),
            controller_handle: metadata.controller_handle.clone(),
            agent_handle: metadata.agent_handle.clone(),
            binding_generation: "1".to_owned(),
            status: "registered".to_owned(),
            access_token: device_access_token(
                pending,
                "agent-user-skill",
                self.token_device_id_override.as_deref(),
            ),
        })
    }

    async fn publish_device_prekey(&mut self, _did: &crate::ids::Did) -> crate::ImResult<()> {
        self.prekey_calls += 1;
        if self.should_fail(FailurePoint::Prekey) {
            return Err(crate::ImError::TransportUnavailable {
                detail: "PreKey publish unavailable".to_owned(),
            });
        }
        Ok(())
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
    let fixture = Fixture::vault_required();
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
    assert_eq!(remote.prekey_calls, 1);
    assert_eq!(remote.greeting_calls, 1);
    let identities = fixture.core.identities().list_async().await.unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].did, result.agent_did);
    assert!(result
        .agent_did
        .as_str()
        .contains(":agent:skill:skill-test:"));
    assert!(identities[0].is_default);
    let device = fixture
        .core
        .identities()
        .device_summary_async(crate::identity::IdentitySelector::Default)
        .await
        .unwrap();
    assert_eq!(device.mode, crate::identity::IdentityDeviceMode::VNext);
    assert_eq!(
        device.role,
        Some(crate::identity::IdentityDeviceRole::Admin)
    );
    assert_eq!(
        device.readiness,
        crate::identity::IdentityDeviceReadiness::AdminReady,
        "{device:?}"
    );
    assert_eq!(
        device.protocol_device_id,
        Some(remote.exchanged_device_ids[0].clone())
    );
    let client = fixture
        .core
        .client_async(crate::identity::IdentitySelector::Default)
        .await
        .unwrap();
    let binding = client.active_sync_account_binding().await.unwrap();
    assert_eq!(binding.account_id, "agent-user-skill");
    assert_eq!(binding.current_did, result.agent_did.as_str());
    assert_eq!(
        binding.protocol_device_id,
        remote.exchanged_device_ids[0].as_str()
    );
    assert_eq!(binding.identity_generation, "1");
    assert_eq!(binding.device_auth_generation, "1");
    assert!(!fixture.all_text().contains(&token_secret));
    assert!(!fixture.journal_text().contains("access_token"));
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
async fn restart_recovers_vault_pending_before_journal_with_same_did_device_and_keys() {
    let fixture = Fixture::vault_required();
    let pending = stage_pending_without_journal(&fixture.core);
    assert!(!journal_path(&fixture.core).exists());
    let restarted = fixture.reopen_core();
    let mut remote = FakeRemote::default();

    let result = claim_with_remote(&restarted, valid_request(), &mut remote)
        .await
        .unwrap();

    assert_eq!(result.agent_did, pending.generated.did);
    assert_remote_reused_pending(&remote, &pending);
    assert_eq!(remote.verify_calls, 1);
    assert_eq!(remote.exchange_calls, 1);
}

#[tokio::test]
async fn restart_recovers_truncated_initial_journal_from_pending_without_regenerating_keys() {
    let fixture = Fixture::file_compat();
    let pending = stage_pending_without_journal(&fixture.core);
    fs::write(
        journal_path(&fixture.core),
        br#"{"schema_version":2,"token_id":"agtok_skill_test""#,
    )
    .unwrap();
    let restarted = fixture.reopen_core();
    let mut remote = FakeRemote::default();

    let result = claim_with_remote(&restarted, valid_request(), &mut remote)
        .await
        .unwrap();

    assert_eq!(result.agent_did, pending.generated.did);
    assert_remote_reused_pending(&remote, &pending);
    let recovered = read_journal(&journal_path(&restarted)).unwrap().unwrap();
    assert_eq!(recovered.agent_did, pending.generated.did);
    assert_eq!(recovered.phase, JournalPhase::Completed);
}

#[tokio::test]
async fn restart_discards_truncated_unpublished_journal_temp_and_reuses_pending_keys() {
    let fixture = Fixture::file_compat();
    let pending = stage_pending_without_journal(&fixture.core);
    let journal = journal_path(&fixture.core);
    let temp = initial_write_temp_path(&journal);
    fs::write(&temp, br#"{"schema_version":2"#).unwrap();
    assert!(!journal.exists());
    let restarted = fixture.reopen_core();
    let mut remote = FakeRemote::default();

    let result = claim_with_remote(&restarted, valid_request(), &mut remote)
        .await
        .unwrap();

    assert_eq!(result.agent_did, pending.generated.did);
    assert_remote_reused_pending(&remote, &pending);
    assert!(journal.exists());
    assert!(!temp.exists());
}

#[tokio::test]
async fn truncated_journal_without_verified_pending_fails_closed_without_new_identity() {
    let fixture = Fixture::file_compat();
    fs::write(
        journal_path(&fixture.core),
        br#"{"schema_version":2,"token_id":"agtok_skill_test""#,
    )
    .unwrap();
    let restarted = fixture.reopen_core();
    let mut remote = FakeRemote::default();

    let error = claim_with_remote(&restarted, valid_request(), &mut remote)
        .await
        .unwrap_err();

    assert_skill_error(&error, "skill_onboarding_workspace_conflict", false);
    assert_eq!(remote.verify_calls, 1);
    assert_eq!(remote.exchange_calls, 0);
    assert!(restarted
        .identities()
        .list_async()
        .await
        .unwrap()
        .is_empty());
    assert!(!pending_dir(&restarted).exists());
}

#[tokio::test]
async fn prekey_failure_resumes_without_reexchange_and_keeps_same_device() {
    let fixture = Fixture::file_compat();
    let request = valid_request();
    let mut remote = FakeRemote::with_failure(FailurePoint::Prekey);

    let first = claim_with_remote(&fixture.core, request.clone(), &mut remote)
        .await
        .unwrap_err();
    assert_skill_error(&first, "skill_onboarding_prekey_pending", true);
    let result = claim_with_remote(&fixture.core, request, &mut remote)
        .await
        .unwrap();

    assert_eq!(
        result.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(remote.exchange_calls, 1);
    assert_eq!(remote.prekey_calls, 2);
    assert_eq!(remote.exchanged_device_ids.len(), 1);
}

#[test]
fn prekey_failure_classification_only_retries_transient_failures() {
    let transient = super::map_prekey_error(crate::ImError::TransportUnavailable {
        detail: "redacted".to_owned(),
    });
    assert_skill_error(&transient, "skill_onboarding_prekey_pending", true);

    let authorization = super::map_prekey_error(crate::ImError::PermissionDenied);
    assert_skill_error(
        &authorization,
        "skill_onboarding_prekey_not_authorized",
        false,
    );

    let local_state = super::map_prekey_error(crate::ImError::LocalStateUnavailable {
        detail: "redacted".to_owned(),
    });
    assert_skill_error(
        &local_state,
        "skill_onboarding_prekey_local_state_unavailable",
        false,
    );
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
    assert_eq!(remote.prekey_calls, 1);
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
async fn exchange_rejects_same_controller_account_and_wrong_device_token() {
    let controller_fixture = Fixture::file_compat();
    let mut same_account = FakeRemote {
        controller_user_id_override: Some("agent-user-skill".to_owned()),
        ..FakeRemote::default()
    };
    let controller_error =
        claim_with_remote(&controller_fixture.core, valid_request(), &mut same_account)
            .await
            .unwrap_err();
    assert_skill_error(
        &controller_error,
        "skill_onboarding_response_mismatch",
        false,
    );
    assert!(controller_fixture
        .core
        .identities()
        .list_async()
        .await
        .unwrap()
        .is_empty());

    let token_fixture = Fixture::file_compat();
    let mut wrong_device = FakeRemote {
        token_device_id_override: Some("dev_wrong".to_owned()),
        ..FakeRemote::default()
    };
    let token_error = claim_with_remote(&token_fixture.core, valid_request(), &mut wrong_device)
        .await
        .unwrap_err();
    assert_skill_error(&token_error, "skill_onboarding_response_mismatch", false);
    assert!(token_fixture
        .core
        .identities()
        .list_async()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn legacy_v1_artifacts_are_rejected_before_remote_calls() {
    let fixture = Fixture::file_compat();
    let legacy_path = fixture
        .core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(LEGACY_JOURNAL_FILE_NAME);
    fs::write(legacy_path, b"{}").unwrap();
    let mut remote = FakeRemote::default();

    let error = claim_with_remote(&fixture.core, valid_request(), &mut remote)
        .await
        .unwrap_err();

    assert_skill_error(
        &error,
        "skill_onboarding_legacy_claim_recovery_required",
        false,
    );
    assert_eq!(remote.verify_calls, 0);
    assert_eq!(remote.exchange_calls, 0);
}

#[tokio::test]
async fn legacy_recovery_missing_pending_material_requires_operator_reconciliation() {
    let fixture = Fixture::file_compat();
    let journal = legacy_journal(
        crate::ids::Did::parse("did:wba:awiki.info:agent:skill:skill-test:e1_missing-pending")
            .unwrap(),
        "legacy-document-hash",
        LegacyJournalPhase::IdentityPending,
    );
    fs::write(
        legacy_journal_path(&fixture.core),
        serde_json::to_vec_pretty(&journal).unwrap(),
    )
    .unwrap();
    let mut remote = FakeRemote::default();

    let error = recover_legacy_claim_with_remote(&fixture.core, valid_request(), &mut remote)
        .await
        .unwrap_err();

    assert_skill_error(&error, "blocked_requires_operator_reconciliation", false);
    assert_eq!(remote.verify_calls, 0);
    assert_eq!(remote.exchange_calls, 0);
}

#[tokio::test]
async fn legacy_recovery_resumes_after_upgrade_before_prekey_with_vnext_document_hash() {
    let fixture = Fixture::vault_required();
    let request = valid_request();
    let mut registration_remote = FakeRemote::default();
    let registered = claim_with_remote(&fixture.core, request.clone(), &mut registration_remote)
        .await
        .unwrap();
    let completed_vnext = read_journal(&journal_path(&fixture.core)).unwrap().unwrap();
    fs::remove_file(journal_path(&fixture.core)).unwrap();
    let legacy = LegacySkillClaimJournal {
        schema_version: 1,
        token_id: completed_vnext.token_id.clone(),
        service_origin: completed_vnext.service_origin.clone(),
        controller_did: completed_vnext.controller_did.clone(),
        controller_full_handle: completed_vnext.controller_full_handle.clone(),
        agent_handle: completed_vnext.agent_handle.clone(),
        agent_did: completed_vnext.agent_did.clone(),
        local_alias: completed_vnext.local_alias.clone(),
        did_document_digest: "pre-upgrade-legacy-document-hash".to_owned(),
        phase: LegacyJournalPhase::ControllerGreetingPending,
        greeting_message_id: completed_vnext.greeting_message_id.clone(),
        last_error_code: None,
        updated_at: now_rfc3339().unwrap(),
    };
    fs::write(
        legacy_journal_path(&fixture.core),
        serde_json::to_vec_pretty(&legacy).unwrap(),
    )
    .unwrap();
    let mut recovery_remote = FakeRemote::with_failure(FailurePoint::Prekey);

    let interrupted =
        recover_legacy_claim_with_remote(&fixture.core, request.clone(), &mut recovery_remote)
            .await
            .unwrap_err();
    assert_skill_error(&interrupted, "skill_onboarding_prekey_pending", true);
    assert!(!legacy_journal_path(&fixture.core).exists());
    let recovered_vnext = read_journal(&journal_path(&fixture.core)).unwrap().unwrap();
    let current_document = crate::internal::identity_document_cache::load_local_did_document_async(
        &fixture.core.inner().sdk_paths().identities,
        registered.agent_did.as_str(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        recovered_vnext.did_document_digest,
        crate::internal::identity_wire::document::document_hash(&current_document).unwrap()
    );
    assert_ne!(
        recovered_vnext.did_document_digest,
        legacy.did_document_digest
    );

    let completed = recover_legacy_claim_with_remote(&fixture.core, request, &mut recovery_remote)
        .await
        .unwrap();
    assert_eq!(
        completed.status,
        crate::onboarding::SkillClaimStatus::Completed
    );
    assert_eq!(completed.agent_did, registered.agent_did);
    assert_eq!(recovery_remote.prekey_calls, 2);
    assert_eq!(recovery_remote.greeting_calls, 1);
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
    assert!(!text.contains("awsk1_test-secret-value"));
    assert!(!text.contains("BEGIN PRIVATE KEY"));
    assert!(!pending_dir(&fixture.core).exists());
}

#[test]
fn token_and_pending_debug_are_redacted() {
    let token = crate::onboarding::SkillOnboardingToken::new("awsk1_super-secret-value").unwrap();
    let token_debug = format!("{token:?}");
    assert!(!token_debug.contains("super-secret-value"));
    assert!(token_debug.contains("redacted"));

    let generated = crate::internal::identity_generation::generate_vnext_agent_handle_identity(
        "awiki.info",
        crate::identity::AgentIdentityKind::Skill,
        "skill-test",
        None,
        None,
    )
    .unwrap();
    let private_key = generated.root_private_pem.clone();
    let pending = PendingIdentityBundle::new(generated).unwrap();
    let debug = format!("{pending:?}");
    assert!(!debug.contains(&private_key));
    assert!(debug.contains("redacted-vnext-identity"));
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

#[test]
fn exchange_response_allows_benign_additive_fields() {
    let result = parse_exchange_result(
        &json!({
            "agent_kind": "skill",
            "token_id": "agtok_skill_test",
            "did": "did:wba:awiki.info:agent:skill:skill-test:e1_test",
            "user_id": "agent-user-skill",
            "controller_user_id": "controller-user",
            "controller_did": "did:wba:awiki.info:user:alice",
            "controller_full_handle": "alice.awiki.info",
            "handle": "skill-test.awiki.info",
            "binding_generation": "1",
            "status": "registered",
            "access_token": "opaque-access-token",
            "future_server_field": {"version": 3}
        }),
        "awiki.info",
    )
    .unwrap();

    assert_eq!(result.user_id, "agent-user-skill");
    assert_eq!(result.binding_generation, "1");
}

fn legacy_journal(
    agent_did: crate::ids::Did,
    did_document_digest: &str,
    phase: LegacyJournalPhase,
) -> LegacySkillClaimJournal {
    let metadata = valid_metadata();
    LegacySkillClaimJournal {
        schema_version: 1,
        token_id: metadata.token_id,
        service_origin: metadata.service_origin,
        controller_did: metadata.controller_did,
        controller_full_handle: metadata.controller_handle,
        agent_handle: metadata.agent_handle,
        agent_did,
        local_alias: "skill-test".to_owned(),
        did_document_digest: did_document_digest.to_owned(),
        phase,
        greeting_message_id: greeting_message_id("agtok_skill_test").unwrap(),
        last_error_code: None,
        updated_at: now_rfc3339().unwrap(),
    }
}

fn valid_request() -> crate::onboarding::SkillClaimRequest {
    crate::onboarding::SkillClaimRequest {
        token: crate::onboarding::SkillOnboardingToken::new("awsk1_test-secret-value").unwrap(),
        service_base_url: "https://awiki.info".to_owned(),
        expected_controller_handle: "alice.awiki.info".to_owned(),
        expected_agent_handle: "skill-test.awiki.info".to_owned(),
    }
}

fn stage_pending_without_journal(core: &crate::core::ImCore) -> PendingIdentityBundle {
    let metadata = valid_metadata();
    let generated = crate::internal::identity_generation::generate_vnext_agent_handle_identity(
        core.inner().sdk_config().did_domain.as_str(),
        crate::identity::AgentIdentityKind::Skill,
        "skill-test",
        core.inner().sdk_config().anp_service_endpoint.as_ref(),
        core.inner().sdk_config().anp_service_did.as_ref(),
    )
    .unwrap();
    let pending = PendingIdentityBundle::new(generated).unwrap();
    let journal = journal_from_pending(&metadata, "skill-test".to_owned(), &pending).unwrap();
    validate_pending_identity(&journal, &pending).unwrap();
    validate_pending_identity_material(&journal, &pending).unwrap();
    save_pending_identity(core, &journal, &pending).unwrap();
    pending
}

fn assert_remote_reused_pending(remote: &FakeRemote, pending: &PendingIdentityBundle) {
    assert_eq!(remote.exchanged_dids, vec![pending.generated.did.clone()]);
    assert_eq!(
        remote.exchanged_device_ids,
        vec![pending.generated.protocol_device_id.clone()]
    );
    assert_eq!(
        remote.exchanged_document_hashes,
        vec![pending.document_hash.clone()]
    );
    assert_eq!(
        remote.exchanged_key_ids,
        vec![(
            pending.generated.root_key_id.clone(),
            pending.generated.device_signing_key_id.clone(),
            pending.generated.device_e2ee_key_id.clone(),
        )]
    );
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

fn device_access_token(
    pending: &PendingIdentityBundle,
    user_id: &str,
    device_id_override: Option<&str>,
) -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": pending.generated.did.as_str(),
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": pending.generated.did.as_str(),
        "user_id": user_id,
        "device_id": device_id_override
            .unwrap_or_else(|| pending.generated.protocol_device_id.as_str()),
        "key_id": pending.generated.device_signing_key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 300,
        "jti": "skill-device-token-1"
    });
    format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
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
