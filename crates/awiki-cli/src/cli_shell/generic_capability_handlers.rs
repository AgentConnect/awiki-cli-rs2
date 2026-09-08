//! Generic Object Proof and authenticated HTTP commands. Never exports keys or auth headers.
use super::App;
use crate::{cli_output::ExitError, cli_parser::ParsedCommand};
use base64::{engine::general_purpose::STANDARD, Engine};
use im_core::{
    ExternalHttpAuthDecision, ExternalHttpHeader, ExternalHttpRequest, ExternalHttpResponse,
};
use serde::Deserialize;
use std::io::{BufRead, Read, Write};

const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInput {
    object_json: String,
    object_hash: String,
}
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
    body_base64: Option<String>,
}
fn failure() -> ExitError {
    ExitError::new("generic_capability_failed", 2, "Invalid input, signature confirmation or authenticated transport failure", "Check the selected identity, exact target and saved request. Reconcile uncertain writes before retrying.")
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
                "host"
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
    pub async fn run_generic_capability_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        // No scripted confirmation or dry-run success can be confused with a real signature/send.
        if self.globals.dry_run || !command.args.is_empty() {
            return Err(failure());
        }
        let input_limit = if command.name == "proof.sign-object" {
            8 * 1024 * 1024
        } else {
            6 * 1024 * 1024
        };
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
        if command.name == "proof.sign-object" {
            let input: SignInput = serde_json::from_slice(&bytes).map_err(|_| failure())?;
            let review = im_core::object_proofs::ObjectProofReview::parse(
                input.object_json.as_bytes(),
                &input.object_hash,
            )
            .map_err(|_| failure())?;
            let client =
                crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
            let capability = client
                .object_proofs()
                .inspect_capability_async()
                .await
                .map_err(|e| {
                    crate::m_core_cli_adapter::error::map_im_error(
                        e,
                        "check object signing capability",
                    )
                })?;
            let display = serde_json::json!({"signer_did":client.did().as_str(),"review":review.presentation(),"capability":capability});
            let confirmed = confirm(&display, review.object_hash())?;
            let proof = client
                .object_proofs()
                .sign_reviewed_async(review, &confirmed)
                .await
                .map_err(|e| {
                    crate::m_core_cli_adapter::error::map_im_error(e, "sign JSON object")
                })?;
            println!(
                "{}",
                serde_json::json!({"proof":proof,"signer_did":client.did().as_str(),"object_hash":input.object_hash})
            );
            return Ok(());
        }
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
        let client = crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
        let auth = client.external_http_auth();
        let mut attempt = auth.prepare_async(request).await.map_err(|_| failure())?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| failure())?;
        loop {
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
    }
}
#[cfg(unix)]
fn confirm(display: &serde_json::Value, hash: &str) -> Result<String, ExitError> {
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| failure())?;
    writeln!(
        tty,
        "{}\n将使用上述身份签署此固定 JSON 对象。确认请键入完整 object_hash（取消请直接回车）：",
        serde_json::to_string_pretty(display).map_err(|_| failure())?
    )
    .map_err(|_| failure())?;
    tty.flush().map_err(|_| failure())?;
    let mut answer = String::new();
    std::io::BufReader::new(&mut tty)
        .take(128)
        .read_line(&mut answer)
        .map_err(|_| failure())?;
    let answer = answer.trim_end_matches(['\r', '\n']);
    if answer != hash {
        return Err(failure());
    }
    Ok(answer.to_owned())
}
#[cfg(windows)]
fn confirm(display: &serde_json::Value, hash: &str) -> Result<String, ExitError> {
    let input = std::fs::OpenOptions::new()
        .read(true)
        .open("CONIN$")
        .map_err(|_| failure())?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .open("CONOUT$")
        .map_err(|_| failure())?;
    writeln!(
        output,
        "{}\n将使用上述身份签署此固定 JSON 对象。确认请键入完整 object_hash（取消请直接回车）：",
        serde_json::to_string_pretty(display).map_err(|_| failure())?
    )
    .map_err(|_| failure())?;
    output.flush().map_err(|_| failure())?;
    let mut answer = String::new();
    std::io::BufReader::new(input)
        .take(128)
        .read_line(&mut answer)
        .map_err(|_| failure())?;
    let answer = answer.trim_end_matches(['\r', '\n']);
    if answer != hash {
        return Err(failure());
    }
    Ok(answer.to_owned())
}
#[cfg(not(any(unix, windows)))]
fn confirm(_: &serde_json::Value, _: &str) -> Result<String, ExitError> {
    Err(failure())
}
#[cfg(test)]
#[path = "generic_capability_tests.rs"]
mod tests;
