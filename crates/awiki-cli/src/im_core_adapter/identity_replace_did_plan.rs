// Temporary migration-only legacy bridge exception.
// Delete in PR C3/C7 when replace-did is hidden/advanced through the cutover
// classifier and any remaining execution path uses im-core public identity APIs
// without legacy local identity mutation bridges in this adapter.

use im_core::prelude::{
    Did, Handle, IdentityId, IdentitySelector, ReplaceDidAffectedLocalState,
    ReplaceDidExecutionRequest, ReplaceDidExecutionResult, ReplaceDidGeneratedIdentity,
    ReplaceDidPlan, ReplaceDidPlanRequest,
};
use serde_json::{json, Value};

use crate::config;
use crate::identity;
use crate::output::ExitError;
use crate::store;
use crate::transportcfg::Profile;

#[derive(Debug, Clone)]
pub struct ReplaceDidPlanBridgeRequest {
    pub sdk: ReplaceDidPlanRequest,
    pub identity_name: String,
}

struct ReplaceDidExecutionBridgeRequest {
    sdk: ReplaceDidExecutionRequest,
    identity_name: String,
    record: identity::types::StoredIdentity,
    generated: ReplaceDidGeneratedLocalIdentity,
}

#[derive(Debug, Clone)]
struct ReplaceDidGeneratedLocalIdentity {
    key1_private_pem: String,
    key1_public_pem: String,
    e2ee_signing_private_pem: String,
    e2ee_agreement_private_pem: String,
}

pub fn replace_did_plan_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = replace_did_plan_bridge_request(
        resolved,
        manager,
        identity_name,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )?;
    let client = super::build_im_client(
        resolved,
        manager,
        IdentitySelector::LocalAlias(bridge.identity_name.clone()),
    )?;
    let plan = im_core::compat::identity::replace_did_plan_with_bridge(&client, bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    replace_did_plan_command_result(plan)
}

pub fn replace_did_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<String>,
    endpoint_url: Option<String>,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = replace_did_execution_bridge_request(
        resolved,
        manager,
        identity_name,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )?;
    let client = super::build_im_client(
        resolved,
        manager,
        IdentitySelector::LocalAlias(bridge.identity_name.clone()),
    )?;
    let result = im_core::compat::identity::replace_did_with_bridge(
        &client,
        bridge.sdk,
        ReplaceDidExecutionBridge {
            resolved,
            manager,
            record: bridge.record,
            generated: bridge.generated,
        },
    )
    .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    replace_did_command_result(result)
}

