use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use im_core::provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCreateIdentityRequest, ProviderDeviceEnrollmentRequest, ProviderDocumentChangeOutcome,
    ProviderDocumentChangePhase, ProviderDocumentChangeSession, ProviderEnrollmentProposal,
    ProviderEnrollmentProposalKind, ProviderEnrollmentSession, ProviderExactHttpRequest,
    ProviderExportedRoot, ProviderHostStatus, ProviderHttpHeader, ProviderIdentityDescriptor,
    ProviderIdentityRef, ProviderIdentityState, ProviderKeyAgreementRequest, ProviderKeyAlgorithm,
    ProviderKeyPurpose, ProviderKeySelector, ProviderLegacyRootExportRequest,
    ProviderObjectProofRequest, ProviderOriginProofRequest, ProviderPreparedDocumentChange,
    ProviderPreparedHttpSignature, ProviderPublicIdentity, ProviderPublicKey,
    ProviderPublicationAttempt, ProviderPublicationResult, ProviderRequestSigningEnrollmentRequest,
    ProviderResult, ProviderSharedSecret, ProviderSignRequest, ProviderSignature,
    ProviderSignedOriginProof, ProviderSigningPurpose, ProviderStoreInfo,
    ProviderVerifiedRemoteDocument,
};
use napi::bindgen_prelude::{Buffer, Promise};
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SEALED_SECRET_PROTOCOL: &str = "anp-sealed-secret/1";
const SEALED_SECRET_INFO: &[u8] = b"anp.identity.sealed-secret.v1";
const IDENTITY_ECDH_CAPABILITY: &str = "IDENTITY_ECDH_SEALED";
const ECDH_SEALED_OPERATION: &str = "ecdh_sealed";
const ENROLLMENT_ECDH_SEALED_OPERATION: &str = "enrollment_ecdh_sealed";
const ROOT_EXPORT_CAPABILITY: &str = "AWIKI_LEGACY_ROOT_TRANSFER_V1";
const ROOT_EXPORT_SEALED_OPERATION: &str = "export_root_key_sealed";

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedSecretDeliveryWire {
    envelope: SealedSecretEnvelopeWire,
    authorization: AuthorizationContextWire,
    aad: String,
}

#[derive(Deserialize)]
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
        let outcome = call_json(
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
        let outcome = call_json(
            &self.dispatch,
            "documentChangeReconcile",
            &ReconcileDocumentChangePayload {
                session_id: &self.session_id,
                observation: &observation,
            },
            Vec::new(),
        )
        .await?;
        if matches!(outcome, ProviderDocumentChangeOutcome::Committed { .. }) {
            *self.public_cache.write().await = None;
        }
        Ok(outcome)
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
    let operation_input_digest = match enrollment {
        Some((_, enrollment_id)) => sealed_enrollment_operation_digest(
            identity,
            enrollment_id,
            kid,
            &peer_public,
            &recipient_public,
            &request_id,
        )?,
        None => sealed_key_agreement_operation_digest(
            identity,
            kid,
            &peer_public,
            &recipient_public,
            &request_id,
        )?,
    };
    let aad = validate_delivery(
        &delivery,
        operation,
        IDENTITY_ECDH_CAPABILITY,
        identity,
        kid,
        &request_id,
        &operation_input_digest,
        &recipient_public,
    )?;
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
    let operation_input_digest = sealed_root_export_operation_digest(
        identity,
        kid,
        &recipient_public,
        &request_id,
        user_presence_confirmed,
    )?;
    let aad = validate_delivery(
        &delivery,
        ROOT_EXPORT_SEALED_OPERATION,
        ROOT_EXPORT_CAPABILITY,
        identity,
        kid,
        &request_id,
        &operation_input_digest,
        &recipient_public,
    )?;
    let handoff = delivery.envelope.to_handoff()?;
    let plaintext = recipient
        .open(&handoff, SEALED_SECRET_INFO, &aad)
        .map_err(|_| provider_incompatible())?;
    if plaintext.is_empty() {
        return Err(provider_incompatible());
    }
    Ok(ProviderExportedRoot::new(plaintext.to_vec()))
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

#[allow(clippy::too_many_arguments)]
fn validate_delivery(
    delivery: &SealedSecretDeliveryWire,
    operation: &str,
    expected_capability: &str,
    identity: &ProviderIdentityRef,
    kid: &str,
    request_id: &str,
    operation_input_digest: &str,
    recipient_public_key: &[u8; 32],
) -> ProviderResult<Vec<u8>> {
    let authorization = &delivery.authorization;
    if authorization.provider_instance_id.trim().is_empty()
        || authorization.parent_lease_id.trim().is_empty()
        || authorization.consumer.trim().is_empty()
        || authorization.capability != expected_capability
        || authorization.store_id != identity.store_id
        || authorization.expires_at <= 0
    {
        return Err(provider_incompatible());
    }
    #[derive(Serialize)]
    struct Aad<'a> {
        protocol_version: &'static str,
        operation: &'a str,
        provider_instance_id: &'a str,
        parent_lease_id: &'a str,
        consumer: &'a str,
        capability: &'a str,
        store_id: &'a str,
        identity_id: &'a str,
        kid: &'a str,
        request_id: &'a str,
        recipient_public_key_digest: String,
        canonical_request_digest: &'a str,
    }
    let expected = serde_json_canonicalizer::to_vec(&Aad {
        protocol_version: SEALED_SECRET_PROTOCOL,
        operation,
        provider_instance_id: &authorization.provider_instance_id,
        parent_lease_id: &authorization.parent_lease_id,
        consumer: &authorization.consumer,
        capability: &authorization.capability,
        store_id: &identity.store_id,
        identity_id: &identity.identity_id,
        kid,
        request_id,
        recipient_public_key_digest: recipient_public_key_digest(recipient_public_key),
        canonical_request_digest: operation_input_digest,
    })
    .map_err(|_| provider_incompatible())?;
    let delivered = URL_SAFE_NO_PAD
        .decode(&delivery.aad)
        .map_err(|_| provider_incompatible())?;
    if delivered != expected {
        return Err(provider_incompatible());
    }
    Ok(expected)
}

