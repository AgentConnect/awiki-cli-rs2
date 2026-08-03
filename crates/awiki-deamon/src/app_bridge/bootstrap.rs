use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use x25519_dalek::PublicKey as X25519PublicKey;
#[cfg(any(test, feature = "system-test-probe"))]
use zeroize::Zeroizing;

use crate::app_bridge::secret_store::{
    public_key_multibase_from_private_material, secret_from_private_key_multibase, SecretString,
};
use crate::state::{
    BootstrapReplayRecord, BootstrapStoreOutcome, DaemonState, SecureBootstrapReplayRecord,
    UserDelegatedIdentityRecord,
};
use crate::DaemonConfig;

pub const DAEMON_BOOTSTRAP_SCHEMA: &str = "awiki.daemon.bootstrap.v1";
pub const DAEMON_BOOTSTRAP_SECURE_SCHEMA: &str = "awiki.daemon.bootstrap.secure.v1";
pub const USER_SUBKEY_PACKAGE_SCHEMA: &str = "awiki.daemon.user_subkey_package.v1";
pub const USER_SUBKEY_PACKAGE_SCHEMA_V2: &str = "awiki.daemon.user_subkey_package.v2";
pub const DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED: &str = "paired_key_received";
const MVP_DAEMON_KEY_FRAGMENT: &str = "daemon-key-1";
const DAEMON_BOOTSTRAP_KEY_FRAGMENT: &str = "key-3";
const SHADOW_IDENTITY_ALIAS_PREFIX: &str = "delegated-inbox-";
const PRIVATE_KEY_ENCODING_PEM: &str = "pem";
const SECURE_BOOTSTRAP_MAX_CLOCK_SKEW: Duration = Duration::minutes(5);

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
    #[serde(default, alias = "desired_message_agent")]
    pub desired_personal_agent: Value,
    #[serde(default)]
    pub sync_policy: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonSecureBootstrapEnvelope {
    pub schema: String,
    pub recipient_daemon_did: String,
    pub recipient_key_id: String,
    pub sender_human_did: String,
    pub operation_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub nonce: String,
    pub sender_ephemeral_public_key: String,
    pub ciphertext: String,
    #[serde(default)]
    pub aad: Value,
    #[serde(default)]
    pub payload_sha256: Option<String>,
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
    #[serde(default)]
    pub key_algorithm: Option<String>,
    pub public_key_multibase: String,
    #[serde(default)]
    pub private_key_encoding: Option<String>,
    #[serde(default)]
    pub private_key_pem: Option<String>,
    #[serde(default)]
    pub private_key_multibase: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub allowed_scopes: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl UserSubkeyPackage {
    fn private_key_material(&self) -> &str {
        self.private_key_pem
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.private_key_multibase.trim())
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureBootstrapProcessOutcome {
    pub bootstrap: BootstrapProcessOutcome,
    pub desired_personal_agent: Value,
    pub capability_policy: Value,
    pub secure_replayed: bool,
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
            .field("desired_personal_agent", &"<redacted-control-payload>")
            .field("sync_policy", &"<redacted-control-payload>")
            .field("extra", &"<redacted-control-payload>")
            .finish()
    }
}

impl std::fmt::Debug for DaemonSecureBootstrapEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonSecureBootstrapEnvelope")
            .field("schema", &self.schema)
            .field("recipient_daemon_did", &self.recipient_daemon_did)
            .field("recipient_key_id", &self.recipient_key_id)
            .field("sender_human_did", &self.sender_human_did)
            .field("operation_id", &self.operation_id)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("nonce", &self.nonce)
            .field(
                "sender_ephemeral_public_key",
                &"<redacted-ephemeral-public-key>",
            )
            .field("ciphertext", &"<redacted-bootstrap-ciphertext>")
            .field("aad", &"<redacted-bootstrap-aad>")
            .field("payload_sha256", &self.payload_sha256)
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
            .field("key_algorithm", &self.key_algorithm)
            .field("public_key_multibase", &self.public_key_multibase)
            .field("private_key_encoding", &self.private_key_encoding)
            .field("private_key_pem", &"<redacted-private-key>")
            .field("private_key_multibase", &"<redacted-legacy-private-key>")
            .field("expires_at", &self.expires_at)
            .field("allowed_scopes", &self.allowed_scopes)
            .field("extra", &"<redacted-control-payload>")
            .finish()
    }
}

pub fn parse_bootstrap_payload(payload: Value) -> Result<DaemonBootstrapEnvelope> {
    let mut envelope: DaemonBootstrapEnvelope =
        serde_json::from_value(payload).context("parse daemon bootstrap payload")?;
    envelope.desired_personal_agent =
        canonicalize_desired_personal_agent_names(&envelope.desired_personal_agent);
    Ok(envelope)
}

