//! Root-signed, metadata-only discovery upgrade for an explicitly selected Home.
use super::*;
use crate::internal::identity_provider::{
    map_provider_error, ProviderDocumentChangeOutcome, ProviderDocumentChangePhase,
    ProviderIdentityState, ProviderPublicationResult, ProviderVerifiedRemoteDocument,
};
use crate::internal::transport::AsyncAuthenticatedRpcTransport;

const GROUP_V2: &str = "anp.group.base.v2";

async fn refresh_request_signer(client: &crate::core::ImClient) -> crate::ImResult<()> {
    // Publication uses a separate custody session and advances its generation.
    // Refresh this caller's signer only after that verified change commits.
    if let Some(session) = client.runtime().identity_session.as_ref() {
        session.recover().await.map_err(map_provider_error)?;
    }
    Ok(())
}

fn conflict() -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: "Community service-profile upgrade conflicts with the published identity document"
            .into(),
    }
}

fn upgraded_document(document: &Value) -> crate::ImResult<Value> {
    let mut candidate = document.clone();
    let services = candidate
        .get_mut("service")
        .and_then(Value::as_array_mut)
        .ok_or_else(conflict)?;
    let mut matches = services
        .iter_mut()
        .filter(|service| service.get("type").and_then(Value::as_str) == Some("ANPMessageService"));
    let service = matches.next().ok_or_else(conflict)?;
    if matches.next().is_some() {
        return Err(conflict());
    }
    let profiles = service
        .get_mut("profiles")
        .and_then(Value::as_array_mut)
        .ok_or_else(conflict)?;
    if !profiles
        .iter()
        .any(|profile| profile.as_str() == Some(GROUP_V2))
    {
        profiles.push(Value::String(GROUP_V2.into()));
    }
    Ok(candidate)
}

fn same_unsigned(left: &Value, right: &Value) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    for document in [&mut left, &mut right] {
        if let Some(object) = document.as_object_mut() {
            object.remove("proof");
        }
    }
    left == right
}

fn service_change(document: &Value) -> crate::ImResult<Value> {
    let did = document
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(conflict)?;
    let services = document
        .get("service")
        .and_then(Value::as_array)
        .ok_or_else(conflict)?;
    let mut replacements = Vec::new();
    for service in services {
        if service.get("type").and_then(Value::as_str) == Some("AgentDescription") {
            continue;
        }
        let object = service.as_object().ok_or_else(conflict)?;
        if object.keys().any(|key| {
            ![
                "id",
                "type",
                "serviceEndpoint",
                "serviceDid",
                "profiles",
                "securityProfiles",
            ]
            .contains(&key.as_str())
        }) {
            return Err(conflict());
        }
        let id = service
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(conflict)?;
        let fragment = id
            .strip_prefix(&format!("{did}#"))
            .or_else(|| id.strip_prefix('#'))
            .ok_or_else(conflict)?;
        replacements.push(serde_json::json!({
            "id":fragment, "service_type":service["type"], "service_endpoint":service["serviceEndpoint"],
            "service_did":service.get("serviceDid").cloned().unwrap_or(Value::Null),
            "profiles":service.get("profiles").cloned().unwrap_or_else(|| serde_json::json!([])),
            "security_profiles":service.get("securityProfiles").cloned().unwrap_or_else(|| serde_json::json!([])),
        }));
    }
    Ok(serde_json::json!({"changes":[{"change":"replace_services","services":replacements}]}))
}

async fn remote_document(
    client: &crate::core::ImClient,
    transport: &mut crate::internal::transport::CoreHttpTransport<'_>,
) -> crate::ImResult<(Value, u64)> {
    let own = transport
        .authenticated_rpc(
            crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT,
            "get_me",
            serde_json::json!({}),
        )
        .await?;
    let version = own
        .get("document_version")
        .and_then(Value::as_u64)
        .filter(|version| *version > 0)
        .ok_or_else(conflict)?;
    let public = crate::internal::discovery::did_document::resolve_did_document_async(
        transport,
        client.did().as_str(),
    )
    .await?;
    if own.get("did_document") != Some(&public) {
        return Err(conflict());
    }
    Ok((public, version))
}

async fn publish_document(
    transport: &mut crate::internal::transport::CoreHttpTransport<'_>,
    old: &Value,
    version: u64,
    candidate: &Value,
) -> crate::ImResult<()> {
    let mut call = crate::internal::identity_wire::update_document::build_update_document_rpc_call(
        crate::internal::identity_wire::UpdateDocumentRpcParams {
            did_document: candidate.clone(),
            is_public: None,
            is_agent: None,
            role: None,
            endpoint_url: None,
        },
    );
    call.params["expected_document_hash"] = Value::String(
        crate::internal::identity_wire::document::document_hash(old)?,
    );
    call.params["expected_document_version"] = serde_json::json!(version);
    let result = transport
        .authenticated_rpc(call.endpoint, call.method, call.params)
        .await?;
    if result.get("did_document") != Some(candidate)
        || result
            .get("document_version")
            .and_then(Value::as_u64)
            .is_none_or(|new| new <= version)
    {
        return Err(conflict());
    }
    Ok(())
}

