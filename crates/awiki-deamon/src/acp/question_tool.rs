//! A task-scoped MCP question tool for clients without native elicitation.
//! ACP remains stable v1. No model proxy, installation, or credential service is
//! involved. This listener only exposes request_user_input on loopback with a random token.
//! Avoid the reserved ask_user name: Gemini ACP excludes that name, including
//! identically named MCP tools, because its terminal question UI is unavailable.
use crate::DaemonState;
use anyhow::{Context, Result};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc, time::Duration};

struct ContextState {
    state: DaemonState,
    key: String,
    run: String,
    token: String,
    host: String,
    closed: tokio::sync::watch::Receiver<bool>,
}
pub struct QuestionTool {
    pub config: Value,
    task: tokio::task::JoinHandle<()>,
    closed: tokio::sync::watch::Sender<bool>,
}
impl Drop for QuestionTool {
    fn drop(&mut self) {
        let _ = self.closed.send(true);
        self.task.abort();
    }
}

impl QuestionTool {
    pub async fn start(state: DaemonState, key: String, run: String) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let host = listener.local_addr()?.to_string();
        let token = crate::security::runtime_token::RuntimeRpcToken::generate()
            .as_str()
            .to_owned();
        let (closed, receiver) = tokio::sync::watch::channel(false);
        let context = Arc::new(ContextState {
            state,
            key,
            run,
            host: host.clone(),
            token: format!("Bearer {token}"),
            closed: receiver,
        });
        let app = Router::new()
            .route("/mcp", post(handle))
            .layer(DefaultBodyLimit::max(64 * 1024))
            .with_state(context);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let config = json!({"type":"http","name":"awiki_questions","url":format!("http://{host}/mcp"),"headers":[{"name":"Authorization","value":format!("Bearer {token}")} ]});
        Ok(Self {
            config,
            task,
            closed,
        })
    }
}

async fn handle(
    State(context): State<Arc<ContextState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.contains_key("origin")
        || headers.get("host").and_then(|h| h.to_str().ok()) != Some(context.host.as_str())
        || headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(context.token.as_str())
        || *context.closed.borrow()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let id = request.get("id").cloned();
    if request["jsonrpc"] != "2.0" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(id) = id else {
        if request["method"] == "notifications/cancelled" {
            let question_id = format!(
                "question-{}",
                super::store::session_key(
                    &context.key,
                    &context.run,
                    &format!("mcp:{}", request["params"]["requestId"])
                )
            );
            let _ = super::store::mutate(&context.state, &context.key, None, |s| {
                if s.active_run(&context.run)
                    && !s.stopping
                    && s.questions
                        .iter()
                        .any(|q| q.id == question_id && q.response.is_none())
                {
                    s.interaction_error = Some("question_cancelled_by_client".into());
                }
                Ok(())
            });
        }
        return StatusCode::ACCEPTED.into_response();
    };
    let progress = request["params"]["_meta"]["progressToken"].clone();
    if request["method"] == "tools/call"
        && (progress.is_string() || progress.is_i64() || progress.is_u64())
    {
        // Waiting for a human is longer than ordinary MCP tool timeouts.
        // Report elapsed waiting checks using the caller's progress token;
        // no answer or completion is sent until the requester responds.
        let pending = Box::pin(async move { rpc_response(&context, &request, id).await });
        let ticks = tokio::time::interval(Duration::from_secs(10));
        let stream = futures_util::stream::unfold(
            (pending, ticks, progress, 0_u64, false),
            |(mut pending, mut ticks, progress, count, done)| async move {
                if done {
                    return None;
                }
                let (value, done) = tokio::select! {
                    biased;
                    result = &mut pending => (result, true),
                    _ = ticks.tick() => (json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":progress,"progress":count,"message":"Waiting for the task requester's answer"}}), false),
                };
                Some((
                    Ok::<_, Infallible>(Event::default().event("message").data(value.to_string())),
                    (pending, ticks, progress, count + 1, done),
                ))
            },
        );
        return Sse::new(stream).into_response();
    }
    Json(rpc_response(&context, &request, id).await).into_response()
}

async fn rpc_response(context: &ContextState, request: &Value, id: Value) -> Value {
    let output = dispatch(&context, &request).await;
    match output {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(_) => {
            if request["method"] == "tools/call" {
                let _ = super::store::mutate(&context.state, &context.key, None, |s| {
                    if s.active_run(&context.run) {
                        s.interaction_error = Some("unsupported_or_expired_question".into());
                    }
                    Ok(())
                });
            }
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Question tool request unavailable or invalid"}})
        }
    }
}

async fn dispatch(context: &ContextState, request: &Value) -> Result<Value> {
    let session = super::store::load(&context.state, &context.key)?;
    if !session.active_run(&context.run) || session.stopping {
        anyhow::bail!("stale_task")
    }
    match request["method"].as_str() {
        Some("initialize") => {
            let requested = request["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = if matches!(requested, "2025-03-26" | "2025-06-18" | "2025-11-25") {
                requested
            } else {
                "2025-11-25"
            };
            Ok(
                json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"awiki_questions","version":"1.0.0"},"instructions":"When user input is needed, call request_user_input and wait. Never select an answer for the user. Decline and cancellation are final decisions; do not ask the same question through another tool."}),
            )
        }
        Some("ping") => Ok(json!({})),
        Some("tools/list") => Ok(
            json!({"tools":[{"name":"request_user_input","description":"Ask the task requester for information and wait for their real answer in the chat. This tool does not execute commands or grant tool permissions.","inputSchema":{"type":"object","required":["message","schema_json"],"properties":{"message":{"type":"string"},"schema_json":{"type":"string","description":"JSON-encoded Schema object with flat string, number, integer, boolean or multi-select fields. Set required for mandatory fields. Example: {\"type\":\"object\",\"required\":[\"color\"],\"properties\":{\"color\":{\"type\":\"string\",\"enum\":[\"red\",\"blue\"]}}}"}},"additionalProperties":false}}]}),
        ),
        Some("tools/call") => {
            if request["params"]["name"] != "request_user_input" {
                anyhow::bail!("unknown_tool")
            }
            let args = &request["params"]["arguments"];
            let schema: Value = serde_json::from_str(
                args["schema_json"]
                    .as_str()
                    .context("schema_json_required")?,
            )?;
            let form = json!({"mode":"form","sessionId":session.native_session_id.context("session_not_ready")?,"message":args["message"],"requestedSchema":schema});
            let mut closed = context.closed.clone();
            let answer = tokio::select! {
                result=super::client::await_answer_id(&context.state,&context.key,&context.run,form,format!("mcp:{}",request["id"]))=>result?,
                _=closed.changed()=>anyhow::bail!("task_closed"),
            };
            Ok(
                json!({"content":[{"type":"text","text":serde_json::to_string(&answer)?}],"isError":false}),
            )
        }
        _ => anyhow::bail!("unsupported_question_tool_method"),
    }
}
