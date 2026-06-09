use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app_bridge::secret_store::{secret_from_private_key_multibase, SecretString};
use crate::state::{
    BootstrapReplayRecord, BootstrapStoreOutcome, DaemonState, UserDelegatedIdentityRecord,
};

pub const DAEMON_BOOTSTRAP_SCHEMA: &str = "awiki.daemon.bootstrap.v1";
pub const USER_SUBKEY_PACKAGE_SCHEMA: &str = "awiki.daemon.user_subkey_package.v1";
pub const DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED: &str = "paired_key_received";
const MVP_DAEMON_KEY_FRAGMENT: &str = "daemon-key-1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonBootstrapEnvelope {
    pub schema: String,
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub app_instance_id: String,
    pub controller_did: String,
    #[serde(default)]
    pub user_handle: Option<String>,
    pub user_subkey_package: UserSubkeyPackage,
    #[serde(default)]
    pub capability_policy: Value,
    #[serde(default)]
    pub desired_message_agent: Value,
    #[serde(default)]
    pub sync_policy: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSubkeyPackage {
    pub schema: String,
    pub user_did: String,
    pub verification_method: String,
    #[serde(default)]
    pub key_type: Option<String>,
    pub public_key_multibase: String,
    pub private_key_multibase: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub allowed_scopes: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapProcessOutcome {
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub user_did: String,
    pub verification_method: String,
    pub app_instance_id: String,
    pub daemon_agent_did: String,
    pub status: String,
    pub replayed: bool,
}

impl std::fmt::Debug for DaemonBootstrapEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonBootstrapEnvelope")
            .field("schema", &self.schema)
            .field("bootstrap_id", &self.bootstrap_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("app_instance_id", &self.app_instance_id)
            .field("controller_did", &self.controller_did)
            .field("user_handle", &self.user_handle)
            .field("user_subkey_package", &self.user_subkey_package)
            .field("capability_policy", &"<redacted-control-payload>")
            .field("desired_message_agent", &"<redacted-control-payload>")
            .field("sync_policy", &"<redacted-control-payload>")
            .field("extra", &"<redacted-control-payload>")
            .finish()
    }
}

impl std::fmt::Debug for UserSubkeyPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserSubkeyPackage")
            .field("schema", &self.schema)
            .field("user_did", &self.user_did)
            .field("verification_method", &self.verification_method)
            .field("key_type", &self.key_type)
            .field("public_key_multibase", &self.public_key_multibase)
            .field("private_key_multibase", &"<redacted-private-key>")
            .field("expires_at", &self.expires_at)
            .field("allowed_scopes", &self.allowed_scopes)
            .field("extra", &"<redacted-control-payload>")
            .finish()
    }
}

pub fn parse_bootstrap_payload(payload: Value) -> Result<DaemonBootstrapEnvelope> {
    serde_json::from_value(payload).context("parse daemon bootstrap payload")
}

pub fn is_daemon_bootstrap_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some(DAEMON_BOOTSTRAP_SCHEMA)
}