pub fn parse_secure_bootstrap_payload(payload: Value) -> Result<DaemonSecureBootstrapEnvelope> {
    let envelope: DaemonSecureBootstrapEnvelope =
        serde_json::from_value(payload).context("parse secure daemon bootstrap payload")?;
    validate_secure_bootstrap_envelope(&envelope)?;
    Ok(envelope)
}

pub fn is_daemon_bootstrap_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some(DAEMON_BOOTSTRAP_SCHEMA)
}

pub fn is_daemon_secure_bootstrap_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some(DAEMON_BOOTSTRAP_SECURE_SCHEMA)
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
    let legacy_payload_hash = legacy_stable_payload_hash(&envelope)?;
    let package = &envelope.user_subkey_package;
    let private_key = secret_from_private_key_multibase(package.private_key_material());
    let identity = UserDelegatedIdentityRecord {
        user_did: package.user_did.clone(),
        verification_method: package.verification_method.clone(),
        app_instance_id: envelope.app_instance_id.clone(),
        controller_did: envelope.controller_did.clone(),
        daemon_agent_did: daemon_agent_did.to_string(),
        public_key_multibase: package.public_key_multibase.clone(),
        private_key_material: private_key.expose_secret().to_string(),
        private_key_ref_json: None,
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
    let outcome = state.store_bootstrap_state_with_legacy_payload_hash(
        &identity,
        &replay,
        &legacy_payload_hash,
    )?;
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
            "run_id": envelope.extra.get("run_id").and_then(Value::as_str),
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

pub fn process_secure_bootstrap_envelope(
    state: &DaemonState,
    daemon_agent_did: &str,
    sender_did: &str,
    did_resolver: &impl BootstrapDidDocumentResolver,
    secure_envelope: DaemonSecureBootstrapEnvelope,
) -> Result<SecureBootstrapProcessOutcome> {
    validate_secure_bootstrap_envelope_for_recipient(
        &secure_envelope,
        daemon_agent_did,
        sender_did,
        OffsetDateTime::now_utc(),
    )?;
    let envelope_hash = stable_secure_envelope_hash(&secure_envelope)?;
    let decrypted = decrypt_secure_bootstrap_envelope(state, daemon_agent_did, &secure_envelope)?;
    if let Some(payload_sha256) = secure_envelope.payload_sha256.as_deref() {
        let actual = hex_lower(&Sha256::digest(&decrypted.plaintext_bytes));
        if !payload_sha256.eq_ignore_ascii_case(&actual) {
            bail!("secure daemon bootstrap payload_sha256 mismatch");
        }
    }
    let envelope = parse_bootstrap_payload(decrypted.payload)?;
    if secure_envelope.operation_id != envelope.idempotency_key {
        bail!("secure daemon bootstrap operation_id must match internal idempotency_key");
    }
    if secure_envelope.sender_human_did != envelope.controller_did {
        bail!("secure daemon bootstrap sender_human_did must match internal controller_did");
    }
    let secure_replay = SecureBootstrapReplayRecord {
        operation_id: secure_envelope.operation_id.clone(),
        nonce: secure_envelope.nonce.clone(),
        envelope_hash,
        recipient_daemon_did: secure_envelope.recipient_daemon_did.clone(),
        recipient_key_id: secure_envelope.recipient_key_id.clone(),
        sender_human_did: secure_envelope.sender_human_did.clone(),
        bootstrap_id: envelope.bootstrap_id.clone(),
        idempotency_key: envelope.idempotency_key.clone(),
        payload_sha256: secure_envelope.payload_sha256.clone(),
        expires_at: secure_envelope.expires_at.clone(),
        status: DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED.to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let secure_replayed = matches!(
        state.store_secure_bootstrap_replay(&secure_replay)?,
        BootstrapStoreOutcome::Duplicate
    );
    let desired_personal_agent = envelope.desired_personal_agent.clone();
    let capability_policy = envelope.capability_policy.clone();
    let bootstrap =
        process_bootstrap_envelope(state, daemon_agent_did, sender_did, did_resolver, envelope)?;
    state.insert_audit_event_json(
        "daemon.bootstrap.secure.received",
        Some(daemon_agent_did),
        None,
        None,
        None,
        json!({
            "operation_id": secure_replay.operation_id,
            "nonce": secure_replay.nonce,
            "recipient_key_id": secure_replay.recipient_key_id,
            "sender_human_did": secure_replay.sender_human_did,
            "bootstrap_id": secure_replay.bootstrap_id,
            "idempotency_key": secure_replay.idempotency_key,
            "payload_sha256": secure_replay.payload_sha256,
            "replayed": secure_replayed,
        }),
    )?;
    Ok(SecureBootstrapProcessOutcome {
        bootstrap,
        desired_personal_agent,
        capability_policy,
        secure_replayed,
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
    reject_forbidden_private_state_keys(&envelope.desired_personal_agent)?;
    reject_forbidden_private_state_keys(&envelope.sync_policy)?;
    validate_user_subkey_package(&envelope.user_subkey_package)?;
    Ok(())
}

fn validate_secure_bootstrap_envelope(envelope: &DaemonSecureBootstrapEnvelope) -> Result<()> {
    require_non_empty("recipient_daemon_did", &envelope.recipient_daemon_did)?;
    require_non_empty("recipient_key_id", &envelope.recipient_key_id)?;
    require_non_empty("sender_human_did", &envelope.sender_human_did)?;
    require_non_empty("operation_id", &envelope.operation_id)?;
    require_non_empty("issued_at", &envelope.issued_at)?;
    require_non_empty("expires_at", &envelope.expires_at)?;
    require_non_empty("nonce", &envelope.nonce)?;
    require_non_empty(
        "sender_ephemeral_public_key",
        &envelope.sender_ephemeral_public_key,
    )?;
    require_non_empty("ciphertext", &envelope.ciphertext)?;
    if envelope.schema != DAEMON_BOOTSTRAP_SECURE_SCHEMA {
        bail!(
            "unsupported secure daemon bootstrap schema: {}",
            envelope.schema
        );
    }
    let issued_at = OffsetDateTime::parse(&envelope.issued_at, &Rfc3339)
        .context("secure daemon bootstrap issued_at must be RFC3339")?;
    let expires_at = OffsetDateTime::parse(&envelope.expires_at, &Rfc3339)
        .context("secure daemon bootstrap expires_at must be RFC3339")?;
    if expires_at <= issued_at {
        bail!("secure daemon bootstrap expires_at must be after issued_at");
    }
    reject_forbidden_private_state_keys(&envelope.aad)?;
    reject_forbidden_private_state_keys(&serde_json::to_value(&envelope.extra)?)?;
    decode_base64url_fixed::<32>(
        "sender_ephemeral_public_key",
        &envelope.sender_ephemeral_public_key,
        32,
    )?;
    decode_base64url_fixed::<12>("nonce", &envelope.nonce, 12)?;
    decode_base64url_bytes("ciphertext", &envelope.ciphertext)?;
    if let Some(payload_sha256) = envelope.payload_sha256.as_deref() {
        require_non_empty("payload_sha256", payload_sha256)?;
        if payload_sha256.len() != 64 || !payload_sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            bail!("secure daemon bootstrap payload_sha256 must be a 64-character hex digest");
        }
    }
    Ok(())
}

fn validate_secure_bootstrap_envelope_for_recipient(
    envelope: &DaemonSecureBootstrapEnvelope,
    daemon_agent_did: &str,
    sender_did: &str,
    now: OffsetDateTime,
) -> Result<()> {
    validate_secure_bootstrap_envelope(envelope)?;
    if envelope.recipient_daemon_did.trim() != daemon_agent_did.trim() {
        bail!("secure daemon bootstrap recipient_daemon_did does not match target daemon");
    }
    if envelope.sender_human_did.trim() != sender_did.trim() {
        bail!("secure daemon bootstrap sender_human_did does not match message sender");
    }
    let expected_key_id = daemon_bootstrap_key_id(daemon_agent_did);
    if envelope.recipient_key_id.trim() != expected_key_id {
        bail!("secure daemon bootstrap recipient_key_id does not match daemon bootstrap key");
    }
    let issued_at = OffsetDateTime::parse(&envelope.issued_at, &Rfc3339)?;
    let expires_at = OffsetDateTime::parse(&envelope.expires_at, &Rfc3339)?;
    if expires_at <= now {
        bail!("secure daemon bootstrap envelope is expired");
    }
    if issued_at > now + SECURE_BOOTSTRAP_MAX_CLOCK_SKEW {
        bail!("secure daemon bootstrap issued_at is too far in the future");
    }
    Ok(())
}

struct DecryptedSecureBootstrap {
    payload: Value,
    plaintext_bytes: Vec<u8>,
}

fn decrypt_secure_bootstrap_envelope(
    state: &DaemonState,
    daemon_agent_did: &str,
    envelope: &DaemonSecureBootstrapEnvelope,
) -> Result<DecryptedSecureBootstrap> {
    let agreement_private_key_pem = match state.load_agent_device_identity(daemon_agent_did)? {
        Some(identity) => identity.device_e2ee_private_key_pem,
        None => {
            state
                .load_agent_identity(daemon_agent_did)
                .context("load legacy daemon identity for secure bootstrap decrypt")?
                .e2ee_agreement_private_key_pem
        }
    };
    let recipient_private = anp::PrivateKeyMaterial::from_pem(&agreement_private_key_pem)
        .context("parse daemon bootstrap agreement private key")?;
    let recipient_private = match recipient_private {
        anp::PrivateKeyMaterial::X25519(key) => key,
        _ => bail!("daemon bootstrap agreement key must be X25519"),
    };
    let sender_ephemeral_public = decode_base64url_fixed::<32>(
        "sender_ephemeral_public_key",
        &envelope.sender_ephemeral_public_key,
        32,
    )?;
    let shared = recipient_private.diffie_hellman(&X25519PublicKey::from(sender_ephemeral_public));
    let key_bytes = derive_secure_bootstrap_key(shared.as_bytes(), envelope)?;
    let nonce = decode_base64url_fixed::<12>("nonce", &envelope.nonce, 12)?;
    let ciphertext = decode_base64url_bytes("ciphertext", &envelope.ciphertext)?;
    let aad = stable_secure_bootstrap_aad(envelope)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("secure daemon bootstrap decrypt failed"))?;
    let payload = serde_json::from_slice(&plaintext).context("parse decrypted daemon bootstrap")?;
    Ok(DecryptedSecureBootstrap {
        payload,
        plaintext_bytes: plaintext,
    })
}

fn derive_secure_bootstrap_key(
    shared_secret: &[u8],
    envelope: &DaemonSecureBootstrapEnvelope,
) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(b"AWIKI daemon bootstrap secure v1");
    hasher.update(shared_secret);
    hasher.update(envelope.recipient_daemon_did.as_bytes());
    hasher.update(envelope.recipient_key_id.as_bytes());
    hasher.update(envelope.sender_human_did.as_bytes());
    hasher.update(envelope.operation_id.as_bytes());
    hasher.update(envelope.nonce.as_bytes());
    let digest = hasher.finalize();
    digest
        .as_slice()
        .try_into()
        .context("secure daemon bootstrap key length")
}

fn stable_secure_bootstrap_aad(envelope: &DaemonSecureBootstrapEnvelope) -> Result<Vec<u8>> {
    let value = stable_secure_bootstrap_aad_value(envelope);
    canonical_json_bytes(&value).context("serialize secure daemon bootstrap aad")
}

fn stable_secure_bootstrap_aad_value(envelope: &DaemonSecureBootstrapEnvelope) -> Value {
    let mut value = Map::new();
    value.insert(
        "schema".to_string(),
        Value::String(envelope.schema.trim().to_string()),
    );
    value.insert(
        "recipient_daemon_did".to_string(),
        Value::String(envelope.recipient_daemon_did.trim().to_string()),
    );
    value.insert(
        "recipient_key_id".to_string(),
        Value::String(envelope.recipient_key_id.trim().to_string()),
    );
    value.insert(
        "sender_human_did".to_string(),
        Value::String(envelope.sender_human_did.trim().to_string()),
    );
    value.insert(
        "operation_id".to_string(),
        Value::String(envelope.operation_id.trim().to_string()),
    );
    value.insert(
        "issued_at".to_string(),
        Value::String(envelope.issued_at.trim().to_string()),
    );
    value.insert(
        "expires_at".to_string(),
        Value::String(envelope.expires_at.trim().to_string()),
    );
    value.insert(
        "nonce".to_string(),
        Value::String(envelope.nonce.trim().to_string()),
    );
    value.insert(
        "sender_ephemeral_public_key".to_string(),
        Value::String(envelope.sender_ephemeral_public_key.trim().to_string()),
    );
    value.insert("aad".to_string(), envelope.aad.clone());
    if !envelope.extra.is_empty() {
        value.insert("extra".to_string(), json!(envelope.extra));
    }
    if let Some(payload_sha256) = envelope.payload_sha256.as_deref() {
        value.insert(
            "payload_sha256".to_string(),
            Value::String(payload_sha256.trim().to_ascii_lowercase()),
        );
    }
    Value::Object(value)
}

fn stable_secure_envelope_hash(envelope: &DaemonSecureBootstrapEnvelope) -> Result<String> {
    let value = json!({
        "aad": stable_secure_bootstrap_aad_value(envelope),
        "ciphertext_sha256": hex_lower(&Sha256::digest(envelope.ciphertext.trim().as_bytes())),
    });
    let bytes = canonical_json_bytes(&value).context("serialize secure daemon bootstrap hash")?;
    Ok(hex_lower(&Sha256::digest(bytes)))
}

fn validate_user_subkey_package(package: &UserSubkeyPackage) -> Result<()> {
    require_non_empty("user_did", &package.user_did)?;
    require_non_empty("verification_method", &package.verification_method)?;
    require_non_empty("public_key_multibase", &package.public_key_multibase)?;
    if !matches!(
        package.schema.as_str(),
        USER_SUBKEY_PACKAGE_SCHEMA | USER_SUBKEY_PACKAGE_SCHEMA_V2
    ) {
        bail!("unsupported user subkey package schema: {}", package.schema);
    }
    if package.schema == USER_SUBKEY_PACKAGE_SCHEMA_V2 {
        if package
            .private_key_encoding
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            != Some(PRIVATE_KEY_ENCODING_PEM)
        {
            bail!("daemon subkey v2 private_key_encoding must be pem");
        }
        require_non_empty("private_key_pem", package.private_key_material())?;
    } else {
        require_non_empty("private_key_multibase", package.private_key_material())?;
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
    if package
        .key_algorithm
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value != "Ed25519")
    {
        bail!("daemon subkey key_algorithm must be Ed25519");
    }
    validate_daemon_key_verification_method(&package.user_did, &package.verification_method)?;
    for scope in &package.allowed_scopes {
        let scope_lower = scope.to_ascii_lowercase();
        if scope_lower == "message.send.plain"
            || scope_lower.contains("message.send")
            || scope_lower.contains("e2ee")
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
        key_algorithm: Some("Ed25519".to_string()),
        public_key_multibase: identity.public_key_multibase.clone(),
        private_key_encoding: Some(PRIVATE_KEY_ENCODING_PEM.to_string()),
        private_key_pem: Some(identity.private_key_material.clone()),
        private_key_multibase: String::new(),
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
    let derived_public = public_key_multibase_from_private_material(package.private_key_material())
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
    stable_payload_hash_with_agent_key(envelope, "desired_personal_agent")
}

fn legacy_stable_payload_hash(envelope: &DaemonBootstrapEnvelope) -> Result<String> {
    stable_payload_hash_with_agent_key(envelope, "desired_message_agent")
}

fn stable_payload_hash_with_agent_key(
    envelope: &DaemonBootstrapEnvelope,
    agent_key: &str,
) -> Result<String> {
    let package = &envelope.user_subkey_package;
    let desired_agent = if agent_key == "desired_personal_agent" {
        sanitized_desired_personal_agent_for_hash(&envelope.desired_personal_agent)
    } else {
        sanitized_legacy_message_agent_for_hash(&envelope.desired_personal_agent)
    };
    let mut stable = json!({
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
            "key_algorithm": package.key_algorithm,
            "public_key_multibase": package.public_key_multibase,
            "private_key_encoding": package.private_key_encoding,
            "private_key_present": !package.private_key_material().trim().is_empty(),
            "expires_at": package.expires_at,
            "allowed_scopes": package.allowed_scopes,
        },
        "capability_policy": envelope.capability_policy,
        "desired_personal_agent": desired_agent,
        "sync_policy": envelope.sync_policy,
    });
    if agent_key != "desired_personal_agent" {
        let object = stable
            .as_object_mut()
            .context("daemon bootstrap stable payload must be an object")?;
        let desired = object
            .remove("desired_personal_agent")
            .context("daemon bootstrap stable payload is missing desired personal agent")?;
        object.insert(agent_key.to_string(), desired);
    }
    let bytes = serde_json::to_vec(&stable).context("serialize daemon bootstrap payload hash")?;
    let digest = Sha256::digest(bytes);
    Ok(hex_lower(&digest))
}

fn sanitized_desired_personal_agent_for_hash(value: &Value) -> Value {
    let mut sanitized = canonicalize_desired_personal_agent_names(value);
    if let Some(object) = sanitized.as_object_mut() {
        object.remove("runtime_registration_token");
        object.remove("registration_token");
        object.remove("token");
    }
    sanitized
}

fn canonicalize_desired_personal_agent_names(value: &Value) -> Value {
    let mut canonical = value.clone();
    if let Some(object) = canonical.as_object_mut() {
        if object.get("runtime_profile").and_then(Value::as_str) == Some("message_agent") {
            object.insert(
                "runtime_profile".to_string(),
                Value::String("personal_agent".to_string()),
            );
        }
        if object.get("display_name").and_then(Value::as_str) == Some("Hermes Message Agent") {
            object.insert(
                "display_name".to_string(),
                Value::String("Hermes Personal Agent".to_string()),
            );
        }
        let canonical_ensure_once_key = object
            .get("ensure_once_key")
            .and_then(Value::as_str)
            .and_then(|value| value.strip_prefix("app-message-agent:"))
            .map(|suffix| format!("app-personal-agent:{suffix}"));
        if let Some(canonical_ensure_once_key) = canonical_ensure_once_key {
            object.insert(
                "ensure_once_key".to_string(),
                Value::String(canonical_ensure_once_key),
            );
        }
    }
    canonical
}

fn sanitized_legacy_message_agent_for_hash(value: &Value) -> Value {
    let mut sanitized = sanitized_desired_personal_agent_for_hash(value);
    if let Some(object) = sanitized.as_object_mut() {
        if object.get("runtime_profile").and_then(Value::as_str) == Some("personal_agent") {
            object.insert(
                "runtime_profile".to_string(),
                Value::String("message_agent".to_string()),
            );
        }
        if object.get("display_name").and_then(Value::as_str) == Some("Hermes Personal Agent") {
            object.insert(
                "display_name".to_string(),
                Value::String("Hermes Message Agent".to_string()),
            );
        }
        let legacy_ensure_once_key = object
            .get("ensure_once_key")
            .and_then(Value::as_str)
            .and_then(|value| value.strip_prefix("app-personal-agent:"))
            .map(|suffix| format!("app-message-agent:{suffix}"));
        if let Some(legacy_ensure_once_key) = legacy_ensure_once_key {
            object.insert(
                "ensure_once_key".to_string(),
                Value::String(legacy_ensure_once_key),
            );
        }
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

fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).context("canonicalize secure daemon bootstrap json")
}

fn daemon_bootstrap_key_id(daemon_agent_did: &str) -> String {
    format!(
        "{}#{}",
        daemon_agent_did.trim(),
        DAEMON_BOOTSTRAP_KEY_FRAGMENT
    )
}

fn decode_base64url_bytes(field_name: &str, value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim())
        .with_context(|| format!("{field_name} must be base64url without padding"))
}

