#[cfg(feature = "blocking")]
use std::sync::mpsc;

pub const MESSAGE_WS_ENDPOINT: &str = "/im/ws";
pub const DIAL_ERROR_BODY_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeClientEndpoints {
    pub request_url: String,
    pub did_auth_url: String,
    pub websocket_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeClientConstructionPlan {
    pub endpoints: RealtimeClientEndpoints,
    pub remembered_scope_inputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeDialOutcome {
    Connected,
    Failed {
        status_code: Option<u16>,
        error: String,
        response_body: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeRefreshOutcome {
    Refreshed { current_jwt: String },
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeConnectAction {
    DialBearer {
        token: String,
        authorization: String,
    },
    RefreshBearer,
    Attach,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeConnectSimulation {
    pub actions: Vec<RealtimeConnectAction>,
    pub error: Option<String>,
}

#[cfg(feature = "blocking")]
pub trait RealtimeTransport {
    fn dial_bearer(&mut self, websocket_url: &str, bearer_token: &str) -> RealtimeDialOutcome;
}

#[cfg(feature = "blocking")]
pub trait RealtimeAuthProvider {
    fn refresh_realtime_bearer(
        &mut self,
        did_auth_url: &str,
    ) -> crate::ImResult<RealtimeRefreshOutcome>;
}

#[cfg(feature = "blocking")]
pub(crate) struct FileRealtimeAuthProvider<'a> {
    client: &'a crate::core::ImClient,
}

#[cfg(feature = "blocking")]
impl<'a> FileRealtimeAuthProvider<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }
}

#[cfg(feature = "blocking")]
impl RealtimeAuthProvider for FileRealtimeAuthProvider<'_> {
    fn refresh_realtime_bearer(
        &mut self,
        _did_auth_url: &str,
    ) -> crate::ImResult<RealtimeRefreshOutcome> {
        let update = self.client.auth().refresh_session()?;
        let token = self.client.runtime().key_provider.valid_auth_token()?;
        Ok(match token {
            Some(current_jwt) => RealtimeRefreshOutcome::Refreshed { current_jwt },
            None if update.refreshed => RealtimeRefreshOutcome::Failed {
                error: "did-auth did not provide a websocket bearer token".to_string(),
            },
            None => RealtimeRefreshOutcome::Failed {
                error: "did-auth did not return a websocket bearer token".to_string(),
            },
        })
    }
}

pub fn realtime_client_endpoints(
    service_base_url: &str,
) -> crate::ImResult<RealtimeClientEndpoints> {
    let request_url = join_base_url(service_base_url, MESSAGE_WS_ENDPOINT);
    if request_url.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("service_base_url".to_string()),
            "service base url is required for websocket mode",
        ));
    }
    Ok(RealtimeClientEndpoints {
        websocket_url: derive_websocket_url(service_base_url, MESSAGE_WS_ENDPOINT),
        did_auth_url: join_base_url(
            service_base_url,
            crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT,
        ),
        request_url,
    })
}

pub fn realtime_client_construction_plan(
    service_base_url: &str,
) -> crate::ImResult<RealtimeClientConstructionPlan> {
    let endpoints = realtime_client_endpoints(service_base_url)?;
    Ok(RealtimeClientConstructionPlan {
        remembered_scope_inputs: vec![
            service_base_url.to_string(),
            endpoints.did_auth_url.clone(),
            endpoints.request_url.clone(),
        ],
        endpoints,
    })
}

