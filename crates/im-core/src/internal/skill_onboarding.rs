use crate::internal::transport::AsyncRpcTransport;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zeroize::Zeroizing;

const JOURNAL_SCHEMA_VERSION: u32 = 2;
const JOURNAL_FILE_NAME: &str = ".skill-onboarding-v2.json";
const PENDING_DIR_NAME: &str = ".skill-onboarding-v2";
const LEGACY_JOURNAL_FILE_NAME: &str = ".skill-onboarding-v1.json";
const LEGACY_PENDING_DIR_NAME: &str = ".skill-onboarding-v1";
const SKILL_PURPOSE: &str = "skill_onboarding_v1";
const GREETING_TEXT: &str = "AWiki Skill Agent 已完成注册，可以开始对话。";
const PENDING_SECRET_KEY_PREFIX: &str = "skill-onboarding-pending-v2-";
const LEGACY_PENDING_SECRET_KEY_PREFIX: &str = "skill-onboarding-pending-v1-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillTokenMetadata {
    token_id: String,
    service_origin: String,
    controller_did: crate::ids::Did,
    controller_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillExchangeResult {
    token_id: String,
    did: crate::ids::Did,
    user_id: String,
    controller_user_id: String,
    controller_did: crate::ids::Did,
    controller_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    binding_generation: String,
    status: String,
    access_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PendingIdentityBundle {
    generated: crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    document_hash: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct LegacyPendingIdentityBundle {
    did: crate::ids::Did,
    unique_id: String,
    did_document: Value,
    key1_private_pem: String,
    key1_public_pem: String,
    e2ee_signing_private_pem: String,
    e2ee_agreement_private_pem: String,
}

impl std::fmt::Debug for LegacyPendingIdentityBundle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LegacyPendingIdentityBundle")
            .field("did", &self.did)
            .field("unique_id", &self.unique_id)
            .field("did_document", &"<redacted-did-document>")
            .field("key1_private_pem", &"<redacted-private-key>")
            .field("key1_public_pem", &"<redacted-public-key>")
            .field("e2ee_signing_private_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_pem", &"<redacted-private-key>")
            .finish()
    }
}

impl std::fmt::Debug for PendingIdentityBundle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingIdentityBundle")
            .field("did", &self.generated.did)
            .field("protocol_device_id", &self.generated.protocol_device_id)
            .field("generated", &"<redacted-vnext-identity>")
            .field("document_hash", &self.document_hash)
            .finish()
    }
}

