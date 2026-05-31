use serde_json::Value;

use crate::internal::transport::{AsyncRpcTransport, RpcTransport};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRecoveryRuntimeResult {
    pub(crate) sdk_result: crate::identity::RecoverHandleResult,
    pub(crate) raw: Value,
}

pub(crate) struct PreparedRecoverHandleRequest {
    pub(crate) request: crate::identity::RecoverHandleRequest,
    pub(crate) local_store: Option<GeneratedRecoveryLocalStore>,
}

pub(crate) struct GeneratedRecoveryLocalStore {
    generated: crate::internal::identity_generation::GeneratedIdentity,
    local_alias: String,
}

pub(crate) struct IdentityRecoveryRuntime<T> {
    core: Option<crate::core::ImCore>,
    transport: T,
}

impl<T> IdentityRecoveryRuntime<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self {
            core: None,
            transport,
        }
    }

    pub(crate) fn new_with_core(core: &crate::core::ImCore, transport: T) -> Self {
        Self {
            core: Some(core.clone()),
            transport,
        }
    }
}

impl<T> IdentityRecoveryRuntime<T>
where
    T: RpcTransport,
{
    pub(crate) fn recover_handle(
        mut self,
        request: crate::identity::RecoverHandleRequest,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        validate_request(&request)?;
        let phone = crate::internal::identity_wire::normalize_phone(&request.phone)?;
        if let Some(otp) = request
            .otp
            .as_deref()
            .map(str::trim)
            .filter(|otp| !otp.is_empty())
            .map(str::to_string)
        {
            return self.recover_with_otp(request, phone, otp);
        }
        let call = crate::internal::identity_wire::directory::build_send_otp_rpc_call(&phone)?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let sdk_result = crate::identity::RecoverHandleResult::with_raw_response(
            request.handle,
            phone,
            crate::identity::RecoverHandleState::OtpSent,
            None,
            None,
            Some(raw.clone()),
            Vec::new(),
        );
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }

    fn recover_with_otp(
        &mut self,
        request: crate::identity::RecoverHandleRequest,
        phone: String,
        otp: String,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        if request.local_finalize.is_some() {
            return self.recover_with_local_finalize(request, phone, otp);
        }
        let generated = request.generated_identity.as_ref().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("generated_identity".to_string()),
                "generated identity is required when otp is provided",
            )
        })?;
        let call = crate::internal::identity_wire::recovery::build_recover_handle_rpc_call(
            crate::internal::identity_wire::RecoverHandleRpcParams {
                did_document: generated.did_document.clone(),
                handle: request.handle.as_str().to_string(),
                phone: phone.clone(),
                otp_code: otp,
            },
        )?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let identity = recovered_identity_summary(&request, generated, &raw)?;
        let recovered_identity = crate::identity::RecoveredIdentity {
            identity,
            user_id: raw
                .get("user_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            access_token_present: raw
                .get("access_token")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
        };
        let sdk_result = crate::identity::RecoverHandleResult::with_raw_response(
            request.handle,
            phone,
            crate::identity::RecoverHandleState::Recovered,
            Some(recovered_identity),
            None,
            Some(raw.clone()),
            Vec::new(),
        );
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }

    fn recover_with_local_finalize(
        &mut self,
        request: crate::identity::RecoverHandleRequest,
        phone: String,
        otp: String,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        let core = self.core.clone().ok_or_else(|| crate::ImError::Internal {
            message: "recover local finalize requires an ImCore runtime".to_string(),
        })?;
        let prepared = prepare_recover_with_local_finalize(&core, &request, &phone, &otp)?;
        let raw = self.transport.rpc(
            prepared.call.endpoint,
            prepared.call.method,
            prepared.call.params.clone(),
        )?;
        finish_recover_with_local_finalize(&core, phone, raw, prepared)
    }
}