fn decode_base64url_fixed<const N: usize>(
    field_name: &str,
    value: &str,
    len: usize,
) -> Result<[u8; N]> {
    let bytes = decode_base64url_bytes(field_name, value)?;
    if bytes.len() != len {
        bail!("{field_name} must decode to {len} bytes");
    }
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{field_name} has invalid decoded length"))
}

/// Encrypt one bootstrap envelope without exposing its plaintext to the host test.
///
/// This seam exists only for the feature-gated stdin-only system-test probe. It is
/// absent from default daemon builds and is not a daemon or CLI product entry point.
#[cfg(any(test, feature = "system-test-probe"))]
pub fn encrypt_secure_bootstrap_payload_for_system_test(
    recipient_daemon_did: &str,
    recipient_public: anp::PublicKeyMaterial,
    sender_human_did: &str,
    operation_id: &str,
    issued_at: &str,
    expires_at: &str,
    nonce_bytes: [u8; 12],
    sender_ephemeral_private: x25519_dalek::StaticSecret,
    aad: Value,
    payload: Value,
) -> Result<Value> {
    encrypt_secure_bootstrap_payload_for_test_with_hash(
        recipient_daemon_did,
        recipient_public,
        sender_human_did,
        operation_id,
        issued_at,
        expires_at,
        nonce_bytes,
        sender_ephemeral_private,
        aad,
        payload,
        None,
    )
}