impl PendingIdentityBundle {
    fn new(
        generated: crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    ) -> crate::ImResult<Self> {
        let document_hash =
            crate::internal::identity_wire::document::document_hash(&generated.did_document)?;
        Ok(Self {
            generated,
            document_hash,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalPhase {
    IdentityPending,
    DevicePrekeyPending,
    ControllerGreetingPending,
    Completed,
}

enum InitialJournalState {
    Missing,
    Valid(SkillClaimJournal),
    Corrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillClaimJournal {
    schema_version: u32,
    token_id: String,
    service_origin: String,
    controller_did: crate::ids::Did,
    controller_full_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    agent_did: crate::ids::Did,
    local_alias: String,
    did_document_digest: String,
    phase: JournalPhase,
    greeting_message_id: crate::ids::MessageId,
    last_error_code: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyJournalPhase {
    IdentityPending,
    ControllerGreetingPending,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacySkillClaimJournal {
    schema_version: u32,
    token_id: String,
    service_origin: String,
    controller_did: crate::ids::Did,
    controller_full_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    agent_did: crate::ids::Did,
    local_alias: String,
    did_document_digest: String,
    phase: LegacyJournalPhase,
    greeting_message_id: crate::ids::MessageId,
    last_error_code: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacySkillExchangeResult {
    token_id: String,
    did: crate::ids::Did,
    user_id: String,
    controller_user_id: Option<String>,
    controller_did: crate::ids::Did,
    controller_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    status: String,
}

pub(crate) trait SkillOnboardingRemote {
    async fn verify_token(
        &mut self,
        token: &crate::onboarding::SkillOnboardingToken,
    ) -> crate::ImResult<SkillTokenMetadata>;

    async fn exchange_token(
        &mut self,
        token: &crate::onboarding::SkillOnboardingToken,
        metadata: &SkillTokenMetadata,
        pending: &PendingIdentityBundle,
    ) -> crate::ImResult<SkillExchangeResult>;

    async fn publish_device_prekey(&mut self, did: &crate::ids::Did) -> crate::ImResult<()>;

    async fn exchange_legacy_token(
        &mut self,
        _token: &crate::onboarding::SkillOnboardingToken,
        _metadata: &SkillTokenMetadata,
        _pending: &LegacyPendingIdentityBundle,
    ) -> crate::ImResult<LegacySkillExchangeResult> {
        Err(crate::ImError::unsupported(
            "skill-onboarding-legacy-claim-recovery",
        ))
    }

    async fn authenticate_legacy(
        &mut self,
        _local_alias: &str,
        _metadata: &SkillTokenMetadata,
        _pending: &LegacyPendingIdentityBundle,
    ) -> crate::ImResult<String> {
        Err(crate::ImError::unsupported(
            "skill-onboarding-legacy-claim-recovery",
        ))
    }

    async fn send_controller_greeting(
        &mut self,
        local_alias: &str,
        controller_did: &crate::ids::Did,
        message_id: &crate::ids::MessageId,
    ) -> crate::ImResult<()>;
}

pub(crate) struct ProductionSkillOnboardingRemote<'a> {
    core: &'a crate::core::ImCore,
    transport: crate::internal::transport::CorePlainTransport<'a>,
}

impl<'a> ProductionSkillOnboardingRemote<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self {
            core,
            transport: crate::internal::transport::CorePlainTransport::new_no_redirect(core),
        }
    }
}

impl SkillOnboardingRemote for ProductionSkillOnboardingRemote<'_> {
    async fn verify_token(
        &mut self,
        token: &crate::onboarding::SkillOnboardingToken,
    ) -> crate::ImResult<SkillTokenMetadata> {
        let result = self
            .transport
            .rpc(
                "/user-service/agent-registration/rpc",
                "verify_token",
                json!({"token": token.expose(), "agent_kind": "skill"}),
            )
            .await?;
        parse_token_metadata(&result, self.core.inner().sdk_config().did_domain.as_str())
    }

    async fn exchange_token(
        &mut self,
        token: &crate::onboarding::SkillOnboardingToken,
        metadata: &SkillTokenMetadata,
        pending: &PendingIdentityBundle,
    ) -> crate::ImResult<SkillExchangeResult> {
        let result = self
            .transport
            .rpc(
                "/user-service/agent-registration/rpc",
                "exchange_token",
                json!({
                    "token": token.expose(),
                    "agent_kind": "skill",
                    "controller_did": metadata.controller_did.as_str(),
                    "handle": metadata.agent_handle.as_str(),
                    "did_document": pending.generated.did_document,
                    "allow_existing_agent_did": false,
                }),
            )
            .await?;
        parse_exchange_result(&result, self.core.inner().sdk_config().did_domain.as_str())
    }

    async fn publish_device_prekey(&mut self, did: &crate::ids::Did) -> crate::ImResult<()> {
        crate::internal::identity_registration_runtime::publish_v2_prekeys_after_registration_async(
            self.core, did,
        )
        .await
    }

    async fn exchange_legacy_token(
        &mut self,
        token: &crate::onboarding::SkillOnboardingToken,
        metadata: &SkillTokenMetadata,
        pending: &LegacyPendingIdentityBundle,
    ) -> crate::ImResult<LegacySkillExchangeResult> {
        let result = self
            .transport
            .rpc(
                "/user-service/agent-registration/rpc",
                "exchange_token",
                json!({
                    "token": token.expose(),
                    "agent_kind": "skill",
                    "controller_did": metadata.controller_did.as_str(),
                    "handle": metadata.agent_handle.as_str(),
                    "did_document": pending.did_document,
                    "allow_existing_agent_did": false,
                }),
            )
            .await?;
        parse_legacy_exchange_result(&result, self.core.inner().sdk_config().did_domain.as_str())
    }

    async fn authenticate_legacy(
        &mut self,
        local_alias: &str,
        metadata: &SkillTokenMetadata,
        pending: &LegacyPendingIdentityBundle,
    ) -> crate::ImResult<String> {
        let client =
            self.core
                .client_with_identity_material(crate::identity::HostedIdentityMaterial {
                    identity_id: local_alias.to_owned(),
                    did: pending.did.as_str().to_owned(),
                    handle: Some(metadata.agent_handle.as_str().to_owned()),
                    display_name: Some("AWiki Skill Agent".to_owned()),
                    did_document: pending.did_document.clone(),
                    default_signing_private_key_pem: pending.key1_private_pem.clone(),
                    e2ee_agreement_private_key_pem: Some(
                        pending.e2ee_agreement_private_pem.clone(),
                    ),
                    auth_token: None,
                })?;
        client
            .auth()
            .refresh_session_async()
            .await?
            .bearer_token
            .filter(|token| !token.trim().is_empty())
            .ok_or(crate::ImError::AuthRequired)
    }

    async fn send_controller_greeting(
        &mut self,
        local_alias: &str,
        controller_did: &crate::ids::Did,
        message_id: &crate::ids::MessageId,
    ) -> crate::ImResult<()> {
        let client = self
            .core
            .client_async(crate::identity::IdentitySelector::LocalAlias(
                local_alias.to_owned(),
            ))
            .await?;
        let result = client
            .messages()
            .send_async(crate::messages::SendMessageRequest {
                target: crate::messages::MessageTarget::Direct(crate::ids::PeerRef::parse(
                    controller_did.as_str(),
                    client.did_domain(),
                )?),
                body: crate::messages::MessageBody::Text {
                    text: GREETING_TEXT.to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                security: crate::messages::MessageSecurityMode::DefaultPlain,
                client_message_id: Some(message_id.clone()),
                delivery: crate::messages::MessageDeliveryOptions {
                    idempotency_key: Some(message_id.as_str().to_owned()),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            })
            .await?;
        match result.delivery {
            crate::messages::DeliveryState::Accepted | crate::messages::DeliveryState::Sent => {
                Ok(())
            }
            crate::messages::DeliveryState::StoredLocally => {
                Err(crate::ImError::TransportUnavailable {
                    detail: "Controller greeting was not accepted by Message Service".to_owned(),
                })
            }
            crate::messages::DeliveryState::Failed { .. } => {
                Err(crate::ImError::TransportUnavailable {
                    detail: "Controller greeting delivery failed".to_owned(),
                })
            }
        }
    }
}

pub(crate) async fn claim_with_remote<R: SkillOnboardingRemote>(
    core: &crate::core::ImCore,
    request: crate::onboarding::SkillClaimRequest,
    remote: &mut R,
) -> crate::ImResult<crate::onboarding::SkillClaimResult> {
    let origin = validated_service_origin(core, &request.service_base_url)?;
    let expected_controller = validated_full_handle(
        &request.expected_controller_handle,
        core.inner().sdk_config().did_domain.as_str(),
        "expected_controller_handle",
    )?;
    let expected_agent = validated_full_handle(
        &request.expected_agent_handle,
        core.inner().sdk_config().did_domain.as_str(),
        "expected_agent_handle",
    )?;
    ensure_workspace_initialized(core)?;
    ensure_no_legacy_artifacts(core)?;

    let journal_path = journal_path(core);
    let initial_journal = read_initial_journal(&journal_path)?;
    let journal_was_corrupt = matches!(initial_journal, InitialJournalState::Corrupt);
    let mut journal = match initial_journal {
        InitialJournalState::Missing | InitialJournalState::Corrupt => None,
        InitialJournalState::Valid(journal) => Some(journal),
    };
    let identities = core.identities().list_async().await.map_err(|_| {
        onboarding_error(
            "skill_onboarding_workspace_conflict",
            "workspace_check",
            false,
        )
    })?;
    if let Some(existing) = journal.as_mut() {
        validate_journal_request(existing, &origin, &expected_controller, &expected_agent)?;
        match matching_ready_identity(&identities, existing) {
            ReadyIdentityState::Matching => {
                validate_matching_vnext_identity(core, existing).await?;
                if existing.phase == JournalPhase::IdentityPending {
                    existing.phase = JournalPhase::DevicePrekeyPending;
                    existing.last_error_code = None;
                    existing.updated_at = now_rfc3339()?;
                    write_journal(&journal_path, existing)?;
                }
            }
            ReadyIdentityState::Conflict => return Err(workspace_conflict()),
            ReadyIdentityState::Empty if existing.phase != JournalPhase::IdentityPending => {
                return Err(workspace_conflict());
            }
            ReadyIdentityState::Empty => {}
        }
    } else {
        if !identities.is_empty() {
            return Err(workspace_conflict());
        }
        let metadata = remote
            .verify_token(&request.token)
            .await
            .map_err(|error| map_remote_error(error, "verify"))?;
        validate_verified_metadata(
            core,
            &metadata,
            &origin,
            &expected_controller,
            &expected_agent,
        )?;
        let local_part = handle_local_part(&metadata.agent_handle, core)?;
        let pending = match load_orphan_pending_identity(core, &metadata.token_id, &local_part)? {
            Some(pending) => pending,
            None if journal_was_corrupt => return Err(workspace_conflict()),
            None => {
                let generated =
                    crate::internal::identity_generation::generate_vnext_agent_handle_identity(
                        core.inner().sdk_config().did_domain.as_str(),
                        crate::identity::AgentIdentityKind::Skill,
                        &local_part,
                        core.inner().sdk_config().anp_service_endpoint.as_ref(),
                        core.inner().sdk_config().anp_service_did.as_ref(),
                    )?;
                PendingIdentityBundle::new(generated)?
            }
        };
        let next = journal_from_pending(&metadata, local_part, &pending)?;
        validate_pending_identity(&next, &pending)?;
        validate_pending_identity_material(&next, &pending)?;
        if !pending_identity_exists(core, &next)? {
            save_pending_identity(core, &next, &pending)?;
        }
        if journal_was_corrupt {
            write_journal(&journal_path, &next)?;
        } else {
            create_journal(&journal_path, &next)?;
        }
        journal = Some(next);
    }

    let mut journal = journal.ok_or_else(|| {
        onboarding_error("skill_onboarding_local_commit_failed", "journal_load", true)
    })?;
    if journal.phase == JournalPhase::IdentityPending {
        let pending = load_pending_identity(core, &journal)?;
        validate_pending_identity(&journal, &pending)?;
        let metadata = metadata_from_journal(&journal);
        let exchange = remote
            .exchange_token(&request.token, &metadata, &pending)
            .await
            .map_err(|error| map_remote_error(error, "exchange"))?;
        validate_exchange(&journal, &pending, &exchange)?;
        persist_ready_identity(core, &journal, &exchange, &pending).await?;
        journal.phase = JournalPhase::DevicePrekeyPending;
        journal.last_error_code = None;
        journal.updated_at = now_rfc3339()?;
        write_journal(&journal_path, &journal)?;
        let _ = delete_pending_identity(core, &journal);
    }

    complete_vnext_onboarding(core, journal, remote).await
}

async fn complete_vnext_onboarding<R: SkillOnboardingRemote>(
    core: &crate::core::ImCore,
    mut journal: SkillClaimJournal,
    remote: &mut R,
) -> crate::ImResult<crate::onboarding::SkillClaimResult> {
    let journal_path = journal_path(core);
    if journal.phase == JournalPhase::DevicePrekeyPending {
        if let Err(error) = remote.publish_device_prekey(&journal.agent_did).await {
            let error = map_prekey_error(error);
            journal.last_error_code = Some(stable_error_code(&error).to_owned());
            journal.updated_at = now_rfc3339()?;
            write_journal(&journal_path, &journal)?;
            return Err(error);
        }
        journal.phase = JournalPhase::ControllerGreetingPending;
        journal.last_error_code = None;
        journal.updated_at = now_rfc3339()?;
        write_journal(&journal_path, &journal)?;
    }

    if journal.phase == JournalPhase::ControllerGreetingPending {
        if let Err(error) = remote
            .send_controller_greeting(
                &journal.local_alias,
                &journal.controller_did,
                &journal.greeting_message_id,
            )
            .await
        {
            journal.last_error_code = Some(stable_error_code(&error).to_owned());
            journal.updated_at = now_rfc3339()?;
            write_journal(&journal_path, &journal)?;
            return claim_result(
                &journal,
                crate::onboarding::SkillClaimStatus::GreetingPending,
                true,
            );
        }
        journal.phase = JournalPhase::Completed;
        journal.last_error_code = None;
        journal.updated_at = now_rfc3339()?;
        write_journal(&journal_path, &journal)?;
    }

    claim_result(
        &journal,
        crate::onboarding::SkillClaimStatus::Completed,
        false,
    )
}

pub(crate) async fn recover_legacy_claim_with_remote<R: SkillOnboardingRemote>(
    core: &crate::core::ImCore,
    request: crate::onboarding::SkillClaimRequest,
    remote: &mut R,
) -> crate::ImResult<crate::onboarding::SkillClaimResult> {
    let origin = validated_service_origin(core, &request.service_base_url)?;
    let expected_controller = validated_full_handle(
        &request.expected_controller_handle,
        core.inner().sdk_config().did_domain.as_str(),
        "expected_controller_handle",
    )?;
    let expected_agent = validated_full_handle(
        &request.expected_agent_handle,
        core.inner().sdk_config().did_domain.as_str(),
        "expected_agent_handle",
    )?;
    ensure_workspace_initialized(core)?;

    if let Some(journal) = read_journal(&journal_path(core))? {
        validate_journal_request(&journal, &origin, &expected_controller, &expected_agent)?;
        let identities = core.identities().list_async().await?;
        if !matches!(
            matching_ready_identity(&identities, &journal),
            ReadyIdentityState::Matching
        ) {
            return Err(workspace_conflict());
        }
        validate_matching_vnext_identity(core, &journal).await?;
        cleanup_legacy_artifacts_after_v2_commit(core)?;
        return complete_vnext_onboarding(core, journal, remote).await;
    }

    let mut legacy = read_legacy_journal(core)?.ok_or_else(|| {
        onboarding_error(
            "skill_onboarding_legacy_recovery_not_found",
            "legacy_journal",
            false,
        )
    })?;
    validate_legacy_journal_request(&legacy, &origin, &expected_controller, &expected_agent)?;
    let identities = core.identities().list_async().await.map_err(|_| {
        onboarding_error(
            "skill_onboarding_workspace_conflict",
            "workspace_check",
            false,
        )
    })?;
    match matching_legacy_identity(&identities, &legacy) {
        ReadyIdentityState::Empty if legacy.phase == LegacyJournalPhase::IdentityPending => {
            let pending = load_legacy_pending_identity(core, &legacy)?;
            validate_legacy_pending_identity(&legacy, &pending)?;
            let metadata = legacy_metadata_from_journal(&legacy);
            let exchange = remote
                .exchange_legacy_token(&request.token, &metadata, &pending)
                .await
                .map_err(|error| map_remote_error(error, "legacy_exchange"))?;
            validate_legacy_exchange(&legacy, &exchange)?;
            let access_token = remote
                .authenticate_legacy(&legacy.local_alias, &metadata, &pending)
                .await
                .map_err(|error| map_remote_error(error, "legacy_authenticate"))?;
            persist_legacy_recovery_identity(
                core,
                &legacy,
                &exchange.user_id,
                &access_token,
                &pending,
            )
            .await?;
            legacy.phase = LegacyJournalPhase::ControllerGreetingPending;
            legacy.last_error_code = None;
            legacy.updated_at = now_rfc3339()?;
            write_legacy_journal(core, &legacy)?;
        }
        ReadyIdentityState::Matching => {}
        ReadyIdentityState::Empty | ReadyIdentityState::Conflict => {
            return Err(workspace_conflict())
        }
    }

    let status = core
        .identities()
        .upgrade_legacy_identity_async(crate::identity::IdentitySelector::LocalAlias(
            legacy.local_alias.clone(),
        ))
        .await?;
    if let crate::identity::LegacyUpgradeStatus::RetryRequired { code, .. } = status {
        return Err(onboarding_error(
            "skill_onboarding_legacy_upgrade_retry_required",
            &code,
            true,
        ));
    }
    if status != crate::identity::LegacyUpgradeStatus::Completed {
        return Err(onboarding_error(
            "skill_onboarding_legacy_upgrade_retry_required",
            "legacy_upgrade_incomplete",
            true,
        ));
    }

    let upgraded_document =
        crate::internal::identity_document_cache::load_local_did_document_async(
            &core.inner().sdk_paths().identities,
            legacy.agent_did.as_str(),
        )
        .await
        .map_err(|_| legacy_operator_reconciliation_required("upgraded_did_document"))?
        .ok_or_else(|| legacy_operator_reconciliation_required("upgraded_did_document"))?;
    crate::core::validate_handle_service_for_did(
        &upgraded_document,
        &legacy.agent_did,
        &legacy.agent_handle,
    )
    .map_err(|_| legacy_operator_reconciliation_required("upgraded_did_document"))?;
    anp::authentication::validate_device_manifest(&upgraded_document)
        .map_err(|_| legacy_operator_reconciliation_required("upgraded_did_document"))?
        .ok_or_else(|| legacy_operator_reconciliation_required("upgraded_did_document"))?;
    let upgraded_document_digest =
        crate::internal::identity_wire::document::document_hash(&upgraded_document)
            .map_err(|_| legacy_operator_reconciliation_required("upgraded_did_document"))?;

    let vnext = SkillClaimJournal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        token_id: legacy.token_id.clone(),
        service_origin: legacy.service_origin.clone(),
        controller_did: legacy.controller_did.clone(),
        controller_full_handle: legacy.controller_full_handle.clone(),
        agent_handle: legacy.agent_handle.clone(),
        agent_did: legacy.agent_did.clone(),
        local_alias: legacy.local_alias.clone(),
        did_document_digest: upgraded_document_digest,
        phase: JournalPhase::DevicePrekeyPending,
        greeting_message_id: legacy.greeting_message_id.clone(),
        last_error_code: None,
        updated_at: now_rfc3339()?,
    };
    create_journal(&journal_path(core), &vnext)?;
    cleanup_legacy_artifacts_after_v2_commit(core)?;
    complete_vnext_onboarding(core, vnext, remote).await
}

async fn persist_legacy_recovery_identity(
    core: &crate::core::ImCore,
    journal: &LegacySkillClaimJournal,
    user_id: &str,
    access_token: &str,
    pending: &LegacyPendingIdentityBundle,
) -> crate::ImResult<()> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
        core.inner().sdk_paths().identities.clone(),
        crate::internal::identity_store::SaveIdentityInput {
            local_alias: journal.local_alias.clone(),
            did: pending.did.clone(),
            unique_id: pending.unique_id.clone(),
            user_id: user_id.to_owned(),
            display_name: "AWiki Skill Agent".to_owned(),
            handle: journal.agent_handle.as_str().to_owned(),
            full_handle: journal.agent_handle.as_str().to_owned(),
            binding_generation: None,
            jwt_token: access_token.to_owned(),
            did_document: Some(pending.did_document.clone()),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
            device_state: None,
            key1_private_pem: pending.key1_private_pem.clone(),
            key1_public_pem: pending.key1_public_pem.clone(),
            e2ee_signing_private_pem: pending.e2ee_signing_private_pem.clone(),
            e2ee_agreement_private_pem: pending.e2ee_agreement_private_pem.clone(),
            daemon_subkey_package: None,
            make_default: true,
        },
        secret_storage,
    )
    .await
    .map(|_| ())
    .map_err(|_| {
        onboarding_error(
            "skill_onboarding_legacy_recovery_retry_required",
            "legacy_identity_save",
            true,
        )
    })
}

async fn persist_ready_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
    exchange: &SkillExchangeResult,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<()> {
    let secret_storage = crate::internal::identity_store::SaveIdentitySecretStorage::from_core(
        core,
    )
    .map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "identity_save",
            true,
        )
    })?;
    let save_input = crate::internal::identity_registration_runtime::vnext_bootstrap_save_input(
        crate::internal::identity_registration_runtime::VNextBootstrapSaveInput {
            generated: &pending.generated,
            document_hash: &pending.document_hash,
            local_alias: &journal.local_alias,
            display_name: "AWiki Skill Agent",
            user_id: &exchange.user_id,
            handle: journal.agent_handle.as_str(),
            full_handle: journal.agent_handle.as_str(),
            binding_generation: &exchange.binding_generation,
            access_token: &exchange.access_token,
            make_default: true,
        },
    )
    .map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "identity_save",
            true,
        )
    })?;
    crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
        core.inner().sdk_paths().identities.clone(),
        save_input,
        secret_storage,
    )
    .await
    .map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "identity_save",
            true,
        )
    })?;
    Ok(())
}