impl<T> IdentityRecoveryRuntime<T>
where
    T: AsyncRpcTransport,
{
    pub(crate) async fn recover_handle_async(
        mut self,
        request: crate::identity::RecoverHandleRequest,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        validate_request(&request)?;
        let phone = crate::internal::identity_wire::normalize_phone(&request.phone)?;
        if let Some(otp) = request
            .otp
            .as_deref()
            .map(str::trim)
            .filter(|otp| !otp.is_empty())
            .map(str::to_string)
        {
            return self.recover_with_otp_async(request, phone, otp).await;
        }
        let call = crate::internal::identity_wire::directory::build_send_otp_rpc_call(&phone)?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())
            .await?;
        let sdk_result = crate::identity::RecoverHandleResult::with_raw_response(
            request.handle,
            phone,
            crate::identity::RecoverHandleState::OtpSent,
            None,
            None,
            Some(raw.clone()),
            Vec::new(),
        );
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }

    async fn recover_with_otp_async(
        &mut self,
        request: crate::identity::RecoverHandleRequest,
        phone: String,
        otp: String,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        if request.local_finalize.is_some() {
            return self
                .recover_with_local_finalize_async(request, phone, otp)
                .await;
        }
        let generated = request.generated_identity.as_ref().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("generated_identity".to_string()),
                "generated identity is required when otp is provided",
            )
        })?;
        let call = crate::internal::identity_wire::recovery::build_recover_handle_rpc_call(
            crate::internal::identity_wire::RecoverHandleRpcParams {
                did_document: generated.did_document.clone(),
                handle: request.handle.as_str().to_string(),
                phone: phone.clone(),
                otp_code: otp,
            },
        )?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())
            .await?;
        let identity = recovered_identity_summary(&request, generated, &raw)?;
        let recovered_identity = crate::identity::RecoveredIdentity {
            identity,
            user_id: raw
                .get("user_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            access_token_present: raw
                .get("access_token")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
        };
        let sdk_result = crate::identity::RecoverHandleResult::with_raw_response(
            request.handle,
            phone,
            crate::identity::RecoverHandleState::Recovered,
            Some(recovered_identity),
            None,
            Some(raw.clone()),
            Vec::new(),
        );
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }

    async fn recover_with_local_finalize_async(
        &mut self,
        request: crate::identity::RecoverHandleRequest,
        phone: String,
        otp: String,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        let core = self.core.clone().ok_or_else(|| crate::ImError::Internal {
            message: "recover local finalize requires an ImCore runtime".to_string(),
        })?;
        let core_for_prepare = core.clone();
        let request_for_prepare = request.clone();
        let phone_for_prepare = phone.clone();
        let otp_for_prepare = otp.clone();
        let prepared = crate::internal::runtime::worker::run_blocking(move || {
            prepare_recover_with_local_finalize(
                &core_for_prepare,
                &request_for_prepare,
                &phone_for_prepare,
                &otp_for_prepare,
            )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;
        let raw = self
            .transport
            .rpc(
                prepared.call.endpoint,
                prepared.call.method,
                prepared.call.params.clone(),
            )
            .await?;
        finish_recover_with_local_finalize_async(core, phone, raw, prepared).await
    }
}

#[derive(Debug, Clone)]
struct PreparedLocalFinalize {
    plan: crate::internal::identity_recovery_local::RecoverLocalPlan,
    local_finalize: crate::identity::RecoverHandleLocalFinalizeRequest,
    generated: crate::internal::identity_generation::GeneratedIdentity,
    active_before: String,
    backup: crate::internal::identity_recovery_local::RecoverBackupResult,
    call: crate::internal::identity_wire::RpcCall,
}

#[derive(Debug, Clone)]
struct PreparedLocalFinalizeSaved {
    plan: crate::internal::identity_recovery_local::RecoverLocalPlan,
    local_finalize: crate::identity::RecoverHandleLocalFinalizeRequest,
    active_before: String,
    backup: crate::internal::identity_recovery_local::RecoverBackupResult,
    stored: crate::internal::identity_store::StoredIdentity,
    recovered_identity: crate::identity::RecoveredIdentity,
}

fn prepare_recover_with_local_finalize(
    core: &crate::core::ImCore,
    request: &crate::identity::RecoverHandleRequest,
    phone: &str,
    otp: &str,
) -> crate::ImResult<PreparedLocalFinalize> {
    let plan = crate::internal::identity_recovery_local::plan_recover_handle(
        &core.inner().sdk_paths().identities,
        &request.handle,
        request
            .local_finalize
            .as_ref()
            .and_then(|local| local.raw_handle.as_deref())
            .or(request.raw_handle.as_deref()),
        &core.inner().sdk_config().did_domain,
    )?;
    let local_finalize = request.local_finalize.clone().unwrap_or_default();
    let generated = crate::internal::identity_generation::generate_identity_with_path_segments(
        &plan.target.effective_domain,
        [plan.target.target_local_part.as_str()],
        core.inner().sdk_config().anp_service_endpoint.as_ref(),
        core.inner().sdk_config().anp_service_did.as_ref(),
    )?;
    let call = crate::internal::identity_wire::recovery::build_recover_handle_rpc_call(
        crate::internal::identity_wire::RecoverHandleRpcParams {
            did_document: generated.did_document.clone(),
            handle: plan.target.target_handle.as_str().to_string(),
            phone: phone.to_string(),
            otp_code: otp.to_string(),
        },
    )?;
    let active_before = local_finalize
        .active_identity_name
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    let backup = crate::internal::identity_recovery_local::create_recover_backup(
        &core.inner().sdk_paths().identities,
        &plan,
        &active_before,
        local_finalize.config_file_path.as_deref(),
    )?;
    Ok(PreparedLocalFinalize {
        plan,
        local_finalize,
        generated,
        active_before,
        backup,
        call,
    })
}

async fn finish_recover_with_local_finalize_async(
    core: crate::core::ImCore,
    phone: String,
    raw: Value,
    prepared: PreparedLocalFinalize,
) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
    let core_for_save = core.clone();
    let raw_for_save = raw.clone();
    let saved = crate::internal::runtime::worker::run_blocking(move || {
        save_recover_with_local_finalize_identity(&core_for_save, &raw_for_save, prepared)
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: err.to_string(),
    })??;
    let old_dids = saved.plan.old_owner_dids_in_merge_order();
    let new_did = saved.stored.did.as_str().to_string();
    let final_owner_identity_id = saved.stored.unique_id.clone();
    let final_identity_name = saved.plan.final_identity_name.clone();
    let (store_merge_counts, e2ee_cleanup_counts) = core
        .inner()
        .local_state_db()
        .await?
        .merge_recovered_handle_local_state(
            old_dids,
            new_did,
            final_owner_identity_id,
            final_identity_name,
        )
        .await?;
    crate::internal::runtime::worker::run_blocking(move || {
        finish_saved_recover_with_local_finalize(
            &core,
            phone,
            raw,
            saved,
            store_merge_counts,
            e2ee_cleanup_counts,
        )
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: err.to_string(),
    })?
}

