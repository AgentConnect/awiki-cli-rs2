//! Ordinary service updates over the existing device_document_update RPC.
//! The Vault journal binds one operation to one candidate across response loss.
//! Its receipt confirms that operation; current Registry/DID state authorizes
//! local adoption independently, including when later updates already exist.

use crate::identity::{DidDocumentService, IdentitySelector};
use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteRegistry,
};
use crate::internal::identity_device_state::*;
use crate::internal::identity_provider::*;
use crate::internal::identity_wire::{device_document_update as wire, document};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, CoreHttpTransport};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod conflict;
mod pending;
pub(crate) use conflict::{checkpoint_conflict, reconcile_rejected};
use pending::Store;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    schema_version: u32,
    did: crate::ids::Did,
    operation_id: String,
    base_document: Value,
    checkpoint: IdentityInternalCheckpoint,
    authorizer: DeviceJoinRemoteDeviceSummary,
    services: Vec<DidDocumentService>,
    candidate: Option<Value>,
    provider_operation_id: Option<String>,
    committed: bool,
    #[serde(default)]
    rejected: bool,
}

impl Pending {
    fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != 1
            || uuid::Uuid::parse_str(&self.operation_id).is_err()
            || self.base_document.get("id").and_then(Value::as_str) != Some(self.did.as_str())
            || self.checkpoint.document_version == 0
            || self.checkpoint.registry_version == 0
            || self.checkpoint.document_hash != document::document_hash(&self.base_document)?
            || !document::validate_control_document_method(&self.base_document)
            || self.authorizer.role != DeviceAuthorizationRole::Admin
            || self.authorizer.status != DeviceAuthorizationStatus::Active
            || !self.authorizer.management_ready
            || self.authorizer.auth_generation == 0
            || self.candidate.is_some() != self.provider_operation_id.is_some()
            || (self.committed && self.candidate.is_none())
            || (self.rejected && (self.committed || self.candidate.is_none()))
        {
            return Err(crate::ImError::PermissionDenied);
        }
        validate_services(&self.did, &self.base_document, &self.services)?;
        if let Some(candidate) = &self.candidate {
            self.validate_candidate(candidate)?;
        }
        Ok(())
    }

    fn validate_candidate(&self, candidate: &Value) -> crate::ImResult<()> {
        let unsigned = |value: &Value| -> crate::ImResult<Value> {
            let mut value = value
                .as_object()
                .cloned()
                .ok_or(crate::ImError::PermissionDenied)?;
            value.remove("proof");
            value.remove("service");
            Ok(Value::Object(value))
        };
        if unsigned(&self.base_document)? != unsigned(candidate)?
            || !document::validate_control_document_method(candidate)
            || services(candidate, &self.did)? != self.services
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn result_checkpoint(&self) -> crate::ImResult<IdentityInternalCheckpoint> {
        Ok(IdentityInternalCheckpoint {
            document_version: self
                .checkpoint
                .document_version
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?,
            document_hash: document::document_hash(
                self.candidate
                    .as_ref()
                    .ok_or(crate::ImError::PermissionDenied)?,
            )?,
            registry_version: self.checkpoint.registry_version,
        })
    }
}

pub(crate) fn has_pending(core: &crate::ImCore, did: &crate::ids::Did) -> crate::ImResult<bool> {
    Ok(Store::new(core)?.load(did)?.is_some())
}

pub(crate) async fn update(
    core: &crate::ImCore,
    selector: IdentitySelector,
    input: Option<Vec<DidDocumentService>>,
) -> crate::ImResult<Value> {
    let (client, _, _) =
        crate::internal::identity_device_join::ready_admin_context_async(core, &selector, None)
            .await?;
    let _guard = core.inner().device_revoke_lock.lock().await;
    let mut remote = HttpRemote {
        transport: CoreHttpTransport::new(&client),
    };
    execute(core, &client, input, &mut remote).await
}

