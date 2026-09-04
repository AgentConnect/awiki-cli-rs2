use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use im_core::provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCreateIdentityRequest, ProviderDeviceEnrollmentRequest, ProviderDocumentChangeOutcome,
    ProviderDocumentChangePhase, ProviderDocumentChangeSession, ProviderEnrollmentProposal,
    ProviderEnrollmentProposalKind, ProviderEnrollmentSession, ProviderExactHttpRequest,
    ProviderExportedRoot, ProviderHostStatus, ProviderHttpHeader, ProviderIdentityDescriptor,
    ProviderIdentityMaterialImportRequest, ProviderIdentityRef, ProviderIdentityState,
    ProviderIdentityTransitionOutcome, ProviderIdentityTransitionPublicationAttempt,
    ProviderIdentityTransitionPublicationResult, ProviderIdentityTransitionRemoteObservation,
    ProviderIdentityTransitionRequest, ProviderIdentityTransitionSession,
    ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose, ProviderKeySelector,
    ProviderLegacyRootExportRequest, ProviderLegacyRootImportEvidence,
    ProviderLegacyRootImportOutcome, ProviderLegacyRootImportRequest, ProviderObjectProofRequest,
    ProviderOriginProofRequest, ProviderPreparedDocumentChange, ProviderPreparedHttpSignature,
    ProviderPreparedIdentityTransition, ProviderPrivateKeyEncoding, ProviderPublicIdentity,
    ProviderPublicKey, ProviderPublicationAttempt, ProviderPublicationResult,
    ProviderRequestSigningEnrollmentRequest, ProviderResult, ProviderSharedSecret,
    ProviderSignRequest, ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose,
    ProviderStoreInfo, ProviderVerifiedRemoteDocument, ProviderWrappedRootEnvelope,
};
use napi::bindgen_prelude::{Buffer, Promise};
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};

const SEALED_SECRET_PROTOCOL: &str = anp::sealed_handoff::IDENTITY_SEALED_SECRET_PROTOCOL;
const SEALED_SECRET_INFO: &[u8] = anp::sealed_handoff::IDENTITY_SEALED_SECRET_INFO;
const ECDH_SEALED_OPERATION: &str = anp::sealed_handoff::IDENTITY_ECDH_OPERATION;
const ENROLLMENT_ECDH_SEALED_OPERATION: &str =
    anp::sealed_handoff::IDENTITY_ENROLLMENT_ECDH_OPERATION;

pub(crate) type IdentityProviderDispatch = ThreadsafeFunction<
    (NodeIdentityProviderCall,),
    Promise<NodeIdentityProviderReply>,
    (NodeIdentityProviderCall,),
    napi::Status,
    false,
>;

#[napi(object)]
pub struct NodeIdentityProviderCall {
    pub operation: String,
    pub payload_json: String,
    pub buffers: Vec<Buffer>,
}

#[napi(object)]
pub struct NodeIdentityProviderReply {
    pub ok: bool,
    pub payload_json: String,
    pub buffers: Vec<Buffer>,
    pub error_code: Option<String>,
    pub retryable: Option<bool>,
}

#[derive(Clone)]
pub(crate) struct ExternalIdentityCustody {
    dispatch: Arc<IdentityProviderDispatch>,
}

struct ExternalIdentitySession {
    dispatch: Arc<IdentityProviderDispatch>,
    identity: ProviderIdentityRef,
    public_cache: Arc<tokio::sync::RwLock<Option<ProviderPublicIdentity>>>,
}

struct ExternalDocumentChangeSession {
    dispatch: Arc<IdentityProviderDispatch>,
    session_id: String,
    candidate: ProviderPreparedDocumentChange,
    public_cache: Arc<tokio::sync::RwLock<Option<ProviderPublicIdentity>>>,
}

struct ExternalIdentityTransitionSession {
    dispatch: Arc<IdentityProviderDispatch>,
    session_id: String,
    candidate: ProviderPreparedIdentityTransition,
}

struct ExternalEnrollmentSession {
    dispatch: Arc<IdentityProviderDispatch>,
    session_id: String,
    proposal: ProviderEnrollmentProposal,
}

impl ExternalIdentitySession {
    async fn default_agreement_kid(&self) -> ProviderResult<String> {
        self.public_identity()
            .await?
            .active_keys
            .into_iter()
            .find(|key| {
                key.algorithm == ProviderKeyAlgorithm::X25519
                    && key.purposes.contains(&ProviderKeyPurpose::KeyAgreement)
            })
            .map(|key| key.kid)
            .ok_or_else(|| provider_error(IdentityProviderErrorCode::CapabilityUnavailable, false))
    }

    async fn default_root_kid(&self) -> ProviderResult<String> {
        self.public_identity()
            .await?
            .active_keys
            .into_iter()
            .find(|key| key.purposes.contains(&ProviderKeyPurpose::RootControl))
            .map(|key| key.kid)
            .ok_or_else(|| provider_error(IdentityProviderErrorCode::CapabilityUnavailable, false))
    }
}

impl ExternalIdentityCustody {
    pub(crate) fn new(dispatch: IdentityProviderDispatch) -> Self {
        Self {
            dispatch: Arc::new(dispatch),
        }
    }