fn finish_recover_with_local_finalize(
    core: &crate::core::ImCore,
    phone: String,
    raw: Value,
    prepared: PreparedLocalFinalize,
) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
    let saved = save_recover_with_local_finalize_identity(core, &raw, prepared)?;
    let old_dids = saved.plan.old_owner_dids_in_merge_order();
    let new_did = saved.stored.did.as_str().to_string();
    let (store_merge_counts, e2ee_cleanup_counts) =
        crate::internal::identity_recover_local_state::merge_recovered_handle_local_state(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &old_dids,
            &new_did,
            &saved.stored.unique_id,
            &saved.plan.final_identity_name,
        )?;
    finish_saved_recover_with_local_finalize(
        core,
        phone,
        raw,
        saved,
        store_merge_counts,
        e2ee_cleanup_counts,
    )
}

fn save_recover_with_local_finalize_identity(
    core: &crate::core::ImCore,
    raw: &Value,
    prepared: PreparedLocalFinalize,
) -> crate::ImResult<PreparedLocalFinalizeSaved> {
    let plan = prepared.plan;
    let generated = prepared.generated;
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let stored = store.save_identity(crate::internal::identity_store::SaveIdentityInput {
        local_alias: plan.temp_identity_name.clone(),
        did: did_from_raw(raw).unwrap_or_else(|| generated.did.clone()),
        unique_id: generated.unique_id.clone(),
        user_id: crate::internal::identity_recovery_local::string_value(raw, "user_id", ""),
        display_name: plan.target.target_local_part.clone(),
        handle: crate::internal::identity_recovery_local::string_value(
            raw,
            "handle",
            &plan.target.target_local_part,
        ),
        full_handle: crate::internal::identity_recovery_local::string_value(
            raw,
            "full_handle",
            plan.target.target_handle.as_str(),
        ),
        jwt_token: crate::internal::identity_recovery_local::string_value(raw, "access_token", ""),
        did_document: Some(generated.did_document.clone()),
        key1_private_pem: generated.key1_private_pem.clone(),
        key1_public_pem: generated.key1_public_pem.clone(),
        e2ee_signing_private_pem: generated.e2ee_signing_private_pem.clone(),
        e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem.clone(),
        make_default: false,
    })?;
    let identity = crate::internal::identity_recovery_local::identity_summary_from_generated(
        &generated,
        raw,
        &plan.target,
        &plan.temp_identity_name,
    )?;
    let recovered_identity = crate::identity::RecoveredIdentity {
        identity: identity.clone(),
        user_id: raw
            .get("user_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        access_token_present: raw
            .get("access_token")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty()),
    };
    Ok(PreparedLocalFinalizeSaved {
        plan,
        local_finalize: prepared.local_finalize,
        active_before: prepared.active_before,
        backup: prepared.backup,
        stored,
        recovered_identity,
    })
}

