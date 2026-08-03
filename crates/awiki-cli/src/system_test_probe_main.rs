//! Feature-gated, secret-contained probe for multi-device system tests.
//!
//! [INPUT]: The resolved CLI workspace, vNext identity/device projections, and JSONL probe
//! requests on stdin.
//! [OUTPUT]: Closed probe results on stdout; runtime failures expose only stable error codes.
//! [POS]: Test-only binary boundary that exercises production `im-core` clients without exposing
//! private keys, tokens, ratchet state, or MLS state.
//!
//! [PROTOCOL]:
//! 1. Resolve the protocol device ID from `IdentityDeviceSummary`, not the legacy identity summary.
//! 2. Build the probe client from that same `ImCore` instance so identity/device state is coherent.
//! 3. Plan attachment ticket requests through the production runtime so Profile, service target,
//!    and per-device wire message authorization remain identical to normal downloads.
//! 4. Keep stdout/stderr free of credentials and cryptographic state.
//! 5. Account-state reads and public mutations return canonical versions, bounded counts, and
//!    match booleans only; they never return account IDs, DIDs, device IDs, profile values, or
//!    bearer tokens.
//! 6. The test-only Account State fail-once code is accepted only for Agent Inventory reads and
//!    is mapped to one secret-free probe code; every other RPC rejection keeps its prior mapping.
//! 7. Direct-wire checks scan a bounded sequence of exact-device `inbox.get` pages with the
//!    current Core/Vault session, but return only match/shape booleans and counts, never raw wire
//!    content or pagination state.
//! 8. Agent bootstrap checks keep Device Access and Daemon Vault records inside Rust and expose
//!    only a closed bool-or-null projection; account, DID, device, key, and claim values never
//!    cross the probe boundary.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use im_core::identity::{IdentityDeviceMode, IdentityDeviceReadiness, IdentityDeviceRole};
use rand::RngCore;
use reqwest::header::{
    HeaderValue as ReqwestHeaderValue, AUTHORIZATION as REQWEST_AUTHORIZATION, CONTENT_TYPE,
};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION as WS_AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};
use zeroize::Zeroizing;

const MAX_REQUEST_LINE_BYTES: usize = 64 * 1024;
const MAX_RPC_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 300_000;
const RPC_PATH: &str = "/im/rpc";
const ACCOUNT_STATE_RPC_PATH: &str = "/user-service/account-state/rpc";
const AGENT_INVENTORY_RPC_PATH: &str = "/user-service/agent-inventory/rpc";
const AGENT_REGISTRATION_RPC_PATH: &str = "/user-service/agent-registration/rpc";
const DID_AUTH_RPC_PATH: &str = "/user-service/did-auth/rpc";
const ME_RPC_PATH: &str = "/user-service/me/rpc";
const DAEMON_STATE_ROOT_ENV: &str = "AWIKI_SYSTEM_TEST_PROBE_DAEMON_STATE_ROOT";
const DAEMON_AGENT_KIND_ENV: &str = "AWIKI_SYSTEM_TEST_PROBE_DAEMON_AGENT_KIND";

const INVALID_REQUEST: &str = "probe.invalid_request";
const INVALID_STATE: &str = "probe.invalid_state";
const TRANSPORT_FAILED: &str = "probe.transport_failed";
const RUNTIME_FAILED: &str = "probe.runtime_failed";
const DAEMON_FIXTURE_BOOTSTRAP_VALIDATION_OR_PERSIST: &str =
    "probe.daemon_fixture.bootstrap_validation_or_persist";
const DAEMON_FIXTURE_BOOTSTRAP_MESSAGE_NOT_ROUTED: &str =
    "probe.daemon_fixture.bootstrap_message_not_routed";
const DAEMON_FIXTURE_BOOTSTRAP_SECURE_ENVELOPE: &str =
    "probe.daemon_fixture.bootstrap_secure_envelope";
const DAEMON_FIXTURE_BOOTSTRAP_STATE_PERSIST: &str = "probe.daemon_fixture.bootstrap_state_persist";
const DAEMON_FIXTURE_BOOTSTRAP_RECEIVED_AUDIT: &str =
    "probe.daemon_fixture.bootstrap_received_audit";
const DAEMON_FIXTURE_RUNTIME_REGISTRATION_PREPARE_OR_EXCHANGE: &str =
    "probe.daemon_fixture.runtime_registration_prepare_or_exchange";
const DAEMON_FIXTURE_BINDING_PERSIST: &str = "probe.daemon_fixture.binding_persist";
const DAEMON_FIXTURE_BINDING_PROJECTION: &str = "probe.daemon_fixture.binding_projection";
const ACCOUNT_STATE_TEST_FAIL_ONCE: &str = "account_state_test_fail_once";
const PROBE_ACCOUNT_STATE_TEST_FAIL_ONCE: &str = "probe.account_state_test_fail_once";

const DEVICE_NOT_ELIGIBLE: &str = "anp.device_not_eligible";
const DEVICE_STATE_CHANGED: &str = "anp.device_state_changed";
const SESSION_UNAUTHORIZED: &str = "client.session_unauthorized";
const DOWNLOAD_TICKET_INVALID: &str = "anp.attachment.download_ticket_invalid";

const DIRECT_E2EE: &str = "direct-e2ee";
const ATTACHMENT_V2: &str = "anp.attachment.v2";
const DIRECT_INIT_CONTENT_TYPE: &str = "application/anp-direct-init+json";
const DIRECT_CIPHER_CONTENT_TYPE: &str = "application/anp-direct-cipher+json";
const DIRECT_WIRE_INBOX_PAGE_LIMIT: i64 = 100;
const DIRECT_WIRE_INBOX_MAX_PAGES: usize = 20;
const DAEMON_SETUP_ATTEMPT_LIMIT: usize = 2;

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone)]
struct RequestId(Value);

struct ProbeRequest {
    id: RequestId,
    action: Action,
}

enum Action {
    DeviceReadiness,
    AgentBootstrapIdentity(AgentBootstrapIdentityParams),
    DaemonContinuityBaseline,
    DaemonContinuityVerify(DaemonContinuityVerifyParams),
    StageDaemonContinuityRoot(StageDaemonContinuityRootParams),
    PrepareDaemonContinuityFixture(PrepareDaemonContinuityFixtureParams),
    DaemonFixtureResources,
    SendPlainMarker(SendPlainMarkerParams),
    DaemonMarkerProcessed { message_id: String },
    HumanDaemonSubkeyState,
    OpenWs,
    WaitWsClosed { timeout_ms: u64 },
    CloseWs,
    ReconnectWs,
    HoldDownloadTicket(AttachmentTicketParams),
    ProbeDownloadTicket(AttachmentTicketParams),
    ProbePrekey(PrekeyParams),
    DirectWireProjection(DirectWireProjectionParams),
    AccountStateManifest,
    AccountStateAgent(AgentSnapshotParams),
    AccountStateAgentRename(AgentRenameParams),
    AccountStateAgentConfig(AgentConfigParams),
    AccountStateAgentUnbind(AgentTargetParams),
    AccountStateAgentRemove(AgentTargetParams),
    AccountStateStatus(AgentStatusParams),
    AccountStateProfile(ProfileSnapshotParams),
    AccountStateProfileUpdate(ProfileUpdateParams),
    AccountStateRegistry(RegistrySnapshotParams),
    RedeemHeldTicket { expected_digest_b64u: String },
    Shutdown,
}

struct AgentBootstrapIdentityParams {
    controller_account_id: String,
}

struct DaemonContinuityVerifyParams {
    old_controller_did: String,
    new_controller_did: String,
    queued_marker: String,
    controller_marker: String,
}

struct PrepareDaemonContinuityFixtureParams {
    daemon_binary: std::path::PathBuf,
    state_root: std::path::PathBuf,
    daemon_agent_did: String,
    daemon_handle: String,
    runtime_handle: String,
    controller_handle: String,
    app_instance_id: String,
}

struct StageDaemonContinuityRootParams {
    state_root: std::path::PathBuf,
    daemon_handle: String,
}

struct SendPlainMarkerParams {
    target_did: String,
    message_id: String,
    marker: Zeroizing<String>,
}

#[derive(Serialize)]
struct ProbeBootstrapPayload<'a> {
    schema: &'static str,
    bootstrap_id: &'a str,
    idempotency_key: &'a str,
    app_instance_id: &'a str,
    controller_did: &'a str,
    user_subkey_package: ProbeUserSubkeyPackage<'a>,
    desired_personal_agent: ProbeDesiredPersonalAgent<'a>,
    capability_policy: ProbeCapabilityPolicy,
}

#[derive(Serialize)]
struct ProbeUserSubkeyPackage<'a> {
    schema: &'a str,
    user_did: &'a str,
    verification_method: &'a str,
    key_type: &'a str,
    key_algorithm: &'a str,
    public_key_multibase: &'a str,
    private_key_encoding: &'a str,
    private_key_pem: &'a str,
    allowed_scopes: [&'static str; 1],
}

#[derive(Serialize)]
struct ProbeDesiredPersonalAgent<'a> {
    role: &'static str,
    runtime: &'static str,
    runtime_provider: &'static str,
    runtime_profile: &'static str,
    display_name: &'static str,
    preferred_language: &'static str,
    ensure_once_key: &'a str,
    runtime_registration_token: &'a str,
}

#[derive(Serialize)]
struct ProbeCapabilityPolicy {
    schema: &'static str,
    capabilities: [&'static str; 1],
    require_confirmation_for_write_actions: bool,
}

#[derive(Deserialize)]
struct RegistrationTokenResult {
    token: String,
}

struct DaemonContinuityBaseline {
    agent_identity_hash: [u8; 32],
    root_key_hash: [u8; 32],
    device_keys_hash: [u8; 32],
    delegated_key_hash: [u8; 32],
    definition_hash: [u8; 32],
    route_state_hash: [u8; 32],
    route_record_count: usize,
    verification_method: String,
    user_did: String,
    app_instance_id: String,
}

#[derive(Clone, Copy)]
struct DaemonMarkerAbsenceEvidence {
    event_absent: bool,
    route_absent: bool,
    task_absent: bool,
    run_absent: bool,
    final_absent: bool,
}

impl DaemonMarkerAbsenceEvidence {
    fn all_absent(self) -> bool {
        self.event_absent
            && self.route_absent
            && self.task_absent
            && self.run_absent
            && self.final_absent
    }
}

#[derive(Clone, Copy)]
struct DaemonContinuityEvidence {
    agent_identity_unchanged: bool,
    root_key_unchanged: bool,
    device_keys_unchanged: bool,
    delegated_key_unchanged: bool,
    old_controller_binding_unchanged: bool,
    new_controller_lacks_delegated_key: bool,
    controller_identity_changed: bool,
    queued_delegated_marker: DaemonMarkerAbsenceEvidence,
    new_controller_marker: DaemonMarkerAbsenceEvidence,
}

fn closed_daemon_continuity_result(evidence: DaemonContinuityEvidence) -> Value {
    let old_controller_denied =
        evidence.controller_identity_changed && evidence.queued_delegated_marker.all_absent();
    let new_controller_denied =
        evidence.controller_identity_changed && evidence.new_controller_marker.all_absent();
    json!({
        "agent_identity_unchanged": evidence.agent_identity_unchanged,
        "root_key_unchanged": evidence.root_key_unchanged,
        "device_keys_unchanged": evidence.device_keys_unchanged,
        "delegated_key_unchanged": evidence.delegated_key_unchanged,
        "old_controller_binding_unchanged": evidence.old_controller_binding_unchanged,
        "new_controller_lacks_delegated_key": evidence.new_controller_lacks_delegated_key,
        "old_delegated_pull_denied": evidence.controller_identity_changed
            && evidence.queued_delegated_marker.event_absent,
        "old_controller_denied": old_controller_denied,
        "new_controller_denied": new_controller_denied,
        "no_route_created": evidence.queued_delegated_marker.route_absent
            && evidence.new_controller_marker.route_absent,
        "no_task_created": evidence.queued_delegated_marker.task_absent
            && evidence.new_controller_marker.task_absent,
        "no_run_created": evidence.queued_delegated_marker.run_absent
            && evidence.new_controller_marker.run_absent,
        "no_final_created": evidence.queued_delegated_marker.final_absent
            && evidence.new_controller_marker.final_absent,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DaemonFixturePrepareFailureStage {
    Token,
    Setup,
    RuntimeMaterial,
    SyncInitializeCallFailed,
    SyncInitializeRecoveryRequired,
    SyncInitializeRetryableFailure,
    SyncInitializeAuthRevoked,
    BootstrapSend,
}

impl DaemonFixturePrepareFailureStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Setup => "setup",
            Self::RuntimeMaterial => "runtime_material",
            Self::SyncInitializeCallFailed => "sync_initialize_call_failed",
            Self::SyncInitializeRecoveryRequired => "sync_initialize_recovery_required",
            Self::SyncInitializeRetryableFailure => "sync_initialize_retryable_failure",
            Self::SyncInitializeAuthRevoked => "sync_initialize_auth_revoked",
            Self::BootstrapSend => "bootstrap_send",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DaemonFixturePrepareFailure(DaemonFixturePrepareFailureStage);

impl From<ProbeFailure> for DaemonFixturePrepareFailure {
    fn from(_failure: ProbeFailure) -> Self {
        Self(DaemonFixturePrepareFailureStage::RuntimeMaterial)
    }
}

fn closed_daemon_fixture_prepare_result(
    prepared: bool,
    daemon_agent_did: &str,
    failure_stage: Option<DaemonFixturePrepareFailureStage>,
) -> Value {
    json!({
        "prepared": prepared,
        "daemon_agent_did": daemon_agent_did,
        "failure_stage": failure_stage.map(DaemonFixturePrepareFailureStage::as_str),
    })
}

fn daemon_fixture_prepare_result_after_boundary(
    daemon_agent_did: &str,
    preparation: Result<(), DaemonFixturePrepareFailure>,
) -> Value {
    match preparation {
        Ok(()) => closed_daemon_fixture_prepare_result(true, daemon_agent_did, None),
        Err(failure) => {
            closed_daemon_fixture_prepare_result(false, daemon_agent_did, Some(failure.0))
        }
    }
}

async fn daemon_fixture_sync_before_send<Sync, SyncFuture, Send, SendFuture>(
    sync: Sync,
    send: Send,
) -> Result<(), DaemonFixturePrepareFailure>
where
    Sync: FnOnce() -> SyncFuture,
    SyncFuture:
        std::future::Future<Output = Result<im_core::messages::MessageSyncStatus, ProbeFailure>>,
    Send: FnOnce() -> SendFuture,
    SendFuture: std::future::Future<Output = Result<(), ProbeFailure>>,
{
    let status = sync().await.map_err(|_| {
        DaemonFixturePrepareFailure(DaemonFixturePrepareFailureStage::SyncInitializeCallFailed)
    })?;
    match status {
        im_core::messages::MessageSyncStatus::Idle
        | im_core::messages::MessageSyncStatus::Changed => {}
        im_core::messages::MessageSyncStatus::RecoveryRequired => {
            return Err(DaemonFixturePrepareFailure(
                DaemonFixturePrepareFailureStage::SyncInitializeRecoveryRequired,
            ))
        }
        im_core::messages::MessageSyncStatus::RetryableFailure => {
            return Err(DaemonFixturePrepareFailure(
                DaemonFixturePrepareFailureStage::SyncInitializeRetryableFailure,
            ))
        }
        im_core::messages::MessageSyncStatus::AuthRevoked => {
            return Err(DaemonFixturePrepareFailure(
                DaemonFixturePrepareFailureStage::SyncInitializeAuthRevoked,
            ))
        }
    }
    send()
        .await
        .map_err(|_| DaemonFixturePrepareFailure(DaemonFixturePrepareFailureStage::BootstrapSend))
}

fn daemon_token_issue_or_root_receipt(
    daemon_agent_did: &str,
    issued: Result<Zeroizing<String>, ProbeFailure>,
) -> Result<Zeroizing<String>, Value> {
    issued.map_err(|_| {
        closed_daemon_fixture_prepare_result(
            false,
            daemon_agent_did,
            Some(DaemonFixturePrepareFailureStage::Token),
        )
    })
}

fn daemon_registration_metadata(daemon_agent_did: &str) -> Value {
    json!({
        "suite_case": "handle-recovery-daemon-continuity",
        "daemon_agent_did": daemon_agent_did,
    })
}

fn daemon_setup_agent_did(mut output: std::process::Output) -> Result<String, ProbeFailure> {
    let stdout = Zeroizing::new(std::mem::take(&mut output.stdout));
    if !output.status.success() || stdout.len() > MAX_RPC_RESPONSE_BYTES {
        return Err(ProbeFailure::Runtime);
    }
    let setup: Value =
        serde_json::from_slice(stdout.as_slice()).map_err(|_| ProbeFailure::Runtime)?;
    let daemon_agent_did = setup
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agent| agent.get("agent_did"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or(ProbeFailure::Runtime)?;
    im_core::ids::Did::parse(&daemon_agent_did).map_err(|_| ProbeFailure::Runtime)?;
    Ok(daemon_agent_did)
}

fn recover_exact_persisted_daemon_agent_did(
    state_root: &Path,
    daemon_handle: &str,
    controller_did: &str,
) -> Result<Option<String>, ProbeFailure> {
    let database_path = state_root.join("daemon.db");
    if !database_path.exists() {
        return Ok(None);
    }
    let connection = rusqlite::Connection::open_with_flags(
        database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| ProbeFailure::Runtime)?;
    let mut candidates = BTreeSet::new();
    for (table, query) in [
        (
            "agent_definition",
            r#"
SELECT agent_did
FROM agent_definition
WHERE agent_kind = 'daemon'
  AND handle = ?1
  AND controller_did = ?2
  AND status = 'active'
"#,
        ),
        (
            "agent_registration_pending",
            r#"
SELECT agent_did
FROM agent_registration_pending
WHERE agent_kind = 'daemon'
  AND handle = ?1
  AND controller_did = ?2
"#,
        ),
    ] {
        let table_exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| ProbeFailure::Runtime)?
            != 0;
        if !table_exists {
            continue;
        }
        let mut statement = connection
            .prepare(query)
            .map_err(|_| ProbeFailure::Runtime)?;
        let rows = statement
            .query_map(rusqlite::params![daemon_handle, controller_did], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| ProbeFailure::Runtime)?;
        for row in rows {
            let candidate = row.map_err(|_| ProbeFailure::Runtime)?;
            im_core::ids::Did::parse(&candidate).map_err(|_| ProbeFailure::InvalidState)?;
            candidates.insert(candidate);
        }
    }
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.into_iter().next()),
        _ => Err(ProbeFailure::InvalidState),
    }
}

fn daemon_setup_failure_result(
    state_root: &Path,
    daemon_handle: &str,
    controller_did: &str,
    original_failure: ProbeFailure,
) -> Result<Value, ProbeFailure> {
    match recover_exact_persisted_daemon_agent_did(state_root, daemon_handle, controller_did)? {
        Some(agent_did) => Ok(closed_daemon_fixture_prepare_result(
            false,
            &agent_did,
            Some(DaemonFixturePrepareFailureStage::Setup),
        )),
        None => Err(original_failure),
    }
}

struct DaemonRouteStateSnapshot {
    hash: [u8; 32],
    record_count: usize,
}

struct DaemonMarkerStateIds {
    event_id: Option<String>,
    message_id: String,
    task_id: String,
    run_id: String,
}

struct AttachmentTicketParams {
    sender_did: String,
    message_id: String,
    attachment_id: String,
    object_uri: String,
}

struct PrekeyParams {
    target_did: String,
    target_device_id: String,
}

struct DirectWireProjectionParams {
    peer_did: String,
    message_id: String,
    expected_shape: DirectWireShape,
    forbidden_plaintext: Zeroizing<String>,
}

#[derive(Clone, Copy)]
enum DirectWireShape {
    Init,
    Cipher,
}

impl DirectWireShape {
    fn content_type(self) -> &'static str {
        match self {
            Self::Init => DIRECT_INIT_CONTENT_TYPE,
            Self::Cipher => DIRECT_CIPHER_CONTENT_TYPE,
        }
    }
}

struct AgentSnapshotParams {
    agent_did: String,
    expected_active_state: String,
    expected_display_name: String,
    expected_active_mode: String,
    expected_whitelist_handles: Vec<String>,
    expected_blacklist_handles: Vec<String>,
}

struct AgentRenameParams {
    agent_did: String,
    display_name: String,
}

struct AgentConfigParams {
    agent_did: String,
    active_mode: String,
    whitelist_handles: Vec<String>,
    blacklist_handles: Vec<String>,
}

struct AgentTargetParams {
    agent_did: String,
}

struct AgentStatusParams {
    agent_did: String,
}

struct ProfileSnapshotParams {
    expected_nick_name: String,
}

struct ProfileUpdateParams {
    nick_name: String,
}

struct RegistrySnapshotParams {
    target_device_id: String,
    expected_status: String,
}

#[derive(Clone, Copy)]
enum ProbeFailure {
    InvalidRequest,
    InvalidState,
    Transport,
    Runtime,
    DaemonFixtureBootstrapValidationOrPersist,
    DaemonFixtureBootstrapMessageNotRouted,
    DaemonFixtureBootstrapSecureEnvelope,
    DaemonFixtureBootstrapStatePersist,
    DaemonFixtureBootstrapReceivedAudit,
    DaemonFixtureRuntimeRegistrationPrepareOrExchange,
    DaemonFixtureBindingPersist,
    DaemonFixtureBindingProjection,
    AccountStateTestFailOnce,
}

impl ProbeFailure {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => INVALID_REQUEST,
            Self::InvalidState => INVALID_STATE,
            Self::Transport => TRANSPORT_FAILED,
            Self::Runtime => RUNTIME_FAILED,
            Self::DaemonFixtureBootstrapValidationOrPersist => {
                DAEMON_FIXTURE_BOOTSTRAP_VALIDATION_OR_PERSIST
            }
            Self::DaemonFixtureBootstrapMessageNotRouted => {
                DAEMON_FIXTURE_BOOTSTRAP_MESSAGE_NOT_ROUTED
            }
            Self::DaemonFixtureBootstrapSecureEnvelope => DAEMON_FIXTURE_BOOTSTRAP_SECURE_ENVELOPE,
            Self::DaemonFixtureBootstrapStatePersist => DAEMON_FIXTURE_BOOTSTRAP_STATE_PERSIST,
            Self::DaemonFixtureBootstrapReceivedAudit => DAEMON_FIXTURE_BOOTSTRAP_RECEIVED_AUDIT,
            Self::DaemonFixtureRuntimeRegistrationPrepareOrExchange => {
                DAEMON_FIXTURE_RUNTIME_REGISTRATION_PREPARE_OR_EXCHANGE
            }
            Self::DaemonFixtureBindingPersist => DAEMON_FIXTURE_BINDING_PERSIST,
            Self::DaemonFixtureBindingProjection => DAEMON_FIXTURE_BINDING_PROJECTION,
            Self::AccountStateTestFailOnce => PROBE_ACCOUNT_STATE_TEST_FAIL_ONCE,
        }
    }
}

#[derive(Clone, Copy)]
enum RpcRejectionPolicy {
    Standard,
    AccountStateAgentInventory,
}

impl RpcRejectionPolicy {
    fn allowlisted_code(self, status: reqwest::StatusCode, error: &Value) -> Option<&'static str> {
        allowlisted_anp_code(error).or_else(|| match self {
            Self::Standard => None,
            Self::AccountStateAgentInventory if status.is_success() => {
                allowlisted_account_state_test_code(error)
            }
            Self::AccountStateAgentInventory => None,
        })
    }
}

struct HeldTicket {
    ticket: Zeroizing<String>,
    object_uri: reqwest::Url,
}

struct Probe {
    _core: Option<im_core::ImCore>,
    _client: Option<im_core::ImClient>,
    http: reqwest::Client,
    bearer: Zeroizing<String>,
    message_rpc_url: reqwest::Url,
    account_state_rpc_url: reqwest::Url,
    agent_inventory_rpc_url: reqwest::Url,
    agent_registration_rpc_url: reqwest::Url,
    did_auth_rpc_url: reqwest::Url,
    me_rpc_url: reqwest::Url,
    websocket_url: String,
    ca_bundle: Option<String>,
    local_did: String,
    local_account_id: String,
    local_device_id: String,
    local_signing_key_id: String,
    local_e2ee_key_id: String,
    local_binding_generation: String,
    local_device_auth_generation: String,
    local_manifest_single_device: bool,
    local_document_hash: Option<String>,
    local_key_roles_separated: bool,
    local_daemon_subkey_present: bool,
    source_controller_account_id: Option<String>,
    device_role: &'static str,
    device_readiness: &'static str,
    local_root_state: &'static str,
    service_did: String,
    ws: Option<WsStream>,
    held_ticket: Option<HeldTicket>,
    daemon_state_root: Option<std::path::PathBuf>,
    daemon_continuity_baseline: Option<DaemonContinuityBaseline>,
}

#[derive(Deserialize)]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct DownloadTicketResult {
    download_ticket_b64u: String,
    #[serde(default)]
    ticket_binding: Map<String, Value>,
}

enum RpcOutcome<T> {
    Success(T),
    Rejected(Option<&'static str>),
}

fn required_account_state_agent_outcome(outcome: RpcOutcome<Value>) -> Result<Value, ProbeFailure> {
    match outcome {
        RpcOutcome::Success(result) => Ok(result),
        RpcOutcome::Rejected(Some(ACCOUNT_STATE_TEST_FAIL_ONCE)) => {
            Err(ProbeFailure::AccountStateTestFailOnce)
        }
        RpcOutcome::Rejected(_) => Err(ProbeFailure::Transport),
    }
}

enum WsConnectOutcome {
    Connected(Box<WsStream>),
    Rejected(Option<&'static str>),
}

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("{RUNTIME_FAILED}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ProbeFailure> {
    let mut probe = Probe::from_source().await?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = input
            .read_line(&mut line)
            .map_err(|_| ProbeFailure::Runtime)?;
        if bytes == 0 {
            break;
        }

        let (response, shutdown) = if line.len() > MAX_REQUEST_LINE_BYTES {
            (
                failure_response(RequestId(json!(0)), ProbeFailure::InvalidRequest),
                false,
            )
        } else {
            let fallback_id = request_id_or_zero(&line);
            match parse_request(&line) {
                Ok(request) => {
                    let id = request.id.clone();
                    match probe.execute(request.action).await {
                        Ok((result, shutdown)) => (success_response(id, result), shutdown),
                        Err(error) => (failure_response(id, error), false),
                    }
                }
                Err(error) => (failure_response(fallback_id, error), false),
            }
        };

        serde_json::to_writer(&mut output, &response).map_err(|_| ProbeFailure::Runtime)?;
        output.write_all(b"\n").map_err(|_| ProbeFailure::Runtime)?;
        output.flush().map_err(|_| ProbeFailure::Runtime)?;
        if shutdown {
            break;
        }
    }
    Ok(())
}

impl Probe {
    async fn from_source() -> Result<Self, ProbeFailure> {
        match env::var_os(DAEMON_STATE_ROOT_ENV) {
            Some(state_root) => {
                let kind =
                    env::var(DAEMON_AGENT_KIND_ENV).map_err(|_| ProbeFailure::InvalidState)?;
                Self::from_daemon(Path::new(&state_root), &kind).await
            }
            None => {
                if env::var_os(DAEMON_AGENT_KIND_ENV).is_some() {
                    return Err(ProbeFailure::InvalidState);
                }
                Self::from_workspace().await
            }
        }
    }

