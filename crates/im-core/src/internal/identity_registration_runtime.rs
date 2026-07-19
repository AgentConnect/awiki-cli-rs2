use serde_json::Value;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

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
        if self.core.inner().device_join_enabled() {
            return self.register_vnext(request, target);
        }
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

    fn register_vnext(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        ensure_vnext_registration_domain(self.core, &target)?;
        // Genesis is never allowed to fall back to plaintext identity files.
        let pending_store =
            crate::internal::identity_genesis_pending::PendingGenesisStore::from_core(self.core)?;
        let method = registration_method(&request.verification);
        let crate::identity::VerificationInput::Phone { phone, otp } = &request.verification else {
            return Err(crate::ImError::unsupported(
                "vnext_genesis_requires_phone_otp",
            ));
        };
        let normalized_phone = crate::internal::identity_wire::normalize_phone(phone)?;
        let otp = otp
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let Some(otp) = otp else {
            let call = crate::internal::identity_wire::device_genesis::build_sms_code_call(
                &normalized_phone,
            )?;
            let raw = self
                .transport
                .rest_post(call.endpoint, call.method, call.body.clone())?;
            return Ok(IdentityRegistrationRuntimeResult {
                sdk_result: self.pending_result(
                    request,
                    target.full_handle,
                    method,
                    crate::identity::HandleRegistrationState::OtpSent,
                ),
                raw: Some(raw),
            });
        };
        self.register_vnext_verified(request, target, normalized_phone, otp, pending_store)
    }

    fn register_vnext_verified(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
        normalized_phone: String,
        otp: String,
        pending_store: crate::internal::identity_genesis_pending::PendingGenesisStore,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let (pending_ref, mut pending) = load_or_create_pending_genesis(
            self.core,
            &pending_store,
            &request,
            &target,
            &normalized_phone,
        )?;
        if pending.normalized_phone != normalized_phone {
            return Err(crate::ImError::PermissionDenied);
        }
        refresh_stale_pending_genesis_credentials(&pending_store, &mut pending)?;
        if pending.remote_result.is_none() && pending.account_grant.is_none() {
            let call = crate::internal::identity_wire::device_genesis::build_account_verification_exchange_call(
                &normalized_phone,
                &otp,
                &pending.target_handle,
                &pending.target_domain,
                &pending.idempotency_scope,
            )?;
            let raw = self
                .transport
                .rest_post(call.endpoint, call.method, call.body.clone())?;
            pending.account_grant = Some(
                crate::internal::identity_wire::device_genesis::parse_account_verification_grant(
                    raw,
                    OffsetDateTime::now_utc(),
                )?,
            );
            pending_store.save(&pending)?;
        }
        if pending.remote_result.is_none() {
            let grant = pending
                .account_grant
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            ensure_unexpired(&grant.expires_at)?;
            let call = crate::internal::identity_wire::device_genesis::build_device_genesis_call(
                &pending.prepared,
                &grant.token,
            )?;
            let raw = self
                .transport
                .rpc(call.endpoint, call.method, call.params.clone())?;
            pending.remote_result = Some(
                crate::internal::identity_wire::device_genesis::parse_device_genesis_result(
                    raw,
                    &pending.generated,
                    OffsetDateTime::now_utc(),
                )?,
            );
            pending.account_grant = None;
            pending_store.save(&pending)?;
        }
        let result = finalize_pending_genesis(self.core, &pending)?;
        pending_store.delete(&pending_ref)?;
        Ok(result)
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
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
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
        if self.core.inner().device_join_enabled() {
            return self.register_vnext_async(request, target).await;
        }
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

    async fn register_vnext_async(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        ensure_vnext_registration_domain(self.core, &target)?;
        let pending_store =
            crate::internal::identity_genesis_pending::PendingGenesisStore::from_core(self.core)?;
        let method = registration_method(&request.verification);
        let crate::identity::VerificationInput::Phone { phone, otp } = &request.verification else {
            return Err(crate::ImError::unsupported(
                "vnext_genesis_requires_phone_otp",
            ));
        };
        let normalized_phone = crate::internal::identity_wire::normalize_phone(phone)?;
        let otp = otp
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let Some(otp) = otp else {
            let call = crate::internal::identity_wire::device_genesis::build_sms_code_call(
                &normalized_phone,
            )?;
            let raw = self
                .transport
                .rest_post(call.endpoint, call.method, call.body.clone())
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
        };
        self.register_vnext_verified_async(request, target, normalized_phone, otp, pending_store)
            .await
    }

    async fn register_vnext_verified_async(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
        normalized_phone: String,
        otp: String,
        pending_store: crate::internal::identity_genesis_pending::PendingGenesisStore,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let (pending_ref, mut pending) = load_or_create_pending_genesis(
            self.core,
            &pending_store,
            &request,
            &target,
            &normalized_phone,
        )?;
        if pending.normalized_phone != normalized_phone {
            return Err(crate::ImError::PermissionDenied);
        }
        refresh_stale_pending_genesis_credentials(&pending_store, &mut pending)?;
        if pending.remote_result.is_none() && pending.account_grant.is_none() {
            let call = crate::internal::identity_wire::device_genesis::build_account_verification_exchange_call(
                &normalized_phone,
                &otp,
                &pending.target_handle,
                &pending.target_domain,
                &pending.idempotency_scope,
            )?;
            let raw = self
                .transport
                .rest_post(call.endpoint, call.method, call.body.clone())
                .await?;
            pending.account_grant = Some(
                crate::internal::identity_wire::device_genesis::parse_account_verification_grant(
                    raw,
                    OffsetDateTime::now_utc(),
                )?,
            );
            pending_store.save(&pending)?;
        }
        if pending.remote_result.is_none() {
            let grant = pending
                .account_grant
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            ensure_unexpired(&grant.expires_at)?;
            let call = crate::internal::identity_wire::device_genesis::build_device_genesis_call(
                &pending.prepared,
                &grant.token,
            )?;
            let raw = self
                .transport
                .rpc(call.endpoint, call.method, call.params.clone())
                .await?;
            pending.remote_result = Some(
                crate::internal::identity_wire::device_genesis::parse_device_genesis_result(
                    raw,
                    &pending.generated,
                    OffsetDateTime::now_utc(),
                )?,
            );
            pending.account_grant = None;
            pending_store.save(&pending)?;
        }
        let result = finalize_pending_genesis_async(self.core, &pending).await?;
        pending_store.delete(&pending_ref)?;
        Ok(result)
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
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
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

fn ensure_vnext_registration_domain(
    core: &crate::core::ImCore,
    target: &RegistrationTarget,
) -> crate::ImResult<()> {
    let configured = core
        .inner()
        .sdk_config()
        .did_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if configured.is_empty() || target.effective_domain != configured {
        return Err(crate::ImError::unsupported(
            "vnext_genesis_same_domain_only",
        ));
    }
    Ok(())
}

fn load_or_create_pending_genesis(
    core: &crate::core::ImCore,
    store: &crate::internal::identity_genesis_pending::PendingGenesisStore,
    request: &crate::identity::RegisterHandleRequest,
    target: &RegistrationTarget,
    normalized_phone: &str,
) -> crate::ImResult<(
    crate::internal::secret_vault::record::SecretRef,
    crate::internal::identity_genesis_pending::PendingGenesisRecord,
)> {
    if let Some(existing) = store.load(&target.local_part, &target.effective_domain)? {
        return Ok(existing);
    }
    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            &target.effective_domain,
            &target.local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
    let operation_id = secure_local_id("op-genesis")?;
    let prepared = crate::internal::identity_wire::device_genesis::prepare_device_genesis(
        &generated,
        operation_id,
        OffsetDateTime::now_utc(),
    )?;
    let pending = crate::internal::identity_genesis_pending::PendingGenesisRecord::new(
        crate::internal::identity_genesis_pending::NewPendingGenesis {
            target_handle: target.local_part.clone(),
            target_domain: target.effective_domain.clone(),
            normalized_phone: normalized_phone.to_owned(),
            local_alias: local_alias(request, target),
            display_name: request
                .profile
                .display_name
                .clone()
                .unwrap_or_else(|| target.local_part.clone()),
            make_default: request.make_default,
            idempotency_scope: secure_local_id("genesis-scope")?,
            generated,
            prepared,
        },
    )?;
    let secret_ref = store.save(&pending)?;
    Ok((secret_ref, pending))
}

fn refresh_stale_pending_genesis_credentials(
    store: &crate::internal::identity_genesis_pending::PendingGenesisStore,
    pending: &mut crate::internal::identity_genesis_pending::PendingGenesisRecord,
) -> crate::ImResult<()> {
    if pending.remote_result.is_some() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    let proof_expired = is_expired_at(&pending.prepared.bootstrap_device_proof.expires_at, now)?;
    let grant_expired = pending
        .account_grant
        .as_ref()
        .map(|grant| is_expired_at(&grant.expires_at, now))
        .transpose()?
        .unwrap_or(false);
    if !proof_expired && !grant_expired {
        return Ok(());
    }

    pending.prepared = crate::internal::identity_wire::device_genesis::prepare_device_genesis(
        &pending.generated,
        pending.prepared.operation_id.clone(),
        now,
    )?;
    pending.account_grant = None;
    store.save(pending)?;
    Ok(())
}

fn finalize_pending_genesis(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_genesis_pending::PendingGenesisRecord,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    let previous_default = core.identities().default_identity().ok().flatten();
    let remote = pending
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let stored =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_identity_with_secret_storage(
            vnext_save_input(pending, remote),
            secret_storage.clone(),
        )?;
    crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
        .persist_vnext_auth_token_pair(
            &pending.local_alias,
            &remote.access_token,
            &remote.refresh_token,
            &remote.token_expires_at,
            &secret_storage,
        )?;
    vnext_registration_result(pending, stored, previous_default)
}

async fn finalize_pending_genesis_async(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_genesis_pending::PendingGenesisRecord,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    let previous_default = core
        .identities()
        .default_identity_async()
        .await
        .ok()
        .flatten();
    let remote = pending
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let paths = core.inner().sdk_paths().identities.clone();
    let stored =
        crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
            paths.clone(),
            vnext_save_input(pending, remote),
            secret_storage.clone(),
        )
        .await?;
    crate::internal::identity_store::IdentityStore::persist_vnext_auth_token_pair_async(
        paths,
        pending.local_alias.clone(),
        remote.access_token.clone(),
        remote.refresh_token.clone(),
        remote.token_expires_at.clone(),
        secret_storage,
    )
    .await?;
    vnext_registration_result(pending, stored, previous_default)
}

fn vnext_save_input(
    pending: &crate::internal::identity_genesis_pending::PendingGenesisRecord,
    remote: &crate::internal::identity_wire::device_genesis::DeviceGenesisResult,
) -> crate::internal::identity_store::SaveIdentityInput {
    crate::internal::identity_store::SaveIdentityInput {
        local_alias: pending.local_alias.clone(),
        did: pending.generated.did.clone(),
        unique_id: pending.generated.unique_id.clone(),
        user_id: remote.user_id.clone(),
        display_name: pending.display_name.clone(),
        handle: pending.target_handle.clone(),
        full_handle: format!("{}.{}", pending.target_handle, pending.target_domain),
        jwt_token: remote.access_token.clone(),
        did_document: Some(pending.generated.did_document.clone()),
        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
            root_key_id: pending.generated.root_key_id.clone(),
            device_signing_key_id: pending.generated.device_signing_key_id.clone(),
            device_e2ee_key_id: pending.generated.device_e2ee_key_id.clone(),
        },
        device_state: Some(remote.device_state()),
        key1_private_pem: pending.generated.root_private_pem.clone(),
        key1_public_pem: pending.generated.root_public_pem.clone(),
        e2ee_signing_private_pem: pending.generated.device_signing_private_pem.clone(),
        e2ee_agreement_private_pem: pending.generated.device_e2ee_private_pem.clone(),
        daemon_subkey_package: Some(pending.generated.daemon_subkey_package.clone()),
        make_default: pending.make_default,
    }
}

