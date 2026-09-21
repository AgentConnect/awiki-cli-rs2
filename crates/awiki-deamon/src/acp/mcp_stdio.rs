//! MCP stdio transport for the existing task-scoped question service.
//! There is no second question store or tool implementation here. The endpoint
//! and credential are inherited through ACP's MCP environment, never argv.
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const ENDPOINT_ENV: &str = "AWIKI_ACP_MCP_ENDPOINT";
const AUTH_ENV: &str = "AWIKI_ACP_MCP_AUTHORIZATION";
const MAX_REQUEST: usize = 64 * 1024;
const MAX_RESPONSE: usize = 128 * 1024;
const MAX_IN_FLIGHT: usize = 32;

#[derive(Clone)]
struct Transport {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    authorization: reqwest::header::HeaderValue,
}

impl Transport {
    fn new(endpoint: &str, authorization: &str) -> Result<Self> {
        // Only our numeric loopback endpoint is valid. No DNS, redirects, proxy,
        // userinfo or arbitrary paths can carry a task credential elsewhere.
        let endpoint = reqwest::Url::parse(endpoint).context("invalid_question_endpoint")?;
        if endpoint.scheme() != "http"
            || endpoint.host_str() != Some("127.0.0.1")
            || endpoint.port().is_none_or(|port| port == 0)
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/mcp"
        {
            bail!("invalid_question_endpoint");
        }
        let credential = authorization.strip_prefix("Bearer ").unwrap_or("");
        if credential.is_empty() || credential.chars().any(char::is_whitespace) {
            bail!("invalid_question_authorization");
        }
        let mut authorization = reqwest::header::HeaderValue::from_str(authorization)
            .map_err(|_| anyhow::anyhow!("invalid_question_authorization"))?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(16 * 60))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            authorization,
        })
    }

    async fn forward(&self, request: Value, output: &mpsc::Sender<Value>) -> Result<()> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .body(serde_json::to_vec(&request)?)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("question_transport_unavailable"))?;
        if !response.status().is_success() {
            bail!("question_transport_rejected");
        }
        if request.get("id").is_none() {
            return Ok(());
        }
        let is_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next() == Some("text/event-stream"));
        let mut chunks = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut events = EventDecoder::default();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("question_transport_interrupted"))?;
            // Process bytewise so several valid SSE frames in a single network
            // chunk are not incorrectly treated as one oversized response.
            for byte in chunk {
                buffer.push(byte);
                if buffer.len() > MAX_RESPONSE {
                    bail!("question_response_too_large");
                }
                if is_stream && byte == b'\n' {
                    if let Some(value) = events.line(&buffer)? {
                        if value.get("id").is_some() {
                            if value.get("id") != request.get("id") {
                                bail!("question_response_id_mismatch");
                            }
                        }
                        let complete = value.get("id").is_some();
                        output.send(value).await.context("question_output_closed")?;
                        if complete {
                            return Ok(());
                        }
                    }
                    buffer.clear();
                }
            }
        }
        if !is_stream {
            let value: Value =
                serde_json::from_slice(&buffer).context("invalid_question_response")?;
            if value["jsonrpc"] != "2.0" || value.get("id") != request.get("id") {
                bail!("question_response_id_mismatch");
            }
            output.send(value).await.context("question_output_closed")?;
        } else {
            bail!("question_transport_interrupted");
        }
        Ok(())
    }
}

#[derive(Default)]
struct EventDecoder {
    data: Vec<u8>,
}

impl EventDecoder {
    fn line(&mut self, line: &[u8]) -> Result<Option<Value>> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            if self.data.is_empty() {
                return Ok(None);
            }
            let value: Value = serde_json::from_slice(&std::mem::take(&mut self.data))
                .context("invalid_question_event")?;
            if value["jsonrpc"] != "2.0" {
                bail!("invalid_question_event");
            }
            return Ok(Some(value));
        }
        if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if self.data.len() + data.len() + 1 > MAX_RESPONSE {
                bail!("question_response_too_large");
            }
            self.data.extend_from_slice(data);
            self.data.push(b'\n');
        }
        Ok(None)
    }
}

async fn serve<R, W>(input: R, output: W, transport: Transport) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (sender, mut receiver) = mpsc::channel::<Value>(32);
    let writer = tokio::spawn(async move {
        let mut output = output;
        while let Some(value) = receiver.recv().await {
            let mut line = serde_json::to_vec(&value)?;
            line.push(b'\n');
            output.write_all(&line).await?;
            output.flush().await?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let mut input = BufReader::new(input);
    let mut requests = tokio::task::JoinSet::new();
    let result = async {
        loop {
            let mut line = Vec::new();
            let mut bounded = (&mut input).take((MAX_REQUEST + 1) as u64);
            let count = tokio::select! {
                count = bounded.read_until(b'\n', &mut line) => count?,
                _ = sender.closed() => bail!("question_output_closed"),
            };
            if count == 0 {
                break;
            }
            if count > MAX_REQUEST {
                bail!("question_request_too_large");
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let request: Value =
                serde_json::from_slice(&line).context("invalid_question_request")?;
            if request["jsonrpc"] != "2.0" || !request["method"].is_string() {
                bail!("invalid_question_request");
            }
            while requests.try_join_next().is_some() {}
            if requests.len() >= MAX_IN_FLIGHT {
                bail!("too_many_question_requests");
            }
            let transport = transport.clone();
            let sender = sender.clone();
            requests.spawn(async move {
                let id = request.get("id").cloned();
                if transport.forward(request, &sender).await.is_err() {
                    if let Some(id) = id {
                        let _ = sender
                            .send(json!({"jsonrpc":"2.0","id":id,"error":{
                            "code":-32603,"message":"Question transport unavailable"}}))
                            .await;
                    }
                }
            });
        }
        Ok(())
    }
    .await;
    // EOF means the owning ACP client ended; don't leave a human-input request
    // running in a detached process. Task closure remains owned by the daemon.
    requests.shutdown().await;
    drop(sender);
    writer.await.context("question_writer_failed")??;
    result
}

pub fn run() -> Result<()> {
    let endpoint = std::env::var(ENDPOINT_ENV).context("question_endpoint_missing")?;
    let authorization = std::env::var(AUTH_ENV).context("question_authorization_missing")?;
    let transport = Transport::new(&endpoint, &authorization)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve(tokio::io::stdin(), tokio::io::stdout(), transport))
}

pub(super) fn server_config(
    http: &Value,
    capabilities: &Value,
    executable: &Path,
) -> Result<Value> {
    if capabilities["mcpCapabilities"]["http"] == true {
        return Ok(http.clone());
    }
    let endpoint = http["url"].as_str().context("question_endpoint_missing")?;
    let authorization = http["headers"][0]["value"]
        .as_str()
        .context("question_authorization_missing")?;
    Transport::new(endpoint, authorization)?;
    Ok(json!({
        "name":"awiki_questions", "command":executable,
        "args":["__acp-mcp-stdio"],
        "env":[{"name":ENDPOINT_ENV,"value":endpoint},{"name":AUTH_ENV,"value":authorization}]
    }))
}

#[cfg(test)]
#[path = "mcp_stdio_tests.rs"]
mod tests;