#[cfg(feature = "blocking")]
pub fn connect_realtime_with_transport<T, A>(
    endpoints: &RealtimeClientEndpoints,
    current_jwt: &str,
    transport: &mut T,
    auth: &mut A,
) -> crate::ImResult<crate::realtime::RealtimeHandle>
where
    T: RealtimeTransport,
    A: RealtimeAuthProvider,
{
    let events = vec![crate::realtime::ImEvent::ConnectionStateChanged(
        crate::realtime::ConnectionStateChanged {
            state: crate::realtime::RealtimeConnectionState::Connecting,
            reason: None,
        },
    )];
    let initial_token = current_jwt.trim().to_string();
    if !initial_token.is_empty() {
        match transport.dial_bearer(&endpoints.websocket_url, &initial_token) {
            RealtimeDialOutcome::Connected => return connected_handle(events),
            RealtimeDialOutcome::Failed {
                status_code: Some(401),
                ..
            } => {}
            RealtimeDialOutcome::Failed {
                error,
                response_body,
                ..
            } => {
                return connect_error(
                    events,
                    format_dial_failure(&error, response_body.as_deref()),
                );
            }
        }
    }

    let refreshed_token = match auth.refresh_realtime_bearer(&endpoints.did_auth_url)? {
        RealtimeRefreshOutcome::Refreshed { current_jwt } => current_jwt.trim().to_string(),
        RealtimeRefreshOutcome::Failed { error } => {
            let error = if initial_token.is_empty() {
                error
            } else {
                format!("refresh websocket session JWT: {error}")
            };
            return connect_error(events, error);
        }
    };
    if refreshed_token.is_empty() {
        return connect_error(
            events,
            "did-auth did not return a websocket bearer token".to_string(),
        );
    }

    match transport.dial_bearer(&endpoints.websocket_url, &refreshed_token) {
        RealtimeDialOutcome::Connected => connected_handle(events),
        RealtimeDialOutcome::Failed {
            error,
            response_body,
            ..
        } => connect_error(
            events,
            format_dial_failure(&error, response_body.as_deref()),
        ),
    }
}

#[cfg(feature = "blocking")]
fn connected_handle(
    mut events: Vec<crate::realtime::ImEvent>,
) -> crate::ImResult<crate::realtime::RealtimeHandle> {
    events.push(crate::realtime::ImEvent::ConnectionStateChanged(
        crate::realtime::ConnectionStateChanged {
            state: crate::realtime::RealtimeConnectionState::Connected,
            reason: None,
        },
    ));
    Ok(handle_with_initial_events(events))
}

#[cfg(feature = "blocking")]
fn connect_error(
    mut events: Vec<crate::realtime::ImEvent>,
    error: String,
) -> crate::ImResult<crate::realtime::RealtimeHandle> {
    events.push(crate::realtime::ImEvent::ConnectionStateChanged(
        crate::realtime::ConnectionStateChanged {
            state: crate::realtime::RealtimeConnectionState::Disconnected,
            reason: Some(error.clone()),
        },
    ));
    Err(crate::ImError::TransportUnavailable { detail: error })
}

pub(crate) async fn connect_async_websocket_session(
    client: &crate::core::ImClient,
) -> crate::ImResult<super::async_ws_transport::AsyncWsTransport> {
    let service_base_url = client.core_inner().sdk_config().service_base_url.as_str();
    let endpoints = realtime_client_endpoints(service_base_url)?;
    let require_sync_changed_v2 = client.realtime_requires_sync_changed_v2()?;
    let p6_client_instance_id =
        prepare_p6_delivery_session_async(client, require_sync_changed_v2).await?;
    let current_jwt = client
        .runtime()
        .key_provider
        .valid_auth_token()?
        .unwrap_or_default();
    connect_async_websocket_session_with_token(
        client,
        &endpoints,
        current_jwt.trim(),
        require_sync_changed_v2,
        p6_client_instance_id.as_deref(),
    )
    .await
}