    async fn from_workspace() -> Result<Self, ProbeFailure> {
        let resolved = awiki_cli::workspace_config::resolve(Default::default())
            .map_err(|_| ProbeFailure::Runtime)?;
        let core = awiki_cli::m_core_cli_adapter::build_im_core_async(&resolved)
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        let device_summary = core
            .identities()
            .device_summary_async(im_core::IdentitySelector::Default)
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        if device_summary.mode != IdentityDeviceMode::VNext {
            return Err(ProbeFailure::InvalidState);
        }
        let local_device_id = device_summary
            .protocol_device_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .ok_or(ProbeFailure::Runtime)?;
        let local_signing_key_id = device_summary
            .signing_key_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ProbeFailure::Runtime)?;
        let local_e2ee_key_id = device_summary
            .e2ee_key_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ProbeFailure::Runtime)?;
        let device_role = match device_summary.role {
            Some(IdentityDeviceRole::Member) => "member",
            Some(IdentityDeviceRole::Admin) => "admin",
            None => "none",
        };
        let device_readiness = match device_summary.readiness {
            IdentityDeviceReadiness::Legacy => "legacy",
            IdentityDeviceReadiness::MemberReady => "member_ready",
            IdentityDeviceReadiness::AdminAwaitingRoot => "admin_awaiting_root",
            IdentityDeviceReadiness::AdminReady => "admin_ready",
            IdentityDeviceReadiness::Blocked => "blocked",
        };
        let local_root_state = if matches!(
            device_summary.readiness,
            IdentityDeviceReadiness::AdminReady
        ) {
            "active"
        } else {
            "not_active"
        };
        let client = core
            .client_async(im_core::IdentitySelector::Default)
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        let mut session = client
            .auth()
            .ensure_session_async(im_core::auth::AuthScope::Messaging)
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        let bearer = session
            .bearer_token
            .take()
            .filter(|value| !value.trim().is_empty())
            .map(Zeroizing::new)
            .ok_or(ProbeFailure::Runtime)?;
        let local_did = client.did().as_str().to_owned();
        let binding = client
            .active_sync_account_binding()
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        if binding.current_did != local_did || binding.protocol_device_id != local_device_id {
            return Err(ProbeFailure::Runtime);
        }
        let local_root_key_id = format!("{local_did}#key-1");
        let local_document = workspace_manifest_projection(
            Path::new(&resolved.paths.identity_dir),
            &local_did,
            &local_device_id,
            &local_root_key_id,
            &local_signing_key_id,
            &local_e2ee_key_id,
        );
        let service_did = im_core::ids::Did::parse(&resolved.anp_service_did)
            .map_err(|_| ProbeFailure::Runtime)?
            .as_str()
            .to_owned();
        let message_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.message_service_endpoint,
            RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&message_rpc_url)?;
        let account_state_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.user_service_endpoint,
            ACCOUNT_STATE_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&account_state_rpc_url)?;
        let agent_inventory_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.user_service_endpoint,
            AGENT_INVENTORY_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&agent_inventory_rpc_url)?;
        let agent_registration_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.user_service_endpoint,
            AGENT_REGISTRATION_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&agent_registration_rpc_url)?;
        let did_auth_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.user_service_endpoint,
            DID_AUTH_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&did_auth_rpc_url)?;
        let me_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &resolved.user_service_endpoint,
            ME_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&me_rpc_url)?;
        let websocket_url = im_core::realtime::realtime_client_construction_plan(
            &resolved.message_service_endpoint,
        )
        .map_err(|_| ProbeFailure::Runtime)?
        .endpoints
        .websocket_url;
        let ca_bundle = non_empty(&resolved.ca_bundle);
        let http = build_http_client(ca_bundle.as_deref())?;

        Ok(Self {
            _core: Some(core),
            _client: Some(client),
            http,
            bearer,
            message_rpc_url,
            account_state_rpc_url,
            agent_inventory_rpc_url,
            agent_registration_rpc_url,
            did_auth_rpc_url,
            me_rpc_url,
            websocket_url,
            ca_bundle,
            local_did,
            local_account_id: binding.account_id,
            local_device_id,
            local_signing_key_id,
            local_e2ee_key_id,
            local_binding_generation: binding.identity_generation,
            local_device_auth_generation: binding.device_auth_generation,
            local_manifest_single_device: local_document.manifest_single_device,
            local_document_hash: local_document.document_hash,
            local_key_roles_separated: local_document.key_roles_separated,
            local_daemon_subkey_present: local_document.daemon_subkey_present,
            source_controller_account_id: None,
            device_role,
            device_readiness,
            local_root_state,
            service_did,
            ws: None,
            held_ticket: None,
            daemon_state_root: None,
            daemon_continuity_baseline: None,
        })
    }

    async fn from_daemon(state_root: &Path, kind: &str) -> Result<Self, ProbeFailure> {
        let expected_kind =
            awiki_deamon::agent::AgentKind::parse(kind).map_err(|_| ProbeFailure::InvalidState)?;
        let config = awiki_deamon::DaemonConfig::for_state_root(state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        config.validate().map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let definitions = state
            .list_agent_definitions()
            .map_err(|_| ProbeFailure::Runtime)?
            .into_iter()
            .filter(|item| item.status == "active" && item.agent_kind == expected_kind)
            .collect::<Vec<_>>();
        if definitions.len() != 1 {
            return Err(ProbeFailure::InvalidState);
        }
        let definition = &definitions[0];
        let identity = state
            .load_agent_device_identity(&definition.agent_did)
            .map_err(|_| ProbeFailure::Runtime)?
            .ok_or(ProbeFailure::InvalidState)?;
        identity.validate().map_err(|_| ProbeFailure::Runtime)?;
        if identity.agent_kind != expected_kind || identity.agent_did != definition.agent_did {
            return Err(ProbeFailure::InvalidState);
        }
        let mut local_document = project_local_document(
            &identity.did_document,
            &identity.agent_did,
            &identity.protocol_device_id,
            &identity.root_key_id,
            &identity.device_signing_key_id,
            &identity.device_e2ee_key_id,
        );
        if local_document.document_hash.as_deref() != Some(identity.document_hash.as_str()) {
            local_document.document_hash = None;
        }
        let adapter =
            awiki_deamon::ImCoreAdapter::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let client = adapter
            .client_for_agent_device_identity(&identity)
            .map_err(|_| ProbeFailure::Runtime)?;
        let binding = client
            .active_sync_account_binding()
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        if binding.account_id != identity.account_id
            || binding.current_did != identity.agent_did
            || binding.protocol_device_id != identity.protocol_device_id
            || binding.identity_generation != identity.binding_generation
            || binding.device_auth_generation != identity.auth_generation.to_string()
        {
            return Err(ProbeFailure::Runtime);
        }
        let mut session = client
            .auth()
            .ensure_session_async(im_core::auth::AuthScope::Messaging)
            .await
            .map_err(|_| ProbeFailure::Runtime)?;
        let bearer = session
            .bearer_token
            .take()
            .filter(|value| !value.trim().is_empty())
            .map(Zeroizing::new)
            .ok_or(ProbeFailure::Runtime)?;
        let service_did = im_core::ids::Did::parse(&config.anp_service_did)
            .map_err(|_| ProbeFailure::Runtime)?
            .as_str()
            .to_owned();
        let message_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.message_service_base_url,
            RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        validate_service_url(&message_rpc_url)?;
        let account_state_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.user_service_base_url,
            ACCOUNT_STATE_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        let agent_inventory_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.user_service_base_url,
            AGENT_INVENTORY_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        let agent_registration_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.user_service_base_url,
            AGENT_REGISTRATION_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        let did_auth_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.user_service_base_url,
            DID_AUTH_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        let me_rpc_url = reqwest::Url::parse(&im_core::realtime::join_base_url(
            &config.user_service_base_url,
            ME_RPC_PATH,
        ))
        .map_err(|_| ProbeFailure::Runtime)?;
        for url in [
            &account_state_rpc_url,
            &agent_inventory_rpc_url,
            &agent_registration_rpc_url,
            &did_auth_rpc_url,
            &me_rpc_url,
        ] {
            validate_service_url(url)?;
        }
        let websocket_url =
            im_core::realtime::realtime_client_construction_plan(&config.message_service_base_url)
                .map_err(|_| ProbeFailure::Runtime)?
                .endpoints
                .websocket_url;

        Ok(Self {
            _core: None,
            _client: Some(client),
            http: build_http_client(None)?,
            bearer,
            message_rpc_url,
            account_state_rpc_url,
            agent_inventory_rpc_url,
            agent_registration_rpc_url,
            did_auth_rpc_url,
            me_rpc_url,
            websocket_url,
            ca_bundle: None,
            local_did: identity.agent_did,
            local_account_id: identity.account_id,
            local_device_id: identity.protocol_device_id,
            local_signing_key_id: identity.device_signing_key_id,
            local_e2ee_key_id: identity.device_e2ee_key_id,
            local_binding_generation: identity.binding_generation,
            local_device_auth_generation: identity.auth_generation.to_string(),
            local_manifest_single_device: local_document.manifest_single_device,
            local_document_hash: local_document.document_hash,
            local_key_roles_separated: local_document.key_roles_separated,
            local_daemon_subkey_present: local_document.daemon_subkey_present,
            source_controller_account_id: Some(definition.controller_user_id.clone()),
            device_role: "admin",
            device_readiness: "admin_ready",
            local_root_state: "active",
            service_did,
            ws: None,
            held_ticket: None,
            daemon_state_root: Some(state_root.to_path_buf()),
            daemon_continuity_baseline: None,
        })
    }

    async fn execute(&mut self, action: Action) -> Result<(Value, bool), ProbeFailure> {
        match action {
            Action::DeviceReadiness => Ok((
                json!({
                    "protocol_device_id_matches_current": true,
                    "role": self.device_role,
                    "readiness": self.device_readiness,
                    "local_root_state": self.local_root_state,
                }),
                false,
            )),
            Action::AgentBootstrapIdentity(params) => {
                let registry = self
                    .required_user_rpc(&self.did_auth_rpc_url, "device_registry_get", json!({}))
                    .await?;
                let manifest = self
                    .required_user_rpc(
                        &self.account_state_rpc_url,
                        "account_state.manifest_get",
                        json!({}),
                    )
                    .await?;
                let (device_access_standard, sync_capability_absent) =
                    device_access_projection_matches(
                        &self.bearer,
                        &self.local_did,
                        &self.local_account_id,
                        &self.local_device_id,
                        &self.local_signing_key_id,
                        &self.local_device_auth_generation,
                    );
                Ok((
                    json!({
                        "agent_account_independent": self.local_account_id != params.controller_account_id,
                        "controller_binding_matches": controller_binding_projection(
                            self.source_controller_account_id.as_deref(),
                            &params.controller_account_id,
                        ),
                        "manifest_single_device": self.local_manifest_single_device,
                        "registry_single_ready_admin": bootstrap_registry_matches(
                            &registry,
                            &self.local_did,
                            &self.local_device_id,
                            &self.local_signing_key_id,
                            &self.local_e2ee_key_id,
                            &self.local_device_auth_generation,
                            self.local_document_hash.as_deref().unwrap_or(""),
                        ),
                        "key_roles_separated": self.local_key_roles_separated,
                        "bootstrap_generations_one": self.local_binding_generation == "1"
                            && self.local_device_auth_generation == "1"
                            && bootstrap_manifest_matches(
                                &manifest,
                                &self.local_account_id,
                                &self.local_did,
                                &self.local_binding_generation,
                            ),
                        "device_access_standard": device_access_standard,
                        "sync_capability_absent": sync_capability_absent,
                    }),
                    false,
                ))
            }
            Action::DaemonContinuityBaseline => {
                let baseline = self.capture_daemon_continuity_baseline()?;
                self.daemon_continuity_baseline = Some(baseline);
                Ok((
                    json!({
                        "captured": true,
                        "old_identity_has_daemon_key": true,
                        "delegated_identity_ready": true,
                        "controller_binding_ready": true,
                    }),
                    false,
                ))
            }
            Action::DaemonContinuityVerify(params) => {
                let result = self.verify_daemon_continuity(&params)?;
                Ok((result, false))
            }
            Action::StageDaemonContinuityRoot(params) => {
                let result = self.stage_daemon_continuity_root(&params)?;
                Ok((result, false))
            }
            Action::PrepareDaemonContinuityFixture(params) => {
                let result = self.prepare_daemon_continuity_fixture(&params).await?;
                Ok((result, false))
            }
            Action::DaemonFixtureResources => Ok((self.daemon_fixture_resources()?, false)),
            Action::SendPlainMarker(params) => {
                let result = self.send_plain_marker(&params).await?;
                Ok((result, false))
            }
            Action::DaemonMarkerProcessed { message_id } => {
                Ok((self.daemon_marker_processed(&message_id)?, false))
            }
            Action::HumanDaemonSubkeyState => {
                let core = self._core.as_ref().ok_or(ProbeFailure::InvalidState)?;
                let vault_present = core
                    .identities()
                    .load_daemon_subkey_package_async(im_core::IdentitySelector::Default)
                    .await
                    .is_ok();
                Ok((
                    json!({
                        "document_present": self.local_daemon_subkey_present,
                        "vault_present": vault_present,
                    }),
                    false,
                ))
            }
            Action::OpenWs => {
                if self.ws.is_some() {
                    return Err(ProbeFailure::InvalidState);
                }
                match self.connect_ws().await? {
                    WsConnectOutcome::Connected(stream) => {
                        self.ws = Some(*stream);
                        Ok((json!({"opened": true}), false))
                    }
                    WsConnectOutcome::Rejected(_) => Err(ProbeFailure::Transport),
                }
            }
            Action::WaitWsClosed { timeout_ms } => {
                let stream = self.ws.as_mut().ok_or(ProbeFailure::InvalidState)?;
                let closed = wait_ws_closed(stream, timeout_ms).await;
                if closed {
                    self.ws.take();
                }
                Ok((json!({"closed": closed}), false))
            }
            Action::CloseWs => {
                if let Some(mut stream) = self.ws.take() {
                    let _ = stream.close(None).await;
                }
                Ok((json!({"closed": true}), false))
            }
            Action::ReconnectWs => match self.connect_ws().await? {
                WsConnectOutcome::Connected(mut stream) => {
                    let _ = stream.as_mut().close(None).await;
                    Ok((json!({"connected": true}), false))
                }
                WsConnectOutcome::Rejected(code) => {
                    Ok((result_with_code("connected", false, code), false))
                }
            },
            Action::HoldDownloadTicket(params) => {
                match self.request_held_download_ticket(&params).await? {
                    RpcOutcome::Success(held) => {
                        self.held_ticket = Some(held);
                        Ok((json!({"held": true}), false))
                    }
                    RpcOutcome::Rejected(_) => Ok((json!({"held": false}), false)),
                }
            }
            Action::ProbeDownloadTicket(params) => {
                match self.request_held_download_ticket(&params).await? {
                    RpcOutcome::Success(held) => {
                        drop(held);
                        Ok((json!({"allowed": true}), false))
                    }
                    RpcOutcome::Rejected(code) => {
                        Ok((result_with_code("allowed", false, code), false))
                    }
                }
            }
            Action::ProbePrekey(params) => match self.request_prekey(&params).await? {
                RpcOutcome::Success(()) => Ok((json!({"available": true}), false)),
                RpcOutcome::Rejected(code) => {
                    Ok((result_with_code("available", false, code), false))
                }
            },
            Action::DirectWireProjection(params) => {
                Ok((self.direct_wire_projection(&params).await?, false))
            }
            Action::AccountStateManifest => {
                let result = self
                    .required_user_rpc(
                        &self.account_state_rpc_url,
                        "account_state.manifest_get",
                        json!({}),
                    )
                    .await?;
                Ok((closed_manifest_result(&result)?, false))
            }
            Action::AccountStateAgent(params) => {
                let result = self.required_account_state_agent_rpc().await?;
                Ok((closed_agent_result(&result, &params)?, false))
            }
            Action::AccountStateAgentRename(params) => {
                let result = self
                    .required_user_rpc(
                        &self.agent_inventory_rpc_url,
                        "update_display_name",
                        json!({
                            "agent_did": params.agent_did,
                            "display_name": params.display_name,
                        }),
                    )
                    .await?;
                Ok((closed_agent_rename_result(&result, &params)?, false))
            }
            Action::AccountStateAgentConfig(params) => {
                let result = self
                    .required_user_rpc(
                        &self.agent_inventory_rpc_url,
                        "update_invocation_policy",
                        json!({
                            "agent_did": params.agent_did,
                            "active_mode": params.active_mode,
                            "whitelist_handles": params.whitelist_handles,
                            "blacklist_handles": params.blacklist_handles,
                        }),
                    )
                    .await?;
                Ok((closed_agent_config_result(&result, &params)?, false))
            }
            Action::AccountStateAgentUnbind(params) => {
                let result = self
                    .required_user_rpc(
                        &self.agent_inventory_rpc_url,
                        "unbind_agent",
                        json!({"agent_did": params.agent_did}),
                    )
                    .await?;
                Ok((closed_agent_unbind_result(&result)?, false))
            }
            Action::AccountStateAgentRemove(params) => {
                let result = self
                    .required_user_rpc(
                        &self.agent_inventory_rpc_url,
                        "remove_agent_from_account",
                        json!({"agent_did": params.agent_did}),
                    )
                    .await?;
                Ok((closed_agent_remove_result(&result, &params)?, false))
            }
            Action::AccountStateStatus(params) => {
                let result = self
                    .required_user_rpc(
                        &self.account_state_rpc_url,
                        "account_state.agent_status_get",
                        json!({}),
                    )
                    .await?;
                Ok((closed_agent_status_result(&result, &params)?, false))
            }
            Action::AccountStateProfile(params) => {
                let result = self
                    .required_user_rpc(
                        &self.account_state_rpc_url,
                        "account_state.profile_get",
                        json!({}),
                    )
                    .await?;
                Ok((closed_profile_result(&result, &params)?, false))
            }
            Action::AccountStateProfileUpdate(params) => {
                let result = self
                    .required_user_rpc(
                        &self.me_rpc_url,
                        "update_me",
                        json!({"nick_name": params.nick_name}),
                    )
                    .await?;
                Ok((closed_profile_update_result(&result, &params)?, false))
            }
            Action::AccountStateRegistry(params) => {
                let result = self
                    .required_user_rpc(&self.did_auth_rpc_url, "device_registry_get", json!({}))
                    .await?;
                Ok((closed_registry_result(&result, &params)?, false))
            }
            Action::RedeemHeldTicket {
                expected_digest_b64u,
            } => {
                let result = self.redeem_held_ticket(&expected_digest_b64u).await?;
                Ok((result, false))
            }
            Action::Shutdown => {
                if let Some(mut stream) = self.ws.take() {
                    let _ = stream.close(None).await;
                }
                self.held_ticket.take();
                Ok((json!({"shutdown": true}), true))
            }
        }
    }

    fn capture_daemon_continuity_baseline(&self) -> Result<DaemonContinuityBaseline, ProbeFailure> {
        let state_root = self
            .daemon_state_root
            .as_deref()
            .ok_or(ProbeFailure::InvalidState)?;
        let config = awiki_deamon::DaemonConfig::for_state_root(state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let definition = state
            .list_agent_definitions()
            .map_err(|_| ProbeFailure::Runtime)?
            .into_iter()
            .find(|item| item.agent_did == self.local_did && item.status == "active")
            .ok_or(ProbeFailure::InvalidState)?;
        let identity = state
            .load_agent_device_identity(&self.local_did)
            .map_err(|_| ProbeFailure::Runtime)?
            .ok_or(ProbeFailure::InvalidState)?;
        let bindings = state
            .list_active_app_personal_agent_bindings()
            .map_err(|_| ProbeFailure::Runtime)?;
        if bindings.len() != 1 {
            return Err(ProbeFailure::InvalidState);
        }
        let binding = &bindings[0];
        if binding.daemon_agent_did != self.local_did
            || binding.user_did != definition.controller_did
            || binding.revoked_at_ms.is_some()
        {
            return Err(ProbeFailure::InvalidState);
        }
        let delegated = state
            .load_user_delegated_identity(&binding.inbox_auth_verification_method)
            .map_err(|_| ProbeFailure::Runtime)?
            .ok_or(ProbeFailure::InvalidState)?;
        if delegated.user_did != binding.user_did
            || delegated.controller_did != definition.controller_did
            || delegated.daemon_agent_did != self.local_did
            || delegated.verification_method != binding.inbox_auth_verification_method
            || !delegated.verification_method.ends_with("#daemon-key-1")
            || delegated.private_key_material.trim().is_empty()
            || delegated.status != "active"
        {
            return Err(ProbeFailure::InvalidState);
        }
        let route_state = daemon_route_state_snapshot(&state)?;

        Ok(DaemonContinuityBaseline {
            agent_identity_hash: hash_serializable(&identity)?,
            root_key_hash: hash_parts(&[&identity.root_key_id, &identity.root_private_key_pem]),
            device_keys_hash: hash_parts(&[
                &identity.device_signing_key_id,
                &identity.device_signing_private_key_pem,
                &identity.device_e2ee_key_id,
                &identity.device_e2ee_private_key_pem,
            ]),
            delegated_key_hash: hash_serializable(&delegated)?,
            definition_hash: hash_serializable(&definition)?,
            route_state_hash: route_state.hash,
            route_record_count: route_state.record_count,
            verification_method: delegated.verification_method,
            user_did: binding.user_did.clone(),
            app_instance_id: binding.app_instance_id.clone(),
        })
    }

    fn verify_daemon_continuity(
        &self,
        params: &DaemonContinuityVerifyParams,
    ) -> Result<Value, ProbeFailure> {
        let baseline = self
            .daemon_continuity_baseline
            .as_ref()
            .ok_or(ProbeFailure::InvalidState)?;
        let state_root = self
            .daemon_state_root
            .as_deref()
            .ok_or(ProbeFailure::InvalidState)?;
        let config = awiki_deamon::DaemonConfig::for_state_root(state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let definition = state
            .list_agent_definitions()
            .map_err(|_| ProbeFailure::Runtime)?
            .into_iter()
            .find(|item| item.agent_did == self.local_did && item.status == "active")
            .ok_or(ProbeFailure::InvalidState)?;
        let identity = state
            .load_agent_device_identity(&self.local_did)
            .map_err(|_| ProbeFailure::Runtime)?
            .ok_or(ProbeFailure::InvalidState)?;
        let bindings = state
            .list_active_app_personal_agent_bindings()
            .map_err(|_| ProbeFailure::Runtime)?;
        let old_binding = bindings.iter().find(|binding| {
            binding.daemon_agent_did == self.local_did
                && binding.user_did == params.old_controller_did
                && binding.app_instance_id == baseline.app_instance_id
                && binding.revoked_at_ms.is_none()
        });
        let delegated = state
            .load_user_delegated_identity(&baseline.verification_method)
            .map_err(|_| ProbeFailure::Runtime)?
            .ok_or(ProbeFailure::InvalidState)?;
        let controller_identity_changed =
            awiki_deamon::agent_status::controller_identity_change_observed(
                &state,
                &self.local_did,
            )
            .map_err(|_| ProbeFailure::Runtime)?;
        let route_state = daemon_route_state_snapshot(&state)?;
        let route_state_unchanged = route_state.hash == baseline.route_state_hash
            && route_state.record_count == baseline.route_record_count;
        let queued_ids =
            queued_delegated_marker_ids(&params.old_controller_did, &params.queued_marker)?;
        let new_controller_ids = new_controller_marker_ids(&params.controller_marker)?;
        let mut queued_delegated_marker = daemon_marker_absence_evidence(&state, &queued_ids)?;
        let mut new_controller_marker =
            daemon_marker_absence_evidence(&state, &new_controller_ids)?;
        queued_delegated_marker.route_absent &= route_state_unchanged;
        new_controller_marker.route_absent &= route_state_unchanged;

        let agent_identity_unchanged =
            hash_serializable(&identity)? == baseline.agent_identity_hash;
        let root_key_unchanged =
            hash_parts(&[&identity.root_key_id, &identity.root_private_key_pem])
                == baseline.root_key_hash;
        let device_keys_unchanged = hash_parts(&[
            &identity.device_signing_key_id,
            &identity.device_signing_private_key_pem,
            &identity.device_e2ee_key_id,
            &identity.device_e2ee_private_key_pem,
        ]) == baseline.device_keys_hash;
        let delegated_key_unchanged = hash_serializable(&delegated)? == baseline.delegated_key_hash
            && delegated.user_did == baseline.user_did;
        let old_controller_binding_unchanged = old_binding.is_some()
            && definition.controller_did == params.old_controller_did
            && hash_serializable(&definition)? == baseline.definition_hash;
        let new_controller_lacks_delegated_key = params.new_controller_did != baseline.user_did
            && delegated.user_did == baseline.user_did
            && !bindings
                .iter()
                .any(|binding| binding.user_did == params.new_controller_did);
        Ok(closed_daemon_continuity_result(DaemonContinuityEvidence {
            agent_identity_unchanged,
            root_key_unchanged,
            device_keys_unchanged,
            delegated_key_unchanged,
            old_controller_binding_unchanged,
            new_controller_lacks_delegated_key,
            controller_identity_changed,
            queued_delegated_marker,
            new_controller_marker,
        }))
    }

    fn stage_daemon_continuity_root(
        &self,
        params: &StageDaemonContinuityRootParams,
    ) -> Result<Value, ProbeFailure> {
        if !params.state_root.is_absolute() || params.state_root.exists() {
            return Err(ProbeFailure::InvalidRequest);
        }
        let config = awiki_deamon::DaemonConfig::for_state_root(&params.state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        config
            .ensure_state_layout()
            .map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        state.initialize().map_err(|_| ProbeFailure::Runtime)?;
        let authority =
            awiki_deamon::commands::stage_daemon_registration_authority_for_system_test(
                &config,
                &state,
                &self.local_did,
                &params.daemon_handle,
            )
            .map_err(|_| ProbeFailure::Runtime)?;
        im_core::ids::Did::parse(&authority.agent_did).map_err(|_| ProbeFailure::Runtime)?;
        Ok(json!({"daemon_agent_did": authority.agent_did}))
    }

    async fn prepare_daemon_continuity_fixture(
        &self,
        params: &PrepareDaemonContinuityFixtureParams,
    ) -> Result<Value, ProbeFailure> {
        let core = self._core.as_ref().ok_or(ProbeFailure::InvalidState)?;
        let client = self._client.as_ref().ok_or(ProbeFailure::InvalidState)?;
        if !params.daemon_binary.is_file()
            || !params.daemon_binary.is_absolute()
            || !params.state_root.is_absolute()
            || !params.state_root.is_dir()
        {
            return Err(ProbeFailure::InvalidRequest);
        }

        let user_service_base = service_base_from_rpc(&self.agent_registration_rpc_url)?;
        let message_service_base = service_base_from_rpc(&self.message_rpc_url)?;
        let authority_config = awiki_deamon::DaemonConfig::for_state_root(&params.state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        let authority_state = awiki_deamon::DaemonState::open(&authority_config)
            .map_err(|_| ProbeFailure::Runtime)?;
        let authority = awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
            &authority_state,
            &self.local_did,
            &params.daemon_handle,
            &params.daemon_agent_did,
        )
        .map_err(|_| ProbeFailure::InvalidState)?;
        im_core::ids::Did::parse(&authority.agent_did).map_err(|_| ProbeFailure::Runtime)?;
        let daemon_token = match daemon_token_issue_or_root_receipt(
            &authority.agent_did,
            self.issue_agent_registration_token(
                "daemon",
                &params.daemon_handle,
                Some(&params.controller_handle),
                daemon_registration_metadata(&authority.agent_did),
            )
            .await,
        ) {
            Ok(token) => token,
            Err(receipt) => return Ok(receipt),
        };
        let registration_token = match awiki_deamon::registration::RegistrationToken::new(
            daemon_token.as_str().to_owned(),
        ) {
            Ok(token) => token,
            Err(_) => {
                return Ok(closed_daemon_fixture_prepare_result(
                    false,
                    &authority.agent_did,
                    Some(DaemonFixturePrepareFailureStage::Token),
                ));
            }
        };
        if awiki_deamon::commands::bind_daemon_registration_token_for_system_test(
            &authority_state,
            &authority,
            registration_token,
        )
        .is_err()
        {
            return Ok(closed_daemon_fixture_prepare_result(
                false,
                &authority.agent_did,
                Some(DaemonFixturePrepareFailureStage::Token),
            ));
        }
        let mut daemon_agent_did = None;
        for _ in 0..DAEMON_SETUP_ATTEMPT_LIMIT {
            let daemon_binary = params.daemon_binary.clone();
            let state_root = params.state_root.clone();
            let daemon_handle = params.daemon_handle.clone();
            let controller_did = self.local_did.clone();
            let registration_token = Zeroizing::new(daemon_token.as_str().to_owned());
            let user_service_base = user_service_base.clone();
            let message_service_base = message_service_base.clone();
            let service_did = self.service_did.clone();
            let setup_output = tokio::task::spawn_blocking(move || {
                Command::new(daemon_binary)
                    .arg("setup-daemon-agent")
                    .arg("--state-root")
                    .arg(state_root)
                    .arg("--handle")
                    .arg(daemon_handle)
                    .arg("--controller-did")
                    .arg(controller_did)
                    .env(
                        "AWIKI_DAEMON_REGISTRATION_TOKEN",
                        registration_token.as_str(),
                    )
                    .env("AWIKI_DAEMON_BASE_URL", &user_service_base)
                    .env("AWIKI_DAEMON_USER_SERVICE_BASE_URL", &user_service_base)
                    .env(
                        "AWIKI_DAEMON_MESSAGE_SERVICE_BASE_URL",
                        &message_service_base,
                    )
                    .env("AWIKI_DAEMON_ANP_SERVICE_DID", service_did)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
            })
            .await;
            if let Ok(Ok(setup_output)) = setup_output {
                if let Ok(agent_did) = daemon_setup_agent_did(setup_output) {
                    if recover_exact_persisted_daemon_agent_did(
                        &params.state_root,
                        &params.daemon_handle,
                        &self.local_did,
                    )
                    .is_ok_and(|persisted| persisted.as_deref() == Some(agent_did.as_str()))
                    {
                        daemon_agent_did = Some(agent_did);
                        break;
                    }
                }
            }
        }
        let daemon_agent_did = match daemon_agent_did {
            Some(agent_did) => agent_did,
            None => {
                return daemon_setup_failure_result(
                    &params.state_root,
                    &params.daemon_handle,
                    &self.local_did,
                    ProbeFailure::Runtime,
                )
            }
        };
        if daemon_agent_did != authority.agent_did {
            return Ok(closed_daemon_fixture_prepare_result(
                false,
                &authority.agent_did,
                Some(DaemonFixturePrepareFailureStage::Setup),
            ));
        }

        let preparation = async {
            let config = awiki_deamon::DaemonConfig::for_state_root(&params.state_root)
                .map_err(|_| ProbeFailure::Runtime)?;
            let state =
                awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
            let daemon_identity = state
                .load_agent_device_identity(&daemon_agent_did)
                .map_err(|_| ProbeFailure::Runtime)?
                .ok_or(ProbeFailure::Runtime)?;
            let daemon_adapter =
                awiki_deamon::ImCoreAdapter::open(&config).map_err(|_| ProbeFailure::Runtime)?;
            let daemon_client = daemon_adapter
                .client_for_agent_device_identity(&daemon_identity)
                .map_err(|_| ProbeFailure::Runtime)?;
            let recipient_public = daemon_bootstrap_public_key(
                &daemon_identity.did_document,
                &daemon_agent_did,
                &daemon_identity.device_e2ee_key_id,
            )?;

            let runtime_token = self
                .issue_agent_registration_token(
                    "runtime",
                    &params.runtime_handle,
                    None,
                    json!({
                        "suite_case": "handle-recovery-daemon-continuity",
                        "runtime": "hermes",
                        "runtime_profile": "personal_agent",
                        "daemon_agent_did": daemon_agent_did,
                    }),
                )
                .await?;
            let mut subkey = core
                .identities()
                .ensure_daemon_subkey_package_async(im_core::IdentitySelector::Default)
                .await
                .map_err(|_| ProbeFailure::Runtime)?;
            if subkey.user_did.as_str() != self.local_did
                || !subkey.verification_method.ends_with("#daemon-key-1")
                || !subkey.is_v2_pem()
            {
                return Err(ProbeFailure::InvalidState.into());
            }
            let private_key = Zeroizing::new(std::mem::take(&mut subkey.private_key_pem));
            let legacy_private_key =
                Zeroizing::new(std::mem::take(&mut subkey.private_key_multibase));
            let private_key_material = if private_key.trim().is_empty() {
                legacy_private_key.as_str()
            } else {
                private_key.as_str()
            };
            if private_key_material.trim().is_empty() {
                return Err(ProbeFailure::InvalidState.into());
            }
            let bootstrap_id = format!("boot_{}", random_hex(12)?);
            let idempotency_key = format!("personal-agent-bootstrap:{}", random_hex(12)?);
            let ensure_once_key = format!(
                "app-personal-agent:{}:{}",
                self.local_did, params.app_instance_id
            );
            let payload = ProbeBootstrapPayload {
                schema: "awiki.daemon.bootstrap.v1",
                bootstrap_id: &bootstrap_id,
                idempotency_key: &idempotency_key,
                app_instance_id: &params.app_instance_id,
                controller_did: &self.local_did,
                user_subkey_package: ProbeUserSubkeyPackage {
                    schema: &subkey.schema,
                    user_did: subkey.user_did.as_str(),
                    verification_method: &subkey.verification_method,
                    key_type: &subkey.key_type,
                    key_algorithm: subkey.key_algorithm.as_deref().unwrap_or("Ed25519"),
                    public_key_multibase: &subkey.public_key_multibase,
                    private_key_encoding: &subkey.private_key_encoding,
                    private_key_pem: private_key_material,
                    allowed_scopes: ["message.inbox.read.plain"],
                },
                desired_personal_agent: ProbeDesiredPersonalAgent {
                    role: "app_message_handler",
                    runtime: "hermes",
                    runtime_provider: "hermes",
                    runtime_profile: "personal_agent",
                    display_name: "Recovery Continuity Agent",
                    preferred_language: "zh-Hans",
                    ensure_once_key: &ensure_once_key,
                    runtime_registration_token: runtime_token.as_str(),
                },
                capability_policy: ProbeCapabilityPolicy {
                    schema: "awiki.app.capabilities.v1",
                    capabilities: ["message.summarize_plain"],
                    require_confirmation_for_write_actions: true,
                },
            };
            let plaintext =
                Zeroizing::new(serde_json::to_vec(&payload).map_err(|_| ProbeFailure::Runtime)?);
            let now = time::OffsetDateTime::now_utc();
            let issued_at = now
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| ProbeFailure::Runtime)?;
            let expires_at = (now + time::Duration::minutes(5))
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| ProbeFailure::Runtime)?;
            let mut nonce = [0_u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce);
            let mut ephemeral = [0_u8; 32];
            rand::thread_rng().fill_bytes(&mut ephemeral);
            let operation_id = idempotency_key.clone();
            let envelope =
            awiki_deamon::app_bridge::bootstrap::encrypt_secure_bootstrap_bytes_for_system_test(
                &daemon_agent_did,
                recipient_public,
                &self.local_did,
                &operation_id,
                &issued_at,
                &expires_at,
                nonce,
                x25519_dalek::StaticSecret::from(ephemeral),
                json!({
                    "human_did": self.local_did,
                    "daemon_agent_did": daemon_agent_did,
                    "binding_id": format!(
                        "app-personal-agent:{}:{}",
                        self.local_did, params.app_instance_id
                    ),
                }),
                plaintext.as_slice(),
            )
            .map_err(|_| ProbeFailure::Runtime)?;
            let message_id = im_core::ids::MessageId::parse(&format!("msg-{}", random_hex(12)?))
                .map_err(|_| ProbeFailure::Runtime)?;
            let send_request = im_core::messages::SendMessageRequest {
                target: im_core::messages::MessageTarget::Direct(
                    im_core::ids::PeerRef::parse(&daemon_agent_did, "")
                        .map_err(|_| ProbeFailure::Runtime)?,
                ),
                body: im_core::messages::MessageBody::Payload { payload: envelope },
                security: im_core::messages::MessageSecurityMode::Plain,
                client_message_id: Some(message_id),
                delivery: im_core::messages::MessageDeliveryOptions {
                    idempotency_key: Some(operation_id),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            };
            daemon_fixture_sync_before_send(
                || async {
                    let outcome = daemon_client
                        .messages()
                        .sync_now_async(im_core::messages::MessageSyncRequest {
                            reason: "system_test_fixture_initialize".to_owned(),
                            limit: Some(100),
                        })
                        .await
                        .map_err(|_| ProbeFailure::Runtime)?;
                    Ok(outcome.status)
                },
                move || async move {
                    let send = client
                        .messages()
                        .send_async(send_request)
                        .await
                        .map_err(|_| ProbeFailure::Transport)?;
                    if matches!(
                        send.delivery,
                        im_core::messages::DeliveryState::Failed { .. }
                    ) {
                        return Err(ProbeFailure::Transport);
                    }
                    Ok(())
                },
            )
            .await
        }
        .await;
        Ok(daemon_fixture_prepare_result_after_boundary(
            &daemon_agent_did,
            preparation,
        ))
    }

    fn daemon_fixture_resources(&self) -> Result<Value, ProbeFailure> {
        let state_root = self
            .daemon_state_root
            .as_deref()
            .ok_or(ProbeFailure::InvalidState)?;
        let config = awiki_deamon::DaemonConfig::for_state_root(state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let bindings = state
            .list_active_app_personal_agent_bindings()
            .map_err(|_| ProbeFailure::Runtime)?;
        let binding_contract_valid = bindings.len() == 1
            && bindings[0].daemon_agent_did == self.local_did
            && im_core::ids::Did::parse(&bindings[0].runtime_agent_did).is_ok();
        if !binding_contract_valid {
            if !state
                .audit_event_exists("daemon.bootstrap.received", Some(&self.local_did), None)
                .map_err(|_| ProbeFailure::Runtime)?
            {
                let definition = match state.load_agent_definition(&self.local_did) {
                    Ok(definition)
                        if definition.validate().is_ok()
                            && definition.agent_did == self.local_did
                            && definition.agent_kind == awiki_deamon::agent::AgentKind::Daemon
                            && definition.status == "active" =>
                    {
                        definition
                    }
                    Ok(_) | Err(_) => {
                        return Err(ProbeFailure::DaemonFixtureBootstrapValidationOrPersist)
                    }
                };
                let verification_method = format!("{}#daemon-key-1", definition.controller_did);
                return match state.load_user_delegated_identity(&verification_method) {
                    Ok(Some(identity))
                        if identity.validate().is_ok()
                            && identity.user_did == definition.controller_did
                            && identity.controller_did == definition.controller_did
                            && identity.daemon_agent_did == self.local_did
                            && identity.verification_method == verification_method
                            && identity.status
                                == awiki_deamon::app_bridge::bootstrap::DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED =>
                    {
                        Err(ProbeFailure::DaemonFixtureBootstrapReceivedAudit)
                    }
                    Ok(Some(_)) | Err(_) => {
                        Err(ProbeFailure::DaemonFixtureBootstrapValidationOrPersist)
                    }
                    Ok(None) => {
                        if state
                            .secure_bootstrap_replay_exists_for_scope(
                                &definition.controller_did,
                                &self.local_did,
                            )
                            .map_err(|_| ProbeFailure::Runtime)?
                        {
                            Err(ProbeFailure::DaemonFixtureBootstrapStatePersist)
                        } else if state
                            .audit_event_exists(
                                "daemon.inbox.message.route.failed",
                                Some(&self.local_did),
                                None,
                            )
                            .map_err(|_| ProbeFailure::Runtime)?
                        {
                            Err(ProbeFailure::DaemonFixtureBootstrapSecureEnvelope)
                        } else {
                            Err(ProbeFailure::DaemonFixtureBootstrapMessageNotRouted)
                        }
                    }
                };
            }
            if !state
                .audit_event_exists(
                    "agent.registration.exchange",
                    None,
                    Some(r#""agent_kind":"runtime""#),
                )
                .map_err(|_| ProbeFailure::Runtime)?
            {
                return Err(ProbeFailure::DaemonFixtureRuntimeRegistrationPrepareOrExchange);
            }
            if !state
                .audit_event_exists(
                    "app_personal_agent.binding.ready",
                    Some(&self.local_did),
                    None,
                )
                .map_err(|_| ProbeFailure::Runtime)?
            {
                return Err(ProbeFailure::DaemonFixtureBindingPersist);
            }
            return Err(ProbeFailure::DaemonFixtureBindingProjection);
        }
        let binding = &bindings[0];
        Ok(json!({
            "daemon_agent_did": self.local_did,
            "runtime_agent_did": binding.runtime_agent_did,
        }))
    }

    async fn send_plain_marker(
        &self,
        params: &SendPlainMarkerParams,
    ) -> Result<Value, ProbeFailure> {
        let client = self._client.as_ref().ok_or(ProbeFailure::InvalidState)?;
        let result = client
            .messages()
            .send_async(im_core::messages::SendMessageRequest {
                target: im_core::messages::MessageTarget::Direct(
                    im_core::ids::PeerRef::parse(&params.target_did, "")
                        .map_err(|_| ProbeFailure::InvalidRequest)?,
                ),
                body: im_core::messages::MessageBody::Text {
                    text: params.marker.to_string(),
                    kind: im_core::messages::MessageKind::Text,
                },
                security: im_core::messages::MessageSecurityMode::Plain,
                client_message_id: Some(
                    im_core::ids::MessageId::parse(&params.message_id)
                        .map_err(|_| ProbeFailure::InvalidRequest)?,
                ),
                delivery: im_core::messages::MessageDeliveryOptions {
                    idempotency_key: Some(params.message_id.clone()),
                    wait_for_final_acceptance: true,
                },
                delegated_signing: None,
            })
            .await
            .map_err(|_| ProbeFailure::Transport)?;
        if matches!(
            result.delivery,
            im_core::messages::DeliveryState::Failed { .. }
        ) {
            return Err(ProbeFailure::Transport);
        }
        Ok(json!({"sent": true}))
    }

    fn daemon_marker_processed(&self, message_id: &str) -> Result<Value, ProbeFailure> {
        let state_root = self
            .daemon_state_root
            .as_deref()
            .ok_or(ProbeFailure::InvalidState)?;
        let config = awiki_deamon::DaemonConfig::for_state_root(state_root)
            .map_err(|_| ProbeFailure::Runtime)?;
        let state = awiki_deamon::DaemonState::open(&config).map_err(|_| ProbeFailure::Runtime)?;
        let bindings = state
            .list_active_app_personal_agent_bindings()
            .map_err(|_| ProbeFailure::Runtime)?;
        if bindings.len() != 1 || bindings[0].daemon_agent_did != self.local_did {
            return Err(ProbeFailure::InvalidState);
        }
        let owner_did = &bindings[0].user_did;
        let suffix = stable_id_suffix(&format!("{owner_did}:{message_id}"));
        let event_created = state
            .load_message_event(&format!("evt_{suffix}"))
            .map_err(|_| ProbeFailure::Runtime)?
            .is_some();
        let task_id = format!("task_user_msg_{suffix}");
        let task_created = state.load_runtime_task(&task_id).is_ok();
        let run_id = format!("run_{task_id}");
        let run_finished = state
            .load_runtime_run(&run_id)
            .map(|run| run.status == awiki_deamon::runtime::RuntimeRunStatus::Finished)
            .unwrap_or(false);
        let final_created = state
            .load_runtime_final_outbox_by_run(&run_id)
            .map_err(|_| ProbeFailure::Runtime)?
            .is_some();
        Ok(json!({
            "event_created": event_created,
            "task_created": task_created,
            "run_finished": run_finished,
            "final_created": final_created,
        }))
    }

    async fn issue_agent_registration_token(
        &self,
        agent_kind: &'static str,
        handle: &str,
        controller_handle: Option<&str>,
        metadata: Value,
    ) -> Result<Zeroizing<String>, ProbeFailure> {
        let mut params = json!({
            "agent_kind": agent_kind,
            "controller_did": self.local_did,
            "issued_by_did": self.local_did,
            "handle": handle,
            "expires_in_seconds": 600,
            "metadata": metadata,
        });
        if let Some(controller_handle) = controller_handle {
            params.as_object_mut().ok_or(ProbeFailure::Runtime)?.insert(
                "controller_handle".to_owned(),
                Value::String(controller_handle.to_owned()),
            );
        }
        match self
            .rpc_to::<RegistrationTokenResult>(
                &self.agent_registration_rpc_url,
                &self.bearer,
                "issue_token",
                params,
                RpcRejectionPolicy::Standard,
            )
            .await?
        {
            RpcOutcome::Success(mut result) if !result.token.trim().is_empty() => {
                Ok(Zeroizing::new(std::mem::take(&mut result.token)))
            }
            RpcOutcome::Success(_) => Err(ProbeFailure::Runtime),
            RpcOutcome::Rejected(_) => Err(ProbeFailure::Transport),
        }
    }

    async fn connect_ws(&self) -> Result<WsConnectOutcome, ProbeFailure> {
        let mut request = self
            .websocket_url
            .as_str()
            .into_client_request()
            .map_err(|_| ProbeFailure::Runtime)?;
        let authorization = zeroizing_bearer_header(&self.bearer);
        let mut header: tokio_tungstenite::tungstenite::http::HeaderValue =
            authorization.parse().map_err(|_| ProbeFailure::Runtime)?;
        header.set_sensitive(true);
        request.headers_mut().insert(WS_AUTHORIZATION, header);
        let connector = ws_connector(self.ca_bundle.as_deref()).await?;
        match connect_async_tls_with_config(request, None, false, connector).await {
            Ok((stream, _)) => Ok(WsConnectOutcome::Connected(Box::new(stream))),
            Err(WsError::Http(response)) => {
                let status = response.status().as_u16();
                if status == 401 || status == 403 {
                    Ok(WsConnectOutcome::Rejected(Some(SESSION_UNAUTHORIZED)))
                } else {
                    Err(ProbeFailure::Transport)
                }
            }
            Err(_) => Err(ProbeFailure::Transport),
        }
    }

    async fn request_held_download_ticket(
        &self,
        params: &AttachmentTicketParams,
    ) -> Result<RpcOutcome<HeldTicket>, ProbeFailure> {
        let rpc_params = self.download_ticket_rpc_params(params).await?;
        let target_service_did = ticket_target_service_did(&rpc_params)?;
        let object_uri = self.validate_object_uri(&params.object_uri, target_service_did)?;
        let request_body = validated_ticket_request_body(
            &rpc_params,
            params,
            &self.local_did,
            target_service_did,
        )?;
        match self
            .rpc::<DownloadTicketResult>("attachment.get_download_ticket", rpc_params)
            .await?
        {
            RpcOutcome::Success(result) => Ok(RpcOutcome::Success(self.validated_held_ticket(
                result,
                &request_body,
                object_uri,
            )?)),
            RpcOutcome::Rejected(code) => Ok(RpcOutcome::Rejected(code)),
        }
    }

    async fn download_ticket_rpc_params(
        &self,
        params: &AttachmentTicketParams,
    ) -> Result<Value, ProbeFailure> {
        if let Some(client) = self._client.as_ref() {
            let request = im_core::attachments::DownloadAttachmentRequest {
                thread: im_core::messages::ThreadRef::Direct(
                    im_core::ids::PeerRef::parse(&params.sender_did, "")
                        .map_err(|_| ProbeFailure::Runtime)?,
                ),
                message_id: im_core::ids::MessageId::parse(&params.message_id)
                    .map_err(|_| ProbeFailure::Runtime)?,
                attachment_id: Some(params.attachment_id.clone()),
                destination: im_core::attachments::AttachmentDestination::Memory,
                overwrite: false,
            };
            return im_core::compat::attachments::build_download_ticket_rpc_params_for_system_test(
                client,
                request,
                Some(params.sender_did.clone()),
            )
            .await
            .map_err(|_| ProbeFailure::Runtime);
        }

        #[cfg(test)]
        {
            let object_uri = self.validate_object_uri(&params.object_uri, &self.service_did)?;
            let selection = im_core::attachments::AttachmentSelection {
                message_id: params.message_id.clone(),
                sender_did: params.sender_did.clone(),
                attachment_id: params.attachment_id.clone(),
                object_uri: object_uri.to_string(),
                message_security_profile: DIRECT_E2EE.to_owned(),
                ..Default::default()
            };
            let mut rpc_params =
                im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
                    &self.local_did,
                    &self.service_did,
                    &params.sender_did,
                    &params.message_id,
                    "",
                    &selection,
                )
                .map_err(|_| ProbeFailure::Runtime)?;
            rpc_params["meta"]["profile"] = Value::String(ATTACHMENT_V2.to_owned());
            rpc_params["meta"]["anp_version"] = Value::String("2.0".to_owned());
            return Ok(rpc_params);
        }

        #[cfg(not(test))]
        Err(ProbeFailure::Runtime)
    }

    fn validated_held_ticket(
        &self,
        mut result: DownloadTicketResult,
        request_body: &Map<String, Value>,
        object_uri: reqwest::Url,
    ) -> Result<HeldTicket, ProbeFailure> {
        if result.download_ticket_b64u.trim().is_empty()
            || !ticket_binding_matches_request(&result.ticket_binding, request_body)
        {
            return Err(ProbeFailure::Runtime);
        }
        Ok(HeldTicket {
            ticket: Zeroizing::new(std::mem::take(&mut result.download_ticket_b64u)),
            object_uri,
        })
    }

    async fn request_prekey(&self, params: &PrekeyParams) -> Result<RpcOutcome<()>, ProbeFailure> {
        let operation_id = random_operation_id()?;
        let meta = anp::direct_e2ee::key_service_metadata_v2(
            &self.local_did,
            &self.local_device_id,
            &self.service_did,
            &operation_id,
        );
        let body = anp::direct_e2ee::V2GetPrekeyBundleBody {
            target_did: params.target_did.clone(),
            target_device_id: params.target_device_id.clone(),
            preferred_suite: Some(anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2.to_owned()),
            require_opk: Some(false),
        };
        let request = anp::direct_e2ee::get_prekey_bundle_request_v2(meta, body)
            .map_err(|_| ProbeFailure::Runtime)?;
        let object = request.as_object().ok_or(ProbeFailure::Runtime)?;
        let rpc_params = object.get("params").cloned().ok_or(ProbeFailure::Runtime)?;
        let outcome: RpcOutcome<Value> = self
            .rpc("direct.e2ee.get_prekey_bundle", rpc_params)
            .await?;
        match outcome {
            RpcOutcome::Success(value) => {
                let result = anp::direct_e2ee::parse_get_prekey_bundle_result_v2(&value)
                    .map_err(|_| ProbeFailure::Runtime)?;
                if result.target_did != params.target_did
                    || result.target_device_id != params.target_device_id
                {
                    return Err(ProbeFailure::Runtime);
                }
                Ok(RpcOutcome::Success(()))
            }
            RpcOutcome::Rejected(code) => Ok(RpcOutcome::Rejected(code)),
        }
    }

    async fn redeem_held_ticket(
        &mut self,
        expected_digest_b64u: &str,
    ) -> Result<Value, ProbeFailure> {
        let held = self.held_ticket.take().ok_or(ProbeFailure::InvalidState)?;
        let first = self.get_object(&held).await?;
        let bytes = match first {
            ObjectOutcome::Success(bytes) => bytes,
            ObjectOutcome::Rejected { code, .. } => {
                return Ok(redeem_result(false, false, code));
            }
        };
        let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(bytes.as_slice()));
        let digest_matches = digest.as_bytes() == expected_digest_b64u.as_bytes();
        let replay = self.get_object(&held).await?;
        match replay {
            ObjectOutcome::Rejected { status, code } => Ok(redeem_result(
                digest_matches,
                status == 400 && code == Some(DOWNLOAD_TICKET_INVALID),
                code,
            )),
            ObjectOutcome::Success(_) => Ok(redeem_result(digest_matches, false, None)),
        }
    }

    async fn direct_wire_projection(
        &self,
        params: &DirectWireProjectionParams,
    ) -> Result<Value, ProbeFailure> {
        let mut matching_messages = Vec::new();
        let mut skip = 0_i64;

        for _ in 0..DIRECT_WIRE_INBOX_MAX_PAGES {
            let mut rpc_params = im_core::realtime::wire::build_inbox_rpc_params(
                &im_core::realtime::wire::WireIdentity {
                    did: self.local_did.clone(),
                },
                im_core::realtime::wire::InboxWireRequest {
                    limit: DIRECT_WIRE_INBOX_PAGE_LIMIT,
                    auth: None,
                },
            );
            rpc_params
                .get_mut("body")
                .and_then(Value::as_object_mut)
                .ok_or(ProbeFailure::Runtime)?
                .insert("skip".to_owned(), json!(skip));
            let result = match self.rpc::<Value>("inbox.get", rpc_params).await? {
                RpcOutcome::Success(result) => result,
                RpcOutcome::Rejected(_) => return Err(ProbeFailure::Transport),
            };
            let progress = append_direct_wire_matches(&result, params, &mut matching_messages)?;
            if !progress.has_more {
                return closed_direct_wire_projection(
                    &json!({"messages": matching_messages}),
                    params,
                );
            }
            if progress.message_count == 0 {
                return Err(ProbeFailure::Runtime);
            }
            let consumed =
                i64::try_from(progress.message_count).map_err(|_| ProbeFailure::Runtime)?;
            skip = skip.checked_add(consumed).ok_or(ProbeFailure::Runtime)?;
        }

        Err(ProbeFailure::Runtime)
    }

    async fn get_object(&self, held: &HeldTicket) -> Result<ObjectOutcome, ProbeFailure> {
        let mut authorization = reqwest_authorization_header(&held.ticket)?;
        authorization.set_sensitive(true);
        let response = self
            .http
            .get(held.object_uri.clone())
            .header(REQWEST_AUTHORIZATION, authorization)
            .send()
            .await
            .map_err(|_| ProbeFailure::Transport)?;
        let status = response.status();
        let raw = read_limited(response, MAX_OBJECT_BYTES).await?;
        if status.is_success() {
            return Ok(ObjectOutcome::Success(raw));
        }
        Ok(ObjectOutcome::Rejected {
            status: status.as_u16(),
            code: response_error_code(&raw, status.as_u16()),
        })
    }

    async fn rpc<T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: Value,
    ) -> Result<RpcOutcome<T>, ProbeFailure> {
        self.rpc_to(
            &self.message_rpc_url,
            &self.bearer,
            method,
            params,
            RpcRejectionPolicy::Standard,
        )
        .await
    }

    async fn required_user_rpc(
        &self,
        endpoint: &reqwest::Url,
        method: &'static str,
        params: Value,
    ) -> Result<Value, ProbeFailure> {
        match self
            .rpc_to(
                endpoint,
                &self.bearer,
                method,
                params,
                RpcRejectionPolicy::Standard,
            )
            .await?
        {
            RpcOutcome::Success(result) => Ok(result),
            RpcOutcome::Rejected(_) => Err(ProbeFailure::Transport),
        }
    }

    async fn required_account_state_agent_rpc(&self) -> Result<Value, ProbeFailure> {
        let outcome = self
            .rpc_to(
                &self.account_state_rpc_url,
                &self.bearer,
                "account_state.agent_inventory_get",
                json!({}),
                RpcRejectionPolicy::AccountStateAgentInventory,
            )
            .await?;
        required_account_state_agent_outcome(outcome)
    }

    async fn rpc_to<T: DeserializeOwned>(
        &self,
        endpoint: &reqwest::Url,
        bearer: &Zeroizing<String>,
        method: &'static str,
        params: Value,
        rejection_policy: RpcRejectionPolicy,
    ) -> Result<RpcOutcome<T>, ProbeFailure> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": "system-test-probe",
            "method": method,
            "params": params,
        });
        let payload = serde_json::to_vec(&payload).map_err(|_| ProbeFailure::Runtime)?;
        let mut authorization = reqwest_authorization_header(bearer)?;
        authorization.set_sensitive(true);
        let response = self
            .http
            .post(endpoint.clone())
            .header(REQWEST_AUTHORIZATION, authorization)
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .map_err(|_| ProbeFailure::Transport)?;
        let status = response.status();
        let raw = read_limited(response, MAX_RPC_RESPONSE_BYTES).await?;
        let envelope: RpcEnvelope<T> = match serde_json::from_slice(&raw) {
            Ok(value) => value,
            Err(_) if status.as_u16() == 401 || status.as_u16() == 403 => {
                return Ok(RpcOutcome::Rejected(Some(SESSION_UNAUTHORIZED)));
            }
            Err(_) => return Err(ProbeFailure::Runtime),
        };
        match (envelope.result, envelope.error) {
            (Some(result), None) if status.is_success() => Ok(RpcOutcome::Success(result)),
            (None, Some(error)) => Ok(RpcOutcome::Rejected(
                rejection_policy
                    .allowlisted_code(status, &error)
                    .or_else(|| auth_status_code(status.as_u16())),
            )),
            _ if status.as_u16() == 401 || status.as_u16() == 403 => {
                Ok(RpcOutcome::Rejected(Some(SESSION_UNAUTHORIZED)))
            }
            _ => Err(ProbeFailure::Runtime),
        }
    }

    fn validate_object_uri(
        &self,
        raw: &str,
        target_service_did: &str,
    ) -> Result<reqwest::Url, ProbeFailure> {
        let url = reqwest::Url::parse(raw).map_err(|_| ProbeFailure::InvalidRequest)?;
        validate_service_url(&url).map_err(|_| ProbeFailure::InvalidRequest)?;
        if !service_did_matches_url(target_service_did, &url)
            || !url.path().starts_with("/objects/")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ProbeFailure::InvalidRequest);
        }
        Ok(url)
    }
}

