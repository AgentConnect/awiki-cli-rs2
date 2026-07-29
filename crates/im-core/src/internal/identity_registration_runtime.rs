//! Identity registration through the single public `register` RPC.
//!
//! Every completed registration is a vNext identity with one bootstrap
//! Manifest device. `PendingRegistration` keeps the exact generated material
//! restart-safe until both the local identity commit and the mandatory P5
//! PreKey publication have succeeded.

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
            join_required: None,
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
        ensure_registration_domain(self.core, &target)?;
        let method = registration_method(&request.verification);
        match &request.verification {
            crate::identity::VerificationInput::Phone { phone, otp } => {
                let phone = crate::internal::identity_wire::normalize_phone(phone)?;
                if otp.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                    let call = crate::internal::identity_wire::directory::build_registration_send_otp_rpc_call(
                        &phone,
                        &target.local_part,
                        &target.effective_domain,
                        target.full_handle.as_str(),
                    )?;
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
            }
            crate::identity::VerificationInput::Otp { .. }
            | crate::identity::VerificationInput::AlreadyVerified => {}
        }
        self.register_verified(request, target)
    }

    fn register_verified(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let store =
            crate::internal::identity_registration_pending::PendingRegistrationStore::from_core(
                self.core,
            )?;
        let (pending_ref, mut pending) =
            load_or_create_pending_registration(self.core, &store, &request, &target)?;
        verify_pending_matches_request(&pending, &request, &target)?;
        if let Some(join_required) =
            ensure_remote_registration(&mut self.transport, &mut pending, &request, |pending| {
                store.save(pending).map(|_| ())
            })?
        {
            store.delete(&pending_ref)?;
            return join_required_result(&request, target.full_handle, join_required);
        }
        let result = commit_pending_registration(
            self.core,
            &pending,
            registration_method(&request.verification),
        )?;
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        store.save(&pending)?;
        finish_registration_after_prekey_publish(
            publish_v2_prekeys_after_registration(self.core, &pending.generated.did),
            || store.delete(&pending_ref),
        )?;
        Ok(result)
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
        ensure_registration_domain(self.core, &target)?;
        let method = registration_method(&request.verification);
        match &request.verification {
            crate::identity::VerificationInput::Phone { phone, otp } => {
                let phone = crate::internal::identity_wire::normalize_phone(phone)?;
                if otp.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                    let call = crate::internal::identity_wire::directory::build_registration_send_otp_rpc_call(
                        &phone,
                        &target.local_part,
                        &target.effective_domain,
                        target.full_handle.as_str(),
                    )?;
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
            }
            crate::identity::VerificationInput::Otp { .. }
            | crate::identity::VerificationInput::AlreadyVerified => {}
        }
        self.register_verified_async(request, target).await
    }

    async fn register_verified_async(
        &mut self,
        request: crate::identity::RegisterHandleRequest,
        target: RegistrationTarget,
    ) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
        let store =
            crate::internal::identity_registration_pending::PendingRegistrationStore::from_core(
                self.core,
            )?;
        let (pending_ref, mut pending) =
            load_or_create_pending_registration(self.core, &store, &request, &target)?;
        verify_pending_matches_request(&pending, &request, &target)?;
        if pending.remote_result.is_none() {
            if pending.remote_attempted {
                match self
                    .transport
                    .reconcile_pending_registration(&pending)
                    .await?
                {
                    crate::internal::transport::PendingRegistrationReconciliation::Absent => {}
                    committed => {
                        apply_registration_reconciliation(&mut pending, committed)?;
                        store.save(&pending)?;
                    }
                }
            }
        }
        if pending.remote_result.is_none() {
            pending.remote_attempted = true;
            store.save(&pending)?;
            let call = register_call(&pending, &request)?;
            match self
                .transport
                .rpc(call.endpoint, call.method, call.params.clone())
                .await
            {
                Ok(raw) => match parse_register_outcome(&pending, raw)? {
                    RegistrationRemoteOutcome::Registered(result) => {
                        pending.remote_result = Some(result);
                        pending.phase =
                                crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted;
                    }
                    RegistrationRemoteOutcome::JoinRequired(join_required) => {
                        store.delete(&pending_ref)?;
                        return join_required_result(&request, target.full_handle, join_required);
                    }
                },
                Err(error @ crate::ImError::TransportUnavailable { .. }) => {
                    match self
                        .transport
                        .reconcile_pending_registration(&pending)
                        .await?
                    {
                        crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                            return Err(error);
                        }
                        committed => apply_registration_reconciliation(&mut pending, committed)?,
                    }
                }
                Err(error) => return Err(error),
            }
            store.save(&pending)?;
        }
        let result = commit_pending_registration_async(
            self.core,
            &pending,
            registration_method(&request.verification),
        )
        .await?;
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        store.save(&pending)?;
        let publish_result =
            publish_v2_prekeys_after_registration_async(self.core, &pending.generated.did).await;
        finish_registration_after_prekey_publish(publish_result, || store.delete(&pending_ref))?;
        Ok(result)
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