fn finish_saved_recover_with_local_finalize(
    core: &crate::core::ImCore,
    phone: String,
    raw: Value,
    saved: PreparedLocalFinalizeSaved,
    store_merge_counts: std::collections::BTreeMap<String, i64>,
    e2ee_cleanup_counts: std::collections::BTreeMap<String, i64>,
) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let archived_identity_names = saved.plan.archived_identity_names();
    let promoted = store.promote_recovered_handle(
        &saved.plan.final_identity_name,
        &saved.plan.temp_identity_name,
        &archived_identity_names,
    )?;
    let active_was_archived = archived_identity_names
        .iter()
        .any(|name| name.trim() == saved.active_before && !name.trim().is_empty());
    let active_config_updated = if active_was_archived {
        if let Some(config_file) = saved.local_finalize.config_file_path.as_deref() {
            crate::internal::identity_recovery_local::update_active_identity_in_config(
                config_file,
                &saved.plan.final_identity_name,
            )?
        } else {
            false
        }
    } else {
        false
    };
    let mut summary_stored = saved.stored.clone();
    summary_stored.local_alias = saved.plan.final_identity_name.clone();
    summary_stored.is_default = promoted.default_updated;
    let final_summary =
        crate::internal::identity_recovery_local::local_identity_summary_from_stored(
            &summary_stored,
        );
    let local_recovery = crate::identity::RecoverHandleLocalResult {
        identity: final_summary,
        backup_path: saved.backup.backup_path,
        archived_identities: archived_identity_names,
        archived_dids: saved.plan.archived_dids(),
        full_handle: saved.plan.target.target_handle.as_str().to_string(),
        final_identity_name: saved.plan.final_identity_name,
        store_merge_counts,
        e2ee_cleanup_counts,
        default_updated: promoted.default_updated,
        active_config_updated,
    };
    let mut warnings = Vec::new();
    if !local_recovery.archived_identities.is_empty() {
        warnings.push(format!(
            "Archived {} same-handle local identities; they were removed from the live index, while their original directories and the recover backup were kept.",
            local_recovery.archived_identities.len()
        ));
    }
    let sdk_result = crate::identity::RecoverHandleResult::with_raw_response(
        saved.plan.target.target_handle,
        phone,
        crate::identity::RecoverHandleState::Recovered,
        Some(saved.recovered_identity),
        Some(local_recovery),
        Some(raw.clone()),
        warnings,
    );
    Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
}

