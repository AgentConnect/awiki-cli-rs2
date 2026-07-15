use serde_json::Value;
use std::time::{Duration, Instant};

use crate::internal::transport::{
    AsyncRestTransport, AsyncRpcTransport, RestTransport, RpcTransport,
};

const DEFAULT_EMAIL_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_EMAIL_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRegistrationRuntimeResult {
    pub(crate) sdk_result: crate::identity::HandleRegistrationResult,
    pub(crate) raw: Option<Value>,
}

pub(crate) struct IdentityRegistrationRuntime<'a, T> {
    core: &'a crate::core::ImCore,
    transport: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistrationTarget {
    local_part: String,
    full_handle: crate::ids::Handle,
    effective_domain: String,
    explicit_domain: bool,
}

impl<'a, T> IdentityRegistrationRuntime<'a, T> {
    pub(crate) fn new(core: &'a crate::core::ImCore, transport: T) -> Self {
        Self { core, transport }
    }

    fn pending_result(
        &self,
        request: crate::identity::RegisterHandleRequest,
        handle: crate::ids::Handle,
        method: crate::identity::RegistrationMethod,
        state: crate::identity::HandleRegistrationState,
    ) -> crate::identity::HandleRegistrationResult {
        crate::identity::HandleRegistrationResult {
            identity: None,
            handle,
            method,
            state,
            default_identity_change: None,
            warnings: warnings_for_request(&request),
        }
    }
}

