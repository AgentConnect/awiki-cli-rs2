use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::app_bridge::secret_store::{
    public_key_multibase_from_private_material, secret_from_private_key_multibase, SecretString,
};
use crate::state::{
    BootstrapReplayRecord, BootstrapStoreOutcome, DaemonState, UserDelegatedIdentityRecord,
};
use crate::DaemonConfig;

pub const DAEMON_BOOTSTRAP_SCHEMA: &str = "awiki.daemon.bootstrap.v1";
pub const USER_SUBKEY_PACKAGE_SCHEMA: &str = "awiki.daemon.user_subkey_package.v1";
pub const DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED: &str = "paired_key_received";
const MVP_DAEMON_KEY_FRAGMENT: &str = "daemon-key-1";
const SHADOW_IDENTITY_ALIAS_PREFIX: &str = "delegated-inbox-";

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

pub trait BootstrapDidDocumentResolver {
    fn resolve_user_did_document(&self, user_did: &str) -> Result<Value>;
}

pub struct DefaultBootstrapDidDocumentResolver<'a> {
    config: &'a DaemonConfig,
    http: reqwest::Client,
}

impl<'a> DefaultBootstrapDidDocumentResolver<'a> {
    pub fn new(config: &'a DaemonConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }
}

impl BootstrapDidDocumentResolver for DefaultBootstrapDidDocumentResolver<'_> {
    fn resolve_user_did_document(&self, user_did: &str) -> Result<Value> {
        if did_document_http_url(self.config, user_did)?.is_some() {
            return fetch_remote_did_document(self.config, self.http.clone(), user_did)
                .with_context(|| format!("resolve current DID Document for {user_did}"));
        }
        load_local_user_did_document(self.config, user_did)?.with_context(|| {
            format!("DID Document for {user_did} is not available in daemon identity cache")
        })
    }
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
    did_resolver: &impl BootstrapDidDocumentResolver,
    envelope: DaemonBootstrapEnvelope,
) -> Result<BootstrapProcessOutcome> {
    validate_bootstrap_envelope(&envelope, sender_did)?;
    let did_document = did_resolver
        .resolve_user_did_document(&envelope.user_subkey_package.user_did)
        .with_context(|| {
            format!(
                "resolve DID Document for delegated bootstrap {}",
                envelope.user_subkey_package.user_did
            )
        })?;
    validate_user_subkey_package_against_did_document(
        &envelope.user_subkey_package,
        &did_document,
        OffsetDateTime::now_utc(),
    )?;
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
    if envelope.controller_did != envelope.user_subkey_package.user_did {
        bail!("bootstrap user_did does not match controller_did");
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
    if package
        .key_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value != "Multikey/Ed25519")
    {
        bail!("daemon subkey key_type must be Multikey/Ed25519");
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

pub(crate) fn validate_user_delegated_identity_against_did_document(
    identity: &UserDelegatedIdentityRecord,
    did_document: &Value,
    now: OffsetDateTime,
) -> Result<()> {
    let package = UserSubkeyPackage {
        schema: USER_SUBKEY_PACKAGE_SCHEMA.to_string(),
        user_did: identity.user_did.clone(),
        verification_method: identity.verification_method.clone(),
        key_type: Some("Multikey/Ed25519".to_string()),
        public_key_multibase: identity.public_key_multibase.clone(),
        private_key_multibase: identity.private_key_material.clone(),
        expires_at: identity.expires_at.clone(),
        allowed_scopes: Vec::new(),
        extra: BTreeMap::new(),
    };
    validate_user_subkey_package_against_did_document(&package, did_document, now)
}

pub(crate) fn validate_user_subkey_package_against_did_document(
    package: &UserSubkeyPackage,
    did_document: &Value,
    now: OffsetDateTime,
) -> Result<()> {
    validate_user_subkey_package(package)?;
    validate_package_expiration(package, now)?;
    let derived_public = public_key_multibase_from_private_material(&package.private_key_multibase)
        .context("derive delegated private key public key")?;
    if derived_public != package.public_key_multibase {
        bail!("daemon subkey private/public key mismatch");
    }
    validate_did_document_daemon_subkey(package, did_document)?;
    Ok(())
}

fn validate_package_expiration(package: &UserSubkeyPackage, now: OffsetDateTime) -> Result<()> {
    let Some(expires_at) = package.expires_at.as_deref().map(str::trim) else {
        return Ok(());
    };
    if expires_at.is_empty() {
        return Ok(());
    }
    let expires_at = OffsetDateTime::parse(expires_at, &Rfc3339)
        .context("daemon bootstrap expires_at must be RFC3339")?;
    if expires_at <= now {
        bail!("daemon bootstrap package is expired");
    }
    Ok(())
}

fn validate_did_document_daemon_subkey(
    package: &UserSubkeyPackage,
    did_document: &Value,
) -> Result<()> {
    if did_document.get("id").and_then(Value::as_str) != Some(package.user_did.as_str()) {
        bail!("DID Document id does not match user_did");
    }
    if !authentication_contains_method(did_document, &package.verification_method) {
        bail!("DID Document authentication does not contain delegated daemon key");
    }
    let method = verification_method_entry(did_document, &package.verification_method)?;
    if method.get("controller").and_then(Value::as_str) != Some(package.user_did.as_str()) {
        bail!("DID Document delegated daemon key controller does not match user_did");
    }
    let document_public = method
        .get("publicKeyMultibase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("DID Document delegated daemon key missing publicKeyMultibase")?;
    if document_public != package.public_key_multibase {
        bail!("DID Document delegated daemon key public key mismatch");
    }
    Ok(())
}

fn authentication_contains_method(did_document: &Value, verification_method: &str) -> bool {
    did_document
        .get("authentication")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str()
                    .is_some_and(|candidate| candidate.trim() == verification_method)
                    || item
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|candidate| candidate.trim() == verification_method)
            })
        })
}

