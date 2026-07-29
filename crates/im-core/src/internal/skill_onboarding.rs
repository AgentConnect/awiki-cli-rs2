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

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const JOURNAL_FILE_NAME: &str = ".skill-onboarding-v1.json";
const PENDING_DIR_NAME: &str = ".skill-onboarding-v1";
const SKILL_PURPOSE: &str = "skill_onboarding_v1";
const GREETING_TEXT: &str = "AWiki Skill Agent 已完成注册，可以开始对话。";
const PENDING_SECRET_KEY_PREFIX: &str = "skill-onboarding-pending-v1-";

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
    controller_did: crate::ids::Did,
    controller_handle: crate::ids::Handle,
    agent_handle: crate::ids::Handle,
    status: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PendingIdentityBundle {
    did: crate::ids::Did,
    unique_id: String,
    did_document: Value,
    key1_private_pem: String,
    key1_public_pem: String,
    e2ee_signing_private_pem: String,
    e2ee_agreement_private_pem: String,
}

impl std::fmt::Debug for PendingIdentityBundle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingIdentityBundle")
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

impl From<crate::internal::identity_generation::GeneratedIdentity> for PendingIdentityBundle {
    fn from(value: crate::internal::identity_generation::GeneratedIdentity) -> Self {
        Self {
            did: value.did,
            unique_id: value.unique_id,
            did_document: value.did_document,
            key1_private_pem: value.key1_private_pem,
            key1_public_pem: value.key1_public_pem,
            e2ee_signing_private_pem: value.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: value.e2ee_agreement_private_pem,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalPhase {
    IdentityPending,
    ControllerGreetingPending,
    Completed,
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

    async fn authenticate(
        &mut self,
        local_alias: &str,
        metadata: &SkillTokenMetadata,
        pending: &PendingIdentityBundle,
    ) -> crate::ImResult<String>;

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
                    "did_document": pending.did_document,
                    "allow_existing_agent_did": false,
                }),
            )
            .await?;
        parse_exchange_result(&result, self.core.inner().sdk_config().did_domain.as_str())
    }

    async fn authenticate(
        &mut self,
        local_alias: &str,
        metadata: &SkillTokenMetadata,
        pending: &PendingIdentityBundle,
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

    let journal_path = journal_path(core);
    let mut journal = read_journal(&journal_path)?;
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
                if existing.phase == JournalPhase::IdentityPending {
                    existing.phase = JournalPhase::ControllerGreetingPending;
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
        if !identities.is_empty() || has_orphan_pending_secret(core)? {
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
        let generated = crate::internal::identity_generation::generate_skill_handle_identity(
            core.inner().sdk_config().did_domain.as_str(),
            &local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
        let pending = PendingIdentityBundle::from(generated);
        let digest = document_digest(&pending.did_document)?;
        let local_alias = local_part;
        let greeting_message_id = greeting_message_id(&metadata.token_id)?;
        let next = SkillClaimJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            token_id: metadata.token_id,
            service_origin: metadata.service_origin,
            controller_did: metadata.controller_did,
            controller_full_handle: metadata.controller_handle,
            agent_handle: metadata.agent_handle,
            agent_did: pending.did.clone(),
            local_alias,
            did_document_digest: digest,
            phase: JournalPhase::IdentityPending,
            greeting_message_id,
            last_error_code: None,
            updated_at: now_rfc3339()?,
        };
        save_pending_identity(core, &next, &pending)?;
        if let Err(error) = create_journal(&journal_path, &next) {
            let _ = delete_pending_identity(core, &next);
            return Err(error);
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
        validate_exchange(&journal, &exchange)?;
        let jwt = remote
            .authenticate(&journal.local_alias, &metadata, &pending)
            .await
            .map_err(|error| map_remote_error(error, "authenticate"))?;
        persist_ready_identity(core, &journal, exchange.user_id, jwt, pending).await?;
        journal.phase = JournalPhase::ControllerGreetingPending;
        journal.last_error_code = None;
        journal.updated_at = now_rfc3339()?;
        write_journal(&journal_path, &journal)?;
        let _ = delete_pending_identity(core, &journal);
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

async fn persist_ready_identity(
    core: &crate::core::ImCore,
    journal: &SkillClaimJournal,
    user_id: String,
    jwt_token: String,
    pending: PendingIdentityBundle,
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
    crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
        core.inner().sdk_paths().identities.clone(),
        crate::internal::identity_store::SaveIdentityInput {
            local_alias: journal.local_alias.clone(),
            did: pending.did,
            unique_id: pending.unique_id,
            user_id,
            display_name: "AWiki Skill Agent".to_owned(),
            handle: journal.agent_handle.as_str().to_owned(),
            full_handle: journal.agent_handle.as_str().to_owned(),
            binding_generation: None,
            jwt_token,
            did_document: Some(pending.did_document),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
            device_state: None,
            key1_private_pem: pending.key1_private_pem,
            key1_public_pem: pending.key1_public_pem,
            e2ee_signing_private_pem: pending.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: pending.e2ee_agreement_private_pem,
            daemon_subkey_package: None,
            make_default: true,
        },
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
    if string_field(result, "agent_kind")? != "skill" {
        return Err(response_mismatch("exchange"));
    }
    Ok(SkillExchangeResult {
        token_id: string_field(result, "token_id")?.to_owned(),
        did: crate::ids::Did::parse(string_field(result, "did")?)?,
        user_id: string_field(result, "user_id")?.to_owned(),
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
    result: &SkillExchangeResult,
) -> crate::ImResult<()> {
    if result.token_id != journal.token_id
        || result.did != journal.agent_did
        || result.controller_did != journal.controller_did
        || result.controller_handle != journal.controller_full_handle
        || result.agent_handle != journal.agent_handle
        || result.status != "registered"
        || result.user_id.trim().is_empty()
    {
        return Err(response_mismatch("exchange"));
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

fn validate_pending_identity(
    journal: &SkillClaimJournal,
    pending: &PendingIdentityBundle,
) -> crate::ImResult<()> {
    if pending.did != journal.agent_did
        || document_digest(&pending.did_document)? != journal.did_document_digest
    {
        return Err(onboarding_error(
            "skill_onboarding_workspace_conflict",
            "pending_identity",
            false,
        ));
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

fn has_orphan_pending_secret(core: &crate::core::ImCore) -> crate::ImResult<bool> {
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    match secret_storage {
        crate::internal::identity_store::SaveIdentitySecretStorage::FileCompat => {
            let dir = pending_dir(core);
            if !dir.exists() {
                return Ok(false);
            }
            Ok(fs::read_dir(dir)?.next().is_some())
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault { vault, .. } => {
            Ok(vault
                .list()?
                .iter()
                .any(|secret| secret.key_id.starts_with(PENDING_SECRET_KEY_PREFIX)))
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
            write_new_secure(&pending_file(core, &journal.token_id), &encoded)
        }
        crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
            workspace_id,
            device_id,
            vault,
        } => {
            let expected = pending_secret_ref(&workspace_id, &device_id, journal);
            let actual = vault.seal(crate::internal::secret_vault::SealSecretRequest {
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
            if actual != expected {
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

fn create_journal(path: &Path, journal: &SkillClaimJournal) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(journal).map_err(|_| {
        onboarding_error(
            "skill_onboarding_local_commit_failed",
            "journal_write",
            true,
        )
    })?;
    write_new_secure(path, &raw).map_err(|_| workspace_conflict())
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

fn write_atomic_secure(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    let temp = path.with_extension("tmp");
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    write_new_secure(&temp, raw)?;
    fs::rename(temp, path)?;
    set_private_file_mode(path)?;
    Ok(())
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

fn onboarding_error(code: &str, phase: &str, retryable: bool) -> crate::ImError {
    crate::ImError::SkillOnboarding {
        code: code.to_owned(),
        phase: phase.to_owned(),
        retryable,
    }
}

#[cfg(test)]
mod tests;