/// Encrypt already serialized, zeroizing plaintext for the system-test probe.
#[cfg(feature = "system-test-probe")]
pub fn encrypt_secure_bootstrap_bytes_for_system_test(
    recipient_daemon_did: &str,
    recipient_public: anp::PublicKeyMaterial,
    sender_human_did: &str,
    operation_id: &str,
    issued_at: &str,
    expires_at: &str,
    nonce_bytes: [u8; 12],
    sender_ephemeral_private: x25519_dalek::StaticSecret,
    aad: Value,
    plaintext: &[u8],
) -> Result<Value> {
    encrypt_secure_bootstrap_plaintext_with_hash(
        recipient_daemon_did,
        recipient_public,
        sender_human_did,
        operation_id,
        issued_at,
        expires_at,
        nonce_bytes,
        sender_ephemeral_private,
        aad,
        plaintext,
        None,
    )
}

#[cfg(test)]
pub(crate) fn encrypt_secure_bootstrap_payload_for_test(
    recipient_daemon_did: &str,
    recipient_public: anp::PublicKeyMaterial,
    sender_human_did: &str,
    operation_id: &str,
    issued_at: &str,
    expires_at: &str,
    nonce_bytes: [u8; 12],
    sender_ephemeral_private: x25519_dalek::StaticSecret,
    aad: Value,
    payload: Value,
) -> Result<Value> {
    encrypt_secure_bootstrap_payload_for_system_test(
        recipient_daemon_did,
        recipient_public,
        sender_human_did,
        operation_id,
        issued_at,
        expires_at,
        nonce_bytes,
        sender_ephemeral_private,
        aad,
        payload,
    )
}