fn vnext_registration_result(
    pending: &crate::internal::identity_genesis_pending::PendingGenesisRecord,
    stored: crate::internal::identity_store::StoredIdentity,
    previous_default: Option<crate::identity::IdentitySummary>,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    let mut identity = identity_summary_from_stored(&stored)?;
    identity.device_id = Some(pending.generated.protocol_device_id.as_str().to_owned());
    let handle = crate::ids::Handle::parse(
        format!("{}.{}", pending.target_handle, pending.target_domain),
        "",
    )?;
    Ok(IdentityRegistrationRuntimeResult {
        sdk_result: crate::identity::HandleRegistrationResult {
            identity: Some(identity.clone()),
            handle,
            method: crate::identity::RegistrationMethod::Phone,
            state: crate::identity::HandleRegistrationState::Registered,
            default_identity_change: pending.make_default.then(|| {
                crate::identity::DefaultIdentityChange {
                    previous: previous_default,
                    next: identity,
                    requires_default_identity_write: false,
                    warnings: Vec::new(),
                }
            }),
            warnings: Vec::new(),
        },
        // The raw server value contains an internal checkpoint and refresh
        // token, so vNext registration never exposes it through the facade.
        raw: None,
    })
}

fn ensure_unexpired(value: &str) -> crate::ImResult<()> {
    if is_expired_at(value, OffsetDateTime::now_utc())? {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(())
}

fn is_expired_at(value: &str, now: OffsetDateTime) -> crate::ImResult<bool> {
    let expires =
        time::OffsetDateTime::parse(value.trim(), &time::format_description::well_known::Rfc3339)
            .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(expires <= now)
}

fn secure_local_id(prefix: &str) -> crate::ImResult<String> {
    use base64::Engine as _;
    use rand::RngCore as _;

    let mut bytes = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| crate::ImError::Internal {
            message: "secure Genesis operation id generation failed".to_owned(),
        })?;
    Ok(format!(
        "{prefix}-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
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