fn ensure_registration_domain(
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
            "vnext_registration_same_domain_only",
        ));
    }
    Ok(())
}

fn load_or_create_pending_registration(
    core: &crate::core::ImCore,
    store: &crate::internal::identity_registration_pending::PendingRegistrationStore,
    request: &crate::identity::RegisterHandleRequest,
    target: &RegistrationTarget,
) -> crate::ImResult<(
    crate::internal::secret_vault::record::SecretRef,
    crate::internal::identity_registration_pending::PendingRegistration,
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
    let pending = crate::internal::identity_registration_pending::PendingRegistration::new(
        target.local_part.clone(),
        target.effective_domain.clone(),
        local_alias(request, target),
        request
            .profile
            .display_name
            .clone()
            .unwrap_or_else(|| target.local_part.clone()),
        request.make_default,
        pending_verification_kind(&request.verification).to_owned(),
        pending_verification_target(&request.verification),
        request.invite_code.clone(),
        generated,
    )?;
    let secret_ref = store.save(&pending)?;
    Ok((secret_ref, pending))
}

fn verify_pending_matches_request(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    request: &crate::identity::RegisterHandleRequest,
    target: &RegistrationTarget,
) -> crate::ImResult<()> {
    pending.validate()?;
    if pending.local_alias != local_alias(request, target)
        || pending.make_default != request.make_default
        || pending.display_name
            != request
                .profile
                .display_name
                .clone()
                .unwrap_or_else(|| target.local_part.clone())
        || pending.verification_kind != pending_verification_kind(&request.verification)
        || pending.verification_target != pending_verification_target(&request.verification)
        || pending.invite_code != request.invite_code
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn pending_verification_kind(verification: &crate::identity::VerificationInput) -> &'static str {
    match verification {
        crate::identity::VerificationInput::Phone { .. } => "phone",
        crate::identity::VerificationInput::Email { .. } => "email",
        crate::identity::VerificationInput::Otp { .. } => "otp",
        crate::identity::VerificationInput::AlreadyVerified => "already_verified",
    }
}

fn pending_verification_target(
    verification: &crate::identity::VerificationInput,
) -> Option<String> {
    match verification {
        crate::identity::VerificationInput::Phone { phone, .. } => {
            crate::internal::identity_wire::normalize_phone(phone).ok()
        }
        crate::identity::VerificationInput::Email { email, .. } => {
            Some(email.trim().to_ascii_lowercase()).filter(|value| !value.is_empty())
        }
        crate::identity::VerificationInput::Otp { .. }
        | crate::identity::VerificationInput::AlreadyVerified => None,
    }
}

fn register_call(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    request: &crate::identity::RegisterHandleRequest,
) -> crate::ImResult<crate::internal::identity_wire::RpcCall> {
    crate::internal::identity_wire::registration::build_register_rpc_call(
        crate::internal::identity_wire::RegisterRpcParams {
            did_document: pending.generated.did_document.clone(),
            handle: pending.target_handle.clone(),
            phone: registration_phone(&request.verification),
            otp_code: registration_otp(&request.verification),
            email: registration_email(&request.verification),
            invite_code: request.invite_code.clone().unwrap_or_default(),
        },
    )
}

