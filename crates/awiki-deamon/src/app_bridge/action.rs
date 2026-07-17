use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::state::{
    AppMessageAgentBindingRecord, AuthorizedRuntimeContext, DaemonState, MessageSyncOutboxRecord,
};

pub const APP_CAPABILITIES_SCHEMA: &str = "awiki.app.capabilities.v1";
pub const APP_ACTION_SCHEMA: &str = "awiki.app.action.v1";
pub const APP_ACTION_RESULT_SCHEMA: &str = "awiki.app.action.result.v1";

pub const MVP_ALLOWED_ACTIONS: &[&str] = &[
    "message.summarize_plain",
    "message.create_draft",
    "contact.read",
    "contact.update_display_name",
    "contact.update_note",
];

const WRITE_ACTIONS: &[&str] = &["contact.update_display_name", "contact.update_note"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCapabilitiesEnvelope {
    pub schema: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub require_confirmation_for_write_actions: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAppActionRequest {
    #[serde(default)]
    pub action_id: Option<String>,
    pub action: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub source_message_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub args: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl std::fmt::Debug for RuntimeAppActionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeAppActionRequest")
            .field("action_id", &self.action_id)
            .field("action", &self.action)
            .field("idempotency_key", &self.idempotency_key)
            .field("source_message_id", &self.source_message_id)
            .field("conversation_id", &self.conversation_id)
            .field("args", &"<redacted-app-action-args>")
            .field("extra", &"<redacted-app-action-extra>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppActionResultEnvelope {
    pub schema: String,
    pub action_id: String,
    pub action: String,
    pub state: String,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_summary: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppActionQueueOutcome {
    pub action_id: String,
    pub action: String,
    pub state: String,
    pub idempotency_key: String,
    pub requires_confirmation: bool,
}

pub fn is_app_capabilities_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some(APP_CAPABILITIES_SCHEMA)
}

pub fn is_app_action_result_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some(APP_ACTION_RESULT_SCHEMA)
}

pub fn parse_app_capabilities_payload(payload: Value) -> Result<AppCapabilitiesEnvelope> {
    let envelope: AppCapabilitiesEnvelope =
        serde_json::from_value(payload).context("parse app capabilities payload")?;
    if envelope.schema != APP_CAPABILITIES_SCHEMA {
        bail!("unsupported app capabilities schema: {}", envelope.schema);
    }
    for capability in &envelope.capabilities {
        validate_action_name(capability)
            .with_context(|| format!("validate app capability {}", capability.trim()))?;
    }
    Ok(envelope)
}

pub fn parse_app_action_result_payload(payload: Value) -> Result<AppActionResultEnvelope> {
    let envelope: AppActionResultEnvelope =
        serde_json::from_value(payload).context("parse app action result payload")?;
    if envelope.schema != APP_ACTION_RESULT_SCHEMA {
        bail!("unsupported app action result schema: {}", envelope.schema);
    }
    require_non_empty("action_id", &envelope.action_id)?;
    validate_action_name(&envelope.action)?;
    validate_action_state(&envelope.state)?;
    reject_forbidden_action_payload(&envelope.result)?;
    reject_forbidden_action_payload(&serde_json::to_value(&envelope.extra)?)?;
    Ok(envelope)
}

pub fn queue_runtime_app_action_request(
    state: &DaemonState,
    context: &AuthorizedRuntimeContext,
    params: &Value,
) -> Result<AppActionQueueOutcome> {
    let request: RuntimeAppActionRequest =
        serde_json::from_value(params.clone()).context("parse app.action.request params")?;
    validate_runtime_app_action_request(&request)?;
    let binding = state
        .load_active_app_message_agent_binding_by_runtime(&context.agent_did)?
        .with_context(|| {
            format!(
                "missing active app message binding for runtime {}",
                context.agent_did
            )
        })?;
    let action_id = request.action_id.clone().unwrap_or_else(|| {
        format!(
            "act_{}",
            stable_id_suffix(&format!(
                "{}:{}:{}",
                context.run_id, request.action, request.args
            ))
        )
    });
    let idempotency_key = request.idempotency_key.clone().unwrap_or_else(|| {
        format!(
            "app-action:{}:{}:{}",
            binding.user_did, context.run_id, action_id
        )
    });
    if let Err(error) = validate_action_allowed(&binding, &request.action) {
        queue_app_action_rejected_result(
            state,
            context,
            &binding,
            &request,
            &action_id,
            &idempotency_key,
            "action_not_allowed",
            &error.to_string(),
        )?;
        state.insert_audit_event_json(
            "runtime.app_action.rejected",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "action_id": action_id,
                "action": request.action,
                "reason": "action_not_allowed",
            }),
        )?;
        return Err(error);
    }

    let requires_confirmation = action_requires_confirmation(&binding, &request.action);
    let action_state = if requires_confirmation {
        "requires_confirmation"
    } else {
        "requested"
    };
    state.upsert_message_sync_outbox(&MessageSyncOutboxRecord {
        idempotency_key: idempotency_key.clone(),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": APP_ACTION_SCHEMA,
            "action_id": action_id,
            "action": request.action,
            "state": action_state,
            "binding_id": binding.binding_id,
            "owner_did": binding.user_did,
            "app_instance_id": binding.app_instance_id,
            "daemon_agent_did": binding.daemon_agent_did,
            "runtime_agent_did": context.agent_did,
            "runtime_profile_id": context.runtime_profile_id,
            "run_id": context.run_id,
            "source_message_id": request.source_message_id,
            "conversation_id": request.conversation_id,
            "requires_confirmation": requires_confirmation,
            "args": request.args,
            "allowed_actions": effective_allowed_actions(&binding),
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })?;
    state.insert_audit_event_json(
        "runtime.app_action.queued",
        Some(&context.agent_did),
        Some(&context.runtime_profile_id),
        Some(&context.run_id),
        Some(&context.token_id),
        json!({
            "action_id": action_id,
            "action": request.action,
            "state": action_state,
            "requires_confirmation": requires_confirmation,
        }),
    )?;
    Ok(AppActionQueueOutcome {
        action_id,
        action: request.action,
        state: action_state.to_string(),
        idempotency_key,
        requires_confirmation,
    })
}

