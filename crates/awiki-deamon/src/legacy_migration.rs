use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{AgentDefinition, AgentIdentityRecord};
use crate::registration::{AgentLegacyUpgradeClient, AgentLegacyUpgradeRequest, DidAuthMaterial};
use crate::state::{AgentDeviceIdentityRecord, DaemonState, PendingAgentLegacyUpgradeRecord};
use crate::{DaemonConfig, ImCoreAdapter};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingLegacyUpgradeSecret {
    identity_id: String,
    did: String,
    source_did_document: Value,
    endpoint_url: Option<String>,
    target_did_document: Value,
    target_document_hash: String,
    protocol_device_id: String,
    root_key_id: String,
    root_private_key_pem: String,
    device_signing_key_id: String,
    device_signing_private_key_pem: String,
    device_e2ee_key_id: String,
    device_e2ee_private_key_pem: String,
}

impl std::fmt::Debug for PendingLegacyUpgradeSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingLegacyUpgradeSecret")
            .field("identity_id", &self.identity_id)
            .field("did", &self.did)
            .field("source_did_document", &"<redacted-did-document>")
            .field("target_did_document", &"<redacted-did-document>")
            .field("target_document_hash", &self.target_document_hash)
            .field("protocol_device_id", &self.protocol_device_id)
            .field("root_key_material", &"<redacted-private-key>")
            .field("device_signing_key_material", &"<redacted-private-key>")
            .field("device_e2ee_key_material", &"<redacted-private-key>")
            .finish()
    }
}

