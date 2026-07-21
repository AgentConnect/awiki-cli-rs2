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

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
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

const INVALID_REQUEST: &str = "probe.invalid_request";
const INVALID_STATE: &str = "probe.invalid_state";
const TRANSPORT_FAILED: &str = "probe.transport_failed";
const RUNTIME_FAILED: &str = "probe.runtime_failed";

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
    OpenWs,
    WaitWsClosed { timeout_ms: u64 },
    CloseWs,
    ReconnectWs,
    HoldDownloadTicket(AttachmentTicketParams),
    ProbeDownloadTicket(AttachmentTicketParams),
    ProbePrekey(PrekeyParams),
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

#[derive(Clone, Copy)]
enum ProbeFailure {
    InvalidRequest,
    InvalidState,
    Transport,
    Runtime,
}

impl ProbeFailure {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => INVALID_REQUEST,
            Self::InvalidState => INVALID_STATE,
            Self::Transport => TRANSPORT_FAILED,
            Self::Runtime => RUNTIME_FAILED,
        }
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
    websocket_url: String,
    ca_bundle: Option<String>,
    local_did: String,
    local_device_id: String,
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
        let local_device_id = core
            .identities()
            .device_summary_async(im_core::IdentitySelector::Default)
            .await
            .map_err(|_| ProbeFailure::Runtime)?
            .protocol_device_id
            .map(|value| value.as_str().to_owned())
            .ok_or(ProbeFailure::Runtime)?;
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
            websocket_url,
            ca_bundle,
            local_did,
            local_device_id,
            service_did,
            ws: None,
            held_ticket: None,
        })
    }

    async fn execute(&mut self, action: Action) -> Result<(Value, bool), ProbeFailure> {
        match action {
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
        let request_body =
            validated_ticket_request_body(&rpc_params, params, &self.local_did, &self.service_did)?;
        match self
            .rpc::<DownloadTicketResult>("attachment.get_download_ticket", rpc_params)
            .await?
        {
            RpcOutcome::Success(result) => Ok(RpcOutcome::Success(self.validated_held_ticket(
                result,
                params,
                &request_body,
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
            let object_uri = self.validate_object_uri(&params.object_uri)?;
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
        params: &AttachmentTicketParams,
        request_body: &Map<String, Value>,
    ) -> Result<HeldTicket, ProbeFailure> {
        if result.download_ticket_b64u.trim().is_empty()
            || !ticket_binding_matches_request(&result.ticket_binding, request_body)
        {
            return Err(ProbeFailure::Runtime);
        }
        let object_uri = self.validate_object_uri(&params.object_uri)?;
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
        let payload = json!({
            "jsonrpc": "2.0",
            "id": "system-test-probe",
            "method": method,
            "params": params,
        });
        let payload = serde_json::to_vec(&payload).map_err(|_| ProbeFailure::Runtime)?;
        let mut authorization = reqwest_authorization_header(&self.bearer)?;
        authorization.set_sensitive(true);
        let response = self
            .http
            .post(self.message_rpc_url.clone())
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
                allowlisted_anp_code(&error).or_else(|| auth_status_code(status.as_u16())),
            )),
            _ if status.as_u16() == 401 || status.as_u16() == 403 => {
                Ok(RpcOutcome::Rejected(Some(SESSION_UNAUTHORIZED)))
            }
            _ => Err(ProbeFailure::Runtime),
        }
    }

    fn validate_object_uri(&self, raw: &str) -> Result<reqwest::Url, ProbeFailure> {
        let url = reqwest::Url::parse(raw).map_err(|_| ProbeFailure::InvalidRequest)?;
        validate_service_url(&url).map_err(|_| ProbeFailure::InvalidRequest)?;
        if !same_origin(&url, &self.message_rpc_url)
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

fn validate_service_url(url: &reqwest::Url) -> Result<(), ProbeFailure> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host_str().is_some_and(is_loopback_host) => Ok(()),
        _ => Err(ProbeFailure::Runtime),
    }
}

fn same_origin(left: &reqwest::Url, right: &reqwest::Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
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
    const SERVICE_DID: &str = "did:wba:example.test:service:message";
    const TOKEN_SECRET: &str = "jwt-secret-must-not-leak";
    const TICKET_SECRET: &str = "ticket-secret-must-not-leak";
    const SERVER_ERROR_SECRET: &str = "server-error-must-not-leak";

    #[test]
    fn protocol_rejects_unknown_actions_and_extra_fields() {
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
            websocket_url: base_url.replacen("http://", "ws://", 1),
            ca_bundle: None,
            local_did: LOCAL_DID.to_owned(),
            local_device_id: "dev-local-1".to_owned(),
            service_did: SERVICE_DID.to_owned(),
            ws: None,
            held_ticket: None,
        }
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