async fn connect_async_websocket_session_with_token(
    client: &crate::core::ImClient,
    endpoints: &RealtimeClientEndpoints,
    current_jwt: &str,
    require_sync_changed_v2: bool,
    p6_client_instance_id: Option<&str>,
) -> crate::ImResult<super::async_ws_transport::AsyncWsTransport> {
    let current_jwt = current_jwt.trim();
    let config = client.core_inner().sdk_config();
    let ca_bundle = config.ca_bundle_path();
    let client_version = config
        .client_version_info
        .as_ref()
        .map(crate::ClientVersionInfo::header_value);
    if !current_jwt.is_empty() {
        match super::async_ws_transport::AsyncWsTransport::connect(
            &endpoints.websocket_url,
            current_jwt,
            ca_bundle,
            require_sync_changed_v2,
            client_version.as_deref(),
            p6_client_instance_id,
        )
        .await
        {
            Ok(transport) => return Ok(transport),
            Err(err) if err.status_code == Some(401) => {}
            Err(err) => {
                return Err(crate::ImError::TransportUnavailable {
                    detail: err.message,
                });
            }
        }
    }

    let update = client.auth().refresh_session_async().await?;
    let refreshed_token = client
        .runtime()
        .key_provider
        .valid_auth_token()?
        .ok_or_else(|| {
            if update.refreshed {
                crate::ImError::TransportUnavailable {
                    detail: "did-auth did not provide a websocket bearer token".to_owned(),
                }
            } else {
                crate::ImError::TransportUnavailable {
                    detail: "did-auth did not return a websocket bearer token".to_owned(),
                }
            }
        })?;
    if refreshed_token.trim().is_empty() {
        return Err(crate::ImError::TransportUnavailable {
            detail: "did-auth did not return a websocket bearer token".to_owned(),
        });
    }
    super::async_ws_transport::AsyncWsTransport::connect(
        &endpoints.websocket_url,
        refreshed_token.trim(),
        ca_bundle,
        require_sync_changed_v2,
        client_version.as_deref(),
        p6_client_instance_id,
    )
    .await
    .map_err(|err| crate::ImError::TransportUnavailable {
        detail: err.message,
    })
}

#[cfg(feature = "blocking")]
pub(crate) fn connect_native_websocket_session(
    client: &crate::core::ImClient,
) -> crate::ImResult<super::ws_transport::WsTransport> {
    let service_base_url = client.core_inner().sdk_config().service_base_url.as_str();
    let endpoints = realtime_client_endpoints(service_base_url)?;
    let require_sync_changed_v2 = client.realtime_requires_sync_changed_v2()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| crate::ImError::Internal {
            message: format!("build realtime capability runtime: {error}"),
        })?;
    let p6_client_instance_id = runtime.block_on(prepare_p6_delivery_session_async(
        client,
        require_sync_changed_v2,
    ))?;
    let current_jwt = client
        .runtime()
        .key_provider
        .valid_auth_token()?
        .unwrap_or_default();
    connect_native_websocket_session_with_token(
        client,
        &endpoints,
        current_jwt.trim(),
        require_sync_changed_v2,
        p6_client_instance_id.as_deref(),
    )
}

#[cfg(feature = "blocking")]
fn connect_native_websocket_session_with_token(
    client: &crate::core::ImClient,
    endpoints: &RealtimeClientEndpoints,
    current_jwt: &str,
    require_sync_changed_v2: bool,
    p6_client_instance_id: Option<&str>,
) -> crate::ImResult<super::ws_transport::WsTransport> {
    let current_jwt = current_jwt.trim();
    let config = client.core_inner().sdk_config();
    let ca_bundle = config.ca_bundle_path();
    let client_version = config
        .client_version_info
        .as_ref()
        .map(crate::ClientVersionInfo::header_value);
    if !current_jwt.is_empty() {
        match super::ws_transport::WsTransport::connect_with_ca_bundle(
            &endpoints.websocket_url,
            current_jwt,
            ca_bundle,
            require_sync_changed_v2,
            client_version.as_deref(),
            p6_client_instance_id,
        ) {
            Ok(transport) => return Ok(transport),
            Err(err) if err.status_code == Some(401) => {}
            Err(err) => {
                return Err(crate::ImError::TransportUnavailable {
                    detail: err.message,
                });
            }
        }
    }

    let mut auth = FileRealtimeAuthProvider::new(client);
    let refreshed_token = match auth.refresh_realtime_bearer(&endpoints.did_auth_url)? {
        RealtimeRefreshOutcome::Refreshed { current_jwt } => current_jwt.trim().to_string(),
        RealtimeRefreshOutcome::Failed { error } => {
            let error = if current_jwt.is_empty() {
                error
            } else {
                format!("refresh websocket session JWT: {error}")
            };
            return Err(crate::ImError::TransportUnavailable { detail: error });
        }
    };
    if refreshed_token.is_empty() {
        return Err(crate::ImError::TransportUnavailable {
            detail: "did-auth did not return a websocket bearer token".to_string(),
        });
    }

    super::ws_transport::WsTransport::connect_with_ca_bundle(
        &endpoints.websocket_url,
        &refreshed_token,
        ca_bundle,
        require_sync_changed_v2,
        client_version.as_deref(),
        p6_client_instance_id,
    )
    .map_err(|err| crate::ImError::TransportUnavailable {
        detail: err.message,
    })
}