fn validate_runtime_app_action_request(request: &RuntimeAppActionRequest) -> Result<()> {
    validate_action_name(&request.action)?;
    if let Some(action_id) = request.action_id.as_deref() {
        validate_wire_id("action_id", action_id)?;
    }
    if let Some(idempotency_key) = request.idempotency_key.as_deref() {
        validate_wire_id("idempotency_key", idempotency_key)?;
    }
    reject_forbidden_action_payload(&request.args)?;
    reject_forbidden_action_payload(&serde_json::to_value(&request.extra)?)?;
    Ok(())
}

fn validate_action_allowed(binding: &AppMessageAgentBindingRecord, action: &str) -> Result<()> {
    validate_action_name(action)?;
    if !MVP_ALLOWED_ACTIONS.contains(&action) {
        bail!("app action is not in MVP allowlist: {action}");
    }
    let allowed = effective_allowed_actions(binding);
    if !allowed.iter().any(|candidate| candidate == action) {
        bail!("app action is not enabled for this binding: {action}");
    }
    Ok(())
}

fn effective_allowed_actions(binding: &AppMessageAgentBindingRecord) -> Vec<String> {
    let has_explicit_capability_policy = binding
        .capability_policy_json
        .get("schema")
        .and_then(Value::as_str)
        == Some(APP_CAPABILITIES_SCHEMA);
    let configured = if has_explicit_capability_policy {
        binding
            .capability_policy_json
            .get("capabilities")
            .and_then(Value::as_array)
            .or_else(|| {
                binding
                    .capability_policy_json
                    .get("allowed_actions")
                    .and_then(Value::as_array)
            })
    } else {
        binding
            .desired_agent_json
            .get("allowed_actions")
            .and_then(Value::as_array)
    };
    let actions = configured
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| MVP_ALLOWED_ACTIONS.contains(value))
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    actions.into_iter().collect()
}