pub fn replace_did_plan_bridge_request(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanBridgeRequest, ExitError> {
    build_replace_did_plan_bridge_request(
        resolved,
        manager,
        identity_name,
        None,
        is_public,
        is_agent,
        role,
        endpoint_url,
    )
}

fn replace_did_execution_bridge_request(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<String>,
    endpoint_url: Option<String>,
) -> Result<ReplaceDidExecutionBridgeRequest, ExitError> {
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_name)
        .map_err(crate::app::identity_exit)?;
    let (did_domain, path_segments) = handle_path_prefix_from_did(&record.did)?;
    let generated = identity::generate_identity_with_path_segments(
        &did_domain,
        path_segments.iter().map(String::as_str),
        &default_value_for_replacement(
            &resolved.anp_service_endpoint,
            &config::derive_anp_service_endpoint(&resolved.service_base_url),
        ),
        &default_value_for_replacement(
            &resolved.anp_service_did,
            &config::derive_anp_service_did(&resolved.service_base_url),
        ),
    )
    .map_err(crate::app::identity_exit)?;
    let generated_sdk = ReplaceDidGeneratedIdentity {
        did: Did::parse(&generated.did)
            .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        unique_id: generated.unique_id.clone(),
        did_document: generated.did_document.clone(),
    };
    let generated_local = ReplaceDidGeneratedLocalIdentity {
        key1_private_pem: generated.key1_private_pem,
        key1_public_pem: generated.key1_public_pem,
        e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
        e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
    };
    let bridge = build_replace_did_plan_bridge_request(
        resolved,
        manager,
        &record.identity_name,
        Some(generated_sdk.did.as_str().to_string()),
        is_public,
        is_agent,
        role.as_deref(),
        endpoint_url.as_deref(),
    )?;
    let client = super::build_im_client(
        resolved,
        manager,
        IdentitySelector::LocalAlias(bridge.identity_name.clone()),
    )?;
    let plan = im_core::compat::identity::replace_did_plan_with_bridge(&client, bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id replace-did"))?;
    Ok(ReplaceDidExecutionBridgeRequest {
        sdk: ReplaceDidExecutionRequest {
            plan,
            generated_identity: generated_sdk,
            is_public,
            is_agent,
            role,
            endpoint_url,
        },
        identity_name: record.identity_name.clone(),
        record,
        generated: generated_local,
    })
}

fn build_replace_did_plan_bridge_request(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_name: &str,
    planned_new_did_override: Option<String>,
    is_public: Option<bool>,
    is_agent: Option<bool>,
    role: Option<&str>,
    endpoint_url: Option<&str>,
) -> Result<ReplaceDidPlanBridgeRequest, ExitError> {
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_name)
        .map_err(crate::app::identity_exit)?;
    let planned_new_did = match planned_new_did_override {
        Some(value) => value,
        None => planned_replace_did(&record)?,
    };
    let (store_rebind_counts, e2ee_cleanup_counts) =
        store::plan_rebind_local_identity_state(&resolved.paths, &record.did, &planned_new_did)
            .map_err(|err| super::map_im_error(store_error_to_im_error(err), "id replace-did"))?;
    let summary = identity::store::identity_summary_from_record(&record);
    let sdk = ReplaceDidPlanRequest {
        identity: sdk_identity_summary(&summary)?,
        linked_identity_names: linked_identity_names(manager, &record)?,
        planned_new_did: Did::parse(&planned_new_did)
            .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        backup_path_preview: replace_did_backup_path_preview(manager, &record),
        old_dir_name: record.dir_name.clone(),
        is_public,
        is_agent,
        role: role.map(str::to_string),
        endpoint_url: endpoint_url.map(str::to_string),
        affected_local_state: ReplaceDidAffectedLocalState {
            store_rebind_counts,
            e2ee_cleanup_counts,
        },
    };
    Ok(ReplaceDidPlanBridgeRequest {
        sdk,
        identity_name: record.identity_name,
    })
}

fn replace_did_plan_command_result(
    plan: ReplaceDidPlan,
) -> Result<identity::CommandResult, ExitError> {
    let value = serde_json::to_value(&plan).map_err(|err| {
        ExitError::new(
            "serialization_error",
            1,
            format!("serialize replace DID plan: {err}"),
            "Report this issue with the command output.",
        )
    })?;
    Ok(identity::CommandResult {
        data: json!({
            "plan": value,
        }),
        summary: "Dry run: DID replacement planned".to_string(),
        warnings: vec![identity::replace_did_danger_warning().to_string()],
    })
}

fn replace_did_command_result(
    result: ReplaceDidExecutionResult,
) -> Result<identity::CommandResult, ExitError> {
    let identity = cli_identity_summary(&result.identity);
    let mut command_result = identity::wire::replace_did_result(
        &identity,
        result.old_did.as_str(),
        result.new_did.as_str(),
        &result.backup_path,
        result.remote_result,
    );
    insert_rebind_counts(
        &mut command_result.data,
        result.affected_local_state.store_rebind_counts,
        result.affected_local_state.e2ee_cleanup_counts,
    );
    command_result
        .warnings
        .insert(0, identity::replace_did_danger_warning().to_string());
    command_result.warnings.extend(result.warnings);
    command_result.warnings.extend(result.recovery_notes);
    Ok(command_result)
}

struct ReplaceDidExecutionBridge<'a> {
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
    generated: ReplaceDidGeneratedLocalIdentity,
}