fn verification_method_entry<'a>(
    did_document: &'a Value,
    verification_method: &str,
) -> Result<&'a Value> {
    let methods = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .context("DID Document verificationMethod must be an array")?;
    methods
        .iter()
        .find(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate.trim() == verification_method)
        })
        .context("DID Document delegated daemon key verification method is missing")
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

fn fetch_remote_did_document(
    config: &DaemonConfig,
    http: reqwest::Client,
    user_did: &str,
) -> Result<Value> {
    let url = did_document_http_url(config, user_did)?
        .context("DID method does not support HTTP DID Document resolution")?;
    if tokio::runtime::Handle::try_current().is_ok() {
        let join = std::thread::Builder::new()
            .name("awiki-bootstrap-did-resolve".to_string())
            .spawn(move || fetch_remote_did_document_in_new_runtime(http, url))
            .context("spawn DID Document resolve runtime thread")?;
        return join
            .join()
            .map_err(|_| anyhow::anyhow!("DID Document resolve runtime thread panicked"))?;
    }
    fetch_remote_did_document_in_new_runtime(http, url)
}

fn fetch_remote_did_document_in_new_runtime(http: reqwest::Client, url: String) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create DID Document resolve runtime")?;
    runtime.block_on(fetch_remote_did_document_async(http, url))
}

async fn fetch_remote_did_document_async(http: reqwest::Client, url: String) -> Result<Value> {
    let response = http
        .get(&url)
        .header("accept", "application/did+json, application/json")
        .send()
        .await
        .with_context(|| format!("GET DID Document {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("DID Document HTTP resolve failed with status {status}");
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read DID Document response {url}"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse DID Document response {url}"))
}

fn did_document_http_url(config: &DaemonConfig, did: &str) -> Result<Option<String>> {
    let did = did.trim();
    let Some(rest) = did.strip_prefix("did:") else {
        return Ok(None);
    };
    let Some((method, suffix)) = rest.split_once(':') else {
        bail!("invalid DID format");
    };
    if !matches!(method, "wba" | "web") {
        return Ok(None);
    }
    let mut parts = suffix.split(':');
    let domain = parts
        .next()
        .map(percent_decode_lossy)
        .filter(|value| !value.is_empty())
        .context("invalid DID format")?;
    let path_segments = parts.map(percent_decode_lossy).collect::<Vec<_>>();
    let path = if path_segments.is_empty() {
        "/.well-known/did.json".to_string()
    } else {
        format!("/{}/did.json", path_segments.join("/"))
    };
    if did_domain_uses_configured_service(config, &domain) {
        Ok(Some(join_base_url(&config.user_service_base_url, &path)))
    } else {
        Ok(Some(format!("https://{domain}{path}")))
    }
}

fn did_domain_uses_configured_service(config: &DaemonConfig, did_domain: &str) -> bool {
    let did_domain = did_domain.trim();
    did_domain.eq_ignore_ascii_case(config.did_domain.trim())
        || did_domain.eq_ignore_ascii_case(&host_from_base_url(&config.user_service_base_url))
        || did_domain.eq_ignore_ascii_case(&host_from_base_url(&config.service_base_url))
}