fn ensure_remote_registration<T, P>(
    transport: &mut T,
    pending: &mut crate::internal::identity_registration_pending::PendingRegistration,
    request: &crate::identity::RegisterHandleRequest,
    mut persist: P,
) -> crate::ImResult<Option<crate::identity::HandleRegistrationJoinRequired>>
where
    T: RpcTransport,
    P: FnMut(
        &crate::internal::identity_registration_pending::PendingRegistration,
    ) -> crate::ImResult<()>,
{
    if pending.remote_result.is_some() {
        return Ok(None);
    }
    if pending.remote_attempted {
        match transport.reconcile_pending_registration(pending)? {
            crate::internal::transport::PendingRegistrationReconciliation::Absent => {}
            committed => {
                apply_registration_reconciliation(pending, committed)?;
                persist(pending)?;
                return Ok(None);
            }
        }
    }

    // Persist before the first byte is sent. A process crash or lost response
    // must enter signed reconciliation on restart and must never blindly
    // replay register.
    pending.remote_attempted = true;
    persist(pending)?;
    let call = register_call(pending, request)?;
    match transport.rpc(call.endpoint, call.method, call.params) {
        Ok(raw) => match parse_register_outcome(pending, raw)? {
            RegistrationRemoteOutcome::Registered(result) => {
                pending.remote_result = Some(result);
                pending.phase =
                        crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted;
            }
            RegistrationRemoteOutcome::JoinRequired(join_required) => {
                return Ok(Some(join_required));
            }
        },
        Err(error @ crate::ImError::TransportUnavailable { .. }) => {
            match transport.reconcile_pending_registration(pending)? {
                crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                    return Err(error);
                }
                committed => apply_registration_reconciliation(pending, committed)?,
            }
        }
        Err(error) => return Err(error),
    }
    persist(pending)?;
    Ok(None)
}

enum RegistrationRemoteOutcome {
    Registered(crate::internal::identity_registration_pending::PendingRegistrationRemoteResult),
    JoinRequired(crate::identity::HandleRegistrationJoinRequired),
}

fn parse_register_outcome(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    raw: Value,
) -> crate::ImResult<RegistrationRemoteOutcome> {
    let state = required_string(&raw, "state")?;
    if state == "join_required" {
        require_exact_fields(
            &raw,
            &[
                "state",
                "handle",
                "domain",
                "full_handle",
                "did",
                "account_verification_token",
            ],
        )?;
        let handle = required_string(&raw, "handle")?;
        let domain = required_string(&raw, "domain")?;
        let full_handle = required_string(&raw, "full_handle")?;
        let did = crate::ids::Did::parse(required_string(&raw, "did")?)?;
        let token = required_string(&raw, "account_verification_token")?;
        if handle != pending.target_handle
            || domain != pending.target_domain
            || full_handle != format!("{}.{}", pending.target_handle, pending.target_domain)
            || crate::internal::identity_join_activation_pending::service_domain_from_did(&did)?
                != pending.target_domain
        {
            return Err(crate::ImError::PermissionDenied);
        }
        return Ok(RegistrationRemoteOutcome::JoinRequired(
            crate::identity::HandleRegistrationJoinRequired {
                did,
                account_verification_token: token,
            },
        ));
    }
    if state != "registered" {
        return Err(crate::ImError::PermissionDenied);
    }
    require_exact_fields(
        &raw,
        &[
            "state",
            "did",
            "user_id",
            "message",
            "access_token",
            "handle",
            "domain",
            "full_handle",
        ],
    )?;
    let did = required_string(&raw, "did")?;
    let user_id = required_string(&raw, "user_id")?;
    let message = required_string(&raw, "message")?;
    let access_token = required_string(&raw, "access_token")?;
    let handle = required_string(&raw, "handle")?;
    let domain = required_string(&raw, "domain")?;
    let full_handle = required_string(&raw, "full_handle")?;
    if did != pending.generated.did.as_str()
        || handle != pending.target_handle
        || domain != pending.target_domain
        || full_handle != format!("{}.{}", pending.target_handle, pending.target_domain)
        || message != "Registration successful"
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::access_token::validate_device_access_token(
        &access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: pending.generated.did.as_str(),
            user_id: &user_id,
            device_id: pending.generated.protocol_device_id.as_str(),
            key_id: &pending.generated.device_signing_key_id,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )?;
    Ok(RegistrationRemoteOutcome::Registered(
        crate::internal::identity_registration_pending::PendingRegistrationRemoteResult {
            did,
            user_id,
            handle,
            full_handle,
            access_token,
        },
    ))
}