pub(crate) fn migrate_legacy_agent_identity<C: AgentLegacyUpgradeClient>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    client: &C,
    agent: &AgentDefinition,
) -> Result<()> {
    if let Some(active) = state.load_agent_device_identity(&agent.agent_did)? {
        if active.identity_status != "active" {
            bail!(
                "agent_device_identity_unavailable: {} is {}",
                agent.agent_did,
                active.identity_status
            );
        }
        im_core.client_for_agent_device_identity(&active)?;
        let _ = state.scrub_completed_agent_legacy_upgrade(&agent.agent_did)?;
        return Ok(());
    }

    let (pending, resumed_pending) =
        match state.load_pending_agent_legacy_upgrade(&agent.agent_did)? {
            Some(pending) => (pending, true),
            None => (
                prepare_legacy_upgrade(config, state, im_core, agent)?,
                false,
            ),
        };
    if pending.status == "blocked" {
        bail!("agent_identity_migration_blocked: {}", agent.agent_did);
    }
    let mut secret: PendingLegacyUpgradeSecret =
        serde_json::from_value(pending.secret_payload_json.clone())
            .context("open encrypted Agent Legacy upgrade pending payload")?;
    validate_pending_binding(agent, &pending, &secret)?;
    state.mark_pending_agent_legacy_upgrade_attempt(&agent.agent_did, "prepared", None)?;

    let mut remote_already_committed = None;
    if resumed_pending {
        let remote_document = match client.resolve_agent_document(&agent.agent_did) {
            Ok(document) => document,
            Err(error) => {
                state.mark_pending_agent_legacy_upgrade_attempt(
                    &agent.agent_did,
                    "retryable",
                    Some("legacy_remote_document_unresolved"),
                )?;
                return Err(error).context(
                    "same-DID Agent Legacy remote state unresolved; update_document suppressed",
                );
            }
        };
        let target = bootstrap_material_from_pending(config, agent, &secret)?;
        match im_core.reconcile_vnext_agent_legacy_upgrade(
            &secret.source_did_document,
            target,
            &remote_document,
            &secret.root_private_key_pem,
        ) {
            Ok(im_core::VNextAgentLegacyUpgradeReconciliation::TargetCommitted) => {
                let target = bootstrap_material_from_pending(config, agent, &secret)?;
                let recovered = match client.recover_committed_agent_device(im_core, &target) {
                    Ok(recovered) => recovered,
                    Err(error) => {
                        state.mark_pending_agent_legacy_upgrade_attempt(
                            &agent.agent_did,
                            "retryable",
                            Some("legacy_committed_device_session_unavailable"),
                        )?;
                        return Err(error).context(
                            "recover exact-device session for committed Agent Legacy upgrade",
                        );
                    }
                };
                state.insert_audit_event_json(
                    "agent.identity.legacy_upgrade.remote_already_committed",
                    Some(&agent.agent_did),
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "protocol_device_id": secret.protocol_device_id.clone(),
                        "update_document_replayed": false,
                    }),
                )?;
                remote_already_committed = Some(recovered);
            }
            Ok(im_core::VNextAgentLegacyUpgradeReconciliation::LegacyRebuilt { target }) => {
                apply_rebuilt_pending_target(&mut secret, &remote_document, target)?;
                state.replace_pending_agent_legacy_upgrade_payload(
                    &agent.agent_did,
                    &secret.protocol_device_id,
                    &secret.target_document_hash,
                    &serde_json::to_value(&secret)?,
                )?;
            }
            Err(error) => {
                state.mark_pending_agent_legacy_upgrade_attempt(
                    &agent.agent_did,
                    "blocked",
                    Some("legacy_remote_state_conflict"),
                )?;
                return Err(error)
                    .context("same-DID Agent Legacy remote state conflicts with pending target");
            }
        }
    }

    let update_result = if let Some(recovered) = remote_already_committed {
        Ok(recovered)
    } else {
        client.update_agent_document(AgentLegacyUpgradeRequest {
            agent_kind: agent.agent_kind,
            did_document: secret.target_did_document.clone(),
            endpoint_url: secret.endpoint_url.clone(),
            legacy_auth: DidAuthMaterial {
                did_document: secret.source_did_document.clone(),
                private_key_pem: secret.root_private_key_pem.clone(),
                bearer_token: None,
            },
        })
    };
    let response = match update_result {
        Ok(response) => response,
        Err(error) => {
            state.mark_pending_agent_legacy_upgrade_attempt(
                &agent.agent_did,
                "retryable",
                Some("legacy_update_document_failed"),
            )?;
            return Err(error).context("same-DID Agent Legacy update_document failed");
        }
    };

    let activation = (|| -> Result<AgentDeviceIdentityRecord> {
        if response.did != agent.agent_did {
            bail!("Legacy upgrade response returned a different DID");
        }
        if response.user_id.trim().is_empty() || response.user_id == agent.controller_user_id {
            bail!("Legacy upgrade response returned an invalid Agent account");
        }
        let full_handle = canonical_full_handle(config, &agent.handle)?;
        let identity = AgentDeviceIdentityRecord {
            identity_id: secret.identity_id.clone(),
            agent_did: secret.did.clone(),
            handle: full_handle.clone(),
            display_name: agent.handle.clone(),
            agent_kind: agent.agent_kind,
            account_id: response.user_id.clone(),
            full_handle,
            binding_generation: response.binding_generation.clone(),
            did_document: secret.target_did_document.clone(),
            protocol_device_id: secret.protocol_device_id.clone(),
            root_key_id: secret.root_key_id.clone(),
            root_private_key_pem: secret.root_private_key_pem.clone(),
            device_signing_key_id: secret.device_signing_key_id.clone(),
            device_signing_private_key_pem: secret.device_signing_private_key_pem.clone(),
            device_e2ee_key_id: secret.device_e2ee_key_id.clone(),
            device_e2ee_private_key_pem: secret.device_e2ee_private_key_pem.clone(),
            daemon_subkey_package_json: None,
            authorization_status: "active".to_owned(),
            role: "admin".to_owned(),
            management_ready: true,
            auth_generation: 1,
            access_token: response.access_token.clone(),
            document_version: 1,
            document_hash: secret.target_document_hash.clone(),
            registry_version: 1,
            identity_status: "active".to_owned(),
            legacy_migration_state: "completed".to_owned(),
            last_error_code: None,
        };
        identity.validate()?;
        im_core
            .client_for_agent_device_identity(&identity)
            .context("validate migrated Agent exact-device binding")?;
        Ok(identity)
    })();
    let identity = match activation {
        Ok(identity) => identity,
        Err(error) => {
            state.mark_pending_agent_legacy_upgrade_attempt(
                &agent.agent_did,
                "blocked",
                Some("legacy_device_access_binding_invalid"),
            )?;
            return Err(error).context("activate same-DID Agent Legacy upgrade");
        }
    };
    state.promote_pending_agent_legacy_upgrade(&identity)?;
    state.scrub_completed_agent_legacy_upgrade(&agent.agent_did)?;
    Ok(())
}