fn join_base_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let path = path.trim();
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn host_from_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let authority = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    authority.split(':').next().unwrap_or_default().to_string()
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Some(byte) = hex_pair(bytes[index + 1], bytes[index + 2]) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_value(high)? * 16 + hex_value(low)?)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn load_local_user_did_document(config: &DaemonConfig, user_did: &str) -> Result<Option<Value>> {
    let user_did = user_did.trim();
    if user_did.is_empty() || !user_did.starts_with("did:") {
        return Ok(None);
    }
    let raw = match fs::read(&config.identity_registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "read daemon identity registry {}",
                    config.identity_registry_path.display()
                )
            });
        }
    };
    let registry: Value = serde_json::from_slice(&raw).with_context(|| {
        format!(
            "parse daemon identity registry {}",
            config.identity_registry_path.display()
        )
    })?;
    let Some(dir_name) = sdk_identity_dir_name(&registry, user_did)
        .or_else(|| legacy_identity_dir_name(&registry, user_did))
    else {
        return Ok(None);
    };
    read_local_did_document(&config.identity_root_dir.join(dir_name), user_did)
}

fn sdk_identity_dir_name(registry: &Value, did: &str) -> Option<String> {
    registry
        .get("identities")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|entry| {
            let candidate = entry.get("did").and_then(Value::as_str)?.trim();
            if candidate != did || is_shadow_identity_entry(entry) {
                return None;
            }
            first_nonempty([
                string_field(entry, "dir_name"),
                string_field(entry, "local_alias"),
                string_field(entry, "id"),
            ])
        })
}

fn legacy_identity_dir_name(registry: &Value, did: &str) -> Option<String> {
    registry
        .get("credentials")
        .and_then(Value::as_object)?
        .iter()
        .find_map(|(alias, entry)| {
            let candidate = entry.get("did").and_then(Value::as_str)?.trim();
            if candidate != did
                || is_shadow_identity_entry(entry)
                || alias.starts_with(SHADOW_IDENTITY_ALIAS_PREFIX)
            {
                return None;
            }
            first_nonempty([
                string_field(entry, "dir_name"),
                string_field(entry, "unique_id"),
                string_field(entry, "credential_name"),
                Some(alias.as_str()),
            ])
        })
}

fn is_shadow_identity_entry(entry: &Value) -> bool {
    [
        "dir_name",
        "local_alias",
        "id",
        "credential_name",
        "unique_id",
    ]
    .into_iter()
    .filter_map(|field| string_field(entry, field))
    .any(|value| value.starts_with(SHADOW_IDENTITY_ALIAS_PREFIX))
}

fn read_local_did_document(identity_dir: &Path, did: &str) -> Result<Option<Value>> {
    for path in did_document_paths(identity_dir) {
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read DID Document {}", path.display()))
            }
        };
        let document: Value = serde_json::from_slice(&raw)
            .with_context(|| format!("parse DID Document {}", path.display()))?;
        if document.get("id").and_then(Value::as_str) == Some(did)
            && document.get("verificationMethod").is_some()
        {
            return Ok(Some(document));
        }
    }
    Ok(None)
}

fn did_document_paths(identity_dir: &Path) -> [PathBuf; 2] {
    [
        identity_dir.join("did.json"),
        identity_dir.join("did_document.json"),
    ]
}

fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn first_nonempty<const N: usize>(values: [Option<&str>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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
            "private_key_present": !package.private_key_multibase.trim().is_empty(),
            "expires_at": package.expires_at,
            "allowed_scopes": package.allowed_scopes,
        },
        "capability_policy": envelope.capability_policy,
        "desired_message_agent": sanitized_desired_message_agent_for_hash(&envelope.desired_message_agent),
        "sync_policy": envelope.sync_policy,
    });
    let bytes = serde_json::to_vec(&stable).context("serialize daemon bootstrap payload hash")?;
    let digest = Sha256::digest(bytes);
    Ok(hex_lower(&digest))
}

