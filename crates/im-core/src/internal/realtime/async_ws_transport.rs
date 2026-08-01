use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::{ProtocolError, SubProtocolError};
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL};
use tokio_tungstenite::tungstenite::http::{HeaderMap, Request};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AsyncWsConnectError {
    pub(crate) status_code: Option<u16>,
    pub(crate) message: String,
}

impl std::fmt::Display for AsyncWsConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AsyncWsConnectError {}

pub(crate) struct AsyncWsTransport {
    stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    ping_counter: i32,
    sync_changed_v2: bool,
}

impl AsyncWsTransport {
    pub(crate) async fn connect(
        websocket_url: &str,
        bearer_token: &str,
        ca_bundle: Option<&str>,
        require_sync_changed_v2: bool,
    ) -> Result<Self, AsyncWsConnectError> {
        let connector = async_ws_rustls_connector(ca_bundle).await?;
        let request = build_async_ws_request(websocket_url, bearer_token, true)?;
        let connected = if let Some(connector) = connector.clone() {
            connect_async_tls_with_config(request, None, false, Some(connector)).await
        } else {
            tokio_tungstenite::connect_async(request).await
        };
        let (stream, response) = match connected {
            Ok(connected) => connected,
            Err(error) if is_missing_async_ws_subprotocol(&error) && !require_sync_changed_v2 => {
                let request = build_async_ws_request(websocket_url, bearer_token, false)?;
                if let Some(connector) = connector {
                    connect_async_tls_with_config(request, None, false, Some(connector)).await
                } else {
                    tokio_tungstenite::connect_async(request).await
                }
                .map_err(async_ws_connect_error)?
            }
            Err(error) => return Err(async_ws_connect_error(error)),
        };
        let sync_changed_v2 =
            validate_async_ws_subprotocol(response.headers(), require_sync_changed_v2)?;
        Ok(Self {
            stream,
            ping_counter: 0,
            sync_changed_v2,
        })
    }

    pub(crate) async fn read_json_message(
        &mut self,
    ) -> crate::ImResult<Option<Map<String, Value>>> {
        loop {
            let Some(message) = self.stream.next().await else {
                return Ok(None);
            };
            match message.map_err(async_ws_error)? {
                Message::Text(raw) => {
                    let message = decode_json_object(raw.as_str())?;
                    if crate::internal::realtime::accepts_negotiated_notification(
                        self.sync_changed_v2,
                        &message,
                    ) {
                        return Ok(Some(message));
                    }
                }
                Message::Binary(raw) => {
                    let message = decode_json_object(&raw)?;
                    if crate::internal::realtime::accepts_negotiated_notification(
                        self.sync_changed_v2,
                        &message,
                    ) {
                        return Ok(Some(message));
                    }
                }
                Message::Ping(payload) => self
                    .stream
                    .send(Message::Pong(payload))
                    .await
                    .map_err(async_ws_error)?,
                Message::Pong(_) => {}
                Message::Close(_) => return Ok(None),
                Message::Frame(_) => {}
            }
        }
    }

    pub(crate) async fn ping(&mut self) -> crate::ImResult<()> {
        self.ping_counter = self.ping_counter.wrapping_add(1);
        self.stream
            .send(Message::Ping(self.ping_counter.to_string().into()))
            .await
            .map_err(async_ws_error)
    }
}

fn build_async_ws_request(
    websocket_url: &str,
    bearer_token: &str,
    request_sync_changed_v2: bool,
) -> Result<Request<()>, AsyncWsConnectError> {
    let mut request = websocket_url
        .into_client_request()
        .map_err(|err| async_ws_connect_message(format!("build websocket request: {err}")))?;
    let authorization =
        crate::internal::realtime::transport::bearer_authorization_header(bearer_token);
    let authorization = authorization.parse().map_err(|err| {
        async_ws_connect_message(format!("build websocket authorization header: {err}"))
    })?;
    request.headers_mut().insert(AUTHORIZATION, authorization);
    if request_sync_changed_v2 {
        request.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
            crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL
                .parse()
                .expect("static websocket subprotocol is a valid header value"),
        );
    }
    Ok(request)
}

fn is_missing_async_ws_subprotocol(error: &TungsteniteError) -> bool {
    matches!(
        error,
        TungsteniteError::Protocol(ProtocolError::SecWebSocketSubProtocolError(
            SubProtocolError::NoSubProtocol
        ))
    )
}

