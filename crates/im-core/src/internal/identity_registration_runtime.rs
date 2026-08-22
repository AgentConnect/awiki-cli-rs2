//! Identity registration through the single public `register` RPC.
//!
//! Every completed registration is a vNext identity with one bootstrap
//! Manifest device. `PendingRegistration` keeps the exact generated material
//! restart-safe until the remote and local identity commits have succeeded. An
//! explicit expired-root-proof service reason refreshes only that proof and is
//! retried once; ambiguous transport errors never trigger re-signing.
//! P5 PreKey and optional P6 KeyPackage publication have durable, idempotent
//! local state and are reported as non-fatal completion warnings after that
//! commit boundary.

#[cfg(feature = "group-e2ee")]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
#[cfg(feature = "group-e2ee")]
use sha2::{Digest as _, Sha256};
use std::time::{Duration, Instant};

use crate::internal::transport::{
    AsyncRestTransport, AsyncRpcTransport, RestTransport, RpcTransport,
};

const DEFAULT_EMAIL_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_EMAIL_POLL_INTERVAL: Duration = Duration::from_secs(5);
const REGISTRATION_PREKEY_PUBLISH_PENDING_WARNING: &str = "registration_prekey_publish_pending";
const REGISTRATION_GROUP_KEY_PACKAGE_PUBLISH_PENDING_WARNING: &str =
    "registration_group_key_package_publish_pending";
const REGISTRATION_PENDING_CLEANUP_REQUIRED_WARNING: &str = "registration_pending_cleanup_required";
const REGISTRATION_PROOF_EXPIRED_AWIKI_CODE: &str = "device.document_proof_expired";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRegistrationRuntimeResult {
    pub(crate) sdk_result: crate::identity::HandleRegistrationResult,
    pub(crate) raw: Option<Value>,
}

pub(crate) struct IdentityRegistrationRuntime<'a, T> {
    core: &'a crate::core::ImCore,
    transport: T,
}

pub(crate) struct AnpVNextBootstrapSaveInput<'a> {
    pub(crate) identity:
        &'a crate::internal::identity_registration_pending::PendingRegistrationIdentity,
    pub(crate) document_hash: &'a str,
    pub(crate) local_alias: &'a str,
    pub(crate) display_name: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) handle: &'a str,
    pub(crate) full_handle: &'a str,
    pub(crate) binding_generation: &'a str,
    pub(crate) access_token: &'a str,
    pub(crate) make_default: bool,
}

pub(crate) struct VNextBootstrapSaveInput<'a> {
    pub(crate) generated:
        &'a crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    pub(crate) document_hash: &'a str,
    pub(crate) local_alias: &'a str,
    pub(crate) display_name: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) handle: &'a str,
    pub(crate) full_handle: &'a str,
    pub(crate) binding_generation: &'a str,
    pub(crate) access_token: &'a str,
    pub(crate) make_default: bool,
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
        raw_response: Option<&serde_json::Value>,
    ) -> crate::identity::HandleRegistrationResult {
        let (retry_after_seconds, retry_at) =
            if state == crate::identity::HandleRegistrationState::OtpSent {
                registration_otp_retry(raw_response)
            } else {
                (None, None)
            };
        crate::identity::HandleRegistrationResult {
            identity: None,
            account_id: None,
            handle,
            method,
            state,
            join_required: None,
            default_identity_change: None,
            retry_after_seconds,
            retry_at,
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
                            Some(&raw),
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
                                Some(&raw),
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
                                None,
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
        if let Some(join_required) = ensure_remote_registration(
            self.core,
            &mut self.transport,
            &mut pending,
            &request,
            |pending| store.save(pending).map(|_| ()),
        )? {
            crate::internal::identity_custody::discard_unpublished_registration(
                self.core,
                &pending.identity,
            )?;
            store.delete(&pending_ref)?;
            let preparation = prepare_join_required(self.core, join_required)?;
            return join_required_result(&request, target.full_handle, preparation);
        }
        let mut result = commit_pending_registration(
            self.core,
            &pending,
            registration_method(&request.verification),
        )?;
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        store.save(&pending)?;
        result.sdk_result.warnings.extend(finish_registration(
            publish_v2_messaging_material_after_registration(self.core, &pending.identity.did),
            || store.delete(&pending_ref),
        ));
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
                            Some(&raw),
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
                                Some(&raw),
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
                                None,
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
            load_or_create_pending_registration_async(self.core, &store, &request, &target).await?;
        verify_pending_matches_request(&pending, &request, &target)?;
        if pending.remote_result.is_none() && pending.remote_attempted {
            match self
                .transport
                .reconcile_pending_registration(&pending)
                .await?
            {
                crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                    crate::internal::identity_custody::reconcile_registration_publication_async(
                        self.core,
                        &pending.identity,
                        false,
                    )
                    .await?;
                }
                committed => {
                    crate::internal::identity_custody::reconcile_registration_publication_async(
                        self.core,
                        &pending.identity,
                        true,
                    )
                    .await?;
                    apply_registration_reconciliation(&mut pending, committed)?;
                    store.save(&pending)?;
                }
            }
        }
        if pending.remote_result.is_none() {
            let mut refreshed_expired_proof = false;
            loop {
                crate::internal::identity_custody::begin_registration_publication_async(
                    self.core,
                    &pending.identity,
                )
                .await?;
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
                            crate::internal::identity_custody::commit_registration_publication_async(
                                self.core,
                                &pending.identity,
                            )
                            .await?;
                            pending.remote_result = Some(result);
                            pending.phase =
                                    crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted;
                        }
                        RegistrationRemoteOutcome::JoinRequired(join_required) => {
                            crate::internal::identity_custody::reconcile_registration_publication_async(
                                self.core,
                                &pending.identity,
                                false,
                            )
                            .await?;
                            crate::internal::identity_custody::discard_unpublished_registration_async(
                                self.core,
                                &pending.identity,
                            )
                            .await?;
                            store.delete(&pending_ref)?;
                            let preparation = prepare_join_required(self.core, join_required)?;
                            return join_required_result(&request, target.full_handle, preparation);
                        }
                    },
                    Err(error @ crate::ImError::TransportUnavailable { .. }) => {
                        match self
                            .transport
                            .reconcile_pending_registration(&pending)
                            .await?
                        {
                            crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                                crate::internal::identity_custody::reconcile_registration_publication_async(
                                    self.core,
                                    &pending.identity,
                                    false,
                                )
                                .await?;
                                return Err(error);
                            }
                            committed => {
                                crate::internal::identity_custody::reconcile_registration_publication_async(
                                    self.core,
                                    &pending.identity,
                                    true,
                                )
                                .await?;
                                apply_registration_reconciliation(&mut pending, committed)?
                            }
                        }
                    }
                    Err(error)
                        if !refreshed_expired_proof && registration_proof_expired(&error) =>
                    {
                        crate::internal::identity_custody::reconcile_registration_publication_async(
                            self.core,
                            &pending.identity,
                            false,
                        )
                        .await?;
                        let (document, revision_id) =
                            crate::internal::identity_custody::refresh_registration_document_async(
                                self.core,
                                &pending.identity,
                            )
                            .await?;
                        pending.replace_prepared_document(document, Some(revision_id))?;
                        store.save(&pending)?;
                        refreshed_expired_proof = true;
                        continue;
                    }
                    Err(error) => return Err(error),
                }
                store.save(&pending)?;
                break;
            }
        }
        let mut result = commit_pending_registration_async(
            self.core,
            &pending,
            registration_method(&request.verification),
        )
        .await?;
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        store.save(&pending)?;
        let publish_warnings = publish_v2_messaging_material_after_registration_async(
            self.core,
            &pending.identity.did,
        )
        .await;
        result
            .sdk_result
            .warnings
            .extend(finish_registration(publish_warnings, || {
                store.delete(&pending_ref)
            }));
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
    let identity = crate::internal::identity_custody::provision_registration_identity(
        core,
        &target.effective_domain,
        &target.local_part,
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
        identity,
    )?;
    let secret_ref = store.save(&pending)?;
    Ok((secret_ref, pending))
}

