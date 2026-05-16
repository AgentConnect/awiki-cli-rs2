use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_transport::GroupE2eeTransport;
use super::service::{require_active_identity, string_value};
use super::{build_group_e2ee_publish_key_package_rpc_params, CommandResult, MessageError};
use crate::anpsdk::generate_did_wba_binding;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::message::{load_private_key_material, verification_method_id_from_document};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eePublishKeyPackageRequest {
    pub identity_name: String,
    pub device_id: String,
    pub group: String,
    pub purpose: String,
    pub contract_test: bool,
}

pub fn publish_group_e2ee_key_package(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupE2eePublishKeyPackageRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let device_id = default_string(request.device_id.trim(), "default");
    let group_did = request.group.trim().to_string();
    let purpose = normalize_group_key_package_purpose(&request.purpose)?;
    if matches!(purpose.as_str(), "recovery" | "update") && group_did.is_empty() {
        return Err(publish_error(format!(
            "group DID is required when publishing a {purpose} KeyPackage"
        )));
    }

    let mut params = Map::new();
    params.insert("agent_did".to_string(), Value::String(record.did.clone()));
    params.insert("device_id".to_string(), Value::String(device_id.clone()));
    params.insert("owner_did".to_string(), Value::String(record.did.clone()));
    if purpose != "normal" {
        params.insert("purpose".to_string(), Value::String(purpose.clone()));
        params.insert("group_did".to_string(), Value::String(group_did.clone()));
    }

    let mut provider_request = Map::new();
    provider_request.insert(
        "api_version".to_string(),
        Value::String(ANP_MLS_API_VERSION.to_string()),
    );
    provider_request.insert(
        "request_id".to_string(),
        Value::String(format!(
            "group-e2ee-key-package-{}",
            super::wire::generate_operation_id()
        )),
    );
    provider_request.insert("agent_did".to_string(), Value::String(record.did.clone()));
    provider_request.insert("device_id".to_string(), Value::String(device_id.clone()));
    if request.contract_test {
        provider_request.insert("contract_test_enabled".to_string(), Value::Bool(true));
    }
    provider_request.insert("params".to_string(), Value::Object(params));
    let provider = MlsExecProvider::new(resolved);
    let mut package_result =
        provider.generate_key_package(&Value::Object(provider_request), &record.did, &device_id)?;
    package_result =
        tag_group_key_package_purpose(package_result, &group_did, &device_id, &purpose);
    package_result = sign_group_key_package_did_wba_binding(&record, package_result)?;

    let mut transport = GroupE2eePublishTransport::new(resolved, manager, &record)?;
    let published = transport.publish_key_package(package_result.clone())?;

    Ok(CommandResult {
        data: json!({
            "mls": Value::Object(package_result),
            "published": Value::Object(published),
            "recovery": purpose == "recovery",
            "purpose": purpose,
            "group": group_did,
            "device_id": device_id,
            "argv_safe": true,
            "p4_mutates": false,
        }),
        summary: "Published group E2EE KeyPackage".to_string(),
        warnings: Vec::new(),
    })
}

fn normalize_group_key_package_purpose(raw: &str) -> Result<String, MessageError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "normal" => Ok("normal".to_string()),
        "recovery" => Ok("recovery".to_string()),
        "update" => Ok("update".to_string()),
        _ => Err(publish_error(
            "group E2EE KeyPackage purpose must be normal, recovery, or update",
        )),
    }
}

fn tag_group_key_package_purpose(
    mut package_result: Map<String, Value>,
    group_did: &str,
    device_id: &str,
    purpose: &str,
) -> Map<String, Value> {
    if purpose == "normal" {
        return package_result;
    }
    let Some(group_key_package) = package_result
        .get("group_key_package")
        .and_then(Value::as_object)
        .cloned()
    else {
        return package_result;
    };
    if group_key_package.is_empty() {
        return package_result;
    }
    let mut tagged = group_key_package;
    tagged.insert("purpose".to_string(), Value::String(purpose.to_string()));
    tagged.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    tagged.insert(
        "device_id".to_string(),
        Value::String(default_string(device_id.trim(), "default")),
    );
    package_result.insert("group_key_package".to_string(), Value::Object(tagged));
    package_result
}