impl im_core::compat::identity::BridgeReplaceDidExecution for ReplaceDidExecutionBridge<'_> {
    fn create_replace_did_backup(
        &mut self,
        plan: &ReplaceDidPlan,
    ) -> im_core::ImResult<im_core::compat::identity::ReplaceDidBackupBridgeResult> {
        let backup = self
            .manager
            .backup_identity_for_did_replacement(
                &self.record.identity_name,
                plan.backup_plan.manifest_preview.planned_new_did.as_str(),
            )
            .map_err(identity_error_to_im_error)?;
        Ok(im_core::compat::identity::ReplaceDidBackupBridgeResult {
            backup_path: backup.backup_path,
            manifest: im_core::identity::ReplaceDidBackupManifestPreview {
                reason: backup.manifest.reason,
                identity_name: backup.manifest.identity_name,
                linked_identity_names: backup.manifest.linked_identity_names,
                old_did: Did::parse(backup.manifest.old_did)?,
                old_dir_name: backup.manifest.old_dir_name,
                planned_new_did: Did::parse(backup.manifest.planned_new_did)?,
            },
        })
    }

    fn remote_replace_did(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        let mut auth = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        client
            .authenticated_rpc_call_profile(
                Profile::RpcDefault,
                endpoint,
                method,
                params,
                &mut auth,
            )
            .map_err(identity_error_to_im_error)
    }

    fn replace_local_identity(
        &mut self,
        request: &ReplaceDidExecutionRequest,
        remote_result: &Value,
    ) -> im_core::ImResult<im_core::compat::identity::ReplaceDidLocalIdentityUpdate> {
        let new_did = string_value(
            remote_result,
            "did",
            request.generated_identity.did.as_str(),
        );
        let replaced = self
            .manager
            .replace_identity(
                &self.record.identity_name,
                identity::types::SaveInput {
                    identity_name: self.record.identity_name.clone(),
                    did: new_did.clone(),
                    unique_id: identity::did_suffix(&new_did),
                    user_id: string_value(remote_result, "user_id", &self.record.user_id),
                    display_name: self.record.display_name.clone(),
                    handle: string_value(remote_result, "handle", &self.record.handle),
                    full_handle: default_string_value(
                        remote_result,
                        "full_handle",
                        &self.record.full_handle,
                    ),
                    jwt_token: string_value(remote_result, "access_token", &self.record.jwt_token),
                    did_document: Some(request.generated_identity.did_document.clone()),
                    key1_private_pem: self.generated.key1_private_pem.clone(),
                    key1_public_pem: self.generated.key1_public_pem.clone(),
                    e2ee_signing_private_pem: self.generated.e2ee_signing_private_pem.clone(),
                    e2ee_agreement_private_pem: self.generated.e2ee_agreement_private_pem.clone(),
                    replace_existing: true,
                },
            )
            .map_err(identity_error_to_im_error)?;
        let identity =
            sdk_identity_summary(&identity::store::identity_summary_from_record(&replaced))
                .map_err(|err| im_core::ImError::Internal {
                    message: err.to_string(),
                })?;
        Ok(im_core::compat::identity::ReplaceDidLocalIdentityUpdate {
            identity,
            new_did: Did::parse(&replaced.did)?,
        })
    }

    fn rebind_local_identity_state(
        &mut self,
        old_owner_did: &Did,
        new_owner_did: &Did,
    ) -> im_core::ImResult<ReplaceDidAffectedLocalState> {
        let (store_rebind_counts, e2ee_cleanup_counts) =
            store::rebind_local_identity_state_with_partial(
                &self.resolved.paths,
                old_owner_did.as_str(),
                new_owner_did.as_str(),
            )
            .map(|outcome| (outcome.store_rebind, outcome.e2ee_cleanup))
            .map_err(|err| store_error_to_im_error(err.error))?;
        Ok(ReplaceDidAffectedLocalState {
            store_rebind_counts,
            e2ee_cleanup_counts,
        })
    }
}