fn daemon_route_state_snapshot(
    state: &awiki_deamon::DaemonState,
) -> Result<DaemonRouteStateSnapshot, ProbeFailure> {
    let connection = state.connection().map_err(|_| ProbeFailure::Runtime)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT kind, record_id, message_id, task_id, run_id, route_key, status, version
FROM (
    SELECT
        'session' AS kind,
        route_key AS record_id,
        COALESCE(last_message_id, '') AS message_id,
        '' AS task_id,
        COALESCE(lock_run_id, last_run_id, '') AS run_id,
        route_key,
        status,
        CAST(version AS TEXT) AS version
    FROM cli_route_sessions
    UNION ALL
    SELECT
        'queue' AS kind,
        queue_id AS record_id,
        source_message_id AS message_id,
        COALESCE(task_id, '') AS task_id,
        COALESCE(run_id, '') AS run_id,
        route_key,
        status,
        CAST(route_sequence AS TEXT) AS version
    FROM cli_route_message_queue
    UNION ALL
    SELECT
        'driver' AS kind,
        run_id AS record_id,
        '' AS message_id,
        '' AS task_id,
        run_id,
        route_key,
        status,
        '' AS version
    FROM cli_driver_run
)
ORDER BY kind, record_id
"#,
        )
        .map_err(|_| ProbeFailure::Runtime)?;
    let rows = statement
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ]))
        })
        .map_err(|_| ProbeFailure::Runtime)?;
    let mut canonical_rows = Vec::new();
    for row in rows {
        canonical_rows.push(row.map_err(|_| ProbeFailure::Runtime)?);
    }
    Ok(DaemonRouteStateSnapshot {
        hash: hash_serializable(&canonical_rows)?,
        record_count: canonical_rows.len(),
    })
}

fn queued_delegated_marker_ids(
    old_controller_did: &str,
    message_id: &str,
) -> Result<DaemonMarkerStateIds, ProbeFailure> {
    im_core::ids::MessageId::parse(message_id).map_err(|_| ProbeFailure::InvalidRequest)?;
    let suffix = stable_id_suffix(&format!("{old_controller_did}:{message_id}"));
    let task_id = format!("task_user_msg_{suffix}");
    Ok(DaemonMarkerStateIds {
        event_id: Some(format!("evt_{suffix}")),
        message_id: message_id.to_owned(),
        run_id: format!("run_{task_id}"),
        task_id,
    })
}