fn bootstrap_material_from_pending(
    config: &DaemonConfig,
    agent: &AgentDefinition,
    secret: &PendingLegacyUpgradeSecret,
) -> Result<im_core::VNextAgentBootstrapMaterial> {
    let root_private = anp::PrivateKeyMaterial::from_pem(&secret.root_private_key_pem)
        .context("parse pending Agent Legacy root private key")?;
    let signing_private = anp::PrivateKeyMaterial::from_pem(&secret.device_signing_private_key_pem)
        .context("parse pending Agent Legacy device signing private key")?;
    let e2ee_private = anp::PrivateKeyMaterial::from_pem(&secret.device_e2ee_private_key_pem)
        .context("parse pending Agent Legacy device E2EE private key")?;
    Ok(im_core::VNextAgentBootstrapMaterial {
        kind: match agent.agent_kind {
            crate::agent::AgentKind::Daemon => im_core::AgentIdentityKind::Daemon,
            crate::agent::AgentKind::Runtime => im_core::AgentIdentityKind::Runtime,
        },
        handle_local_part: canonical_local_handle(config, &agent.handle)?,
        identity_id: secret.identity_id.clone(),
        did: im_core::ids::Did::parse(&secret.did)?,
        did_document: secret.target_did_document.clone(),
        document_hash: secret.target_document_hash.clone(),
        protocol_device_id: im_core::ids::ProtocolDeviceId::parse(&secret.protocol_device_id)?,
        root_key_id: secret.root_key_id.clone(),
        root_private_key_pem: secret.root_private_key_pem.clone(),
        root_public_key_pem: root_private.public_key().to_pem(),
        device_signing_key_id: secret.device_signing_key_id.clone(),
        device_signing_private_key_pem: secret.device_signing_private_key_pem.clone(),
        device_signing_public_key_pem: signing_private.public_key().to_pem(),
        device_e2ee_key_id: secret.device_e2ee_key_id.clone(),
        device_e2ee_private_key_pem: secret.device_e2ee_private_key_pem.clone(),
        device_e2ee_public_key_pem: e2ee_private.public_key().to_pem(),
        daemon_subkey_package: None,
    })
}

fn apply_rebuilt_pending_target(
    secret: &mut PendingLegacyUpgradeSecret,
    remote_legacy_document: &Value,
    target: im_core::VNextAgentBootstrapMaterial,
) -> Result<()> {
    if target.did.as_str() != secret.did
        || target.identity_id != secret.identity_id
        || target.protocol_device_id.as_str() != secret.protocol_device_id
        || target.root_key_id != secret.root_key_id
        || target.root_private_key_pem != secret.root_private_key_pem
        || target.device_signing_key_id != secret.device_signing_key_id
        || target.device_signing_private_key_pem != secret.device_signing_private_key_pem
        || target.device_e2ee_key_id != secret.device_e2ee_key_id
        || target.device_e2ee_private_key_pem != secret.device_e2ee_private_key_pem
    {
        bail!("refreshed Agent Legacy upgrade changed exact bootstrap device material");
    }
    secret.source_did_document = remote_legacy_document.clone();
    secret.target_did_document = target.did_document;
    secret.target_document_hash = target.document_hash;
    Ok(())
}