fn require_exact_fields(raw: &Value, expected: &[&str]) -> crate::ImResult<()> {
    let object = raw.as_object().ok_or(crate::ImError::PermissionDenied)?;
    if object.len() != expected.len() || !expected.iter().all(|field| object.contains_key(*field)) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn join_required_result(
    request: &crate::identity::RegisterHandleRequest,
    handle: crate::ids::Handle,
    join_required: crate::identity::HandleRegistrationJoinRequired,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    Ok(IdentityRegistrationRuntimeResult {
        sdk_result: crate::identity::HandleRegistrationResult {
            identity: None,
            handle,
            method: registration_method(&request.verification),
            state: crate::identity::HandleRegistrationState::JoinRequired,
            join_required: Some(join_required),
            default_identity_change: None,
            warnings: warnings_for_request(request),
        },
        raw: None,
    })
}

fn apply_registration_reconciliation(
    pending: &mut crate::internal::identity_registration_pending::PendingRegistration,
    reconciliation: crate::internal::transport::PendingRegistrationReconciliation,
) -> crate::ImResult<()> {
    let crate::internal::transport::PendingRegistrationReconciliation::Committed {
        user_id,
        access_token,
    } = reconciliation
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    crate::internal::access_token::validate_device_access_token(
        &access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: pending.generated.did.as_str(),
            user_id: &user_id,
            device_id: pending.generated.protocol_device_id.as_str(),
            key_id: &pending.generated.device_signing_key_id,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )?;
    pending.remote_result = Some(
        crate::internal::identity_registration_pending::PendingRegistrationRemoteResult {
            did: pending.generated.did.as_str().to_owned(),
            user_id,
            handle: pending.target_handle.clone(),
            full_handle: format!("{}.{}", pending.target_handle, pending.target_domain),
            access_token,
        },
    );
    pending.phase =
        crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted;
    pending.validate()
}

fn commit_pending_registration(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    method: crate::identity::RegistrationMethod,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    let previous_default = core.identities().default_identity().ok().flatten();
    let remote = pending
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let storage = crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let stored =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_identity_with_secret_storage(registration_save_input(pending, remote), storage)?;
    registration_result(pending, stored, previous_default, method)
}

async fn commit_pending_registration_async(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    method: crate::identity::RegistrationMethod,
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
    let storage = crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let stored =
        crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
            core.inner().sdk_paths().identities.clone(),
            registration_save_input(pending, remote),
            storage,
        )
        .await?;
    registration_result(pending, stored, previous_default, method)
}

fn registration_save_input(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    remote: &crate::internal::identity_registration_pending::PendingRegistrationRemoteResult,
) -> crate::internal::identity_store::SaveIdentityInput {
    crate::internal::identity_store::SaveIdentityInput {
        local_alias: pending.local_alias.clone(),
        did: pending.generated.did.clone(),
        unique_id: pending.generated.unique_id.clone(),
        user_id: remote.user_id.clone(),
        display_name: pending.display_name.clone(),
        handle: remote.handle.clone(),
        full_handle: remote.full_handle.clone(),
        jwt_token: remote.access_token.clone(),
        did_document: Some(pending.generated.did_document.clone()),
        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
            root_key_id: pending.generated.root_key_id.clone(),
            device_signing_key_id: pending.generated.device_signing_key_id.clone(),
            device_e2ee_key_id: pending.generated.device_e2ee_key_id.clone(),
        },
        device_state: Some(bootstrap_device_state(pending)),
        key1_private_pem: pending.generated.root_private_pem.clone(),
        key1_public_pem: pending.generated.root_public_pem.clone(),
        e2ee_signing_private_pem: pending.generated.device_signing_private_pem.clone(),
        e2ee_agreement_private_pem: pending.generated.device_e2ee_private_pem.clone(),
        daemon_subkey_package: Some(pending.generated.daemon_subkey_package.clone()),
        make_default: pending.make_default,
    }
}