fn new_controller_marker_ids(task_id: &str) -> Result<DaemonMarkerStateIds, ProbeFailure> {
    let message_id = task_id
        .strip_prefix("task_")
        .filter(|value| !value.trim().is_empty())
        .ok_or(ProbeFailure::InvalidRequest)?;
    im_core::ids::MessageId::parse(message_id).map_err(|_| ProbeFailure::InvalidRequest)?;
    Ok(DaemonMarkerStateIds {
        event_id: None,
        message_id: message_id.to_owned(),
        run_id: format!("run_{task_id}"),
        task_id: task_id.to_owned(),
    })
}

fn daemon_marker_absence_evidence(
    state: &awiki_deamon::DaemonState,
    ids: &DaemonMarkerStateIds,
) -> Result<DaemonMarkerAbsenceEvidence, ProbeFailure> {
    let connection = state.connection().map_err(|_| ProbeFailure::Runtime)?;
    let evidence = connection
        .query_row(
            r#"
SELECT
    EXISTS(
        SELECT 1
        FROM message_event
        WHERE message_id = ?1
           OR (?2 IS NOT NULL AND event_id = ?2)
    ),
    EXISTS(
        SELECT 1
        FROM cli_route_sessions
        WHERE last_message_id = ?1
           OR last_run_id = ?4
           OR lock_run_id = ?4
    ) OR EXISTS(
        SELECT 1
        FROM cli_route_message_queue
        WHERE source_message_id = ?1
           OR task_id = ?3
           OR run_id = ?4
    ) OR EXISTS(
        SELECT 1
        FROM cli_driver_run
        WHERE run_id = ?4
    ),
    EXISTS(SELECT 1 FROM runtime_task WHERE task_id = ?3),
    EXISTS(SELECT 1 FROM runtime_run WHERE task_id = ?3),
    EXISTS(
        SELECT 1
        FROM runtime_final_outbox AS final
        JOIN runtime_run AS run ON run.run_id = final.run_id
        WHERE run.task_id = ?3
    )
"#,
            rusqlite::params![
                ids.message_id,
                ids.event_id.as_deref(),
                ids.task_id,
                ids.run_id,
            ],
            |row| {
                Ok(DaemonMarkerAbsenceEvidence {
                    event_absent: row.get::<_, i64>(0)? == 0,
                    route_absent: row.get::<_, i64>(1)? == 0,
                    task_absent: row.get::<_, i64>(2)? == 0,
                    run_absent: row.get::<_, i64>(3)? == 0,
                    final_absent: row.get::<_, i64>(4)? == 0,
                })
            },
        )
        .map_err(|_| ProbeFailure::Runtime)?;
    Ok(evidence)
}

enum ObjectOutcome {
    Success(Zeroizing<Vec<u8>>),
    Rejected {
        status: u16,
        code: Option<&'static str>,
    },
}

async fn wait_ws_closed(stream: &mut WsStream, timeout_ms: u64) -> bool {
    let wait = async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Ping(payload))) => {
                    if stream.send(Message::Pong(payload)).await.is_err() {
                        return true;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return true,
                Some(Ok(_)) => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_millis(timeout_ms), wait)
        .await
        .unwrap_or(false)
}

async fn read_limited(
    response: reqwest::Response,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, ProbeFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ProbeFailure::Transport)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ProbeFailure::Runtime);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn build_http_client(ca_bundle: Option<&str>) -> Result<reqwest::Client, ProbeFailure> {
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30));
    if let Some(path) = ca_bundle {
        let raw = fs::read(Path::new(path)).map_err(|_| ProbeFailure::Runtime)?;
        let certs =
            reqwest::Certificate::from_pem_bundle(&raw).map_err(|_| ProbeFailure::Runtime)?;
        if certs.is_empty() {
            return Err(ProbeFailure::Runtime);
        }
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }
    builder.build().map_err(|_| ProbeFailure::Runtime)
}

async fn ws_connector(ca_bundle: Option<&str>) -> Result<Option<Connector>, ProbeFailure> {
    let Some(path) = ca_bundle else {
        return Ok(None);
    };
    let raw = tokio::fs::read(Path::new(path))
        .await
        .map_err(|_| ProbeFailure::Runtime)?;
    let certs = CertificateDer::pem_slice_iter(&raw)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProbeFailure::Runtime)?;
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let (valid, _) = roots.add_parsable_certificates(certs);
    if valid == 0 {
        return Err(ProbeFailure::Runtime);
    }
    Ok(Some(Connector::Rustls(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))))
}

fn reqwest_authorization_header(
    secret: &Zeroizing<String>,
) -> Result<ReqwestHeaderValue, ProbeFailure> {
    let authorization = zeroizing_bearer_header(secret);
    ReqwestHeaderValue::from_str(&authorization).map_err(|_| ProbeFailure::Runtime)
}

fn zeroizing_bearer_header(secret: &Zeroizing<String>) -> Zeroizing<String> {
    Zeroizing::new(im_core::realtime::bearer_authorization_header(secret))
}

fn hash_serializable<T: Serialize>(value: &T) -> Result<[u8; 32], ProbeFailure> {
    let encoded = serde_json::to_vec(value).map_err(|_| ProbeFailure::Runtime)?;
    let encoded = Zeroizing::new(encoded);
    Ok(Sha256::digest(encoded.as_slice()).into())
}

fn hash_parts(parts: &[&str]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.len().to_be_bytes());
        digest.update(part.as_bytes());
    }
    digest.finalize().into()
}

fn stable_id_suffix(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn random_hex(byte_count: usize) -> Result<String, ProbeFailure> {
    if byte_count == 0 || byte_count > 64 {
        return Err(ProbeFailure::Runtime);
    }
    let mut bytes = Zeroizing::new(vec![0_u8; byte_count]);
    rand::thread_rng().fill_bytes(bytes.as_mut_slice());
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn service_base_from_rpc(url: &reqwest::Url) -> Result<String, ProbeFailure> {
    let mut base = url.clone();
    base.set_path("");
    base.set_query(None);
    base.set_fragment(None);
    Ok(base.to_string().trim_end_matches('/').to_owned())
}

fn daemon_bootstrap_public_key(
    document: &Value,
    daemon_agent_did: &str,
    device_e2ee_key_id: &str,
) -> Result<anp::PublicKeyMaterial, ProbeFailure> {
    if !device_e2ee_key_id.starts_with(&format!("{daemon_agent_did}#")) {
        return Err(ProbeFailure::InvalidState);
    }
    let key_agreement = document
        .get("keyAgreement")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::InvalidState)?;
    if key_agreement.len() != 1 || key_agreement[0].as_str() != Some(device_e2ee_key_id) {
        return Err(ProbeFailure::InvalidState);
    }
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::InvalidState)?;
    let mut matching_methods = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(device_e2ee_key_id));
    let method = matching_methods.next().ok_or(ProbeFailure::InvalidState)?;
    if matching_methods.next().is_some()
        || method.get("controller").and_then(Value::as_str) != Some(daemon_agent_did)
        || method.get("type").and_then(Value::as_str) != Some("X25519KeyAgreementKey2019")
    {
        return Err(ProbeFailure::InvalidState);
    }
    let multibase = method
        .get("publicKeyMultibase")
        .and_then(Value::as_str)
        .ok_or(ProbeFailure::InvalidState)?;
    let encoded = multibase
        .strip_prefix('z')
        .ok_or(ProbeFailure::InvalidState)?;
    let mut decoded = Zeroizing::new(
        bs58::decode(encoded)
            .into_vec()
            .map_err(|_| ProbeFailure::InvalidState)?,
    );
    if decoded.len() == 34 && decoded.starts_with(&[0xec, 0x01]) {
        decoded.drain(..2);
    }
    let bytes: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| ProbeFailure::InvalidState)?;
    Ok(anp::PublicKeyMaterial::X25519(bytes))
}

fn parse_request(raw: &str) -> Result<ProbeRequest, ProbeFailure> {
    let value: Value = serde_json::from_str(raw).map_err(|_| ProbeFailure::InvalidRequest)?;
    let object = value.as_object().ok_or(ProbeFailure::InvalidRequest)?;
    require_exact_keys(object, &["action", "id", "params"])?;
    let id = parse_request_id(object.get("id").ok_or(ProbeFailure::InvalidRequest)?)?;
    let action_name = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or(ProbeFailure::InvalidRequest)?;
    let params = object
        .get("params")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::InvalidRequest)?;
    let action = match action_name {
        "device_readiness" => {
            require_exact_keys(params, &[])?;
            Action::DeviceReadiness
        }
        "agent_bootstrap_identity" => {
            require_exact_keys(params, &["controller_account_id"])?;
            Action::AgentBootstrapIdentity(AgentBootstrapIdentityParams {
                controller_account_id: required_string(params, "controller_account_id", 512)?,
            })
        }
        "daemon_continuity_baseline" => {
            require_exact_keys(params, &[])?;
            Action::DaemonContinuityBaseline
        }
        "daemon_continuity_verify" => {
            require_exact_keys(
                params,
                &[
                    "controller_marker",
                    "new_controller_did",
                    "old_controller_did",
                    "queued_marker",
                ],
            )?;
            Action::DaemonContinuityVerify(DaemonContinuityVerifyParams {
                old_controller_did: required_did(params, "old_controller_did")?,
                new_controller_did: required_did(params, "new_controller_did")?,
                queued_marker: required_string(params, "queued_marker", 512)?,
                controller_marker: required_string(params, "controller_marker", 512)?,
            })
        }
        "prepare_daemon_continuity_fixture" => {
            require_exact_keys(
                params,
                &[
                    "app_instance_id",
                    "controller_handle",
                    "daemon_agent_did",
                    "daemon_binary",
                    "daemon_handle",
                    "runtime_handle",
                    "state_root",
                ],
            )?;
            let daemon_binary =
                std::path::PathBuf::from(required_string(params, "daemon_binary", 4096)?);
            let state_root = std::path::PathBuf::from(required_string(params, "state_root", 4096)?);
            Action::PrepareDaemonContinuityFixture(PrepareDaemonContinuityFixtureParams {
                daemon_binary,
                state_root,
                daemon_agent_did: required_did(params, "daemon_agent_did")?,
                daemon_handle: required_string(params, "daemon_handle", 255)?,
                runtime_handle: required_string(params, "runtime_handle", 255)?,
                controller_handle: required_string(params, "controller_handle", 255)?,
                app_instance_id: required_string(params, "app_instance_id", 255)?,
            })
        }
        "stage_daemon_continuity_root" => {
            require_exact_keys(params, &["daemon_handle", "state_root"])?;
            Action::StageDaemonContinuityRoot(StageDaemonContinuityRootParams {
                state_root: std::path::PathBuf::from(required_string(params, "state_root", 4096)?),
                daemon_handle: required_string(params, "daemon_handle", 255)?,
            })
        }
        "daemon_fixture_resources" => {
            require_exact_keys(params, &[])?;
            Action::DaemonFixtureResources
        }
        "send_plain_marker" => {
            require_exact_keys(params, &["marker", "message_id", "target_did"])?;
            Action::SendPlainMarker(SendPlainMarkerParams {
                target_did: required_did(params, "target_did")?,
                message_id: required_string(params, "message_id", 512)?,
                marker: Zeroizing::new(required_opaque_string(params, "marker", 16 * 1024)?),
            })
        }
        "daemon_marker_processed" => {
            require_exact_keys(params, &["message_id"])?;
            Action::DaemonMarkerProcessed {
                message_id: required_string(params, "message_id", 512)?,
            }
        }
        "human_daemon_subkey_state" => {
            require_exact_keys(params, &[])?;
            Action::HumanDaemonSubkeyState
        }
        "open_ws" => {
            require_exact_keys(params, &[])?;
            Action::OpenWs
        }
        "wait_ws_closed" => {
            require_exact_keys(params, &["timeout_ms"])?;
            let timeout_ms = params
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0 && *value <= MAX_TIMEOUT_MS)
                .ok_or(ProbeFailure::InvalidRequest)?;
            Action::WaitWsClosed { timeout_ms }
        }
        "close_ws" => {
            require_exact_keys(params, &[])?;
            Action::CloseWs
        }
        "reconnect_ws" => {
            require_exact_keys(params, &[])?;
            Action::ReconnectWs
        }
        "hold_download_ticket" => {
            Action::HoldDownloadTicket(parse_attachment_ticket_params(params)?)
        }
        "probe_download_ticket" => {
            Action::ProbeDownloadTicket(parse_attachment_ticket_params(params)?)
        }
        "probe_prekey" => Action::ProbePrekey(parse_prekey_params(params)?),
        "direct_wire_projection" => {
            Action::DirectWireProjection(parse_direct_wire_projection_params(params)?)
        }
        "account_state_manifest" => {
            require_exact_keys(params, &[])?;
            Action::AccountStateManifest
        }
        "account_state_agent" => Action::AccountStateAgent(parse_agent_snapshot_params(params)?),
        "account_state_agent_rename" => {
            Action::AccountStateAgentRename(parse_agent_rename_params(params)?)
        }
        "account_state_agent_config" => {
            Action::AccountStateAgentConfig(parse_agent_config_params(params)?)
        }
        "account_state_agent_unbind" => {
            Action::AccountStateAgentUnbind(parse_agent_target_params(params)?)
        }
        "account_state_agent_remove" => {
            Action::AccountStateAgentRemove(parse_agent_target_params(params)?)
        }
        "account_state_status" => Action::AccountStateStatus(parse_agent_status_params(params)?),
        "account_state_profile" => {
            Action::AccountStateProfile(parse_profile_snapshot_params(params)?)
        }
        "account_state_profile_update" => {
            Action::AccountStateProfileUpdate(parse_profile_update_params(params)?)
        }
        "account_state_registry" => {
            Action::AccountStateRegistry(parse_registry_snapshot_params(params)?)
        }
        "redeem_held_ticket" => {
            require_exact_keys(params, &["expected_digest_b64u"])?;
            let expected_digest_b64u = required_string(params, "expected_digest_b64u", 64)?;
            let decoded = URL_SAFE_NO_PAD
                .decode(&expected_digest_b64u)
                .map_err(|_| ProbeFailure::InvalidRequest)?;
            if decoded.len() != 32 {
                return Err(ProbeFailure::InvalidRequest);
            }
            Action::RedeemHeldTicket {
                expected_digest_b64u,
            }
        }
        "shutdown" => {
            require_exact_keys(params, &[])?;
            Action::Shutdown
        }
        _ => return Err(ProbeFailure::InvalidRequest),
    };
    Ok(ProbeRequest { id, action })
}

fn parse_attachment_ticket_params(
    params: &Map<String, Value>,
) -> Result<AttachmentTicketParams, ProbeFailure> {
    require_exact_keys(
        params,
        &["attachment_id", "message_id", "object_uri", "sender_did"],
    )?;
    let sender_did = required_string(params, "sender_did", 2048)?;
    im_core::ids::Did::parse(&sender_did).map_err(|_| ProbeFailure::InvalidRequest)?;
    Ok(AttachmentTicketParams {
        sender_did,
        message_id: required_string(params, "message_id", 512)?,
        attachment_id: required_string(params, "attachment_id", 512)?,
        object_uri: required_string(params, "object_uri", 4096)?,
    })
}

fn parse_prekey_params(params: &Map<String, Value>) -> Result<PrekeyParams, ProbeFailure> {
    require_exact_keys(params, &["target_device_id", "target_did"])?;
    let target_did = required_string(params, "target_did", 2048)?;
    im_core::ids::Did::parse(&target_did).map_err(|_| ProbeFailure::InvalidRequest)?;
    let target_device_id = required_string(params, "target_device_id", 512)?;
    im_core::ids::ProtocolDeviceId::parse(&target_device_id)
        .map_err(|_| ProbeFailure::InvalidRequest)?;
    Ok(PrekeyParams {
        target_did,
        target_device_id,
    })
}

fn parse_direct_wire_projection_params(
    params: &Map<String, Value>,
) -> Result<DirectWireProjectionParams, ProbeFailure> {
    require_exact_keys(
        params,
        &[
            "expected_shape",
            "forbidden_plaintext",
            "message_id",
            "peer_did",
        ],
    )?;
    let peer_did = required_string(params, "peer_did", 2048)?;
    im_core::ids::Did::parse(&peer_did).map_err(|_| ProbeFailure::InvalidRequest)?;
    let message_id = required_string(params, "message_id", 512)?;
    im_core::ids::MessageId::parse(&message_id).map_err(|_| ProbeFailure::InvalidRequest)?;
    let expected_shape = match required_string(params, "expected_shape", 16)?.as_str() {
        "init" => DirectWireShape::Init,
        "cipher" => DirectWireShape::Cipher,
        _ => return Err(ProbeFailure::InvalidRequest),
    };
    let forbidden_plaintext = Zeroizing::new(required_opaque_string(
        params,
        "forbidden_plaintext",
        16 * 1024,
    )?);
    Ok(DirectWireProjectionParams {
        peer_did,
        message_id,
        expected_shape,
        forbidden_plaintext,
    })
}

fn parse_agent_snapshot_params(
    params: &Map<String, Value>,
) -> Result<AgentSnapshotParams, ProbeFailure> {
    require_exact_keys(
        params,
        &[
            "agent_did",
            "expected_active_state",
            "expected_active_mode",
            "expected_blacklist_handles",
            "expected_display_name",
            "expected_whitelist_handles",
        ],
    )?;
    let agent_did = required_string(params, "agent_did", 2048)?;
    im_core::ids::Did::parse(&agent_did).map_err(|_| ProbeFailure::InvalidRequest)?;
    let expected_active_state = required_string(params, "expected_active_state", 32)?;
    if !matches!(
        expected_active_state.as_str(),
        "active" | "inactive" | "revoked" | "archived"
    ) {
        return Err(ProbeFailure::InvalidRequest);
    }
    let expected_active_mode = required_agent_access_mode(params, "expected_active_mode")?;
    Ok(AgentSnapshotParams {
        agent_did,
        expected_active_state,
        expected_display_name: required_string(params, "expected_display_name", 512)?,
        expected_active_mode,
        expected_whitelist_handles: required_string_list(
            params,
            "expected_whitelist_handles",
            100,
            255,
        )?,
        expected_blacklist_handles: required_string_list(
            params,
            "expected_blacklist_handles",
            100,
            255,
        )?,
    })
}

fn parse_agent_rename_params(
    params: &Map<String, Value>,
) -> Result<AgentRenameParams, ProbeFailure> {
    require_exact_keys(params, &["agent_did", "display_name"])?;
    Ok(AgentRenameParams {
        agent_did: required_did(params, "agent_did")?,
        display_name: required_string(params, "display_name", 40)?,
    })
}

fn parse_agent_config_params(
    params: &Map<String, Value>,
) -> Result<AgentConfigParams, ProbeFailure> {
    require_exact_keys(
        params,
        &[
            "active_mode",
            "agent_did",
            "blacklist_handles",
            "whitelist_handles",
        ],
    )?;
    Ok(AgentConfigParams {
        agent_did: required_did(params, "agent_did")?,
        active_mode: required_agent_access_mode(params, "active_mode")?,
        whitelist_handles: required_string_list(params, "whitelist_handles", 100, 255)?,
        blacklist_handles: required_string_list(params, "blacklist_handles", 100, 255)?,
    })
}

fn parse_agent_target_params(
    params: &Map<String, Value>,
) -> Result<AgentTargetParams, ProbeFailure> {
    require_exact_keys(params, &["agent_did"])?;
    Ok(AgentTargetParams {
        agent_did: required_did(params, "agent_did")?,
    })
}

fn parse_agent_status_params(
    params: &Map<String, Value>,
) -> Result<AgentStatusParams, ProbeFailure> {
    require_exact_keys(params, &["agent_did"])?;
    Ok(AgentStatusParams {
        agent_did: required_did(params, "agent_did")?,
    })
}

fn parse_profile_snapshot_params(
    params: &Map<String, Value>,
) -> Result<ProfileSnapshotParams, ProbeFailure> {
    require_exact_keys(params, &["expected_nick_name"])?;
    Ok(ProfileSnapshotParams {
        expected_nick_name: required_string(params, "expected_nick_name", 512)?,
    })
}

fn parse_profile_update_params(
    params: &Map<String, Value>,
) -> Result<ProfileUpdateParams, ProbeFailure> {
    require_exact_keys(params, &["nick_name"])?;
    Ok(ProfileUpdateParams {
        nick_name: required_string(params, "nick_name", 50)?,
    })
}

fn parse_registry_snapshot_params(
    params: &Map<String, Value>,
) -> Result<RegistrySnapshotParams, ProbeFailure> {
    require_exact_keys(params, &["expected_status", "target_device_id"])?;
    let target_device_id = required_string(params, "target_device_id", 512)?;
    im_core::ids::ProtocolDeviceId::parse(&target_device_id)
        .map_err(|_| ProbeFailure::InvalidRequest)?;
    let expected_status = required_string(params, "expected_status", 32)?;
    if !matches!(expected_status.as_str(), "active" | "revoked") {
        return Err(ProbeFailure::InvalidRequest);
    }
    Ok(RegistrySnapshotParams {
        target_device_id,
        expected_status,
    })
}

fn required_string(
    params: &Map<String, Value>,
    key: &str,
    max_len: usize,
) -> Result<String, ProbeFailure> {
    let value = params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= max_len)
        .filter(|value| !value.chars().any(char::is_control))
        .ok_or(ProbeFailure::InvalidRequest)?;
    Ok(value.to_owned())
}

fn required_opaque_string(
    params: &Map<String, Value>,
    key: &str,
    max_len: usize,
) -> Result<String, ProbeFailure> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= max_len)
        .map(str::to_owned)
        .ok_or(ProbeFailure::InvalidRequest)
}

fn required_did(params: &Map<String, Value>, key: &str) -> Result<String, ProbeFailure> {
    let did = required_string(params, key, 2048)?;
    im_core::ids::Did::parse(&did).map_err(|_| ProbeFailure::InvalidRequest)?;
    Ok(did)
}

fn required_agent_access_mode(
    params: &Map<String, Value>,
    key: &str,
) -> Result<String, ProbeFailure> {
    let mode = required_string(params, key, 16)?;
    matches!(mode.as_str(), "whitelist" | "blacklist")
        .then_some(mode)
        .ok_or(ProbeFailure::InvalidRequest)
}

fn required_string_list(
    params: &Map<String, Value>,
    key: &str,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, ProbeFailure> {
    let values = params
        .get(key)
        .and_then(Value::as_array)
        .filter(|values| values.len() <= max_items)
        .ok_or(ProbeFailure::InvalidRequest)?;
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= max_len)
                .filter(|value| !value.chars().any(char::is_control))
                .ok_or(ProbeFailure::InvalidRequest)?;
            Ok(value.to_owned())
        })
        .collect()
}

fn parse_request_id(value: &Value) -> Result<RequestId, ProbeFailure> {
    match value {
        Value::Number(number) if number.as_u64().is_some() => Ok(RequestId(value.clone())),
        Value::String(text)
            if !text.is_empty()
                && text.len() <= 64
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)) =>
        {
            Ok(RequestId(value.clone()))
        }
        _ => Err(ProbeFailure::InvalidRequest),
    }
}

fn request_id_or_zero(raw: &str) -> RequestId {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.get("id").cloned())
        .and_then(|value| parse_request_id(&value).ok())
        .unwrap_or_else(|| RequestId(json!(0)))
}

fn require_exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ProbeFailure> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(ProbeFailure::InvalidRequest)
    }
}

fn success_response(id: RequestId, result: Value) -> Value {
    json!({"id": id.0, "ok": true, "result": result})
}

fn failure_response(id: RequestId, error: ProbeFailure) -> Value {
    json!({"id": id.0, "ok": false, "error": {"code": error.code()}})
}

fn result_with_code(key: &str, value: bool, code: Option<&'static str>) -> Value {
    let mut result = Map::new();
    result.insert(key.to_owned(), Value::Bool(value));
    if let Some(code) = code {
        result.insert("anp_code".to_owned(), Value::String(code.to_owned()));
    }
    Value::Object(result)
}

#[derive(Default)]
struct LocalDocumentProjection {
    manifest_single_device: bool,
    document_hash: Option<String>,
    key_roles_separated: bool,
    daemon_subkey_present: bool,
}

fn workspace_manifest_projection(
    identity_root: &Path,
    local_did: &str,
    local_device_id: &str,
    local_root_key_id: &str,
    local_signing_key_id: &str,
    local_e2ee_key_id: &str,
) -> LocalDocumentProjection {
    let result = (|| -> Result<LocalDocumentProjection, ProbeFailure> {
        let index =
            fs::read(identity_root.join("index.json")).map_err(|_| ProbeFailure::Runtime)?;
        let index: Value = serde_json::from_slice(&index).map_err(|_| ProbeFailure::Runtime)?;
        let Some(dir_name) = workspace_identity_dir_name(&index, local_did)? else {
            return Ok(LocalDocumentProjection::default());
        };
        let relative = Path::new(&dir_name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Ok(LocalDocumentProjection::default());
        }
        let identity_dir = identity_root.join(relative);
        let document_path = ["did.json", "did_document.json"]
            .into_iter()
            .map(|name| identity_dir.join(name))
            .find(|path| path.is_file())
            .ok_or(ProbeFailure::Runtime)?;
        let document = fs::read(document_path).map_err(|_| ProbeFailure::Runtime)?;
        let document: Value =
            serde_json::from_slice(&document).map_err(|_| ProbeFailure::Runtime)?;
        Ok(project_local_document(
            &document,
            local_did,
            local_device_id,
            local_root_key_id,
            local_signing_key_id,
            local_e2ee_key_id,
        ))
    })();
    result.unwrap_or_default()
}

fn workspace_identity_dir_name(
    index: &Value,
    local_did: &str,
) -> Result<Option<String>, ProbeFailure> {
    let object = index.as_object().ok_or(ProbeFailure::Runtime)?;
    let mut entries = Vec::new();
    if let Some(identities) = object.get("identities") {
        let identities = identities.as_array().ok_or(ProbeFailure::Runtime)?;
        entries.extend(identities.iter().filter_map(Value::as_object));
    }
    if let Some(credentials) = object.get("credentials") {
        let credentials = credentials.as_object().ok_or(ProbeFailure::Runtime)?;
        entries.extend(credentials.values().filter_map(Value::as_object));
    }
    if !object.contains_key("identities") && !object.contains_key("credentials") {
        return Err(ProbeFailure::Runtime);
    }
    let matching = entries
        .into_iter()
        .filter(|entry| entry.get("did").and_then(Value::as_str) == Some(local_did))
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Ok(None);
    }
    Ok(Some(
        required_response_string(matching[0], "dir_name")?.to_owned(),
    ))
}

fn project_local_document(
    document: &Value,
    local_did: &str,
    local_device_id: &str,
    local_root_key_id: &str,
    local_signing_key_id: &str,
    local_e2ee_key_id: &str,
) -> LocalDocumentProjection {
    LocalDocumentProjection {
        manifest_single_device: manifest_matches(
            document,
            local_did,
            local_device_id,
            local_signing_key_id,
            local_e2ee_key_id,
        ),
        document_hash: canonical_document_hash(document),
        key_roles_separated: key_roles_separated(
            document,
            local_root_key_id,
            local_signing_key_id,
            local_e2ee_key_id,
        ),
        daemon_subkey_present: document_has_daemon_subkey(document, local_did),
    }
}

fn document_has_daemon_subkey(document: &Value, local_did: &str) -> bool {
    let expected = format!("{local_did}#daemon-key-1");
    document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .is_some_and(|methods| {
            methods
                .iter()
                .any(|method| method.get("id").and_then(Value::as_str) == Some(expected.as_str()))
        })
}