fn sanitized_desired_message_agent_for_hash(value: &Value) -> Value {
    let mut sanitized = value.clone();
    if let Some(object) = sanitized.as_object_mut() {
        object.remove("runtime_registration_token");
        object.remove("registration_token");
        object.remove("token");
    }
    sanitized
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

    struct StaticDidResolver {
        document: Value,
    }

    impl BootstrapDidDocumentResolver for StaticDidResolver {
        fn resolve_user_did_document(&self, _user_did: &str) -> Result<Value> {
            Ok(self.document.clone())
        }
    }

    struct FailingDidResolver;

    impl BootstrapDidDocumentResolver for FailingDidResolver {
        fn resolve_user_did_document(&self, user_did: &str) -> Result<Value> {
            bail!("DID Document resolve failed for {user_did}")
        }
    }

    fn delegated_key_fixture() -> (String, String) {
        let mut key_bytes = [0_u8; 32];
        key_bytes[0] = 9;
        let private_key =
            crate::app_bridge::secret_store::ed25519_private_key_pem_for_test(&key_bytes);
        let public_key =
            crate::app_bridge::secret_store::public_key_multibase_from_private_material(
                &private_key,
            )
            .unwrap();
        (public_key, private_key)
    }

    fn did_document_for_payload(payload: &Value) -> Value {
        let package = &payload["user_subkey_package"];
        json!({
            "id": package["user_did"].as_str().unwrap(),
            "verificationMethod": [{
                "id": package["verification_method"].as_str().unwrap(),
                "type": "Multikey",
                "controller": package["user_did"].as_str().unwrap(),
                "publicKeyMultibase": package["public_key_multibase"].as_str().unwrap()
            }],
            "authentication": [package["verification_method"].as_str().unwrap()]
        })
    }

    fn config_for_test(root: &std::path::Path) -> DaemonConfig {
        let mut config = DaemonConfig::for_state_root(root).unwrap();
        config.user_service_base_url = "https://user-service.test".to_string();
        config.service_base_url = "https://service.test".to_string();
        config.did_domain = "example.com".to_string();
        config
    }

    fn valid_payload() -> Value {
        let (public_key, private_key) = delegated_key_fixture();
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
                "public_key_multibase": public_key,
                "private_key_multibase": private_key,
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
        assert!(!debug.contains("BEGIN PRIVATE KEY"));
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
    fn package_user_did_must_match_bootstrap_controller() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["user_did"] = json!("did:wba:example.com:user:bob:e1_user");
        payload["user_subkey_package"]["verification_method"] =
            json!("did:wba:example.com:user:bob:e1_user#daemon-key-1");
        let envelope = parse_bootstrap_payload(payload).unwrap();

        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();

        assert!(error.to_string().contains("user_did"));
    }

    #[test]
    fn valid_bootstrap_payload_passes_crypto_and_did_document_validation() {
        let payload = valid_payload();
        let did_document = did_document_for_payload(&payload);
        let envelope = parse_bootstrap_payload(payload).unwrap();

        validate_user_subkey_package_against_did_document(
            &envelope.user_subkey_package,
            &did_document,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    }

    #[test]
    fn bootstrap_payload_hash_ignores_runtime_registration_token() {
        let mut with_token = valid_payload();
        with_token["desired_message_agent"]["runtime_registration_token"] =
            json!("tok_runtime_secret_value");
        let without_token = valid_payload();

        let with_token = parse_bootstrap_payload(with_token).unwrap();
        let without_token = parse_bootstrap_payload(without_token).unwrap();

        assert_eq!(
            stable_payload_hash(&with_token).unwrap(),
            stable_payload_hash(&without_token).unwrap()
        );
    }

    #[test]
    fn bootstrap_payload_hash_does_not_fingerprint_private_key_material() {
        let mut first = valid_payload();
        let mut second = first.clone();
        first["user_subkey_package"]["private_key_multibase"] =
            json!("-----BEGIN PRIVATE KEY-----\nfirst\n-----END PRIVATE KEY-----");
        second["user_subkey_package"]["private_key_multibase"] =
            json!("-----BEGIN PRIVATE KEY-----\nsecond\n-----END PRIVATE KEY-----");

        let first = parse_bootstrap_payload(first).unwrap();
        let second = parse_bootstrap_payload(second).unwrap();

        assert_eq!(
            stable_payload_hash(&first).unwrap(),
            stable_payload_hash(&second).unwrap()
        );
    }

    #[test]
    fn did_wba_url_uses_configured_user_service_for_current_domain() {
        let root = tempfile::tempdir().unwrap();
        let config = config_for_test(root.path());

        let url = did_document_http_url(&config, "did:wba:example.com:user:alice:e1_user")
            .unwrap()
            .unwrap();

        assert_eq!(url, "https://user-service.test/user/alice/e1_user/did.json");
    }

    #[test]
    fn local_resolver_ignores_delegated_shadow_identity_cache() {
        let root = tempfile::tempdir().unwrap();
        let config = config_for_test(root.path());
        config.ensure_state_layout().unwrap();
        let did = "did:human:alice";
        let alias = "delegated-inbox-shadow";
        let identity_dir = config.identity_root_dir.join(alias);
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&json!({
                "id": did,
                "verificationMethod": [{"id": "did:human:alice#daemon-key-1"}],
                "authentication": ["did:human:alice#daemon-key-1"]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &config.identity_registry_path,
            serde_json::to_vec_pretty(&json!({
                "default_identity": "",
                "identities": [{
                    "id": alias,
                    "did": did,
                    "dir_name": alias,
                    "local_alias": alias
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(load_local_user_did_document(&config, did)
            .unwrap()
            .is_none());
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

    #[test]
    fn private_public_key_mismatch_is_rejected() {
        let payload = valid_payload();
        let did_document = did_document_for_payload(&payload);
        let mut envelope = parse_bootstrap_payload(payload).unwrap();
        envelope.user_subkey_package.public_key_multibase = "zBadPublicKey".to_string();

        let error = validate_user_subkey_package_against_did_document(
            &envelope.user_subkey_package,
            &did_document,
            OffsetDateTime::now_utc(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("private/public"));
    }

    #[test]
    fn missing_did_authentication_method_is_rejected() {
        let payload = valid_payload();
        let mut did_document = did_document_for_payload(&payload);
        did_document["authentication"] = json!([]);
        let envelope = parse_bootstrap_payload(payload).unwrap();

        let error = validate_user_subkey_package_against_did_document(
            &envelope.user_subkey_package,
            &did_document,
            OffsetDateTime::now_utc(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("authentication"));
    }

    #[test]
    fn did_document_public_key_mismatch_is_rejected() {
        let payload = valid_payload();
        let mut did_document = did_document_for_payload(&payload);
        did_document["verificationMethod"][0]["publicKeyMultibase"] = json!("zOtherPublicKey");
        let envelope = parse_bootstrap_payload(payload).unwrap();

        let error = validate_user_subkey_package_against_did_document(
            &envelope.user_subkey_package,
            &did_document,
            OffsetDateTime::now_utc(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("public key mismatch"));
    }

    #[test]
    fn expired_package_is_rejected() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["expires_at"] = json!("2000-01-01T00:00:00Z");
        let did_document = did_document_for_payload(&payload);
        let envelope = parse_bootstrap_payload(payload).unwrap();

        let error = validate_user_subkey_package_against_did_document(
            &envelope.user_subkey_package,
            &did_document,
            OffsetDateTime::now_utc(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("expired"));
    }

    #[test]
    fn invalid_did_document_is_rejected_before_state_is_written() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let payload = valid_payload();
        let verification_method = payload["user_subkey_package"]["verification_method"]
            .as_str()
            .unwrap()
            .to_string();
        let mut did_document = did_document_for_payload(&payload);
        did_document["authentication"] = json!([]);
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let resolver = StaticDidResolver {
            document: did_document,
        };

        let error = process_bootstrap_envelope(
            &state,
            "did:agent:daemon",
            "did:wba:example.com:user:alice:e1_user",
            &resolver,
            envelope,
        )
        .unwrap_err();

        assert!(error.to_string().contains("authentication"));
        assert!(state
            .load_user_delegated_identity(&verification_method)
            .unwrap()
            .is_none());
        assert!(state.load_bootstrap_replay("boot_1").unwrap().is_none());
    }

    #[test]
    fn did_resolve_failure_is_rejected_before_state_is_written() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let payload = valid_payload();
        let verification_method = payload["user_subkey_package"]["verification_method"]
            .as_str()
            .unwrap()
            .to_string();
        let envelope = parse_bootstrap_payload(payload).unwrap();

        let error = process_bootstrap_envelope(
            &state,
            "did:agent:daemon",
            "did:wba:example.com:user:alice:e1_user",
            &FailingDidResolver,
            envelope,
        )
        .unwrap_err();

        assert!(error.to_string().contains("resolve DID Document"));
        assert!(state
            .load_user_delegated_identity(&verification_method)
            .unwrap()
            .is_none());
        assert!(state.load_bootstrap_replay("boot_1").unwrap().is_none());
    }
}
