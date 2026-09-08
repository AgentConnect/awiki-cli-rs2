use super::ObjectProofReview;
use crate::internal::identity_provider::{
    ProviderKeyAlgorithm, ProviderKeySelector, ProviderSignRequest, ProviderSigningPurpose,
};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, CoreHttpTransport};
use crate::{ImClient, ImError, ImResult};

#[derive(Debug, serde::Serialize)]
pub struct ObjectProofCapability {
    pub cryptosuite: String,
    pub signer_did: String,
    pub verification_method: String,
}
pub struct ObjectProofService<'a> {
    client: &'a ImClient,
}
impl ImClient {
    pub fn object_proofs(&self) -> ObjectProofService<'_> {
        ObjectProofService { client: self }
    }
}
impl ObjectProofService<'_> {
    /// Checks the exact active device before prompting. Signing repeats this check after review.
    pub async fn inspect_capability_async(&self) -> ImResult<ObjectProofCapability> {
        let (kid, _, _) = self.current_device().await?;
        Ok(ObjectProofCapability {
            cryptosuite: "eddsa-jcs-2022".to_owned(),
            signer_did: self.client.did().as_str().to_owned(),
            verification_method: kid,
        })
    }
    async fn current_device(&self) -> ImResult<(String, serde_json::Value, std::time::Instant)> {
        let client = self.client;
        let device_id = client.exact_protocol_device_id()?;
        let signer = client.runtime().key_provider.as_ref();
        let kid = signer.request_signing_key_id()?;
        let observed = std::time::Instant::now();
        let mut transport = CoreHttpTransport::new(client);
        let call =
            crate::internal::identity_wire::device_join::build_registry_call(client.did(), false);
        let raw = transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let registry = crate::internal::identity_wire::device_join::parse_registry_result(
            raw,
            client.did(),
            false,
        )?;
        let document = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut transport,
            client.did().as_str(),
        )
        .await?;
        validate_device(
            &document,
            client.did().as_str(),
            &device_id,
            &kid,
            &registry,
        )?;
        if observed.elapsed().as_secs() >= 5 {
            return Err(ImError::PermissionDenied);
        }
        Ok((kid, document, observed))
    }
    /// Signs the fixed reviewed JSON object with the current active device.
    /// Hosts must collect confirmation themselves; an incoming message is not authorization.
    /// This method does not submit objects or send notifications.
    pub async fn sign_reviewed_async(
        &self,
        review: ObjectProofReview,
        expected_object_hash: &str,
    ) -> ImResult<serde_json::Value> {
        if review.object_hash() != expected_object_hash {
            return Err(ImError::PermissionDenied);
        }
        let client = self.client;
        let signer = client.runtime().key_provider.as_ref();
        let (kid, document, observed) = self.current_device().await?;
        let public_key = signer.public_key(&kid)?;
        let prepared = anp::proof::prepare_object_proof(
            &review.object,
            &public_key,
            &kid,
            client.did().as_str(),
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        )
        .map_err(crate::internal::key_provider::map_crypto_error)?;
        let signature = if let Some(session) = client.runtime().identity_session.as_ref() {
            let signed = session
                .sign(ProviderSignRequest {
                    purpose: ProviderSigningPurpose::DeviceAssertion,
                    key: ProviderKeySelector::Kid(kid.clone()),
                    payload: prepared.signing_input().to_vec(),
                })
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?;
            if signed.kid != kid || signed.algorithm != ProviderKeyAlgorithm::Ed25519 {
                return Err(ImError::PermissionDenied);
            }
            signed.bytes
        } else {
            signer.sign_device_assertion(&kid, prepared.signing_input())?
        };
        if observed.elapsed().as_secs() >= 5 {
            return Err(ImError::PermissionDenied);
        }
        let signed = anp::proof::complete_object_proof(prepared, &signature)
            .map_err(crate::internal::key_provider::map_crypto_error)?;
        anp::proof::verify_object_proof(&signed, client.did().as_str(), &document)
            .map_err(|_| ImError::PermissionDenied)?;
        signed
            .get("proof")
            .cloned()
            .ok_or(ImError::PermissionDenied)
    }
}
pub(super) fn validate_device(
    document: &serde_json::Value,
    did: &str,
    device: &str,
    kid: &str,
    registry: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
) -> ImResult<()> {
    use sha2::{Digest, Sha256};
    let document_hash = format!(
        "{:x}",
        Sha256::digest(
            serde_json_canonicalizer::to_vec(document).map_err(|_| ImError::PermissionDenied)?
        )
    );
    if registry.checkpoint.document_hash != document_hash
        || document["id"] != did
        || registry.did.as_str() != did
        || !anp::authentication::validate_did_document_binding(document, true)
        || document["proof"]["verificationMethod"] == kid
    {
        return Err(ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| ImError::PermissionDenied)?
        .ok_or(ImError::PermissionDenied)?;
    let entry = manifest
        .devices
        .iter()
        .find(|entry| entry.device_id == device && entry.signing_key_id == kid)
        .ok_or(ImError::PermissionDenied)?;
    if !registry.devices.iter().any(|r| {
        r.device_id == device
            && r.signing_key_id == kid
            && r.e2ee_key_id == entry.e2ee_key_id
            && r.auth_generation > 0
            && r.status == crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
    }) || !document["assertionMethod"]
        .as_array()
        .is_some_and(|methods| methods.iter().any(|m| m == kid || m["id"] == kid))
    {
        return Err(ImError::PermissionDenied);
    }
    Ok(())
}