fn canonical_document_hash(document: &Value) -> Option<String> {
    let canonical = serde_json_canonicalizer::to_vec(document).ok()?;
    Some(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

fn key_roles_separated(
    document: &Value,
    local_root_key_id: &str,
    local_signing_key_id: &str,
    local_e2ee_key_id: &str,
) -> bool {
    let Some(methods) = document.get("verificationMethod").and_then(Value::as_array) else {
        return false;
    };
    let material = |key_id: &str| -> Option<Vec<u8>> {
        let matching = methods
            .iter()
            .filter_map(Value::as_object)
            .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return None;
        }
        let method = matching[0];
        let fields = ["publicKeyJwk", "publicKeyMultibase", "publicKeyBase58"]
            .into_iter()
            .filter_map(|key| method.get(key).map(|value| (key, value)))
            .collect::<Vec<_>>();
        if fields.len() != 1 {
            return None;
        }
        serde_json_canonicalizer::to_vec(&json!({fields[0].0: fields[0].1})).ok()
    };
    let Some(root) = material(local_root_key_id) else {
        return false;
    };
    let Some(signing) = material(local_signing_key_id) else {
        return false;
    };
    let Some(e2ee) = material(local_e2ee_key_id) else {
        return false;
    };
    local_root_key_id != local_signing_key_id
        && local_root_key_id != local_e2ee_key_id
        && local_signing_key_id != local_e2ee_key_id
        && root != signing
        && root != e2ee
        && signing != e2ee
}

fn manifest_matches(
    document: &Value,
    local_did: &str,
    local_device_id: &str,
    local_signing_key_id: &str,
    local_e2ee_key_id: &str,
) -> bool {
    let Ok(Some(manifest)) = anp::authentication::validate_device_manifest(document) else {
        return false;
    };
    if document.get("id").and_then(Value::as_str) != Some(local_did) || manifest.devices.len() != 1
    {
        return false;
    }
    let device = &manifest.devices[0];
    device.device_id == local_device_id
        && device.signing_key_id == local_signing_key_id
        && device.e2ee_key_id == local_e2ee_key_id
}

fn bootstrap_registry_matches(
    result: &Value,
    local_did: &str,
    local_device_id: &str,
    local_signing_key_id: &str,
    local_e2ee_key_id: &str,
    local_auth_generation: &str,
    local_document_hash: &str,
) -> bool {
    let checked = (|| -> Result<bool, ProbeFailure> {
        let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
        if required_response_string(object, "did")? != local_did {
            return Ok(false);
        }
        let checkpoint = object
            .get("checkpoint")
            .and_then(Value::as_object)
            .ok_or(ProbeFailure::Runtime)?;
        if canonical_u64(checkpoint, "document_version")? != 1
            || canonical_u64(checkpoint, "registry_version")? != 1
            || local_document_hash.is_empty()
            || required_response_string(checkpoint, "document_hash")? != local_document_hash
        {
            return Ok(false);
        }
        let devices = object
            .get("devices")
            .and_then(Value::as_array)
            .ok_or(ProbeFailure::Runtime)?;
        if devices.len() != 1 {
            return Ok(false);
        }
        let device = devices[0].as_object().ok_or(ProbeFailure::Runtime)?;
        let expected_generation = local_auth_generation
            .parse::<u64>()
            .ok()
            .filter(|value| value.to_string() == local_auth_generation)
            .ok_or(ProbeFailure::Runtime)?;
        Ok(
            required_response_string(device, "device_id")? == local_device_id
                && required_response_string(device, "signing_key_id")? == local_signing_key_id
                && required_response_string(device, "e2ee_key_id")? == local_e2ee_key_id
                && required_response_string(device, "status")? == "active"
                && required_response_string(device, "role")? == "admin"
                && device.get("management_ready").and_then(Value::as_bool) == Some(true)
                && canonical_u64(device, "auth_generation")? == expected_generation,
        )
    })();
    checked.unwrap_or(false)
}

fn bootstrap_manifest_matches(
    result: &Value,
    local_account_id: &str,
    local_did: &str,
    local_binding_generation: &str,
) -> bool {
    let checked = (|| -> Result<bool, ProbeFailure> {
        let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
        let versions = object
            .get("versions")
            .and_then(Value::as_object)
            .ok_or(ProbeFailure::Runtime)?;
        Ok(
            required_response_string(object, "account_id")? == local_account_id
                && required_response_string(object, "current_did")? == local_did
                && canonical_decimal_string(object, "identity_generation")?
                    == local_binding_generation
                && local_binding_generation == "1"
                && canonical_decimal_string(versions, "device_registry")? == "1",
        )
    })();
    checked.unwrap_or(false)
}

fn controller_binding_projection(
    source_controller_account_id: Option<&str>,
    expected_controller_account_id: &str,
) -> Option<bool> {
    source_controller_account_id.map(|value| value == expected_controller_account_id)
}

fn device_access_projection_matches(
    token: &Zeroizing<String>,
    local_did: &str,
    local_account_id: &str,
    local_device_id: &str,
    local_signing_key_id: &str,
    local_auth_generation: &str,
) -> (bool, bool) {
    let checked = (|| -> Result<(bool, bool), ProbeFailure> {
        let mut parts = token.split('.');
        let (Some(_header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ProbeFailure::Runtime);
        };
        if payload.is_empty() || signature.is_empty() {
            return Err(ProbeFailure::Runtime);
        }
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| ProbeFailure::Runtime)?;
        let claims: Value = serde_json::from_slice(&payload).map_err(|_| ProbeFailure::Runtime)?;
        let claims = claims.as_object().ok_or(ProbeFailure::Runtime)?;
        let sync_capability_absent = !claims.contains_key("sync_capability");
        let expected_generation = local_auth_generation
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0 && value.to_string() == local_auth_generation)
            .ok_or(ProbeFailure::Runtime)?;
        let audiences = exact_unique_string_set(claims, "aud")?;
        let scopes = exact_unique_string_set(claims, "scopes")?;
        let issued_at = canonical_u64(claims, "iat")?;
        let not_before = canonical_u64(claims, "nbf")?;
        let expires_at = canonical_u64(claims, "exp")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ProbeFailure::Runtime)?
            .as_secs();
        let standard = required_response_string(claims, "iss")? == "user-service"
            && required_response_string(claims, "type")? == "access"
            && required_response_string(claims, "purpose")? == "awiki.device.access.v1"
            && required_response_string(claims, "sub")? == local_did
            && required_response_string(claims, "did")? == local_did
            && required_response_string(claims, "user_id")? == local_account_id
            && required_response_string(claims, "device_id")? == local_device_id
            && required_response_string(claims, "key_id")? == local_signing_key_id
            && canonical_u64(claims, "auth_generation")? == expected_generation
            && audiences
                == BTreeSet::from([
                    "awiki-message-service".to_owned(),
                    "awiki-user-service".to_owned(),
                ])
            && scopes
                == BTreeSet::from([
                    "device:manage".to_owned(),
                    "device:read".to_owned(),
                    "message:connect".to_owned(),
                ])
            && issued_at == not_before
            && expires_at > issued_at
            && not_before <= now
            && expires_at > now
            && !required_response_string(claims, "jti")?.is_empty()
            && !claims.contains_key("profile")
            && sync_capability_absent;
        Ok((standard, sync_capability_absent))
    })();
    checked.unwrap_or((false, false))
}

fn exact_unique_string_set(
    object: &Map<String, Value>,
    key: &str,
) -> Result<BTreeSet<String>, ProbeFailure> {
    let values = object
        .get(key)
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= 16)
        .ok_or(ProbeFailure::Runtime)?;
    let set = values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty() && value.len() <= 128)
                .map(str::to_owned)
                .ok_or(ProbeFailure::Runtime)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if set.len() != values.len() {
        return Err(ProbeFailure::Runtime);
    }
    Ok(set)
}

struct DirectWirePageProgress {
    message_count: usize,
    has_more: bool,
}

fn append_direct_wire_matches(
    result: &Value,
    params: &DirectWireProjectionParams,
    matching: &mut Vec<Value>,
) -> Result<DirectWirePageProgress, ProbeFailure> {
    let result = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let messages = result
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| messages.len() <= DIRECT_WIRE_INBOX_PAGE_LIMIT as usize)
        .ok_or(ProbeFailure::Runtime)?;
    let has_more = result
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or(ProbeFailure::Runtime)?;

    for message in messages {
        let message_object = message.as_object().ok_or(ProbeFailure::Runtime)?;
        let message_id = required_response_string(message_object, "id")?;
        if message_id != params.message_id {
            continue;
        }
        let sender_did = required_response_string(message_object, "sender_did")?;
        if sender_did == params.peer_did {
            matching.push(message.clone());
        }
    }

    Ok(DirectWirePageProgress {
        message_count: messages.len(),
        has_more,
    })
}

fn closed_direct_wire_projection(
    result: &Value,
    params: &DirectWireProjectionParams,
) -> Result<Value, ProbeFailure> {
    let messages = result
        .as_object()
        .and_then(|result| result.get("messages"))
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::Runtime)?;
    let mut matching = Vec::new();
    for message in messages {
        let message_object = message.as_object().ok_or(ProbeFailure::Runtime)?;
        let message_id = required_response_string(message_object, "id")?;
        if message_id == params.message_id {
            let sender_did = required_response_string(message_object, "sender_did")?;
            if sender_did == params.peer_did {
                matching.push((message, message_object));
            }
        }
    }

    let canonical_match_count = bounded_count(matching.len())?;
    let Some((message_value, message)) = matching.first().filter(|_| matching.len() == 1) else {
        return Ok(json!({
            "canonical_match_count": canonical_match_count,
            "content_type_matches": false,
            "wire_kind_matches": false,
            "ciphertext_present": false,
            "shape_matches": false,
            "plaintext_absent": false,
        }));
    };
    let content = message.get("content").and_then(Value::as_object);
    let content_type_matches =
        string_field(message, "content_type") == Some(params.expected_shape.content_type());
    let wire_kind_matches = string_field(message, "type") == Some("json");
    let ciphertext_present = content
        .and_then(|content| string_field(content, "ciphertext_b64u"))
        .is_some_and(|value| !value.is_empty());
    let shape_matches = content.is_some_and(|content| match params.expected_shape {
        DirectWireShape::Init => [
            "session_id",
            "suite",
            "sender_static_key_agreement_id",
            "recipient_bundle_id",
            "recipient_signed_prekey_id",
            "sender_ephemeral_pub_b64u",
        ]
        .into_iter()
        .all(|field| string_field(content, field).is_some_and(|value| !value.is_empty())),
        DirectWireShape::Cipher => {
            string_field(content, "session_id").is_some_and(|value| !value.is_empty())
                && content
                    .get("ratchet_header")
                    .and_then(Value::as_object)
                    .is_some_and(|header| {
                        string_field(header, "dh_pub_b64u").is_some_and(|value| !value.is_empty())
                            && ["pn", "n"].into_iter().all(|field| {
                                header
                                    .get(field)
                                    .is_some_and(canonical_nonnegative_wire_number)
                            })
                    })
        }
    });
    let plaintext_absent =
        !json_value_contains_decoded_string(message_value, params.forbidden_plaintext.as_str());

    Ok(json!({
        "canonical_match_count": canonical_match_count,
        "content_type_matches": content_type_matches,
        "wire_kind_matches": wire_kind_matches,
        "ciphertext_present": ciphertext_present,
        "shape_matches": shape_matches,
        "plaintext_absent": plaintext_absent,
    }))
}

fn json_value_contains_decoded_string(value: &Value, forbidden: &str) -> bool {
    match value {
        Value::String(value) => value.contains(forbidden),
        Value::Array(values) => values
            .iter()
            .any(|value| json_value_contains_decoded_string(value, forbidden)),
        Value::Object(values) => values.iter().any(|(key, value)| {
            key.contains(forbidden) || json_value_contains_decoded_string(value, forbidden)
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn canonical_nonnegative_wire_number(value: &Value) -> bool {
    if value.as_u64().is_some() {
        return true;
    }
    value.as_str().is_some_and(|value| {
        value == "0"
            || (!value.is_empty()
                && !value.starts_with('0')
                && value.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn closed_manifest_result(result: &Value) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    required_response_string(object, "account_id")?;
    let current_did = required_response_string(object, "current_did")?;
    im_core::ids::Did::parse(current_did).map_err(|_| ProbeFailure::Runtime)?;
    required_response_string(object, "server_time")?;
    let versions = object
        .get("versions")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::Runtime)?;
    Ok(json!({
        "identity_generation": canonical_decimal_string(object, "identity_generation")?,
        "profile_version": canonical_decimal_string(versions, "profile")?,
        "agent_inventory_version": canonical_decimal_string(versions, "agent_inventory")?,
        "agent_status_version": canonical_decimal_string(versions, "agent_status")?,
        "device_registry_version": canonical_decimal_string(versions, "device_registry")?,
    }))
}

fn closed_agent_result(
    result: &Value,
    params: &AgentSnapshotParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    required_response_string(object, "account_id")?;
    let agents = object
        .get("agents")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::Runtime)?;
    let mut active_count = 0_u64;
    let mut inactive_count = 0_u64;
    let mut revoked_count = 0_u64;
    let mut archived_count = 0_u64;
    let mut match_count = 0_u64;
    let mut matched_expected = false;
    for agent in agents {
        let agent = agent.as_object().ok_or(ProbeFailure::Runtime)?;
        let agent_did = required_response_string(agent, "agent_did")?;
        im_core::ids::Did::parse(agent_did).map_err(|_| ProbeFailure::Runtime)?;
        let active_state = required_response_string(agent, "active_state")?;
        match active_state {
            "active" => active_count += 1,
            "inactive" => inactive_count += 1,
            "revoked" => revoked_count += 1,
            "archived" => archived_count += 1,
            _ => return Err(ProbeFailure::Runtime),
        }
        let display_name = required_response_string(agent, "display_name")?;
        let invocation_policy = agent
            .get("invocation_policy")
            .and_then(Value::as_object)
            .ok_or(ProbeFailure::Runtime)?;
        if required_response_string(invocation_policy, "schema")?
            != "awiki.agent_invocation_policy.v1"
        {
            return Err(ProbeFailure::Runtime);
        }
        let active_mode = required_response_string(invocation_policy, "active_mode")?;
        if !matches!(active_mode, "whitelist" | "blacklist") {
            return Err(ProbeFailure::Runtime);
        }
        let whitelist_handles =
            response_string_list(invocation_policy, "whitelist_handles", 100, 255)?;
        let blacklist_handles =
            response_string_list(invocation_policy, "blacklist_handles", 100, 255)?;
        if agent_did == params.agent_did {
            match_count += 1;
            matched_expected |= active_state == params.expected_active_state
                && display_name == params.expected_display_name
                && active_mode == params.expected_active_mode
                && whitelist_handles == params.expected_whitelist_handles
                && blacklist_handles == params.expected_blacklist_handles;
        }
    }
    Ok(json!({
        "inventory_version": canonical_decimal_string(object, "inventory_version")?,
        "total_count": bounded_count(agents.len())?,
        "match_count": match_count,
        "active_count": active_count,
        "inactive_count": inactive_count,
        "revoked_count": revoked_count,
        "archived_count": archived_count,
        "matched_expected": matched_expected,
    }))
}

fn closed_agent_rename_result(
    result: &Value,
    params: &AgentRenameParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let agent_did = required_response_did(object, "agent_did")?;
    let display_name = required_response_string(object, "display_name")?;
    Ok(json!({
        "inventory_version": canonical_decimal_string(object, "inventory_version")?,
        "matched_expected": agent_did == params.agent_did
            && display_name == params.display_name,
    }))
}

fn closed_agent_config_result(
    result: &Value,
    params: &AgentConfigParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    if required_response_string(object, "schema")? != "awiki.agent_invocation_policy.v1" {
        return Err(ProbeFailure::Runtime);
    }
    let active_mode = required_response_string(object, "active_mode")?;
    if !matches!(active_mode, "whitelist" | "blacklist") {
        return Err(ProbeFailure::Runtime);
    }
    let whitelist_handles = response_string_list(object, "whitelist_handles", 100, 255)?;
    let blacklist_handles = response_string_list(object, "blacklist_handles", 100, 255)?;
    Ok(json!({
        "inventory_version": canonical_decimal_string(object, "inventory_version")?,
        "matched_expected": active_mode == params.active_mode
            && whitelist_handles == params.whitelist_handles
            && blacklist_handles == params.blacklist_handles,
    }))
}

fn closed_agent_unbind_result(result: &Value) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let matched_expected = object
        .get("ok")
        .and_then(Value::as_bool)
        .ok_or(ProbeFailure::Runtime)?;
    Ok(json!({
        "inventory_version": canonical_decimal_string(object, "inventory_version")?,
        "matched_expected": matched_expected,
    }))
}

fn closed_agent_remove_result(
    result: &Value,
    params: &AgentTargetParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let removed = object
        .get("removed")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::Runtime)?;
    let mut match_count = 0_u64;
    for agent in removed {
        let agent = agent.as_object().ok_or(ProbeFailure::Runtime)?;
        let agent_did = required_response_did(agent, "agent_did")?;
        let active_state = required_response_string(agent, "active_state")?;
        if !matches!(active_state, "active" | "inactive" | "revoked" | "archived") {
            return Err(ProbeFailure::Runtime);
        }
        if agent_did == params.agent_did && active_state == "archived" {
            match_count += 1;
        }
    }
    Ok(json!({
        "inventory_version": canonical_decimal_string(object, "inventory_version")?,
        "matched_expected": match_count == 1,
    }))
}

fn closed_agent_status_result(
    result: &Value,
    params: &AgentStatusParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    required_response_string(object, "account_id")?;
    let statuses = object
        .get("statuses")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::Runtime)?;
    let mut match_count = 0_u64;
    for status in statuses {
        let status = status.as_object().ok_or(ProbeFailure::Runtime)?;
        let agent_did = required_response_string(status, "agent_did")?;
        im_core::ids::Did::parse(agent_did).map_err(|_| ProbeFailure::Runtime)?;
        if agent_did == params.agent_did {
            match_count += 1;
        }
    }
    Ok(json!({
        "agent_status_version": canonical_decimal_string(object, "agent_status_version")?,
        "total_count": bounded_count(statuses.len())?,
        "match_count": match_count,
    }))
}

fn closed_profile_result(
    result: &Value,
    params: &ProfileSnapshotParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    required_response_string(object, "account_id")?;
    let profile = object
        .get("profile")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::Runtime)?;
    let nick_name = required_response_string(profile, "nick_name")?;
    Ok(json!({
        "profile_version": canonical_decimal_string(object, "profile_version")?,
        "matched_expected": nick_name == params.expected_nick_name,
    }))
}

fn closed_profile_update_result(
    result: &Value,
    params: &ProfileUpdateParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let nick_name = required_response_string(object, "nick_name")?;
    Ok(json!({
        "profile_version": canonical_decimal_string(object, "profile_version")?,
        "matched_expected": nick_name == params.nick_name,
    }))
}

fn closed_registry_result(
    result: &Value,
    params: &RegistrySnapshotParams,
) -> Result<Value, ProbeFailure> {
    let object = result.as_object().ok_or(ProbeFailure::Runtime)?;
    let did = required_response_string(object, "did")?;
    im_core::ids::Did::parse(did).map_err(|_| ProbeFailure::Runtime)?;
    let checkpoint = object
        .get("checkpoint")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::Runtime)?;
    let registry_version = canonical_u64(checkpoint, "registry_version")?.to_string();
    let devices = object
        .get("devices")
        .and_then(Value::as_array)
        .ok_or(ProbeFailure::Runtime)?;
    let mut match_count = 0_u64;
    let mut matched_expected = false;
    for device in devices {
        let device = device.as_object().ok_or(ProbeFailure::Runtime)?;
        let device_id = required_response_string(device, "device_id")?;
        let status = required_response_string(device, "status")?;
        if !matches!(status, "active" | "revoked") {
            return Err(ProbeFailure::Runtime);
        }
        if device_id == params.target_device_id {
            match_count += 1;
            matched_expected |= status == params.expected_status;
        }
    }
    Ok(json!({
        "registry_version": registry_version,
        "total_count": bounded_count(devices.len())?,
        "match_count": match_count,
        "matched_expected": matched_expected,
    }))
}

fn required_response_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, ProbeFailure> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .filter(|value| !value.chars().any(char::is_control))
        .ok_or(ProbeFailure::Runtime)
}

fn required_response_did<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, ProbeFailure> {
    let did = required_response_string(object, key)?;
    im_core::ids::Did::parse(did).map_err(|_| ProbeFailure::Runtime)?;
    Ok(did)
}

fn response_string_list(
    object: &Map<String, Value>,
    key: &str,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, ProbeFailure> {
    let values = object
        .get(key)
        .and_then(Value::as_array)
        .filter(|values| values.len() <= max_items)
        .ok_or(ProbeFailure::Runtime)?;
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .filter(|value| !value.trim().is_empty() && value.len() <= max_len)
                .filter(|value| !value.chars().any(char::is_control))
                .ok_or(ProbeFailure::Runtime)?;
            Ok(value.to_owned())
        })
        .collect()
}

fn canonical_decimal_string(
    object: &Map<String, Value>,
    key: &str,
) -> Result<String, ProbeFailure> {
    let value = required_response_string(object, key)?;
    if value == "0"
        || (value.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && !value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(value.to_owned())
    } else {
        Err(ProbeFailure::Runtime)
    }
}

fn canonical_u64(object: &Map<String, Value>, key: &str) -> Result<u64, ProbeFailure> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(ProbeFailure::Runtime)
}

fn bounded_count(value: usize) -> Result<u64, ProbeFailure> {
    u64::try_from(value).map_err(|_| ProbeFailure::Runtime)
}

fn redeem_result(digest_matches: bool, replay_rejected: bool, code: Option<&'static str>) -> Value {
    let mut result = Map::new();
    result.insert("digest_matches".to_owned(), Value::Bool(digest_matches));
    result.insert("replay_rejected".to_owned(), Value::Bool(replay_rejected));
    if let Some(code) = code {
        result.insert("anp_code".to_owned(), Value::String(code.to_owned()));
    }
    Value::Object(result)
}

fn validated_ticket_request_body(
    rpc_params: &Value,
    params: &AttachmentTicketParams,
    local_did: &str,
    service_did: &str,
) -> Result<Map<String, Value>, ProbeFailure> {
    let object = rpc_params.as_object().ok_or(ProbeFailure::Runtime)?;
    let meta = object
        .get("meta")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::Runtime)?;
    let body = object
        .get("body")
        .and_then(Value::as_object)
        .ok_or(ProbeFailure::Runtime)?;
    let target_service_did = meta
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| string_field(target, "did"));
    let target_kind = meta
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| string_field(target, "kind"));
    if string_field(meta, "profile") != Some(ATTACHMENT_V2)
        || string_field(meta, "anp_version") != Some("2.0")
        || string_field(meta, "security_profile") != Some("transport-protected")
        || string_field(meta, "sender_did") != Some(local_did)
        || target_kind != Some("service")
        || target_service_did != Some(service_did)
        || string_field(body, "attachment_id") != Some(params.attachment_id.as_str())
        || string_field(body, "object_uri") != Some(params.object_uri.as_str())
        || string_field(body, "requester_did") != Some(local_did)
        || string_field(body, "message_security_profile") != Some(DIRECT_E2EE)
        || string_field(body, "message_target_did") != Some(local_did)
        || string_field(body, "message_id").is_none_or(str::is_empty)
    {
        return Err(ProbeFailure::Runtime);
    }
    Ok(body.clone())
}

fn ticket_target_service_did(rpc_params: &Value) -> Result<&str, ProbeFailure> {
    let target_service_did = rpc_params
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("target"))
        .and_then(Value::as_object)
        .and_then(|target| string_field(target, "did"))
        .ok_or(ProbeFailure::Runtime)?;
    im_core::ids::Did::parse(target_service_did).map_err(|_| ProbeFailure::Runtime)?;
    Ok(target_service_did)
}

fn ticket_binding_matches_request(
    binding: &Map<String, Value>,
    request_body: &Map<String, Value>,
) -> bool {
    [
        "attachment_id",
        "object_uri",
        "requester_did",
        "message_id",
        "message_security_profile",
        "message_target_did",
    ]
    .into_iter()
    .all(|key| string_field(binding, key) == string_field(request_body, key))
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn response_error_code(raw: &[u8], status: u16) -> Option<&'static str> {
    serde_json::from_slice::<Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(allowlisted_anp_code)
                .or_else(|| allowlisted_anp_code(&value))
        })
        .or_else(|| auth_status_code(status))
}

fn auth_status_code(status: u16) -> Option<&'static str> {
    (status == 401 || status == 403).then_some(SESSION_UNAUTHORIZED)
}

fn allowlisted_anp_code(error: &Value) -> Option<&'static str> {
    let candidates = [
        error.as_str(),
        error.get("anp_code").and_then(Value::as_str),
        error.get("awiki_code").and_then(Value::as_str),
        error
            .get("data")
            .and_then(|data| data.get("anp_code"))
            .and_then(Value::as_str),
        error
            .get("data")
            .and_then(|data| data.get("awiki_code"))
            .and_then(Value::as_str),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|code| match code {
            DEVICE_NOT_ELIGIBLE => Some(DEVICE_NOT_ELIGIBLE),
            DEVICE_STATE_CHANGED => Some(DEVICE_STATE_CHANGED),
            SESSION_UNAUTHORIZED => Some(SESSION_UNAUTHORIZED),
            DOWNLOAD_TICKET_INVALID => Some(DOWNLOAD_TICKET_INVALID),
            _ => None,
        })
}

fn allowlisted_account_state_test_code(error: &Value) -> Option<&'static str> {
    let error = error.as_object()?;
    require_exact_keys(error, &["code", "data", "message"]).ok()?;
    if error.get("code").and_then(Value::as_i64) != Some(-32603)
        || error
            .get("message")
            .and_then(Value::as_str)
            .filter(|message| !message.is_empty())
            .is_none()
    {
        return None;
    }
    let data = error.get("data").and_then(Value::as_object)?;
    require_exact_keys(data, &["code", "retryable"]).ok()?;
    (data.get("code").and_then(Value::as_str) == Some(ACCOUNT_STATE_TEST_FAIL_ONCE)
        && data.get("retryable").and_then(Value::as_bool) == Some(true))
    .then_some(ACCOUNT_STATE_TEST_FAIL_ONCE)
}

fn validate_service_url(url: &reqwest::Url) -> Result<(), ProbeFailure> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host_str().is_some_and(is_loopback_host) => Ok(()),
        _ => Err(ProbeFailure::Runtime),
    }
}

fn service_did_matches_url(service_did: &str, url: &reqwest::Url) -> bool {
    let mut parts = service_did.split(':');
    let domain = match (parts.next(), parts.next(), parts.next()) {
        (Some("did"), Some("wba"), Some(domain)) if !domain.is_empty() => domain,
        _ => return false,
    };
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(domain))
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

