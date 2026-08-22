use std::sync::Arc;

use im_core::provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderCreateIdentityRequest, ProviderDeviceEnrollmentRequest, ProviderDocumentChangeOutcome,
    ProviderDocumentChangePhase, ProviderDocumentChangeSession, ProviderEnrollmentProposal,
    ProviderEnrollmentSession, ProviderExactHttpRequest, ProviderHostStatus, ProviderHttpHeader,
    ProviderIdentityDescriptor, ProviderIdentityRef, ProviderIdentityState,
    ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose, ProviderKeySelector,
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
use serde::{Deserialize, Serialize};

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
        _request: ProviderKeyAgreementRequest,
    ) -> ProviderResult<ProviderSharedSecret> {
        // The current provider response omits the authorization/AAD context
        // needed to authenticate and open the HPKE ciphertext. Never downgrade
        // this operation to a raw shared-secret bridge.
        Err(provider_error(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
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
        _peer_public: [u8; 32],
    ) -> ProviderResult<ProviderSharedSecret> {
        // The sealed enrollment response has the same unresolved AAD delivery
        // requirement as active-identity ECDH. Keep it unavailable instead of
        // accepting a raw shared secret through TypeScript.
        Err(provider_error(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
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