fn prepare_legacy_upgrade(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    agent: &AgentDefinition,
) -> Result<PendingAgentLegacyUpgradeRecord> {
    let legacy = match state.load_agent_identity(&agent.agent_did) {
        Ok(legacy) => legacy,
        Err(error) => {
            state.record_agent_identity_migration_required(
                &agent.agent_did,
                "legacy_root_or_document_unavailable",
            )?;
            return Err(error).context(format!(
                "agent_identity_migration_required: {} Legacy root/document unavailable",
                agent.agent_did
            ));
        }
    };
    if let Err(error) = validate_legacy_source(agent, &legacy) {
        state.record_agent_identity_migration_required(
            &agent.agent_did,
            "legacy_identity_binding_invalid",
        )?;
        return Err(error).context(format!(
            "agent_identity_migration_required: {} Legacy identity binding invalid",
            agent.agent_did
        ));
    }
    let local_handle = canonical_local_handle(config, &agent.handle)?;
    let generated = match im_core.prepare_vnext_agent_legacy_upgrade(
        agent.agent_kind,
        &local_handle,
        legacy.did_document.clone(),
        legacy.auth_private_key_pem.clone(),
    ) {
        Ok(generated) => generated,
        Err(error) => {
            state.record_agent_identity_migration_required(
                &agent.agent_did,
                "legacy_identity_invalid",
            )?;
            return Err(error).context(format!(
                "agent_identity_migration_required: {} Legacy identity invalid",
                agent.agent_did
            ));
        }
    };
    if generated.did.as_str() != agent.agent_did {
        state.record_agent_identity_migration_required(
            &agent.agent_did,
            "legacy_same_did_invariant_failed",
        )?;
        bail!("Agent Legacy upgrade must preserve the exact DID");
    }
    let secret = PendingLegacyUpgradeSecret {
        identity_id: generated.identity_id,
        did: generated.did.as_str().to_owned(),
        source_did_document: legacy.did_document,
        endpoint_url: legacy.endpoint_url,
        target_did_document: generated.did_document,
        target_document_hash: generated.document_hash,
        protocol_device_id: generated.protocol_device_id.as_str().to_owned(),
        root_key_id: generated.root_key_id,
        root_private_key_pem: generated.root_private_key_pem,
        device_signing_key_id: generated.device_signing_key_id,
        device_signing_private_key_pem: generated.device_signing_private_key_pem,
        device_e2ee_key_id: generated.device_e2ee_key_id,
        device_e2ee_private_key_pem: generated.device_e2ee_private_key_pem,
    };
    let record = PendingAgentLegacyUpgradeRecord {
        agent_did: secret.did.clone(),
        agent_kind: agent.agent_kind,
        protocol_device_id: secret.protocol_device_id.clone(),
        target_document_hash: secret.target_document_hash.clone(),
        secret_payload_json: serde_json::to_value(&secret)?,
        status: "prepared".to_owned(),
        attempt_count: 0,
        last_error_code: None,
        updated_at_ms: crate::security::runtime_token::current_time_millis()?,
    };
    state.store_pending_agent_legacy_upgrade(&record)?;
    Ok(record)
}

fn validate_legacy_source(agent: &AgentDefinition, legacy: &AgentIdentityRecord) -> Result<()> {
    if legacy.agent_did != agent.agent_did || legacy.agent_kind != agent.agent_kind {
        bail!("Legacy Agent identity does not match its public definition");
    }
    if legacy.auth_private_key_pem.trim().is_empty() || !legacy.did_document.is_object() {
        bail!("Legacy Agent identity is missing its root key or DID document");
    }
    Ok(())
}

fn validate_pending_binding(
    agent: &AgentDefinition,
    pending: &PendingAgentLegacyUpgradeRecord,
    secret: &PendingLegacyUpgradeSecret,
) -> Result<()> {
    if pending.agent_kind != agent.agent_kind
        || pending.agent_did != agent.agent_did
        || pending.agent_did != secret.did
        || pending.protocol_device_id != secret.protocol_device_id
        || pending.target_document_hash != secret.target_document_hash
        || secret.target_did_document.get("id").and_then(Value::as_str)
            != Some(agent.agent_did.as_str())
    {
        bail!("encrypted Agent Legacy upgrade pending binding is inconsistent");
    }
    Ok(())
}