pub(crate) fn prepare_recover_handle_request(
    core: &crate::core::ImCore,
    mut request: crate::identity::RecoverHandleRequest,
) -> crate::ImResult<PreparedRecoverHandleRequest> {
    let otp_present = request
        .otp
        .as_deref()
        .is_some_and(|otp| !otp.trim().is_empty());
    let mut local_store = None;
    if otp_present && request.local_finalize.is_none() && request.generated_identity.is_none() {
        let target = recovery_target(&request.handle, &core.inner().sdk_config().did_domain)?;
        let generated = crate::internal::identity_generation::generate_identity_with_path_segments(
            &target.effective_domain,
            [target.local_part.as_str()],
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
        request.generated_identity = Some(crate::identity::RecoverGeneratedIdentity {
            did: generated.did.clone(),
            unique_id: generated.unique_id.clone(),
            did_document: generated.did_document.clone(),
        });
        request.handle = target.full_handle;
        local_store = Some(GeneratedRecoveryLocalStore {
            generated,
            local_alias: if target.explicit_domain {
                request.handle.as_str().to_string()
            } else {
                target.local_part
            },
        });
    }
    validate_request(&request)?;
    Ok(PreparedRecoverHandleRequest {
        request,
        local_store,
    })
}

pub(crate) fn finalize_recover_handle_result(
    core: &crate::core::ImCore,
    local_store: Option<GeneratedRecoveryLocalStore>,
    result: IdentityRecoveryRuntimeResult,
) -> crate::ImResult<crate::identity::RecoverHandleResult> {
    let Some(local_store) = local_store else {
        return Ok(result.sdk_result);
    };
    if !matches!(
        result.sdk_result.state,
        crate::identity::RecoverHandleState::Recovered
    ) {
        return Ok(result.sdk_result);
    }
    let target = recovery_target(
        &result.sdk_result.handle,
        &core.inner().sdk_config().did_domain,
    )?;
    let generated = local_store.generated;
    let did = did_from_raw(&result.raw).unwrap_or_else(|| generated.did.clone());
    let stored =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_identity(crate::internal::identity_store::SaveIdentityInput {
            local_alias: local_store.local_alias,
            did,
            unique_id: generated.unique_id,
            user_id: string_value(&result.raw, "user_id", ""),
            display_name: string_value(&result.raw, "handle", &target.local_part),
            handle: string_value(&result.raw, "handle", &target.local_part),
            full_handle: string_value(&result.raw, "full_handle", target.full_handle.as_str()),
            jwt_token: string_value(&result.raw, "access_token", ""),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            make_default: true,
        })?;
    let identity =
        crate::internal::identity_registration_runtime::identity_summary_from_stored(&stored)?;
    let recovered_identity = crate::identity::RecoveredIdentity {
        identity,
        user_id: result
            .raw
            .get("user_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        access_token_present: result
            .raw
            .get("access_token")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty()),
    };
    Ok(crate::identity::RecoverHandleResult::with_raw_response(
        result.sdk_result.handle,
        result.sdk_result.phone,
        result.sdk_result.state,
        Some(recovered_identity),
        None,
        Some(result.raw),
        result.sdk_result.warnings,
    ))
}

pub(crate) fn validate_request(
    request: &crate::identity::RecoverHandleRequest,
) -> crate::ImResult<()> {
    crate::internal::identity_wire::required_trimmed(request.handle.as_str(), "handle")?;
    crate::internal::identity_wire::normalize_phone(&request.phone)?;
    if request
        .otp
        .as_deref()
        .is_some_and(|otp| !otp.trim().is_empty())
        && request.local_finalize.is_none()
    {
        let generated = request.generated_identity.as_ref().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("generated_identity".to_string()),
                "generated identity is required when otp is provided",
            )
        })?;
        if generated.unique_id.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("generated_identity.unique_id".to_string()),
                "generated identity unique_id must not be empty",
            ));
        }
        if !generated.did_document.is_object() {
            return Err(crate::ImError::invalid_input(
                Some("generated_identity.did_document".to_string()),
                "generated identity did_document must be an object",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryTarget {
    local_part: String,
    full_handle: crate::ids::Handle,
    effective_domain: String,
    explicit_domain: bool,
}

fn recovery_target(
    handle: &crate::ids::Handle,
    default_domain: &str,
) -> crate::ImResult<RecoveryTarget> {
    let raw = handle
        .as_str()
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    if raw.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle must not be empty",
        ));
    }
    if raw.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "DID values are not supported in handle recovery",
        ));
    }
    if let Some(dot) = raw.find('.') {
        let local_part = raw[..dot].trim().to_string();
        let domain = raw[dot + 1..].trim().trim_end_matches('.').to_string();
        if local_part.is_empty() || domain.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("handle".to_string()),
                "invalid handle",
            ));
        }
        return Ok(RecoveryTarget {
            full_handle: crate::ids::Handle::parse(format!("{local_part}.{domain}"), "")?,
            local_part,
            effective_domain: domain,
            explicit_domain: true,
        });
    }

    let domain = default_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if domain.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("did_domain".to_string()),
            "did_domain is required to complete bare handle recovery",
        ));
    }
    Ok(RecoveryTarget {
        full_handle: crate::ids::Handle::parse(format!("{raw}.{domain}"), "")?,
        local_part: raw,
        effective_domain: domain,
        explicit_domain: false,
    })
}