pub fn process_bootstrap_envelope(
    state: &DaemonState,
    daemon_agent_did: &str,
    sender_did: &str,
    envelope: DaemonBootstrapEnvelope,
) -> Result<BootstrapProcessOutcome> {
    validate_bootstrap_envelope(&envelope, sender_did)?;
    let payload_hash = stable_payload_hash(&envelope)?;
    let package = &envelope.user_subkey_package;
    let private_key = secret_from_private_key_multibase(&package.private_key_multibase);
    let identity = UserDelegatedIdentityRecord {
        user_did: package.user_did.clone(),
        verification_method: package.verification_method.clone(),
        app_instance_id: envelope.app_instance_id.clone(),
        controller_did: envelope.controller_did.clone(),
        daemon_agent_did: daemon_agent_did.to_string(),
        public_key_multibase: package.public_key_multibase.clone(),
        private_key_material: private_key.expose_secret().to_string(),
        allowed_scopes_json: json!(package.allowed_scopes),
        status: DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED.to_string(),
        expires_at: package.expires_at.clone(),
        bootstrap_id: envelope.bootstrap_id.clone(),
        idempotency_key: envelope.idempotency_key.clone(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let replay = BootstrapReplayRecord {
        bootstrap_id: envelope.bootstrap_id.clone(),
        idempotency_key: envelope.idempotency_key.clone(),
        payload_hash: payload_hash.clone(),
        user_did: package.user_did.clone(),
        verification_method: package.verification_method.clone(),
        app_instance_id: envelope.app_instance_id.clone(),
        daemon_agent_did: daemon_agent_did.to_string(),
        status: DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED.to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let outcome = state.store_bootstrap_state(&identity, &replay)?;
    let replayed = matches!(outcome, BootstrapStoreOutcome::Duplicate);
    state.insert_audit_event_json(
        "daemon.bootstrap.received",
        Some(daemon_agent_did),
        None,
        None,
        None,
        json!({
            "bootstrap_id": &envelope.bootstrap_id,
            "idempotency_key": &envelope.idempotency_key,
            "user_did": &package.user_did,
            "verification_method": &package.verification_method,
            "app_instance_id": &envelope.app_instance_id,
            "status": DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED,
            "replayed": replayed,
            "payload_hash": payload_hash,
        }),
    )?;
    Ok(BootstrapProcessOutcome {
        bootstrap_id: replay.bootstrap_id,
        idempotency_key: replay.idempotency_key,
        user_did: replay.user_did,
        verification_method: replay.verification_method,
        app_instance_id: replay.app_instance_id,
        daemon_agent_did: replay.daemon_agent_did,
        status: replay.status,
        replayed,
    })
}

fn validate_bootstrap_envelope(
    envelope: &DaemonBootstrapEnvelope,
    message_sender_did: &str,
) -> Result<()> {
    require_non_empty("bootstrap_id", &envelope.bootstrap_id)?;
    require_non_empty("idempotency_key", &envelope.idempotency_key)?;
    require_non_empty("app_instance_id", &envelope.app_instance_id)?;
    require_non_empty("controller_did", &envelope.controller_did)?;
    if envelope.schema != DAEMON_BOOTSTRAP_SCHEMA {
        bail!("unsupported daemon bootstrap schema: {}", envelope.schema);
    }
    if envelope.controller_did != message_sender_did {
        bail!("bootstrap controller_did does not match message sender");
    }
    reject_forbidden_private_state_keys(&serde_json::to_value(&envelope.extra)?)?;
    reject_forbidden_private_state_keys(&envelope.capability_policy)?;
    reject_forbidden_private_state_keys(&envelope.desired_message_agent)?;
    reject_forbidden_private_state_keys(&envelope.sync_policy)?;
    validate_user_subkey_package(&envelope.user_subkey_package)?;
    Ok(())
}

fn validate_user_subkey_package(package: &UserSubkeyPackage) -> Result<()> {
    require_non_empty("user_did", &package.user_did)?;
    require_non_empty("verification_method", &package.verification_method)?;
    require_non_empty("public_key_multibase", &package.public_key_multibase)?;
    require_non_empty("private_key_multibase", &package.private_key_multibase)?;
    if package.schema != USER_SUBKEY_PACKAGE_SCHEMA {
        bail!("unsupported user subkey package schema: {}", package.schema);
    }
    validate_daemon_key_verification_method(&package.user_did, &package.verification_method)?;
    for scope in &package.allowed_scopes {
        let scope_lower = scope.to_ascii_lowercase();
        if scope_lower.contains("e2ee")
            || scope_lower.contains("private_state")
            || scope_lower.contains("private-state")
            || scope_lower.contains("session_private")
            || scope_lower.contains("key_package_private")
        {
            bail!("daemon bootstrap scope is not allowed for MVP: {scope}");
        }
    }
    reject_forbidden_private_state_keys(&serde_json::to_value(&package.extra)?)?;
    Ok(())
}

fn validate_daemon_key_verification_method(
    user_did: &str,
    verification_method: &str,
) -> Result<()> {
    let Some((owner, fragment)) = verification_method.rsplit_once('#') else {
        bail!("verification_method must include a DID fragment");
    };
    if owner != user_did {
        bail!("verification_method owner does not match user_did");
    }
    if fragment != MVP_DAEMON_KEY_FRAGMENT {
        bail!("verification_method must use #daemon-key-1");
    }
    Ok(())
}

fn require_non_empty(field_name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field_name} must not be empty");
    }
    Ok(())
}

fn reject_forbidden_private_state_keys(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                reject_forbidden_private_state_name(key)?;
                reject_forbidden_private_state_keys(value)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                reject_forbidden_private_state_keys(value)?;
            }
        }
        Value::String(value) => reject_forbidden_private_state_name(value)?,
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn reject_forbidden_private_state_name(value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    for forbidden in [
        "user_main_key",
        "main_private_key",
        "main-key-private",
        "private_key",
        "private-key",
        "e2ee_private",
        "e2ee.private",
        "e2ee_session",
        "e2ee.session",
        "private_state",
        "private-state",
        "key_package_private",
        "session_private",
        "ratchet_state",
        "mls_private",
        "group_private",
    ] {
        if lower.contains(forbidden) {
            bail!("daemon bootstrap must not include forbidden private state: {forbidden}");
        }
    }
    Ok(())
}