fn action_requires_confirmation(binding: &AppMessageAgentBindingRecord, action: &str) -> bool {
    if !WRITE_ACTIONS.contains(&action) {
        return false;
    }
    binding
        .capability_policy_json
        .get("require_confirmation_for_write_actions")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn queue_app_action_rejected_result(
    state: &DaemonState,
    context: &AuthorizedRuntimeContext,
    binding: &AppMessageAgentBindingRecord,
    request: &RuntimeAppActionRequest,
    action_id: &str,
    idempotency_key: &str,
    error_code: &str,
    error_summary: &str,
) -> Result<()> {
    state.upsert_message_sync_outbox(&MessageSyncOutboxRecord {
        idempotency_key: format!("{idempotency_key}:rejected"),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": APP_ACTION_RESULT_SCHEMA,
            "action_id": action_id,
            "action": request.action,
            "state": "rejected",
            "binding_id": binding.binding_id,
            "owner_did": binding.user_did,
            "app_instance_id": binding.app_instance_id,
            "daemon_agent_did": binding.daemon_agent_did,
            "runtime_agent_did": context.agent_did,
            "runtime_profile_id": context.runtime_profile_id,
            "run_id": context.run_id,
            "source_message_id": request.source_message_id,
            "conversation_id": request.conversation_id,
            "error_code": error_code,
            "error_summary": sanitize_user_visible_error(error_summary),
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })?;
    Ok(())
}

fn validate_action_name(action: &str) -> Result<()> {
    require_non_empty("action", action)?;
    if action.contains("e2ee")
        || action.contains("export")
        || action.contains("delete")
        || action.contains("identity")
        || action.contains("key")
    {
        bail!("app action is not allowed in MVP: {action}");
    }
    if !action
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_'))
    {
        bail!("app action contains unsupported characters: {action}");
    }
    Ok(())
}

fn validate_action_state(state: &str) -> Result<()> {
    match state {
        "requested"
        | "requires_confirmation"
        | "accepted"
        | "rejected"
        | "succeeded"
        | "failed" => Ok(()),
        other => bail!("unsupported app action state: {other}"),
    }
}

fn validate_wire_id(field: &str, value: &str) -> Result<()> {
    require_non_empty(field, value)?;
    if value.len() > 160 {
        bail!("{field} is too long");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.'))
    {
        bail!("{field} contains unsupported characters");
    }
    Ok(())
}

fn reject_forbidden_action_payload(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                reject_forbidden_action_name(key)?;
                reject_forbidden_action_payload(value)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                reject_forbidden_action_payload(value)?;
            }
        }
        Value::String(value) => reject_forbidden_action_name(value)?,
        _ => {}
    }
    Ok(())
}

fn reject_forbidden_action_name(value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("private_key")
        || lower.contains("private key")
        || lower.contains("private_state")
        || lower.contains("session_private")
        || lower.contains("key_package_private")
        || lower.contains("e2ee_plaintext")
        || lower.contains("rtok_")
        || lower.contains("jwt")
        || lower.contains("bearer ")
        || lower.contains("begin private key")
        || lower.contains("registration_token")
        || lower.contains("secret")
    {
        bail!("app action payload contains forbidden private state");
    }
    Ok(())
}

fn sanitize_user_visible_error(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
            {
                "<redacted>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.chars().count() > 200 {
        sanitized = sanitized.chars().take(200).collect();
    }
    sanitized
}

fn require_non_empty(field_name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field_name} must not be empty");
    }
    Ok(())
}