fn parse_token_metadata(result: &Value, domain: &str) -> crate::ImResult<SkillTokenMetadata> {
    if string_field(result, "agent_kind")? != "skill"
        || string_field(result, "status")? != "active"
        || result.get("one_time").and_then(Value::as_bool) != Some(true)
    {
        return Err(response_mismatch("verify"));
    }
    let scope = result
        .get("scope")
        .and_then(Value::as_object)
        .ok_or_else(|| response_mismatch("verify"))?;
    if scope.get("purpose").and_then(Value::as_str) != Some(SKILL_PURPOSE) {
        return Err(response_mismatch("verify"));
    }
    let agent_handle = crate::ids::Handle::parse(string_field(result, "handle")?, domain)?;
    if scope.get("agent_handle").and_then(Value::as_str) != Some(agent_handle.as_str()) {
        return Err(response_mismatch("verify"));
    }
    Ok(SkillTokenMetadata {
        token_id: string_field(result, "token_id")?.to_owned(),
        service_origin: scope
            .get("service_origin")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| response_mismatch("verify"))?
            .to_owned(),
        controller_did: crate::ids::Did::parse(string_field(result, "controller_did")?)?,
        controller_handle: crate::ids::Handle::parse(
            string_field(result, "controller_full_handle")?,
            domain,
        )?,
        agent_handle,
        expires_at: string_field(result, "expires_at")?.to_owned(),
    })
}