fn canonical_local_handle(config: &DaemonConfig, handle: &str) -> Result<String> {
    let normalized = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    let suffix = format!(".{}", config.did_domain.trim().to_ascii_lowercase());
    let local = normalized.strip_suffix(&suffix).unwrap_or(&normalized);
    if local.is_empty() || local.contains('.') {
        bail!("Legacy Agent Handle is not a canonical local-part for this DID domain");
    }
    Ok(local.to_owned())
}

fn canonical_full_handle(config: &DaemonConfig, handle: &str) -> Result<String> {
    let local = canonical_local_handle(config, handle)?;
    Ok(format!(
        "{local}.{}",
        config.did_domain.trim().to_ascii_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct LostResponseClient {
        requests: Mutex<Vec<AgentLegacyUpgradeRequest>>,
    }

    impl AgentLegacyUpgradeClient for LostResponseClient {
        fn resolve_agent_document(&self, _did: &str) -> Result<Value> {
            self.requests
                .lock()
                .unwrap()
                .last()
                .map(|request| request.legacy_auth.did_document.clone())
                .context("no prior Legacy update request")
        }

        fn recover_committed_agent_device(
            &self,
            _im_core: &ImCoreAdapter,
            _target: &im_core::VNextAgentBootstrapMaterial,
        ) -> Result<crate::registration::AgentLegacyUpgradeResult> {
            bail!("lost-response client never commits the target")
        }

        fn update_agent_document(
            &self,
            request: AgentLegacyUpgradeRequest,
        ) -> Result<crate::registration::AgentLegacyUpgradeResult> {
            self.requests.lock().unwrap().push(request);
            bail!("simulated response loss")
        }
    }

    #[test]
    fn response_loss_refreshes_proof_but_reuses_the_exact_same_device() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open_with_root_key_bytes(&config, [91_u8; 32]);
        state.initialize().unwrap();
        let generated = crate::agent::generate_agent_identity(
            &config,
            crate::agent::AgentKind::Daemon,
            "legacydaemon",
        )
        .unwrap();
        let did = generated.did.clone();
        let full_handle = format!("legacydaemon.{}", config.did_domain);
        let legacy =
            generated.into_record("legacydaemon".to_owned(), crate::agent::AgentKind::Daemon);
        state.store_agent_identity(&legacy).unwrap();
        let agent = AgentDefinition {
            agent_did: did.clone(),
            handle: full_handle,
            agent_kind: crate::agent::AgentKind::Daemon,
            controller_user_id: "controller-account".to_owned(),
            controller_full_handle: "controller.awiki.info".to_owned(),
            controller_scope_key: "controller-scope:v1:legacydaemon".to_owned(),
            controller_did: "did:wba:awiki.info:controller".to_owned(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_owned(),
            local_agent_db_path: "agents/legacydaemon/agent.db".to_owned(),
            message_db_path: "agents/legacydaemon/messages.db".to_owned(),
            status: "active".to_owned(),
        };
        state.upsert_agent_definition(&agent).unwrap();
        let im_core = ImCoreAdapter::open(&config).unwrap();
        let client = LostResponseClient::default();

        for _ in 0..2 {
            let error = migrate_legacy_agent_identity(&config, &state, &im_core, &client, &agent)
                .unwrap_err();
            assert!(
                error.to_string().contains("update_document failed"),
                "unexpected migration error: {error:#}"
            );
        }

        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].did_document.get("id").and_then(Value::as_str),
            Some(did.as_str())
        );
        let first_manifest =
            anp::authentication::validate_device_manifest(&requests[0].did_document)
                .unwrap()
                .unwrap();
        let second_manifest =
            anp::authentication::validate_device_manifest(&requests[1].did_document)
                .unwrap()
                .unwrap();
        assert_eq!(first_manifest.devices.len(), 1);
        assert_eq!(first_manifest.devices, second_manifest.devices);
        let pending = state
            .load_pending_agent_legacy_upgrade(&did)
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, "retryable");
        assert_eq!(pending.attempt_count, 4);
        assert!(state.load_agent_device_identity(&did).unwrap().is_none());
        let raw_db =
            String::from_utf8_lossy(&std::fs::read(&config.daemon_db_path).unwrap()).into_owned();
        assert!(!raw_db.contains(&legacy.auth_private_key_pem));
    }

    #[derive(Default)]
    struct CommitThenLostClient {
        remote_document: Mutex<Option<Value>>,
        update_count: AtomicUsize,
        recovery_count: AtomicUsize,
        resolution_mode: AtomicU8,
    }

    impl CommitThenLostClient {
        fn set_unresolved(&self) {
            self.resolution_mode.store(1, Ordering::SeqCst);
        }

        fn set_conflicting_manifest(&self) {
            self.resolution_mode.store(2, Ordering::SeqCst);
        }
    }

    impl AgentLegacyUpgradeClient for CommitThenLostClient {
        fn resolve_agent_document(&self, did: &str) -> Result<Value> {
            match self.resolution_mode.load(Ordering::SeqCst) {
                1 => bail!("simulated remote resolution outage"),
                2 => {
                    let mut document = self
                        .remote_document
                        .lock()
                        .unwrap()
                        .clone()
                        .context("committed document is missing")?;
                    document["x-awiki-conflicting-manifest"] = serde_json::json!(true);
                    Ok(document)
                }
                _ => {
                    let document = self
                        .remote_document
                        .lock()
                        .unwrap()
                        .clone()
                        .context("committed document is missing")?;
                    if document.get("id").and_then(Value::as_str) != Some(did) {
                        bail!("committed document DID mismatch");
                    }
                    Ok(document)
                }
            }
        }

        fn recover_committed_agent_device(
            &self,
            _im_core: &ImCoreAdapter,
            target: &im_core::VNextAgentBootstrapMaterial,
        ) -> Result<crate::registration::AgentLegacyUpgradeResult> {
            self.recovery_count.fetch_add(1, Ordering::SeqCst);
            if self.remote_document.lock().unwrap().as_ref() != Some(&target.did_document) {
                bail!("session recovery target differs from committed document");
            }
            let request = crate::registration::AgentRegistrationExchangeRequest {
                token: crate::registration::RegistrationToken::new("legacy-recovery-test-token")?,
                agent_kind: crate::agent::AgentKind::Daemon,
                controller_did: "did:wba:awiki.info:controller".to_owned(),
                handle: target.handle_local_part.clone(),
                name: None,
                did_document: target.did_document.clone(),
                endpoint_url: None,
                key_algorithm: "Ed25519".to_owned(),
                public_key: target.device_signing_public_key_pem.clone(),
                allow_existing_agent_did: false,
            };
            let (did, _, access_token) = crate::registration::mock_vnext_exchange_fields(
                &request,
                "agent-account-after-legacy-upgrade",
            )?;
            Ok(crate::registration::AgentLegacyUpgradeResult {
                did,
                user_id: "agent-account-after-legacy-upgrade".to_owned(),
                binding_generation: "1".to_owned(),
                access_token,
            })
        }

        fn update_agent_document(
            &self,
            request: AgentLegacyUpgradeRequest,
        ) -> Result<crate::registration::AgentLegacyUpgradeResult> {
            let previous = self.update_count.fetch_add(1, Ordering::SeqCst);
            if previous != 0 {
                bail!("update_document was replayed after remote commit");
            }
            *self.remote_document.lock().unwrap() = Some(request.did_document);
            bail!("simulated response loss after remote commit")
        }
    }

    struct LegacyMigrationFixture {
        _root: tempfile::TempDir,
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
        agent: AgentDefinition,
    }

    fn legacy_migration_fixture(handle: &str, root_key: u8) -> LegacyMigrationFixture {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open_with_root_key_bytes(&config, [root_key; 32]);
        state.initialize().unwrap();
        let generated =
            crate::agent::generate_agent_identity(&config, crate::agent::AgentKind::Daemon, handle)
                .unwrap();
        let did = generated.did.clone();
        let legacy = generated.into_record(handle.to_owned(), crate::agent::AgentKind::Daemon);
        state.store_agent_identity(&legacy).unwrap();
        let agent = AgentDefinition {
            agent_did: did,
            handle: format!("{handle}.{}", config.did_domain),
            agent_kind: crate::agent::AgentKind::Daemon,
            controller_user_id: "controller-account".to_owned(),
            controller_full_handle: "controller.awiki.info".to_owned(),
            controller_scope_key: format!("controller-scope:v1:{handle}"),
            controller_did: "did:wba:awiki.info:controller".to_owned(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_owned(),
            local_agent_db_path: format!("agents/{handle}/agent.db"),
            message_db_path: format!("agents/{handle}/messages.db"),
            status: "active".to_owned(),
        };
        state.upsert_agent_definition(&agent).unwrap();
        let im_core = ImCoreAdapter::open(&config).unwrap();
        LegacyMigrationFixture {
            _root: root,
            config,
            state,
            im_core,
            agent,
        }
    }

    #[test]
    fn commit_then_response_loss_recovers_without_replaying_update_document() {
        let fixture = legacy_migration_fixture("commitlost", 93);
        let client = CommitThenLostClient::default();

        let first = migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap_err();
        assert!(format!("{first:#}").contains("response loss"));

        migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap();

        assert_eq!(client.update_count.load(Ordering::SeqCst), 1);
        assert_eq!(client.recovery_count.load(Ordering::SeqCst), 1);
        let active = fixture
            .state
            .load_agent_device_identity(&fixture.agent.agent_did)
            .unwrap()
            .unwrap();
        assert_eq!(active.identity_status, "active");
        assert_eq!(active.account_id, "agent-account-after-legacy-upgrade");
        assert_eq!(active.legacy_migration_state, "completed");
        let remote_committed_audits: i64 = fixture
            .state
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE event_type = 'agent.identity.legacy_upgrade.remote_already_committed' AND agent_did = ?1",
                [&fixture.agent.agent_did],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remote_committed_audits, 1);
    }

    #[test]
    fn unresolved_remote_state_fails_closed_without_replaying_update_document() {
        let fixture = legacy_migration_fixture("unresolved", 94);
        let client = CommitThenLostClient::default();
        migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap_err();
        client.set_unresolved();

        let error = migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("update_document suppressed"));
        assert_eq!(client.update_count.load(Ordering::SeqCst), 1);
        assert_eq!(client.recovery_count.load(Ordering::SeqCst), 0);
        let pending = fixture
            .state
            .load_pending_agent_legacy_upgrade(&fixture.agent.agent_did)
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, "retryable");
        assert_eq!(
            pending.last_error_code.as_deref(),
            Some("legacy_remote_document_unresolved")
        );
    }

    #[test]
    fn conflicting_remote_manifest_blocks_without_replaying_update_document() {
        let fixture = legacy_migration_fixture("conflict", 95);
        let client = CommitThenLostClient::default();
        migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap_err();
        client.set_conflicting_manifest();

        let error = migrate_legacy_agent_identity(
            &fixture.config,
            &fixture.state,
            &fixture.im_core,
            &client,
            &fixture.agent,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("conflicts with pending target"));
        assert_eq!(client.update_count.load(Ordering::SeqCst), 1);
        assert_eq!(client.recovery_count.load(Ordering::SeqCst), 0);
        let pending = fixture
            .state
            .load_pending_agent_legacy_upgrade(&fixture.agent.agent_did)
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, "blocked");
        assert_eq!(
            pending.last_error_code.as_deref(),
            Some("legacy_remote_state_conflict")
        );
    }
}