fn validate_async_ws_subprotocol(
    headers: &HeaderMap,
    require_sync_changed_v2: bool,
) -> Result<bool, AsyncWsConnectError> {
    let selected = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok());
    if selected.is_some()
        && selected != Some(crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL)
    {
        return Err(async_ws_connect_message(
            "websocket server selected an unsupported sync subprotocol",
        ));
    }
    if require_sync_changed_v2 && selected.is_none() {
        return Err(async_ws_connect_message(
            "exact-device websocket requires awiki.sync.changed.v2",
        ));
    }
    Ok(selected.is_some())
}

fn decode_json_object(raw: impl AsRef<[u8]>) -> crate::ImResult<Map<String, Value>> {
    let value: Value =
        serde_json::from_slice(raw.as_ref()).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "websocket message payload must be a JSON object".to_owned(),
        })
}

async fn async_ws_rustls_connector(
    ca_bundle: Option<&str>,
) -> Result<Option<Connector>, AsyncWsConnectError> {
    let Some(ca_bundle) = ca_bundle.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let raw = tokio::fs::read(Path::new(ca_bundle))
        .await
        .map_err(|err| async_ws_connect_message(format!("read ca bundle: {err}")))?;
    let certs = CertificateDer::pem_slice_iter(&raw)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| async_ws_connect_message(format!("parse ca bundle: {err}")))?;
    let mut root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let (valid_count, _) = root_store.add_parsable_certificates(certs);
    if valid_count == 0 {
        return Err(async_ws_connect_message(format!(
            "invalid ca bundle: {ca_bundle}"
        )));
    }
    Ok(Some(Connector::Rustls(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    ))))
}

fn async_ws_connect_error(error: TungsteniteError) -> AsyncWsConnectError {
    match error {
        TungsteniteError::Http(response) => {
            let status_code = response.status().as_u16();
            async_ws_connect_status(
                status_code,
                format!("websocket handshake failed with HTTP status {status_code}"),
            )
        }
        error => async_ws_connect_message(error.to_string()),
    }
}

fn async_ws_connect_status(status_code: u16, message: impl Into<String>) -> AsyncWsConnectError {
    AsyncWsConnectError {
        status_code: Some(status_code),
        message: message.into(),
    }
}

fn async_ws_connect_message(message: impl Into<String>) -> AsyncWsConnectError {
    AsyncWsConnectError {
        status_code: None,
        message: message.into(),
    }
}

fn async_ws_error(error: TungsteniteError) -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn async_ws_http_error_preserves_status_code_for_auth_retry() {
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .body(Some(Vec::new()))
            .unwrap();
        let error = async_ws_connect_error(TungsteniteError::Http(Box::new(response)));

        assert_eq!(error.status_code, Some(401));
        assert!(error.message.contains("401"));
    }

    #[test]
    fn async_ws_allows_v1_fallback_and_rejects_wrong_selected_subprotocol() {
        let mut headers = HeaderMap::new();
        assert!(!validate_async_ws_subprotocol(&headers, false).unwrap());

        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            "awiki.sync.changed.v1".parse().unwrap(),
        );
        assert!(validate_async_ws_subprotocol(&headers, false).is_err());

        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL
                .parse()
                .unwrap(),
        );
        assert!(validate_async_ws_subprotocol(&headers, false).unwrap());
    }

    #[test]
    fn async_ws_requests_sync_changed_v2_subprotocol() {
        let request = build_async_ws_request("wss://example.test/im/ws", "token", true).unwrap();
        assert_eq!(
            request
                .headers()
                .get(SEC_WEBSOCKET_PROTOCOL)
                .unwrap()
                .to_str()
                .unwrap(),
            crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL
        );
    }

    #[tokio::test]
    async fn async_ws_reconnects_without_subprotocol_for_legacy_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                tokio_tungstenite::accept_async(stream).await.unwrap();
            }
        });

        let transport = AsyncWsTransport::connect(
            &format!("ws://{address}/im/ws"),
            "legacy-token",
            None,
            false,
        )
        .await
        .unwrap();

        assert!(!transport.sync_changed_v2);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn async_ws_exact_device_does_not_retry_without_v2_subprotocol() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap();
            if let Ok(Ok((stream, _))) =
                tokio::time::timeout(std::time::Duration::from_millis(250), listener.accept()).await
            {
                tokio_tungstenite::accept_async(stream).await.unwrap();
                true
            } else {
                false
            }
        });

        let result =
            AsyncWsTransport::connect(&format!("ws://{address}/im/ws"), "device-token", None, true)
                .await;

        assert!(result.is_err());
        assert!(!server.await.unwrap());
    }
}