async fn prepare_p6_delivery_session_async(
    client: &crate::core::ImClient,
    required: bool,
) -> crate::ImResult<Option<String>> {
    if !required {
        return Ok(None);
    }
    let mut retry = 0_u32;
    loop {
        match prepare_p6_delivery_session_once_async(client).await {
            Err(error) if retry < 2 && is_transient_sqlite_lock(&error) => {
                retry += 1;
                tokio::time::sleep(std::time::Duration::from_millis(u64::from(retry) * 100)).await;
            }
            outcome => return outcome,
        }
    }
}

async fn prepare_p6_delivery_session_once_async(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<String>> {
    let binding = client.active_sync_account_binding().await?;
    let owner_identity_id = client.current_identity().id.as_str();
    if binding.owner_identity_id != owner_identity_id
        || binding.current_did != client.did().as_str()
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "realtime P6 capability binding does not match the active identity".to_owned(),
        });
    }
    let db = client.core_inner().local_state_db().await?;
    if db
        .lane_capability_negotiation_required(
            owner_identity_id.to_owned(),
            binding.device_auth_generation.clone(),
        )
        .await?
    {
        let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
        crate::internal::message_runtime::sync_v2::refresh_lane_bootstrap_with_transport_async(
            client,
            &mut transport,
            &db,
            &binding,
        )
        .await?;
    }
    db.load_or_create_sync_client_instance_id(owner_identity_id.to_owned())
        .await
        .map(Some)
}

fn is_transient_sqlite_lock(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::LocalStateUnavailable { detail }
            if detail == "database is locked" || detail == "database table is locked"
    )
}

pub fn bearer_authorization_header(token: &str) -> String {
    format!("Bearer {}", token.trim())
}

pub fn validate_refresh_bearer_preconditions(
    has_auth_session: bool,
    did_auth_url: &str,
) -> Result<(), String> {
    if !has_auth_session {
        return Err("auth session is required for websocket mode".to_string());
    }
    if did_auth_url.trim().is_empty() {
        return Err("did-auth rpc url is required for websocket mode".to_string());
    }
    Ok(())
}

pub fn simulate_realtime_connect(
    current_jwt: &str,
    mut dial_bearer: impl FnMut(&str) -> RealtimeDialOutcome,
    mut refresh_bearer: impl FnMut() -> RealtimeRefreshOutcome,
) -> RealtimeConnectSimulation {
    let mut actions = Vec::new();
    let initial_token = current_jwt.trim().to_string();
    if !initial_token.is_empty() {
        actions.push(dial_bearer_action(&initial_token));
        match dial_bearer(&initial_token) {
            RealtimeDialOutcome::Connected => {
                actions.push(RealtimeConnectAction::Attach);
                return RealtimeConnectSimulation {
                    actions,
                    error: None,
                };
            }
            RealtimeDialOutcome::Failed {
                status_code: Some(401),
                ..
            } => {}
            RealtimeDialOutcome::Failed {
                error,
                response_body,
                ..
            } => {
                return RealtimeConnectSimulation {
                    actions,
                    error: Some(format_dial_failure(&error, response_body.as_deref())),
                };
            }
        }
    }

    actions.push(RealtimeConnectAction::RefreshBearer);
    let refreshed_token = match refresh_bearer() {
        RealtimeRefreshOutcome::Refreshed { current_jwt } => current_jwt.trim().to_string(),
        RealtimeRefreshOutcome::Failed { error } => {
            return RealtimeConnectSimulation {
                actions,
                error: Some(if initial_token.is_empty() {
                    error
                } else {
                    format!("refresh websocket session JWT: {error}")
                }),
            };
        }
    };
    if refreshed_token.is_empty() {
        return RealtimeConnectSimulation {
            actions,
            error: Some("did-auth did not return a websocket bearer token".to_string()),
        };
    }

    actions.push(dial_bearer_action(&refreshed_token));
    match dial_bearer(&refreshed_token) {
        RealtimeDialOutcome::Connected => {
            actions.push(RealtimeConnectAction::Attach);
            RealtimeConnectSimulation {
                actions,
                error: None,
            }
        }
        RealtimeDialOutcome::Failed {
            error,
            response_body,
            ..
        } => RealtimeConnectSimulation {
            actions,
            error: Some(format_dial_failure(&error, response_body.as_deref())),
        },
    }
}