async fn load_or_create_pending_registration_async(
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
    let identity = crate::internal::identity_custody::provision_registration_identity_async(
        core,
        &target.effective_domain,
        &target.local_part,
    )
    .await?;
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
        identity,
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
            did_document: pending.identity.did_document.clone(),
            handle: pending.target_handle.clone(),
            phone: registration_phone(&request.verification),
            otp_code: registration_otp(&request.verification),
            email: registration_email(&request.verification),
            invite_code: request.invite_code.clone().unwrap_or_default(),
        },
    )
}

fn ensure_remote_registration<T, P>(
    core: &crate::core::ImCore,
    transport: &mut T,
    pending: &mut crate::internal::identity_registration_pending::PendingRegistration,
    request: &crate::identity::RegisterHandleRequest,
    mut persist: P,
) -> crate::ImResult<Option<ParsedRegistrationJoinRequired>>
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
            crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                crate::internal::identity_custody::reconcile_registration_publication(
                    core,
                    &pending.identity,
                    false,
                )?;
            }
            committed => {
                crate::internal::identity_custody::reconcile_registration_publication(
                    core,
                    &pending.identity,
                    true,
                )?;
                apply_registration_reconciliation(pending, committed)?;
                persist(pending)?;
                return Ok(None);
            }
        }
    }

    let mut refreshed_expired_proof = false;
    loop {
        // Persist before the first byte is sent. A process crash or lost response
        // must enter signed reconciliation on restart and must never blindly
        // replay register.
        crate::internal::identity_custody::begin_registration_publication(core, &pending.identity)?;
        pending.remote_attempted = true;
        persist(pending)?;
        let call = register_call(pending, request)?;
        match transport.rpc(call.endpoint, call.method, call.params) {
            Ok(raw) => match parse_register_outcome(pending, raw)? {
                RegistrationRemoteOutcome::Registered(result) => {
                    crate::internal::identity_custody::commit_registration_publication(
                        core,
                        &pending.identity,
                    )?;
                    pending.remote_result = Some(result);
                    pending.phase =
                            crate::internal::identity_registration_pending::PendingRegistrationPhase::RemoteCommitted;
                }
                RegistrationRemoteOutcome::JoinRequired(join_required) => {
                    crate::internal::identity_custody::reconcile_registration_publication(
                        core,
                        &pending.identity,
                        false,
                    )?;
                    return Ok(Some(join_required));
                }
            },
            Err(error @ crate::ImError::TransportUnavailable { .. }) => {
                match transport.reconcile_pending_registration(pending)? {
                    crate::internal::transport::PendingRegistrationReconciliation::Absent => {
                        crate::internal::identity_custody::reconcile_registration_publication(
                            core,
                            &pending.identity,
                            false,
                        )?;
                        return Err(error);
                    }
                    committed => {
                        crate::internal::identity_custody::reconcile_registration_publication(
                            core,
                            &pending.identity,
                            true,
                        )?;
                        apply_registration_reconciliation(pending, committed)?
                    }
                }
            }
            Err(error) if !refreshed_expired_proof && registration_proof_expired(&error) => {
                crate::internal::identity_custody::reconcile_registration_publication(
                    core,
                    &pending.identity,
                    false,
                )?;
                let (document, revision_id) =
                    crate::internal::identity_custody::refresh_registration_document(
                        core,
                        &pending.identity,
                    )?;
                pending.replace_prepared_document(document, Some(revision_id))?;
                persist(pending)?;
                refreshed_expired_proof = true;
                continue;
            }
            Err(error) => return Err(error),
        }
        break;
    }
    persist(pending)?;
    Ok(None)
}

fn registration_proof_expired(error: &crate::ImError) -> bool {
    let crate::ImError::Service {
        data: Some(data), ..
    } = error
    else {
        return false;
    };
    data.get("awiki_code").and_then(Value::as_str) == Some(REGISTRATION_PROOF_EXPIRED_AWIKI_CODE)
}

enum RegistrationRemoteOutcome {
    Registered(crate::internal::identity_registration_pending::PendingRegistrationRemoteResult),
    JoinRequired(ParsedRegistrationJoinRequired),
}

struct ParsedRegistrationJoinRequired {
    raw_result_hash: String,
    did: crate::ids::Did,
    full_handle: crate::ids::Handle,
    account_verification_token: crate::internal::platform_secret::SecretBytes,
    transition:
        Option<crate::internal::identity_registration_join_preparation::RegistrationJoinTransition>,
}

impl std::fmt::Debug for ParsedRegistrationJoinRequired {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedRegistrationJoinRequired")
            .field("raw_result_hash", &self.raw_result_hash)
            .field("did", &self.did)
            .field("full_handle", &self.full_handle)
            .field("account_verification_token", &"<redacted>")
            .field("transition", &self.transition)
            .finish()
    }
}