trait Remote {
    async fn current(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<(DeviceJoinRemoteRegistry, Value)>;
    async fn submit(
        &mut self,
        request: &wire::PreparedDeviceDocumentUpdate,
        did: &crate::ids::Did,
        checkpoint: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<IdentityInternalCheckpoint>;
}

struct HttpRemote<'a> {
    transport: CoreHttpTransport<'a>,
}
impl Remote for HttpRemote<'_> {
    async fn current(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<(DeviceJoinRemoteRegistry, Value)> {
        let call = crate::internal::identity_wire::device_join::build_registry_call(did, false);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let registry =
            crate::internal::identity_wire::device_join::parse_registry_result(raw, did, false)?;
        let document = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut self.transport,
            did.as_str(),
        )
        .await?;
        Ok((registry, document))
    }
    async fn submit(
        &mut self,
        request: &wire::PreparedDeviceDocumentUpdate,
        did: &crate::ids::Did,
        checkpoint: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<IdentityInternalCheckpoint> {
        let call = wire::build_update_call(request)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        wire::parse_update_result(raw, did, checkpoint)
    }
}

async fn execute<R: Remote>(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    input: Option<Vec<DidDocumentService>>,
    remote: &mut R,
) -> crate::ImResult<Value> {
    let store = Store::new(core)?;
    let identity = client
        .runtime()
        .identity_session
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let input = input
        .map(|s| normalize_services(client.did(), s))
        .transpose()?;
    let mut pending = match store.load(client.did())? {
        Some(pending) => {
            if input.as_ref().is_some_and(|s| *s != pending.services) {
                return Err(crate::ImError::invalid_input(
                    Some("services".into()),
                    "Resume the pending service update before starting another",
                ));
            }
            pending
        }
        None => {
            let Some(services) = input else {
                return core
                    .identities()
                    .identity_document_async(IdentitySelector::Did(client.did().clone()))
                    .await;
            };
            if identity
                .resume_document_change()
                .await
                .map_err(map_provider_error)?
                .is_some()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let (registry, base_document) = remote.current(client.did()).await?;
            let public = identity
                .public_identity()
                .await
                .map_err(map_provider_error)?;
            let authorizer =
                validate_current(client, &registry, &base_document, &public.document, None)?;
            validate_services(client.did(), &base_document, &services)?;
            adopt(identity.as_ref(), &registry, &base_document).await?;
            let pending = Pending {
                schema_version: 1,
                did: client.did().clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                base_document,
                checkpoint: registry.checkpoint,
                authorizer,
                services,
                candidate: None,
                provider_operation_id: None,
                committed: false,
                rejected: false,
            };
            store.save(&pending)?;
            pending
        }
    };
    pending.validate()?;
    if pending.authorizer.signing_key_id
        != client.runtime().key_provider.request_signing_key_id()?
        || pending.authorizer.e2ee_key_id != client.runtime().key_provider.agreement_key_id()?
    {
        return Err(crate::ImError::PermissionDenied);
    }

    if pending.rejected {
        finish_rejected(core, client, &pending, remote).await?;
        store.delete(client.did())?;
        return Err(rejected_error());
    }
    if !pending.committed {
        let change = match identity
            .resume_document_change()
            .await
            .map_err(map_provider_error)?
        {
            Some(change) => change,
            None if pending.candidate.is_none() => {
                let replacements = pending
                    .services
                    .iter()
                    .filter(|s| s.service_type != "AgentDescription")
                    .map(|s| ProviderIdentityService {
                        id: s
                            .id
                            .rsplit_once('#')
                            .map(|(_, fragment)| fragment)
                            .unwrap_or(&s.id)
                            .into(),
                        service_type: s.service_type.clone(),
                        service_endpoint: s.service_endpoint.clone(),
                        service_did: s.service_did.clone(),
                        profiles: s.profiles.clone(),
                        security_profiles: s.security_profiles.clone(),
                    })
                    .collect::<Vec<_>>();
                match identity.prepare_document_change(json!({"changes":[{"change":"replace_services", "services": replacements}]})).await {
                    Ok(change) => change,
                    Err(error) => {
                        // No HTTP submission has occurred. Clear only when
                        // custody confirms it did not persist a candidate.
                        if identity.resume_document_change().await.map_err(map_provider_error)?.is_none() { store.delete(client.did())?; }
                        return Err(map_provider_error(error));
                    }
                }
            }
            None => return Err(crate::ImError::PermissionDenied),
        };
        let candidate = change.candidate().await.map_err(map_provider_error)?;
        pending.validate_candidate(&candidate.candidate_document)?;
        if pending
            .candidate
            .as_ref()
            .is_some_and(|doc| *doc != candidate.candidate_document)
            || pending
                .provider_operation_id
                .as_ref()
                .is_some_and(|id| *id != candidate.operation_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        pending.candidate = Some(candidate.candidate_document);
        pending.provider_operation_id = Some(candidate.operation_id);
        store.save(&pending)?;
        let unsigned = wire::prepare_update_unsigned(
            pending.operation_id.clone(),
            pending.checkpoint.clone(),
            pending
                .candidate
                .clone()
                .ok_or(crate::ImError::PermissionDenied)?,
            pending.authorizer.device_id.clone(),
            &pending.authorizer.signing_key_id,
            core.inner().multi_device_audience(),
            time::OffsetDateTime::now_utc(),
        )?;
        let signature = identity
            .sign(ProviderSignRequest {
                purpose: ProviderSigningPurpose::DeviceAssertion,
                key: ProviderKeySelector::Kid(unsigned.signing_key_id.clone()),
                payload: unsigned.signing_input.clone(),
            })
            .await
            .map_err(map_provider_error)?;
        if signature.kid != unsigned.signing_key_id
            || signature.algorithm != ProviderKeyAlgorithm::Ed25519
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let request = wire::complete_update(unsigned, &signature.bytes)?;
        let attempt = match change.host_phase().await.map_err(map_provider_error)? {
            ProviderDocumentChangePhase::PublicationUncertain => None,
            _ => Some(
                change
                    .begin_publication()
                    .await
                    .map_err(map_provider_error)?,
            ),
        };
        let expected = pending.result_checkpoint()?;
        let response = remote.submit(&request, client.did(), &expected).await;
        if response.as_ref().ok() != Some(&expected) {
            if let Some(attempt) = attempt {
                change
                    .complete(attempt, ProviderPublicationResult::Unknown)
                    .await
                    .map_err(map_provider_error)?;
            }
            let error = response.err().unwrap_or(crate::ImError::PermissionDenied);
            if client.did().as_str().starts_with("did:web:") && checkpoint_conflict(&error) {
                pending.rejected = true;
                store.save(&pending)?;
                finish_rejected(core, client, &pending, remote).await?;
                store.delete(client.did())?;
            }
            return Err(error);
        }
        // Seal the exact result before custody can retire its original candidate.
        pending.committed = true;
        store.save(&pending)?;
    }

    let (registry, current) = remote.current(client.did()).await?;
    let authorizer = validate_current(
        client,
        &registry,
        &current,
        &pending.base_document,
        Some(&pending.result_checkpoint()?),
    )?;
    if authorizer != pending.authorizer {
        return Err(crate::ImError::PermissionDenied);
    }
    if let Some(change) = identity
        .resume_document_change()
        .await
        .map_err(map_provider_error)?
    {
        if change
            .candidate()
            .await
            .map_err(map_provider_error)?
            .operation_id
            != pending
                .provider_operation_id
                .clone()
                .ok_or(crate::ImError::PermissionDenied)?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_device_join::complete_provider_document_change(
            client,
            pending
                .candidate
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?,
            &pending.result_checkpoint()?,
        )
        .await?;
    }
    adopt(identity.as_ref(), &registry, &current).await?;
    persist_current(core, client, &registry, &current, authorizer)?;
    store.delete(client.did())?;
    Ok(current)
}

fn rejected_error() -> crate::ImError {
    crate::ImError::invalid_input(Some("services".into()),
        "The previous service update was rejected by a checkpoint conflict; current state was refreshed. Submit a new update")
}

async fn finish_rejected<R: Remote>(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    pending: &Pending,
    remote: &mut R,
) -> crate::ImResult<()> {
    let (registry, current) = remote.current(client.did()).await?;
    reconcile_rejected(
        core,
        client,
        &pending.checkpoint,
        &pending.authorizer,
        pending
            .candidate
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?,
        pending.provider_operation_id.as_deref(),
        &registry,
        &current,
    )
    .await
}

fn persist_current(
    core: &crate::ImCore,
    client: &crate::core::ImClient,
    registry: &DeviceJoinRemoteRegistry,
    current: &Value,
    authorizer: DeviceJoinRemoteDeviceSummary,
) -> crate::ImResult<()> {
    crate::internal::identity_device_revoke::write_document_atomic(
        &client.runtime().did_document_path,
        &current,
    )?;
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
        .save_device_state(
            alias,
            IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(&authorizer.device_id)?,
                    signing_key_id: authorizer.signing_key_id,
                    e2ee_key_id: authorizer.e2ee_key_id,
                    status: authorizer.status,
                    role: authorizer.role,
                    management_ready: authorizer.management_ready,
                    auth_generation: authorizer.auth_generation,
                }),
                checkpoint: Some(registry.checkpoint.clone()),
            },
        )?;
    Ok(())
}