fn sealed_key_agreement_operation_digest(
    identity: &ProviderIdentityRef,
    kid: &str,
    peer_public: &[u8; 32],
    recipient_public_key: &[u8; 32],
    request_id: &str,
) -> ProviderResult<String> {
    #[derive(Serialize)]
    struct DigestInput<'a> {
        protocol: &'static str,
        operation: &'static str,
        store_id: &'a str,
        identity_id: &'a str,
        did: &'a str,
        kid: &'a str,
        algorithm: &'static str,
        peer_public_b64u: String,
        recipient_public_key_digest: String,
        request_id: &'a str,
    }
    sealed_operation_digest(&DigestInput {
        protocol: SEALED_SECRET_PROTOCOL,
        operation: ECDH_SEALED_OPERATION,
        store_id: &identity.store_id,
        identity_id: &identity.identity_id,
        did: &identity.did,
        kid,
        algorithm: "X25519",
        peer_public_b64u: URL_SAFE_NO_PAD.encode(peer_public),
        recipient_public_key_digest: recipient_public_key_digest(recipient_public_key),
        request_id,
    })
}

fn sealed_enrollment_operation_digest(
    identity: &ProviderIdentityRef,
    enrollment_id: &str,
    kid: &str,
    peer_public: &[u8; 32],
    recipient_public_key: &[u8; 32],
    request_id: &str,
) -> ProviderResult<String> {
    #[derive(Serialize)]
    struct DigestInput<'a> {
        protocol: &'static str,
        operation: &'static str,
        store_id: &'a str,
        identity_id: &'a str,
        did: &'a str,
        enrollment_id: &'a str,
        kid: &'a str,
        algorithm: &'static str,
        peer_public_b64u: String,
        recipient_public_key_digest: String,
        request_id: &'a str,
    }
    sealed_operation_digest(&DigestInput {
        protocol: SEALED_SECRET_PROTOCOL,
        operation: ENROLLMENT_ECDH_SEALED_OPERATION,
        store_id: &identity.store_id,
        identity_id: &identity.identity_id,
        did: &identity.did,
        enrollment_id,
        kid,
        algorithm: "X25519",
        peer_public_b64u: URL_SAFE_NO_PAD.encode(peer_public),
        recipient_public_key_digest: recipient_public_key_digest(recipient_public_key),
        request_id,
    })
}

fn sealed_root_export_operation_digest(
    identity: &ProviderIdentityRef,
    kid: &str,
    recipient_public_key: &[u8; 32],
    request_id: &str,
    user_presence_confirmed: bool,
) -> ProviderResult<String> {
    #[derive(Serialize)]
    struct DigestInput<'a> {
        protocol: &'static str,
        operation: &'static str,
        store_id: &'a str,
        identity_id: &'a str,
        did: &'a str,
        kid: &'a str,
        recipient_public_key_digest: String,
        request_id: &'a str,
        user_presence_confirmed: bool,
    }
    sealed_operation_digest(&DigestInput {
        protocol: SEALED_SECRET_PROTOCOL,
        operation: ROOT_EXPORT_SEALED_OPERATION,
        store_id: &identity.store_id,
        identity_id: &identity.identity_id,
        did: &identity.did,
        kid,
        recipient_public_key_digest: recipient_public_key_digest(recipient_public_key),
        request_id,
        user_presence_confirmed,
    })
}