fn sdk_identity_summary(
    summary: &identity::IdentitySummary,
) -> Result<im_core::IdentitySummary, ExitError> {
    Ok(im_core::IdentitySummary {
        id: IdentityId::parse(first_non_empty([
            &summary.unique_id,
            &summary.identity_name,
            &summary.dir_name,
        ]))
        .map_err(|err| super::map_im_error(err, "id replace-did"))?,
        did: Did::parse(&summary.did).map_err(|err| super::map_im_error(err, "id replace-did"))?,
        handle: trimmed_optional(&summary.full_handle)
            .map(|handle| {
                Handle::parse(handle, "").map_err(|err| super::map_im_error(err, "id replace-did"))
            })
            .transpose()?,
        display_name: trimmed_optional(&summary.display_name),
        local_alias: trimmed_optional(&summary.identity_name),
        device_id: None,
        is_default: summary.is_default,
        readiness: im_core::identity::IdentityReadiness {
            ready_for_auth: summary.has_did_document
                && summary.has_key1_private
                && summary.has_key1_public,
            ready_for_messaging: summary.user_state.ready_for_messaging,
            missing: summary
                .user_state
                .missing
                .iter()
                .map(|item| match item.as_str() {
                    "handle" => im_core::identity::IdentityMissingItem::Handle,
                    "registration" => {
                        im_core::identity::IdentityMissingItem::Other("registration".to_string())
                    }
                    other => im_core::identity::IdentityMissingItem::Other(other.to_string()),
                })
                .collect(),
        },
    })
}

fn cli_identity_summary(summary: &im_core::IdentitySummary) -> identity::IdentitySummary {
    let identity_name = summary
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| summary.id.as_str())
        .to_string();
    let full_handle = summary
        .handle
        .as_ref()
        .map(|handle| handle.as_str().to_string())
        .unwrap_or_default();
    let handle = full_handle
        .split_once('.')
        .map(|(local, _)| local)
        .unwrap_or(full_handle.as_str())
        .to_string();
    identity::IdentitySummary {
        identity_name,
        did: summary.did.as_str().to_string(),
        unique_id: summary.id.as_str().to_string(),
        display_name: summary.display_name.clone().unwrap_or_default(),
        handle,
        full_handle,
        created_at: String::new(),
        dir_name: summary.id.as_str().to_string(),
        is_default: summary.is_default,
        has_jwt: summary.readiness.ready_for_auth,
        has_did_document: summary.readiness.ready_for_auth,
        has_key1_private: summary.readiness.ready_for_auth,
        has_key1_public: summary.readiness.ready_for_auth,
        has_e2ee_signing_private: summary.readiness.ready_for_messaging,
        has_e2ee_agreement_private: summary.readiness.ready_for_messaging,
        user_state: identity::UserState {
            registration_state: if summary.readiness.ready_for_messaging {
                "registered".to_string()
            } else {
                "local".to_string()
            },
            ready_for_messaging: summary.readiness.ready_for_messaging,
            missing: summary
                .readiness
                .missing
                .iter()
                .map(|item| match item {
                    im_core::identity::IdentityMissingItem::DidDocument => "did_document",
                    im_core::identity::IdentityMissingItem::PrivateKey => "private_key",
                    im_core::identity::IdentityMissingItem::AuthState => "auth",
                    im_core::identity::IdentityMissingItem::Handle => "handle",
                    im_core::identity::IdentityMissingItem::MessageEndpoint => "message_endpoint",
                    im_core::identity::IdentityMissingItem::Other(value) => value.as_str(),
                })
                .map(ToOwned::to_owned)
                .collect(),
        },
        user_id: String::new(),
    }
}

fn planned_replace_did(record: &identity::types::StoredIdentity) -> Result<String, ExitError> {
    let suffix = record
        .unique_id
        .trim()
        .strip_prefix("e1_")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| record.unique_id.trim());
    let suffix = if suffix.is_empty() { "planned" } else { suffix };
    let base = record
        .did
        .rsplit_once(':')
        .map(|(base, _)| base)
        .filter(|base| !base.trim().is_empty())
        .ok_or_else(|| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid current DID {:?}.", record.did),
                "Use a handle-backed did:wba identity before replacing DID.",
            )
        })?;
    Ok(format!("{base}:e1_replacement_{suffix}"))
}

fn replace_did_backup_path_preview(
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
) -> String {
    let identity_name = sanitize_component(&record.identity_name);
    let dir_name = sanitize_component(&record.dir_name);
    std::path::Path::new(manager.root_dir())
        .join(identity::types::LEGACY_BACKUP_DIR_NAME)
        .join("replace-did")
        .join(format!("<timestamp>-{identity_name}-{dir_name}"))
        .to_string_lossy()
        .into_owned()
}