fn stable_payload_hash(envelope: &DaemonBootstrapEnvelope) -> Result<String> {
    let package = &envelope.user_subkey_package;
    let stable = json!({
        "schema": envelope.schema,
        "bootstrap_id": envelope.bootstrap_id,
        "idempotency_key": envelope.idempotency_key,
        "app_instance_id": envelope.app_instance_id,
        "controller_did": envelope.controller_did,
        "user_subkey_package": {
            "schema": package.schema,
            "user_did": package.user_did,
            "verification_method": package.verification_method,
            "key_type": package.key_type,
            "public_key_multibase": package.public_key_multibase,
            "private_key_multibase": package.private_key_multibase,
            "expires_at": package.expires_at,
            "allowed_scopes": package.allowed_scopes,
        },
        "capability_policy": envelope.capability_policy,
        "desired_message_agent": envelope.desired_message_agent,
        "sync_policy": envelope.sync_policy,
    });
    let bytes = serde_json::to_vec(&stable).context("serialize daemon bootstrap payload hash")?;
    let digest = Sha256::digest(bytes);
    Ok(hex_lower(&digest))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[allow(dead_code)]
fn _assert_secret_redaction(secret: &SecretString) -> String {
    format!("{secret:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_payload() -> Value {
        json!({
            "schema": DAEMON_BOOTSTRAP_SCHEMA,
            "bootstrap_id": "boot_1",
            "idempotency_key": "message-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1",
            "app_instance_id": "app_1",
            "controller_did": "did:wba:example.com:user:alice:e1_user",
            "user_subkey_package": {
                "schema": USER_SUBKEY_PACKAGE_SCHEMA,
                "user_did": "did:wba:example.com:user:alice:e1_user",
                "verification_method": "did:wba:example.com:user:alice:e1_user#daemon-key-1",
                "key_type": "Multikey/Ed25519",
                "public_key_multibase": "z-public",
                "private_key_multibase": "z-private-secret",
                "allowed_scopes": [
                    "message.inbox.read.plain",
                    "message.history.read.plain",
                    "message.send.plain"
                ]
            },
            "desired_message_agent": {
                "role": "app_message_handler",
                "runtime": "hermes",
                "e2ee_visible": false
            },
            "sync_policy": {
                "e2ee_default": "not_supported_in_mvp"
            }
        })
    }

    #[test]
    fn bootstrap_payload_debug_redacts_private_key() {
        let envelope = parse_bootstrap_payload(valid_payload()).unwrap();
        let debug = format!("{envelope:?}");
        assert!(!debug.contains("z-private-secret"));
        assert!(debug.contains("<redacted-private-key>"));
        assert!(debug.contains("<redacted-control-payload>"));
    }

    #[test]
    fn valid_bootstrap_payload_passes_validation_and_hashes() {
        let envelope = parse_bootstrap_payload(valid_payload()).unwrap();
        validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user").unwrap();
        let hash = stable_payload_hash(&envelope).unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn wrong_owner_verification_method_is_rejected() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["verification_method"] =
            json!("did:wba:example.com:user:bob:e1_user#daemon-key-1");
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();
        assert!(error.to_string().contains("owner"));
    }

    #[test]
    fn non_daemon_key_fragment_is_rejected() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["verification_method"] =
            json!("did:wba:example.com:user:alice:e1_user#key-1");
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();
        assert!(error.to_string().contains("#daemon-key-1"));
    }

    #[test]
    fn e2ee_private_scope_is_rejected() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["allowed_scopes"]
            .as_array_mut()
            .unwrap()
            .push(json!("e2ee.private_state.read"));
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();
        assert!(error.to_string().contains("not allowed"));
    }

    #[test]
    fn user_main_key_field_is_rejected() {
        let mut payload = valid_payload();
        payload["user_main_key"] = json!("main-secret");
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();
        assert!(error.to_string().contains("forbidden private state"));
    }

    #[test]
    fn private_key_field_in_extra_payload_is_rejected_and_redacted() {
        let mut payload = valid_payload();
        payload["unexpected_private_key"] = json!("nested-secret");
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let debug = format!("{envelope:?}");
        assert!(!debug.contains("nested-secret"));
        assert!(debug.contains("<redacted-control-payload>"));

        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();
        assert!(error.to_string().contains("forbidden private state"));
    }
}