fn parse_register_outcome(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    raw: Value,
) -> crate::ImResult<RegistrationRemoteOutcome> {
    let state = required_string(&raw, "state")?;
    if state == "join_required" {
        let has_account = raw.get("account_user_id").is_some();
        let has_transition = raw.get("identity_transition").is_some();
        if has_account != has_transition {
            return Err(crate::ImError::PermissionDenied);
        }
        let expected = if has_account {
            &[
                "state",
                "handle",
                "domain",
                "full_handle",
                "did",
                "account_verification_token",
                "account_user_id",
                "identity_transition",
            ][..]
        } else {
            &[
                "state",
                "handle",
                "domain",
                "full_handle",
                "did",
                "account_verification_token",
            ][..]
        };
        require_exact_fields(&raw, expected)?;
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
        let transition = if has_account {
            let account_user_id = required_string(&raw, "account_user_id")?;
            let raw_transition = raw
                .get("identity_transition")
                .ok_or(crate::ImError::PermissionDenied)?;
            require_exact_fields(
                raw_transition,
                &["kind", "previous_did", "current_did", "binding_generation"],
            )?;
            if required_string(raw_transition, "kind")? != "handle_recovery" {
                return Err(crate::ImError::PermissionDenied);
            }
            let previous_did =
                crate::ids::Did::parse(required_string(raw_transition, "previous_did")?)?
                    .as_str()
                    .to_owned();
            let current_did =
                crate::ids::Did::parse(required_string(raw_transition, "current_did")?)?
                    .as_str()
                    .to_owned();
            let binding_generation = anp::wns::BindingGeneration::new(required_string(
                raw_transition,
                "binding_generation",
            )?)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .to_string();
            if previous_did == current_did || current_did != did.as_str() {
                return Err(crate::ImError::PermissionDenied);
            }
            Some(
                crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
                    account_user_id,
                    previous_did,
                    current_did,
                    binding_generation,
                },
            )
        } else {
            None
        };
        return Ok(RegistrationRemoteOutcome::JoinRequired(
            ParsedRegistrationJoinRequired {
                raw_result_hash: crate::internal::identity_registration_join_preparation::registration_result_hash(&raw)?,
                did,
                full_handle: crate::ids::Handle::parse(&full_handle, "")?,
                account_verification_token:
                    crate::internal::platform_secret::SecretBytes::from_vec(token.into_bytes()),
                transition,
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
            "binding_generation",
        ],
    )?;
    let did = required_string(&raw, "did")?;
    let user_id = required_string(&raw, "user_id")?;
    // `message` is a required diagnostic field, not registration authority.
    // Identity and device binding are proven by the fields and access token
    // validated below.
    required_string(&raw, "message")?;
    let access_token = required_string(&raw, "access_token")?;
    let handle = required_string(&raw, "handle")?;
    let domain = required_string(&raw, "domain")?;
    let full_handle = required_string(&raw, "full_handle")?;
    let binding_generation =
        anp::wns::BindingGeneration::new(required_string(&raw, "binding_generation")?.to_owned())
            .map_err(|_| crate::ImError::PermissionDenied)?
            .to_string();
    if did != pending.identity.did.as_str()
        || handle != pending.target_handle
        || domain != pending.target_domain
        || full_handle != format!("{}.{}", pending.target_handle, pending.target_domain)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::access_token::validate_device_access_token(
        &access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: pending.identity.did.as_str(),
            user_id: &user_id,
            device_id: pending.identity.protocol_device_id.as_str(),
            key_id: &pending.identity.device_signing_key_id,
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
            binding_generation,
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

fn registration_otp_retry(
    raw_response: Option<&serde_json::Value>,
) -> (Option<u32>, Option<String>) {
    let Some(raw) = raw_response else {
        return (None, None);
    };
    let value = raw.get("result").unwrap_or(raw);
    let retry_after_seconds = value
        .get("retry_after_seconds")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let retry_at = value
        .get("retry_at")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    (retry_after_seconds, retry_at)
}

fn join_required_result(
    request: &crate::identity::RegisterHandleRequest,
    handle: crate::ids::Handle,
    join_required: crate::identity::HandleRegistrationJoinRequiredPreparation,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    Ok(IdentityRegistrationRuntimeResult {
        sdk_result: crate::identity::HandleRegistrationResult {
            identity: None,
            account_id: None,
            handle,
            method: registration_method(&request.verification),
            state: crate::identity::HandleRegistrationState::JoinRequired,
            join_required: Some(join_required),
            default_identity_change: None,
            retry_after_seconds: None,
            retry_at: None,
            warnings: warnings_for_request(request),
        },
        raw: None,
    })
}

fn prepare_join_required(
    core: &crate::core::ImCore,
    parsed: ParsedRegistrationJoinRequired,
) -> crate::ImResult<crate::identity::HandleRegistrationJoinRequiredPreparation> {
    use crate::internal::identity_local_owner_matcher::{StableOwnerAuthority, StableOwnerMatch};

    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let (mode, owner_identity_id) = match parsed.transition.as_ref() {
        Some(transition) => match crate::internal::identity_local_owner_matcher::match_stable_owner(
            sqlite_path,
            &index,
            StableOwnerAuthority {
                account_user_id: &transition.account_user_id,
                full_handle: parsed.full_handle.as_str(),
                previous_did: &transition.previous_did,
                binding_generation: &transition.binding_generation,
            },
            None,
            None,
        )? {
            StableOwnerMatch::Exact(owner) => (
                crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind,
                Some(owner.owner_identity_id),
            ),
            StableOwnerMatch::None => {
                (crate::identity::HandleRegistrationJoinMode::Ordinary, None)
            }
            StableOwnerMatch::Conflict => {
                return Err(crate::internal::identity_registration_join_preparation::continuity_error(
                    "handle_recovery.local_state_conflict",
                ));
            }
        },
        None => match crate::internal::identity_local_owner_matcher::match_stable_owner_without_transition(
            sqlite_path,
            &core.inner().sdk_paths().identities.identity_root_dir,
            &index,
            parsed.full_handle.as_str(),
            parsed.did.as_str(),
        )? {
            StableOwnerMatch::None => {
                (crate::identity::HandleRegistrationJoinMode::Ordinary, None)
            }
            StableOwnerMatch::Exact(_) | StableOwnerMatch::Conflict => {
                return Err(crate::internal::identity_registration_join_preparation::continuity_error(
                    "handle_recovery.transition_missing",
                ));
            }
        },
    };
    core.inner().registration_join_preparations.issue(
        crate::internal::identity_registration_join_preparation::RegistrationJoinPreparationInput {
            raw_result_hash: parsed.raw_result_hash,
            expected_did: parsed.did,
            full_handle: parsed.full_handle,
            account_verification_token: parsed.account_verification_token,
            transition: parsed.transition,
            mode,
            owner_identity_id,
            state_root_fingerprint:
                crate::internal::identity_transition_pending::state_root_fingerprint(sqlite_path),
            identity_index_fingerprint:
                crate::internal::identity_registration_join_preparation::identity_index_fingerprint(
                    &index,
                )?,
        },
    )
}

fn apply_registration_reconciliation(
    pending: &mut crate::internal::identity_registration_pending::PendingRegistration,
    reconciliation: crate::internal::transport::PendingRegistrationReconciliation,
) -> crate::ImResult<()> {
    let crate::internal::transport::PendingRegistrationReconciliation::Committed {
        user_id,
        binding_generation,
        access_token,
    } = reconciliation
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    crate::internal::access_token::validate_device_access_token(
        &access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: pending.identity.did.as_str(),
            user_id: &user_id,
            device_id: pending.identity.protocol_device_id.as_str(),
            key_id: &pending.identity.device_signing_key_id,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )?;
    pending.remote_result = Some(
        crate::internal::identity_registration_pending::PendingRegistrationRemoteResult {
            did: pending.identity.did.as_str().to_owned(),
            user_id,
            handle: pending.target_handle.clone(),
            full_handle: format!("{}.{}", pending.target_handle, pending.target_domain),
            binding_generation,
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
    let storage = crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
        core,
        pending.identity.controller_store_id.clone(),
        pending.identity.controller_identity_id.clone(),
    )?;
    let stored =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_anp_identity_projection(registration_save_input(pending, remote)?, storage)?;
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
    let core = core.clone();
    let identity = pending.identity.clone();
    let input = registration_save_input(pending, remote)?;
    let stored = tokio::task::spawn_blocking(move || {
        let storage = crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
            &core,
            identity.controller_store_id,
            identity.controller_identity_id,
        )?;
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_anp_identity_projection(input, storage)
    })
    .await
    .map_err(|_| crate::ImError::Internal {
        message: "registration projection task failed".to_owned(),
    })??;
    registration_result(pending, stored, previous_default, method)
}

fn registration_save_input(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    remote: &crate::internal::identity_registration_pending::PendingRegistrationRemoteResult,
) -> crate::ImResult<crate::internal::identity_store::SaveIdentityInput> {
    anp_vnext_bootstrap_save_input(AnpVNextBootstrapSaveInput {
        identity: &pending.identity,
        document_hash: &pending.document_hash,
        local_alias: &pending.local_alias,
        display_name: &pending.display_name,
        user_id: &remote.user_id,
        handle: &remote.handle,
        full_handle: &remote.full_handle,
        binding_generation: &remote.binding_generation,
        access_token: &remote.access_token,
        make_default: pending.make_default,
    })
}

pub(crate) fn anp_vnext_bootstrap_save_input(
    input: AnpVNextBootstrapSaveInput<'_>,
) -> crate::ImResult<crate::internal::identity_store::SaveIdentityInput> {
    let identity = input.identity;
    identity.validate()?;
    if crate::internal::identity_wire::document::document_hash(&identity.did_document)?
        != input.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let unique_id = identity
        .did
        .as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    let handle = crate::ids::Handle::parse(input.full_handle, "")?;
    crate::core::validate_handle_service_for_did(&identity.did_document, &identity.did, &handle)?;
    let device_state = crate::internal::identity_device_state::IdentityDeviceState {
        schema_version:
            crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
        authorization: Some(
            crate::internal::identity_device_state::DeviceAuthorizationProjection {
                protocol_device_id: identity.protocol_device_id.clone(),
                signing_key_id: identity.device_signing_key_id.clone(),
                e2ee_key_id: identity.device_e2ee_key_id.clone(),
                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            },
        ),
        checkpoint: Some(
            crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: input.document_hash.to_owned(),
                registry_version: 1,
            },
        ),
    };
    Ok(crate::internal::identity_store::SaveIdentityInput {
        local_alias: input.local_alias.to_owned(),
        did: identity.did.clone(),
        unique_id: unique_id.to_owned(),
        user_id: input.user_id.to_owned(),
        display_name: input.display_name.to_owned(),
        handle: input.handle.to_owned(),
        full_handle: input.full_handle.to_owned(),
        binding_generation: Some(input.binding_generation.to_owned()),
        jwt_token: input.access_token.to_owned(),
        did_document: Some(identity.did_document.clone()),
        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
            root_key_id: identity.root_key_id.clone(),
            device_signing_key_id: identity.device_signing_key_id.clone(),
            device_e2ee_key_id: identity.device_e2ee_key_id.clone(),
        },
        device_state: Some(device_state),
        key1_private_pem: String::new(),
        key1_public_pem: String::new(),
        e2ee_signing_private_pem: String::new(),
        e2ee_agreement_private_pem: String::new(),
        daemon_subkey_package: None,
        make_default: input.make_default,
    })
}

pub(crate) fn vnext_bootstrap_save_input(
    input: VNextBootstrapSaveInput<'_>,
) -> crate::ImResult<crate::internal::identity_store::SaveIdentityInput> {
    let generated = input.generated;
    if crate::internal::identity_wire::document::document_hash(&generated.did_document)?
        != input.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_identity_id = generated
        .did
        .as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    if generated.unique_id != expected_identity_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let handle = crate::ids::Handle::parse(input.full_handle, "")?;
    crate::core::validate_handle_service_for_did(&generated.did_document, &generated.did, &handle)?;
    let _validated_provider = crate::internal::key_provider::HostBackedDeviceIdentitySigner::new(
        &crate::identity::HostBackedDeviceIdentityMaterial {
            identity_id: generated.unique_id.clone(),
            did: generated.did.as_str().to_owned(),
            handle: Some(input.full_handle.to_owned()),
            display_name: Some(input.display_name.to_owned()),
            account_id: input.user_id.to_owned(),
            binding_generation: input.binding_generation.to_owned(),
            did_document: generated.did_document.clone(),
            protocol_device_id: generated.protocol_device_id.clone(),
            device_signing_key_id: generated.device_signing_key_id.clone(),
            device_signing_private_key_pem: generated.device_signing_private_pem.clone(),
            device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
            device_e2ee_private_key_pem: generated.device_e2ee_private_pem.clone(),
            root_key_id: generated.root_key_id.clone(),
            root_private_key_pem: generated.root_private_pem.clone(),
            authorization_status: crate::identity::IdentityDeviceAuthorizationStatus::Active,
            role: crate::identity::IdentityDeviceRole::Admin,
            management_ready: true,
            auth_generation: "1".to_owned(),
            access_token: input.access_token.to_owned(),
        },
    )?;
    let device_state = crate::internal::identity_device_state::IdentityDeviceState {
        schema_version:
            crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
        authorization: Some(
            crate::internal::identity_device_state::DeviceAuthorizationProjection {
                protocol_device_id: generated.protocol_device_id.clone(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            },
        ),
        checkpoint: Some(
            crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: input.document_hash.to_owned(),
                registry_version: 1,
            },
        ),
    };
    Ok(crate::internal::identity_store::SaveIdentityInput {
        local_alias: input.local_alias.to_owned(),
        did: generated.did.clone(),
        unique_id: generated.unique_id.clone(),
        user_id: input.user_id.to_owned(),
        display_name: input.display_name.to_owned(),
        handle: input.handle.to_owned(),
        full_handle: input.full_handle.to_owned(),
        binding_generation: Some(input.binding_generation.to_owned()),
        jwt_token: input.access_token.to_owned(),
        did_document: Some(generated.did_document.clone()),
        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
            root_key_id: generated.root_key_id.clone(),
            device_signing_key_id: generated.device_signing_key_id.clone(),
            device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
        },
        device_state: Some(device_state),
        key1_private_pem: generated.root_private_pem.clone(),
        key1_public_pem: generated.root_public_pem.clone(),
        e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
        e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
        daemon_subkey_package: Some(generated.daemon_subkey_package.clone()),
        make_default: input.make_default,
    })
}