fn linked_identity_names(
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
) -> Result<Vec<String>, ExitError> {
    let index = manager.load_index().map_err(crate::app::identity_exit)?;
    let mut names = index
        .credentials
        .iter()
        .filter_map(|(name, entry)| {
            (entry.dir_name == record.dir_name || entry.did == record.did).then(|| name.clone())
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        names.push(record.identity_name.clone());
    }
    names.sort();
    Ok(names)
}

fn sanitize_component(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .to_string()
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("identity")
        .to_string()
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn insert_rebind_counts(
    data: &mut serde_json::Value,
    store_rebind: std::collections::BTreeMap<String, i64>,
    e2ee_cleanup: std::collections::BTreeMap<String, i64>,
) {
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "store_rebind".to_string(),
            serde_json::to_value(store_rebind).unwrap_or_else(|_| json!({})),
        );
        object.insert(
            "e2ee_cleanup".to_string(),
            serde_json::to_value(e2ee_cleanup).unwrap_or_else(|_| json!({})),
        );
    }
}

fn identity_error_to_im_error(err: identity::IdentityError) -> im_core::ImError {
    match err {
        identity::IdentityError::InvalidInput(message) => {
            im_core::ImError::invalid_input(None, message)
        }
        identity::IdentityError::NotFound(message)
        | identity::IdentityError::NoDefaultIdentity(message) => {
            im_core::ImError::IdentityNotFound { selector: message }
        }
        identity::IdentityError::AuthRequired(_) => im_core::ImError::AuthRequired,
        identity::IdentityError::Service(service) => im_core::ImError::Service {
            status_code: (service.status_code != 0).then_some(service.status_code),
            code: (service.rpc_code != 0).then(|| service.rpc_code.to_string()),
            message: service.message,
        },
        identity::IdentityError::Io(err) => im_core::ImError::Io {
            detail: err.to_string(),
        },
        identity::IdentityError::Json(err) => im_core::ImError::Serialization {
            detail: err.to_string(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}

fn store_error_to_im_error(err: store::StoreError) -> im_core::ImError {
    match err {
        store::StoreError::Invalid(message) => im_core::ImError::invalid_input(None, message),
        store::StoreError::NotFound(message) => {
            im_core::ImError::LocalStateUnavailable { detail: message }
        }
        err => im_core::ImError::LocalStateUnavailable {
            detail: err.to_string(),
        },
    }
}

fn handle_path_prefix_from_did(did: &str) -> Result<(String, Vec<String>), ExitError> {
    let (domain, path_segments) = parse_did_path(did)?;
    if path_segments.is_empty() || path_segments[0].eq_ignore_ascii_case("user") {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: current did is not a handle did",
            "Use a handle-backed identity before replacing DID.",
        ));
    }
    Ok((domain, path_segments))
}

fn parse_did_path(did: &str) -> Result<(String, Vec<String>), ExitError> {
    let trimmed = did.trim();
    if !trimmed.starts_with("did:wba:") {
        return Err(invalid_did_exit(did));
    }
    let parts = trimmed.split(':').collect::<Vec<_>>();
    if parts.len() < 5 {
        return Err(invalid_did_exit(did));
    }
    let domain = path_unescape(parts[2]).ok_or_else(|| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid input: invalid did domain {:?}", parts[2]),
            "Use a handle-backed did:wba identity before replacing DID.",
        )
    })?;
    let path_segments = parts[3..parts.len() - 1]
        .iter()
        .map(|segment| (*segment).to_string())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: missing did path segments",
            "Use a handle-backed did:wba identity before replacing DID.",
        ));
    }
    Ok((domain, path_segments))
}

fn invalid_did_exit(did: &str) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        format!("invalid input: invalid did {did:?}"),
        "Use a handle-backed did:wba identity before replacing DID.",
    )
}

fn path_unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let hi = hex_value(bytes[index + 1])?;
        let lo = hex_value(bytes[index + 2])?;
        output.push((hi << 4) | lo);
        index += 3;
    }
    String::from_utf8(output).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn default_value_for_replacement(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn string_value(result: &Value, key: &str, fallback: &str) -> String {
    result
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.to_string())
}

fn default_string_value(result: &Value, key: &str, fallback: &str) -> String {
    let value = string_value(result, key, "");
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}