fn parse_exchange_result(result: &Value, domain: &str) -> crate::ImResult<SkillExchangeResult> {
    if !result.is_object() {
        return Err(response_mismatch("exchange"));
    }
    if string_field(result, "agent_kind")? != "skill" {
        return Err(response_mismatch("exchange"));
    }
    Ok(SkillExchangeResult {
        token_id: string_field(result, "token_id")?.to_owned(),
        did: crate::ids::Did::parse(string_field(result, "did")?)?,
        user_id: string_field(result, "user_id")?.to_owned(),
        controller_user_id: string_field(result, "controller_user_id")?.to_owned(),
        controller_did: crate::ids::Did::parse(string_field(result, "controller_did")?)?,
        controller_handle: crate::ids::Handle::parse(
            string_field(result, "controller_full_handle")?,
            domain,
        )?,
        agent_handle: crate::ids::Handle::parse(string_field(result, "handle")?, domain)?,
        binding_generation: string_field(result, "binding_generation")?.to_owned(),
        status: string_field(result, "status")?.to_owned(),
        access_token: string_field(result, "access_token")?.to_owned(),
    })
}

fn parse_legacy_exchange_result(
    result: &Value,
    domain: &str,
) -> crate::ImResult<LegacySkillExchangeResult> {
    if !result.is_object() || string_field(result, "agent_kind")? != "skill" {
        return Err(response_mismatch("legacy_exchange"));
    }
    Ok(LegacySkillExchangeResult {
        token_id: string_field(result, "token_id")?.to_owned(),
        did: crate::ids::Did::parse(string_field(result, "did")?)?,
        user_id: string_field(result, "user_id")?.to_owned(),
        controller_user_id: result
            .get("controller_user_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        controller_did: crate::ids::Did::parse(string_field(result, "controller_did")?)?,
        controller_handle: crate::ids::Handle::parse(
            string_field(result, "controller_full_handle")?,
            domain,
        )?,
        agent_handle: crate::ids::Handle::parse(string_field(result, "handle")?, domain)?,
        status: string_field(result, "status")?.to_owned(),
    })
}

fn validate_verified_metadata(
    core: &crate::core::ImCore,
    metadata: &SkillTokenMetadata,
    origin: &str,
    expected_controller: &crate::ids::Handle,
    expected_agent: &crate::ids::Handle,
) -> crate::ImResult<()> {
    let expires_at = OffsetDateTime::parse(&metadata.expires_at, &Rfc3339)
        .map_err(|_| response_mismatch("verify"))?;
    if metadata.service_origin != origin
        || &metadata.controller_handle != expected_controller
        || &metadata.agent_handle != expected_agent
        || expires_at <= OffsetDateTime::now_utc()
        || did_domain(metadata.controller_did.as_str())
            != Some(core.inner().sdk_config().did_domain.as_str())
    {
        return Err(onboarding_error(
            "skill_onboarding_scope_mismatch",
            "verify",
            false,
        ));
    }
    Ok(())
}

fn validate_exchange(
    journal: &SkillClaimJournal,
    pending: &PendingIdentityBundle,
    result: &SkillExchangeResult,
) -> crate::ImResult<()> {
    let binding_generation = anp::wns::BindingGeneration::new(result.binding_generation.clone())
        .map_err(|_| response_mismatch("exchange"))?;
    if result.token_id != journal.token_id
        || result.did != journal.agent_did
        || result.controller_did != journal.controller_did
        || result.controller_handle != journal.controller_full_handle
        || result.agent_handle != journal.agent_handle
        || result.status != "registered"
        || result.user_id.trim().is_empty()
        || result.controller_user_id.trim().is_empty()
        || result.controller_user_id == result.user_id
    {
        return Err(response_mismatch("exchange"));
    }
    if binding_generation.to_string() != result.binding_generation {
        return Err(response_mismatch("exchange"));
    }
    crate::internal::access_token::validate_device_access_token(
        &result.access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: pending.generated.did.as_str(),
            user_id: &result.user_id,
            device_id: pending.generated.protocol_device_id.as_str(),
            key_id: &pending.generated.device_signing_key_id,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )
    .map_err(|_| response_mismatch("exchange"))?;
    Ok(())
}

fn validate_legacy_exchange(
    journal: &LegacySkillClaimJournal,
    result: &LegacySkillExchangeResult,
) -> crate::ImResult<()> {
    if result.token_id != journal.token_id
        || result.did != journal.agent_did
        || result.controller_did != journal.controller_did
        || result.controller_handle != journal.controller_full_handle
        || result.agent_handle != journal.agent_handle
        || result.status != "registered"
        || result.user_id.trim().is_empty()
        || result
            .controller_user_id
            .as_deref()
            .is_some_and(|controller| controller == result.user_id)
    {
        return Err(response_mismatch("legacy_exchange"));
    }
    Ok(())
}

fn metadata_from_journal(journal: &SkillClaimJournal) -> SkillTokenMetadata {
    SkillTokenMetadata {
        token_id: journal.token_id.clone(),
        service_origin: journal.service_origin.clone(),
        controller_did: journal.controller_did.clone(),
        controller_handle: journal.controller_full_handle.clone(),
        agent_handle: journal.agent_handle.clone(),
        expires_at: String::new(),
    }
}

fn journal_from_pending(
    metadata: &SkillTokenMetadata,
    local_alias: String,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<SkillClaimJournal> {
    Ok(SkillClaimJournal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        token_id: metadata.token_id.clone(),
        service_origin: metadata.service_origin.clone(),
        controller_did: metadata.controller_did.clone(),
        controller_full_handle: metadata.controller_handle.clone(),
        agent_handle: metadata.agent_handle.clone(),
        agent_did: pending.generated.did.clone(),
        local_alias,
        did_document_digest: pending.document_hash.clone(),
        phase: JournalPhase::IdentityPending,
        greeting_message_id: greeting_message_id(&metadata.token_id)?,
        last_error_code: None,
        updated_at: now_rfc3339()?,
    })
}

fn legacy_metadata_from_journal(journal: &LegacySkillClaimJournal) -> SkillTokenMetadata {
    SkillTokenMetadata {
        token_id: journal.token_id.clone(),
        service_origin: journal.service_origin.clone(),
        controller_did: journal.controller_did.clone(),
        controller_handle: journal.controller_full_handle.clone(),
        agent_handle: journal.agent_handle.clone(),
        expires_at: String::new(),
    }
}

fn validate_journal_request(
    journal: &SkillClaimJournal,
    origin: &str,
    controller: &crate::ids::Handle,
    agent: &crate::ids::Handle,
) -> crate::ImResult<()> {
    if journal.schema_version != JOURNAL_SCHEMA_VERSION
        || journal.service_origin != origin
        || &journal.controller_full_handle != controller
        || &journal.agent_handle != agent
    {
        return Err(workspace_conflict());
    }
    Ok(())
}

fn validate_legacy_journal_request(
    journal: &LegacySkillClaimJournal,
    origin: &str,
    controller: &crate::ids::Handle,
    agent: &crate::ids::Handle,
) -> crate::ImResult<()> {
    if journal.schema_version != 1
        || journal.service_origin != origin
        || &journal.controller_full_handle != controller
        || &journal.agent_handle != agent
        || journal.token_id.trim().is_empty()
    {
        return Err(workspace_conflict());
    }
    Ok(())
}

enum ReadyIdentityState {
    Empty,
    Matching,
    Conflict,
}

fn matching_ready_identity(
    identities: &[crate::identity::IdentitySummary],
    journal: &SkillClaimJournal,
) -> ReadyIdentityState {
    if identities.is_empty() {
        return ReadyIdentityState::Empty;
    }
    if identities.len() == 1
        && identities[0].did == journal.agent_did
        && identities[0].local_alias.as_deref() == Some(journal.local_alias.as_str())
    {
        ReadyIdentityState::Matching
    } else {
        ReadyIdentityState::Conflict
    }
}

fn matching_legacy_identity(
    identities: &[crate::identity::IdentitySummary],
    journal: &LegacySkillClaimJournal,
) -> ReadyIdentityState {
    if identities.is_empty() {
        return ReadyIdentityState::Empty;
    }
    if identities.len() == 1
        && identities[0].did == journal.agent_did
        && identities[0].local_alias.as_deref() == Some(journal.local_alias.as_str())
    {
        ReadyIdentityState::Matching
    } else {
        ReadyIdentityState::Conflict
    }
}

async fn validate_matching_vnext_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
) -> crate::ImResult<()> {
    let document = crate::internal::identity_document_cache::load_local_did_document_async(
        &core.inner().sdk_paths().identities,
        journal.agent_did.as_str(),
    )
    .await
    .map_err(|_| workspace_conflict())?
    .ok_or_else(workspace_conflict)?;
    let document_hash = crate::internal::identity_wire::document::document_hash(&document)
        .map_err(|_| workspace_conflict())?;
    if document_hash != journal.did_document_digest
        || anp::authentication::validate_device_manifest(&document)
            .map_err(|_| workspace_conflict())?
            .is_none()
    {
        return Err(workspace_conflict());
    }
    crate::core::validate_handle_service_for_did(
        &document,
        &journal.agent_did,
        &journal.agent_handle,
    )
    .map_err(|_| workspace_conflict())
}