fn random_operation_id() -> Result<String, ProbeFailure> {
    let mut bytes = [0_u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ProbeFailure::Runtime)?;
    Ok(format!("probe-{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const LOCAL_DID: &str = "did:wba:example.test:user:local";
    const HUMAN_DID: &str = "did:wba:example.test:user:fixture-human";
    const DAEMON_DID: &str = "did:wba:example.test:agent:fixture-daemon";
    const SENDER_DID: &str = "did:wba:example.test:user:sender";
    const TARGET_DID: &str = "did:wba:example.test:user:target";
    const SERVICE_DID: &str = "did:wba:127.0.0.1:service:message";
    const TOKEN_SECRET: &str = "jwt-secret-must-not-leak";
    const TICKET_SECRET: &str = "ticket-secret-must-not-leak";
    const SERVER_ERROR_SECRET: &str = "server-error-must-not-leak";
    const WIRE_CIPHERTEXT_SECRET: &str = "wire-ciphertext-must-not-leak";
    const WIRE_PLAINTEXT_SECRET: &str = "wire plaintext \"quoted\" \\\n+second line";

    struct TestStateRoot {
        path: std::path::PathBuf,
    }

    impl TestStateRoot {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Unix time")
                .as_nanos();
            Self {
                path: std::env::temp_dir().join(format!(
                    "awiki-system-test-probe-{name}-{}-{nanos}",
                    std::process::id()
                )),
            }
        }
    }

    impl Drop for TestStateRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn probe_ok<T>(result: Result<T, ProbeFailure>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(_) => panic!("{context}"),
        }
    }

    #[test]
    fn protocol_rejects_unknown_actions_and_extra_fields() {
        let readiness = match parse_request(r#"{"id":1,"action":"device_readiness","params":{}}"#) {
            Ok(request) => request,
            Err(_) => panic!("closed readiness request"),
        };
        assert!(matches!(readiness.action, Action::DeviceReadiness));

        let unknown = match parse_request(r#"{"id":7,"action":"raw_rpc","params":{}}"#) {
            Err(error) => error,
            Ok(_) => panic!("raw RPC must be rejected"),
        };
        assert_eq!(unknown.code(), INVALID_REQUEST);

        let extra = match parse_request(
            r#"{"id":"safe-id","action":"open_ws","params":{},"url":"https://evil.test"}"#,
        ) {
            Err(error) => error,
            Ok(_) => panic!("extra fields must be rejected"),
        };
        assert_eq!(extra.code(), INVALID_REQUEST);

        let response = failure_response(RequestId(json!(7)), unknown);
        assert_eq!(
            response,
            json!({"id": 7, "ok": false, "error": {"code": INVALID_REQUEST}})
        );
    }

    #[test]
    fn agent_bootstrap_identity_request_is_closed_and_exact() {
        let request = match parse_request(
            r#"{"id":"agent-1","action":"agent_bootstrap_identity","params":{"controller_account_id":"controller-account"}}"#,
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed Agent bootstrap request"),
        };
        let Action::AgentBootstrapIdentity(params) = request.action else {
            panic!("Agent bootstrap action")
        };
        assert_eq!(params.controller_account_id, "controller-account");

        for raw in [
            r#"{"id":"agent-2","action":"agent_bootstrap_identity","params":{}}"#,
            r#"{"id":"agent-3","action":"agent_bootstrap_identity","params":{"controller_account_id":"controller-account","access_token":"secret"}}"#,
        ] {
            assert!(parse_request(raw).is_err());
        }
    }

    #[test]
    fn probe_bootstrap_desired_personal_agent_includes_exact_preferred_language() {
        let payload = ProbeBootstrapPayload {
            schema: "awiki.daemon.bootstrap.v1",
            bootstrap_id: "boot-test",
            idempotency_key: "personal-agent-bootstrap:test",
            app_instance_id: "app-test",
            controller_did: LOCAL_DID,
            user_subkey_package: ProbeUserSubkeyPackage {
                schema: "awiki.user-subkey-package.v2",
                user_did: LOCAL_DID,
                verification_method: "did:wba:example.test:user:local#daemon-key-1",
                key_type: "JsonWebKey2020",
                key_algorithm: "Ed25519",
                public_key_multibase: "zPublic",
                private_key_encoding: "pkcs8-pem",
                private_key_pem: "private-test-material",
                allowed_scopes: ["message.inbox.read.plain"],
            },
            desired_personal_agent: ProbeDesiredPersonalAgent {
                role: "app_message_handler",
                runtime: "hermes",
                runtime_provider: "hermes",
                runtime_profile: "personal_agent",
                display_name: "Recovery Continuity Agent",
                preferred_language: "zh-Hans",
                ensure_once_key: "app-personal-agent:test",
                runtime_registration_token: "registration-test-token",
            },
            capability_policy: ProbeCapabilityPolicy {
                schema: "awiki.app.capabilities.v1",
                capabilities: ["message.summarize_plain"],
                require_confirmation_for_write_actions: true,
            },
        };

        let serialized = serde_json::to_value(payload).expect("serialize Probe bootstrap payload");
        assert_eq!(
            serialized["desired_personal_agent"],
            json!({
                "role": "app_message_handler",
                "runtime": "hermes",
                "runtime_provider": "hermes",
                "runtime_profile": "personal_agent",
                "display_name": "Recovery Continuity Agent",
                "preferred_language": "zh-Hans",
                "ensure_once_key": "app-personal-agent:test",
                "runtime_registration_token": "registration-test-token",
            })
        );
    }

    fn complete_marker_absence() -> DaemonMarkerAbsenceEvidence {
        DaemonMarkerAbsenceEvidence {
            event_absent: true,
            route_absent: true,
            task_absent: true,
            run_absent: true,
            final_absent: true,
        }
    }

    fn complete_daemon_continuity_evidence() -> DaemonContinuityEvidence {
        DaemonContinuityEvidence {
            agent_identity_unchanged: true,
            root_key_unchanged: true,
            device_keys_unchanged: true,
            delegated_key_unchanged: true,
            old_controller_binding_unchanged: true,
            new_controller_lacks_delegated_key: true,
            controller_identity_changed: true,
            queued_delegated_marker: complete_marker_absence(),
            new_controller_marker: complete_marker_absence(),
        }
    }

    #[test]
    fn daemon_continuity_action_is_exact_and_rejects_extra_secret_fields() {
        let request = match parse_request(
            r#"{"id":"continuity-1","action":"daemon_continuity_verify","params":{"old_controller_did":"did:wba:example.test:user:old","new_controller_did":"did:wba:example.test:user:new","queued_marker":"msg-queued","controller_marker":"task_msg-new"}}"#,
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed daemon continuity request"),
        };
        let Action::DaemonContinuityVerify(params) = request.action else {
            panic!("daemon continuity action")
        };
        assert_eq!(params.old_controller_did, "did:wba:example.test:user:old");
        assert_eq!(params.new_controller_did, "did:wba:example.test:user:new");
        assert_eq!(params.queued_marker, "msg-queued");
        assert_eq!(params.controller_marker, "task_msg-new");

        let extra = match parse_request(
            r#"{"id":"continuity-2","action":"daemon_continuity_verify","params":{"old_controller_did":"did:wba:example.test:user:old","new_controller_did":"did:wba:example.test:user:new","queued_marker":"msg-queued","controller_marker":"task_msg-new","access_token":"must-not-be-accepted"}}"#,
        ) {
            Err(error) => error,
            Ok(_) => panic!("extra continuity fields must fail closed"),
        };
        assert_eq!(extra.code(), INVALID_REQUEST);
    }

    #[test]
    fn daemon_continuity_result_is_closed_exact_and_secret_free() {
        let result = closed_daemon_continuity_result(complete_daemon_continuity_evidence());
        assert_eq!(
            result,
            json!({
                "agent_identity_unchanged": true,
                "root_key_unchanged": true,
                "device_keys_unchanged": true,
                "delegated_key_unchanged": true,
                "old_controller_binding_unchanged": true,
                "new_controller_lacks_delegated_key": true,
                "old_delegated_pull_denied": true,
                "old_controller_denied": true,
                "new_controller_denied": true,
                "no_route_created": true,
                "no_task_created": true,
                "no_run_created": true,
                "no_final_created": true,
            })
        );
        let serialized = serde_json::to_string(&result).expect("serialize continuity result");
        for forbidden in [
            "did:wba:example.test:user:old",
            "did:wba:example.test:user:new",
            "msg-queued",
            "task_msg-new",
            TOKEN_SECRET,
            SERVER_ERROR_SECRET,
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn daemon_prepare_post_boundary_failures_return_exact_secret_free_root_receipts() {
        let daemon_agent_did = "did:wba:example.test:agent:daemon";
        for (stage, failure_stage) in [
            (
                "runtime-token",
                DaemonFixturePrepareFailureStage::RuntimeMaterial,
            ),
            ("subkey", DaemonFixturePrepareFailureStage::RuntimeMaterial),
            ("encrypt", DaemonFixturePrepareFailureStage::RuntimeMaterial),
            (
                "preflight-sync",
                DaemonFixturePrepareFailureStage::SyncInitializeCallFailed,
            ),
            ("send", DaemonFixturePrepareFailureStage::BootstrapSend),
        ] {
            let result = daemon_fixture_prepare_result_after_boundary(
                daemon_agent_did,
                Err(DaemonFixturePrepareFailure(failure_stage)),
            );
            assert_eq!(
                result,
                json!({
                    "prepared": false,
                    "daemon_agent_did": daemon_agent_did,
                    "failure_stage": failure_stage.as_str(),
                }),
                "closed partial receipt for injected {stage} failure",
            );
            let encoded = result.to_string();
            for forbidden in [
                "runtime_agent_did",
                "runtime-token",
                "subkey",
                "private_key",
                "ciphertext",
                "error",
            ] {
                assert!(
                    !encoded.contains(forbidden),
                    "{stage} receipt leaked {forbidden}",
                );
            }
        }
        assert_eq!(
            daemon_fixture_prepare_result_after_boundary(daemon_agent_did, Ok(())),
            json!({
                "prepared": true,
                "daemon_agent_did": daemon_agent_did,
                "failure_stage": null,
            }),
        );
    }

    #[test]
    fn daemon_prepare_early_failure_stages_are_exact_and_allowlisted() {
        let daemon_agent_did = "did:wba:example.test:agent:daemon";
        for stage in [
            DaemonFixturePrepareFailureStage::Token,
            DaemonFixturePrepareFailureStage::Setup,
        ] {
            assert_eq!(
                closed_daemon_fixture_prepare_result(false, daemon_agent_did, Some(stage)),
                json!({
                    "prepared": false,
                    "daemon_agent_did": daemon_agent_did,
                    "failure_stage": stage.as_str(),
                }),
            );
        }
    }

    #[tokio::test]
    async fn daemon_fixture_preflight_sync_is_required_before_bootstrap_send() {
        async fn exercise(
            sync_result: Result<im_core::messages::MessageSyncStatus, ProbeFailure>,
        ) -> (Result<(), DaemonFixturePrepareFailure>, Vec<&'static str>) {
            let observed = Arc::new(Mutex::new(Vec::new()));
            let sync_observed = Arc::clone(&observed);
            let send_observed = Arc::clone(&observed);
            let result = daemon_fixture_sync_before_send(
                move || {
                    sync_observed.lock().expect("sync observation").push("sync");
                    std::future::ready(sync_result)
                },
                move || {
                    assert_eq!(
                        *send_observed.lock().expect("pre-send observation"),
                        ["sync"]
                    );
                    send_observed.lock().expect("send observation").push("send");
                    std::future::ready(Ok(()))
                },
            )
            .await;
            let events = observed.lock().expect("final observation").clone();
            (result, events)
        }

        for status in [
            im_core::messages::MessageSyncStatus::Idle,
            im_core::messages::MessageSyncStatus::Changed,
        ] {
            let (result, events) = exercise(Ok(status)).await;
            assert!(result.is_ok());
            assert_eq!(events, ["sync", "send"]);
        }

        for (status, expected_stage) in [
            (
                im_core::messages::MessageSyncStatus::RecoveryRequired,
                DaemonFixturePrepareFailureStage::SyncInitializeRecoveryRequired,
            ),
            (
                im_core::messages::MessageSyncStatus::RetryableFailure,
                DaemonFixturePrepareFailureStage::SyncInitializeRetryableFailure,
            ),
            (
                im_core::messages::MessageSyncStatus::AuthRevoked,
                DaemonFixturePrepareFailureStage::SyncInitializeAuthRevoked,
            ),
        ] {
            let (result, events) = exercise(Ok(status)).await;
            assert!(matches!(
                result,
                Err(DaemonFixturePrepareFailure(stage)) if stage == expected_stage
            ));
            assert_eq!(events, ["sync"]);
        }

        let (result, events) = exercise(Err(ProbeFailure::Runtime)).await;
        assert!(matches!(
            result,
            Err(DaemonFixturePrepareFailure(
                DaemonFixturePrepareFailureStage::SyncInitializeCallFailed
            ))
        ));
        assert_eq!(events, ["sync"]);
    }

    #[test]
    fn daemon_root_stage_and_prepare_actions_are_closed_and_exact() {
        let stage = match parse_request(
            r#"{"id":"stage-1","action":"stage_daemon_continuity_root","params":{"state_root":"/tmp/daemon-stage","daemon_handle":"daemon-one"}}"#,
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed Daemon root stage request"),
        };
        let Action::StageDaemonContinuityRoot(params) = stage.action else {
            panic!("Daemon root stage action")
        };
        assert_eq!(params.state_root, Path::new("/tmp/daemon-stage"));
        assert_eq!(params.daemon_handle, "daemon-one");
        assert!(parse_request(
            r#"{"id":"stage-2","action":"stage_daemon_continuity_root","params":{"state_root":"/tmp/daemon-stage","daemon_handle":"daemon-one","access_token":"must-not-pass"}}"#,
        )
        .is_err());

        let prepare = match parse_request(
            r#"{"id":"prepare-1","action":"prepare_daemon_continuity_fixture","params":{"daemon_binary":"/tmp/awiki-deamon","state_root":"/tmp/daemon-stage","daemon_agent_did":"did:wba:example.test:agent:daemon","daemon_handle":"daemon-one","runtime_handle":"runtime-one","controller_handle":"controller-one","app_instance_id":"app-one"}}"#,
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed staged Daemon prepare request"),
        };
        let Action::PrepareDaemonContinuityFixture(params) = prepare.action else {
            panic!("staged Daemon prepare action")
        };
        assert_eq!(params.daemon_agent_did, "did:wba:example.test:agent:daemon");
        assert!(parse_request(
            r#"{"id":"prepare-2","action":"prepare_daemon_continuity_fixture","params":{"daemon_binary":"/tmp/awiki-deamon","state_root":"/tmp/daemon-stage","daemon_handle":"daemon-one","runtime_handle":"runtime-one","controller_handle":"controller-one","app_instance_id":"app-one"}}"#,
        )
        .is_err());
    }

    #[test]
    fn daemon_bootstrap_key_uses_exact_persisted_device_e2ee_binding() {
        let daemon_did = "did:wba:example.test:agent:daemon";
        let key_id = format!("{daemon_did}#dev-dynamic-e2ee");
        let expected_key = [0x5a_u8; 32];
        let mut multicodec_key = vec![0xec, 0x01];
        multicodec_key.extend_from_slice(&expected_key);
        let multibase = format!("z{}", bs58::encode(multicodec_key).into_string());
        let method = json!({
            "id": key_id,
            "type": "X25519KeyAgreementKey2019",
            "controller": daemon_did,
            "publicKeyMultibase": multibase,
        });
        let document = json!({
            "id": daemon_did,
            "verificationMethod": [method.clone()],
            "keyAgreement": [key_id],
        });
        let key = probe_ok(
            daemon_bootstrap_public_key(&document, daemon_did, &key_id),
            "dynamic device E2EE bootstrap key",
        );
        assert!(matches!(
            key,
            anp::PublicKeyMaterial::X25519(bytes) if bytes == expected_key
        ));

        let invalid_documents = [
            json!({
                "id": daemon_did,
                "verificationMethod": [method.clone()],
                "keyAgreement": [],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [method.clone()],
                "keyAgreement": [key_id.clone(), key_id.clone()],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [method.clone()],
                "keyAgreement": [format!("{daemon_did}#different-e2ee")],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [method.clone(), method.clone()],
                "keyAgreement": [key_id.clone()],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [{
                    "id": key_id,
                    "type": "X25519KeyAgreementKey2019",
                    "controller": "did:wba:example.test:agent:different",
                    "publicKeyMultibase": multibase,
                }],
                "keyAgreement": [key_id.clone()],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [{
                    "id": key_id,
                    "type": "Multikey",
                    "controller": daemon_did,
                    "publicKeyMultibase": multibase,
                }],
                "keyAgreement": [key_id.clone()],
            }),
            json!({
                "id": daemon_did,
                "verificationMethod": [{
                    "id": key_id,
                    "type": "X25519KeyAgreementKey2019",
                    "controller": daemon_did,
                    "publicKeyMultibase": "not-multibase",
                }],
                "keyAgreement": [key_id.clone()],
            }),
        ];
        for invalid in invalid_documents {
            assert!(matches!(
                daemon_bootstrap_public_key(&invalid, daemon_did, &key_id),
                Err(ProbeFailure::InvalidState)
            ));
        }
        assert!(matches!(
            daemon_bootstrap_public_key(
                &document,
                daemon_did,
                "did:wba:example.test:agent:different#dev-e2ee",
            ),
            Err(ProbeFailure::InvalidState)
        ));
    }

    #[tokio::test]
    async fn daemon_root_is_durable_before_token_issue_and_closes_response_loss_without_inventory()
    {
        let root = TestStateRoot::new("daemon-preissue-root-authority");
        let mut probe = test_probe("http://127.0.0.1:9");
        let staged = probe
            .execute(Action::StageDaemonContinuityRoot(
                StageDaemonContinuityRootParams {
                    state_root: root.path.clone(),
                    daemon_handle: "daemon-preissue".to_owned(),
                },
            ))
            .await
            .unwrap_or_else(|_| panic!("execute local-only Daemon root stage"))
            .0;
        let staged_did = staged
            .get("daemon_agent_did")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("exact staged Daemon root receipt"))
            .to_owned();
        assert_eq!(staged.as_object().map(Map::len), Some(1));
        let config = awiki_deamon::DaemonConfig::for_state_root(&root.path)
            .unwrap_or_else(|_| panic!("daemon config"));
        let state =
            awiki_deamon::DaemonState::open(&config).unwrap_or_else(|_| panic!("daemon state"));
        let authority = awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
            &state,
            LOCAL_DID,
            "daemon-preissue",
            &staged_did,
        )
        .unwrap_or_else(|_| panic!("load pre-issue Daemon root"));
        im_core::ids::Did::parse(&authority.agent_did)
            .unwrap_or_else(|_| panic!("authoritative Daemon DID"));
        assert!(
            awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
                &state,
                LOCAL_DID,
                "daemon-preissue",
                "did:wba:example.test:agent:wrong",
            )
            .is_err()
        );
        assert!(
            awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
                &state,
                LOCAL_DID,
                "daemon-not-staged",
                &authority.agent_did,
            )
            .is_err()
        );
        let loaded = awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
            &state,
            LOCAL_DID,
            "daemon-preissue",
            &authority.agent_did,
        )
        .unwrap_or_else(|_| panic!("load exact staged Daemon root"));
        assert_eq!(loaded.agent_did, authority.agent_did);
        assert!(state
            .list_agent_definitions()
            .unwrap_or_else(|_| panic!("list local Agent definitions"))
            .is_empty());
        assert_eq!(
            probe_ok(
                recover_exact_persisted_daemon_agent_did(&root.path, "daemon-preissue", LOCAL_DID,),
                "recover pre-issue root",
            )
            .as_deref(),
            Some(authority.agent_did.as_str()),
        );
        assert_eq!(
            daemon_registration_metadata(&authority.agent_did),
            json!({
                "suite_case": "handle-recovery-daemon-continuity",
                "daemon_agent_did": authority.agent_did,
            }),
        );

        let response_loss =
            daemon_token_issue_or_root_receipt(&authority.agent_did, Err(ProbeFailure::Transport));
        let receipt = match response_loss {
            Err(receipt) => receipt,
            Ok(_) => panic!("injected issue response loss must return root receipt"),
        };
        assert_eq!(
            receipt,
            json!({
                "prepared": false,
                "daemon_agent_did": authority.agent_did,
                "failure_stage": "token",
            }),
        );
        let encoded = receipt.to_string();
        for forbidden in [TOKEN_SECRET, "registration_token", "token_id", "error"] {
            assert!(!encoded.contains(forbidden));
        }

        awiki_deamon::commands::bind_daemon_registration_token_for_system_test(
            &state,
            &authority,
            awiki_deamon::registration::RegistrationToken::new(TOKEN_SECRET)
                .unwrap_or_else(|_| panic!("test registration token")),
        )
        .unwrap_or_else(|_| panic!("atomically bind issued token to staged root"));
        assert!(
            awiki_deamon::commands::load_daemon_registration_authority_for_system_test(
                &state,
                LOCAL_DID,
                "daemon-preissue",
                &authority.agent_did,
            )
            .is_err()
        );
        assert_eq!(
            probe_ok(
                recover_exact_persisted_daemon_agent_did(&root.path, "daemon-preissue", LOCAL_DID,),
                "recover token-bound root",
            )
            .as_deref(),
            Some(staged_did.as_str()),
        );
    }

    #[test]
    fn daemon_setup_failure_returns_root_only_for_exact_durable_scope() {
        let root = TestStateRoot::new("daemon-prepare-setup-recovery");
        let no_scope = daemon_setup_failure_result(
            &root.path,
            "daemon-one",
            LOCAL_DID,
            ProbeFailure::Transport,
        );
        assert!(matches!(no_scope, Err(ProbeFailure::Transport)));

        let config = awiki_deamon::DaemonConfig::for_state_root(&root.path)
            .unwrap_or_else(|_| panic!("daemon config"));
        config
            .ensure_state_layout()
            .unwrap_or_else(|_| panic!("daemon state layout"));
        let state =
            awiki_deamon::DaemonState::open(&config).unwrap_or_else(|_| panic!("daemon state"));
        state
            .initialize()
            .unwrap_or_else(|_| panic!("daemon state schema"));
        let pending_did = "did:wba:example.test:agent:pending-daemon";
        state
            .connection()
            .unwrap_or_else(|_| panic!("daemon connection"))
            .execute(
                r#"
INSERT INTO agent_registration_pending (
    registration_id, dedupe_key, agent_kind, controller_did, handle,
    display_name, agent_did, protocol_device_id, document_digest,
    request_digest, secret_ref_json, status, attempt_count,
    last_error_code, last_error_summary, created_at_ms, updated_at_ms
) VALUES (
    'agentreg-test', 'dedupe-test', 'daemon', ?1, ?2,
    'daemon-one', ?3, 'device-test', 'document-test',
    'request-test', '{}', 'retryable', 1,
    'registration_exchange_failed', NULL, 1, 1
)
"#,
                rusqlite::params![LOCAL_DID, "daemon-one", pending_did],
            )
            .unwrap_or_else(|error| panic!("insert exact pending daemon scope: {error}"));
        assert_eq!(
            probe_ok(
                daemon_setup_failure_result(
                    &root.path,
                    "daemon-one",
                    LOCAL_DID,
                    ProbeFailure::Runtime,
                ),
                "pending daemon root receipt",
            ),
            json!({
                "prepared": false,
                "daemon_agent_did": pending_did,
                "failure_stage": "setup",
            }),
        );
        state
            .connection()
            .unwrap_or_else(|_| panic!("daemon connection"))
            .execute("DELETE FROM agent_registration_pending", [])
            .unwrap_or_else(|_| panic!("clear pending daemon scope"));
        let active_did = "did:wba:example.test:agent:active-daemon";
        state
            .upsert_agent_definition(&awiki_deamon::agent::AgentDefinition {
                agent_did: active_did.to_owned(),
                handle: "daemon-one".to_owned(),
                agent_kind: awiki_deamon::agent::AgentKind::Daemon,
                controller_user_id: "controller-user".to_owned(),
                controller_full_handle: "controller.example.test".to_owned(),
                controller_scope_key: "controller-scope".to_owned(),
                controller_did: LOCAL_DID.to_owned(),
                runtime_plugin_id: None,
                runtime_profile_id: None,
                workspace_id: None,
                policy_id: "default".to_owned(),
                local_agent_db_path: "agent.db".to_owned(),
                message_db_path: "message.db".to_owned(),
                status: "active".to_owned(),
            })
            .unwrap_or_else(|_| panic!("insert exact active daemon scope"));
        assert_eq!(
            probe_ok(
                daemon_setup_failure_result(
                    &root.path,
                    "daemon-one",
                    LOCAL_DID,
                    ProbeFailure::Runtime,
                ),
                "active daemon root receipt",
            ),
            json!({
                "prepared": false,
                "daemon_agent_did": active_did,
                "failure_stage": "setup",
            }),
        );
        assert!(matches!(
            daemon_setup_failure_result(
                &root.path,
                "different-handle",
                LOCAL_DID,
                ProbeFailure::Runtime,
            ),
            Err(ProbeFailure::Runtime)
        ));
        assert!(matches!(
            daemon_setup_failure_result(
                &root.path,
                "daemon-one",
                "did:wba:example.test:user:different",
                ProbeFailure::Runtime,
            ),
            Err(ProbeFailure::Runtime)
        ));
    }

    #[test]
    fn daemon_fixture_resources_reports_closed_persisted_stage_without_exposing_audit_detail() {
        let root = TestStateRoot::new("daemon-fixture-resource-stage");
        let config =
            awiki_deamon::DaemonConfig::for_state_root(&root.path).expect("daemon probe config");
        config.ensure_state_layout().expect("daemon state layout");
        let state = awiki_deamon::DaemonState::open(&config).expect("daemon state");
        state.initialize().expect("daemon state schema");
        let mut probe = test_probe("http://127.0.0.1:9");
        probe.daemon_state_root = Some(root.path.clone());
        probe.local_did = DAEMON_DID.to_owned();
        let missing_definition = match probe.daemon_fixture_resources() {
            Err(failure) => failure,
            Ok(_) => panic!("missing Daemon definition must fail closed"),
        };
        assert_eq!(
            missing_definition.code(),
            DAEMON_FIXTURE_BOOTSTRAP_VALIDATION_OR_PERSIST
        );
        state
            .upsert_agent_definition(&awiki_deamon::agent::AgentDefinition {
                agent_did: DAEMON_DID.to_owned(),
                handle: "fixture-daemon".to_owned(),
                agent_kind: awiki_deamon::agent::AgentKind::Daemon,
                controller_user_id: "fixture-human-user".to_owned(),
                controller_full_handle: "fixture-human.example.test".to_owned(),
                controller_scope_key: "fixture-human-scope".to_owned(),
                controller_did: HUMAN_DID.to_owned(),
                runtime_plugin_id: None,
                runtime_profile_id: None,
                workspace_id: None,
                policy_id: "default".to_owned(),
                local_agent_db_path: "agent.db".to_owned(),
                message_db_path: "message.db".to_owned(),
                status: "active".to_owned(),
            })
            .expect("store active Daemon definition");

        let assert_closed_failure = |expected_code: &str| {
            let failure = match probe.daemon_fixture_resources() {
                Err(failure) => failure,
                Ok(_) => panic!("missing binding must fail closed"),
            };
            assert_eq!(failure.code(), expected_code);
            let response = failure_response(RequestId(json!("resources")), failure);
            assert_eq!(
                response,
                json!({
                    "id": "resources",
                    "ok": false,
                    "error": {"code": expected_code},
                })
            );
            let encoded = response.to_string();
            for forbidden in [
                HUMAN_DID,
                DAEMON_DID,
                "audit-bootstrap-secret",
                "audit-registration-secret",
                "audit-binding-secret",
            ] {
                assert!(!encoded.contains(forbidden));
            }
        };

        assert_closed_failure("probe.daemon_fixture.bootstrap_message_not_routed");

        state
            .insert_audit_event_json(
                "daemon.inbox.message.route.failed",
                Some("did:wba:example.test:agent:different"),
                None,
                None,
                None,
                json!({}),
            )
            .expect("insert unrelated route failure");
        assert_closed_failure("probe.daemon_fixture.bootstrap_message_not_routed");

        state
            .insert_audit_event_json(
                "daemon.inbox.message.route.failed",
                Some(DAEMON_DID),
                None,
                None,
                None,
                json!({}),
            )
            .expect("insert exact route failure");
        assert_closed_failure("probe.daemon_fixture.bootstrap_secure_envelope");

        let secure_replay = |operation_suffix: &str,
                             nonce: &str,
                             sender_human_did: &str,
                             recipient_daemon_did: &str| {
            awiki_deamon::state::SecureBootstrapReplayRecord {
                operation_id: format!("personal-agent-bootstrap:{operation_suffix}"),
                nonce: nonce.to_owned(),
                envelope_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_owned(),
                recipient_daemon_did: recipient_daemon_did.to_owned(),
                recipient_key_id: format!("{recipient_daemon_did}#daemon-key-1"),
                sender_human_did: sender_human_did.to_owned(),
                bootstrap_id: format!("boot-{operation_suffix}"),
                idempotency_key: format!("personal-agent-bootstrap:{operation_suffix}"),
                payload_sha256: None,
                expires_at: "2026-09-09T00:00:00Z".to_owned(),
                status:
                    awiki_deamon::app_bridge::bootstrap::DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED
                        .to_owned(),
                created_at_ms: 0,
                updated_at_ms: 0,
            }
        };
        state
            .store_secure_bootstrap_replay(&secure_replay(
                "unrelated",
                "AQEBAQEBAQEBAQEB",
                "did:wba:example.test:user:different",
                "did:wba:example.test:agent:different",
            ))
            .expect("store unrelated secure replay");
        assert_closed_failure("probe.daemon_fixture.bootstrap_secure_envelope");

        state
            .store_secure_bootstrap_replay(&secure_replay(
                "exact",
                "AgICAgICAgICAgIC",
                HUMAN_DID,
                DAEMON_DID,
            ))
            .expect("store exact secure replay");
        assert_closed_failure("probe.daemon_fixture.bootstrap_state_persist");

        let verification_method = format!("{HUMAN_DID}#daemon-key-1");
        let delegated = awiki_deamon::state::UserDelegatedIdentityRecord {
            user_did: HUMAN_DID.to_owned(),
            verification_method: verification_method.clone(),
            app_instance_id: "app-stage-diagnostic".to_owned(),
            controller_did: HUMAN_DID.to_owned(),
            daemon_agent_did: DAEMON_DID.to_owned(),
            public_key_multibase: "z-stage-diagnostic-public".to_owned(),
            private_key_material: "stage-diagnostic-private".to_owned(),
            private_key_ref_json: None,
            allowed_scopes_json: json!(["message.inbox.read.plain"]),
            status: "paired_key_received".to_owned(),
            expires_at: None,
            bootstrap_id: "boot-stage-diagnostic".to_owned(),
            idempotency_key: "personal-agent-bootstrap:stage-diagnostic".to_owned(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let replay = awiki_deamon::state::BootstrapReplayRecord {
            bootstrap_id: delegated.bootstrap_id.clone(),
            idempotency_key: delegated.idempotency_key.clone(),
            payload_hash: "stage-diagnostic-payload".to_owned(),
            user_did: delegated.user_did.clone(),
            verification_method: delegated.verification_method.clone(),
            app_instance_id: delegated.app_instance_id.clone(),
            daemon_agent_did: delegated.daemon_agent_did.clone(),
            status: delegated.status.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        state
            .store_bootstrap_state(&delegated, &replay)
            .expect("store exact delegated bootstrap state");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET user_did = ?1 WHERE verification_method = ?2",
                rusqlite::params!["did:wba:example.test:user:different", verification_method],
            )
            .expect("make delegated identity contract-invalid");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET user_did = ?1 WHERE verification_method = ?2",
                rusqlite::params![HUMAN_DID, verification_method],
            )
            .expect("restore delegated identity contract");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET daemon_agent_did = ?1 WHERE verification_method = ?2",
                rusqlite::params!["did:wba:example.test:agent:different", verification_method],
            )
            .expect("make delegated Daemon contract-invalid");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET daemon_agent_did = ?1 WHERE verification_method = ?2",
                rusqlite::params![DAEMON_DID, verification_method],
            )
            .expect("restore delegated Daemon contract");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET controller_did = ?1 WHERE verification_method = ?2",
                rusqlite::params!["did:wba:example.test:user:different", verification_method],
            )
            .expect("make delegated Controller contract-invalid");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET controller_did = ?1 WHERE verification_method = ?2",
                rusqlite::params![HUMAN_DID, verification_method],
            )
            .expect("restore delegated Controller contract");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET status = 'revoked' WHERE verification_method = ?1",
                [&verification_method],
            )
            .expect("make delegated status contract-invalid");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET status = ?1 WHERE verification_method = ?2",
                rusqlite::params![
                    awiki_deamon::app_bridge::bootstrap::DAEMON_BOOTSTRAP_STATUS_PAIRED_KEY_RECEIVED,
                    verification_method,
                ],
            )
            .expect("restore delegated status contract");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET allowed_scopes_json = '{}' WHERE verification_method = ?1",
                [&verification_method],
            )
            .expect("make delegated record validation-invalid");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE user_delegated_identity SET allowed_scopes_json = '[\"message.inbox.read.plain\"]' WHERE verification_method = ?1",
                [&verification_method],
            )
            .expect("restore delegated record validation contract");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE agent_definition SET status = 'inactive' WHERE agent_did = ?1",
                [DAEMON_DID],
            )
            .expect("make Daemon definition inactive");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE agent_definition SET status = 'active' WHERE agent_did = ?1",
                [DAEMON_DID],
            )
            .expect("restore active Daemon definition");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE agent_definition SET agent_kind = 'runtime' WHERE agent_did = ?1",
                [DAEMON_DID],
            )
            .expect("make definition kind non-Daemon");
        assert_closed_failure("probe.daemon_fixture.bootstrap_validation_or_persist");
        state
            .connection()
            .expect("daemon state connection")
            .execute(
                "UPDATE agent_definition SET agent_kind = 'daemon' WHERE agent_did = ?1",
                [DAEMON_DID],
            )
            .expect("restore Daemon definition kind");
        assert_closed_failure("probe.daemon_fixture.bootstrap_received_audit");

        state
            .insert_audit_event_json(
                "daemon.bootstrap.received",
                Some(DAEMON_DID),
                None,
                None,
                None,
                json!({"private_detail": "audit-bootstrap-secret"}),
            )
            .expect("insert bootstrap audit event");
        assert_closed_failure("probe.daemon_fixture.runtime_registration_prepare_or_exchange");

        state
            .insert_audit_event_json(
                "agent.registration.exchange",
                Some("did:wba:example.test:agent:unrelated"),
                None,
                None,
                None,
                json!({
                    "agent_kind": "daemon",
                    "private_detail": "audit-registration-secret",
                }),
            )
            .expect("insert unrelated registration audit event");
        assert_closed_failure("probe.daemon_fixture.runtime_registration_prepare_or_exchange");

        state
            .insert_audit_event_json(
                "agent.registration.exchange",
                Some("did:wba:example.test:agent:runtime"),
                None,
                None,
                None,
                json!({
                    "agent_kind": "runtime",
                    "private_detail": "audit-registration-secret",
                }),
            )
            .expect("insert runtime registration audit event");
        assert_closed_failure("probe.daemon_fixture.binding_persist");

        state
            .insert_audit_event_json(
                "app_personal_agent.binding.ready",
                Some(DAEMON_DID),
                None,
                None,
                None,
                json!({"private_detail": "audit-binding-secret"}),
            )
            .expect("insert binding ready audit event");
        assert_closed_failure("probe.daemon_fixture.binding_projection");
    }

    #[test]
    fn daemon_fixture_resources_keeps_delegated_identity_load_error_on_broad_closed_failure() {
        let root = TestStateRoot::new("daemon-fixture-resource-load-error");
        let config =
            awiki_deamon::DaemonConfig::for_state_root(&root.path).expect("daemon probe config");
        config.ensure_state_layout().expect("daemon state layout");
        let state = awiki_deamon::DaemonState::open(&config).expect("daemon state");
        state.initialize().expect("daemon state schema");
        state
            .upsert_agent_definition(&awiki_deamon::agent::AgentDefinition {
                agent_did: DAEMON_DID.to_owned(),
                handle: "fixture-daemon".to_owned(),
                agent_kind: awiki_deamon::agent::AgentKind::Daemon,
                controller_user_id: "fixture-human-user".to_owned(),
                controller_full_handle: "fixture-human.example.test".to_owned(),
                controller_scope_key: "fixture-human-scope".to_owned(),
                controller_did: HUMAN_DID.to_owned(),
                runtime_plugin_id: None,
                runtime_profile_id: None,
                workspace_id: None,
                policy_id: "default".to_owned(),
                local_agent_db_path: "agent.db".to_owned(),
                message_db_path: "message.db".to_owned(),
                status: "active".to_owned(),
            })
            .expect("store active Daemon definition");
        state
            .connection()
            .expect("daemon state connection")
            .execute("DROP TABLE user_delegated_identity", [])
            .expect("remove delegated identity table");
        let mut probe = test_probe("http://127.0.0.1:9");
        probe.daemon_state_root = Some(root.path.clone());
        probe.local_did = DAEMON_DID.to_owned();

        let failure = match probe.daemon_fixture_resources() {
            Err(failure) => failure,
            Ok(_) => panic!("delegated identity load error must fail closed"),
        };
        assert_eq!(
            failure.code(),
            DAEMON_FIXTURE_BOOTSTRAP_VALIDATION_OR_PERSIST
        );
        assert_eq!(
            failure_response(RequestId(json!("resources")), failure),
            json!({
                "id": "resources",
                "ok": false,
                "error": {"code": DAEMON_FIXTURE_BOOTSTRAP_VALIDATION_OR_PERSIST},
            })
        );
    }

    #[test]
    fn daemon_continuity_residual_route_run_or_final_fails_the_bound_projection() {
        for residual in ["route", "run", "final"] {
            let mut evidence = complete_daemon_continuity_evidence();
            match residual {
                "route" => evidence.queued_delegated_marker.route_absent = false,
                "run" => evidence.queued_delegated_marker.run_absent = false,
                "final" => evidence.queued_delegated_marker.final_absent = false,
                _ => unreachable!(),
            }
            let result = closed_daemon_continuity_result(evidence);
            assert_eq!(result["old_controller_denied"], false, "{residual}");
            let aggregate_key = format!("no_{residual}_created");
            assert_eq!(result[aggregate_key.as_str()], false);

            let mut evidence = complete_daemon_continuity_evidence();
            match residual {
                "route" => evidence.new_controller_marker.route_absent = false,
                "run" => evidence.new_controller_marker.run_absent = false,
                "final" => evidence.new_controller_marker.final_absent = false,
                _ => unreachable!(),
            }
            let result = closed_daemon_continuity_result(evidence);
            assert_eq!(result["new_controller_denied"], false, "{residual}");
            let aggregate_key = format!("no_{residual}_created");
            assert_eq!(result[aggregate_key.as_str()], false);
        }
    }

    #[test]
    fn daemon_marker_queries_are_exact_independent_and_cover_every_persisted_stage() {
        let root = TestStateRoot::new("daemon-continuity-marker-evidence");
        let config =
            awiki_deamon::DaemonConfig::for_state_root(&root.path).expect("daemon probe config");
        config.ensure_state_layout().expect("daemon state layout");
        let state = awiki_deamon::DaemonState::open(&config).expect("daemon state");
        state.initialize().expect("daemon state schema");
        let old_controller_did = "did:wba:example.test:user:old";
        let queued = probe_ok(
            queued_delegated_marker_ids(old_controller_did, "msg-queued"),
            "queued delegated marker ids",
        );
        let new_controller = probe_ok(
            new_controller_marker_ids("task_msg-new"),
            "new Controller marker ids",
        );
        let route_baseline = probe_ok(daemon_route_state_snapshot(&state), "route baseline");
        assert_eq!(route_baseline.record_count, 0);
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &queued),
            "queued marker absence",
        )
        .all_absent());
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &new_controller),
            "new Controller marker absence",
        )
        .all_absent());

        let connection = state.connection().expect("daemon state connection");
        let retry_run_id = format!("{}_retry_1", new_controller.run_id);
        connection
            .execute(
                r#"
INSERT INTO message_event (
    event_id, owner_did, conversation_id, message_id, message_kind, sender_did,
    received_at, plain_text_ref_or_excerpt, content_hash, schema,
    processing_status, retention_class, created_at_ms, updated_at_ms
) VALUES (?1, ?2, NULL, ?3, 'text', ?2, NULL, NULL, 'hash', 'test', 'received', 'test', 1, 1)
"#,
                rusqlite::params![
                    queued.event_id.as_deref(),
                    old_controller_did,
                    queued.message_id,
                ],
            )
            .expect("insert queued event residue");
        assert!(
            !probe_ok(
                daemon_marker_absence_evidence(&state, &queued),
                "queued event residue",
            )
            .event_absent
        );
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &new_controller),
            "new marker remains independent",
        )
        .all_absent());
        connection
            .execute("DELETE FROM message_event", [])
            .expect("clear event residue");

        connection
            .execute(
                r#"
INSERT INTO runtime_task (
    task_id, agent_did, controller_did, sender_did, task_text,
    status, created_at_ms, updated_at_ms
) VALUES (?1, 'did:agent:test', ?2, ?2, 'residue', 'created', 1, 1)
"#,
                rusqlite::params![new_controller.task_id, "did:wba:example.test:user:new"],
            )
            .expect("insert new Controller task residue");
        assert!(
            !probe_ok(
                daemon_marker_absence_evidence(&state, &new_controller),
                "new Controller task residue",
            )
            .task_absent
        );
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &queued),
            "queued marker remains independent",
        )
        .all_absent());
        connection
            .execute("DELETE FROM runtime_task", [])
            .expect("clear task residue");

        connection
            .execute(
                r#"
INSERT INTO cli_driver_run (
    run_id, agent_did, runtime_profile_id, driver_id, controller_did,
    route_key, status, created_at_ms, updated_at_ms
) VALUES (?1, 'did:agent:test', 'profile-test', 'driver-test', ?2, 'route-test', 'running', 1, 1)
"#,
                rusqlite::params![new_controller.run_id, "did:wba:example.test:user:new",],
            )
            .expect("insert route residue");
        assert!(
            !probe_ok(
                daemon_marker_absence_evidence(&state, &new_controller),
                "route residue",
            )
            .route_absent
        );
        let route_changed = probe_ok(daemon_route_state_snapshot(&state), "changed route state");
        assert_ne!(route_changed.record_count, route_baseline.record_count);
        assert_ne!(route_changed.hash, route_baseline.hash);
        connection
            .execute("DELETE FROM cli_driver_run", [])
            .expect("clear route residue");

        connection
            .execute(
                r#"
INSERT INTO runtime_run (
    run_id, task_id, agent_did, runtime_profile_id, runtime_plugin_id,
    status, started_at, updated_at, started_at_ms, updated_at_ms
) VALUES (?1, ?2, 'did:agent:test', 'profile-test', 'plugin-test', 'running', '1', '1', 1, 1)
"#,
                rusqlite::params![retry_run_id, new_controller.task_id],
            )
            .expect("insert retry run residue");
        assert!(
            !probe_ok(
                daemon_marker_absence_evidence(&state, &new_controller),
                "retry run residue",
            )
            .run_absent
        );
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &queued),
            "queued marker remains independent from retry run residue",
        )
        .all_absent());

        connection
            .execute(
                r#"
INSERT INTO runtime_final_outbox (
    idempotency_key, run_id, agent_did, runtime_profile_id, controller_did,
    final_text, status, created_at_ms, updated_at_ms
) VALUES ('final-test', ?1, 'did:agent:test', 'profile-test', ?2, 'residue', 'pending', 1, 1)
"#,
                rusqlite::params![retry_run_id, "did:wba:example.test:user:new",],
            )
            .expect("insert retry final residue");
        assert!(
            !probe_ok(
                daemon_marker_absence_evidence(&state, &new_controller),
                "retry final residue",
            )
            .final_absent
        );
        assert!(probe_ok(
            daemon_marker_absence_evidence(&state, &queued),
            "queued marker remains independent from retry final residue",
        )
        .all_absent());
    }

    #[test]
    fn agent_bootstrap_workspace_index_accepts_current_and_legacy_shapes_exactly() {
        let current = json!({
            "schema_version": 3,
            "credentials": {
                "skill": {"did": LOCAL_DID, "dir_name": "current-dir"},
            },
        });
        assert_eq!(
            workspace_identity_dir_name(&current, LOCAL_DID)
                .unwrap_or_else(|_| panic!("current index")),
            Some("current-dir".to_owned())
        );

        let legacy = json!({
            "identities": [{"did": LOCAL_DID, "dir_name": "legacy-dir"}],
        });
        assert_eq!(
            workspace_identity_dir_name(&legacy, LOCAL_DID)
                .unwrap_or_else(|_| panic!("legacy index")),
            Some("legacy-dir".to_owned())
        );

        let duplicate = json!({
            "credentials": {
                "one": {"did": LOCAL_DID, "dir_name": "one"},
                "two": {"did": LOCAL_DID, "dir_name": "two"},
            },
        });
        assert_eq!(
            workspace_identity_dir_name(&duplicate, LOCAL_DID)
                .unwrap_or_else(|_| panic!("duplicate index")),
            None
        );
        assert!(workspace_identity_dir_name(&json!({}), LOCAL_DID).is_err());
    }

    #[test]
    fn agent_bootstrap_registry_projection_requires_one_ready_admin() {
        let device_id = "device-local";
        let signing_key_id = format!("{LOCAL_DID}#device-local-sign");
        let e2ee_key_id = format!("{LOCAL_DID}#device-local-e2ee");
        let registry = json!({
            "did": LOCAL_DID,
            "checkpoint": {
                "document_version": 1,
                "document_hash": "document-hash",
                "registry_version": 1,
            },
            "devices": [{
                "device_id": device_id,
                "signing_key_id": signing_key_id,
                "e2ee_key_id": e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1,
            }],
        });
        assert!(bootstrap_registry_matches(
            &registry,
            LOCAL_DID,
            device_id,
            &signing_key_id,
            &e2ee_key_id,
            "1",
            "document-hash",
        ));

        for mutation in [
            json!({"registry_version": 2}),
            json!({"management_ready": false}),
            json!({"auth_generation": 2}),
            json!({"document_hash": "other-document-hash"}),
        ] {
            let mut invalid = registry.clone();
            if let Some(registry_version) = mutation.get("registry_version") {
                invalid["checkpoint"]["registry_version"] = registry_version.clone();
            }
            if let Some(management_ready) = mutation.get("management_ready") {
                invalid["devices"][0]["management_ready"] = management_ready.clone();
            }
            if let Some(auth_generation) = mutation.get("auth_generation") {
                invalid["devices"][0]["auth_generation"] = auth_generation.clone();
            }
            if let Some(document_hash) = mutation.get("document_hash") {
                invalid["checkpoint"]["document_hash"] = document_hash.clone();
            }
            assert!(!bootstrap_registry_matches(
                &invalid,
                LOCAL_DID,
                device_id,
                &signing_key_id,
                &e2ee_key_id,
                "1",
                "document-hash",
            ));
        }
    }

    #[test]
    fn agent_bootstrap_manifest_requires_exact_remote_generation_and_principal() {
        let manifest = json!({
            "account_id": "account-local",
            "current_did": LOCAL_DID,
            "identity_generation": "1",
            "versions": {"device_registry": "1"},
        });
        assert!(bootstrap_manifest_matches(
            &manifest,
            "account-local",
            LOCAL_DID,
            "1",
        ));
        for (pointer, value) in [
            ("/account_id", json!("account-other")),
            ("/current_did", json!("did:wba:example.test:agent:other")),
            ("/identity_generation", json!("2")),
            ("/versions/device_registry", json!("2")),
        ] {
            let mut invalid = manifest.clone();
            *invalid.pointer_mut(pointer).expect("manifest field") = value;
            assert!(!bootstrap_manifest_matches(
                &invalid,
                "account-local",
                LOCAL_DID,
                "1",
            ));
        }
    }

    #[test]
    fn agent_bootstrap_controller_binding_projection_is_three_state() {
        assert_eq!(
            controller_binding_projection(None, "controller-account"),
            None
        );
        assert_eq!(
            controller_binding_projection(Some("controller-account"), "controller-account"),
            Some(true)
        );
        assert_eq!(
            controller_binding_projection(Some("other-account"), "controller-account"),
            Some(false)
        );
        assert!(
            json!({"controller_binding_matches": controller_binding_projection(
                None,
                "controller-account"
            )})["controller_binding_matches"]
                .is_null()
        );
    }

    #[test]
    fn agent_bootstrap_key_roles_require_distinct_public_material() {
        let root_id = format!("{LOCAL_DID}#key-1");
        let signing_id = format!("{LOCAL_DID}#device-local-sign");
        let e2ee_id = format!("{LOCAL_DID}#device-local-e2ee");
        let document = json!({
            "verificationMethod": [
                {"id": root_id, "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": "root"}},
                {"id": signing_id, "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": "signing"}},
                {"id": e2ee_id, "publicKeyJwk": {"kty": "OKP", "crv": "X25519", "x": "e2ee"}},
            ],
        });
        assert!(key_roles_separated(
            &document,
            &root_id,
            &signing_id,
            &e2ee_id,
        ));

        let mut reused = document;
        reused["verificationMethod"][1]["publicKeyJwk"] =
            reused["verificationMethod"][0]["publicKeyJwk"].clone();
        assert!(!key_roles_separated(
            &reused,
            &root_id,
            &signing_id,
            &e2ee_id,
        ));
    }

    #[test]
    fn agent_device_access_projection_is_secret_free_and_rejects_extensions() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Unix time")
            .as_secs();
        let device_id = "device-local";
        let signing_key_id = format!("{LOCAL_DID}#device-local-sign");
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": LOCAL_DID,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": LOCAL_DID,
            "user_id": "agent-account",
            "device_id": device_id,
            "key_id": signing_key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 3600,
            "jti": "device-access-token-id",
        });
        let token = Zeroizing::new(format!(
            "e30.{}.test-signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"))
        ));
        assert_eq!(
            device_access_projection_matches(
                &token,
                LOCAL_DID,
                "agent-account",
                device_id,
                &signing_key_id,
                "1",
            ),
            (true, true)
        );

        let mut extended = claims;
        extended["sync_capability"] = json!("sync-v2-secret");
        let token = Zeroizing::new(format!(
            "e30.{}.test-signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&extended).expect("claims"))
        ));
        assert_eq!(
            device_access_projection_matches(
                &token,
                LOCAL_DID,
                "agent-account",
                device_id,
                &signing_key_id,
                "1",
            ),
            (false, false)
        );
        assert!(!format!("{:?}", (false, false)).contains("sync-v2-secret"));
    }

    #[test]
    fn direct_wire_projection_request_is_exact_and_rejects_invalid_selectors() {
        let exact = match parse_request(
            &json!({
                "id": "wire-1",
                "action": "direct_wire_projection",
                "params": {
                    "peer_did": SENDER_DID,
                    "message_id": "message-1",
                    "expected_shape": "init",
                    "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
                }
            })
            .to_string(),
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed direct-wire request"),
        };
        let Action::DirectWireProjection(params) = exact.action else {
            panic!("direct-wire action")
        };
        assert_eq!(params.peer_did, SENDER_DID);
        assert_eq!(params.message_id, "message-1");
        assert!(matches!(params.expected_shape, DirectWireShape::Init));
        assert_eq!(params.forbidden_plaintext.as_str(), WIRE_PLAINTEXT_SECRET);

        for invalid_params in [
            json!({
                "peer_did": SENDER_DID,
                "message_id": "message-1",
                "expected_shape": "unknown",
                "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
            }),
            json!({
                "peer_did": "not-a-did",
                "message_id": "message-1",
                "expected_shape": "cipher",
                "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
            }),
            json!({
                "peer_did": SENDER_DID,
                "message_id": "",
                "expected_shape": "cipher",
                "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
            }),
            json!({
                "peer_did": SENDER_DID,
                "message_id": "message-1",
                "expected_shape": "cipher",
                "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
                "raw": true,
            }),
        ] {
            let raw = json!({
                "id": "wire-invalid",
                "action": "direct_wire_projection",
                "params": invalid_params,
            })
            .to_string();
            let error = match parse_request(&raw) {
                Err(error) => error,
                Ok(_) => panic!("invalid direct-wire request must fail"),
            };
            assert_eq!(error.code(), INVALID_REQUEST);
        }
    }

    #[test]
    fn direct_wire_projection_is_closed_for_init_cipher_duplicate_and_plaintext() {
        let init_params = direct_wire_params("message-init", DirectWireShape::Init);
        let init_message = json!({
            "id": "message-init",
            "sender_did": SENDER_DID,
            "type": "json",
            "content_type": DIRECT_INIT_CONTENT_TYPE,
            "content": {
                "session_id": "session-1",
                "suite": "suite-1",
                "sender_static_key_agreement_id": "did:wba:example.test:user:sender#key-3",
                "recipient_bundle_id": "bundle-1",
                "recipient_signed_prekey_id": "spk-1",
                "sender_ephemeral_pub_b64u": "ephemeral-public",
                "ciphertext_b64u": WIRE_CIPHERTEXT_SECRET,
            }
        });
        let init = closed_projection_or_panic(
            &json!({
                "messages": [{"id": "unrelated"}, init_message.clone()],
                "has_more": false,
            }),
            &init_params,
        );
        assert_eq!(init, successful_direct_wire_projection());

        let cipher_params = direct_wire_params("message-cipher", DirectWireShape::Cipher);
        let cipher_message = json!({
            "id": "message-cipher",
            "sender_did": SENDER_DID,
            "type": "json",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {
                    "dh_pub_b64u": "ratchet-public",
                    "pn": 0,
                    "n": 1,
                },
                "ciphertext_b64u": WIRE_CIPHERTEXT_SECRET,
            }
        });
        let cipher = closed_projection_or_panic(
            &json!({"messages": [cipher_message], "has_more": false}),
            &cipher_params,
        );
        assert_eq!(cipher, successful_direct_wire_projection());

        let duplicate = closed_projection_or_panic(
            &json!({
                "messages": [init_message.clone(), init_message.clone()],
                "has_more": false,
            }),
            &init_params,
        );
        assert_eq!(
            duplicate,
            json!({
                "canonical_match_count": 2,
                "content_type_matches": false,
                "wire_kind_matches": false,
                "ciphertext_present": false,
                "shape_matches": false,
                "plaintext_absent": false,
            })
        );

        let mut plaintext_message = init_message;
        plaintext_message["content"]["debug"] =
            json!({"nested": [format!("prefix {WIRE_PLAINTEXT_SECRET} suffix")]});
        let plaintext = closed_projection_or_panic(
            &json!({"messages": [plaintext_message], "has_more": false}),
            &init_params,
        );
        assert_eq!(plaintext["canonical_match_count"], 1);
        assert_eq!(plaintext["content_type_matches"], true);
        assert_eq!(plaintext["wire_kind_matches"], true);
        assert_eq!(plaintext["ciphertext_present"], true);
        assert_eq!(plaintext["shape_matches"], true);
        assert_eq!(plaintext["plaintext_absent"], false);

        let encoded = serde_json::to_string(&(init, cipher, duplicate, plaintext))
            .expect("serialize closed wire projections");
        assert!(!encoded.contains(WIRE_CIPHERTEXT_SECRET));
        assert!(!encoded.contains(WIRE_PLAINTEXT_SECRET));
    }

    #[tokio::test]
    async fn direct_wire_projection_uses_bounded_inbox_pages_and_closed_output() {
        let observed_skips = Arc::new(Mutex::new(Vec::new()));
        let (base_url, server) = spawn_direct_wire_pagination_server(observed_skips.clone()).await;
        let mut probe = test_probe(&base_url);
        let request = json!({
            "id": "wire-page",
            "action": "direct_wire_projection",
            "params": {
                "peer_did": SENDER_DID,
                "message_id": "message-page-2",
                "expected_shape": "init",
                "forbidden_plaintext": WIRE_PLAINTEXT_SECRET,
            }
        })
        .to_string();

        let output = execute_line(&mut probe, &request).await;
        server.await.expect("direct-wire pagination server task");

        assert_eq!(output["result"], successful_direct_wire_projection());
        assert_eq!(
            observed_skips
                .lock()
                .expect("direct-wire skip observations")
                .as_slice(),
            &[0, DIRECT_WIRE_INBOX_PAGE_LIMIT]
        );
        let encoded = serde_json::to_string(&output).expect("serialize closed pagination output");
        for forbidden in [
            TOKEN_SECRET,
            WIRE_CIPHERTEXT_SECRET,
            WIRE_PLAINTEXT_SECRET,
            "message-page-2",
            SENDER_DID,
        ] {
            assert!(!encoded.contains(forbidden));
        }
    }

    fn direct_wire_params(
        message_id: &str,
        expected_shape: DirectWireShape,
    ) -> DirectWireProjectionParams {
        DirectWireProjectionParams {
            peer_did: SENDER_DID.to_owned(),
            message_id: message_id.to_owned(),
            expected_shape,
            forbidden_plaintext: Zeroizing::new(WIRE_PLAINTEXT_SECRET.to_owned()),
        }
    }

    fn successful_direct_wire_projection() -> Value {
        json!({
            "canonical_match_count": 1,
            "content_type_matches": true,
            "wire_kind_matches": true,
            "ciphertext_present": true,
            "shape_matches": true,
            "plaintext_absent": true,
        })
    }

    fn closed_projection_or_panic(result: &Value, params: &DirectWireProjectionParams) -> Value {
        match closed_direct_wire_projection(result, params) {
            Ok(projection) => projection,
            Err(_) => panic!("closed direct-wire projection"),
        }
    }

    #[test]
    fn account_state_test_fail_once_code_is_exact_scoped_and_secret_free() {
        let exact = json!({
            "code": -32603,
            "message": SERVER_ERROR_SECRET,
            "data": {
                "code": ACCOUNT_STATE_TEST_FAIL_ONCE,
                "retryable": true,
            }
        });
        assert_eq!(
            RpcRejectionPolicy::Standard.allowlisted_code(reqwest::StatusCode::OK, &exact),
            None
        );
        assert_eq!(
            RpcRejectionPolicy::AccountStateAgentInventory
                .allowlisted_code(reqwest::StatusCode::OK, &exact),
            Some(ACCOUNT_STATE_TEST_FAIL_ONCE)
        );
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert_eq!(
                RpcRejectionPolicy::AccountStateAgentInventory.allowlisted_code(status, &exact),
                None
            );
        }
        for invalid in [
            json!({
                "code": -32603,
                "message": SERVER_ERROR_SECRET,
                "data": {
                    "code": ACCOUNT_STATE_TEST_FAIL_ONCE,
                    "retryable": false,
                }
            }),
            json!({
                "code": -32000,
                "message": SERVER_ERROR_SECRET,
                "data": {
                    "code": ACCOUNT_STATE_TEST_FAIL_ONCE,
                    "retryable": true,
                }
            }),
            json!({
                "code": -32603,
                "message": SERVER_ERROR_SECRET,
                "data": {
                    "code": ACCOUNT_STATE_TEST_FAIL_ONCE,
                    "retryable": true,
                    "receipt_id": "must-not-be-accepted",
                }
            }),
            json!({
                "code": -32603,
                "message": SERVER_ERROR_SECRET,
                "data": {
                    "awiki_code": ACCOUNT_STATE_TEST_FAIL_ONCE,
                    "retryable": true,
                }
            }),
        ] {
            assert_eq!(
                RpcRejectionPolicy::AccountStateAgentInventory
                    .allowlisted_code(reqwest::StatusCode::OK, &invalid),
                None
            );
        }

        let response =
            failure_response(RequestId(json!(8)), ProbeFailure::AccountStateTestFailOnce);
        assert_eq!(
            response,
            json!({
                "id": 8,
                "ok": false,
                "error": {"code": PROBE_ACCOUNT_STATE_TEST_FAIL_ONCE}
            })
        );
        let serialized = response.to_string();
        assert!(!serialized.contains(SERVER_ERROR_SECRET));
        assert!(!serialized.contains("receipt_id"));
        assert_eq!(
            required_account_state_agent_outcome(RpcOutcome::Rejected(Some(
                ACCOUNT_STATE_TEST_FAIL_ONCE
            )))
            .expect_err("exact fail-once rejection")
            .code(),
            PROBE_ACCOUNT_STATE_TEST_FAIL_ONCE
        );
        assert_eq!(
            required_account_state_agent_outcome(RpcOutcome::Rejected(None))
                .expect_err("broad rejection")
                .code(),
            TRANSPORT_FAILED
        );
        assert_eq!(
            required_account_state_agent_outcome(RpcOutcome::Rejected(Some(SESSION_UNAUTHORIZED)))
                .expect_err("ordinary allowlisted rejection")
                .code(),
            TRANSPORT_FAILED
        );
    }

    #[test]
    fn ticket_object_is_bound_to_target_service_did() {
        let object_uri =
            reqwest::Url::parse("https://home-a.example.test/objects/object-1?download=1").unwrap();
        assert!(service_did_matches_url(
            "did:wba:home-a.example.test:service:message",
            &object_uri,
        ));
        assert!(!service_did_matches_url(
            "did:wba:home-b.example.test:service:message",
            &object_uri,
        ));
    }

    #[tokio::test]
    async fn readiness_result_is_closed_and_secret_free() {
        let mut probe = test_probe("http://127.0.0.1:9");
        let request = r#"{"id":2,"action":"device_readiness","params":{}}"#;
        let response = execute_line(&mut probe, request).await;
        assert_eq!(
            response,
            json!({
                "id": 2,
                "ok": true,
                "result": {
                    "protocol_device_id_matches_current": true,
                    "role": "admin",
                    "readiness": "admin_ready",
                    "local_root_state": "active",
                }
            })
        );
    }

    #[tokio::test]
    async fn agent_bootstrap_action_returns_only_closed_authoritative_projection() {
        let observed_authorization = Arc::new(Mutex::new(Vec::new()));
        let (base_url, server) =
            spawn_agent_bootstrap_fake_server(observed_authorization.clone()).await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Unix time")
            .as_secs();
        let signing_key_id = format!("{LOCAL_DID}#device-local-sign");
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": LOCAL_DID,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": LOCAL_DID,
            "user_id": "account-local",
            "device_id": "dev-local-1",
            "key_id": signing_key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 3600,
            "jti": "bootstrap-jti-secret",
        });
        let mut probe = test_probe(&base_url);
        probe.bearer = Zeroizing::new(format!(
            "e30.{}.bootstrap-signature-secret",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"))
        ));
        let output = execute_line(
            &mut probe,
            r#"{"id":21,"action":"agent_bootstrap_identity","params":{"controller_account_id":"controller-account-secret"}}"#,
        )
        .await;
        server.await.expect("Agent bootstrap fake server task");

        assert_eq!(
            output["result"],
            json!({
                "agent_account_independent": true,
                "controller_binding_matches": null,
                "manifest_single_device": true,
                "registry_single_ready_admin": true,
                "key_roles_separated": true,
                "bootstrap_generations_one": true,
                "device_access_standard": true,
                "sync_capability_absent": true,
            })
        );
        let serialized = serde_json::to_string(&output).expect("serialize bootstrap output");
        for forbidden in [
            "bootstrap-signature-secret",
            "bootstrap-jti-secret",
            "controller-account-secret",
            "account-local",
            LOCAL_DID,
            "dev-local-1",
            "device-local-sign",
            "document-hash",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        assert_eq!(
            observed_authorization
                .lock()
                .expect("authorization observations")
                .as_slice(),
            &[true, true]
        );
    }

    #[tokio::test]
    async fn fake_server_exercises_closed_protocol_without_secret_output() {
        let observed_authorization = Arc::new(Mutex::new(Vec::new()));
        let (base_url, server) = spawn_fake_server(observed_authorization.clone()).await;
        let object_uri = format!("{base_url}/objects/attachment-1");
        let mut probe = test_probe(&base_url);
        let mut outputs = Vec::new();

        let forbidden = attachment_action(
            0,
            "probe_download_ticket",
            "https://evil.test/objects/attachment-1",
        );
        outputs.push(execute_line(&mut probe, &forbidden).await);
        assert_eq!(
            outputs[0],
            json!({"id": 0, "ok": false, "error": {"code": INVALID_REQUEST}})
        );

        let hold = attachment_action(1, "hold_download_ticket", &object_uri);
        outputs.push(execute_line(&mut probe, &hold).await);
        assert_eq!(outputs[1]["result"], json!({"held": true}));

        let ticket_probe = attachment_action(2, "probe_download_ticket", &object_uri);
        outputs.push(execute_line(&mut probe, &ticket_probe).await);
        assert_eq!(
            outputs[2]["result"],
            json!({"allowed": false, "anp_code": DEVICE_NOT_ELIGIBLE})
        );

        let prekey = json!({
            "id": 3,
            "action": "probe_prekey",
            "params": {
                "target_did": TARGET_DID,
                "target_device_id": "dev-target-1"
            }
        })
        .to_string();
        outputs.push(execute_line(&mut probe, &prekey).await);
        assert_eq!(
            outputs[3]["result"],
            json!({"available": false, "anp_code": DEVICE_NOT_ELIGIBLE})
        );

        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(b"probe-object"));
        let redeem = json!({
            "id": 4,
            "action": "redeem_held_ticket",
            "params": {"expected_digest_b64u": expected}
        })
        .to_string();
        outputs.push(execute_line(&mut probe, &redeem).await);
        assert_eq!(
            outputs[4]["result"],
            json!({
                "digest_matches": true,
                "replay_rejected": true,
                "anp_code": DOWNLOAD_TICKET_INVALID
            })
        );

        server.await.expect("fake server task");
        let serialized = serde_json::to_string(&outputs).expect("serialize responses");
        assert!(!serialized.contains(TOKEN_SECRET));
        assert!(!serialized.contains(TICKET_SECRET));
        assert!(!serialized.contains(SERVER_ERROR_SECRET));
        assert!(!serialized.contains(&object_uri));
        assert_eq!(
            observed_authorization
                .lock()
                .expect("authorization observations")
                .as_slice(),
            &[true, true, true, true, true]
        );
    }

    #[tokio::test]
    async fn account_state_mutations_use_public_rpc_with_device_bearer() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let (base_url, server) = spawn_account_mutation_fake_server(observed.clone()).await;
        let mut probe = test_probe(&base_url);
        let agent_did = "did:wba:example.test:agent:runtime";
        let actions = [
            json!({
                "id": 30,
                "action": "account_state_agent_rename",
                "params": {"agent_did": agent_did, "display_name": "Renamed"}
            }),
            json!({
                "id": 31,
                "action": "account_state_agent_config",
                "params": {
                    "agent_did": agent_did,
                    "active_mode": "blacklist",
                    "whitelist_handles": [],
                    "blacklist_handles": []
                }
            }),
            json!({
                "id": 32,
                "action": "account_state_agent_unbind",
                "params": {"agent_did": agent_did}
            }),
            json!({
                "id": 33,
                "action": "account_state_agent_remove",
                "params": {"agent_did": agent_did}
            }),
            json!({
                "id": 34,
                "action": "account_state_profile_update",
                "params": {"nick_name": "Profile Name"}
            }),
        ];
        let mut outputs = Vec::new();
        for action in actions {
            outputs.push(execute_line(&mut probe, &action.to_string()).await);
        }
        server.await.expect("account mutation fake server task");

        assert_eq!(
            outputs
                .iter()
                .map(|output| output["result"].clone())
                .collect::<Vec<_>>(),
            vec![
                json!({"inventory_version": "45", "matched_expected": true}),
                json!({"inventory_version": "46", "matched_expected": true}),
                json!({"inventory_version": "47", "matched_expected": true}),
                json!({"inventory_version": "48", "matched_expected": true}),
                json!({"profile_version": "22", "matched_expected": true}),
            ]
        );
        assert_eq!(
            observed.lock().expect("mutation observations").as_slice(),
            &[
                (
                    AGENT_INVENTORY_RPC_PATH.to_owned(),
                    "update_display_name".to_owned(),
                    true,
                ),
                (
                    AGENT_INVENTORY_RPC_PATH.to_owned(),
                    "update_invocation_policy".to_owned(),
                    true,
                ),
                (
                    AGENT_INVENTORY_RPC_PATH.to_owned(),
                    "unbind_agent".to_owned(),
                    true,
                ),
                (
                    AGENT_INVENTORY_RPC_PATH.to_owned(),
                    "remove_agent_from_account".to_owned(),
                    true,
                ),
                (ME_RPC_PATH.to_owned(), "update_me".to_owned(), true),
            ]
        );
        let serialized = serde_json::to_string(&outputs).expect("serialize mutation outputs");
        assert!(!serialized.contains(TOKEN_SECRET));
        assert!(!serialized.contains(agent_did));
        assert!(!serialized.contains("Profile Name"));
        assert!(!serialized.contains(SERVER_ERROR_SECRET));
    }

    fn test_probe(base_url: &str) -> Probe {
        let rpc_url = reqwest::Url::parse(&format!("{base_url}{RPC_PATH}")).expect("fake RPC URL");
        let http = match build_http_client(None) {
            Ok(client) => client,
            Err(_) => panic!("build fake HTTP client"),
        };
        Probe {
            _core: None,
            _client: None,
            http,
            bearer: Zeroizing::new(TOKEN_SECRET.to_owned()),
            message_rpc_url: rpc_url,
            account_state_rpc_url: reqwest::Url::parse(&format!(
                "{base_url}{ACCOUNT_STATE_RPC_PATH}"
            ))
            .expect("fake account-state RPC URL"),
            agent_inventory_rpc_url: reqwest::Url::parse(&format!(
                "{base_url}{AGENT_INVENTORY_RPC_PATH}"
            ))
            .expect("fake Agent Inventory RPC URL"),
            agent_registration_rpc_url: reqwest::Url::parse(&format!(
                "{base_url}{AGENT_REGISTRATION_RPC_PATH}"
            ))
            .expect("fake Agent Registration RPC URL"),
            did_auth_rpc_url: reqwest::Url::parse(&format!("{base_url}{DID_AUTH_RPC_PATH}"))
                .expect("fake DID-auth RPC URL"),
            me_rpc_url: reqwest::Url::parse(&format!("{base_url}{ME_RPC_PATH}"))
                .expect("fake Me RPC URL"),
            websocket_url: base_url.replacen("http://", "ws://", 1),
            ca_bundle: None,
            local_did: LOCAL_DID.to_owned(),
            local_account_id: "account-local".to_owned(),
            local_device_id: "dev-local-1".to_owned(),
            local_signing_key_id: format!("{LOCAL_DID}#device-local-sign"),
            local_e2ee_key_id: format!("{LOCAL_DID}#device-local-e2ee"),
            local_binding_generation: "1".to_owned(),
            local_device_auth_generation: "1".to_owned(),
            local_manifest_single_device: true,
            local_document_hash: Some("document-hash".to_owned()),
            local_key_roles_separated: true,
            local_daemon_subkey_present: false,
            source_controller_account_id: None,
            device_role: "admin",
            device_readiness: "admin_ready",
            local_root_state: "active",
            service_did: SERVICE_DID.to_owned(),
            ws: None,
            held_ticket: None,
            daemon_state_root: None,
            daemon_continuity_baseline: None,
        }
    }

    #[test]
    fn account_state_actions_and_results_are_closed_and_secret_free() {
        let manifest =
            match parse_request(r#"{"id":10,"action":"account_state_manifest","params":{}}"#) {
                Ok(request) => request,
                Err(_) => panic!("closed manifest action"),
            };
        assert!(matches!(manifest.action, Action::AccountStateManifest));

        let agent = match parse_request(
            r#"{"id":11,"action":"account_state_agent","params":{"agent_did":"did:wba:example.test:agent:runtime","expected_active_state":"revoked","expected_display_name":"Runtime","expected_active_mode":"blacklist","expected_whitelist_handles":[],"expected_blacklist_handles":["blocked.example.test"]}}"#,
        ) {
            Ok(request) => request,
            Err(_) => panic!("closed agent action"),
        };
        assert!(matches!(agent.action, Action::AccountStateAgent(_)));

        let extra = match parse_request(
            r#"{"id":12,"action":"account_state_manifest","params":{"account_id":"secret"}}"#,
        ) {
            Err(error) => error,
            Ok(_) => panic!("manifest selectors must be rejected"),
        };
        assert_eq!(extra.code(), INVALID_REQUEST);

        let manifest_result = match closed_manifest_result(&json!({
            "account_id": "account-secret",
            "current_did": LOCAL_DID,
            "identity_generation": "8",
            "versions": {
                "profile": "21",
                "agent_inventory": "44",
                "agent_status": "813",
                "device_registry": "12"
            },
            "server_time": "2026-07-28T12:00:00Z"
        })) {
            Ok(result) => result,
            Err(_) => panic!("closed manifest result"),
        };
        assert_eq!(
            manifest_result,
            json!({
                "identity_generation": "8",
                "profile_version": "21",
                "agent_inventory_version": "44",
                "agent_status_version": "813",
                "device_registry_version": "12"
            })
        );

        let agent_result = match closed_agent_result(
            &json!({
                "account_id": "account-secret",
                "inventory_version": "44",
                "agents": [
                    {
                        "agent_did": "did:wba:example.test:agent:runtime",
                        "active_state": "revoked",
                        "display_name": "Runtime",
                        "invocation_policy": {
                            "schema": "awiki.agent_invocation_policy.v1",
                            "active_mode": "blacklist",
                            "whitelist_handles": [],
                            "blacklist_handles": ["blocked.example.test"]
                        },
                        "profile_summary": {"secret": SERVER_ERROR_SECRET}
                    }
                ]
            }),
            &AgentSnapshotParams {
                agent_did: "did:wba:example.test:agent:runtime".to_owned(),
                expected_active_state: "revoked".to_owned(),
                expected_display_name: "Runtime".to_owned(),
                expected_active_mode: "blacklist".to_owned(),
                expected_whitelist_handles: Vec::new(),
                expected_blacklist_handles: vec!["blocked.example.test".to_owned()],
            },
        ) {
            Ok(result) => result,
            Err(_) => panic!("closed agent result"),
        };
        assert_eq!(
            agent_result,
            json!({
                "inventory_version": "44",
                "total_count": 1,
                "match_count": 1,
                "active_count": 0,
                "inactive_count": 0,
                "revoked_count": 1,
                "archived_count": 0,
                "matched_expected": true
            })
        );

        let serialized = serde_json::to_string(&(manifest_result, agent_result))
            .expect("serialize closed output");
        assert!(!serialized.contains("account-secret"));
        assert!(!serialized.contains(LOCAL_DID));
        assert!(!serialized.contains(SERVER_ERROR_SECRET));
    }

    #[test]
    fn account_state_mutation_actions_are_exact_and_results_are_closed() {
        let requests = [
            (
                r#"{"id":20,"action":"account_state_agent_rename","params":{"agent_did":"did:wba:example.test:agent:runtime","display_name":"Renamed"}}"#,
                "rename",
            ),
            (
                r#"{"id":21,"action":"account_state_agent_config","params":{"agent_did":"did:wba:example.test:agent:runtime","active_mode":"blacklist","whitelist_handles":[],"blacklist_handles":["blocked.example.test"]}}"#,
                "config",
            ),
            (
                r#"{"id":22,"action":"account_state_agent_unbind","params":{"agent_did":"did:wba:example.test:agent:runtime"}}"#,
                "unbind",
            ),
            (
                r#"{"id":23,"action":"account_state_agent_remove","params":{"agent_did":"did:wba:example.test:agent:runtime"}}"#,
                "remove",
            ),
            (
                r#"{"id":24,"action":"account_state_profile_update","params":{"nick_name":"Profile Name"}}"#,
                "profile",
            ),
        ];
        for (raw, expected) in requests {
            let request = parse_request(raw).unwrap_or_else(|_| panic!("valid {expected} request"));
            let matches_expected = matches!(
                (request.action, expected),
                (Action::AccountStateAgentRename(_), "rename")
                    | (Action::AccountStateAgentConfig(_), "config")
                    | (Action::AccountStateAgentUnbind(_), "unbind")
                    | (Action::AccountStateAgentRemove(_), "remove")
                    | (Action::AccountStateProfileUpdate(_), "profile")
            );
            assert!(matches_expected, "unexpected parsed {expected} action");
        }

        for invalid in [
            r#"{"id":25,"action":"account_state_agent_rename","params":{"agent_did":"did:wba:example.test:agent:runtime","display_name":"Renamed","account_id":"secret"}}"#,
            r#"{"id":26,"action":"account_state_agent_config","params":{"agent_did":"did:wba:example.test:agent:runtime","active_mode":"open","whitelist_handles":[],"blacklist_handles":[]}}"#,
            r#"{"id":27,"action":"account_state_profile_update","params":{"nick_name":""}}"#,
        ] {
            let error = match parse_request(invalid) {
                Err(error) => error,
                Ok(_) => panic!("invalid mutation action must be rejected"),
            };
            assert_eq!(error.code(), INVALID_REQUEST);
        }

        let rename = closed_agent_rename_result(
            &json!({
                "agent_did": "did:wba:example.test:agent:runtime",
                "display_name": "Renamed",
                "inventory_version": "45",
                "status": {"secret": SERVER_ERROR_SECRET}
            }),
            &AgentRenameParams {
                agent_did: "did:wba:example.test:agent:runtime".to_owned(),
                display_name: "Renamed".to_owned(),
            },
        )
        .unwrap_or_else(|_| panic!("closed rename result"));
        let config = closed_agent_config_result(
            &json!({
                "schema": "awiki.agent_invocation_policy.v1",
                "active_mode": "blacklist",
                "whitelist_handles": [],
                "blacklist_handles": ["blocked.example.test"],
                "inventory_version": "46",
                "secret": SERVER_ERROR_SECRET
            }),
            &AgentConfigParams {
                agent_did: "did:wba:example.test:agent:runtime".to_owned(),
                active_mode: "blacklist".to_owned(),
                whitelist_handles: Vec::new(),
                blacklist_handles: vec!["blocked.example.test".to_owned()],
            },
        )
        .unwrap_or_else(|_| panic!("closed config result"));
        let unbind = closed_agent_unbind_result(&json!({
            "ok": true,
            "inventory_version": "47",
            "secret": SERVER_ERROR_SECRET
        }))
        .unwrap_or_else(|_| panic!("closed unbind result"));
        let remove = closed_agent_remove_result(
            &json!({
                "removed": [{
                    "agent_did": "did:wba:example.test:agent:runtime",
                    "active_state": "archived",
                    "secret": SERVER_ERROR_SECRET
                }],
                "inventory_version": "48"
            }),
            &AgentTargetParams {
                agent_did: "did:wba:example.test:agent:runtime".to_owned(),
            },
        )
        .unwrap_or_else(|_| panic!("closed remove result"));
        let profile = closed_profile_update_result(
            &json!({
                "nick_name": "Profile Name",
                "profile_version": "22",
                "email": SERVER_ERROR_SECRET
            }),
            &ProfileUpdateParams {
                nick_name: "Profile Name".to_owned(),
            },
        )
        .unwrap_or_else(|_| panic!("closed profile mutation result"));
        assert_eq!(
            rename,
            json!({"inventory_version": "45", "matched_expected": true})
        );
        assert_eq!(
            config,
            json!({"inventory_version": "46", "matched_expected": true})
        );
        assert_eq!(
            unbind,
            json!({"inventory_version": "47", "matched_expected": true})
        );
        assert_eq!(
            remove,
            json!({"inventory_version": "48", "matched_expected": true})
        );
        assert_eq!(
            profile,
            json!({"profile_version": "22", "matched_expected": true})
        );

        let serialized = serde_json::to_string(&(rename, config, unbind, remove, profile))
            .expect("serialize mutation outputs");
        assert!(!serialized.contains("did:wba:"));
        assert!(!serialized.contains("Profile Name"));
        assert!(!serialized.contains(SERVER_ERROR_SECRET));
        assert!(
            closed_agent_unbind_result(&json!({"ok": true, "inventory_version": "047"})).is_err()
        );
    }

    fn attachment_action(id: u64, action: &str, object_uri: &str) -> String {
        json!({
            "id": id,
            "action": action,
            "params": {
                "sender_did": SENDER_DID,
                "message_id": "message-1",
                "attachment_id": "attachment-1",
                "object_uri": object_uri
            }
        })
        .to_string()
    }

    async fn execute_line(probe: &mut Probe, raw: &str) -> Value {
        let request = match parse_request(raw) {
            Ok(request) => request,
            Err(_) => panic!("valid probe request"),
        };
        let id = request.id.clone();
        match probe.execute(request.action).await {
            Ok((result, _)) => success_response(id, result),
            Err(error) => failure_response(id, error),
        }
    }

    async fn spawn_fake_server(
        observed_authorization: Arc<Mutex<Vec<bool>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake server");
        let address = listener.local_addr().expect("fake server address");
        let base_url = format!("http://{address}");
        let response_base_url = base_url.clone();
        let server = tokio::spawn(async move {
            let mut ticket_calls = 0_u8;
            let mut object_calls = 0_u8;
            for _ in 0..5 {
                let (mut socket, _) = listener.accept().await.expect("accept request");
                let request = read_http_request(&mut socket).await;
                let header = request.to_ascii_lowercase();
                let has_expected_auth = if header.starts_with("get /objects/") {
                    header.contains(&format!(
                        "authorization: bearer {}",
                        TICKET_SECRET.to_ascii_lowercase()
                    ))
                } else {
                    header.contains(&format!(
                        "authorization: bearer {}",
                        TOKEN_SECRET.to_ascii_lowercase()
                    ))
                };
                observed_authorization
                    .lock()
                    .expect("authorization observations")
                    .push(has_expected_auth);

                let response = if header.starts_with("get /objects/") {
                    object_calls += 1;
                    if object_calls == 1 {
                        http_response(200, b"probe-object")
                    } else {
                        json_response(
                            400,
                            json!({
                                "error": {
                                    "code": -32000,
                                    "data": {"anp_code": DOWNLOAD_TICKET_INVALID}
                                }
                            }),
                        )
                    }
                } else if request.contains("attachment.get_download_ticket") {
                    assert!(request.contains("\"profile\":\"anp.attachment.v2\""));
                    assert!(request.contains("\"anp_version\":\"2.0\""));
                    assert!(request.contains("\"security_profile\":\"transport-protected\""));
                    assert!(request.contains("\"message_security_profile\":\"direct-e2ee\""));
                    assert!(request
                        .contains("\"message_target_did\":\"did:wba:example.test:user:local\""));
                    ticket_calls += 1;
                    if ticket_calls == 1 {
                        json_response(
                            200,
                            json!({
                                "jsonrpc": "2.0",
                                "id": "system-test-probe",
                                "result": {
                                    "download_ticket_b64u": TICKET_SECRET,
                                    "ticket_binding": {
                                        "attachment_id": "attachment-1",
                                        "object_uri": format!("{response_base_url}/objects/attachment-1"),
                                        "requester_did": LOCAL_DID,
                                        "message_id": "message-1",
                                        "message_security_profile": DIRECT_E2EE,
                                        "message_target_did": LOCAL_DID
                                    }
                                }
                            }),
                        )
                    } else {
                        rejected_response(DEVICE_NOT_ELIGIBLE)
                    }
                } else {
                    rejected_response(DEVICE_NOT_ELIGIBLE)
                };
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write fake response");
            }
        });
        (base_url, server)
    }

    async fn spawn_agent_bootstrap_fake_server(
        observed_authorization: Arc<Mutex<Vec<bool>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Agent bootstrap fake server");
        let address = listener
            .local_addr()
            .expect("Agent bootstrap fake server address");
        let base_url = format!("http://{address}");
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener
                    .accept()
                    .await
                    .expect("accept Agent bootstrap request");
                let request = read_http_request(&mut socket).await;
                observed_authorization
                    .lock()
                    .expect("authorization observations")
                    .push(
                        request
                            .to_ascii_lowercase()
                            .contains("authorization: bearer e30."),
                    );
                let (_, body) = request
                    .split_once("\r\n\r\n")
                    .expect("Agent bootstrap request body");
                let payload: Value =
                    serde_json::from_str(body).expect("Agent bootstrap request JSON");
                let result = match payload.get("method").and_then(Value::as_str) {
                    Some("device_registry_get") => json!({
                        "did": LOCAL_DID,
                        "checkpoint": {
                            "document_version": 1,
                            "document_hash": "document-hash",
                            "registry_version": 1,
                        },
                        "devices": [{
                            "device_id": "dev-local-1",
                            "signing_key_id": format!("{LOCAL_DID}#device-local-sign"),
                            "e2ee_key_id": format!("{LOCAL_DID}#device-local-e2ee"),
                            "status": "active",
                            "role": "admin",
                            "management_ready": true,
                            "auth_generation": 1,
                        }],
                    }),
                    Some("account_state.manifest_get") => json!({
                        "account_id": "account-local",
                        "current_did": LOCAL_DID,
                        "identity_generation": "1",
                        "versions": {"device_registry": "1"},
                    }),
                    _ => panic!("unexpected Agent bootstrap RPC method"),
                };
                let response = json_response(
                    200,
                    json!({
                        "jsonrpc": "2.0",
                        "id": "system-test-probe",
                        "result": result,
                    }),
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write Agent bootstrap response");
            }
        });
        (base_url, server)
    }

    async fn spawn_direct_wire_pagination_server(
        observed_skips: Arc<Mutex<Vec<i64>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind direct-wire pagination server");
        let address = listener
            .local_addr()
            .expect("direct-wire pagination address");
        let base_url = format!("http://{address}");
        let server = tokio::spawn(async move {
            for page_index in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept inbox request");
                let request = read_http_request(&mut socket).await;
                let (_, body) = request.split_once("\r\n\r\n").expect("inbox request body");
                let payload: Value = serde_json::from_str(body).expect("inbox request JSON");
                assert_eq!(payload["method"], "inbox.get");
                assert_eq!(
                    payload["params"]["body"]["limit"],
                    DIRECT_WIRE_INBOX_PAGE_LIMIT
                );
                let skip = payload["params"]["body"]["skip"]
                    .as_i64()
                    .expect("inbox skip cursor");
                observed_skips
                    .lock()
                    .expect("direct-wire skip observations")
                    .push(skip);

                let result = if page_index == 0 {
                    json!({
                        "messages": (0..DIRECT_WIRE_INBOX_PAGE_LIMIT)
                            .map(|index| json!({"id": format!("unrelated-{index}")}))
                            .collect::<Vec<_>>(),
                        "total": DIRECT_WIRE_INBOX_PAGE_LIMIT + 1,
                        "has_more": true,
                    })
                } else {
                    json!({
                        "messages": [{
                            "id": "message-page-2",
                            "sender_did": SENDER_DID,
                            "type": "json",
                            "content_type": DIRECT_INIT_CONTENT_TYPE,
                            "content": {
                                "session_id": "session-page-2",
                                "suite": "suite-page-2",
                                "sender_static_key_agreement_id":
                                    "did:wba:example.test:user:sender#key-3",
                                "recipient_bundle_id": "bundle-page-2",
                                "recipient_signed_prekey_id": "spk-page-2",
                                "sender_ephemeral_pub_b64u": "ephemeral-page-2",
                                "ciphertext_b64u": WIRE_CIPHERTEXT_SECRET,
                            }
                        }],
                        "total": DIRECT_WIRE_INBOX_PAGE_LIMIT + 1,
                        "has_more": false,
                    })
                };
                let response = json_response(
                    200,
                    json!({
                        "jsonrpc": "2.0",
                        "id": "system-test-probe",
                        "result": result,
                    }),
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write inbox response");
            }
        });
        (base_url, server)
    }

    async fn spawn_account_mutation_fake_server(
        observed: Arc<Mutex<Vec<(String, String, bool)>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind account mutation fake server");
        let address = listener.local_addr().expect("account mutation address");
        let base_url = format!("http://{address}");
        let server = tokio::spawn(async move {
            for index in 0..5 {
                let (mut socket, _) = listener.accept().await.expect("accept mutation request");
                let request = read_http_request(&mut socket).await;
                let first_line = request.lines().next().expect("mutation request line");
                let path = first_line
                    .split_whitespace()
                    .nth(1)
                    .expect("mutation request path")
                    .to_owned();
                let body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, body)| body)
                    .expect("mutation request body");
                let payload: Value = serde_json::from_str(body).expect("mutation JSON-RPC body");
                let method = payload
                    .get("method")
                    .and_then(Value::as_str)
                    .expect("mutation method")
                    .to_owned();
                let has_expected_auth = request.to_ascii_lowercase().contains(&format!(
                    "authorization: bearer {}",
                    TOKEN_SECRET.to_ascii_lowercase()
                ));
                observed.lock().expect("mutation observations").push((
                    path,
                    method.clone(),
                    has_expected_auth,
                ));

                let result = match index {
                    0 => {
                        assert_eq!(method, "update_display_name");
                        assert_eq!(
                            payload["params"],
                            json!({
                                "agent_did": "did:wba:example.test:agent:runtime",
                                "display_name": "Renamed"
                            })
                        );
                        json!({
                            "agent_did": "did:wba:example.test:agent:runtime",
                            "display_name": "Renamed",
                            "inventory_version": "45",
                            "secret": SERVER_ERROR_SECRET
                        })
                    }
                    1 => {
                        assert_eq!(method, "update_invocation_policy");
                        assert_eq!(
                            payload["params"],
                            json!({
                                "agent_did": "did:wba:example.test:agent:runtime",
                                "active_mode": "blacklist",
                                "whitelist_handles": [],
                                "blacklist_handles": []
                            })
                        );
                        json!({
                            "schema": "awiki.agent_invocation_policy.v1",
                            "active_mode": "blacklist",
                            "whitelist_handles": [],
                            "blacklist_handles": [],
                            "inventory_version": "46",
                            "secret": SERVER_ERROR_SECRET
                        })
                    }
                    2 => {
                        assert_eq!(method, "unbind_agent");
                        json!({
                            "ok": true,
                            "inventory_version": "47",
                            "secret": SERVER_ERROR_SECRET
                        })
                    }
                    3 => {
                        assert_eq!(method, "remove_agent_from_account");
                        json!({
                            "removed": [{
                                "agent_did": "did:wba:example.test:agent:runtime",
                                "active_state": "archived",
                                "secret": SERVER_ERROR_SECRET
                            }],
                            "inventory_version": "48"
                        })
                    }
                    _ => {
                        assert_eq!(method, "update_me");
                        assert_eq!(payload["params"], json!({"nick_name": "Profile Name"}));
                        json!({
                            "nick_name": "Profile Name",
                            "profile_version": "22",
                            "secret": SERVER_ERROR_SECRET
                        })
                    }
                };
                let response = json_response(
                    200,
                    json!({
                        "jsonrpc": "2.0",
                        "id": "system-test-probe",
                        "result": result
                    }),
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write mutation response");
            }
        });
        (base_url, server)
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut raw = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = socket.read(&mut buffer).await.expect("read request");
            assert!(count > 0, "request ended before headers");
            raw.extend_from_slice(&buffer[..count]);
            if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = String::from_utf8_lossy(&raw[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while raw.len() < header_end + content_length {
            let count = socket.read(&mut buffer).await.expect("read request body");
            assert!(count > 0, "request ended before body");
            raw.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(raw).expect("UTF-8 request")
    }

    fn rejected_response(code: &str) -> String {
        json_response(
            403,
            json!({
                "jsonrpc": "2.0",
                "id": "system-test-probe",
                "error": {
                    "code": -32000,
                    "message": SERVER_ERROR_SECRET,
                    "data": {"anp_code": code}
                }
            }),
        )
    }

    fn json_response(status: u16, value: Value) -> String {
        http_response(status, value.to_string().as_bytes())
    }

    fn http_response(status: u16, body: &[u8]) -> String {
        let reason = if status == 200 { "OK" } else { "Bad Request" };
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        )
    }
}
