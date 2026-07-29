use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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
    ) -> Result<Self, AsyncWsConnectError> {
        let request = build_async_ws_request(websocket_url, bearer_token)?;
        let connector = async_ws_rustls_connector(ca_bundle).await?;
        let (stream, response) = if let Some(connector) = connector {
            connect_async_tls_with_config(request, None, false, Some(connector)).await
        } else {
            tokio_tungstenite::connect_async(request).await
        }
        .map_err(async_ws_connect_error)?;
        let sync_changed_v2 = validate_async_ws_subprotocol(response.headers())?;
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
    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL
            .parse()
            .expect("static websocket subprotocol is a valid header value"),
    );
    Ok(request)
}

fn validate_async_ws_subprotocol(headers: &HeaderMap) -> Result<bool, AsyncWsConnectError> {
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
        assert_eq!(validate_async_ws_subprotocol(&headers).unwrap(), false);

        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            "awiki.sync.changed.v1".parse().unwrap(),
        );
        assert!(validate_async_ws_subprotocol(&headers).is_err());

        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            crate::internal::realtime::SYNC_CHANGED_V2_SUBPROTOCOL
                .parse()
                .unwrap(),
        );
        assert_eq!(validate_async_ws_subprotocol(&headers).unwrap(), true);
    }

    #[test]
    fn async_ws_requests_sync_changed_v2_subprotocol() {
        let request = build_async_ws_request("wss://example.test/im/ws", "token").unwrap();
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
}