fn validate_pending_identity(
    journal: &SkillClaimJournal,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<()> {
    if pending.generated.did != journal.agent_did
        || crate::internal::identity_wire::document::document_hash(&pending.generated.did_document)?
            != journal.did_document_digest
        || pending.document_hash != journal.did_document_digest
    {
        return Err(onboarding_error(
            "skill_onboarding_workspace_conflict",
            "pending_identity",
            false,
        ));
    }
    Ok(())
}

fn validate_pending_identity_material(
    journal: &SkillClaimJournal,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<()> {
    let generated = &pending.generated;
    let expected_unique_id = generated
        .did
        .as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(workspace_conflict)?;
    let manifest = anp::authentication::validate_device_manifest(&generated.did_document)
        .map_err(|_| workspace_conflict())?
        .ok_or_else(workspace_conflict)?;
    let matching_devices = manifest
        .devices
        .iter()
        .filter(|device| device.device_id == generated.protocol_device_id.as_str())
        .collect::<Vec<_>>();
    if generated.unique_id != expected_unique_id
        || generated.did_document.get("id").and_then(Value::as_str) != Some(generated.did.as_str())
        || !anp::authentication::validate_did_document_binding(&generated.did_document, true)
        || matching_devices.len() != 1
        || matching_devices[0].signing_key_id != generated.device_signing_key_id
        || matching_devices[0].e2ee_key_id != generated.device_e2ee_key_id
        || generated.root_key_id != format!("{}#key-1", generated.did.as_str())
    {
        return Err(workspace_conflict());
    }
    crate::core::validate_handle_service_for_did(
        &generated.did_document,
        &generated.did,
        &journal.agent_handle,
    )
    .map_err(|_| workspace_conflict())?;
    validate_legacy_private_key(
        &generated.did_document,
        &generated.root_key_id,
        &generated.root_private_pem,
        LegacyPrivateKeyRole::Signing,
    )?;
    validate_legacy_private_key(
        &generated.did_document,
        &generated.device_signing_key_id,
        &generated.device_signing_private_pem,
        LegacyPrivateKeyRole::Signing,
    )?;
    validate_legacy_private_key(
        &generated.did_document,
        &generated.device_e2ee_key_id,
        &generated.device_e2ee_private_pem,
        LegacyPrivateKeyRole::Agreement,
    )?;
    let root_private = anp::PrivateKeyMaterial::from_pem(&generated.root_private_pem)
        .map_err(|_| workspace_conflict())?;
    let signing_private = anp::PrivateKeyMaterial::from_pem(&generated.device_signing_private_pem)
        .map_err(|_| workspace_conflict())?;
    let e2ee_private = anp::PrivateKeyMaterial::from_pem(&generated.device_e2ee_private_pem)
        .map_err(|_| workspace_conflict())?;
    if root_private.public_key().to_pem() != generated.root_public_pem
        || signing_private.public_key().to_pem() != generated.device_signing_public_pem
        || e2ee_private.public_key().to_pem() != generated.device_e2ee_public_pem
    {
        return Err(workspace_conflict());
    }
    Ok(())
}

fn validate_legacy_pending_identity(
    journal: &LegacySkillClaimJournal,
    pending: &LegacyPendingIdentityBundle,
) -> crate::ImResult<()> {
    let did_suffix = pending
        .did
        .as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(workspace_conflict)?;
    if pending.did != journal.agent_did
        || pending.unique_id != did_suffix
        || pending.did_document.get("id").and_then(Value::as_str)
            != Some(journal.agent_did.as_str())
        || pending.did_document.get("deviceManifest").is_some()
        || document_digest(&pending.did_document)? != journal.did_document_digest
        || !anp::authentication::validate_did_document_binding(&pending.did_document, true)
    {
        return Err(workspace_conflict());
    }
    crate::core::validate_handle_service_for_did(
        &pending.did_document,
        &pending.did,
        &journal.agent_handle,
    )?;
    validate_legacy_private_key(
        &pending.did_document,
        &format!("{}#key-1", pending.did.as_str()),
        &pending.key1_private_pem,
        LegacyPrivateKeyRole::Signing,
    )?;
    validate_legacy_private_key(
        &pending.did_document,
        &format!("{}#key-2", pending.did.as_str()),
        &pending.e2ee_signing_private_pem,
        LegacyPrivateKeyRole::Signing,
    )?;
    validate_legacy_private_key(
        &pending.did_document,
        &format!("{}#key-3", pending.did.as_str()),
        &pending.e2ee_agreement_private_pem,
        LegacyPrivateKeyRole::Agreement,
    )?;
    let root_private = anp::PrivateKeyMaterial::from_pem(&pending.key1_private_pem)
        .map_err(|_| workspace_conflict())?;
    if root_private.public_key().to_pem() != pending.key1_public_pem {
        return Err(workspace_conflict());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum LegacyPrivateKeyRole {
    Signing,
    Agreement,
}

fn validate_legacy_private_key(
    document: &Value,
    key_id: &str,
    private_key_pem: &str,
    role: LegacyPrivateKeyRole,
) -> crate::ImResult<()> {
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or_else(workspace_conflict)?;
    let mut matching = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id));
    let method = matching.next().ok_or_else(workspace_conflict)?;
    if matching.next().is_some() {
        return Err(workspace_conflict());
    }
    let private =
        anp::PrivateKeyMaterial::from_pem(private_key_pem).map_err(|_| workspace_conflict())?;
    let role_matches = match role {
        LegacyPrivateKeyRole::Signing => {
            matches!(&private, anp::PrivateKeyMaterial::Ed25519(_))
        }
        LegacyPrivateKeyRole::Agreement => {
            matches!(&private, anp::PrivateKeyMaterial::X25519(_))
        }
    };
    let public = crate::internal::identity_wire::document::extract_identity_public_key(method)?;
    if !role_matches || private.public_key().to_pem() != public.to_pem() {
        return Err(workspace_conflict());
    }
    Ok(())
}

fn validated_service_origin(core: &crate::core::ImCore, input: &str) -> crate::ImResult<String> {
    let url = reqwest::Url::parse(input.trim()).map_err(|_| {
        onboarding_error("skill_onboarding_scope_mismatch", "service_origin", false)
    })?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.host_str() != Some(core.inner().sdk_config().did_domain.as_str())
    {
        return Err(onboarding_error(
            "skill_onboarding_scope_mismatch",
            "service_origin",
            false,
        ));
    }
    let configured = core
        .inner()
        .sdk_config()
        .user_service_endpoint
        .as_ref()
        .unwrap_or(&core.inner().sdk_config().service_base_url);
    let configured_url = reqwest::Url::parse(configured.as_str()).map_err(|_| {
        onboarding_error("skill_onboarding_scope_mismatch", "service_origin", false)
    })?;
    let origin = url.origin().ascii_serialization();
    if configured_url.origin().ascii_serialization() != origin {
        return Err(onboarding_error(
            "skill_onboarding_scope_mismatch",
            "service_origin",
            false,
        ));
    }
    Ok(origin)
}

fn validated_full_handle(
    input: &str,
    domain: &str,
    phase: &str,
) -> crate::ImResult<crate::ids::Handle> {
    let handle = crate::ids::Handle::parse(input, domain)?;
    if !handle.as_str().ends_with(&format!(".{domain}")) {
        return Err(onboarding_error(
            "skill_onboarding_scope_mismatch",
            phase,
            false,
        ));
    }
    Ok(handle)
}

fn handle_local_part(
    handle: &crate::ids::Handle,
    core: &crate::core::ImCore,
) -> crate::ImResult<String> {
    let suffix = format!(".{}", core.inner().sdk_config().did_domain);
    let local = handle
        .as_str()
        .strip_suffix(&suffix)
        .filter(|value| !value.is_empty() && !value.contains('.'))
        .ok_or_else(|| {
            onboarding_error("skill_onboarding_scope_mismatch", "agent_handle", false)
        })?;
    Ok(local.to_owned())
}

fn ensure_workspace_initialized(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let paths = core.inner().sdk_paths();
    let initialized = paths
        .identities
        .identity_root_dir
        .parent()
        .is_some_and(Path::exists)
        && paths
            .local_state
            .sqlite_path
            .parent()
            .is_some_and(Path::exists);
    if !initialized {
        return Err(onboarding_error(
            "skill_onboarding_workspace_conflict",
            "workspace_check",
            false,
        ));
    }
    Ok(())
}

fn ensure_no_legacy_artifacts(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let identity_root = &core.inner().sdk_paths().identities.identity_root_dir;
    let legacy_journal = identity_root.join(LEGACY_JOURNAL_FILE_NAME);
    let legacy_pending = identity_root.join(LEGACY_PENDING_DIR_NAME);
    let legacy_files_exist = legacy_journal.exists()
        || (legacy_pending.exists()
            && fs::read_dir(&legacy_pending)
                .map_err(|_| workspace_conflict())?
                .next()
                .is_some());
    let legacy_vault_secret_exists =
        match crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)? {
            crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => false,
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault { vault, .. } => {
                vault
                    .list()?
                    .iter()
                    .any(|secret| secret.key_id.starts_with(LEGACY_PENDING_SECRET_KEY_PREFIX))
            }
        };
    if legacy_files_exist || legacy_vault_secret_exists {
        return Err(onboarding_error(
            "skill_onboarding_legacy_claim_recovery_required",
            "legacy_artifacts",
            false,
        ));
    }
    Ok(())
}

fn read_legacy_journal(
    core: &crate::core::ImCore,
) -> crate::ImResult<Option<LegacySkillClaimJournal>> {
    let raw = match fs::read(legacy_journal_path(core)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(legacy_operator_reconciliation_required("legacy_journal")),
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|_| legacy_operator_reconciliation_required("legacy_journal"))
}

fn write_legacy_journal(
    core: &crate::core::ImCore,
    journal: &LegacySkillClaimJournal,
) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(journal)
        .map_err(|_| legacy_operator_reconciliation_required("legacy_journal"))?;
    write_atomic_secure(&legacy_journal_path(core), &raw)
        .map_err(|_| legacy_operator_reconciliation_required("legacy_journal"))
}

