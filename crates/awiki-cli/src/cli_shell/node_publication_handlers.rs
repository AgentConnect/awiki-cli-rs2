//! Local process boundary for the fixed Node publication workflow. Never exports keys or auth headers.
use super::App;
use crate::{cli_output::ExitError, cli_parser::ParsedCommand};
use base64::{engine::general_purpose::STANDARD, Engine};
use im_core::{
    ExternalHttpAuthDecision, ExternalHttpHeader, ExternalHttpRequest, ExternalHttpResponse,
};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read, Write};

const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInput {
    origin: String,
    target_service_id: String,
    tenant_id: String,
    operation_id: String,
    intent_hash: String,
    snapshot_json: String,
    markdown: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    origin: String,
    tenant_id: String,
    method: String,
    path: String,
    body_base64: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyInput {
    expected_sender_did: String,
    recipient_handle: String,
    client_message_id: String,
    notification: Notification,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Notification {
    #[serde(rename = "type")]
    event_type: String,
    version: String,
    event_id: String,
    node_origin: String,
    target_service_id: String,
    tenant_id: String,
    information_id: String,
    information_version: i64,
    operation_id: String,
    publication_round: i64,
    intent_hash: String,
    expires_at: String,
}
fn notification(input: &NotifyInput) -> Result<(), ExitError> {
    let n = &input.notification;
    origin(&n.node_origin)?;
    if n.event_type != "awiki.node.publication.review_requested"
        || n.version != "1.0"
        || [
            &n.event_id,
            &n.tenant_id,
            &n.information_id,
            &n.operation_id,
            &input.client_message_id,
        ]
        .into_iter()
        .any(|s| !canonical_uuid(s))
        || !n
            .target_service_id
            .strip_prefix("urn:uuid:")
            .is_some_and(canonical_uuid)
        || !(1..=9_007_199_254_740_991).contains(&n.information_version)
        || !(1..=9_007_199_254_740_991).contains(&n.publication_round)
        || n.intent_hash.len() != 64
        || !n
            .intent_hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || input.recipient_handle.starts_with("did:")
        || !input.recipient_handle.contains('.')
    {
        return Err(failure());
    }
    let expires = time::OffsetDateTime::parse(
        &n.expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| failure())?;
    if n.expires_at.len() != 20
        || !n.expires_at.ends_with('Z')
        || expires <= time::OffsetDateTime::now_utc()
    {
        return Err(failure());
    }
    Ok(())
}
fn failure() -> ExitError {
    ExitError::new("node_publication_bridge_failed", 2, "Node publication input, confirmation or transport failed", "Check the selected identity, trusted Node origin and persisted operation; inspect before retrying an uncertain write.")
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
    if !canonical_uuid(&input.tenant_id)
        || !matches!(input.method.as_str(), "GET" | "POST" | "PUT" | "DELETE")
    {
        return Err(failure());
    }
    let prefix = format!("/v1/tenants/{}", input.tenant_id);
    let path = input.path.split('?').next().ok_or_else(failure)?;
    if !(path == prefix || path.starts_with(&format!("{prefix}/")))
        || input.path.contains(['%', '#', '\\'])
        || path.split('/').any(|v| v == "." || v == "..")
    {
        return Err(failure());
    }
    let url = base.join(&input.path).map_err(|_| failure())?;
    if url.origin() != base.origin()
        || url.query_pairs().any(|(key, _)| {
            !matches!(
                key.as_ref(),
                "version" | "view" | "limit" | "offset" | "after"
            )
        })
    {
        return Err(failure());
    }
    Ok(url)
}
fn canonical_uuid(s: &str) -> bool {
    s.len() == 36
        && s != "00000000-0000-0000-0000-000000000000"
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
impl App {
    pub async fn run_node_publication_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        // No scripted confirmation or dry-run success can be confused with a real signature/send.
        if self.globals.dry_run || !command.args.is_empty() {
            return Err(failure());
        }
        let input_limit = if command.name == "node-publication.sign" {
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
        if command.name == "node-publication.sign" {
            let input: SignInput = serde_json::from_slice(&bytes).map_err(|_| failure())?;
            let node_origin = origin(&input.origin)?;
            let review = im_core::information_publication::InformationPublicationReview::parse(
                input.snapshot_json.as_bytes(),
                input.markdown,
                &input.target_service_id,
                &input.tenant_id,
                &input.operation_id,
                &input.intent_hash,
            )
            .map_err(|_| failure())?;
            let client =
                crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
            let capability = client
                .information_publication()
                .inspect_capability_async()
                .await
                .map_err(|e| {
                    crate::m_core_cli_adapter::error::map_im_error(
                        e,
                        "check Node publication signing capability",
                    )
                })?;
            let display = serde_json::json!({"node_origin":node_origin.as_str(),"signer_did":client.did().as_str(),"review":review.presentation(),"capability":capability});
            let confirmed = confirm(&display, review.intent_hash())?;
            let proof = client
                .information_publication()
                .sign_reviewed_async(review, &confirmed)
                .await
                .map_err(|e| {
                    crate::m_core_cli_adapter::error::map_im_error(e, "sign Node publication")
                })?;
            println!(
                "{}",
                serde_json::json!({"proof":proof,"signer_did":client.did().as_str(),"intent_hash":input.intent_hash})
            );
            return Ok(());
        }
        if command.name == "node-publication.notify" {
            use im_core::prelude::{
                MessageBody, MessageDeliveryOptions, MessageId, MessageSecurityMode, MessageTarget,
                PeerRef, SendMessageRequest,
            };
            let input: NotifyInput = serde_json::from_slice(&bytes).map_err(|_| failure())?;
            notification(&input)?;
            let client =
                crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
            if client.did().as_str() != input.expected_sender_did {
                return Err(failure());
            }
            notification(&input)?;
            let request = SendMessageRequest {
                target: MessageTarget::Direct(
                    PeerRef::parse(&input.recipient_handle, "").map_err(|_| failure())?,
                ),
                body: MessageBody::Payload {
                    payload: serde_json::to_value(&input.notification).map_err(|_| failure())?,
                },
                security: MessageSecurityMode::DefaultPlain,
                client_message_id: Some(
                    MessageId::parse(&input.client_message_id).map_err(|_| failure())?,
                ),
                delivery: MessageDeliveryOptions {
                    idempotency_key: Some(input.client_message_id),
                    wait_for_final_acceptance: false,
                },
                delegated_signing: None,
            };
            let result = client.messages().send_async(request).await.map_err(|e| {
                crate::m_core_cli_adapter::error::map_im_error(
                    e,
                    "notify Node publication reviewer",
                )
            })?;
            println!(
                "{}",
                serde_json::json!({"event_id":input.notification.event_id,"result":result})
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
        let client = crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await?;
        let headers = if body.is_some() {
            vec![ExternalHttpHeader::new("content-type", "application/json")
                .map_err(|_| failure())?]
        } else {
            vec![]
        };
        let request = ExternalHttpRequest::new(url.as_str(), &input.method, headers, body.clone())
            .map_err(|_| failure())?;
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
                request = request
                    .header("content-type", "application/json")
                    .body(body.clone());
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
    writeln!(tty, "{}\n此次签名提交后可能立即向公众发布上述内容。确认请键入完整 intent_hash（取消请直接回车）：", serde_json::to_string_pretty(display).map_err(|_| failure())?).map_err(|_| failure())?;
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
    writeln!(output, "{}\n此次签名提交后可能立即向公众发布上述内容。确认请键入完整 intent_hash（取消请直接回车）：", serde_json::to_string_pretty(display).map_err(|_| failure())?).map_err(|_| failure())?;
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
#[path = "node_publication_tests.rs"]
mod tests;
