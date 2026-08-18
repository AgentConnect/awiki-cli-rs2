use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::{ProtocolError, SubProtocolError};
use tokio_tungstenite::tungstenite::http::header::{
    HeaderName, AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL,
};
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
    sync_subprotocol: super::SyncNotificationSubprotocol,
}

impl AsyncWsTransport {
    pub(crate) async fn connect(
        websocket_url: &str,
        bearer_token: &str,
        ca_bundle: Option<&str>,
        require_sync_changed_v2: bool,
        client_version: Option<&str>,
        p6_client_instance_id: Option<&str>,
    ) -> Result<Self, AsyncWsConnectError> {
        let connector = async_ws_rustls_connector(ca_bundle).await?;
        let request = build_async_ws_request(
            websocket_url,
            bearer_token,
            require_sync_changed_v2,
            client_version,
            p6_client_instance_id,
        )?;
        let connected = if let Some(connector) = connector.clone() {
            connect_async_tls_with_config(request, None, false, Some(connector)).await
        } else {
            tokio_tungstenite::connect_async(request).await
        };
        let (stream, response) = match connected {
            Ok(connected) => connected,
            Err(error) if is_missing_async_ws_subprotocol(&error) && !require_sync_changed_v2 => {
                let request = build_async_ws_request(
                    websocket_url,
                    bearer_token,
                    false,
                    client_version,
                    None,
                )?;
                if let Some(connector) = connector {
                    connect_async_tls_with_config(request, None, false, Some(connector)).await
                } else {
                    tokio_tungstenite::connect_async(request).await
                }
                .map_err(async_ws_connect_error)?
            }
            Err(error) => return Err(async_ws_connect_error(error)),
        };
        let sync_subprotocol =
            validate_async_ws_subprotocol(response.headers(), require_sync_changed_v2)?;
        Ok(Self {
            stream,
            ping_counter: 0,
            sync_subprotocol,
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
                        self.sync_subprotocol,
                        &message,
                    ) {
                        return Ok(Some(message));
                    }
                }
                Message::Binary(raw) => {
                    let message = decode_json_object(&raw)?;
                    if crate::internal::realtime::accepts_negotiated_notification(
                        self.sync_subprotocol,
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
    client_version: Option<&str>,
    p6_client_instance_id: Option<&str>,
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
    if let Some(client_version) = client_version {
        let value = client_version.parse().map_err(|err| {
            async_ws_connect_message(format!("build websocket client version header: {err}"))
        })?;
        request
            .headers_mut()
            .insert(HeaderName::from_static("x-awiki-client-version"), value);
    }
    if request_sync_changed_v2 {
        let client_instance_id = p6_client_instance_id
            .filter(|value| !value.is_empty() && value.trim() == *value && value.len() <= 255)
            .ok_or_else(|| {
                async_ws_connect_message(
                    "strict P6 websocket requires a canonical client instance ID",
                )
            })?;
        request.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
            crate::internal::realtime::P6_DELIVERY_CONTEXT_V1_SUBPROTOCOL
                .parse()
                .expect("static websocket subprotocol list is a valid header value"),
        );
        request.headers_mut().insert(
            HeaderName::from_static("x-awiki-p6-client-instance-id"),
            client_instance_id.parse().map_err(|err| {
                async_ws_connect_message(format!("build P6 client instance header: {err}"))
            })?,
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
) -> Result<super::SyncNotificationSubprotocol, AsyncWsConnectError> {
    let selected = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok());
    let selected = match selected {
        None => super::SyncNotificationSubprotocol::Legacy,
        Some(crate::internal::realtime::P6_DELIVERY_CONTEXT_V1_SUBPROTOCOL) => {
            super::SyncNotificationSubprotocol::V3
        }
        Some(_) => {
            return Err(async_ws_connect_message(
                "websocket server selected an unsupported sync subprotocol",
            ));
        }
    };
    if require_sync_changed_v2 && selected == super::SyncNotificationSubprotocol::Legacy {
        return Err(async_ws_connect_message(
            "exact-device websocket requires a versioned AWiki sync subprotocol",
        ));
    }
    Ok(selected)
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
    fn async_ws_requires_the_strict_p6_subprotocol_for_exact_devices() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            validate_async_ws_subprotocol(&headers, false).unwrap(),
            crate::internal::realtime::SyncNotificationSubprotocol::Legacy
        );

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
        assert!(validate_async_ws_subprotocol(&headers, false).is_err());
        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            crate::internal::realtime::P6_DELIVERY_CONTEXT_V1_SUBPROTOCOL
                .parse()
                .unwrap(),
        );
        assert_eq!(
            validate_async_ws_subprotocol(&headers, true).unwrap(),
            crate::internal::realtime::SyncNotificationSubprotocol::V3
        );
    }

    #[test]
    fn async_ws_requests_only_the_strict_p6_subprotocol() {
        let request = build_async_ws_request(
            "wss://example.test/im/ws",
            "token",
            true,
            Some("awiki-me/0714/1.0.31+214"),
            Some("client-installation-1"),
        )
        .unwrap();
        assert_eq!(
            request
                .headers()
                .get(SEC_WEBSOCKET_PROTOCOL)
                .unwrap()
                .to_str()
                .unwrap(),
            crate::internal::realtime::P6_DELIVERY_CONTEXT_V1_SUBPROTOCOL
        );
        assert_eq!(
            request
                .headers()
                .get("x-awiki-p6-client-instance-id")
                .unwrap(),
            "client-installation-1"
        );
        assert_eq!(
            request
                .headers()
                .get("x-awiki-client-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "awiki-me/0714/1.0.31+214"
        );
    }

    #[tokio::test]
    async fn async_ws_generic_session_connects_once_without_subprotocol() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..1 {
                let (stream, _) = listener.accept().await.unwrap();
                tokio_tungstenite::accept_async(stream).await.unwrap();
            }
        });

        let transport = AsyncWsTransport::connect(
            &format!("ws://{address}/im/ws"),
            "legacy-token",
            None,
            false,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            transport.sync_subprotocol,
            crate::internal::realtime::SyncNotificationSubprotocol::Legacy
        );
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

        let result = AsyncWsTransport::connect(
            &format!("ws://{address}/im/ws"),
            "device-token",
            None,
            true,
            None,
            Some("client-installation-1"),
        )
        .await;

        assert!(result.is_err());
        assert!(!server.await.unwrap());
    }
}