fn load_legacy_pending_identity(
    core: &crate::core::ImCore,
    journal: &LegacySkillClaimJournal,
) -> crate::ImResult<LegacyPendingIdentityBundle> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)
            .map_err(|_| legacy_operator_reconciliation_required("legacy_pending_identity"))?;
    let encoded = match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => Zeroizing::new(
            fs::read(legacy_pending_file(core, &journal.token_id))
                .map_err(|_| legacy_operator_reconciliation_required("legacy_pending_identity"))?,
        ),
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => Zeroizing::new(
            vault
                .open(&legacy_pending_secret_ref(
                    &workspace_id,
                    &device_id,
                    journal,
                ))
                .map_err(|_| legacy_operator_reconciliation_required("legacy_pending_identity"))?
                .expose_secret()
                .to_vec(),
        ),
    };
    serde_json::from_slice(&encoded)
        .map_err(|_| legacy_operator_reconciliation_required("legacy_pending_identity"))
}

fn delete_legacy_pending_identity(
    core: &crate::core::ImCore,
    journal: &LegacySkillClaimJournal,
) -> crate::ImResult<()> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)
            .map_err(|_| legacy_operator_reconciliation_required("legacy_cleanup"))?;
    match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            match fs::remove_file(legacy_pending_file(core, &journal.token_id)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(legacy_operator_reconciliation_required("legacy_cleanup")),
            }
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => vault
            .delete(&legacy_pending_secret_ref(
                &workspace_id,
                &device_id,
                journal,
            ))
            .map_err(|_| legacy_operator_reconciliation_required("legacy_cleanup")),
    }
}

fn cleanup_legacy_artifacts_after_v2_commit(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let Some(journal) = read_legacy_journal(core)? else {
        if has_legacy_pending_artifacts(core)? {
            return Err(legacy_operator_reconciliation_required("legacy_cleanup"));
        }
        return Ok(());
    };
    delete_legacy_pending_identity(core, &journal)?;
    match fs::remove_file(legacy_journal_path(core)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(legacy_operator_reconciliation_required("legacy_cleanup")),
    }
    let pending_dir = legacy_pending_dir(core);
    if pending_dir.exists()
        && fs::read_dir(&pending_dir)
            .map_err(|_| legacy_operator_reconciliation_required("legacy_cleanup"))?
            .next()
            .is_none()
    {
        fs::remove_dir(&pending_dir)
            .map_err(|_| legacy_operator_reconciliation_required("legacy_cleanup"))?;
    }
    if has_legacy_pending_artifacts(core)? {
        return Err(legacy_operator_reconciliation_required("legacy_cleanup"));
    }
    Ok(())
}