impl<'a, T> IdentityRegistrationRuntime<'a, T>
where
    T: RpcTransport + RestTransport,
{
    pub(crate) fn register_handle(
        mut self,
        request: crate::identity::RegisterHandleRequest,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let target = registration_target(
            request.requested_handle.as_str(),
            &self.core.inner().sdk_config().did_domain,
        )?;
        let method = registration_method(&request.verification);
        match &request.verification {
            crate::identity::VerificationInput::Phone { phone, otp } => {
                let phone = crate::internal::identity_wire::normalize_phone(phone)?;
                if otp.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                    let call =
                        crate::internal::identity_wire::directory::build_send_otp_rpc_call(&phone)?;
                    let raw =
                        self.transport
                            .rpc(call.endpoint, call.method, call.params.clone())?;
                    return Ok(IdentityRegistrationRuntimeResult {
                        sdk_result: self.pending_result(
                            request,
                            target.full_handle,
                            method,
                            crate::identity::HandleRegistrationState::OtpSent,
                        ),
                        raw: Some(raw),
                    });
                }
                self.register_verified(request, target)
            }
            crate::identity::VerificationInput::Email {
                email,
                wait_for_verification,
            } => {
                let email = crate::internal::identity_wire::required_normalized_email(email)?;
                let status = self.email_status_value(&email, target.full_handle.as_str())?;
                if !status.as_ref().is_some_and(email_verified) {
                    let call = crate::internal::identity_wire::bind::build_email_send_rest_call(
                        &email,
                        Some(target.full_handle.as_str()),
                        false,
                    )?;
                    let raw =
                        self.transport
                            .rest_post(call.endpoint, call.method, call.body.clone())?;
                    if !wait_for_verification {
                        return Ok(IdentityRegistrationRuntimeResult {
                            sdk_result: self.pending_result(
                                request,
                                target.full_handle,
                                method,
                                crate::identity::HandleRegistrationState::EmailSent,
                            ),
                            raw: Some(raw),
                        });
                    }
                    if !self.wait_for_email_verified(&email, target.full_handle.as_str())? {
                        return Ok(IdentityRegistrationRuntimeResult {
                            sdk_result: self.pending_result(
                                request,
                                target.full_handle,
                                method,
                                crate::identity::HandleRegistrationState::EmailPending,
                            ),
                            raw: Some(raw),
                        });
                    }
                }
                self.register_verified(request, target)
            }
            crate::identity::VerificationInput::Otp { .. }
            | crate::identity::VerificationInput::AlreadyVerified => {
                self.register_verified(request, target)
            }
        }
    }

    fn register_verified(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let previous_default = self.core.identities().default_identity().ok().flatten();
        let generated_with_daemon =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                &target.effective_domain,
                &target.local_part,
                self.core.inner().sdk_config().anp_service_endpoint.as_ref(),
                self.core.inner().sdk_config().anp_service_did.as_ref(),
            )?;
        let crate::internal::identity_generation::GeneratedIdentityWithDaemonSubkey {
            identity: generated,
            daemon_subkey_package,
        } = generated_with_daemon;
        let call = crate::internal::identity_wire::recovery::build_register_rpc_call(
            crate::internal::identity_wire::RegisterRpcParams {
                did_document: generated.did_document.clone(),
                handle: target.local_part.clone(),
                phone: registration_phone(&request.verification),
                otp_code: registration_otp(&request.verification),
                email: registration_email(&request.verification),
                invite_code: request.invite_code.clone().unwrap_or_default(),
            },
        )?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let local_alias = local_alias(&request, &target);
        let secret_storage =
            crate::internal::identity_store::SaveIdentitySecretStorage::from_core(self.core)?;
        let stored = crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        )
        .save_identity_with_secret_storage(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias,
                did: generated.did.clone(),
                unique_id: generated.unique_id,
                user_id: string_value(&raw, "user_id", ""),
                display_name: request
                    .profile
                    .display_name
                    .clone()
                    .unwrap_or_else(|| target.local_part.clone()),
                handle: string_value(&raw, "handle", &target.local_part),
                full_handle: string_value(&raw, "full_handle", target.full_handle.as_str()),
                jwt_token: string_value(&raw, "access_token", ""),
                did_document: Some(generated.did_document),
                key1_private_pem: generated.key1_private_pem,
                key1_public_pem: generated.key1_public_pem,
                e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
                e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
                daemon_subkey_package: Some(daemon_subkey_package),
                make_default: request.make_default,
            },
            secret_storage,
        )?;
        let identity = identity_summary_from_stored(&stored)?;
        let sdk_result = crate::identity::HandleRegistrationResult {
            identity: Some(identity.clone()),
            handle: target.full_handle,
            method: registration_method(&request.verification),
            state: crate::identity::HandleRegistrationState::Registered,
            default_identity_change: request.make_default.then(|| {
                crate::identity::DefaultIdentityChange {
                    previous: previous_default,
                    next: identity,
                    requires_default_identity_write: false,
                    warnings: Vec::new(),
                }
            }),
            warnings: Vec::new(),
        };
        Ok(IdentityRegistrationRuntimeResult {
            sdk_result,
            raw: Some(raw),
        })
    }

    fn email_status_value(&mut self, email: &str, handle: &str) -> crate::ImResult<Option<Value>> {
        let call = crate::internal::identity_wire::bind::build_email_status_rest_call(
            email,
            Some(handle),
            false,
        )?;
        match self
            .transport
            .rest_get(call.endpoint, call.method, &call.query)
        {
            Ok(status) => Ok(Some(status)),
            Err(crate::ImError::Service {
                status_code: Some(404),
                ..
            }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn wait_for_email_verified(&mut self, email: &str, handle: &str) -> crate::ImResult<bool> {
        let deadline = Instant::now() + DEFAULT_EMAIL_VERIFICATION_TIMEOUT;
        loop {
            if self
                .email_status_value(email, handle)?
                .as_ref()
                .is_some_and(email_verified)
            {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(DEFAULT_EMAIL_POLL_INTERVAL);
        }
    }
}

impl<'a, T> IdentityRegistrationRuntime<'a, T>
where
    T: AsyncRpcTransport + AsyncRestTransport,
{
    pub(crate) async fn register_handle_async(
        mut self,
        request: crate::identity::RegisterHandleRequest,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let target = registration_target(
            request.requested_handle.as_str(),
            &self.core.inner().sdk_config().did_domain,
        )?;
        let method = registration_method(&request.verification);
        match &request.verification {
            crate::identity::VerificationInput::Phone { phone, otp } => {
                let phone = crate::internal::identity_wire::normalize_phone(phone)?;
                if otp.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                    let call =
                        crate::internal::identity_wire::directory::build_send_otp_rpc_call(&phone)?;
                    let raw = self
                        .transport
                        .rpc(call.endpoint, call.method, call.params.clone())
                        .await?;
                    return Ok(IdentityRegistrationRuntimeResult {
                        sdk_result: self.pending_result(
                            request,
                            target.full_handle,
                            method,
                            crate::identity::HandleRegistrationState::OtpSent,
                        ),
                        raw: Some(raw),
                    });
                }
                self.register_verified_async(request, target).await
            }
            crate::identity::VerificationInput::Email {
                email,
                wait_for_verification,
            } => {
                let email = crate::internal::identity_wire::required_normalized_email(email)?;
                let status = self
                    .email_status_value_async(&email, target.full_handle.as_str())
                    .await?;
                if !status.as_ref().is_some_and(email_verified) {
                    let call = crate::internal::identity_wire::bind::build_email_send_rest_call(
                        &email,
                        Some(target.full_handle.as_str()),
                        false,
                    )?;
                    let raw = self
                        .transport
                        .rest_post(call.endpoint, call.method, call.body.clone())
                        .await?;
                    if !wait_for_verification {
                        return Ok(IdentityRegistrationRuntimeResult {
                            sdk_result: self.pending_result(
                                request,
                                target.full_handle,
                                method,
                                crate::identity::HandleRegistrationState::EmailSent,
                            ),
                            raw: Some(raw),
                        });
                    }
                    if !self
                        .wait_for_email_verified_async(&email, target.full_handle.as_str())
                        .await?
                    {
                        return Ok(IdentityRegistrationRuntimeResult {
                            sdk_result: self.pending_result(
                                request,
                                target.full_handle,
                                method,
                                crate::identity::HandleRegistrationState::EmailPending,
                            ),
                            raw: Some(raw),
                        });
                    }
                }
                self.register_verified_async(request, target).await
            }
            crate::identity::VerificationInput::Otp { .. }
            | crate::identity::VerificationInput::AlreadyVerified => {
                self.register_verified_async(request, target).await
            }
        }
    }

    async fn register_verified_async(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let previous_default = self
            .core
            .identities()
            .default_identity_async()
            .await
            .ok()
            .flatten();
        let generated_with_daemon =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                &target.effective_domain,
                &target.local_part,
                self.core.inner().sdk_config().anp_service_endpoint.as_ref(),
                self.core.inner().sdk_config().anp_service_did.as_ref(),
            )?;
        let crate::internal::identity_generation::GeneratedIdentityWithDaemonSubkey {
            identity: generated,
            daemon_subkey_package,
        } = generated_with_daemon;
        let call = crate::internal::identity_wire::recovery::build_register_rpc_call(
            crate::internal::identity_wire::RegisterRpcParams {
                did_document: generated.did_document.clone(),
                handle: target.local_part.clone(),
                phone: registration_phone(&request.verification),
                otp_code: registration_otp(&request.verification),
                email: registration_email(&request.verification),
                invite_code: request.invite_code.clone().unwrap_or_default(),
            },
        )?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())
            .await?;
        let local_alias = local_alias(&request, &target);
        let secret_storage =
            crate::internal::identity_store::SaveIdentitySecretStorage::from_core(self.core)?;
        let stored = crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
            self.core.inner().sdk_paths().identities.clone(),
            crate::internal::identity_store::SaveIdentityInput {
                local_alias,
                did: generated.did.clone(),
                unique_id: generated.unique_id,
                user_id: string_value(&raw, "user_id", ""),
                display_name: request
                    .profile
                    .display_name
                    .clone()
                    .unwrap_or_else(|| target.local_part.clone()),
                handle: string_value(&raw, "handle", &target.local_part),
                full_handle: string_value(&raw, "full_handle", target.full_handle.as_str()),
                jwt_token: string_value(&raw, "access_token", ""),
                did_document: Some(generated.did_document),
                key1_private_pem: generated.key1_private_pem,
                key1_public_pem: generated.key1_public_pem,
                e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
                e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
                daemon_subkey_package: Some(daemon_subkey_package),
                make_default: request.make_default,
            },
            secret_storage,
        )
        .await?;
        let identity = identity_summary_from_stored(&stored)?;
        let sdk_result = crate::identity::HandleRegistrationResult {
            identity: Some(identity.clone()),
            handle: target.full_handle,
            method: registration_method(&request.verification),
            state: crate::identity::HandleRegistrationState::Registered,
            default_identity_change: request.make_default.then(|| {
                crate::identity::DefaultIdentityChange {
                    previous: previous_default,
                    next: identity,
                    requires_default_identity_write: false,
                    warnings: Vec::new(),
                }
            }),
            warnings: Vec::new(),
        };
        Ok(IdentityRegistrationRuntimeResult {
            sdk_result,
            raw: Some(raw),
        })
    }

    async fn email_status_value_async(
        &mut self,
        email: &str,
        handle: &str,
    ) -> crate::ImResult<Option<Value>> {
        let call = crate::internal::identity_wire::bind::build_email_status_rest_call(
            email,
            Some(handle),
            false,
        )?;
        match self
            .transport
            .rest_get(call.endpoint, call.method, &call.query)
            .await
        {
            Ok(status) => Ok(Some(status)),
            Err(crate::ImError::Service {
                status_code: Some(404),
                ..
            }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn wait_for_email_verified_async(
        &mut self,
        email: &str,
        handle: &str,
    ) -> crate::ImResult<bool> {
        let deadline = tokio::time::Instant::now() + DEFAULT_EMAIL_VERIFICATION_TIMEOUT;
        loop {
            if self
                .email_status_value_async(email, handle)
                .await?
                .as_ref()
                .is_some_and(email_verified)
            {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(DEFAULT_EMAIL_POLL_INTERVAL).await;
        }
    }
}

pub(crate) fn registration_method(
    verification: &crate::identity::VerificationInput,
) -> crate::identity::RegistrationMethod {
    match verification {
        crate::identity::VerificationInput::Phone { .. }
        | crate::identity::VerificationInput::Otp { .. } => {
            crate::identity::RegistrationMethod::Phone
        }
        crate::identity::VerificationInput::Email { .. } => {
            crate::identity::RegistrationMethod::Email
        }
        crate::identity::VerificationInput::AlreadyVerified => {
            crate::identity::RegistrationMethod::AlreadyVerified
        }
    }
}

fn registration_target(raw: &str, did_domain: &str) -> crate::ImResult<RegistrationTarget> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle is required",
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "DID values are not supported in handle input",
        ));
    }
    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    let (local_part, domain, explicit_domain) = if let Some(dot) = handle.find('.') {
        (
            handle[..dot].trim().to_string(),
            handle[dot + 1..].trim().trim_end_matches('.').to_string(),
            true,
        )
    } else {
        (
            handle.to_string(),
            did_domain.trim().trim_end_matches('.').to_ascii_lowercase(),
            false,
        )
    };
    if local_part.is_empty() || domain.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle domain and local part are required",
        ));
    }
    Ok(RegistrationTarget {
        full_handle: crate::ids::Handle::parse(format!("{local_part}.{domain}"), "")?,
        local_part,
        effective_domain: domain,
        explicit_domain,
    })
}