fn bootstrap_device_state(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
) -> crate::internal::identity_device_state::IdentityDeviceState {
    crate::internal::identity_device_state::IdentityDeviceState {
        schema_version:
            crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
        authorization: Some(
            crate::internal::identity_device_state::DeviceAuthorizationProjection {
                protocol_device_id: pending.generated.protocol_device_id.clone(),
                signing_key_id: pending.generated.device_signing_key_id.clone(),
                e2ee_key_id: pending.generated.device_e2ee_key_id.clone(),
                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            },
        ),
        checkpoint: Some(
            crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: pending.document_hash.clone(),
                registry_version: 1,
            },
        ),
    }
}

fn registration_result(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    stored: crate::internal::identity_store::StoredIdentity,
    previous_default: Option<crate::identity::IdentitySummary>,
    method: crate::identity::RegistrationMethod,
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
            method,
            state: crate::identity::HandleRegistrationState::Registered,
            join_required: None,
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
        raw: None,
    })
}

fn publish_v2_prekeys_after_registration(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(crate::ImError::unsupported(
            "sync-registration-prekey-publish-inside-async-runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| crate::ImError::Internal {
            message: format!("create registration PreKey runtime: {error}"),
        })?;
    runtime.block_on(publish_v2_prekeys_after_registration_async(core, did))
}

async fn publish_v2_prekeys_after_registration_async(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    let client = core
        .client_async(crate::identity::IdentitySelector::Did(did.clone()))
        .await?;
    crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published_from_authorized_document(
        core,
        &client,
    )
    .await?;
    Ok(())
}

fn finish_registration_after_prekey_publish<D>(
    publish_result: crate::ImResult<()>,
    delete_pending: D,
) -> crate::ImResult<()>
where
    D: FnOnce() -> crate::ImResult<()>,
{
    publish_result?;
    delete_pending()
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
            Some("handle".to_owned()),
            "handle is required",
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_owned()),
            "DID values are not supported in handle input",
        ));
    }
    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    let (local_part, domain, explicit_domain) = if let Some(dot) = handle.find('.') {
        (
            handle[..dot].trim().to_owned(),
            handle[dot + 1..].trim().trim_end_matches('.').to_owned(),
            true,
        )
    } else {
        (
            handle.to_owned(),
            did_domain.trim().trim_end_matches('.').to_ascii_lowercase(),
            false,
        )
    };
    if local_part.is_empty() || domain.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_owned()),
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
        .to_owned()
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