    pub(crate) async fn handshake(&self) -> crate::error::SafeResult<()> {
        let info = IdentityCustody::store_info(self)
            .await
            .map_err(safe_provider_error)?;
        if !info.schema_compatible || info.store_id.trim().is_empty() {
            return Err(crate::error::SafeError::new(
                "provider_incompatible",
                "The identity provider is incompatible.",
                false,
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityPayload<'a> {
    identity: &'a ProviderIdentityRef,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentChangePayload<'a> {
    identity: &'a ProviderIdentityRef,
    request: &'a serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentSessionPayload<'a> {
    session_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteDocumentChangePayload<'a> {
    session_id: &'a str,
    attempt: &'a ProviderPublicationAttempt,
    result: &'a ProviderPublicationResult,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReconcileDocumentChangePayload<'a> {
    session_id: &'a str,
    observation: &'a ProviderVerifiedRemoteDocument,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DocumentSessionWire {
    session_id: String,
    candidate: ProviderPreparedDocumentChange,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdentityTransitionSessionWire {
    session_id: String,
    candidate: ProviderPreparedIdentityTransition,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityTransitionSessionPayload<'a> {
    session_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteIdentityTransitionPayload<'a> {
    session_id: &'a str,
    attempt: &'a ProviderIdentityTransitionPublicationAttempt,
    result: &'a ProviderIdentityTransitionPublicationResult,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReconcileIdentityTransitionPayload<'a> {
    session_id: &'a str,
    observation: &'a ProviderIdentityTransitionRemoteObservation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollmentSessionWire {
    session_id: String,
    proposal: ProviderEnrollmentProposal,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnrollmentSessionPayload<'a> {
    session_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedKeyAgreementPayload<'a> {
    identity: &'a ProviderIdentityRef,
    kid: &'a str,
    request_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedEnrollmentKeyAgreementPayload<'a> {
    session_id: &'a str,
    request_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedRootExportPayload<'a> {
    identity: &'a ProviderIdentityRef,
    kid: &'a str,
    request_id: &'a str,
    user_presence_confirmed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedRootImportPreparationPayload<'a> {
    identity: &'a ProviderIdentityRef,
    evidence: &'a ProviderLegacyRootImportEvidence,
    encoding: ProviderPrivateKeyEncoding,
    request_id: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreparedSealedImportWire {
    session_id: String,
    offer: SealedImportOfferWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedImportOfferWire {
    request_id: String,
    token: String,
    authorization: AuthorizationContextWire,
    aad: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteSealedImportPayload<'a> {
    session_id: &'a str,
    token: &'a str,
    envelope: SealedSecretEnvelopeWire,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedIdentityImportPreparationPayload<'a> {
    remote: &'a ProviderVerifiedRemoteDocument,
    did_wba: bool,
    keys: Vec<SealedIdentityMaterialKeySpecWire<'a>>,
    request_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealedIdentityMaterialKeySpecWire<'a> {
    kid: &'a str,
    purpose: ProviderKeyPurpose,
    encoding: ProviderPrivateKeyEncoding,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreparedSealedIdentityImportWire {
    session_id: String,
    offer: SealedIdentityImportOfferWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedIdentityImportOfferWire {
    target: ProviderIdentityRef,
    request_id: String,
    token: String,
    authorization: AuthorizationContextWire,
    item_aad: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteSealedIdentityImportPayload<'a> {
    session_id: &'a str,
    token: &'a str,
    envelopes: Vec<SealedSecretEnvelopeWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedSecretDeliveryWire {
    envelope: SealedSecretEnvelopeWire,
    authorization: AuthorizationContextWire,
    aad: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedSecretEnvelopeWire {
    protocol: String,
    suite: String,
    encapped_key: String,
    ciphertext: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorizationContextWire {
    provider_instance_id: String,
    parent_lease_id: String,
    consumer: String,
    capability: String,
    store_id: String,
    expires_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivateEnrollmentPayload<'a> {
    session_id: &'a str,
    remote: &'a ProviderVerifiedRemoteDocument,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootPromotionPayload<'a> {
    identity: &'a ProviderIdentityRef,
    request: RootPromotionRequestPayload<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootPromotionRequestPayload<'a> {
    remote: &'a ProviderVerifiedRemoteDocument,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingRootProofPayload<'a> {
    identity: &'a ProviderIdentityRef,
    request: PendingRootProofRequestWire<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingRootProofRequestWire<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<&'a str>,
    document: &'a serde_json::Value,
    issuer_did: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignPayload<'a> {
    identity: &'a ProviderIdentityRef,
    purpose: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignaturePayload {
    kid: String,
    algorithm: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OriginProofPayload<'a> {
    identity: &'a ProviderIdentityRef,
    request: OriginProofWire<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OriginProofWire<'a> {
    method: &'a str,
    meta: &'a serde_json::Value,
    body: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OriginProofOptionsWire<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OriginProofOptionsWire<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpPayload<'a> {
    identity: &'a ProviderIdentityRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<&'a str>,
    url: &'a str,
    method: &'a str,
    headers: &'a [ProviderHttpHeader],
    has_body: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    covered_components: Option<&'a [String]>,
}

#[async_trait::async_trait]
impl IdentityCustody for ExternalIdentityCustody {
    async fn store_info(&self) -> ProviderResult<ProviderStoreInfo> {
        let info: WireStoreInfo =
            call_json(&self.dispatch, "info", &serde_json::json!({}), Vec::new()).await?;
        if info.health != "ready" {
            return Err(provider_error(
                IdentityProviderErrorCode::ProviderUnavailable,
                true,
            ));
        }
        Ok(ProviderStoreInfo {
            store_id: info.store_id,
            schema_compatible: info.schema_compatible,
            identity_count: info.identity_count,
        })
    }

    async fn list_identities(&self) -> ProviderResult<Vec<ProviderIdentityDescriptor>> {
        call_json(&self.dispatch, "list", &serde_json::json!({}), Vec::new()).await
    }

    async fn open_identity(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        let session = ExternalIdentitySession {
            dispatch: self.dispatch.clone(),
            identity: identity.clone(),
            public_cache: Arc::new(tokio::sync::RwLock::new(None)),
        };
        let public = session.public_identity().await?;
        if public.reference != *identity {
            return Err(provider_error(IdentityProviderErrorCode::Conflict, false));
        }
        Ok(Arc::new(session))
    }

    async fn create_identity(
        &self,
        request: ProviderCreateIdentityRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        let public: WirePublicIdentity =
            call_json(&self.dispatch, "create", &request, Vec::new()).await?;
        let public = ProviderPublicIdentity::try_from(public)?;
        let session = ExternalIdentitySession {
            dispatch: self.dispatch.clone(),
            identity: public.reference.clone(),
            public_cache: Arc::new(tokio::sync::RwLock::new(Some(public))),
        };
        Ok(Arc::new(session))
    }

    async fn delete_identity(&self, identity: &ProviderIdentityRef) -> ProviderResult<()> {
        call_json::<()>(
            &self.dispatch,
            "delete",
            &IdentityPayload { identity },
            Vec::new(),
        )
        .await
    }

    async fn prepare_identity_transition(
        &self,
        request: ProviderIdentityTransitionRequest,
    ) -> ProviderResult<Arc<dyn ProviderIdentityTransitionSession>> {
        let wire: IdentityTransitionSessionWire = call_json(
            &self.dispatch,
            "prepareIdentityTransition",
            &request,
            Vec::new(),
        )
        .await?;
        Ok(Arc::new(ExternalIdentityTransitionSession {
            dispatch: self.dispatch.clone(),
            session_id: wire.session_id,
            candidate: wire.candidate,
        }))
    }

    async fn resume_identity_transition(
        &self,
        expected_current_did: &str,
    ) -> ProviderResult<Option<Arc<dyn ProviderIdentityTransitionSession>>> {
        let wire: Option<IdentityTransitionSessionWire> = call_json(
            &self.dispatch,
            "resumeIdentityTransition",
            &serde_json::json!({ "expectedCurrentDid": expected_current_did }),
            Vec::new(),
        )
        .await?;
        Ok(wire.map(|wire| {
            Arc::new(ExternalIdentityTransitionSession {
                dispatch: self.dispatch.clone(),
                session_id: wire.session_id,
                candidate: wire.candidate,
            }) as Arc<dyn ProviderIdentityTransitionSession>
        }))
    }

    async fn begin_device_enrollment(
        &self,
        request: ProviderDeviceEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>> {
        let wire: EnrollmentSessionWire = call_json(
            &self.dispatch,
            "beginDeviceEnrollment",
            &request,
            Vec::new(),
        )
        .await?;
        Ok(Arc::new(ExternalEnrollmentSession {
            dispatch: self.dispatch.clone(),
            session_id: wire.session_id,
            proposal: wire.proposal,
        }))
    }

    async fn begin_request_signing_enrollment(
        &self,
        request: ProviderRequestSigningEnrollmentRequest,
    ) -> ProviderResult<Arc<dyn ProviderEnrollmentSession>> {
        let wire: EnrollmentSessionWire = call_json(
            &self.dispatch,
            "beginRequestSigningEnrollment",
            &request,
            Vec::new(),
        )
        .await?;
        Ok(Arc::new(ExternalEnrollmentSession {
            dispatch: self.dispatch.clone(),
            session_id: wire.session_id,
            proposal: wire.proposal,
        }))
    }

    async fn resume_enrollment(
        &self,
        identity: &ProviderIdentityRef,
    ) -> ProviderResult<Option<Arc<dyn ProviderEnrollmentSession>>> {
        let wire: Option<EnrollmentSessionWire> = call_json(
            &self.dispatch,
            "resumeEnrollment",
            &IdentityPayload { identity },
            Vec::new(),
        )
        .await?;
        Ok(wire.map(|wire| {
            Arc::new(ExternalEnrollmentSession {
                dispatch: self.dispatch.clone(),
                session_id: wire.session_id,
                proposal: wire.proposal,
            }) as Arc<dyn ProviderEnrollmentSession>
        }))
    }

    async fn confirm_root_promotion(
        &self,
        identity: &ProviderIdentityRef,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<()> {
        call_json::<()>(
            &self.dispatch,
            "confirmRootPromotion",
            &RootPromotionPayload {
                identity,
                request: RootPromotionRequestPayload { remote: &remote },
            },
            Vec::new(),
        )
        .await
    }

    async fn sign_pending_root_object_proof(
        &self,
        identity: &ProviderIdentityRef,
        request: ProviderObjectProofRequest,
    ) -> ProviderResult<serde_json::Value> {
        call_json(
            &self.dispatch,
            "signPendingRootObjectProof",
            &PendingRootProofPayload {
                identity,
                request: PendingRootProofRequestWire {
                    kid: selector_kid(&request.key),
                    document: &request.document,
                    issuer_did: &request.issuer_did,
                    created: request.created.as_deref(),
                },
            },
            Vec::new(),
        )
        .await
    }

    async fn import_legacy_root(
        &self,
        request: ProviderLegacyRootImportRequest,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        send_sealed_root_import(&self.dispatch, request).await
    }

    async fn import_wrapped_root(
        &self,
        identity: &ProviderIdentityRef,
        envelope: ProviderWrappedRootEnvelope,
    ) -> ProviderResult<ProviderLegacyRootImportOutcome> {
        let outcome: String = call_json(
            &self.dispatch,
            "importWrappedRoot",
            &serde_json::json!({ "identity": identity, "envelope": envelope }),
            Vec::new(),
        )
        .await?;
        match outcome.as_str() {
            "pending" => Ok(ProviderLegacyRootImportOutcome::Pending),
            "active" => Ok(ProviderLegacyRootImportOutcome::Active),
            _ => Err(provider_incompatible()),
        }
    }

    async fn import_identity_material(
        &self,
        request: ProviderIdentityMaterialImportRequest,
    ) -> ProviderResult<Arc<dyn IdentitySession>> {
        let public = send_sealed_identity_material_import(&self.dispatch, request).await?;
        Ok(Arc::new(ExternalIdentitySession {
            dispatch: self.dispatch.clone(),
            identity: public.reference.clone(),
            public_cache: Arc::new(tokio::sync::RwLock::new(Some(public))),
        }))
    }

    async fn recover(&self) -> ProviderResult<()> {
        call_json::<()>(
            &self.dispatch,
            "recover",
            &serde_json::json!({}),
            Vec::new(),
        )
        .await
    }
}

#[async_trait::async_trait]
impl IdentitySession for ExternalIdentitySession {
    async fn public_identity(&self) -> ProviderResult<ProviderPublicIdentity> {
        if let Some(cached) = self.public_cache.read().await.as_ref() {
            return Ok(cached.clone());
        }
        let public: WirePublicIdentity = call_json(
            &self.dispatch,
            "publicIdentity",
            &IdentityPayload {
                identity: &self.identity,
            },
            Vec::new(),
        )
        .await?;
        let public = ProviderPublicIdentity::try_from(public)?;
        let mut cache = self.public_cache.write().await;
        if let Some(cached) = cache.as_ref() {
            return Ok(cached.clone());
        }
        *cache = Some(public.clone());
        Ok(public)
    }

    async fn host_status(&self) -> ProviderResult<ProviderHostStatus> {
        call_json(
            &self.dispatch,
            "hostStatus",
            &IdentityPayload {
                identity: &self.identity,
            },
            Vec::new(),
        )
        .await
    }

    async fn sign(&self, request: ProviderSignRequest) -> ProviderResult<ProviderSignature> {
        let (purpose, domain) = match &request.purpose {
            ProviderSigningPurpose::Authentication => ("authentication", None),
            ProviderSigningPurpose::DeviceAssertion => ("device_assertion", None),
            ProviderSigningPurpose::ApplicationAssertion { domain } => {
                ("application_assertion", Some(domain.as_str()))
            }
        };
        let kid = selector_kid(&request.key);
        let reply = call(
            &self.dispatch,
            "sign",
            &SignPayload {
                identity: &self.identity,
                purpose,
                domain,
                kid,
            },
            vec![Buffer::from(request.payload)],
        )
        .await?;
        let metadata: SignaturePayload = parse_json(&reply.payload_json)?;
        let bytes = exactly_one_buffer(reply.buffers)?;
        if metadata.algorithm != "ed25519" {
            return Err(provider_error(
                IdentityProviderErrorCode::ProviderIncompatible,
                false,
            ));
        }
        Ok(ProviderSignature {
            kid: metadata.kid,
            algorithm: ProviderKeyAlgorithm::Ed25519,
            bytes,
        })
    }

    async fn sign_origin_proof(
        &self,
        request: ProviderOriginProofRequest,
    ) -> ProviderResult<ProviderSignedOriginProof> {
        let has_options = request.options.created.is_some()
            || request.options.expires.is_some()
            || request.options.nonce.is_some();
        call_json(
            &self.dispatch,
            "signOriginProof",
            &OriginProofPayload {
                identity: &self.identity,
                request: OriginProofWire {
                    method: &request.method,
                    meta: &request.meta,
                    body: &request.body,
                    kid: selector_kid(&request.key),
                    options: has_options.then_some(OriginProofOptionsWire {
                        created: request.options.created,
                        expires: request.options.expires,
                        nonce: request.options.nonce.as_deref(),
                    }),
                },
            },
            Vec::new(),
        )
        .await
    }

    async fn prepare_http_signature(
        &self,
        request: ProviderExactHttpRequest,
    ) -> ProviderResult<ProviderPreparedHttpSignature> {
        let buffers = request
            .body
            .as_ref()
            .map(|body| vec![Buffer::from(body.clone())])
            .unwrap_or_default();
        let prepared: WirePreparedHttpSignature = call_json(
            &self.dispatch,
            "prepareHttpSignature",
            &HttpPayload {
                identity: &self.identity,
                kid: selector_kid(&request.key),
                url: &request.url,
                method: &request.method,
                headers: &request.headers,
                has_body: request.body.is_some(),
                nonce: request.options.nonce.as_deref(),
                created: request.options.created,
                expires: request.options.expires,
                covered_components: request.options.covered_components.as_deref(),
            },
            buffers,
        )
        .await?;
        Ok(ProviderPreparedHttpSignature {
            binding_digest: prepared.binding_digest,
            kid: prepared.kid,
            header_patch: prepared.header_patch,
        })
    }

    async fn prepare_document_change(
        &self,
        request: serde_json::Value,
    ) -> ProviderResult<Arc<dyn ProviderDocumentChangeSession>> {
        let wire: DocumentSessionWire = call_json(
            &self.dispatch,
            "prepareDocumentChange",
            &DocumentChangePayload {
                identity: &self.identity,
                request: &request,
            },
            Vec::new(),
        )
        .await?;
        Ok(Arc::new(ExternalDocumentChangeSession {
            dispatch: self.dispatch.clone(),
            session_id: wire.session_id,
            candidate: wire.candidate,
            public_cache: self.public_cache.clone(),
        }))
    }

    async fn resume_document_change(
        &self,
    ) -> ProviderResult<Option<Arc<dyn ProviderDocumentChangeSession>>> {
        let wire: Option<DocumentSessionWire> = call_json(
            &self.dispatch,
            "resumeDocumentChange",
            &IdentityPayload {
                identity: &self.identity,
            },
            Vec::new(),
        )
        .await?;
        Ok(wire.map(|wire| {
            Arc::new(ExternalDocumentChangeSession {
                dispatch: self.dispatch.clone(),
                session_id: wire.session_id,
                candidate: wire.candidate,
                public_cache: self.public_cache.clone(),
            }) as Arc<dyn ProviderDocumentChangeSession>
        }))
    }

    async fn adopt_verified_document(
        &self,
        remote: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderPublicIdentity> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Payload<'a> {
            identity: &'a ProviderIdentityRef,
            remote: &'a ProviderVerifiedRemoteDocument,
        }
        let _: String = call_json(
            &self.dispatch,
            "adoptVerifiedDocument",
            &Payload {
                identity: &self.identity,
                remote: &remote,
            },
            Vec::new(),
        )
        .await?;
        *self.public_cache.write().await = None;
        self.public_identity().await
    }

    async fn derive_shared_secret(
        &self,
        request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret> {
        let kid = match request.key {
            ProviderKeySelector::Kid(kid) => kid,
            ProviderKeySelector::Default => self.default_agreement_kid().await?,
        };
        receive_sealed_shared_secret(
            &self.dispatch,
            ECDH_SEALED_OPERATION,
            &self.identity,
            None,
            &kid,
            request.peer_public,
        )
        .await
    }

    async fn export_root_for_legacy_envelope(
        &self,
        request: ProviderLegacyRootExportRequest,
    ) -> ProviderResult<ProviderExportedRoot> {
        if !request.user_presence_confirmed {
            return Err(provider_error(
                IdentityProviderErrorCode::InvalidRequest,
                false,
            ));
        }
        let kid = match request.key {
            ProviderKeySelector::Kid(kid) => kid,
            ProviderKeySelector::Default => self.default_root_kid().await?,
        };
        receive_sealed_root(
            &self.dispatch,
            &self.identity,
            &kid,
            request.user_presence_confirmed,
        )
        .await
    }

    async fn recover(&self) -> ProviderResult<()> {
        call_json::<()>(
            &self.dispatch,
            "recoverIdentity",
            &IdentityPayload {
                identity: &self.identity,
            },
            Vec::new(),
        )
        .await?;
        *self.public_cache.write().await = None;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProviderDocumentChangeSession for ExternalDocumentChangeSession {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedDocumentChange> {
        Ok(self.candidate.clone())
    }

    async fn host_phase(&self) -> ProviderResult<ProviderDocumentChangePhase> {
        call_json(
            &self.dispatch,
            "documentChangeHostPhase",
            &DocumentSessionPayload {
                session_id: &self.session_id,
            },
            Vec::new(),
        )
        .await
    }

    async fn begin_publication(&self) -> ProviderResult<ProviderPublicationAttempt> {
        call_json(
            &self.dispatch,
            "documentChangeBeginPublication",
            &DocumentSessionPayload {
                session_id: &self.session_id,
            },
            Vec::new(),
        )
        .await
    }

    async fn complete(
        &self,
        attempt: ProviderPublicationAttempt,
        result: ProviderPublicationResult,
    ) -> ProviderResult<ProviderDocumentChangeOutcome> {
        let outcome: WireDocumentChangeOutcome = call_json(
            &self.dispatch,
            "documentChangeComplete",
            &CompleteDocumentChangePayload {
                session_id: &self.session_id,
                attempt: &attempt,
                result: &result,
            },
            Vec::new(),
        )
        .await?;
        let outcome = ProviderDocumentChangeOutcome::try_from(outcome)?;
        if matches!(
            outcome,
            ProviderDocumentChangeOutcome::Committed { .. }
                | ProviderDocumentChangeOutcome::Aborted
        ) {
            *self.public_cache.write().await = None;
        }
        Ok(outcome)
    }

    async fn reconcile(
        &self,
        observation: ProviderVerifiedRemoteDocument,
    ) -> ProviderResult<ProviderDocumentChangeOutcome> {
        let outcome: WireDocumentChangeOutcome = call_json(
            &self.dispatch,
            "documentChangeReconcile",
            &ReconcileDocumentChangePayload {
                session_id: &self.session_id,
                observation: &observation,
            },
            Vec::new(),
        )
        .await?;
        let outcome = ProviderDocumentChangeOutcome::try_from(outcome)?;
        if matches!(outcome, ProviderDocumentChangeOutcome::Committed { .. }) {
            *self.public_cache.write().await = None;
        }
        Ok(outcome)
    }
}

#[async_trait::async_trait]
impl ProviderIdentityTransitionSession for ExternalIdentityTransitionSession {
    async fn candidate(&self) -> ProviderResult<ProviderPreparedIdentityTransition> {
        Ok(self.candidate.clone())
    }

    async fn begin_publication(
        &self,
    ) -> ProviderResult<ProviderIdentityTransitionPublicationAttempt> {
        call_json(
            &self.dispatch,
            "identityTransitionBeginPublication",
            &IdentityTransitionSessionPayload {
                session_id: &self.session_id,
            },
            Vec::new(),
        )
        .await
    }

    async fn complete(
        &self,
        attempt: ProviderIdentityTransitionPublicationAttempt,
        result: ProviderIdentityTransitionPublicationResult,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome> {
        call_json(
            &self.dispatch,
            "identityTransitionComplete",
            &CompleteIdentityTransitionPayload {
                session_id: &self.session_id,
                attempt: &attempt,
                result: &result,
            },
            Vec::new(),
        )
        .await
    }

    async fn reconcile(
        &self,
        observation: ProviderIdentityTransitionRemoteObservation,
    ) -> ProviderResult<ProviderIdentityTransitionOutcome> {
        call_json(
            &self.dispatch,
            "identityTransitionReconcile",
            &ReconcileIdentityTransitionPayload {
                session_id: &self.session_id,
                observation: &observation,
            },
            Vec::new(),
        )
        .await
    }
}

#[async_trait::async_trait]
impl ProviderEnrollmentSession for ExternalEnrollmentSession {
    async fn proposal(&self) -> ProviderResult<ProviderEnrollmentProposal> {
        Ok(self.proposal.clone())
    }

    async fn sign_device_assertion(&self, payload: Vec<u8>) -> ProviderResult<Vec<u8>> {
        let reply = call(
            &self.dispatch,
            "enrollmentSignDeviceAssertion",
            &EnrollmentSessionPayload {
                session_id: &self.session_id,
            },
            vec![Buffer::from(payload)],
        )
        .await?;
        if reply.payload_json != "null" {
            return Err(provider_error(
                IdentityProviderErrorCode::ProviderIncompatible,
                false,
            ));
        }
        exactly_one_buffer(reply.buffers)
    }

    async fn derive_device_shared_secret(
        &self,
        peer_public: [u8; 32],
    ) -> ProviderResult<ProviderSharedSecret> {
        let ProviderEnrollmentProposalKind::Device { agreement_key, .. } = &self.proposal.kind
        else {
            return Err(provider_error(
                IdentityProviderErrorCode::CapabilityUnavailable,
                false,
            ));
        };
        receive_sealed_shared_secret(
            &self.dispatch,
            ENROLLMENT_ECDH_SEALED_OPERATION,
            &self.proposal.identity,
            Some((&self.session_id, &self.proposal.enrollment_id)),
            &agreement_key.kid,
            peer_public,
        )
        .await
    }

    async fn activate(&self, remote: ProviderVerifiedRemoteDocument) -> ProviderResult<()> {
        call_json::<serde_json::Value>(
            &self.dispatch,
            "enrollmentActivate",
            &ActivateEnrollmentPayload {
                session_id: &self.session_id,
                remote: &remote,
            },
            Vec::new(),
        )
        .await
        .map(|_| ())
    }

    async fn cancel(&self) -> ProviderResult<()> {
        call_json::<()>(
            &self.dispatch,
            "enrollmentCancel",
            &EnrollmentSessionPayload {
                session_id: &self.session_id,
            },
            Vec::new(),
        )
        .await
    }
}

async fn receive_sealed_shared_secret(
    dispatch: &IdentityProviderDispatch,
    operation: &'static str,
    identity: &ProviderIdentityRef,
    enrollment: Option<(&str, &str)>,
    kid: &str,
    peer_public: [u8; 32],
) -> ProviderResult<ProviderSharedSecret> {
    let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
    let recipient_public = *recipient.public_key();
    let request_id = random_id();
    let buffers = vec![
        Buffer::from(peer_public.to_vec()),
        Buffer::from(recipient_public.to_vec()),
    ];
    let delivery: SealedSecretDeliveryWire = match enrollment {
        Some((session_id, _)) => {
            call_json(
                dispatch,
                "enrollmentEcdhSealed",
                &SealedEnrollmentKeyAgreementPayload {
                    session_id,
                    request_id: &request_id,
                },
                buffers,
            )
            .await?
        }
        None => {
            call_json(
                dispatch,
                "ecdhSealed",
                &SealedKeyAgreementPayload {
                    identity,
                    kid,
                    request_id: &request_id,
                },
                buffers,
            )
            .await?
        }
    };
    let identity_context = sealed_identity_context(identity);
    let binding = match enrollment {
        Some((_, enrollment_id)) => anp::sealed_handoff::identity_enrollment_ecdh_binding(
            &identity_context,
            enrollment_id,
            kid,
            &peer_public,
            &recipient_public,
            &request_id,
        ),
        None => anp::sealed_handoff::identity_ecdh_binding(
            &identity_context,
            kid,
            &peer_public,
            &recipient_public,
            &request_id,
        ),
    };
    let binding = binding.map_err(|_| provider_incompatible())?;
    if binding.operation != operation {
        return Err(provider_incompatible());
    }
    let aad = validate_delivery(&delivery, &binding, &identity_context)?;
    let handoff = delivery.envelope.to_handoff()?;
    let plaintext = recipient
        .open(&handoff, SEALED_SECRET_INFO, &aad)
        .map_err(|_| provider_incompatible())?;
    let bytes: [u8; 32] = plaintext
        .as_slice()
        .try_into()
        .map_err(|_| provider_incompatible())?;
    Ok(ProviderSharedSecret::new(bytes))
}

async fn receive_sealed_root(
    dispatch: &IdentityProviderDispatch,
    identity: &ProviderIdentityRef,
    kid: &str,
    user_presence_confirmed: bool,
) -> ProviderResult<ProviderExportedRoot> {
    let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
    let recipient_public = *recipient.public_key();
    let request_id = random_id();
    let delivery: SealedSecretDeliveryWire = call_json(
        dispatch,
        "exportRootKeySealed",
        &SealedRootExportPayload {
            identity,
            kid,
            request_id: &request_id,
            user_presence_confirmed,
        },
        vec![Buffer::from(recipient_public.to_vec())],
    )
    .await?;
    let identity_context = sealed_identity_context(identity);
    let binding = anp::sealed_handoff::identity_root_export_binding(
        &identity_context,
        kid,
        &recipient_public,
        &request_id,
        user_presence_confirmed,
    )
    .map_err(|_| provider_incompatible())?;
    let aad = validate_delivery(&delivery, &binding, &identity_context)?;
    let handoff = delivery.envelope.to_handoff()?;
    let plaintext = recipient
        .open(&handoff, SEALED_SECRET_INFO, &aad)
        .map_err(|_| provider_incompatible())?;
    if plaintext.is_empty() {
        return Err(provider_incompatible());
    }
    Ok(ProviderExportedRoot::new(plaintext.to_vec()))
}

async fn send_sealed_root_import(
    dispatch: &IdentityProviderDispatch,
    request: ProviderLegacyRootImportRequest,
) -> ProviderResult<ProviderLegacyRootImportOutcome> {
    let ProviderLegacyRootImportRequest {
        identity,
        evidence,
        encoding,
        root_key,
    } = request;
    let request_id = evidence.transfer_id.clone();
    let reply = call(
        dispatch,
        "prepareLegacyRootImport",
        &SealedRootImportPreparationPayload {
            identity: &identity,
            evidence: &evidence,
            encoding,
            request_id: &request_id,
        },
        Vec::new(),
    )
    .await?;
    let prepared: PreparedSealedImportWire = parse_json(&reply.payload_json)?;
    let recipient_public: [u8; 32] = exactly_one_buffer(reply.buffers)?
        .try_into()
        .map_err(|_| provider_incompatible())?;
    if prepared.offer.request_id != request_id {
        return Err(provider_incompatible());
    }
    let identity_context = sealed_identity_context(&identity);
    let shared_evidence = anp::sealed_handoff::SealedLegacyRootImportEvidence {
        transfer_id: evidence.transfer_id,
        source_did: evidence.source_did,
        target_did: evidence.target_did,
        sender_device_id: evidence.sender_device_id,
        recipient_device_id: evidence.recipient_device_id,
        recipient_agreement_kid: evidence.recipient_agreement_kid,
        root_kid: evidence.root_kid,
        checkpoint: anp::sealed_handoff::SealedDocumentCheckpoint {
            document_version: evidence.checkpoint.document_version,
            registry_version: evidence.checkpoint.registry_version,
            document_digest: evidence.checkpoint.document_digest,
        },
        accepted_at: evidence.accepted_at,
    };
    let shared_encoding = match encoding {
        ProviderPrivateKeyEncoding::Raw32 => anp::sealed_handoff::SealedPrivateKeyEncoding::Raw32,
        ProviderPrivateKeyEncoding::Pkcs8Der => {
            anp::sealed_handoff::SealedPrivateKeyEncoding::Pkcs8Der
        }
    };
    let binding = anp::sealed_handoff::identity_root_import_binding(
        &identity_context,
        &shared_evidence,
        shared_encoding,
        &recipient_public,
        &request_id,
    )
    .map_err(|_| provider_incompatible())?;
    let aad = validate_authorized_aad(
        &prepared.offer.authorization,
        &prepared.offer.aad,
        &binding,
        &identity_context,
    )?;
    let sealed = anp::sealed_handoff::SealedHandoff::seal(
        &recipient_public,
        SEALED_SECRET_INFO,
        &aad,
        &root_key,
    )
    .map_err(|_| provider_incompatible())?;
    let outcome: String = call_json(
        dispatch,
        "completeLegacyRootImport",
        &CompleteSealedImportPayload {
            session_id: &prepared.session_id,
            token: &prepared.offer.token,
            envelope: SealedSecretEnvelopeWire {
                protocol: SEALED_SECRET_PROTOCOL.to_owned(),
                suite: anp::sealed_handoff::SEALED_HANDOFF_SUITE.to_owned(),
                encapped_key: URL_SAFE_NO_PAD.encode(sealed.encapped_key()),
                ciphertext: URL_SAFE_NO_PAD.encode(sealed.ciphertext()),
            },
        },
        Vec::new(),
    )
    .await?;
    match outcome.as_str() {
        "pending" => Ok(ProviderLegacyRootImportOutcome::Pending),
        "active" => Ok(ProviderLegacyRootImportOutcome::Active),
        _ => Err(provider_incompatible()),
    }
}

async fn send_sealed_identity_material_import(
    dispatch: &IdentityProviderDispatch,
    request: ProviderIdentityMaterialImportRequest,
) -> ProviderResult<ProviderPublicIdentity> {
    let ProviderIdentityMaterialImportRequest {
        remote,
        did_wba,
        keys,
        request_id,
    } = request;
    let key_specs = keys
        .iter()
        .map(|key| SealedIdentityMaterialKeySpecWire {
            kid: &key.kid,
            purpose: key.purpose,
            encoding: key.encoding,
        })
        .collect::<Vec<_>>();
    let reply = call(
        dispatch,
        "prepareIdentityMaterialImport",
        &SealedIdentityImportPreparationPayload {
            remote: &remote,
            did_wba,
            keys: key_specs,
            request_id: &request_id,
        },
        Vec::new(),
    )
    .await?;
    let prepared: PreparedSealedIdentityImportWire = parse_json(&reply.payload_json)?;
    let recipient_public: [u8; 32] = exactly_one_buffer(reply.buffers)?
        .try_into()
        .map_err(|_| provider_incompatible())?;
    if prepared.offer.request_id != request_id
        || prepared.offer.item_aad.len() != keys.len()
        || remote
            .document
            .get("id")
            .and_then(serde_json::Value::as_str)
            != Some(prepared.offer.target.did.as_str())
    {
        return Err(provider_incompatible());
    }
    let target = sealed_identity_context(&prepared.offer.target);
    let shared_keys = keys
        .iter()
        .map(|key| anp::sealed_handoff::SealedIdentityMaterialKeySpec {
            kid: key.kid.clone(),
            purpose: shared_material_key_purpose(key.purpose),
            encoding: shared_private_key_encoding(key.encoding),
        })
        .collect::<Vec<_>>();
    let binding = anp::sealed_handoff::identity_material_import_binding(
        &target,
        &request_id,
        &recipient_public,
        did_wba,
        &remote.evidence.document_digest,
        remote.evidence.document_version,
        remote.evidence.registry_version,
        &shared_keys,
    )
    .map_err(|_| provider_incompatible())?;
    let authorization = anp::sealed_handoff::SealedAuthorizationContext {
        provider_instance_id: prepared.offer.authorization.provider_instance_id.clone(),
        parent_lease_id: prepared.offer.authorization.parent_lease_id.clone(),
        consumer: prepared.offer.authorization.consumer.clone(),
        capability: prepared.offer.authorization.capability.clone(),
        store_id: prepared.offer.authorization.store_id.clone(),
        expires_at: prepared.offer.authorization.expires_at,
    };
    let envelopes = keys
        .into_iter()
        .zip(prepared.offer.item_aad.iter())
        .enumerate()
        .map(|(index, (key, delivered_aad))| {
            let expected = anp::sealed_handoff::identity_material_import_item_aad(
                &authorization,
                &binding,
                &target,
                index,
                &key.kid,
            )
            .map_err(|_| provider_incompatible())?;
            let delivered = URL_SAFE_NO_PAD
                .decode(delivered_aad)
                .map_err(|_| provider_incompatible())?;
            if delivered != expected {
                return Err(provider_incompatible());
            }
            let sealed = anp::sealed_handoff::SealedHandoff::seal(
                &recipient_public,
                SEALED_SECRET_INFO,
                &expected,
                &key.secret,
            )
            .map_err(|_| provider_incompatible())?;
            Ok(SealedSecretEnvelopeWire {
                protocol: SEALED_SECRET_PROTOCOL.to_owned(),
                suite: anp::sealed_handoff::SEALED_HANDOFF_SUITE.to_owned(),
                encapped_key: URL_SAFE_NO_PAD.encode(sealed.encapped_key()),
                ciphertext: URL_SAFE_NO_PAD.encode(sealed.ciphertext()),
            })
        })
        .collect::<ProviderResult<Vec<_>>>()?;
    let public: WirePublicIdentity = call_json(
        dispatch,
        "completeIdentityMaterialImport",
        &CompleteSealedIdentityImportPayload {
            session_id: &prepared.session_id,
            token: &prepared.offer.token,
            envelopes,
        },
        Vec::new(),
    )
    .await?;
    let public = ProviderPublicIdentity::try_from(public)?;
    if public.reference != prepared.offer.target
        || public.state != ProviderIdentityState::Active
        || public.revision != remote.evidence.document_version
        || public.document != remote.document
        || public.did_wba != did_wba
    {
        return Err(provider_incompatible());
    }
    Ok(public)
}

fn shared_material_key_purpose(
    purpose: ProviderKeyPurpose,
) -> anp::sealed_handoff::SealedIdentityMaterialKeyPurpose {
    match purpose {
        ProviderKeyPurpose::RootControl => {
            anp::sealed_handoff::SealedIdentityMaterialKeyPurpose::RootControl
        }
        ProviderKeyPurpose::Authentication => {
            anp::sealed_handoff::SealedIdentityMaterialKeyPurpose::Authentication
        }
        ProviderKeyPurpose::DeviceAssertion => {
            anp::sealed_handoff::SealedIdentityMaterialKeyPurpose::DeviceAssertion
        }
        ProviderKeyPurpose::ApplicationAssertion => {
            anp::sealed_handoff::SealedIdentityMaterialKeyPurpose::ApplicationAssertion
        }
        ProviderKeyPurpose::KeyAgreement => {
            anp::sealed_handoff::SealedIdentityMaterialKeyPurpose::KeyAgreement
        }
    }
}

fn shared_private_key_encoding(
    encoding: ProviderPrivateKeyEncoding,
) -> anp::sealed_handoff::SealedPrivateKeyEncoding {
    match encoding {
        ProviderPrivateKeyEncoding::Raw32 => anp::sealed_handoff::SealedPrivateKeyEncoding::Raw32,
        ProviderPrivateKeyEncoding::Pkcs8Der => {
            anp::sealed_handoff::SealedPrivateKeyEncoding::Pkcs8Der
        }
    }
}

impl SealedSecretEnvelopeWire {
    fn to_handoff(&self) -> ProviderResult<anp::sealed_handoff::SealedHandoff> {
        if self.protocol != SEALED_SECRET_PROTOCOL
            || self.suite != anp::sealed_handoff::SEALED_HANDOFF_SUITE
        {
            return Err(provider_incompatible());
        }
        let encapped = URL_SAFE_NO_PAD
            .decode(&self.encapped_key)
            .map_err(|_| provider_incompatible())?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&self.ciphertext)
            .map_err(|_| provider_incompatible())?;
        anp::sealed_handoff::SealedHandoff::from_parts(&encapped, ciphertext)
            .map_err(|_| provider_incompatible())
    }
}

fn validate_delivery(
    delivery: &SealedSecretDeliveryWire,
    binding: &anp::sealed_handoff::SealedOperationBinding,
    identity: &anp::sealed_handoff::SealedIdentityContext,
) -> ProviderResult<Vec<u8>> {
    validate_authorized_aad(&delivery.authorization, &delivery.aad, binding, identity)
}

fn validate_authorized_aad(
    authorization: &AuthorizationContextWire,
    aad: &str,
    binding: &anp::sealed_handoff::SealedOperationBinding,
    identity: &anp::sealed_handoff::SealedIdentityContext,
) -> ProviderResult<Vec<u8>> {
    let expected = anp::sealed_handoff::identity_operation_aad(
        &anp::sealed_handoff::SealedAuthorizationContext {
            provider_instance_id: authorization.provider_instance_id.clone(),
            parent_lease_id: authorization.parent_lease_id.clone(),
            consumer: authorization.consumer.clone(),
            capability: authorization.capability.clone(),
            store_id: authorization.store_id.clone(),
            expires_at: authorization.expires_at,
        },
        binding,
        identity,
    )
    .map_err(|_| provider_incompatible())?;
    let delivered = URL_SAFE_NO_PAD
        .decode(aad)
        .map_err(|_| provider_incompatible())?;
    if delivered != expected {
        return Err(provider_incompatible());
    }
    Ok(expected)
}

fn sealed_identity_context(
    identity: &ProviderIdentityRef,
) -> anp::sealed_handoff::SealedIdentityContext {
    anp::sealed_handoff::SealedIdentityContext {
        store_id: identity.store_id.clone(),
        identity_id: identity.identity_id.clone(),
        did: identity.did.clone(),
    }
}

fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn provider_incompatible() -> IdentityProviderError {
    provider_error(IdentityProviderErrorCode::ProviderIncompatible, false)
}

async fn call_json<T: for<'de> Deserialize<'de>>(
    dispatch: &IdentityProviderDispatch,
    operation: &str,
    payload: &impl Serialize,
    buffers: Vec<Buffer>,
) -> ProviderResult<T> {
    let reply = call(dispatch, operation, payload, buffers).await?;
    if !reply.buffers.is_empty() {
        return Err(provider_error(
            IdentityProviderErrorCode::ProviderIncompatible,
            false,
        ));
    }
    parse_json(&reply.payload_json)
}

async fn call(
    dispatch: &IdentityProviderDispatch,
    operation: &str,
    payload: &impl Serialize,
    buffers: Vec<Buffer>,
) -> ProviderResult<NodeIdentityProviderReply> {
    let payload_json = serde_json::to_string(payload)
        .map_err(|_| provider_error(IdentityProviderErrorCode::InvalidRequest, false))?;
    let promise = dispatch
        .call_async_catch((NodeIdentityProviderCall {
            operation: operation.to_owned(),
            payload_json,
            buffers,
        },))
        .await
        .map_err(|_| provider_error(IdentityProviderErrorCode::ProviderUnavailable, true))?;
    let reply = promise
        .await
        .map_err(|_| provider_error(IdentityProviderErrorCode::ProviderUnavailable, true))?;
    if !reply.ok {
        return Err(provider_error(
            map_error_code(reply.error_code.as_deref()),
            reply.retryable.unwrap_or(false),
        ));
    }
    if reply.error_code.is_some() {
        return Err(provider_error(
            IdentityProviderErrorCode::ProviderIncompatible,
            false,
        ));
    }
    Ok(reply)
}

fn parse_json<T: for<'de> Deserialize<'de>>(raw: &str) -> ProviderResult<T> {
    serde_json::from_str(raw)
        .map_err(|_| provider_error(IdentityProviderErrorCode::ProviderIncompatible, false))
}

fn exactly_one_buffer(buffers: Vec<Buffer>) -> ProviderResult<Vec<u8>> {
    let [buffer]: [Buffer; 1] = buffers
        .try_into()
        .map_err(|_| provider_error(IdentityProviderErrorCode::ProviderIncompatible, false))?;
    Ok(buffer.to_vec())
}

fn selector_kid(selector: &ProviderKeySelector) -> Option<&str> {
    match selector {
        ProviderKeySelector::Default => None,
        ProviderKeySelector::Kid(kid) => Some(kid),
    }
}

fn provider_error(code: IdentityProviderErrorCode, retryable: bool) -> IdentityProviderError {
    IdentityProviderError::new(code, retryable)
}

fn map_error_code(code: Option<&str>) -> IdentityProviderErrorCode {
    match code.unwrap_or_default() {
        "invalid_request" => IdentityProviderErrorCode::InvalidRequest,
        "invalid_state" => IdentityProviderErrorCode::InvalidState,
        "store_not_found" => IdentityProviderErrorCode::StoreNotFound,
        "provider_unavailable" => IdentityProviderErrorCode::ProviderUnavailable,
        "provider_incompatible" => IdentityProviderErrorCode::ProviderIncompatible,
        "provider_disposed" => IdentityProviderErrorCode::ProviderDisposed,
        "root_key_mismatch" => IdentityProviderErrorCode::RootKeyMismatch,
        "corrupt_state" => IdentityProviderErrorCode::CorruptState,
        "identity_not_found" => IdentityProviderErrorCode::IdentityNotFound,
        "identity_already_exists" => IdentityProviderErrorCode::IdentityAlreadyExists,
        "key_not_found" => IdentityProviderErrorCode::KeyNotFound,
        "key_unavailable" => IdentityProviderErrorCode::KeyUnavailable,
        "key_purpose_violation" => IdentityProviderErrorCode::KeyPurposeViolation,
        "ambiguous_key" => IdentityProviderErrorCode::AmbiguousKey,
        "verification_failed" => IdentityProviderErrorCode::VerificationFailed,
        "pending_document_change" => IdentityProviderErrorCode::PendingDocumentChange,
        "document_change_not_found" => IdentityProviderErrorCode::DocumentChangeNotFound,
        "invalid_document_change_state" => IdentityProviderErrorCode::InvalidDocumentChangeState,
        "conflict" => IdentityProviderErrorCode::Conflict,
        "capability_unavailable" | "capability_forbidden" => {
            IdentityProviderErrorCode::CapabilityUnavailable
        }
        "request_cancelled" => IdentityProviderErrorCode::RequestCancelled,
        "request_timeout" => IdentityProviderErrorCode::RequestTimeout,
        "storage" => IdentityProviderErrorCode::Storage,
        _ => IdentityProviderErrorCode::Internal,
    }
}

pub(crate) fn safe_provider_error(error: IdentityProviderError) -> crate::error::SafeError {
    match error.code {
        IdentityProviderErrorCode::ProviderIncompatible
        | IdentityProviderErrorCode::CorruptState
        | IdentityProviderErrorCode::RootKeyMismatch => crate::error::SafeError::new(
            "provider_incompatible",
            "The identity provider is incompatible.",
            false,
        ),
        IdentityProviderErrorCode::ProviderDisposed => crate::error::SafeError::new(
            "provider_disposed",
            "The identity provider lease has been disposed.",
            false,
        ),
        _ => crate::error::SafeError::new(
            "provider_unavailable",
            "The identity provider is unavailable.",
            error.retryable,
        ),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireStoreInfo {
    store_id: String,
    schema_compatible: bool,
    identity_count: usize,
    health: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePublicIdentity {
    reference: ProviderIdentityRef,
    state: ProviderIdentityState,
    revision: u64,
    document: serde_json::Value,
    active_keys: Vec<WirePublicKey>,
    capabilities: WireCapabilities,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePublicKey {
    kid: String,
    algorithm: ProviderKeyAlgorithm,
    purposes: Vec<ProviderKeyPurpose>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireCapabilities {
    did_wba: bool,
}

#[derive(Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum WireDocumentChangeOutcome {
    ReadyForPublication,
    PublicationUncertain,
    Committed { identity: WirePublicIdentity },
    Aborted,
}

impl TryFrom<WirePublicIdentity> for ProviderPublicIdentity {
    type Error = IdentityProviderError;

    fn try_from(value: WirePublicIdentity) -> Result<Self, Self::Error> {
        if value.active_keys.iter().any(|key| key.purposes.is_empty()) {
            return Err(provider_error(
                IdentityProviderErrorCode::ProviderIncompatible,
                false,
            ));
        }
        Ok(Self {
            reference: value.reference,
            state: value.state,
            revision: value.revision,
            document: value.document,
            active_keys: value
                .active_keys
                .into_iter()
                .map(|key| ProviderPublicKey {
                    kid: key.kid,
                    algorithm: key.algorithm,
                    purposes: key.purposes,
                })
                .collect(),
            did_wba: value.capabilities.did_wba,
        })
    }
}

impl TryFrom<WireDocumentChangeOutcome> for ProviderDocumentChangeOutcome {
    type Error = IdentityProviderError;

    fn try_from(value: WireDocumentChangeOutcome) -> Result<Self, Self::Error> {
        Ok(match value {
            WireDocumentChangeOutcome::ReadyForPublication => Self::ReadyForPublication,
            WireDocumentChangeOutcome::PublicationUncertain => Self::PublicationUncertain,
            WireDocumentChangeOutcome::Committed { identity } => Self::Committed {
                identity: identity.try_into()?,
            },
            WireDocumentChangeOutcome::Aborted => Self::Aborted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_change_outcome_accepts_public_facade_identity_shape() {
        let wire: WireDocumentChangeOutcome = serde_json::from_value(serde_json::json!({
            "outcome": "committed",
            "identity": {
                "reference": {
                    "storeId": "store-1",
                    "identityId": "identity-1",
                    "did": "did:wba:example.test:alice",
                },
                "state": "active",
                "revision": 2,
                "document": { "id": "did:wba:example.test:alice" },
                "activeKeys": [{
                    "kid": "did:wba:example.test:alice#device",
                    "algorithm": "ed25519",
                    "purposes": ["authentication", "device_assertion"],
                }],
                "capabilities": { "didWba": true },
            },
        }))
        .unwrap();

        let outcome = ProviderDocumentChangeOutcome::try_from(wire).unwrap();
        let ProviderDocumentChangeOutcome::Committed { identity } = outcome else {
            panic!("expected committed document change outcome");
        };
        assert_eq!(identity.revision, 2);
        assert!(identity.did_wba);
        assert_eq!(identity.active_keys.len(), 1);
    }

    #[test]
    fn sealed_delivery_validation_rejects_aad_substitution() {
        let identity = ProviderIdentityRef {
            store_id: "store-1".to_owned(),
            identity_id: "identity-1".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
        };
        let identity_context = sealed_identity_context(&identity);
        let kid = format!("{}#agreement", identity.did);
        let peer_public = [0x31; 32];
        let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
        let request_id = "request-1";
        let binding = anp::sealed_handoff::identity_ecdh_binding(
            &identity_context,
            &kid,
            &peer_public,
            recipient.public_key(),
            request_id,
        )
        .unwrap();
        let authorization = anp::sealed_handoff::SealedAuthorizationContext {
            provider_instance_id: "provider-1".to_owned(),
            parent_lease_id: "lease-1".to_owned(),
            consumer: "dsh-awiki".to_owned(),
            capability: anp::sealed_handoff::IDENTITY_ECDH_CAPABILITY.to_owned(),
            store_id: identity.store_id.clone(),
            expires_at: 2_000_000_000,
        };
        let aad = anp::sealed_handoff::identity_operation_aad(
            &authorization,
            &binding,
            &identity_context,
        )
        .unwrap();
        let handoff = anp::sealed_handoff::SealedHandoff::seal(
            recipient.public_key(),
            SEALED_SECRET_INFO,
            &aad,
            &[0x42; 32],
        )
        .unwrap();
        let mut delivery = SealedSecretDeliveryWire {
            envelope: SealedSecretEnvelopeWire {
                protocol: SEALED_SECRET_PROTOCOL.to_owned(),
                suite: anp::sealed_handoff::SEALED_HANDOFF_SUITE.to_owned(),
                encapped_key: URL_SAFE_NO_PAD.encode(handoff.encapped_key()),
                ciphertext: URL_SAFE_NO_PAD.encode(handoff.ciphertext()),
            },
            authorization: AuthorizationContextWire {
                provider_instance_id: authorization.provider_instance_id,
                parent_lease_id: authorization.parent_lease_id,
                consumer: authorization.consumer,
                capability: authorization.capability,
                store_id: authorization.store_id,
                expires_at: authorization.expires_at,
            },
            aad: URL_SAFE_NO_PAD.encode(&aad),
        };
        assert_eq!(
            validate_delivery(&delivery, &binding, &identity_context).unwrap(),
            aad
        );

        delivery.aad.push('A');
        assert_eq!(
            validate_delivery(&delivery, &binding, &identity_context)
                .unwrap_err()
                .code,
            IdentityProviderErrorCode::ProviderIncompatible
        );
    }

    #[test]
    fn sealed_root_delivery_uses_the_shared_operation_contract() {
        let identity = ProviderIdentityRef {
            store_id: "store-1".to_owned(),
            identity_id: "identity-1".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
        };
        let identity_context = sealed_identity_context(&identity);
        let kid = format!("{}#root", identity.did);
        let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
        let request_id = "root-export-1";
        let binding = anp::sealed_handoff::identity_root_export_binding(
            &identity_context,
            &kid,
            recipient.public_key(),
            request_id,
            true,
        )
        .unwrap();
        let authorization = anp::sealed_handoff::SealedAuthorizationContext {
            provider_instance_id: "provider-1".to_owned(),
            parent_lease_id: "lease-1".to_owned(),
            consumer: "dsh-awiki".to_owned(),
            capability: anp::sealed_handoff::IDENTITY_ROOT_EXPORT_CAPABILITY.to_owned(),
            store_id: identity.store_id.clone(),
            expires_at: 2_000_000_000,
        };
        let aad = anp::sealed_handoff::identity_operation_aad(
            &authorization,
            &binding,
            &identity_context,
        )
        .unwrap();
        let delivery = SealedSecretDeliveryWire {
            envelope: SealedSecretEnvelopeWire {
                protocol: SEALED_SECRET_PROTOCOL.to_owned(),
                suite: anp::sealed_handoff::SEALED_HANDOFF_SUITE.to_owned(),
                encapped_key: URL_SAFE_NO_PAD.encode([0_u8; 32]),
                ciphertext: URL_SAFE_NO_PAD.encode([0_u8; 32]),
            },
            authorization: AuthorizationContextWire {
                provider_instance_id: authorization.provider_instance_id,
                parent_lease_id: authorization.parent_lease_id,
                consumer: authorization.consumer,
                capability: authorization.capability,
                store_id: authorization.store_id,
                expires_at: authorization.expires_at,
            },
            aad: URL_SAFE_NO_PAD.encode(&aad),
        };
        assert_eq!(
            validate_delivery(&delivery, &binding, &identity_context).unwrap(),
            aad
        );
    }

    #[test]
    fn pending_root_proof_wire_uses_the_provider_kid_shape() {
        let identity = ProviderIdentityRef {
            store_id: "store-1".to_owned(),
            identity_id: "identity-1".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
        };
        let document = serde_json::json!({"type": "root-possession"});
        let payload = serde_json::to_value(PendingRootProofPayload {
            identity: &identity,
            request: PendingRootProofRequestWire {
                kid: Some("did:wba:example.test:alice#root"),
                document: &document,
                issuer_did: &identity.did,
                created: Some("2026-08-23T00:00:00Z"),
            },
        })
        .unwrap();

        assert_eq!(
            payload["request"],
            serde_json::json!({
                "kid": "did:wba:example.test:alice#root",
                "document": document,
                "issuerDid": identity.did,
                "created": "2026-08-23T00:00:00Z",
            })
        );
        assert!(payload["request"].get("key").is_none());
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePreparedHttpSignature {
    binding_digest: String,
    kid: String,
    header_patch: Vec<ProviderHttpHeader>,
}