fn has_legacy_pending_artifacts(core: &crate::core::ImCore) -> crate::ImResult<bool> {
    let pending_files_exist = if legacy_pending_dir(core).exists() {
        fs::read_dir(legacy_pending_dir(core))
            .map_err(|_| legacy_operator_reconciliation_required("legacy_artifacts"))?
            .next()
            .is_some()
    } else {
        false
    };
    let pending_vault_secret_exists =
        match crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)
            .map_err(|_| legacy_operator_reconciliation_required("legacy_artifacts"))?
        {
            crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => false,
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault { vault, .. } => {
                vault
                    .list()
                    .map_err(|_| legacy_operator_reconciliation_required("legacy_artifacts"))?
                    .iter()
                    .any(|secret| secret.key_id.starts_with(LEGACY_PENDING_SECRET_KEY_PREFIX))
            }
        };
    Ok(pending_files_exist || pending_vault_secret_exists)
}

fn load_orphan_pending_identity(
    core: &crate::core::ImCore,
    token_id: &str,
    local_alias: &str,
) -> crate::ImResult<Option<PendingIdentityBundle>> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let encoded = match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            let dir = pending_dir(core);
            if !dir.exists() {
                return Ok(None);
            }
            let expected = pending_file(core, token_id);
            let pending_files = fs::read_dir(&dir)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
                .collect::<Vec<_>>();
            if pending_files.is_empty() {
                return Ok(None);
            }
            if pending_files.len() != 1 || pending_files[0] != expected {
                return Err(workspace_conflict());
            }
            Zeroizing::new(fs::read(expected).map_err(|_| workspace_conflict())?)
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => {
            let expected_key_id =
                format!("{PENDING_SECRET_KEY_PREFIX}{}", token_id_digest(token_id));
            let pending_refs = vault
                .list()?
                .into_iter()
                .filter(|secret| secret.key_id.starts_with(PENDING_SECRET_KEY_PREFIX))
                .collect::<Vec<_>>();
            if pending_refs.is_empty() {
                return Ok(None);
            }
            if pending_refs.len() != 1 {
                return Err(workspace_conflict());
            }
            let secret_ref = &pending_refs[0];
            if secret_ref.workspace_id != workspace_id
                || secret_ref.device_id != device_id
                || secret_ref.identity_id.as_deref() != Some(local_alias)
                || secret_ref.kind != crate::internal::secret_vault::SecretKind::RuntimeSecret
                || secret_ref.key_id != expected_key_id
                || secret_ref.key_version != 1
            {
                return Err(workspace_conflict());
            }
            let opened = vault.open(secret_ref).map_err(|_| workspace_conflict())?;
            let pending: PendingIdentityBundle =
                serde_json::from_slice(opened.expose_secret()).map_err(|_| workspace_conflict())?;
            if secret_ref.did.as_deref() != Some(pending.generated.did.as_str()) {
                return Err(workspace_conflict());
            }
            return Ok(Some(pending));
        }
    };
    serde_json::from_slice(&encoded)
        .map(Some)
        .map_err(|_| workspace_conflict())
}

fn pending_identity_exists(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
) -> crate::ImResult<bool> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            Ok(pending_file(core, &journal.token_id).exists())
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => {
            let expected = pending_secret_ref(&workspace_id, &device_id, journal);
            Ok(vault.list()?.iter().any(|secret| secret == &expected))
        }
    }
}

fn save_pending_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<()> {
    let encoded = Zeroizing::new(serde_json::to_vec(pending).map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "pending_identity",
            true,
        )
    })?);
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            fs::create_dir_all(pending_dir(core))?;
            write_new_atomic_secure(&pending_file(core, &journal.token_id), &encoded)
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => {
            let expected = pending_secret_ref(&workspace_id, &device_id, journal);
            let sealed = vault
                .seal_if_absent(crate::internal::secret_vault::SealSecretRequest {
                metadata: crate::internal::secret_vault::SecretMetadata {
                    workspace_id,
                    device_id,
                    identity_id: Some(journal.local_alias.clone()),
                    did: Some(journal.agent_did.as_str().to_owned()),
                    kind: crate::internal::secret_vault::SecretKind::RuntimeSecret,
                    key_id: expected.key_id.clone(),
                    key_version: 1,
                    policy:
                        crate::internal::secret_vault::SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: crate::internal::platform_secret::SecretBytes::from_vec(
                    encoded.to_vec(),
                ),
            })?;
            let actual = match sealed {
                crate::internal::secret_vault::SealIfAbsentResult::Sealed(secret_ref)
                | crate::internal::secret_vault::SealIfAbsentResult::AlreadyExists(secret_ref) => {
                    secret_ref
                }
            };
            if actual != expected
                || vault
                    .open(&actual)
                    .map(|opened| opened.expose_secret() != encoded.as_slice())
                    .unwrap_or(true)
            {
                return Err(onboarding_error(
                    "skill_onboarding_local_commit_failed",
                    "pending_identity",
                    true,
                ));
            }
            Ok(())
        }
    }
    .map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "pending_identity",
            true,
        )
    })
}

fn load_pending_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
) -> crate::ImResult<PendingIdentityBundle> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let encoded = match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            Zeroizing::new(fs::read(pending_file(core, &journal.token_id))?)
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => Zeroizing::new(
            vault
                .open(&pending_secret_ref(&workspace_id, &device_id, journal))?
                .expose_secret()
                .to_vec(),
        ),
    };
    serde_json::from_slice(&encoded).map_err(|_| {
        onboarding_error(
            "skill_onboarding_workspace_conflict",
            "pending_identity",
            false,
        )
    })
}

fn delete_pending_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
) -> crate::ImResult<()> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            match fs::remove_file(pending_file(core, &journal.token_id)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => vault.delete(&pending_secret_ref(&workspace_id, &device_id, journal)),
    }
}

fn pending_secret_ref(
    workspace_id: &str,
    device_id: &str,
    journal: &SkillClaimJournal,
) -> crate::internal::secret_vault::SecretRef {
    crate::internal::secret_vault::SecretRef {
        workspace_id: workspace_id.to_owned(),
        device_id: device_id.to_owned(),
        identity_id: Some(journal.local_alias.clone()),
        did: Some(journal.agent_did.as_str().to_owned()),
        kind: crate::internal::secret_vault::SecretKind::RuntimeSecret,
        key_id: format!(
            "{PENDING_SECRET_KEY_PREFIX}{}",
            token_id_digest(&journal.token_id)
        ),
        key_version: 1,
    }
}

fn journal_path(core: &crate::core::ImCore) -> PathBuf {
    core.inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(JOURNAL_FILE_NAME)
}

fn pending_dir(core: &crate::core::ImCore) -> PathBuf {
    core.inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(PENDING_DIR_NAME)
}

fn pending_file(core: &crate::core::ImCore, token_id: &str) -> PathBuf {
    pending_dir(core).join(format!("{}.json", token_id_digest(token_id)))
}

fn legacy_journal_path(core: &crate::core::ImCore) -> PathBuf {
    core.inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(LEGACY_JOURNAL_FILE_NAME)
}

fn legacy_pending_dir(core: &crate::core::ImCore) -> PathBuf {
    core.inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(LEGACY_PENDING_DIR_NAME)
}

fn legacy_pending_file(core: &crate::core::ImCore, token_id: &str) -> PathBuf {
    legacy_pending_dir(core).join(format!("{}.json", token_id_digest(token_id)))
}