async fn adopt(
    identity: &dyn IdentitySession,
    registry: &DeviceJoinRemoteRegistry,
    document: &Value,
) -> crate::ImResult<()> {
    identity
        .adopt_verified_document(ProviderVerifiedRemoteDocument {
            document: document.clone(),
            evidence: ProviderPublicationEvidence {
                document_version: registry.checkpoint.document_version,
                registry_version: registry.checkpoint.registry_version,
                document_digest: registry.checkpoint.document_hash.clone(),
            },
        })
        .await
        .map_err(map_provider_error)?;
    let public = identity
        .public_identity()
        .await
        .map_err(map_provider_error)?;
    if public.state != ProviderIdentityState::Active || public.document != *document {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_current(
    client: &crate::core::ImClient,
    registry: &DeviceJoinRemoteRegistry,
    current: &Value,
    previous: &Value,
    minimum: Option<&IdentityInternalCheckpoint>,
) -> crate::ImResult<DeviceJoinRemoteDeviceSummary> {
    if registry.did != *client.did()
        || current.get("id").and_then(Value::as_str) != Some(client.did().as_str())
        || !document::validate_control_document_method(current)
        || registry.checkpoint.document_hash != document::document_hash(current)?
        || minimum.is_some_and(|min| {
            registry.checkpoint.document_version < min.document_version
                || registry.checkpoint.registry_version < min.registry_version
                || (registry.checkpoint.document_version == min.document_version
                    && registry.checkpoint.document_hash != min.document_hash)
        })
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let kid = client.runtime().key_provider.request_signing_key_id()?;
    let agreement = client.runtime().key_provider.agreement_key_id()?;
    let matches = registry
        .devices
        .iter()
        .filter(|d| d.signing_key_id == kid || d.e2ee_key_id == agreement)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let admin = matches[0];
    if admin.role != DeviceAuthorizationRole::Admin
        || admin.status != DeviceAuthorizationStatus::Active
        || !admin.management_ready
        || admin.signing_key_id != kid
        || admin.e2ee_key_id != agreement
        || registry
            .devices
            .iter()
            .filter(|d| d.device_id == admin.device_id)
            .count()
            != 1
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::identity_device_revoke::validate_manifest_device(current, admin)?;
    for key in [&kid, &agreement] {
        if crate::internal::identity_device_join::document_public_key_bytes(current, key)?
            != crate::internal::identity_device_join::document_public_key_bytes(previous, key)?
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(admin.clone())
}

fn services(document: &Value, did: &crate::ids::Did) -> crate::ImResult<Vec<DidDocumentService>> {
    normalize_services(
        did,
        serde_json::from_value(document.get("service").cloned().unwrap_or(json!([])))
            .map_err(|_| crate::ImError::PermissionDenied)?,
    )
}

fn normalize_services(
    did: &crate::ids::Did,
    mut services: Vec<DidDocumentService>,
) -> crate::ImResult<Vec<DidDocumentService>> {
    let mut ids = std::collections::BTreeSet::new();
    for service in &mut services {
        if service.id.starts_with('#') {
            service.id = format!("{}{}", did.as_str(), service.id);
        }
        let fragment = service
            .id
            .strip_prefix(&format!("{}#", did.as_str()))
            .unwrap_or("");
        if fragment.is_empty()
            || fragment.len() > 128
            || !fragment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'))
            || !ids.insert(service.id.clone())
            || service.service_type.trim().is_empty()
            || service.service_endpoint.trim().is_empty()
            || service.service_type.len() > 2048
            || service.service_endpoint.len() > 2048
            || service
                .service_did
                .as_ref()
                .is_some_and(|did| did.trim().is_empty() || did.trim() != did || did.len() > 2048)
        {
            return Err(crate::ImError::invalid_input(
                Some("services".into()),
                "Services require unique same-DID IDs, bounded types and endpoints",
            ));
        }
    }
    services.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(services)
}

fn validate_services(
    did: &crate::ids::Did,
    base: &Value,
    proposed: &[DidDocumentService],
) -> crate::ImResult<()> {
    let protected = |s: &DidDocumentService| {
        matches!(
            s.service_type.as_str(),
            "AgentDescription" | "ANPHandleService" | "ANPMessageService"
        )
    };
    let current = services(base, did)?;
    if current.iter().filter(|s| protected(s)).collect::<Vec<_>>()
        != proposed.iter().filter(|s| protected(s)).collect::<Vec<_>>()
    {
        return Err(crate::ImError::invalid_input(
            Some("services".into()),
            "Handle, messaging ownership and AgentDescription use their existing management flows",
        ));
    }
    Ok(())
}

#[cfg(all(test, feature = "identity-native-anp"))]
mod tests;
