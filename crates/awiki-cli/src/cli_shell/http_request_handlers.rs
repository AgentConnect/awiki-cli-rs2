//! Bounded authenticated HTTP transport; identity and signing stay in the published SDK.
use super::App;
use crate::{cli_output::ExitError, cli_parser::ParsedCommand};
use base64::{engine::general_purpose::STANDARD, Engine};
use im_core::{
    ExternalHttpAuthDecision, ExternalHttpHeader, ExternalHttpRequest, ExternalHttpResponse,
};
use serde::Deserialize;
use std::io::Read;

const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderInput {
    name: String,
    value: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    origin: String,
    method: String,
    path: String,
    #[serde(default)]
    headers: Vec<HeaderInput>,
    #[serde(default)]
    include_client_metadata: bool,
    body_base64: Option<String>,
}
fn failure() -> ExitError {
    ExitError::new("generic_capability_failed", 2, "Invalid input or authenticated transport failure", "Check the selected identity, exact target and saved request. Reconcile uncertain writes before retrying.")
}
fn origin(raw: &str) -> Result<reqwest::Url, ExitError> {
    let url = reqwest::Url::parse(raw).map_err(|_| failure())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failure());
    }
    Ok(url)
}
fn target(input: &RequestInput) -> Result<reqwest::Url, ExitError> {
    let base = origin(&input.origin)?;
    if !matches!(
        input.method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    ) || !input.path.starts_with('/')
        || input.path.starts_with("//")
        || input.path.contains(['#', '\\'])
    {
        return Err(failure());
    }
    let url = base.join(&input.path).map_err(|_| failure())?;
    if url.origin() != base.origin() {
        return Err(failure());
    }
    Ok(url)
}
fn ordinary_headers(inputs: Vec<HeaderInput>) -> Result<Vec<ExternalHttpHeader>, ExitError> {
    inputs
        .into_iter()
        .map(|header| {
            if matches!(
                header.name.to_ascii_lowercase().as_str(),
                "authorization"
                    | "signature-input"
                    | "signature"
                    | "content-digest"
                    | "x-awiki-client-version"
                    | "cookie"
                    | "set-cookie"
                    | "x-forwarded-host"
                    | "forwarded"
                    | "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "proxy-authorization"
            ) {
                return Err(failure());
            }
            ExternalHttpHeader::new(header.name, header.value).map_err(|_| failure())
        })
        .collect()
}
impl App {
    pub async fn run_http_request_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        // No scripted confirmation or dry-run success can be confused with a real signature/send.
        if self.globals.dry_run || !command.args.is_empty() {
            return Err(failure());
        }
        let input_limit = 6 * 1024 * 1024;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take((input_limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| failure())?;
        if bytes.len() > input_limit {
            return Err(failure());
        }
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let input: RequestInput = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let url = target(&input)?;
        let body = input
            .body_base64
            .as_ref()
            .map(|s| STANDARD.decode(s).map_err(|_| failure()))
            .transpose()?;
        if input.method == "GET" && body.is_some() {
            return Err(failure());
        }
        let headers = ordinary_headers(input.headers)?;
        let request =
            ExternalHttpRequest::new(url.as_str(), &input.method, headers.clone(), body.clone())
                .map_err(|_| failure())?;
        // The SDK owns protected auth headers. Host build metadata is not caller
        // input and remains independent of signature/token generation.
        let metadata = if input.include_client_metadata {
            Some(
                crate::build_info::client_version_info()
                    .map_err(|_| failure())?
                    .ok_or_else(failure)?
                    .header_value(),
            )
        } else {
            None
        };
        let client = crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
        let auth = client.external_http_auth();
        let mut attempt = auth.prepare_async(request).await.map_err(|_| failure())?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| failure())?;
        for _ in 0..4 {
            let mut request = http.request(
                reqwest::Method::from_bytes(attempt.method().as_bytes()).map_err(|_| failure())?,
                attempt.target_url(),
            );
            if let Some(body) = &body {
                request = request.body(body.clone());
            }
            for h in &headers {
                request = request.header(h.name(), h.value());
            }
            for h in attempt.header_patch() {
                request = request.header(h.name(), h.value());
            }
            if let Some(value) = &metadata {
                if !attempt.header_patch().iter().any(|h| {
                    h.name()
                        .eq_ignore_ascii_case(im_core::CLIENT_VERSION_HEADER)
                }) {
                    request = request.header(im_core::CLIENT_VERSION_HEADER, value);
                }
            }
            let mut response = request.send().await.map_err(|_| failure())?;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    ExternalHttpHeader::new(name.as_str(), value.to_str().map_err(|_| failure())?)
                        .map_err(|_| failure())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let decision = auth
                .handle_response_async(
                    attempt,
                    ExternalHttpResponse::new(status, headers).map_err(|_| failure())?,
                )
                .await
                .map_err(|_| failure())?;
            match decision {
                ExternalHttpAuthDecision::Retry(next) => {
                    attempt = next;
                }
                ExternalHttpAuthDecision::Complete => {
                    let mut body = Vec::new();
                    while let Some(chunk) = response.chunk().await.map_err(|_| failure())? {
                        if body.len() + chunk.len() > RESPONSE_LIMIT {
                            return Err(failure());
                        }
                        body.extend_from_slice(&chunk);
                    }
                    println!(
                        "{}",
                        serde_json::json!({"status":status,"body_base64":STANDARD.encode(body),"content_type":content_type})
                    );
                    return Ok(());
                }
            }
        }
        Err(failure())
    }
}

#[cfg(test)]
#[path = "http_request_tests.rs"]
mod tests;