fn stable_id_suffix(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::config::DaemonConfig;

    #[test]
    fn allowed_contact_write_action_queues_confirmation_request() {
        let fixture = fixture(json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": MVP_ALLOWED_ACTIONS,
            "require_confirmation_for_write_actions": true
        }));
        let outcome = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_contact_note_1",
                "action": "contact.update_note",
                "source_message_id": "msg_1",
                "conversation_id": "direct:did:human:bob",
                "args": {
                    "contact_did": "did:human:bob",
                    "note": "Follow up about the launch"
                }
            }),
        )
        .unwrap();

        assert_eq!(outcome.state, "requires_confirmation");
        assert!(outcome.requires_confirmation);
        let record = fixture
            .state
            .load_message_sync_outbox(&outcome.idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(record.payload_json["schema"], APP_ACTION_SCHEMA);
        assert_eq!(record.payload_json["action"], "contact.update_note");
        assert_eq!(record.payload_json["state"], "requires_confirmation");
        assert_eq!(record.payload_json["requires_confirmation"], true);
        assert_eq!(record.payload_json["daemon_agent_did"], "did:agent:daemon");
        assert_eq!(record.payload_json["runtime_agent_did"], "did:agent:hermes");
        assert_eq!(record.payload_json["args"]["contact_did"], "did:human:bob");
    }

    #[test]
    fn high_risk_action_is_rejected_and_result_is_queued() {
        let fixture = fixture(json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": MVP_ALLOWED_ACTIONS
        }));
        let error = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_send_1",
                "action": "message.send",
                "args": {
                    "to": "did:human:bob",
                    "text": "send this"
                }
            }),
        )
        .unwrap_err();

        assert!(error.to_string().contains("MVP allowlist"));
        let record = fixture
            .state
            .load_message_sync_outbox(
                "app-action:did:human:alice:run_user_msg_1:act_send_1:rejected",
            )
            .unwrap()
            .unwrap();
        assert_eq!(record.payload_json["schema"], APP_ACTION_RESULT_SCHEMA);
        assert_eq!(record.payload_json["action"], "message.send");
        assert_eq!(record.payload_json["state"], "rejected");
        assert_eq!(record.payload_json["daemon_agent_did"], "did:agent:daemon");
        assert_eq!(record.payload_json["runtime_agent_did"], "did:agent:hermes");
        assert_eq!(record.payload_json["error_code"], "action_not_allowed");
    }

    #[test]
    fn binding_capability_policy_restricts_mvp_action_subset() {
        let fixture = fixture(json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": ["message.summarize_plain"]
        }));
        let error = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_read_contact_1",
                "action": "contact.read",
                "args": {
                    "contact_did": "did:human:bob"
                }
            }),
        )
        .unwrap_err();

        assert!(error.to_string().contains("not enabled"));
    }

    #[test]
    fn empty_explicit_capabilities_disable_app_actions() {
        let fixture = fixture(json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": []
        }));
        let error = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_summary_1",
                "action": "message.summarize_plain",
                "args": {"message_id": "msg_1"}
            }),
        )
        .unwrap_err();

        assert!(error.to_string().contains("not enabled"));
    }

    #[test]
    fn missing_capability_policy_does_not_default_to_all_actions_for_new_binding() {
        let fixture = fixture(json!({}));
        let error = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_summary_1",
                "action": "message.summarize_plain",
                "args": {"message_id": "msg_1"}
            }),
        )
        .unwrap_err();

        assert!(error.to_string().contains("not enabled"));
    }

    #[test]
    fn legacy_binding_without_capability_schema_can_use_desired_allowed_actions() {
        let fixture = fixture_with_desired_actions(json!({}), json!(["message.summarize_plain"]));
        let outcome = queue_runtime_app_action_request(
            &fixture.state,
            &fixture.context,
            &json!({
                "action_id": "act_summary_1",
                "action": "message.summarize_plain",
                "args": {"message_id": "msg_1"}
            }),
        )
        .unwrap();

        assert_eq!(outcome.state, "requested");
        assert!(!outcome.requires_confirmation);
    }

    #[test]
    fn app_capabilities_and_result_payloads_parse_and_reject_private_state() {
        let capabilities = parse_app_capabilities_payload(json!({
            "schema": APP_CAPABILITIES_SCHEMA,
            "capabilities": ["message.summarize_plain", "contact.update_note"],
            "require_confirmation_for_write_actions": true
        }))
        .unwrap();
        assert_eq!(capabilities.capabilities.len(), 2);

        let result = parse_app_action_result_payload(json!({
            "schema": APP_ACTION_RESULT_SCHEMA,
            "action_id": "act_1",
            "action": "message.create_draft",
            "state": "succeeded",
            "result": {"draft_text": "Looks good"}
        }))
        .unwrap();
        assert_eq!(result.state, "succeeded");

        let private = parse_app_action_result_payload(json!({
            "schema": APP_ACTION_RESULT_SCHEMA,
            "action_id": "act_2",
            "action": "message.create_draft",
            "state": "succeeded",
            "result": {"private_key": "secret"}
        }))
        .unwrap_err();
        assert!(private.to_string().contains("forbidden private state"));
    }

    struct TestFixture {
        _root: TempDir,
        state: DaemonState,
        context: AuthorizedRuntimeContext,
    }

    fn fixture(capability_policy_json: Value) -> TestFixture {
        fixture_with_desired_actions(capability_policy_json, Value::Null)
    }

    fn fixture_with_desired_actions(
        capability_policy_json: Value,
        allowed_actions: Value,
    ) -> TestFixture {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let mut desired_agent_json = json!({
            "role": "app_message_handler"
        });
        if !allowed_actions.is_null() {
            desired_agent_json["allowed_actions"] = allowed_actions;
        }
        let binding = AppMessageAgentBindingRecord {
            binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
            user_did: "did:human:alice".to_string(),
            inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "message-agent-bootstrap:did:human:alice:app_1".to_string(),
            daemon_agent_did: "did:agent:daemon".to_string(),
            runtime_agent_did: "did:agent:hermes".to_string(),
            runtime_profile_id: "profile_hermes".to_string(),
            role: "app_message_handler".to_string(),
            desired_agent_json,
            capability_policy_json,
            status: "message_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        };
        state.upsert_app_message_agent_binding(&binding).unwrap();
        let context = AuthorizedRuntimeContext {
            token_id: "token_1".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            run_id: "run_user_msg_1".to_string(),
            method: crate::security::runtime_token::RpcMethod::AppActionRequest,
        };
        TestFixture {
            _root: root,
            state,
            context,
        }
    }
}
