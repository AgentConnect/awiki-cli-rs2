use serde_json::Value;

use crate::internal::transport::{AsyncRpcTransport, RpcTransport};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRecoveryRuntimeResult {
    pub(crate) sdk_result: crate::identity::RecoverHandleResult,
    pub(crate) raw: Value,
}

pub(crate) struct PreparedRecoverHandleRequest {
    pub(crate) request: crate::identity::RecoverHandleRequest,
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
    daemon_subkey_package: crate::identity::DaemonSubkeyPrivatePackage,
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
    let generated_with_daemon =
        crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
            &plan.target.effective_domain,
            &plan.target.target_local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
    let crate::internal::identity_generation::GeneratedIdentityWithDaemonSubkey {
        identity: generated,
        daemon_subkey_package,
    } = generated_with_daemon;
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
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?,
    )?;
    Ok(PreparedLocalFinalize {
        plan,
        local_finalize,
        generated,
        daemon_subkey_package,
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
    let sqlite_path = core.inner().sdk_paths().local_state.sqlite_path.clone();
    let handle = saved.plan.target.target_handle.as_str().to_owned();
    let generation = raw
        .get("binding_generation")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let enqueue_warning = if let Some(generation) = generation {
        let old_dids_for_jobs = saved.plan.old_owner_dids_in_merge_order();
        let new_did_for_jobs = saved.stored.did.as_str().to_owned();
        let owner_for_jobs = saved.stored.unique_id.clone();
        match crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::group_rebind_recovery::enqueue_recovery_jobs(
                &sqlite_path,
                &owner_for_jobs,
                &handle,
                &old_dids_for_jobs,
                &new_did_for_jobs,
                &generation,
            )
        })
        .await
        {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(format!("group rebind outbox enqueue failed: {error}")),
            Err(error) => Some(format!("group rebind outbox worker failed: {error}")),
        }
    } else {
        Some(
            "recovery response omitted binding_generation; group rebind jobs were not created"
                .to_owned(),
        )
    };
    crate::internal::runtime::worker::run_blocking(move || {
        finish_saved_recover_with_local_finalize(
            &core,
            phone,
            raw,
            saved,
            store_merge_counts,
            e2ee_cleanup_counts,
            enqueue_warning.into_iter().collect(),
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
    let mut recovery_warnings = Vec::new();
    if let Some(generation) = raw.get("binding_generation").and_then(Value::as_str) {
        if let Err(error) = crate::internal::group_rebind_recovery::enqueue_recovery_jobs(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &saved.stored.unique_id,
            saved.plan.target.target_handle.as_str(),
            &saved.plan.old_owner_dids_in_merge_order(),
            saved.stored.did.as_str(),
            generation,
        ) {
            recovery_warnings.push(format!("group rebind outbox enqueue failed: {error}"));
        }
    } else {
        recovery_warnings.push(
            "recovery response omitted binding_generation; group rebind jobs were not created"
                .to_owned(),
        );
    }
    finish_saved_recover_with_local_finalize(
        core,
        phone,
        raw,
        saved,
        store_merge_counts,
        e2ee_cleanup_counts,
        recovery_warnings,
    )
}

fn save_recover_with_local_finalize_identity(
    core: &crate::core::ImCore,
    raw: &Value,
    prepared: PreparedLocalFinalize,
) -> crate::ImResult<PreparedLocalFinalizeSaved> {
    let plan = prepared.plan;
    let generated = prepared.generated;
    let daemon_subkey_package = prepared.daemon_subkey_package;
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let did = did_from_raw(raw).unwrap_or_else(|| generated.did.clone());
    if did != daemon_subkey_package.user_did {
        return Err(crate::ImError::IdentityNotReady {
            identity: did.as_str().to_string(),
            missing: vec!["daemon_subkey_did_mismatch".to_string()],
        });
    }
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let stable_owner_identity_id = plan
        .stable_owner_identity_id(&prepared.active_before)
        .unwrap_or_else(|| generated.unique_id.clone());
    let stored = store.save_recovered_identity_with_secret_storage(
        crate::internal::identity_store::SaveIdentityInput {
            local_alias: plan.temp_identity_name.clone(),
            did,
            unique_id: stable_owner_identity_id,
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
            jwt_token: crate::internal::identity_recovery_local::string_value(
                raw,
                "access_token",
                "",
            ),
            did_document: Some(generated.did_document.clone()),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
            device_state: None,
            key1_private_pem: generated.key1_private_pem.clone(),
            key1_public_pem: generated.key1_public_pem.clone(),
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem.clone(),
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem.clone(),
            daemon_subkey_package: Some(daemon_subkey_package),
            make_default: false,
        },
        secret_storage,
        &plan.archived_identity_names(),
    )?;
    let identity =
        crate::internal::identity_registration_runtime::identity_summary_from_stored(&stored)?;
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
    mut saved: PreparedLocalFinalizeSaved,
    store_merge_counts: std::collections::BTreeMap<String, i64>,
    e2ee_cleanup_counts: std::collections::BTreeMap<String, i64>,
    additional_warnings: Vec<String>,
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
    saved.recovered_identity.identity.local_alias = Some(final_summary.identity_name.clone());
    saved.recovered_identity.identity.is_default = final_summary.is_default;
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
    warnings.extend(additional_warnings);
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
    // A registry-backed recovery owns local persistence. When the caller asks
    // im-core to generate the replacement DID, local finalization is not an
    // optional host hint: it is the canonical path that preserves the stable
    // owner identity, records DID history, and creates group rebind work.
    // Explicitly supplied generated identities retain their lower-level flow
    // for compatibility.
    if otp_present && request.local_finalize.is_none() {
        if request.generated_identity.is_none() {
            request.local_finalize =
                Some(crate::identity::RecoverHandleLocalFinalizeRequest::default());
        } else {
            let plan = crate::internal::identity_recovery_local::plan_recover_handle(
                &core.inner().sdk_paths().identities,
                &request.handle,
                request.raw_handle.as_deref(),
                &core.inner().sdk_config().did_domain,
            )?;
            if !plan.same_handle_candidates.is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("local_finalize".to_owned()),
                    "same-handle recovery with local state requires local finalization",
                ));
            }
        }
    }
    validate_request(&request)?;
    Ok(PreparedRecoverHandleRequest { request })
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
    fn prepare_recover_handle_request_defaults_to_local_finalize_when_otp_is_present() {
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

        assert!(prepared.request.generated_identity.is_none());
        assert!(prepared.request.local_finalize.is_some());
        assert_eq!(prepared.request.handle.as_str(), "alice");
    }

    #[test]
    fn prepare_recover_handle_request_rejects_same_handle_remote_only_recovery() {
        let fixture = CoreFixture::new();
        let generated =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "awiki.test",
                "alice",
                fixture.core.inner().sdk_config().anp_service_endpoint.as_ref(),
                fixture.core.inner().sdk_config().anp_service_did.as_ref(),
            )
            .unwrap();
        let stored_identity = generated.identity.clone();
        crate::internal::identity_store::IdentityStore::new(
            &fixture.core.inner().sdk_paths().identities,
        )
        .save_identity(crate::internal::identity_store::SaveIdentityInput {
            local_alias: "alice".to_string(),
            did: stored_identity.did.clone(),
            unique_id: "stable-alice-id".to_string(),
            user_id: "user-alice".to_string(),
            display_name: "Alice".to_string(),
            handle: "alice".to_string(),
            full_handle: "alice.awiki.test".to_string(),
            jwt_token: "jwt-alice".to_string(),
            did_document: Some(stored_identity.did_document.clone()),
            key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
            device_state: None,
            key1_private_pem: stored_identity.key1_private_pem.clone(),
            key1_public_pem: stored_identity.key1_public_pem.clone(),
            e2ee_signing_private_pem: stored_identity.e2ee_signing_private_pem.clone(),
            e2ee_agreement_private_pem: stored_identity.e2ee_agreement_private_pem.clone(),
            daemon_subkey_package: Some(generated.daemon_subkey_package),
            make_default: true,
        })
        .unwrap();

        let error = match prepare_recover_handle_request(
            &fixture.core,
            crate::identity::RecoverHandleRequest {
                handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
                raw_handle: Some("alice.awiki.test".to_string()),
                phone: "+15551234567".to_string(),
                otp: Some("123456".to_string()),
                generated_identity: Some(crate::identity::RecoverGeneratedIdentity {
                    did: stored_identity.did,
                    unique_id: "external-generated-id".to_string(),
                    did_document: stored_identity.did_document,
                }),
                local_finalize: None,
            },
        ) {
            Ok(_) => panic!("same-handle remote-only recovery must fail closed"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("same-handle recovery with local state requires local finalization"));
    }

    #[test]
    fn local_finalize_recovery_generates_handle_service_declaration() {
        let fixture = CoreFixture::new();
        let request = crate::identity::RecoverHandleRequest {
            handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            raw_handle: None,
            phone: "+15551234567".to_string(),
            otp: Some("123456".to_string()),
            generated_identity: None,
            local_finalize: Some(crate::identity::RecoverHandleLocalFinalizeRequest::default()),
        };

        let prepared =
            prepare_recover_with_local_finalize(&fixture.core, &request, "+15551234567", "123456")
                .unwrap();
        let services = prepared.generated.did_document["service"]
            .as_array()
            .unwrap();
        assert!(services.iter().any(|service| {
            service["type"] == "ANPHandleService"
                && service["serviceEndpoint"] == "https://awiki.test/.well-known/handle/alice"
        }));
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
        _root: tempfile::TempDir,
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
            Self { _root: root, core }
        }
    }
}
