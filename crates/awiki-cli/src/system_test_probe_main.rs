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

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use im_core::identity::{IdentityDeviceReadiness, IdentityDeviceRole};
use rand::RngCore;
use reqwest::header::{
    HeaderValue as ReqwestHeaderValue, AUTHORIZATION as REQWEST_AUTHORIZATION, CONTENT_TYPE,
};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
use serde::de::DeserializeOwned;
use serde::Deserialize;
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
const DID_AUTH_RPC_PATH: &str = "/user-service/did-auth/rpc";
const ME_RPC_PATH: &str = "/user-service/me/rpc";

const INVALID_REQUEST: &str = "probe.invalid_request";
const INVALID_STATE: &str = "probe.invalid_state";
const TRANSPORT_FAILED: &str = "probe.transport_failed";
const RUNTIME_FAILED: &str = "probe.runtime_failed";
const ACCOUNT_STATE_TEST_FAIL_ONCE: &str = "account_state_test_fail_once";
const PROBE_ACCOUNT_STATE_TEST_FAIL_ONCE: &str = "probe.account_state_test_fail_once";

const DEVICE_NOT_ELIGIBLE: &str = "anp.device_not_eligible";
const DEVICE_STATE_CHANGED: &str = "anp.device_state_changed";
const SESSION_UNAUTHORIZED: &str = "client.session_unauthorized";
const DOWNLOAD_TICKET_INVALID: &str = "anp.attachment.download_ticket_invalid";

const DIRECT_E2EE: &str = "direct-e2ee";
const ATTACHMENT_V2: &str = "anp.attachment.v2";

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone)]
struct RequestId(Value);

struct ProbeRequest {
    id: RequestId,
    action: Action,
}

enum Action {
    DeviceReadiness,
    OpenWs,
    WaitWsClosed { timeout_ms: u64 },
    CloseWs,
    ReconnectWs,
    HoldDownloadTicket(AttachmentTicketParams),
    ProbeDownloadTicket(AttachmentTicketParams),
    ProbePrekey(PrekeyParams),
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
    AccountStateTestFailOnce,
}

impl ProbeFailure {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => INVALID_REQUEST,
            Self::InvalidState => INVALID_STATE,
            Self::Transport => TRANSPORT_FAILED,
            Self::Runtime => RUNTIME_FAILED,
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
    _client: Option<im_core::ImClient>,
    http: reqwest::Client,
    bearer: Zeroizing<String>,
    message_rpc_url: reqwest::Url,
    account_state_rpc_url: reqwest::Url,
    agent_inventory_rpc_url: reqwest::Url,
    did_auth_rpc_url: reqwest::Url,
    me_rpc_url: reqwest::Url,
    websocket_url: String,
    ca_bundle: Option<String>,
    local_did: String,
    local_device_id: String,
    device_role: &'static str,
    device_readiness: &'static str,
    local_root_state: &'static str,
    service_did: String,
    ws: Option<WsStream>,
    held_ticket: Option<HeldTicket>,
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
    let mut probe = Probe::from_workspace().await?;
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
        let local_device_id = device_summary
            .protocol_device_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
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
            _client: Some(client),
            http,
            bearer,
            message_rpc_url,
            account_state_rpc_url,
            agent_inventory_rpc_url,
            did_auth_rpc_url,
            me_rpc_url,
            websocket_url,
            ca_bundle,
            local_did,
            local_device_id,
            device_role,
            device_readiness,
            local_root_state,
            service_did,
            ws: None,
            held_ticket: None,
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
    const SENDER_DID: &str = "did:wba:example.test:user:sender";
    const TARGET_DID: &str = "did:wba:example.test:user:target";
    const SERVICE_DID: &str = "did:wba:127.0.0.1:service:message";
    const TOKEN_SECRET: &str = "jwt-secret-must-not-leak";
    const TICKET_SECRET: &str = "ticket-secret-must-not-leak";
    const SERVER_ERROR_SECRET: &str = "server-error-must-not-leak";

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
            did_auth_rpc_url: reqwest::Url::parse(&format!("{base_url}{DID_AUTH_RPC_PATH}"))
                .expect("fake DID-auth RPC URL"),
            me_rpc_url: reqwest::Url::parse(&format!("{base_url}{ME_RPC_PATH}"))
                .expect("fake Me RPC URL"),
            websocket_url: base_url.replacen("http://", "ws://", 1),
            ca_bundle: None,
            local_did: LOCAL_DID.to_owned(),
            local_device_id: "dev-local-1".to_owned(),
            device_role: "admin",
            device_readiness: "admin_ready",
            local_root_state: "active",
            service_did: SERVICE_DID.to_owned(),
            ws: None,
            held_ticket: None,
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