fn required_string(value: &Value, field: &str) -> crate::ImResult<String> {
    optional_string(value, field).ok_or(crate::ImError::PermissionDenied)
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use time::OffsetDateTime;

    #[derive(Clone, Copy)]
    enum RpcBehavior {
        JoinRequired,
        Lost,
        Succeeds,
    }

    #[derive(Clone, Copy)]
    enum ProbeBehavior {
        Absent,
        Committed,
        MismatchedPrincipal,
    }

    struct RegistrationTransport {
        rpc_behavior: RpcBehavior,
        probe_behavior: ProbeBehavior,
        rpc_calls: usize,
        probe_calls: usize,
    }

    impl RpcTransport for RegistrationTransport {
        fn rpc(&mut self, _endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
            assert_eq!(method, "register");
            self.rpc_calls += 1;
            match self.rpc_behavior {
                RpcBehavior::JoinRequired => Ok(serde_json::json!({
                    "state": "join_required",
                    "handle": "alice",
                    "domain": "example.test",
                    "full_handle": "alice.example.test",
                    "did": "did:wba:example.test:existing",
                    "account_verification_token": "single-use-account-verification"
                })),
                RpcBehavior::Lost => Err(crate::ImError::TransportUnavailable {
                    detail: "response lost after remote commit".to_owned(),
                }),
                RpcBehavior::Succeeds => Err(crate::ImError::Internal {
                    message: "test must synthesize success through reconciliation".to_owned(),
                }),
            }
        }

        fn reconcile_pending_registration(
            &mut self,
            pending: &crate::internal::identity_registration_pending::PendingRegistration,
        ) -> crate::ImResult<crate::internal::transport::PendingRegistrationReconciliation>
        {
            self.probe_calls += 1;
            match self.probe_behavior {
                ProbeBehavior::Absent => {
                    Ok(crate::internal::transport::PendingRegistrationReconciliation::Absent)
                }
                ProbeBehavior::Committed | ProbeBehavior::MismatchedPrincipal => {
                    let key_id =
                        if matches!(self.probe_behavior, ProbeBehavior::MismatchedPrincipal) {
                            format!("{}#different-device-sign", pending.generated.did.as_str())
                        } else {
                            pending.generated.device_signing_key_id.clone()
                        };
                    Ok(
                        crate::internal::transport::PendingRegistrationReconciliation::Committed {
                            user_id: "user-1".to_owned(),
                            access_token: access_token(pending, &key_id),
                        },
                    )
                }
            }
        }
    }

    #[test]
    fn existing_handle_registration_returns_typed_join_required_without_remote_commit() {
        let request = request();
        let mut pending = pending();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::JoinRequired,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };
        let mut persisted = Vec::new();

        let join_required =
            ensure_remote_registration(&mut transport, &mut pending, &request, |state| {
                persisted.push((state.remote_attempted, state.phase));
                Ok(())
            })
            .unwrap()
            .expect("existing handle must enter Join");

        assert_eq!(transport.rpc_calls, 1);
        assert_eq!(transport.probe_calls, 0);
        assert_eq!(join_required.did.as_str(), "did:wba:example.test:existing");
        assert_eq!(
            join_required.account_verification_token,
            "single-use-account-verification"
        );
        assert!(pending.remote_result.is_none());
        assert_eq!(
            pending.phase,
            crate::internal::identity_registration_pending::PendingRegistrationPhase::Prepared
        );
        assert_eq!(
            persisted,
            vec![(
                true,
                crate::internal::identity_registration_pending::PendingRegistrationPhase::Prepared
            )]
        );
    }

    #[test]
    fn response_loss_reconciles_same_device_without_replaying_register() {
        let request = request();
        let mut pending = pending();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Lost,
            probe_behavior: ProbeBehavior::Committed,
            rpc_calls: 0,
            probe_calls: 0,
        };
        let mut persisted = Vec::new();

        ensure_remote_registration(&mut transport, &mut pending, &request, |state| {
            persisted.push((state.remote_attempted, state.phase));
            Ok(())
        })
        .unwrap();

        assert_eq!(transport.rpc_calls, 1);
        assert_eq!(transport.probe_calls, 1);
        assert_eq!(
            persisted,
            vec![
                (
                    true,
                    crate::internal::identity_registration_pending::PendingRegistrationPhase::Prepared
                ),
                (
                    true,
                    crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted
                )
            ]
        );
        assert_eq!(
            pending.remote_result.as_ref().unwrap().access_token,
            access_token(&pending, &pending.generated.device_signing_key_id)
        );
    }

    #[test]
    fn registered_response_accepts_the_existing_closed_user_service_shape() {
        let pending = pending();
        let token = access_token(&pending, &pending.generated.device_signing_key_id);
        let outcome = parse_register_outcome(
            &pending,
            serde_json::json!({
                "state": "registered",
                "did": pending.generated.did.as_str(),
                "user_id": "user-1",
                "message": "Registration successful",
                "handle": "alice",
                "domain": "example.test",
                "full_handle": "alice.example.test",
                "access_token": token,
            }),
        )
        .unwrap();

        let RegistrationRemoteOutcome::Registered(result) = outcome else {
            panic!("new registration must not be projected as Join-required");
        };
        assert_eq!(result.did, pending.generated.did.as_str());
        assert_eq!(result.handle, "alice");
        assert_eq!(result.full_handle, "alice.example.test");
    }

    #[test]
    fn restart_replays_register_only_after_explicit_absence_and_fails_closed_on_mismatch() {
        let request = request();
        let mut absent = pending();
        absent.remote_attempted = true;
        let mut absent_transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Lost,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };

        let error =
            ensure_remote_registration(&mut absent_transport, &mut absent, &request, |_| Ok(()))
                .unwrap_err();

        assert!(matches!(error, crate::ImError::TransportUnavailable { .. }));
        assert_eq!(absent_transport.probe_calls, 2);
        assert_eq!(absent_transport.rpc_calls, 1);

        let mut mismatch = pending();
        mismatch.remote_attempted = true;
        let mut mismatch_transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Succeeds,
            probe_behavior: ProbeBehavior::MismatchedPrincipal,
            rpc_calls: 0,
            probe_calls: 0,
        };

        assert_eq!(
            ensure_remote_registration(&mut mismatch_transport, &mut mismatch, &request, |_| {
                Ok(())
            }),
            Err(crate::ImError::PermissionDenied)
        );
        assert_eq!(mismatch_transport.probe_calls, 1);
        assert_eq!(mismatch_transport.rpc_calls, 0);
    }

    #[test]
    fn local_commit_prekey_failure_keeps_pending_and_retry_skips_register() {
        let request = request();
        let mut pending = pending();
        pending.remote_attempted = true;
        let token = access_token(&pending, &pending.generated.device_signing_key_id);
        apply_registration_reconciliation(
            &mut pending,
            crate::internal::transport::PendingRegistrationReconciliation::Committed {
                user_id: "user-1".to_owned(),
                access_token: token,
            },
        )
        .unwrap();
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        let p5_state_id = format!(
            "{}:{}:{}",
            pending.generated.did.as_str(),
            pending.generated.protocol_device_id.as_str(),
            pending.generated.device_e2ee_key_id
        );
        let mut publish_attempts = Vec::new();
        let mut deleted = 0;

        publish_attempts.push(p5_state_id.clone());
        assert!(finish_registration_after_prekey_publish(
            Err(crate::ImError::TransportUnavailable {
                detail: "message service unavailable".to_owned(),
            }),
            || {
                deleted += 1;
                Ok(())
            },
        )
        .is_err());
        assert_eq!(deleted, 0);
        assert_eq!(
            pending.phase,
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted
        );

        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Succeeds,
            probe_behavior: ProbeBehavior::Committed,
            rpc_calls: 0,
            probe_calls: 0,
        };
        ensure_remote_registration(&mut transport, &mut pending, &request, |_| Ok(())).unwrap();
        assert_eq!(transport.rpc_calls, 0);
        assert_eq!(transport.probe_calls, 0);

        publish_attempts.push(p5_state_id);
        finish_registration_after_prekey_publish(Ok(()), || {
            deleted += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(publish_attempts.len(), 2);
        assert_eq!(publish_attempts[0], publish_attempts[1]);
    }

    fn request() -> crate::identity::RegisterHandleRequest {
        crate::identity::RegisterHandleRequest {
            local_alias: Some("alice".to_owned()),
            requested_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
            verification: crate::identity::VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: crate::identity::InitialProfile {
                display_name: Some("Alice".to_owned()),
                avatar_url: None,
            },
            make_default: true,
        }
    }

    fn pending() -> crate::internal::identity_registration_pending::PendingRegistration {
        let generated =
            crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap();
        crate::internal::identity_registration_pending::PendingRegistration::new(
            "alice".to_owned(),
            "example.test".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            "already_verified".to_owned(),
            None,
            None,
            generated,
        )
        .unwrap()
    }

    fn access_token(
        pending: &crate::internal::identity_registration_pending::PendingRegistration,
        key_id: &str,
    ) -> String {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let claims = serde_json::json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": pending.generated.did.as_str(),
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": pending.generated.did.as_str(),
            "user_id": "user-1",
            "device_id": pending.generated.protocol_device_id.as_str(),
            "key_id": key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 300,
            "jti": "registration-reconciliation-test"
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }
}