pub fn format_dial_error_message(
    error: Option<&str>,
    response_body: Option<&[u8]>,
) -> Option<String> {
    let error = error?;
    let Some(body) = response_body else {
        return Some(error.to_string());
    };
    if body.is_empty() {
        return Some(error.to_string());
    }
    let capped = &body[..body.len().min(DIAL_ERROR_BODY_LIMIT)];
    let body_text = String::from_utf8_lossy(capped).trim().to_string();
    Some(format!("{error}: {body_text}"))
}

pub fn derive_websocket_url(base_url: &str, path: &str) -> String {
    let http_url = join_base_url(base_url, path);
    let trimmed = http_url.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return format!("wss://{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return format!("ws://{rest}");
    }
    trimmed.to_string()
}

pub fn join_base_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return path.trim().to_string();
    }
    let mut path = path.trim().to_string();
    if path.is_empty() {
        return base.to_string();
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{base}{path}")
}

fn dial_bearer_action(token: &str) -> RealtimeConnectAction {
    RealtimeConnectAction::DialBearer {
        token: token.trim().to_string(),
        authorization: bearer_authorization_header(token),
    }
}

fn format_dial_failure(error: &str, response_body: Option<&[u8]>) -> String {
    format_dial_error_message(Some(error), response_body).unwrap_or_else(|| error.to_string())
}

#[cfg(feature = "blocking")]
fn handle_with_initial_events(
    events: Vec<crate::realtime::ImEvent>,
) -> crate::realtime::RealtimeHandle {
    let (sender, receiver) = mpsc::channel();
    for event in events {
        if sender.send(event).is_err() {
            break;
        }
    }
    drop(sender);
    crate::realtime::RealtimeHandle::new(receiver, crate::realtime::RealtimeControl::default())
}

#[cfg(feature = "blocking")]
fn read_auth_token(path: &std::path::Path) -> crate::ImResult<Option<String>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(crate::ImError::from(err)),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    Ok(value
        .get("jwt_token")
        .or_else(|| value.get("token"))
        .or_else(|| value.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned))
}

async fn read_auth_token_async(path: std::path::PathBuf) -> crate::ImResult<Option<String>> {
    let raw = match tokio::fs::read(&path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(crate::ImError::from(err)),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    Ok(value
        .get("jwt_token")
        .or_else(|| value.get("token"))
        .or_else(|| value.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned))
}

#[cfg(test)]
mod tests {
    use super::is_transient_sqlite_lock;

    #[test]
    fn realtime_p6_preflight_retries_only_sqlite_locks() {
        assert!(is_transient_sqlite_lock(
            &crate::ImError::LocalStateUnavailable {
                detail: "database is locked".to_owned(),
            }
        ));
        assert!(is_transient_sqlite_lock(
            &crate::ImError::LocalStateUnavailable {
                detail: "database table is locked".to_owned(),
            }
        ));
        assert!(!is_transient_sqlite_lock(
            &crate::ImError::LocalStateUnavailable {
                detail: "disk I/O error".to_owned(),
            }
        ));
        assert!(!is_transient_sqlite_lock(&crate::ImError::PermissionDenied));
    }
}