fn sealed_operation_digest(value: &impl Serialize) -> ProviderResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(value).map_err(|_| provider_incompatible())?;
    let mut digest = Sha256::new();
    digest.update(b"anp.identity.sealed-operation.v1\0");
    digest.update(canonical);
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(digest.finalize())
    ))
}

fn recipient_public_key_digest(public_key: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"anp.identity.recipient-public-key.v1\0");
    digest.update(public_key);
    format!("sha256:{}", URL_SAFE_NO_PAD.encode(digest.finalize()))
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

fn safe_provider_error(error: IdentityProviderError) -> crate::error::SafeError {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_ecdh_contract_matches_anp_identity_and_rejects_aad_substitution() {
        let identity = ProviderIdentityRef {
            store_id: "store-1".to_owned(),
            identity_id: "identity-1".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
        };
        let anp_identity = anp_identity::IdentityRef {
            store_id: identity.store_id.clone(),
            identity_id: identity.identity_id.clone(),
            did: identity.did.clone(),
        };
        let kid = format!("{}#agreement", identity.did);
        let peer_public = [0x31; 32];
        let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
        let request_id = "request-1";
        let binding = anp_identity::host::sealed_key_agreement_binding(
            &anp_identity,
            &kid,
            &peer_public,
            recipient.public_key(),
            request_id,
        )
        .unwrap();
        assert_eq!(
            sealed_key_agreement_operation_digest(
                &identity,
                &kid,
                &peer_public,
                recipient.public_key(),
                request_id,
            )
            .unwrap(),
            binding.operation_input_digest
        );
        let authorization = anp_identity::host::IssuedAuthorizationContext {
            provider_instance_id: "provider-1".to_owned(),
            parent_lease_id: "lease-1".to_owned(),
            consumer: "dsh-awiki".to_owned(),
            capability: IDENTITY_ECDH_CAPABILITY.to_owned(),
            store_id: identity.store_id.clone(),
            expires_at: 2_000_000_000,
        };
        let aad = anp_identity::host::sealed_operation_aad(&authorization, &binding, &anp_identity)
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
            validate_delivery(
                &delivery,
                ECDH_SEALED_OPERATION,
                IDENTITY_ECDH_CAPABILITY,
                &identity,
                &kid,
                request_id,
                &binding.operation_input_digest,
                recipient.public_key(),
            )
            .unwrap(),
            aad
        );

        delivery.aad.push('A');
        assert_eq!(
            validate_delivery(
                &delivery,
                ECDH_SEALED_OPERATION,
                IDENTITY_ECDH_CAPABILITY,
                &identity,
                &kid,
                request_id,
                &binding.operation_input_digest,
                recipient.public_key(),
            )
            .unwrap_err()
            .code,
            IdentityProviderErrorCode::ProviderIncompatible
        );
    }

    #[test]
    fn sealed_root_export_contract_matches_anp_identity() {
        let identity = ProviderIdentityRef {
            store_id: "store-1".to_owned(),
            identity_id: "identity-1".to_owned(),
            did: "did:wba:example.test:alice".to_owned(),
        };
        let anp_identity = anp_identity::IdentityRef {
            store_id: identity.store_id.clone(),
            identity_id: identity.identity_id.clone(),
            did: identity.did.clone(),
        };
        let kid = format!("{}#root", identity.did);
        let recipient = anp::sealed_handoff::SealedHandoffRecipient::generate();
        let request_id = "root-export-1";
        let binding = anp_identity::host::sealed_root_export_binding(
            &anp_identity,
            &kid,
            recipient.public_key(),
            request_id,
            true,
        )
        .unwrap();
        assert_eq!(
            sealed_root_export_operation_digest(
                &identity,
                &kid,
                recipient.public_key(),
                request_id,
                true,
            )
            .unwrap(),
            binding.operation_input_digest
        );
        let authorization = anp_identity::host::IssuedAuthorizationContext {
            provider_instance_id: "provider-1".to_owned(),
            parent_lease_id: "lease-1".to_owned(),
            consumer: "dsh-awiki".to_owned(),
            capability: ROOT_EXPORT_CAPABILITY.to_owned(),
            store_id: identity.store_id.clone(),
            expires_at: 2_000_000_000,
        };
        let aad = anp_identity::host::sealed_operation_aad(&authorization, &binding, &anp_identity)
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
            validate_delivery(
                &delivery,
                ROOT_EXPORT_SEALED_OPERATION,
                ROOT_EXPORT_CAPABILITY,
                &identity,
                &kid,
                request_id,
                &binding.operation_input_digest,
                recipient.public_key(),
            )
            .unwrap(),
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