fn registration_result(
    pending: &crate::internal::identity_registration_pending::PendingRegistration,
    stored: crate::internal::identity_store::StoredIdentity,
    previous_default: Option<crate::identity::IdentitySummary>,
    method: crate::identity::RegistrationMethod,
) -> crate::ImResult<IdentityRegistrationRuntimeResult> {
    let mut identity = identity_summary_from_stored(&stored)?;
    identity.device_id = Some(pending.identity.protocol_device_id.as_str().to_owned());
    identity.readiness.ready_for_auth = true;
    let handle = crate::ids::Handle::parse(
        format!("{}.{}", pending.target_handle, pending.target_domain),
        "",
    )?;
    Ok(IdentityRegistrationRuntimeResult {
        sdk_result: crate::identity::HandleRegistrationResult {
            identity: Some(identity.clone()),
            account_id: Some(
                pending
                    .remote_result
                    .as_ref()
                    .ok_or(crate::ImError::PermissionDenied)?
                    .user_id
                    .clone(),
            ),
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
            retry_after_seconds: None,
            retry_at: None,
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

fn publish_v2_messaging_material_after_registration(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> Vec<String> {
    let group_e2ee_enabled = core.inner().group_e2ee_v2_enabled();
    if tokio::runtime::Handle::try_current().is_ok() {
        return registration_messaging_material_unavailable_warnings(group_e2ee_enabled);
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return registration_messaging_material_unavailable_warnings(group_e2ee_enabled);
    };
    runtime.block_on(publish_v2_messaging_material_after_registration_async(
        core, did,
    ))
}

async fn publish_v2_messaging_material_after_registration_async(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> Vec<String> {
    let prekey_result = publish_v2_prekeys_after_registration_async(core, did).await;
    let group_key_package_result = if core.inner().group_e2ee_v2_enabled() {
        Some(publish_v2_group_key_package_after_registration_async(core, did).await)
    } else {
        None
    };
    registration_messaging_material_warnings(prekey_result, group_key_package_result)
}

async fn publish_v2_group_key_package_after_registration_async(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    #[cfg(feature = "group-e2ee")]
    {
        let client = core
            .client_async(crate::identity::IdentitySelector::Did(did.clone()))
            .await?;
        let device_id = client.exact_protocol_device_id()?;
        let (operation_id, key_package_id) =
            deterministic_registration_group_key_package_ids(did, &device_id);
        crate::internal::group_e2ee::v2_lifecycle::publish_stable_key_package(
            &client,
            &device_id,
            &operation_id,
            &key_package_id,
        )
        .await
    }
    #[cfg(not(feature = "group-e2ee"))]
    {
        let _ = (core, did);
        Err(crate::ImError::invalid_input(
            Some("multi_device_group_e2ee_enabled".to_owned()),
            "Group E2EE v2 requires the group-e2ee build feature",
        ))
    }
}

#[cfg(feature = "group-e2ee")]
fn deterministic_registration_group_key_package_ids(
    did: &crate::ids::Did,
    device_id: &str,
) -> (String, String) {
    let digest = |kind: &str| {
        let mut digest = Sha256::new();
        for value in [
            "awiki.identity.registration.p6-key-package.v1",
            kind,
            did.as_str(),
            device_id,
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        base64::Engine::encode(&URL_SAFE_NO_PAD, digest.finalize())
    };
    (
        format!("registration-p6-publish-{}", digest("operation")),
        format!("registration-kp-{}", digest("key-package")),
    )
}

pub(crate) async fn publish_v2_prekeys_after_registration_async(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    let client = core
        .client_async(crate::identity::IdentitySelector::Did(did.clone()))
        .await?;
    let has_device_authorization = client.runtime().owner.sync_account.is_some();
    let has_valid_bearer = client.runtime().key_provider.valid_auth_token()?.is_some();
    if registration_prekey_access_requires_refresh(has_device_authorization, has_valid_bearer) {
        let mut auth = crate::internal::transport::CoreHttpTransport::new_signature_only(&client);
        auth.refresh_jwt_async().await?;
    }
    crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published_from_authorized_document(
        core,
        &client,
    )
    .await?;
    Ok(())
}

fn registration_prekey_access_requires_refresh(
    has_device_authorization: bool,
    has_valid_bearer: bool,
) -> bool {
    has_device_authorization && !has_valid_bearer
}

fn registration_messaging_material_warnings(
    prekey_result: crate::ImResult<()>,
    group_key_package_result: Option<crate::ImResult<()>>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if prekey_result.is_err() {
        warnings.push(REGISTRATION_PREKEY_PUBLISH_PENDING_WARNING.to_owned());
    }
    if group_key_package_result.is_some_and(|result| result.is_err()) {
        warnings.push(REGISTRATION_GROUP_KEY_PACKAGE_PUBLISH_PENDING_WARNING.to_owned());
    }
    warnings
}

fn registration_messaging_material_unavailable_warnings(group_e2ee_enabled: bool) -> Vec<String> {
    let mut warnings = vec![REGISTRATION_PREKEY_PUBLISH_PENDING_WARNING.to_owned()];
    if group_e2ee_enabled {
        warnings.push(REGISTRATION_GROUP_KEY_PACKAGE_PUBLISH_PENDING_WARNING.to_owned());
    }
    warnings
}

fn finish_registration<D>(mut warnings: Vec<String>, delete_pending: D) -> Vec<String>
where
    D: FnOnce() -> crate::ImResult<()>,
{
    if delete_pending().is_err() {
        warnings.push(REGISTRATION_PENDING_CLEANUP_REQUIRED_WARNING.to_owned());
    }
    warnings
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

    #[test]
    fn registration_otp_retry_preserves_server_guidance() {
        let raw = serde_json::json!({
            "result": {
                "ok": true,
                "retry_after_seconds": 60,
                "retry_at": "2099-08-20T12:00:00Z"
            }
        });
        assert_eq!(
            registration_otp_retry(Some(&raw)),
            (Some(60), Some("2099-08-20T12:00:00Z".to_owned()))
        );
        assert_eq!(registration_otp_retry(None), (None, None));
        assert_eq!(
            registration_otp_retry(Some(&serde_json::json!({
                "retry_after_seconds": -1,
                "retry_at": " "
            }))),
            (None, None)
        );
    }

    #[derive(Clone, Copy)]
    enum RpcBehavior {
        AlwaysExpired,
        ExpiredThenJoinRequired,
        JoinRequired,
        Lost,
        OtherServiceError,
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
                RpcBehavior::AlwaysExpired => Err(expired_proof_error()),
                RpcBehavior::ExpiredThenJoinRequired if self.rpc_calls == 1 => {
                    Err(expired_proof_error())
                }
                RpcBehavior::ExpiredThenJoinRequired | RpcBehavior::JoinRequired => {
                    Ok(serde_json::json!({
                        "state": "join_required",
                        "handle": "alice",
                        "domain": "example.test",
                        "full_handle": "alice.example.test",
                        "did": "did:wba:example.test:existing",
                        "account_verification_token": "single-use-account-verification"
                    }))
                }
                RpcBehavior::Lost => Err(crate::ImError::TransportUnavailable {
                    detail: "response lost after remote commit".to_owned(),
                }),
                RpcBehavior::OtherServiceError => Err(crate::ImError::Service {
                    status_code: Some(200),
                    code: Some("-32004".to_owned()),
                    message: "DID document root proof has expired".to_owned(),
                    data: Some(serde_json::json!({
                        "awiki_code": "device.document_invalid"
                    })),
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
                            format!("{}#different-device-sign", pending.identity.did.as_str())
                        } else {
                            pending.identity.device_signing_key_id.clone()
                        };
                    Ok(
                        crate::internal::transport::PendingRegistrationReconciliation::Committed {
                            user_id: "user-1".to_owned(),
                            binding_generation: "1".to_owned(),
                            access_token: access_token(pending, &key_id),
                        },
                    )
                }
            }
        }
    }

    #[test]
    fn registration_recovery_join_parses_ordinary_closed_shape_without_remote_commit() {
        let request = request();
        let (_root, core, mut pending) = pending_with_core();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::JoinRequired,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };
        let mut persisted = Vec::new();

        let join_required =
            ensure_remote_registration(&core, &mut transport, &mut pending, &request, |state| {
                persisted.push((state.remote_attempted, state.phase));
                Ok(())
            })
            .unwrap()
            .expect("existing handle must enter Join");

        assert_eq!(transport.rpc_calls, 1);
        assert_eq!(transport.probe_calls, 0);
        assert_eq!(join_required.did.as_str(), "did:wba:example.test:existing");
        assert_eq!(
            join_required.account_verification_token.expose_secret(),
            b"single-use-account-verification"
        );
        assert!(join_required.transition.is_none());
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
    fn registration_recovery_join_accepts_only_the_closed_recovery_shape() {
        let pending = pending();
        let recovery = parse_register_outcome(
            &pending,
            serde_json::json!({
                "state": "join_required",
                "handle": "alice",
                "domain": "example.test",
                "full_handle": "alice.example.test",
                "did": "did:wba:example.test:existing",
                "account_verification_token": "single-use-account-verification",
                "account_user_id": "account-alice",
                "identity_transition": {
                    "kind": "handle_recovery",
                    "previous_did": "did:wba:example.test:previous",
                    "current_did": "did:wba:example.test:existing",
                    "binding_generation": "8"
                }
            }),
        )
        .unwrap();
        let RegistrationRemoteOutcome::JoinRequired(recovery) = recovery else {
            panic!("Recovery registration must require Join");
        };
        let transition = recovery.transition.expect("closed Recovery transition");
        assert_eq!(transition.account_user_id, "account-alice");
        assert_eq!(transition.previous_did, "did:wba:example.test:previous");
        assert_eq!(transition.binding_generation, "8");

        let malformed = parse_register_outcome(
            &pending,
            serde_json::json!({
                "state": "join_required",
                "handle": "alice",
                "domain": "example.test",
                "full_handle": "alice.example.test",
                "did": "did:wba:example.test:existing",
                "account_verification_token": "single-use-account-verification",
                "account_user_id": "account-alice"
            }),
        );
        assert!(matches!(malformed, Err(crate::ImError::PermissionDenied)));
    }

    #[test]
    fn response_loss_reconciles_same_device_without_replaying_register() {
        let request = request();
        let (_root, core, mut pending) = pending_with_core();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Lost,
            probe_behavior: ProbeBehavior::Committed,
            rpc_calls: 0,
            probe_calls: 0,
        };
        let mut persisted = Vec::new();

        ensure_remote_registration(&core, &mut transport, &mut pending, &request, |state| {
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
            access_token(&pending, &pending.identity.device_signing_key_id)
        );
    }

    #[test]
    fn expired_root_proof_refreshes_same_identity_and_retries_once() {
        let request = request();
        let (_root, core, mut expired_pending) = pending_with_core();
        let original = expired_pending.identity.clone();
        let original_hash = expired_pending.document_hash.clone();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::ExpiredThenJoinRequired,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };
        let mut persisted = Vec::new();

        let outcome = ensure_remote_registration(
            &core,
            &mut transport,
            &mut expired_pending,
            &request,
            |state| {
                persisted.push(state.remote_attempted);
                Ok(())
            },
        )
        .unwrap();

        assert!(outcome.is_some());
        assert_eq!(transport.rpc_calls, 2);
        assert_eq!(transport.probe_calls, 0);
        assert_eq!(expired_pending.identity.did, original.did);
        assert_eq!(
            expired_pending.identity.controller_identity_id,
            original.controller_identity_id
        );
        assert_eq!(
            expired_pending.identity.device_signing_key_id,
            original.device_signing_key_id
        );
        assert_eq!(
            expired_pending.identity.device_e2ee_key_id,
            original.device_e2ee_key_id
        );
        assert_ne!(expired_pending.document_hash, original_hash);
        assert_eq!(persisted, vec![true, false, true]);
    }

    #[test]
    fn expired_root_proof_retry_is_capped_and_other_errors_do_not_refresh() {
        let request = request();
        let (_expired_root, expired_core, mut expired_pending) = pending_with_core();
        let mut transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::AlwaysExpired,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };

        let error = ensure_remote_registration(
            &expired_core,
            &mut transport,
            &mut expired_pending,
            &request,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(registration_proof_expired(&error));
        assert_eq!(transport.rpc_calls, 2);

        let (_other_root, other_core, mut other_pending) = pending_with_core();
        let original_hash = other_pending.document_hash.clone();
        let mut other_transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::OtherServiceError,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };

        let error = ensure_remote_registration(
            &other_core,
            &mut other_transport,
            &mut other_pending,
            &request,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(!registration_proof_expired(&error));
        assert_eq!(other_transport.rpc_calls, 1);
        assert_eq!(other_pending.document_hash, original_hash);
    }

    #[test]
    fn registered_response_treats_message_as_non_authoritative_diagnostics() {
        let pending = pending();
        for message in [
            "Registration successful",
            "Legacy Handle recovered successfully",
        ] {
            let token = access_token(&pending, &pending.identity.device_signing_key_id);
            let outcome = parse_register_outcome(
                &pending,
                serde_json::json!({
                    "state": "registered",
                    "did": pending.identity.did.as_str(),
                    "user_id": "user-1",
                    "message": message,
                    "handle": "alice",
                    "domain": "example.test",
                    "full_handle": "alice.example.test",
                    "binding_generation": "1",
                    "access_token": token,
                }),
            )
            .unwrap();

            let RegistrationRemoteOutcome::Registered(result) = outcome else {
                panic!("registered response must not be projected as Join-required");
            };
            assert_eq!(result.did, pending.identity.did.as_str());
            assert_eq!(result.handle, "alice");
            assert_eq!(result.full_handle, "alice.example.test");
        }
    }

    #[test]
    fn registered_response_still_rejects_authoritative_binding_mismatch() {
        let pending = pending();
        let token = access_token(&pending, &pending.identity.device_signing_key_id);
        let error = match parse_register_outcome(
            &pending,
            serde_json::json!({
                "state": "registered",
                "did": pending.identity.did.as_str(),
                "user_id": "user-1",
                "message": "Legacy Handle recovered successfully",
                "handle": "mallory",
                "domain": "example.test",
                "full_handle": "mallory.example.test",
                "binding_generation": "1",
                "access_token": token,
            }),
        ) {
            Ok(_) => panic!("authoritative binding mismatch must fail closed"),
            Err(error) => error,
        };

        assert_eq!(error, crate::ImError::PermissionDenied);
    }

    #[test]
    fn restart_replays_register_only_after_explicit_absence_and_fails_closed_on_mismatch() {
        let request = request();
        let (_absent_root, absent_core, mut absent) = pending_with_core();
        absent.remote_attempted = true;
        let mut absent_transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Lost,
            probe_behavior: ProbeBehavior::Absent,
            rpc_calls: 0,
            probe_calls: 0,
        };

        let error = ensure_remote_registration(
            &absent_core,
            &mut absent_transport,
            &mut absent,
            &request,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(matches!(error, crate::ImError::TransportUnavailable { .. }));
        assert_eq!(absent_transport.probe_calls, 2);
        assert_eq!(absent_transport.rpc_calls, 1);

        let (_mismatch_root, mismatch_core, mut mismatch) = pending_with_core();
        mismatch.remote_attempted = true;
        let mut mismatch_transport = RegistrationTransport {
            rpc_behavior: RpcBehavior::Succeeds,
            probe_behavior: ProbeBehavior::MismatchedPrincipal,
            rpc_calls: 0,
            probe_calls: 0,
        };

        assert!(matches!(
            ensure_remote_registration(
                &mismatch_core,
                &mut mismatch_transport,
                &mut mismatch,
                &request,
                |_| Ok(())
            ),
            Err(crate::ImError::PermissionDenied)
        ));
        assert_eq!(mismatch_transport.probe_calls, 1);
        assert_eq!(mismatch_transport.rpc_calls, 0);
    }

    #[test]
    fn local_commit_prekey_failure_returns_success_warning_and_finishes_pending() {
        let request = request();
        let (_root, core, mut pending) = pending_with_core();
        pending.remote_attempted = true;
        let token = access_token(&pending, &pending.identity.device_signing_key_id);
        apply_registration_reconciliation(
            &mut pending,
            crate::internal::transport::PendingRegistrationReconciliation::Committed {
                user_id: "user-1".to_owned(),
                binding_generation: "1".to_owned(),
                access_token: token,
            },
        )
        .unwrap();
        pending.phase =
            crate::internal::identity_registration_pending::PendingRegistrationPhase::LocalCommitted;
        let p5_state_id = format!(
            "{}:{}:{}",
            pending.identity.did.as_str(),
            pending.identity.protocol_device_id.as_str(),
            pending.identity.device_e2ee_key_id
        );
        let mut publish_attempts = Vec::new();
        let mut deleted = 0;

        publish_attempts.push(p5_state_id.clone());
        let warnings = finish_registration(
            registration_messaging_material_warnings(
                Err(crate::ImError::TransportUnavailable {
                    detail: "message service unavailable".to_owned(),
                }),
                None,
            ),
            || {
                deleted += 1;
                Ok(())
            },
        );
        assert_eq!(
            warnings,
            vec![REGISTRATION_PREKEY_PUBLISH_PENDING_WARNING.to_owned()]
        );
        assert_eq!(deleted, 1);
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
        ensure_remote_registration(&core, &mut transport, &mut pending, &request, |_| Ok(()))
            .unwrap();
        assert_eq!(transport.rpc_calls, 0);
        assert_eq!(transport.probe_calls, 0);

        assert_eq!(publish_attempts, vec![p5_state_id]);
    }

    #[test]
    fn registration_commit_projects_anp_custody_without_awiki_private_key_files() {
        let (_root, core, mut pending) = pending_with_core();
        crate::internal::identity_custody::begin_registration_publication(&core, &pending.identity)
            .unwrap();
        crate::internal::identity_custody::commit_registration_publication(
            &core,
            &pending.identity,
        )
        .unwrap();
        pending.remote_attempted = true;
        let token = access_token(&pending, &pending.identity.device_signing_key_id);
        apply_registration_reconciliation(
            &mut pending,
            crate::internal::transport::PendingRegistrationReconciliation::Committed {
                user_id: "user-1".to_owned(),
                binding_generation: "1".to_owned(),
                access_token: token,
            },
        )
        .unwrap();

        let result = commit_pending_registration(
            &core,
            &pending,
            crate::identity::RegistrationMethod::AlreadyVerified,
        )
        .unwrap();
        assert!(
            result
                .sdk_result
                .identity
                .as_ref()
                .unwrap()
                .readiness
                .ready_for_auth
        );

        let index = crate::internal::identity_store::IdentityStore::new(
            &core.inner().sdk_paths().identities,
        )
        .load_index()
        .unwrap();
        let entry = index.credentials.get("alice").unwrap();
        assert_eq!(
            entry.identity_custody_backend.as_deref(),
            Some("anp_identity")
        );
        assert_eq!(
            entry.anp_identity_id.as_deref(),
            Some(pending.identity.controller_identity_id.as_str())
        );
        let identity_dir = core
            .inner()
            .sdk_paths()
            .identities
            .identity_root_dir
            .join(&entry.dir_name);
        for secret_file in [
            "private_key.pem",
            "key-1-private.pem",
            "key-2-private.pem",
            "key-3-private.pem",
            "e2ee-signing-private.pem",
            "e2ee-agreement-private.pem",
            "device-signing-private.pem",
            "device-e2ee-private.pem",
            "daemon-key-1-private.pem",
        ] {
            assert!(!identity_dir.join(secret_file).exists(), "{secret_file}");
        }

        let runtime = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .unwrap();
        runtime
            .key_provider
            .sign(&pending.identity.device_signing_key_id, b"registration")
            .unwrap();
    }

    #[test]
    fn committed_device_refreshes_expired_bearer_before_prekey_publish() {
        assert!(registration_prekey_access_requires_refresh(true, false));
        assert!(!registration_prekey_access_requires_refresh(true, true));
        assert!(!registration_prekey_access_requires_refresh(false, false));
    }

    #[test]
    fn committed_registration_reports_cleanup_failure_without_hiding_success() {
        let warnings = finish_registration(Vec::new(), || {
            Err(crate::ImError::LocalStateUnavailable {
                detail: "vault cleanup unavailable".to_owned(),
            })
        });

        assert_eq!(
            warnings,
            vec![REGISTRATION_PENDING_CLEANUP_REQUIRED_WARNING.to_owned()]
        );
    }

    #[test]
    fn committed_registration_can_report_both_recoverable_warnings() {
        let warnings = finish_registration(
            registration_messaging_material_warnings(
                Err(crate::ImError::TransportUnavailable {
                    detail: "message service unavailable".to_owned(),
                }),
                None,
            ),
            || {
                Err(crate::ImError::LocalStateUnavailable {
                    detail: "vault cleanup unavailable".to_owned(),
                })
            },
        );

        assert_eq!(
            warnings,
            vec![
                REGISTRATION_PREKEY_PUBLISH_PENDING_WARNING.to_owned(),
                REGISTRATION_PENDING_CLEANUP_REQUIRED_WARNING.to_owned(),
            ]
        );
    }

    #[test]
    fn committed_registration_reports_group_key_package_failure_separately() {
        let warnings = registration_messaging_material_warnings(
            Ok(()),
            Some(Err(crate::ImError::TransportUnavailable {
                detail: "message service unavailable".to_owned(),
            })),
        );

        assert_eq!(
            warnings,
            vec![REGISTRATION_GROUP_KEY_PACKAGE_PUBLISH_PENDING_WARNING.to_owned()]
        );
    }

    #[cfg(feature = "group-e2ee")]
    #[test]
    fn registration_group_key_package_family_ids_are_deterministic_and_device_scoped() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let first = deterministic_registration_group_key_package_ids(&did, "device-a");
        let repeated = deterministic_registration_group_key_package_ids(&did, "device-a");
        let sibling = deterministic_registration_group_key_package_ids(&did, "device-b");

        assert_eq!(first, repeated);
        assert_ne!(first, sibling);
        assert!(first.0.starts_with("registration-p6-publish-"));
        assert!(first.1.starts_with("registration-kp-"));
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
        pending_with_core().2
    }

    fn pending_with_core() -> (
        tempfile::TempDir,
        crate::ImCore,
        crate::internal::identity_registration_pending::PendingRegistration,
    ) {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let identity = crate::internal::identity_custody::provision_registration_identity(
            &core,
            "example.test",
            "alice",
        )
        .unwrap();
        let pending = crate::internal::identity_registration_pending::PendingRegistration::new(
            "alice".to_owned(),
            "example.test".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            "already_verified".to_owned(),
            None,
            None,
            identity,
        )
        .unwrap();
        (root, core, pending)
    }

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "example.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        }
    }

    fn test_paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    fn access_token(
        pending: &crate::internal::identity_registration_pending::PendingRegistration,
        key_id: &str,
    ) -> String {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let claims = serde_json::json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": pending.identity.did.as_str(),
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": pending.identity.did.as_str(),
            "user_id": "user-1",
            "device_id": pending.identity.protocol_device_id.as_str(),
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

    fn expired_proof_error() -> crate::ImError {
        crate::ImError::Service {
            status_code: Some(200),
            code: Some("-32004".to_owned()),
            message: "DID document root proof has expired".to_owned(),
            data: Some(serde_json::json!({
                "awiki_code": REGISTRATION_PROOF_EXPIRED_AWIKI_CODE,
                "retryable": true
            })),
        }
    }
}
