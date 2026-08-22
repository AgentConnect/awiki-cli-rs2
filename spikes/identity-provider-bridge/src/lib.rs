use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use anp::proof::{
    complete_rfc9421_origin_proof, prepare_rfc9421_origin_proof,
    Rfc9421OriginProofGenerationOptions,
};
use ed25519_dalek::VerifyingKey;
use napi::{
    bindgen_prelude::{Buffer, Promise},
    threadsafe_function::ThreadsafeFunction,
    Error, Result, Status,
};
use napi_derive::napi;

#[napi(object)]
pub struct OriginProofRequest {
    pub method: String,
    pub meta_json: String,
    pub body_json: String,
    pub public_key: Buffer,
    pub key_id: String,
}

#[derive(Default)]
struct BridgeState {
    lease_revoked: AtomicBool,
    host_shutdown: AtomicBool,
    cancellation_epoch: AtomicU64,
}

#[napi]
pub struct IdentityProviderBridge {
    state: Arc<BridgeState>,
}

#[napi]
impl IdentityProviderBridge {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(BridgeState::default()),
        }
    }

    #[napi]
    pub fn revoke_lease(&self) {
        self.state.lease_revoked.store(true, Ordering::Release);
    }

    #[napi]
    pub fn cancel_in_flight(&self) {
        self.state.cancellation_epoch.fetch_add(1, Ordering::AcqRel);
    }

    #[napi]
    pub fn shutdown(&self) {
        self.state.host_shutdown.store(true, Ordering::Release);
    }

    #[napi]
    pub async fn sign_origin_proof(
        &self,
        request: OriginProofRequest,
        provider_sign: ThreadsafeFunction<Buffer, Promise<Buffer>>,
        timeout_ms: u32,
    ) -> Result<String> {
        check_available(&self.state)?;
        let cancellation_epoch = self.state.cancellation_epoch.load(Ordering::Acquire);
        let prepared = prepare(request)?;
        let signing_input = Buffer::from(prepared.signing_input().to_vec());

        let signature = tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms)), async {
            let promise = provider_sign.call_async(Ok(signing_input)).await?;
            promise.await
        })
        .await
        .map_err(|_| bridge_error("provider_timeout", "identity provider timed out"))?
        .map_err(|_| bridge_error("provider_error", "identity provider rejected the request"))?;

        check_available(&self.state)?;
        if self.state.cancellation_epoch.load(Ordering::Acquire) != cancellation_epoch {
            return Err(bridge_error(
                "request_cancelled",
                "identity provider request was cancelled",
            ));
        }

        let proof = complete_rfc9421_origin_proof(prepared, signature.as_ref()).map_err(|_| {
            bridge_error(
                "invalid_provider_signature",
                "identity provider returned an invalid signature",
            )
        })?;
        serde_json::to_string(&proof)
            .map_err(|_| bridge_error("internal", "failed to serialize Origin Proof"))
    }
}

fn prepare(request: OriginProofRequest) -> Result<anp::proof::PreparedRfc9421OriginProof> {
    let public_key: [u8; 32] = request
        .public_key
        .as_ref()
        .try_into()
        .map_err(|_| bridge_error("invalid_request", "Ed25519 public key must be 32 bytes"))?;
    let public_key = VerifyingKey::from_bytes(&public_key)
        .map(anp::PublicKeyMaterial::Ed25519)
        .map_err(|_| bridge_error("invalid_request", "Ed25519 public key is invalid"))?;
    let meta = serde_json::from_str(&request.meta_json)
        .map_err(|_| bridge_error("invalid_request", "meta JSON is invalid"))?;
    let body = serde_json::from_str(&request.body_json)
        .map_err(|_| bridge_error("invalid_request", "body JSON is invalid"))?;
    prepare_rfc9421_origin_proof(
        &request.method,
        &meta,
        &body,
        &public_key,
        &request.key_id,
        Rfc9421OriginProofGenerationOptions::default(),
    )
    .map_err(|_| bridge_error("invalid_request", "Origin Proof input is invalid"))
}

fn check_available(state: &BridgeState) -> Result<()> {
    if state.host_shutdown.load(Ordering::Acquire) {
        return Err(bridge_error(
            "host_shutdown",
            "identity provider host is shutting down",
        ));
    }
    if state.lease_revoked.load(Ordering::Acquire) {
        return Err(bridge_error(
            "lease_revoked",
            "identity provider lease was revoked",
        ));
    }
    Ok(())
}

fn bridge_error(code: &str, message: &str) -> Error {
    Error::new(Status::GenericFailure, format!("{code}: {message}"))
}