#[cfg(any(test, feature = "system-test-probe"))]
pub(crate) fn encrypt_secure_bootstrap_payload_for_test_with_hash(
    recipient_daemon_did: &str,
    recipient_public: anp::PublicKeyMaterial,
    sender_human_did: &str,
    operation_id: &str,
    issued_at: &str,
    expires_at: &str,
    nonce_bytes: [u8; 12],
    sender_ephemeral_private: x25519_dalek::StaticSecret,
    aad: Value,
    payload: Value,
    payload_sha256_override: Option<String>,
) -> Result<Value> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&payload).context("serialize test secure bootstrap payload")?,
    );
    encrypt_secure_bootstrap_plaintext_with_hash(
        recipient_daemon_did,
        recipient_public,
        sender_human_did,
        operation_id,
        issued_at,
        expires_at,
        nonce_bytes,
        sender_ephemeral_private,
        aad,
        plaintext.as_slice(),
        payload_sha256_override,
    )
}

#[cfg(any(test, feature = "system-test-probe"))]
#[allow(clippy::too_many_arguments)]
fn encrypt_secure_bootstrap_plaintext_with_hash(
    recipient_daemon_did: &str,
    recipient_public: anp::PublicKeyMaterial,
    sender_human_did: &str,
    operation_id: &str,
    issued_at: &str,
    expires_at: &str,
    nonce_bytes: [u8; 12],
    sender_ephemeral_private: x25519_dalek::StaticSecret,
    aad: Value,
    plaintext: &[u8],
    payload_sha256_override: Option<String>,
) -> Result<Value> {
    let recipient_public = match recipient_public {
        anp::PublicKeyMaterial::X25519(bytes) => X25519PublicKey::from(bytes),
        _ => bail!("recipient public key must be X25519"),
    };
    let sender_ephemeral_public = X25519PublicKey::from(&sender_ephemeral_private).to_bytes();
    let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
    let mut envelope = DaemonSecureBootstrapEnvelope {
        schema: DAEMON_BOOTSTRAP_SECURE_SCHEMA.to_string(),
        recipient_daemon_did: recipient_daemon_did.to_string(),
        recipient_key_id: daemon_bootstrap_key_id(recipient_daemon_did),
        sender_human_did: sender_human_did.to_string(),
        operation_id: operation_id.to_string(),
        issued_at: issued_at.to_string(),
        expires_at: expires_at.to_string(),
        nonce,
        sender_ephemeral_public_key: URL_SAFE_NO_PAD.encode(sender_ephemeral_public),
        ciphertext: String::new(),
        aad,
        payload_sha256: None,
        extra: BTreeMap::new(),
    };
    envelope.payload_sha256 =
        Some(payload_sha256_override.unwrap_or_else(|| hex_lower(&Sha256::digest(plaintext))));
    let shared = sender_ephemeral_private.diffie_hellman(&recipient_public);
    let key = derive_secure_bootstrap_key(shared.as_bytes(), &envelope)?;
    let aad = stable_secure_bootstrap_aad(&envelope)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("test secure bootstrap encrypt failed"))?;
    envelope.ciphertext = URL_SAFE_NO_PAD.encode(ciphertext);
    Ok(serde_json::to_value(envelope)?)
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
            "idempotency_key": "personal-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1",
            "app_instance_id": "app_1",
            "controller_did": "did:wba:example.com:user:alice:e1_user",
            "user_subkey_package": {
                "schema": USER_SUBKEY_PACKAGE_SCHEMA_V2,
                "user_did": "did:wba:example.com:user:alice:e1_user",
                "verification_method": "did:wba:example.com:user:alice:e1_user#daemon-key-1",
                "key_type": "Multikey/Ed25519",
                "key_algorithm": "Ed25519",
                "public_key_multibase": public_key,
                "private_key_encoding": "pem",
                "private_key_pem": private_key,
                "allowed_scopes": [
                    "message.inbox.read.plain",
                    "message.history.read.plain",
                    "message.summarize_plain"
                ]
            },
            "desired_personal_agent": {
                "role": "app_message_handler",
                "runtime": "hermes",
                "runtime_provider": "hermes",
                "runtime_profile": "personal_agent",
                "preferred_language": "zh-Hans",
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
    fn legacy_desired_message_agent_field_decodes_but_serializes_canonical_name() {
        let mut canonical_payload = valid_payload();
        canonical_payload["desired_personal_agent"]["display_name"] =
            json!("Hermes Personal Agent");
        canonical_payload["desired_personal_agent"]["ensure_once_key"] =
            json!("app-personal-agent:did:wba:example.com:user:alice:e1_user:app_1");
        let canonical_envelope = parse_bootstrap_payload(canonical_payload.clone()).unwrap();
        let mut legacy_payload = canonical_payload;
        let desired = legacy_payload
            .as_object_mut()
            .unwrap()
            .remove("desired_personal_agent")
            .unwrap();
        let mut desired = desired;
        desired["runtime_profile"] = json!("message_agent");
        desired["display_name"] = json!("Hermes Message Agent");
        desired["ensure_once_key"] =
            json!("app-message-agent:did:wba:example.com:user:alice:e1_user:app_1");
        legacy_payload
            .as_object_mut()
            .unwrap()
            .insert("desired_message_agent".to_string(), desired);

        let envelope = parse_bootstrap_payload(legacy_payload).unwrap();
        assert_eq!(
            envelope.desired_personal_agent["runtime_profile"],
            "personal_agent"
        );
        assert_eq!(
            stable_payload_hash(&envelope).unwrap(),
            stable_payload_hash(&canonical_envelope).unwrap()
        );
        assert_eq!(
            legacy_stable_payload_hash(&envelope).unwrap(),
            legacy_stable_payload_hash(&canonical_envelope).unwrap()
        );
        assert_ne!(
            stable_payload_hash(&envelope).unwrap(),
            legacy_stable_payload_hash(&envelope).unwrap()
        );

        let serialized = serde_json::to_value(envelope).unwrap();
        assert!(serialized.get("desired_personal_agent").is_some());
        assert!(serialized.get("desired_message_agent").is_none());
    }

    #[test]
    fn secure_bootstrap_payload_validates_contract_and_redacts_ciphertext() {
        let payload = json!({
            "schema": DAEMON_BOOTSTRAP_SECURE_SCHEMA,
            "recipient_daemon_did": "did:agent:daemon",
            "recipient_key_id": "did:agent:daemon#key-3",
            "sender_human_did": "did:wba:example.com:user:alice:e1_user",
            "operation_id": "personal-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1",
            "issued_at": "2026-06-19T01:00:00Z",
            "expires_at": "2026-06-19T01:05:00Z",
            "nonce": URL_SAFE_NO_PAD.encode([1_u8; 12]),
            "sender_ephemeral_public_key": URL_SAFE_NO_PAD.encode([2_u8; 32]),
            "ciphertext": URL_SAFE_NO_PAD.encode(b"encrypted-private-package"),
            "payload_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aad": {
                "human_did": "did:wba:example.com:user:alice:e1_user",
                "daemon_agent_did": "did:agent:daemon",
                "binding_id": "app-personal-agent:did:wba:example.com:user:alice:e1_user:app_1"
            }
        });

        let envelope = parse_secure_bootstrap_payload(payload).unwrap();
        let debug = format!("{envelope:?}");

        assert_eq!(envelope.schema, DAEMON_BOOTSTRAP_SECURE_SCHEMA);
        assert_eq!(envelope.recipient_daemon_did, "did:agent:daemon");
        assert!(!debug.contains("encrypted-private-package"));
        assert!(debug.contains("<redacted-bootstrap-ciphertext>"));
        assert!(debug.contains("<redacted-bootstrap-aad>"));
    }

    #[test]
    fn secure_bootstrap_payload_rejects_private_state_in_aad() {
        let payload = json!({
            "schema": DAEMON_BOOTSTRAP_SECURE_SCHEMA,
            "recipient_daemon_did": "did:agent:daemon",
            "recipient_key_id": "did:agent:daemon#key-3",
            "sender_human_did": "did:wba:example.com:user:alice:e1_user",
            "operation_id": "personal-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1",
            "issued_at": "2026-06-19T01:00:00Z",
            "expires_at": "2026-06-19T01:05:00Z",
            "nonce": URL_SAFE_NO_PAD.encode([1_u8; 12]),
            "sender_ephemeral_public_key": URL_SAFE_NO_PAD.encode([2_u8; 32]),
            "ciphertext": URL_SAFE_NO_PAD.encode(b"encrypted-private-package"),
            "aad": {
                "private_key_pem": "secret"
            }
        });

        let error = parse_secure_bootstrap_payload(payload).unwrap_err();

        assert!(error.to_string().contains("forbidden private state"));
    }

    #[test]
    fn message_send_scope_is_rejected_for_mvp_bootstrap() {
        let mut payload = valid_payload();
        payload["user_subkey_package"]["allowed_scopes"]
            .as_array_mut()
            .unwrap()
            .push(json!("message.send.plain"));
        let envelope = parse_bootstrap_payload(payload).unwrap();
        let error =
            validate_bootstrap_envelope(&envelope, "did:wba:example.com:user:alice:e1_user")
                .unwrap_err();

        assert!(error.to_string().contains("not allowed"));
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
    fn legacy_v1_private_key_multibase_payload_still_validates() {
        let mut payload = valid_payload();
        let private_key = payload["user_subkey_package"]["private_key_pem"]
            .as_str()
            .unwrap()
            .to_string();
        let package = payload["user_subkey_package"].as_object_mut().unwrap();
        package.insert(
            "schema".to_string(),
            Value::String(USER_SUBKEY_PACKAGE_SCHEMA.to_string()),
        );
        package.remove("key_algorithm");
        package.remove("private_key_encoding");
        package.remove("private_key_pem");
        package.insert(
            "private_key_multibase".to_string(),
            Value::String(private_key),
        );
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
        with_token["desired_personal_agent"]["runtime_registration_token"] =
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
        first["user_subkey_package"]["private_key_pem"] =
            json!("-----BEGIN PRIVATE KEY-----\nfirst\n-----END PRIVATE KEY-----");
        second["user_subkey_package"]["private_key_pem"] =
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