fn recovered_identity_summary(
    request: &crate::identity::RecoverHandleRequest,
    generated: &crate::identity::RecoverGeneratedIdentity,
    raw: &Value,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let did = raw
        .get("did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| generated.did.as_str());
    let handle = raw
        .get("full_handle")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| request.handle.as_str());
    let local_alias = local_part(handle).to_string();
    Ok(crate::identity::IdentitySummary {
        id: crate::ids::IdentityId::parse(&generated.unique_id)?,
        did: crate::ids::Did::parse(did)?,
        handle: Some(crate::ids::Handle::parse(handle, "")?),
        display_name: Some(
            raw.get("handle")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| local_part(handle))
                .to_string(),
        ),
        local_alias: Some(local_alias),
        device_id: None,
        is_default: false,
        readiness: crate::identity::IdentityReadiness {
            ready_for_auth: true,
            ready_for_messaging: true,
            missing: Vec::new(),
        },
    })
}

fn local_part(handle: &str) -> &str {
    handle
        .trim_start_matches('@')
        .split('.')
        .next()
        .unwrap_or(handle)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_recovery_without_otp_sends_recover_otp() {
        let result = IdentityRecoveryRuntime::new(TestTransport {
            responses: vec![serde_json::json!({"sent": true})],
            calls: Vec::new(),
        })
        .recover_handle(crate::identity::RecoverHandleRequest {
            handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            raw_handle: None,
            phone: "13800138000".to_string(),
            otp: None,
            generated_identity: None,
            local_finalize: None,
        })
        .unwrap();

        assert_eq!(
            result.sdk_result.state,
            crate::identity::RecoverHandleState::OtpSent
        );
        assert_eq!(result.sdk_result.phone, "+8613800138000");
        assert_eq!(result.raw["sent"], true);
    }

    #[test]
    fn identity_recovery_with_otp_calls_recover_handle_and_maps_summary() {
        let generated = crate::identity::RecoverGeneratedIdentity {
            did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_generated").unwrap(),
            unique_id: "e1_generated".to_string(),
            did_document: serde_json::json!({
                "id": "did:wba:awiki.test:alice:e1_generated"
            }),
        };
        let result = IdentityRecoveryRuntime::new(TestTransport {
            responses: vec![serde_json::json!({
                "did": "did:wba:awiki.test:alice:e1_recovered",
                "user_id": "user-alice",
                "handle": "alice",
                "full_handle": "alice.awiki.test",
                "access_token": "jwt-recover"
            })],
            calls: Vec::new(),
        })
        .recover_handle(crate::identity::RecoverHandleRequest {
            handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: Some(" 12 34 56 ".to_string()),
            generated_identity: Some(generated),
            local_finalize: None,
        })
        .unwrap();

        let recovered = result.sdk_result.recovered_identity.unwrap();
        assert_eq!(
            result.sdk_result.state,
            crate::identity::RecoverHandleState::Recovered
        );
        assert_eq!(
            recovered.identity.did.as_str(),
            "did:wba:awiki.test:alice:e1_recovered"
        );
        assert_eq!(
            recovered.identity.handle.unwrap().as_str(),
            "alice.awiki.test"
        );
        assert_eq!(recovered.user_id.as_deref(), Some("user-alice"));
        assert!(recovered.access_token_present);
    }

    #[test]
    fn prepare_recover_handle_request_generates_identity_when_otp_is_present() {
        let fixture = CoreFixture::new();
        let prepared = prepare_recover_handle_request(
            &fixture.core,
            crate::identity::RecoverHandleRequest {
                handle: crate::ids::Handle::parse("alice", "").unwrap(),
                raw_handle: None,
                phone: "+15551234567".to_string(),
                otp: Some("123456".to_string()),
                generated_identity: None,
                local_finalize: None,
            },
        )
        .unwrap();

        let generated = prepared
            .request
            .generated_identity
            .as_ref()
            .expect("generated identity");
        assert!(generated
            .did
            .as_str()
            .starts_with("did:wba:awiki.test:alice:"));
        assert_eq!(prepared.request.handle.as_str(), "alice.awiki.test");
        assert_eq!(
            prepared
                .local_store
                .as_ref()
                .unwrap()
                .generated
                .did
                .as_str(),
            generated.did.as_str()
        );
    }

    #[test]
    fn finalize_generated_recovery_persists_identity_for_flutter_clients() {
        let fixture = CoreFixture::new();
        let prepared = prepare_recover_handle_request(
            &fixture.core,
            crate::identity::RecoverHandleRequest {
                handle: crate::ids::Handle::parse("alice", "").unwrap(),
                raw_handle: None,
                phone: "+15551234567".to_string(),
                otp: Some("123456".to_string()),
                generated_identity: None,
                local_finalize: None,
            },
        )
        .unwrap();
        let generated = prepared.request.generated_identity.clone().unwrap();
        let raw = serde_json::json!({
            "did": generated.did.as_str(),
            "user_id": "user-alice",
            "handle": "alice",
            "full_handle": "alice.awiki.test",
            "access_token": "jwt-recover"
        });
        let runtime_result = IdentityRecoveryRuntimeResult {
            sdk_result: crate::identity::RecoverHandleResult::with_raw_response(
                prepared.request.handle,
                "+15551234567".to_string(),
                crate::identity::RecoverHandleState::Recovered,
                Some(crate::identity::RecoveredIdentity {
                    identity: crate::identity::IdentitySummary {
                        id: crate::ids::IdentityId::parse(&generated.unique_id).unwrap(),
                        did: generated.did.clone(),
                        handle: Some(crate::ids::Handle::parse("alice.awiki.test", "").unwrap()),
                        display_name: Some("alice".to_string()),
                        local_alias: Some("alice".to_string()),
                        device_id: None,
                        is_default: false,
                        readiness: crate::identity::IdentityReadiness {
                            ready_for_auth: true,
                            ready_for_messaging: true,
                            missing: Vec::new(),
                        },
                    },
                    user_id: Some("user-alice".to_string()),
                    access_token_present: true,
                }),
                None,
                Some(raw.clone()),
                Vec::new(),
            ),
            raw,
        };

        let result =
            finalize_recover_handle_result(&fixture.core, prepared.local_store, runtime_result)
                .unwrap();

        let recovered = result.recovered_identity.unwrap();
        assert_eq!(recovered.identity.local_alias.as_deref(), Some("alice"));
        assert!(recovered.identity.is_default);
        assert!(recovered.identity.readiness.ready_for_auth);
        assert!(recovered.identity.readiness.ready_for_messaging);
        let default = fixture
            .core
            .identities()
            .default_identity()
            .unwrap()
            .unwrap();
        assert_eq!(default.handle.unwrap().as_str(), "alice.awiki.test");
        assert!(std::fs::read_to_string(
            fixture
                .root
                .path()
                .join("identities")
                .join(&generated.unique_id)
                .join("auth.json")
        )
        .unwrap()
        .contains("jwt-recover"));
    }

    struct TestTransport {
        responses: Vec<serde_json::Value>,
        calls: Vec<(String, String, serde_json::Value)>,
    }

    impl crate::internal::transport::RpcTransport for TestTransport {
        fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: serde_json::Value,
        ) -> crate::ImResult<serde_json::Value> {
            self.calls
                .push((endpoint.to_string(), method.to_string(), params));
            Ok(self.responses.remove(0))
        }
    }

    struct CoreFixture {
        root: tempfile::TempDir,
        core: crate::core::ImCore,
    }

    impl CoreFixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let mut config = crate::ImCoreConfig::new(
                crate::ServiceEndpoint::parse("https://awiki.test").unwrap(),
                "awiki.test",
            )
            .unwrap();
            config.anp_service_endpoint =
                Some(crate::ServiceEndpoint::parse("https://awiki.test/anp-im/rpc").unwrap());
            config.anp_service_did = Some(crate::ids::Did::parse("did:wba:awiki.test").unwrap());
            let paths = crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("identities").join("index.json"),
                    default_identity_path: Some(root.path().join("identities").join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            };
            let core = crate::core::ImCore::new(config, paths).unwrap();
            Self { root, core }
        }
    }
}