fn legacy_pending_secret_ref(
    workspace_id: &str,
    device_id: &str,
    journal: &LegacySkillClaimJournal,
) -> crate::internal::secret_vault::SecretRef {
    crate::internal::secret_vault::SecretRef {
        workspace_id: workspace_id.to_owned(),
        device_id: device_id.to_owned(),
        identity_id: Some(journal.local_alias.clone()),
        did: Some(journal.agent_did.as_str().to_owned()),
        kind: crate::internal::secret_vault::SecretKind::RuntimeSecret,
        key_id: format!(
            "{LEGACY_PENDING_SECRET_KEY_PREFIX}{}",
            token_id_digest(&journal.token_id)
        ),
        key_version: 1,
    }
}

fn read_journal(path: &Path) -> crate::ImResult<Option<SkillClaimJournal>> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(workspace_conflict()),
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|_| workspace_conflict())
}

fn read_initial_journal(path: &Path) -> crate::ImResult<InitialJournalState> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InitialJournalState::Missing)
        }
        Err(_) => return Err(workspace_conflict()),
    };
    Ok(match serde_json::from_slice(&raw) {
        Ok(journal) => InitialJournalState::Valid(journal),
        Err(_) => InitialJournalState::Corrupt,
    })
}

fn create_journal(path: &Path, journal: &SkillClaimJournal) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(journal).map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "journal_write",
            true,
        )
    })?;
    write_new_atomic_secure(path, &raw).map_err(|_| workspace_conflict())
}

fn write_journal(path: &Path, journal: &SkillClaimJournal) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(journal).map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "journal_write",
            true,
        )
    })?;
    write_atomic_secure(path, &raw).map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "journal_write",
            true,
        )
    })
}

fn write_new_secure(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(raw)?;
    file.sync_all()?;
    set_private_file_mode(path)?;
    Ok(())
}

/// Publishes a fully synced file without ever exposing a partial final path or
/// replacing a concurrently-created record. The hard link is the publish point.
fn write_new_atomic_secure(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "skill_onboarding_file".to_owned(),
            detail: "path has no parent".to_owned(),
        })?;
    fs::create_dir_all(parent)?;
    let stale_temp = initial_write_temp_path(path);
    match fs::remove_file(&stale_temp) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temp = unique_initial_write_temp_path(path);
    write_new_secure(&temp, raw)?;
    let publish = fs::hard_link(&temp, path).map_err(crate::ImError::from);
    if publish.is_ok() {
        set_private_file_mode(path)?;
        sync_directory(parent);
    }
    let _ = fs::remove_file(&temp);
    publish
}

fn initial_write_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill-onboarding");
    path.with_file_name(format!(".{file_name}.initial.tmp"))
}

fn unique_initial_write_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill-onboarding");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{file_name}.initial.{}.{}.tmp",
        std::process::id(),
        nonce
    ))
}

fn write_atomic_secure(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    let temp = path.with_extension("tmp");
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    write_new_secure(&temp, raw)?;
    fs::rename(temp, path)?;
    set_private_file_mode(path)?;
    if let Some(parent) = path.parent() {
        sync_directory(parent);
    }
    Ok(())
}

fn sync_directory(path: &Path) {
    if let Ok(directory) = fs::File::open(path) {
        let _ = directory.sync_all();
    }
}

fn set_private_file_mode(path: &Path) -> crate::ImResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn claim_result(
    journal: &SkillClaimJournal,
    status: crate::onboarding::SkillClaimStatus,
    retryable: bool,
) -> crate::ImResult<crate::onboarding::SkillClaimResult> {
    Ok(crate::onboarding::SkillClaimResult {
        phase: if status == crate::onboarding::SkillClaimStatus::Completed {
            crate::onboarding::SkillClaimPhase::Completed
        } else {
            crate::onboarding::SkillClaimPhase::ControllerGreetingPending
        },
        status,
        agent_did: journal.agent_did.clone(),
        agent_handle: journal.agent_handle.clone(),
        controller_handle: journal.controller_full_handle.clone(),
        greeting_message_id: journal.greeting_message_id.clone(),
        retryable,
        error_code: journal.last_error_code.clone(),
    })
}

fn greeting_message_id(token_id: &str) -> crate::ImResult<crate::ids::MessageId> {
    let digest =
        Sha256::digest(format!("awiki:skill-onboarding:v1:greeting:{token_id}").as_bytes());
    crate::ids::MessageId::parse(format!(
        "skill-greeting-{}",
        hex_lower(&digest)[..32].to_owned()
    ))
}

fn document_digest(document: &Value) -> crate::ImResult<String> {
    let canonical = canonical_json(document);
    let raw = serde_json::to_vec(&canonical).map_err(|_| response_mismatch("document_digest"))?;
    Ok(hex_lower(&Sha256::digest(raw)))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn token_id_digest(token_id: &str) -> String {
    hex_lower(&Sha256::digest(token_id.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn string_field<'a>(value: &'a Value, field: &str) -> crate::ImResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| response_mismatch("response_parse"))
}

fn did_domain(did: &str) -> Option<&str> {
    let mut parts = did.split(':');
    (parts.next() == Some("did") && parts.next() == Some("wba"))
        .then(|| parts.next())
        .flatten()
}

fn now_rfc3339() -> crate::ImResult<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| onboarding_error("skill_onboarding_local_commit_failed", "clock", true))
}

fn map_remote_error(error: crate::ImError, phase: &str) -> crate::ImError {
    if let crate::ImError::Service { data, .. } = &error {
        if let Some(reason) = data
            .as_ref()
            .and_then(|data| data.get("reason"))
            .and_then(Value::as_str)
        {
            return onboarding_error(reason, phase, false);
        }
    }
    let retryable = matches!(
        error,
        crate::ImError::TransportUnavailable { .. }
            | crate::ImError::Io { .. }
            | crate::ImError::Service {
                status_code: Some(500..=599),
                ..
            }
    );
    onboarding_error(
        if retryable {
            "skill_onboarding_transport_unavailable"
        } else {
            "skill_onboarding_response_mismatch"
        },
        phase,
        retryable,
    )
}

fn map_prekey_error(error: crate::ImError) -> crate::ImError {
    let (code, retryable) = match error {
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::Io { .. }
        | crate::ImError::Service {
            status_code: Some(500..=599),
            ..
        } => ("skill_onboarding_prekey_pending", true),
        crate::ImError::IdentityRequired
        | crate::ImError::AuthRequired
        | crate::ImError::SessionExpired
        | crate::ImError::PermissionDenied => ("skill_onboarding_prekey_not_authorized", false),
        crate::ImError::IdentityVault { .. } => {
            ("skill_onboarding_prekey_vault_unavailable", false)
        }
        crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::LocalStateUpgradeRequired { .. }
        | crate::ImError::LocalStateUpgradeInProgress
        | crate::ImError::LocalStateUpgradeFailed { .. } => {
            ("skill_onboarding_prekey_local_state_unavailable", false)
        }
        _ => ("skill_onboarding_prekey_failed", false),
    };
    onboarding_error(code, "device_prekey", retryable)
}

fn stable_error_code(error: &crate::ImError) -> &str {
    match error {
        crate::ImError::SkillOnboarding { code, .. } => code,
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::Io { .. }
        | crate::ImError::Service {
            status_code: Some(500..=599),
            ..
        } => "skill_onboarding_greeting_pending",
        _ => "skill_onboarding_greeting_pending",
    }
}

fn response_mismatch(phase: &str) -> crate::ImError {
    onboarding_error("skill_onboarding_response_mismatch", phase, false)
}

fn workspace_conflict() -> crate::ImError {
    onboarding_error(
        "skill_onboarding_workspace_conflict",
        "workspace_check",
        false,
    )
}

fn legacy_operator_reconciliation_required(phase: &str) -> crate::ImError {
    onboarding_error("blocked_requires_operator_reconciliation", phase, false)
}

fn onboarding_error(code: &str, phase: &str, retryable: bool) -> crate::ImError {
    crate::ImError::SkillOnboarding {
        code: code.to_owned(),
        phase: phase.to_owned(),
        retryable,
    }
}

#[cfg(test)]
mod tests;