fn local_alias(
    request: &crate::identity::RegisterHandleRequest,
    target: &RegistrationTarget,
) -> String {
    request
        .local_alias
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if target.explicit_domain {
            target.full_handle.as_str()
        } else {
            &target.local_part
        })
        .to_string()
}

fn registration_phone(verification: &crate::identity::VerificationInput) -> Option<String> {
    match verification {
        crate::identity::VerificationInput::Phone { phone, .. } => Some(phone.clone()),
        _ => None,
    }
}

fn registration_otp(verification: &crate::identity::VerificationInput) -> Option<String> {
    match verification {
        crate::identity::VerificationInput::Phone { otp, .. } => otp.clone(),
        crate::identity::VerificationInput::Otp { code } => Some(code.clone()),
        _ => None,
    }
}

fn registration_email(verification: &crate::identity::VerificationInput) -> Option<String> {
    match verification {
        crate::identity::VerificationInput::Email { email, .. } => Some(email.clone()),
        _ => None,
    }
}

fn did_from_raw(raw: &Value) -> Option<crate::ids::Did> {
    raw.get("did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| crate::ids::Did::parse(value).ok())
}

fn string_value(raw: &Value, key: &str, fallback: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn identity_summary_from_stored(
    stored: &crate::internal::identity_store::StoredIdentity,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    Ok(crate::identity::IdentitySummary {
        id: crate::ids::IdentityId::parse(&stored.unique_id)?,
        did: stored.did.clone(),
        handle: Some(crate::ids::Handle::parse(&stored.full_handle, "")?),
        display_name: Some(stored.display_name.clone()).filter(|value| !value.trim().is_empty()),
        local_alias: Some(stored.local_alias.clone()),
        device_id: None,
        is_default: stored.is_default,
        readiness: crate::identity::IdentityReadiness {
            ready_for_auth: !stored.jwt_token.trim().is_empty()
                && stored.has_did_document
                && stored.has_key1_private,
            ready_for_messaging: !stored.user_id.trim().is_empty()
                && !stored.handle.trim().is_empty(),
            missing: Vec::new(),
        },
    })
}

fn warnings_for_request(_request: &crate::identity::RegisterHandleRequest) -> Vec<String> {
    Vec::new()
}

pub(crate) fn email_verified(value: &Value) -> bool {
    value
        .get("verified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
