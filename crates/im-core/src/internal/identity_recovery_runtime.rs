use serde_json::Value;

use crate::internal::transport::RpcTransport;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRecoveryRuntimeResult {
    pub(crate) sdk_result: crate::identity::RecoverHandleResult,
    pub(crate) raw: Value,
}

pub(crate) struct IdentityRecoveryRuntime<T> {
    core: Option<crate::core::ImCore>,
    transport: T,
}

impl<T> IdentityRecoveryRuntime<T>
where
    T: RpcTransport,
{
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
                phone: phone.clone(),
                otp_code: otp,
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
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let store = crate::internal::identity_store::IdentityStore::new(
            &core.inner().sdk_paths().identities,
        );
        let stored = store.save_identity(crate::internal::identity_store::SaveIdentityInput {
            local_alias: plan.temp_identity_name.clone(),
            did: did_from_raw(&raw).unwrap_or_else(|| generated.did.clone()),
            unique_id: generated.unique_id.clone(),
            user_id: crate::internal::identity_recovery_local::string_value(&raw, "user_id", ""),
            display_name: plan.target.target_local_part.clone(),
            handle: crate::internal::identity_recovery_local::string_value(
                &raw,
                "handle",
                &plan.target.target_local_part,
            ),
            full_handle: crate::internal::identity_recovery_local::string_value(
                &raw,
                "full_handle",
                plan.target.target_handle.as_str(),
            ),
            jwt_token: crate::internal::identity_recovery_local::string_value(
                &raw,
                "access_token",
                "",
            ),
            did_document: Some(generated.did_document.clone()),
            key1_private_pem: generated.key1_private_pem.clone(),
            key1_public_pem: generated.key1_public_pem.clone(),
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem.clone(),
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem.clone(),
            make_default: false,
        })?;
        let identity = crate::internal::identity_recovery_local::identity_summary_from_generated(
            &generated,
            &raw,
            &plan.target,
            &plan.temp_identity_name,
        )?;
        let old_dids = plan.old_owner_dids_in_merge_order();
        let new_did = stored.did.as_str().to_string();
        let (store_merge_counts, e2ee_cleanup_counts) =
            crate::internal::identity_recover_local_state::merge_recovered_handle_local_state(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &old_dids,
                &new_did,
                &plan.final_identity_name,
            )?;
        let archived_identity_names = plan.archived_identity_names();
        let promoted = store.promote_recovered_handle(
            &plan.final_identity_name,
            &plan.temp_identity_name,
            &archived_identity_names,
        )?;
        let active_was_archived = archived_identity_names
            .iter()
            .any(|name| name.trim() == active_before && !name.trim().is_empty());
        let active_config_updated = if active_was_archived {
            if let Some(config_file) = local_finalize.config_file_path.as_deref() {
                crate::internal::identity_recovery_local::update_active_identity_in_config(
                    config_file,
                    &plan.final_identity_name,
                )?
            } else {
                false
            }
        } else {
            false
        };
        let mut summary_stored = stored.clone();
        summary_stored.local_alias = plan.final_identity_name.clone();
        summary_stored.is_default = promoted.default_updated;
        let final_summary =
            crate::internal::identity_recovery_local::local_identity_summary_from_stored(
                &summary_stored,
            );
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
        let local_recovery = crate::identity::RecoverHandleLocalResult {
            identity: final_summary,
            backup_path: backup.backup_path,
            archived_identities: archived_identity_names,
            archived_dids: plan.archived_dids(),
            full_handle: plan.target.target_handle.as_str().to_string(),
            final_identity_name: plan.final_identity_name,
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
            plan.target.target_handle,
            phone,
            crate::identity::RecoverHandleState::Recovered,
            Some(recovered_identity),
            Some(local_recovery),
            Some(raw.clone()),
            warnings,
        );
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }
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

fn did_from_raw(raw: &Value) -> Option<crate::ids::Did> {
    raw.get("did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| crate::ids::Did::parse(value).ok())
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
}
