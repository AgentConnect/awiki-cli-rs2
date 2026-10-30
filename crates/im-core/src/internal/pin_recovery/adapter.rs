//! Pinned, unmodified Juicebox SDK with exact transport retry and receipts.
use super::{
    crypto, failure, http,
    pending::{ExactRequest, Operation, Session, Store},
    wire,
};
use async_trait::async_trait;
use juicebox_networking::http as jb_http;
use juicebox_sdk::{AuthToken, RealmId};
use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
};

struct Inner {
    core: crate::ImCore,
    store: Store,
    state: tokio::sync::Mutex<State>,
    registration_barrier: tokio::sync::Notify,
}
struct State {
    session: Session,
    operation: Operation,
    registration_count: usize,
    failed: Option<String>,
}
#[derive(Clone)]
pub(crate) struct Adapter(Arc<Inner>);
impl Adapter {
    pub fn new(
        core: &crate::ImCore,
        store: &Store,
        session: Session,
        operation: Operation,
    ) -> Self {
        Self(Arc::new(Inner {
            core: core.clone(),
            store: store.clone(),
            state: tokio::sync::Mutex::new(State {
                session,
                operation,
                registration_count: 0,
                failed: None,
            }),
            registration_barrier: Default::default(),
        }))
    }
    pub async fn snapshot(&self) -> (Session, Operation) {
        let s = self.0.state.lock().await;
        (s.session.clone(), s.operation.clone())
    }
    pub async fn run_register(
        &self,
        pin: &crate::pin_recovery::SecretPin,
        key: &[u8],
    ) -> crate::ImResult<()> {
        let client = self.client().await?;
        let info = {
            let s = self.0.state.lock().await;
            crypto::user_info(
                &s.session,
                s.operation
                    .remote
                    .as_ref()
                    .ok_or_else(|| failure("corrupt_pending", false))?,
            )?
        };
        let result = client
            .register(
                &juicebox_sdk::Pin::from(pin.bytes().to_vec()),
                &juicebox_sdk::UserSecret::from(key.to_vec()),
                &juicebox_sdk::UserInfo::from(info),
                juicebox_sdk::Policy { num_guesses: 10 },
            )
            .await
            .map_err(|e| match e {
                juicebox_sdk::RegisterError::InvalidAuth => {
                    failure("phone_verification_required", false)
                }
                juicebox_sdk::RegisterError::RateLimitExceeded => failure("rate_limited", true),
                juicebox_sdk::RegisterError::Transient => failure("realm_unavailable", true),
                _ => failure("registration_failed", false),
            });
        match result {
            Ok(()) => Ok(()),
            Err(fallback) => Err(self.recorded_error(fallback).await),
        }
    }
    pub async fn run_recover(
        &self,
        pin: &crate::pin_recovery::SecretPin,
    ) -> crate::ImResult<zeroize::Zeroizing<Vec<u8>>> {
        let client = self.client().await?;
        let info = {
            let s = self.0.state.lock().await;
            crypto::user_info(
                &s.session,
                s.operation
                    .remote
                    .as_ref()
                    .ok_or_else(|| failure("corrupt_pending", false))?,
            )?
        };
        let result = client
            .recover(
                &juicebox_sdk::Pin::from(pin.bytes().to_vec()),
                &juicebox_sdk::UserInfo::from(info),
            )
            .await
            .map_err(|e| match e {
                juicebox_sdk::RecoverError::InvalidPin { guesses_remaining } => failure(
                    if guesses_remaining == 0 {
                        "guess_budget_exhausted"
                    } else {
                        "invalid_pin"
                    },
                    false,
                ),
                juicebox_sdk::RecoverError::InvalidAuth => {
                    failure("phone_verification_required", false)
                }
                juicebox_sdk::RecoverError::RateLimitExceeded => failure("rate_limited", true),
                juicebox_sdk::RecoverError::Transient => failure("realm_unavailable", true),
                juicebox_sdk::RecoverError::NotRegistered => failure("backup_unavailable", false),
                _ => failure("recovery_failed", false),
            });
        let secret = match result {
            Ok(secret) => secret,
            Err(fallback)
                if matches!(
                    super::error_code(&fallback),
                    "invalid_pin" | "guess_budget_exhausted"
                ) =>
            {
                return Err(fallback)
            }
            Err(fallback) => return Err(self.recorded_error(fallback).await),
        };
        if secret.expose_secret().len() != 32 {
            return Err(failure("invalid_backup", false));
        }
        Ok(zeroize::Zeroizing::new(secret.expose_secret().to_vec()))
    }
    async fn recorded_error(&self, fallback: crate::ImError) -> crate::ImError {
        let state = self.0.state.lock().await;
        match state.failed.as_deref() {
            Some("network_unavailable") => failure("realm_unavailable", true),
            Some(code) => failure(
                code,
                matches!(
                    code,
                    "realm_unavailable" | "rate_limited" | "session_expiring" | "attempt_expiring"
                ),
            ),
            None => fallback,
        }
    }
    async fn client(
        &self,
    ) -> crate::ImResult<juicebox_sdk::Client<juicebox_sdk::TokioSleeper, Self, Self>> {
        let s = self.0.state.lock().await;
        let realms = s
            .session
            .config
            .realms
            .iter()
            .map(|r| {
                Ok(juicebox_sdk::Realm {
                    id: RealmId::from_str(&r.id)
                        .map_err(|_| failure("invalid_configuration", false))?,
                    address: r
                        .address
                        .parse()
                        .map_err(|_| failure("invalid_configuration", false))?,
                    public_key: None,
                })
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        Ok(juicebox_sdk::ClientBuilder::new()
            .tokio_sleeper()
            .configuration(juicebox_sdk::Configuration {
                realms,
                register_threshold: 3,
                recover_threshold: 2,
                pin_hashing_mode: juicebox_sdk::PinHashingMode::Standard2019,
            })
            .auth_token_manager(self.clone())
            .http(self.clone())
            .build())
    }
    async fn send_inner(&self, request: jb_http::Request) -> crate::ImResult<jb_http::Response> {
        if request.method != jb_http::Method::Post {
            return Err(failure("invalid_realm_request", false));
        }
        let body = zeroize::Zeroizing::new(request.body.unwrap_or_default());
        if body.len() > 2048 {
            return Err(failure("invalid_realm_request", false));
        }
        let decoded: juicebox_realm_api::requests::SecretsRequest =
            juicebox_marshalling::from_slice(&body)
                .map_err(|_| failure("invalid_realm_request", false))?;
        let phase = match &decoded {
            juicebox_realm_api::requests::SecretsRequest::Register1 => "Register1",
            juicebox_realm_api::requests::SecretsRequest::Register2(_) => "Register2",
            juicebox_realm_api::requests::SecretsRequest::Recover1 => "Recover1",
            juicebox_realm_api::requests::SecretsRequest::Recover2(_) => "Recover2",
            juicebox_realm_api::requests::SecretsRequest::Recover3(_) => "Recover3",
            juicebox_realm_api::requests::SecretsRequest::Delete => "Delete",
        };
        let mut state = self.0.state.lock().await;
        let realm = state
            .session
            .config
            .realms
            .iter()
            .find(|r| {
                let expected = reqwest::Url::parse(&r.address)
                    .ok()
                    .and_then(|u| u.join("req").ok());
                expected.is_some_and(|u| u.as_str() == request.url)
            })
            .cloned()
            .ok_or_else(|| failure("untrusted_endpoint", false))?;
        let attempt = state
            .operation
            .attempt
            .clone()
            .ok_or_else(|| failure("corrupt_pending", false))?;
        let permitted = match attempt.action.as_str() {
            "register" => phase == "Register1" || phase == "Register2",
            "recover" | "verify_stage" => phase.starts_with("Recover"),
            _ => false,
        };
        if !permitted {
            return Err(failure("invalid_realm_request", false));
        }
        let token = attempt
            .realm_tokens
            .iter()
            .find(|t| t.realm_id == realm.id)
            .ok_or_else(|| failure("invalid_wire", false))?
            .access_token
            .clone();
        let url = reqwest::Url::parse(&realm.address)
            .map_err(|_| failure("invalid_configuration", false))?
            .join("req")
            .map_err(|_| failure("invalid_configuration", false))?
            .to_string();
        let key = format!(
            "realm:{}:{}:{}:{}",
            attempt.attempt_id,
            realm.id,
            phase,
            crypto::hash(&body)
        );
        let mut exact = http::prepare(
            &self.0.store,
            &mut state.session,
            &key,
            "POST",
            &url,
            body.to_vec(),
        )?;
        let operation_id = state.operation.id.clone();
        http::bind_operation(
            &self.0.store,
            &mut state.session,
            &key,
            &mut exact,
            &operation_id,
        )?;
        if phase == "Register2" {
            let original = state.operation.registration_payloads.get(&realm.id);
            if original.is_some_and(|e| e.body_b64u != exact.body_b64u) {
                return Err(failure("registration_payload_changed", false));
            }
            if original.is_none() {
                state
                    .operation
                    .registration_payloads
                    .insert(realm.id.clone(), exact.clone());
                state.registration_count += 1;
                self.0.store.save_operation(&state.operation)?;
            }
            if state.operation.registration_payloads.len() < 3 {
                let notified = self.0.registration_barrier.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                drop(state);
                if tokio::time::timeout(std::time::Duration::from_secs(45), notified)
                    .await
                    .is_err()
                {
                    return Err(failure("registration_incomplete", true));
                }
                state = self.0.state.lock().await;
                if state.operation.registration_payloads.len() != 3 {
                    return Err(failure("registration_incomplete", true));
                }
            } else {
                self.0.store.save_operation(&state.operation)?;
                super::fault::cut("registration_payloads_sealed", &state.operation)?;
                self.0.registration_barrier.notify_waiters();
            }
        }
        if phase == "Register2" && super::fault::block_register2(&realm.id) {
            // Do not let the SDK fail early and cancel the two genuine writes.
            // This test-only transport failure waits for their signed receipts.
            while state.operation.receipts.len() < 2 {
                let notified = self.0.registration_barrier.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                drop(state);
                tokio::time::timeout(std::time::Duration::from_secs(45), notified)
                    .await
                    .map_err(|_| failure("realm_unavailable", true))?;
                state = self.0.state.lock().await;
            }
            return Err(failure("realm_unavailable", true));
        }
        let (status, headers, bytes) = http::send(
            &self.0.core,
            &self.0.store,
            &mut state.session,
            &exact,
            Some(&token),
            true,
        )
        .await?;
        if phase == "Recover2" && status == 200 {
            super::fault::cut("recover2_response_received", &state.operation)?;
        }
        if !(200..300).contains(&status) {
            state.failed =
                Some(super::error_code(&realm_response_error(status, &headers, &bytes)).into());
        }
        if let Some(receipt) = headers.get("x-awiki-realm-receipt") {
            verify_receipt(
                receipt,
                &realm,
                &state.session,
                &state.operation,
                &attempt,
                &exact,
                phase,
            )?;
            if !state.operation.receipts.contains(receipt) {
                state.operation.receipts.push(receipt.clone());
            }
            self.0.store.save_operation(&state.operation)?;
            self.0.registration_barrier.notify_waiters();
        }
        // Recover1 is verified by the SDK too. The Realm's live authorizer pins
        // Recover2/3 to the operation registration version; no old config fallback.
        Ok(jb_http::Response {
            status_code: status,
            headers: headers.into_iter().collect::<HashMap<_, _>>(),
            body: bytes.to_vec(),
        })
    }
    /// Restart continuation sends the already sealed three Register2 payloads.
    pub async fn resume_register(&self) -> crate::ImResult<()> {
        let mut state = self.0.state.lock().await;
        if state.operation.registration_payloads.len() != 3 {
            return Err(failure("registration_restart_required", false));
        }
        let attempt = state
            .operation
            .attempt
            .clone()
            .ok_or_else(|| failure("corrupt_pending", false))?;
        for (realm_id, exact) in state.operation.registration_payloads.clone() {
            let realm = state
                .session
                .config
                .realms
                .iter()
                .find(|r| r.id == realm_id)
                .cloned()
                .ok_or_else(|| failure("corrupt_pending", false))?;
            let token = attempt
                .realm_tokens
                .iter()
                .find(|t| t.realm_id == realm_id)
                .ok_or_else(|| failure("corrupt_pending", false))?
                .access_token
                .clone();
            let (status, headers, bytes) = http::send(
                &self.0.core,
                &self.0.store,
                &mut state.session,
                &exact,
                Some(&token),
                true,
            )
            .await?;
            if status != 200 {
                return Err(http::response_error(status, &headers, &bytes));
            }
            let receipt = headers
                .get("x-awiki-realm-receipt")
                .ok_or_else(|| failure("missing_receipt", false))?;
            verify_receipt(
                receipt,
                &realm,
                &state.session,
                &state.operation,
                &attempt,
                &exact,
                "Register2",
            )?;
            if !state.operation.receipts.contains(receipt) {
                state.operation.receipts.push(receipt.clone());
            }
            self.0.store.save_operation(&state.operation)?;
        }
        Ok(())
    }
}
#[async_trait]
impl juicebox_sdk::AuthTokenManager for Adapter {
    async fn get(&self, realm: &RealmId) -> Option<AuthToken> {
        let mut state = self.0.state.lock().await;
        let expiring = state.operation.attempt.as_ref().is_some_and(|a| {
            a.realm_tokens.iter().any(|t| {
                chrono::DateTime::parse_from_rfc3339(&t.expires_at)
                    .is_ok_and(|e| e.timestamp() <= crypto::now() as i64 + 10)
            })
        });
        if expiring {
            let State {
                session, operation, ..
            } = &mut *state;
            if let Err(error) =
                super::refresh_tokens(&self.0.core, &self.0.store, session, operation).await
            {
                state.failed = Some(super::error_code(&error).into());
                return None;
            }
        }
        let id = realm
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        state
            .operation
            .attempt
            .as_ref()?
            .realm_tokens
            .iter()
            .find(|t| t.realm_id == id)
            .map(|t| AuthToken::from(t.access_token.clone()))
    }
}
#[async_trait]
impl jb_http::Client for Adapter {
    async fn send(&self, request: jb_http::Request) -> Option<jb_http::Response> {
        match self.send_inner(request).await {
            Ok(r) => Some(r),
            Err(error) => {
                let mut state = self.0.state.lock().await;
                state.failed = Some(super::error_code(&error).into());
                None
            }
        }
    }
}
fn verify_receipt(
    jws: &str,
    realm: &wire::RealmConfig,
    session: &Session,
    operation: &Operation,
    attempt: &wire::AttemptResponse,
    exact: &ExactRequest,
    phase: &str,
) -> crate::ImResult<()> {
    let keys = realm
        .receipt_keys
        .iter()
        .map(|k| (k.kid.clone(), k.public_jwk.x.clone()))
        .collect::<BTreeMap<_, _>>();
    let r: wire::RealmReceiptClaims = crypto::verify_jws(jws, "awiki-realm-receipt+jwt", &keys)?;
    let remote = operation
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let expected_phase = match phase {
        "Register2" => "registered",
        "Recover3" => "recovered",
        _ => return Err(failure("invalid_receipt", false)),
    };
    if r.iss != format!("urn:awiki:realm:{}", realm.id)
        || r.aud != session.config.control_audience
        || r.realm_id != realm.id
        || r.tenant_id != session.tenant_id
        || r.operation_id != operation.id
        || r.attempt_id != attempt.attempt_id
        || r.slot_id != remote.slot_id
        || r.backup_epoch != remote.backup_epoch
        || r.backup_hash != remote.backup_hash.clone().unwrap_or_default()
        || r.phase != expected_phase
        || r.request_sha256 != crypto::hash(&crypto::decode(&exact.body_b64u)?)
        || r.iat > crypto::now() + 5
        || r.exp <= crypto::now()
        || r.exp.saturating_sub(r.iat) > 86400
        || r.record_revision == 0
        || remote
            .registration_version
            .as_ref()
            .is_some_and(|v| v != &r.registration_version)
    {
        return Err(failure("invalid_receipt", false));
    }
    Ok(())
}

pub(super) fn realm_response_error(
    status: u16,
    headers: &BTreeMap<String, String>,
    body: &[u8],
) -> crate::ImError {
    let error = http::response_error(status, headers, body);
    if super::error_code(&error) == "remote_rejected" {
        if status >= 500 {
            return failure("realm_unavailable", true);
        }
        if status == 429 {
            return failure("rate_limited", true);
        }
    }
    error
}