fn sign_group_key_package_did_wba_binding(
    record: &StoredIdentity,
    package_result: Map<String, Value>,
) -> Result<Map<String, Value>, MessageError> {
    if package_result.is_empty() {
        return Err(publish_error("anp-mls key-package response is empty"));
    }
    let group_key_package = package_result
        .get("group_key_package")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| publish_error("anp-mls key-package response missing group_key_package"))?;
    let binding = group_key_package
        .get("did_wba_binding")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| publish_error("group_key_package.did_wba_binding is required"))?;

    let owner_did = default_string(
        &string_value(group_key_package.get("owner_did")),
        &record.did,
    );
    if owner_did != record.did {
        return Err(publish_error(
            "group_key_package.owner_did must match active identity",
        ));
    }
    let agent_did = default_string(&string_value(binding.get("agent_did")), &record.did);
    if agent_did != record.did {
        return Err(publish_error(
            "did_wba_binding.agent_did must match active identity",
        ));
    }
    let did_document = record.did_document.as_ref().ok_or_else(|| {
        publish_error("active DID document does not expose a signing verification method")
    })?;
    let verification_method =
        verification_method_id_from_document(did_document).ok_or_else(|| {
            publish_error("active DID document does not expose a signing verification method")
        })?;
    if verification_method.trim().is_empty() {
        return Err(publish_error(
            "active DID document does not expose a signing verification method",
        ));
    }

    let leaf_signature_key = required_binding_string(binding, "leaf_signature_key_b64u")?;
    let issued_at = required_binding_string(binding, "issued_at")?;
    let expires_at = required_binding_string(binding, "expires_at")?;
    let private_key = load_private_key_material(&record.key1_private_pem)
        .map_err(|err| publish_error(format!("load active identity signing key: {err}")))?;
    let signed_binding = generate_did_wba_binding(
        &record.did,
        &verification_method,
        &leaf_signature_key,
        &private_key,
        &issued_at,
        &expires_at,
        Some(issued_at.clone()),
    )
    .map_err(|err| publish_error(format!("sign did_wba_binding: {err}")))?;

    let mut signed_group_key_package = group_key_package.clone();
    signed_group_key_package.insert("owner_did".to_string(), Value::String(record.did.clone()));
    signed_group_key_package.insert("did_wba_binding".to_string(), signed_binding);
    let mut signed_package_result = package_result;
    signed_package_result.insert(
        "group_key_package".to_string(),
        Value::Object(signed_group_key_package),
    );
    Ok(signed_package_result)
}

fn required_binding_string(
    binding: &Map<String, Value>,
    key: &str,
) -> Result<String, MessageError> {
    let value = string_value(binding.get(key));
    if value.trim().is_empty() {
        return Err(publish_error(format!("did_wba_binding.{key} is required")));
    }
    Ok(value)
}

fn publish_error(message: impl Into<String>) -> MessageError {
    MessageError::Internal(message.into())
}

struct GroupE2eePublishTransport<'a> {
    inner: GroupE2eeTransport<'a>,
    record: &'a StoredIdentity,
}

impl<'a> GroupE2eePublishTransport<'a> {
    fn new(
        resolved: &'a Resolved,
        manager: &Manager,
        record: &'a StoredIdentity,
    ) -> Result<Self, MessageError> {
        Ok(Self {
            inner: GroupE2eeTransport::new(resolved, manager, record)?,
            record,
        })
    }

    fn publish_key_package(
        &mut self,
        package_result: Map<String, Value>,
    ) -> Result<Map<String, Value>, MessageError> {
        let service_did = self.inner.message_service_did()?;
        let params = build_group_e2ee_publish_key_package_rpc_params(
            self.record,
            &service_did,
            package_result,
        )?;
        self.inner
            .rpc_call("group.e2ee.publish_key_package", params)
    }
}
