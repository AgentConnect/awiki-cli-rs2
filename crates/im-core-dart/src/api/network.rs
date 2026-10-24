//! Explicit desktop opt-in; no account/tenant material is retained here.
use crate::dto::error::DartImError;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy)]
pub enum DartNetworkPolicy {
    SystemPreferred,
    Direct,
}
#[derive(Clone, Copy)]
pub enum DartNetworkStatus {
    SystemProxy,
    Direct,
    ProxyFallback,
}
#[derive(Clone)]
pub struct DartNetworkHeader {
    pub name: String,
    pub value: String,
}
pub struct DartNetworkHead {
    pub status_code: u16,
    pub headers: Vec<DartNetworkHeader>,
}
#[flutter_rust_bridge::frb(opaque)]
pub struct DartNetworkResponse {
    head: DartNetworkHead,
    response: tokio::sync::Mutex<Option<reqwest::Response>>,
    request: Arc<DartNetworkRequest>,
}
#[flutter_rust_bridge::frb(opaque)]
pub struct DartNetworkRequest {
    cancelled: tokio::sync::watch::Sender<bool>,
}
pub async fn create_network_request() -> Arc<DartNetworkRequest> {
    Arc::new(DartNetworkRequest {
        cancelled: tokio::sync::watch::channel(false).0,
    })
}
pub async fn cancel_network_request(request: &Arc<DartNetworkRequest>) {
    request.cancelled.send_replace(true);
}
static DESKTOP_NETWORK: OnceLock<Arc<im_core::network::NetworkRuntime>> = OnceLock::new();
pub(crate) fn configured_runtime() -> Option<Arc<im_core::network::NetworkRuntime>> {
    DESKTOP_NETWORK.get().cloned()
}
pub async fn configure_desktop_network(
    policy: DartNetworkPolicy,
    ca_bundle: Option<String>,
) -> Result<(), DartImError> {
    let runtime =
        DESKTOP_NETWORK.get_or_init(|| Arc::new(im_core::network::NetworkRuntime::new(ca_bundle)));
    runtime.update_policy(match policy {
        DartNetworkPolicy::SystemPreferred => im_core::network::NetworkPolicy::SystemPreferred,
        DartNetworkPolicy::Direct => im_core::network::NetworkPolicy::Direct,
    });
    Ok(())
}
pub async fn desktop_network_status() -> DartNetworkStatus {
    match DESKTOP_NETWORK
        .get()
        .map(|network| network.diagnostics().status)
        .unwrap_or_default()
    {
        im_core::network::NetworkStatus::SystemProxy => DartNetworkStatus::SystemProxy,
        im_core::network::NetworkStatus::Direct => DartNetworkStatus::Direct,
        im_core::network::NetworkStatus::ProxyFallback => DartNetworkStatus::ProxyFallback,
    }
}
pub async fn send_network_request(
    request: &Arc<DartNetworkRequest>,
    method: String,
    url: String,
    headers: Vec<DartNetworkHeader>,
    body: Vec<u8>,
    follow_redirects: bool,
) -> Result<Arc<DartNetworkResponse>, DartImError> {
    let runtime = DESKTOP_NETWORK
        .get()
        .ok_or_else(|| DartImError::internal("desktop_network_not_configured"))?;
    let mut cancelled = request.cancelled.subscribe();
    let response = tokio::select! {
        result = runtime.send(&method, &url, headers.into_iter().map(|header| (header.name, header.value)).collect(), body, follow_redirects) => result.map_err(DartImError::from)?,
        _ = cancelled.wait_for(|cancelled| *cancelled) => return Err(DartImError::internal("network_request_cancelled")),
    };
    let head = DartNetworkHead {
        status_code: response.status().as_u16(),
        headers: response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some(DartNetworkHeader {
                    name: name.to_string(),
                    value: value.to_str().ok()?.to_owned(),
                })
            })
            .collect(),
    };
    Ok(Arc::new(DartNetworkResponse {
        head,
        response: tokio::sync::Mutex::new(Some(response)),
        request: request.clone(),
    }))
}
pub async fn network_response_head(response: &Arc<DartNetworkResponse>) -> DartNetworkHead {
    DartNetworkHead {
        status_code: response.head.status_code,
        headers: response.head.headers.clone(),
    }
}
pub async fn network_response_chunk(
    response: &Arc<DartNetworkResponse>,
) -> Result<Vec<u8>, DartImError> {
    let response_cancel = response.request.cancelled.clone();
    let mut guard = response.response.lock().await;
    if let Some(response) = guard.as_mut() {
        let mut cancelled = response_cancel.subscribe();
        let chunk = tokio::select! {
            result = response.chunk() => result,
            _ = cancelled.wait_for(|cancelled| *cancelled) => { *guard = None; return Err(DartImError::internal("network_request_cancelled")); }
        };
        match chunk {
            Ok(Some(chunk)) => return Ok(chunk.to_vec()),
            Ok(None) => {}
            Err(_) => {
                *guard = None;
                return Err(DartImError::internal("network_response_failed"));
            }
        }
    }
    *guard = None;
    Ok(Vec::new())
}
pub async fn close_network_response(response: &Arc<DartNetworkResponse>) {
    response.request.cancelled.send_replace(true);
    if let Ok(mut guard) = response.response.try_lock() {
        *guard = None;
    }
}

pub struct DartNetworkDiagnostic {
    pub at: String,
    pub operation: String,
    pub stage: String,
    pub elapsed_ms: u32,
}
pub async fn desktop_network_diagnostics() -> Vec<DartNetworkDiagnostic> {
    configured_runtime()
        .map(|runtime| {
            runtime
                .diagnostics()
                .records
                .into_iter()
                .map(|record| DartNetworkDiagnostic {
                    at: record.at,
                    operation: record.operation.to_string(),
                    stage: record.stage,
                    elapsed_ms: record.elapsed_ms.min(u32::MAX as u64) as u32,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
