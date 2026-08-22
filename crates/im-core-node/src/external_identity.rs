use std::sync::Arc;

use im_core::provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderExactHttpRequest, ProviderHttpHeader, ProviderIdentityDescriptor, ProviderIdentityRef,
    ProviderIdentityState, ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose,
    ProviderKeySelector, ProviderOriginProofRequest, ProviderPreparedHttpSignature,
    ProviderPublicIdentity, ProviderPublicKey, ProviderResult, ProviderSharedSecret,
    ProviderSignRequest, ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose,
    ProviderStoreInfo,
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
    public_cache: tokio::sync::RwLock<Option<ProviderPublicIdentity>>,
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
            public_cache: tokio::sync::RwLock::new(None),
        };
        let public = session.public_identity().await?;
        if public.reference != *identity {
            return Err(provider_error(IdentityProviderErrorCode::Conflict, false));
        }
        Ok(Arc::new(session))
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePreparedHttpSignature {
    binding_digest: String,
    kid: String,
    header_patch: Vec<ProviderHttpHeader>,
}