impl IdentityRegistry<'_> {
    pub(crate) async fn ensure_community_service_discovery(
        &self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<()> {
        // Host-backed callers already declaring v2 need no registry mutation.
        if client.current_identity().local_alias.is_none() {
            let document = client.runtime().key_provider.did_document()?;
            if upgraded_document(&document)? == document {
                return Ok(());
            }
            return Err(crate::ImError::unsupported(
                "community-service-profile-upgrade-requires-root-custody",
            ));
        }
        let registry = self.load_registry_async().await?;
        let entry = registry
            .find_entry(super::super::IdentitySelector::Id(
                client.current_identity().id.clone(),
            ))?
            .clone();
        let dir = entry.identity_dir_name().ok_or_else(conflict)?;
        let store = crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        );
        let local = store.load_did_document(&dir)?;
        let wanted = upgraded_document(&local)?;
        if wanted == local {
            return Ok(());
        }
        let provider = open_registry_provider_session(self.core, &entry).await?;
        let public = provider
            .public_identity()
            .await
            .map_err(map_provider_error)?;
        if public.state != ProviderIdentityState::Active
            || public.document.get("id") != local.get("id")
        {
            return Err(conflict());
        }
        let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
        let (mut remote, mut remote_version) = remote_document(client, &mut transport).await?;
        let checkpoint = entry
            .device_state
            .as_ref()
            .and_then(|state| state.checkpoint.as_ref())
            .ok_or_else(conflict)?;
        let evidence = |document: &Value, version: u64| {
            let mut verified = checkpoint.clone();
            verified.document_version = version;
            provider_publication_evidence(document, Some(&verified))
        };
        let persist = |document: &Value, version: u64| -> crate::ImResult<()> {
            let mut state = entry.device_state.clone().ok_or_else(conflict)?;
            let checkpoint = state.checkpoint.as_mut().ok_or_else(conflict)?;
            if version < checkpoint.document_version {
                return Err(conflict());
            }
            checkpoint.document_version = version;
            checkpoint.document_hash =
                crate::internal::identity_wire::document::document_hash(document)?;
            state.validate_for_did(client.did())?;
            store.save_device_state(entry.local_alias.as_deref().ok_or_else(conflict)?, state)?;
            save_identity_document_projection(self.core, &dir, document)
        };
        if same_unsigned(&wanted, &public.document) && public.document == remote {
            persist(&public.document, remote_version)?;
            return refresh_request_signer(client).await;
        }
        if !same_unsigned(&local, &public.document) {
            return Err(conflict());
        }
        let pending = provider
            .resume_document_change()
            .await
            .map_err(map_provider_error)?;
        let newly_prepared = pending.is_none();
        let publication = if let Some(pending) = pending {
            pending
        } else {
            if remote != public.document {
                return Err(conflict());
            }
            provider
                .prepare_document_change(service_change(&wanted)?)
                .await
                .map_err(map_provider_error)?
        };
        let candidate = publication
            .candidate()
            .await
            .map_err(map_provider_error)?
            .candidate_document;
        if !same_unsigned(&wanted, &candidate) {
            if newly_prepared {
                let attempt = publication
                    .begin_publication()
                    .await
                    .map_err(map_provider_error)?;
                publication
                    .complete(attempt, ProviderPublicationResult::RejectedBeforeAcceptance)
                    .await
                    .map_err(map_provider_error)?;
            }
            return Err(conflict());
        }
        let phase = publication.host_phase().await.map_err(map_provider_error)?;
        if phase == ProviderDocumentChangePhase::PublicationInFlight {
            let attempt = publication
                .begin_publication()
                .await
                .map_err(map_provider_error)?;
            publication
                .complete(attempt, ProviderPublicationResult::Unknown)
                .await
                .map_err(map_provider_error)?;
        }
        // An older, unversioned Home may already have accepted this exact
        // candidate. Reconfirm it using a real CAS revision before SDK commit.
        if remote == candidate && remote_version <= checkpoint.document_version {
            if remote_version != checkpoint.document_version {
                return Err(conflict());
            }
            publish_document(&mut transport, &remote, remote_version, &candidate).await?;
            (remote, remote_version) = remote_document(client, &mut transport).await?;
            if remote != candidate {
                return Err(conflict());
            }
        }
        if matches!(
            phase,
            ProviderDocumentChangePhase::PublicationInFlight
                | ProviderDocumentChangePhase::PublicationUncertain
        ) {
            // Identity 0.2.3's reconcile API compares the canonical document
            // hash (including its algorithm prefix), while complete compares
            // the prepared candidate digest. Keep those two contracts distinct.
            let mut remote_evidence = evidence(&remote, remote_version)?;
            remote_evidence.document_digest =
                crate::internal::identity_wire::document::document_hash(&remote)?;
            let outcome = publication
                .reconcile(ProviderVerifiedRemoteDocument {
                    evidence: remote_evidence,
                    document: remote.clone(),
                })
                .await
                .map_err(map_provider_error)?;
            if let ProviderDocumentChangeOutcome::Committed { identity } = outcome {
                persist(&identity.document, remote_version)?;
                return refresh_request_signer(client).await;
            }
        }
        if remote != public.document && remote != candidate {
            return Err(conflict());
        }
        let attempt = publication
            .begin_publication()
            .await
            .map_err(map_provider_error)?;
        let publication_result = async {
            if remote != candidate {
                publish_document(&mut transport, &remote, remote_version, &candidate).await?;
            }
            let (observed, version) = remote_document(client, &mut transport).await?;
            if observed != candidate {
                return Err(conflict());
            }
            Ok(version)
        }
        .await;
        let version = match publication_result {
            Ok(version) => version,
            Err(error) => {
                if phase != ProviderDocumentChangePhase::Published {
                    publication
                        .complete(attempt, ProviderPublicationResult::Unknown)
                        .await
                        .map_err(map_provider_error)?;
                }
                return Err(error);
            }
        };
        let outcome = publication
            .complete(
                attempt,
                ProviderPublicationResult::Confirmed {
                    evidence: evidence(&candidate, version)?,
                },
            )
            .await
            .map_err(map_provider_error)?;
        if let ProviderDocumentChangeOutcome::Committed { identity } = outcome {
            persist(&identity.document, version)?;
            refresh_request_signer(client).await
        } else {
            Err(conflict())
        }
    }
}

#[cfg(test)]
mod tests;
