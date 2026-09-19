use acp::schema::{v1::*, ProtocolVersion};
use agent_client_protocol::{self as acp, AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

// Gemini CLI 0.59 still advertises the ACP v1 legacy models surface.
// Keep those response fields while the official SDK owns all framing/I/O.
#[derive(Debug, Clone, Serialize, Deserialize, acp::JsonRpcRequest)]
#[request(method="session/new",response=Value)]
struct NewSessionWithModels {
    #[serde(flatten)]
    inner: NewSessionRequest,
}
#[derive(Debug, Clone, Serialize, Deserialize, acp::JsonRpcRequest)]
#[request(method="session/load",response=Value)]
struct LoadSessionWithModels {
    #[serde(flatten)]
    inner: LoadSessionRequest,
}
#[derive(Debug, Clone, Serialize, Deserialize, acp::JsonRpcRequest)]
#[request(method="session/resume",response=Value)]
struct ResumeSessionWithModels {
    #[serde(flatten)]
    inner: ResumeSessionRequest,
}
// Keep every list item: the SDK's tolerant list decoder skips malformed items,
// which cannot establish that a previously recorded session is absent.
#[derive(Debug, Clone, Serialize, Deserialize, acp::JsonRpcRequest)]
#[request(method="session/list",response=Value)]
struct StrictListSessions {
    #[serde(flatten)]
    inner: ListSessionsRequest,
}
#[derive(Debug, Clone, Serialize, Deserialize, acp::JsonRpcRequest)]
#[request(method="session/set_model",response=Value)]
#[serde(rename_all = "camelCase")]
struct SetSessionModelRequest {
    session_id: SessionId,
    model_id: String,
}

use super::{
    store::{self, Question},
    Brand,
};
use crate::security::runtime_token::current_time_millis;
use crate::{state::CliRuntimeProfileRecord, DaemonState};

mod configuration;
pub use super::models::current_model;
use super::models::session_options;
use configuration::{configure_model, open_session};
pub use configuration::{
    prepare as prepare_configuration, prepare_cancellable as prepare_configuration_cancellable,
    PreparedConfiguration,
};

pub fn launch_config(profile: &CliRuntimeProfileRecord) -> Result<AcpAgentConfig> {
    let brand = Brand::parse(&profile.driver_id)?;
    let mut config = AcpAgentConfig::new(
        profile
            .binary_path
            .clone()
            .unwrap_or_else(|| crate::cli_runtime_env::resolve_cli_binary(brand.command())),
    )
    .args(brand.args().iter().copied());
    if let Some(path) = crate::cli_runtime_env::cli_child_path() {
        config = config.env("PATH", path.to_string_lossy());
    }
    if brand == Brand::Kimi {
        // Kimi does not reset its MCP tool timeout from progress events.
        // Keep the task child alive past the question's 15-minute expiry;
        // per-server user settings retain their official higher precedence.
        config = config.env("KIMI_MCP_TOOL_TIMEOUT_MS", "960000");
    }
    if let Some(home) = &profile.config_home {
        let key = match brand {
            Brand::OpenCode => "OPENCODE_CONFIG_DIR",
            Brand::Gemini => "GEMINI_CLI_HOME",
            Brand::Kimi => "KIMI_CODE_HOME",
            Brand::DeepseekHarness => "DSH_HOME",
        };
        config = config.env(key, home.to_string_lossy());
    }
    Ok(config)
}

fn launch_in_workspace(
    profile: &CliRuntimeProfileRecord,
    cwd: &std::path::Path,
    native_session_id: Option<&str>,
) -> Result<AcpAgentConfig> {
    let mut config = launch_config(profile)?;
    // Gemini's startup retention worker must know which native session is in
    // use. Otherwise an empty checkpoint from an earlier ACP load can cause
    // cleanup to delete every file for that session (including its history).
    if Brand::parse(&profile.driver_id)? == Brand::Gemini {
        if let Some(id) = native_session_id {
            config = config.args(["--resume", id]);
        }
    }
    // SDK 2.1 has no child cwd option. A fixed POSIX launcher changes only the
    // child directory and execs the client in the SDK-owned process group.
    // Paths are positional arguments, never interpolated into shell source.
    Ok(AcpAgentConfig::new("/bin/sh")
        .args([
            "-c",
            r#"cd -- "$1" || exit 1
shift
daemon_pid="$1"
shift
agent_pid=$$
(
  while [ "$(/bin/ps -p "$agent_pid" -o ppid= | /usr/bin/tr -d '[:space:]')" = "$daemon_pid" ]; do
    sleep 1
  done
  kill -KILL "-$agent_pid"
) </dev/null >/dev/null 2>&1 &
exec "$@""#,
            "awiki-acp",
        ])
        .arg(cwd.to_string_lossy())
        .arg(std::process::id().to_string())
        .arg(config.command().to_string_lossy())
        .args(config.arguments().iter().cloned())
        .envs(config.environment().clone()))
}

fn configure_question_tool(
    brand: Brand,
    launch: AcpAgentConfig,
    question_config: &Value,
) -> Result<(AcpAgentConfig, Vec<tempfile::NamedTempFile>)> {
    if brand != Brand::DeepseekHarness {
        return Ok((launch, vec![]));
    }
    // DSH's ACP adapter fixes MCP calls at 60 seconds. Its official profile
    // overlay supplies a task-local client with the human-answer timeout.
    // The file contains only environment references; the guard removes it.
    use std::io::Write;
    let mut shell_env = tempfile::Builder::new()
        .prefix("awiki-acp-shell-env-")
        .suffix(".cjs")
        .tempfile()?;
    shell_env.write_all(
        br#"module.exports = {
  name: 'awiki-acp-shell-env',
  inject: ['shellEnv'],
  apply(ctx) {
    ctx.shellEnv.register({
      name: 'awiki-acp-file-delivery',
      variables: {
        DSH_AWIKI_RUNTIME_RPC_TOKEN: {
          description: 'Current AWiki task file-delivery credential; never print it.'
        }
      },
      resolve() {
        const value = process.env.AWIKI_RUNTIME_RPC_TOKEN;
        return value ? { DSH_AWIKI_RUNTIME_RPC_TOKEN: value } : {};
      }
    });
  }
};
"#,
    )?;
    let mut patch = tempfile::Builder::new()
        .prefix("awiki-acp-questions-")
        .suffix(".yml")
        .tempfile()?;
    patch.write_all(
        br#"- insert:
    - id: awiki-acp-questions
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: awiki_questions
        transport: streamable-http
        url: !!js process.env.AWIKI_ACP_QUESTION_URL
        headers:
          Authorization: !!js process.env.AWIKI_ACP_QUESTION_AUTH
        toolCallTimeoutMs: 960000
        failOnStartupError: true
"#,
    )?;
    writeln!(
        patch,
        "    - id: awiki-acp-shell-env\n      name: {}",
        serde_json::to_string(&shell_env.path().to_string_lossy())?
    )?;
    let launch = launch
        .args([
            "--patch".into(),
            patch.path().to_string_lossy().into_owned(),
        ])
        .env(
            "AWIKI_ACP_QUESTION_URL",
            question_config["url"]
                .as_str()
                .context("question_url_missing")?,
        )
        .env(
            "AWIKI_ACP_QUESTION_AUTH",
            question_config["headers"][0]["value"]
                .as_str()
                .context("question_auth_missing")?,
        );
    Ok((launch, vec![patch, shell_env]))
}

fn configure_gemini_replay(
    brand: Brand,
    launch: AcpAgentConfig,
) -> Result<(AcpAgentConfig, Option<tempfile::NamedTempFile>)> {
    if brand != Brand::Gemini {
        return Ok((launch, None));
    }
    use base64::Engine;
    use std::io::Write;
    let mut hook = tempfile::Builder::new()
        .prefix("awiki-gemini-replay-")
        .suffix(".mjs")
        .tempfile()?;
    hook.write_all(include_bytes!("gemini_replay.mjs"))?;
    let url = reqwest::Url::from_file_path(hook.path())
        .map_err(|_| anyhow::anyhow!("acp_invalid_hook_path"))?;
    let preload = format!(
        "import {{register}} from 'node:module';register({});",
        serde_json::to_string(url.as_str())?
    );
    // A base64 data URL is one NODE_OPTIONS token even when TMPDIR has spaces.
    // It contains only a module path, never credentials or user message text.
    let prior = launch
        .environment()
        .get("NODE_OPTIONS")
        .cloned()
        .unwrap_or_else(|| std::env::var("NODE_OPTIONS").unwrap_or_default());
    let options = format!(
        "{prior} --import=data:text/javascript;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(preload)
    );
    Ok((launch.env("NODE_OPTIONS", options), Some(hook)))
}

fn missing_native_context(error: &acp::Error, brand: Brand, id: &str) -> bool {
    error.code == ErrorCode::ResourceNotFound
        || (error.code == ErrorCode::InvalidParams
            && match brand {
                Brand::Kimi => error.message == format!("Invalid params: Unknown sessionId: {id}"),
                Brand::DeepseekHarness => {
                    error.message == format!("Invalid params: session is not resumable: {id}")
                }
                _ => false,
            })
        || (brand == Brand::Gemini
            && error.code == ErrorCode::InternalError
            && error
                .data
                .as_ref()
                .and_then(|data| data["details"].as_str())
                .is_some_and(|details| {
                    matches!(
                        details,
                        "No previous sessions found for this project."
                            | "awiki_gemini_history_unrecoverable"
                    )
                }))
}

fn gemini_startup_missing(line: &str, id: &str) -> bool {
    let line = line.trim();
    line == "Error resuming session: No previous sessions found for this project."
        || line == "Error resuming session: awiki_gemini_history_unrecoverable"
        || line == format!("Error resuming session: Invalid session identifier \"{id}\".")
}

async fn session_absent(cx: &ConnectionTo<Agent>, cwd: &std::path::Path, id: &str) -> bool {
    // Failure, malformed pages, repeated cursors and excessive pagination are
    // inconclusive. They must never offer destructive context replacement.
    let query = async {
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let page = cx
                .send_request(StrictListSessions {
                    inner: ListSessionsRequest::new()
                        .cwd(cwd.to_path_buf())
                        .cursor(cursor.clone()),
                })
                .block_task()
                .await
                .ok()?;
            for session in page.get("sessions")?.as_array()? {
                let listed = session.get("sessionId")?.as_str()?;
                if listed.is_empty() || listed == id {
                    return Some(false);
                }
            }
            cursor = match page.get("nextCursor") {
                None | Some(Value::Null) => return Some(true),
                Some(Value::String(next)) if !next.is_empty() && seen.insert(next.clone()) => {
                    Some(next.clone())
                }
                _ => return None,
            };
        }
        None
    };
    tokio::time::timeout(Duration::from_secs(10), query)
        .await
        .ok()
        .flatten()
        == Some(true)
}

fn mark_context_lost(state: &DaemonState, key: &str, run: &str, native: &str) {
    let _ = store::mutate(state, key, None, |s| {
        if s.active_run(run) && !s.stopping && s.native_session_id.as_deref() == Some(native) {
            s.context_lost = true;
        }
        Ok(())
    });
}

fn initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
        serde_json::from_value(json!({"elicitation":{"form":{}}})).expect("ACP v1 capabilities"),
    )
}

pub async fn inspect(profile: &CliRuntimeProfileRecord) -> Result<Value> {
    let brand = Brand::parse(&profile.driver_id)?;
    let binary = profile
        .binary_path
        .clone()
        .unwrap_or_else(|| crate::cli_runtime_env::resolve_cli_binary(brand.command()));
    let version = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(binary)
            .envs(crate::cli_runtime_env::cli_child_path().map(|path| ("PATH", path)))
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("acp_version_timeout")?
    .context("acp_not_installed")?;
    if !version.status.success() {
        bail!("acp_version_failed")
    }
    let version_text = String::from_utf8_lossy(&version.stdout);
    let binary_version = regex::Regex::new(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.]+)?")
        .unwrap()
        .find(&version_text)
        .context("acp_version_unavailable")?
        .as_str()
        .to_owned();
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let (launch, _replay_hook) = configure_gemini_replay(brand, launch_config(profile)?)?;
    let connection = acp::Client.builder().connect_with(
        AcpAgent::new(launch),
        async move |cx: ConnectionTo<Agent>| {
            let initialized = cx.send_request(initialize_request()).block_task().await?;
            if initialized.protocol_version != ProtocolVersion::V1 {
                return Err(acp::Error::invalid_params());
            }
            *output.lock().unwrap() = Some(serde_json::to_value(initialized).unwrap());
            Ok(())
        },
    );
    tokio::time::timeout(Duration::from_secs(20), connection)
        .await
        .context("acp_probe_timeout")?
        .map_err(|_| anyhow::anyhow!("acp_setup_required"))?;
    let mut value = result.lock().unwrap().take().context("acp_probe_failed")?;
    value["binaryVersion"] = json!(binary_version);
    if value["agentInfo"]["version"].as_str().is_none() {
        bail!("acp_version_unavailable");
    }
    if value["agentCapabilities"]["mcpCapabilities"]["http"] != true {
        bail!("acp_question_tool_unsupported");
    }
    Ok(value)
}

pub struct Turn {
    pub state: DaemonState,
    pub key: String,
    pub run_id: String,
    pub profile: CliRuntimeProfileRecord,
    pub cwd: PathBuf,
    pub prompt: Vec<ContentBlock>,
    pub environment: Vec<(String, String)>,
}

pub struct TurnResult {
    pub text: String,
    pub cancelled: bool,
}

pub async fn run(mut turn: Turn) -> Result<TurnResult> {
    // Native clients key their project history by cwd. Keep startup and ACP
    // session paths identical on macOS (/var may resolve to /private/var).
    turn.cwd = std::fs::canonicalize(&turn.cwd).context("acp_workspace_unavailable")?;
    // Gemini 0.59's load initializes a new checkpoint before reading history.
    // Its filenames include only the UTC minute, so loading during the native
    // session's creation minute destroys that history (upstream #28693). Wait
    // past that boundary once; never rewrite the client's files or replay text.
    let prior = store::load(&turn.state, &turn.key)?;
    if Brand::parse(&turn.profile.driver_id)? == Brand::Gemini && prior.native_session_id.is_some()
    {
        let boundary = (prior.native_created_at_ms.unwrap_or(current_time_millis()?) / 60_000 + 1)
            * 60_000
            + 1_000;
        if current_time_millis()? < boundary {
            store::mutate(&turn.state, &turn.key, None, |s| {
                s.restoring = true;
                Ok(())
            })?;
        }
        while current_time_millis()? < boundary {
            let current = store::load(&turn.state, &turn.key)?;
            if !current.active_run(&turn.run_id) || current.stopping {
                return Ok(TurnResult {
                    text: String::new(),
                    cancelled: true,
                });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let question_tool = super::question_tool::QuestionTool::start(
        turn.state.clone(),
        turn.key.clone(),
        turn.run_id.clone(),
    )
    .await?;
    let brand = Brand::parse(&turn.profile.driver_id)?;
    let mcp_servers = if brand == Brand::DeepseekHarness {
        json!([])
    } else {
        json!([question_tool.config])
    };
    let watchdog_state = turn.state.clone();
    let watchdog_key = turn.key.clone();
    let watchdog_run = turn.run_id.clone();
    let accepting = Arc::new(AtomicBool::new(false));
    let input_session = store::load(&turn.state, &turn.key)?;
    let native = Arc::new(Mutex::new(input_session.native_session_id.clone()));
    let failure = Arc::new(Mutex::new(None::<String>));
    let result = Arc::new(Mutex::new(None));
    let state = turn.state.clone();
    let key = turn.key.clone();
    let run_id = turn.run_id.clone();
    let update_accepting = accepting.clone();
    let update_native = native.clone();
    let update_failure = failure.clone();
    let permission_failure = failure.clone();
    let permission_state = state.clone();
    let permission_key = key.clone();
    let permission_run = run_id.clone();
    let question_state = state.clone();
    let question_key = key.clone();
    let question_run = run_id.clone();
    let question_failure = failure.clone();
    let unknown_failure = failure.clone();
    let output = result.clone();
    let output_native = native.clone();
    let output_accepting = accepting.clone();
    let launch = launch_in_workspace(&turn.profile, &turn.cwd, prior.native_session_id.as_deref())?;
    let (launch, _question_patch) = configure_question_tool(brand, launch, &question_tool.config)?;
    let (launch, _replay_hook) = configure_gemini_replay(brand, launch)?;
    let initialized = Arc::new(AtomicBool::new(false));
    let did_initialize = initialized.clone();
    let startup_missing = Arc::new(AtomicBool::new(false));
    let saw_missing = startup_missing.clone();
    let startup_native = prior.native_session_id.clone();
    let agent = AcpAgent::new(
        turn.environment
            .iter()
            .fold(launch, |config, (key, value)| config.env(key, value)),
    )
    .with_debug(move |line, direction| {
        // Classify only the exact startup diagnostic; never retain or log
        // stderr/protocol payloads, and never infer loss from exit code 42.
        if brand == Brand::Gemini
            && direction == acp::LineDirection::Stderr
            && !initialized.load(Ordering::Acquire)
            && startup_native
                .as_deref()
                .is_some_and(|id| gemini_startup_missing(line, id))
        {
            saw_missing.store(true, Ordering::Release);
        }
    });
    let connection = acp::Client.builder()
        .on_receive_notification(async move |notification: SessionNotification, _cx| {
            if !update_accepting.load(Ordering::Acquire) { return Ok(()); }
            if update_native.lock().unwrap().as_deref() != Some(notification.session_id.to_string().as_str()) { return Err(acp::Error::invalid_params()); }
            let update = serde_json::to_value(&notification.update).unwrap();
            let result = store::mutate(&state, &key, None, |s| {
                if !s.active_run(&run_id) || s.stopping { return Ok(()); }
                match update["sessionUpdate"].as_str() {
                    Some("agent_message_chunk") => {
                        if update["content"]["type"].as_str() != Some("text") { bail!("unsupported_output_content"); }
                        let text = update["content"]["text"].as_str().context("invalid_output_content")?;
                        if s.text.len() + text.len() > 1024 * 1024 { bail!("acp_output_limit"); }
                        s.text.push_str(text);
                    }
                    Some("tool_call" | "tool_call_update") => {
                        let id = update["toolCallId"].as_str().context("invalid_tool_id")?;
                        let item = s.tools.iter_mut().find(|v|v["id"].as_str()==Some(id));
                        let summary = super::tools::update_summary(item.as_deref(), &update);
                        if let Some(item) = item { *item=summary; } else { if s.tools.len() == 64 { s.tools.remove(0); s.omitted_tool_count += 1; } s.tools.push(summary); }
                    }
                    Some("config_option_update") => s.update_configuration(update["configOptions"].clone()),
                    Some("current_model_update") => {
                        if let Some(model)=update["currentModelId"].as_str() {
                            s.model=Some(model.to_owned());
                            if s.options.is_object() {s.options["currentModelId"]=json!(model);}
                        }
                    }
                    // Thoughts and usage are not assistant reply content.
                    _ => {},
                }
                Ok(())
            });
            if result.is_err() { *update_failure.lock().unwrap()=Some("acp_update_failed".into()); }
            Ok(())
        }, acp::on_receive_notification!())
        .on_receive_request(async move |request: RequestPermissionRequest, responder, _cx| {
            let current = store::load(&permission_state,&permission_key).ok();
            let allowed = current.as_ref().is_some_and(|s|s.active_run(&permission_run) && !s.stopping && s.native_session_id.as_deref()==Some(request.session_id.to_string().as_str()));
            let raw=serde_json::to_value(&request).unwrap();
            let title=raw["toolCall"]["title"].as_str().unwrap_or("").to_ascii_lowercase();
            if allowed && matches!(title.as_str(),"askuserquestion"|"ask_user"|"question") {
                // No verified native business-question adapter is advertised.
                // Permission option IDs such as allow_once are not answers.
                *permission_failure.lock().unwrap()=Some("unsupported_native_question".into());
                responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled))?;
                return Ok(());
            }
            let option = if allowed { request.options.iter().find(|o|o.kind==PermissionOptionKind::AllowOnce) } else { None };
            let outcome = option.map(|o|RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(o.option_id.clone()))).unwrap_or(RequestPermissionOutcome::Cancelled);
            responder.respond(RequestPermissionResponse::new(outcome))
        }, acp::on_receive_request!())
        .on_receive_request(async move |request: CreateElicitationRequest, responder, cx| {
            let state=question_state.clone();let key=question_key.clone();let run_id=question_run.clone(); let failed=question_failure.clone();
            let question_id=format!("elicitation:{}",responder.id());
            // Do not hold the connection dispatcher while waiting for a human.
            cx.spawn(async move {
                let value=serde_json::to_value(request).unwrap();
                let response = await_answer_id(&state,&key,&run_id,value,question_id).await;
                match response {
                    Ok(value) => responder.respond(serde_json::from_value::<CreateElicitationResponse>(value).map_err(|_|acp::Error::invalid_params())?),
                    Err(_) => {
                        if store::load(&state,&key).is_ok_and(|s|s.active_run(&run_id) && !s.stopping) {
                            *failed.lock().unwrap()=Some("unsupported_or_expired_question".into());
                        }
                        responder.respond(serde_json::from_value::<CreateElicitationResponse>(json!({"action":"cancel"})).unwrap())
                    }
                }
            })?;
            Ok(())
        }, acp::on_receive_request!())
        .on_receive_request(async move |_request: acp::UntypedMessage,responder,_cx| {
            *unknown_failure.lock().unwrap()=Some("unsupported_interaction".into());
            responder.respond_with_error(acp::Error::method_not_found())
        },acp::on_receive_request!())
        .connect_with(agent, async move |cx: ConnectionTo<Agent>| {
            let init=cx.send_request(initialize_request()).block_task().await?;
            did_initialize.store(true, Ordering::Release);
            if init.protocol_version!=ProtocolVersion::V1 { return Err(acp::Error::invalid_params()); }
            let caps=serde_json::to_value(&init.agent_capabilities).unwrap();
            if caps["mcpCapabilities"]["http"]!=true {return Err(acp::Error::invalid_params());}
            let existing=store::load(&turn.state,&turn.key).map_err(|_|acp::Error::internal_error())?;
            let session_result = open_session(&cx, &caps, &turn.cwd, existing.native_session_id.as_deref(), mcp_servers).await;
            let session = match session_result {
                Ok(v)=>v,
                Err(error)=>{
                    if let Some(id) = &existing.native_session_id {
                        let missing = missing_native_context(&error, brand, id)
                            || caps["loadSession"]!=true && !caps["sessionCapabilities"]["resume"].is_object()
                            || brand == Brand::OpenCode && error.code == ErrorCode::InternalError
                                && error.data.as_ref().is_some_and(|data| data["service"] == "session")
                                && caps["sessionCapabilities"]["list"].is_object()
                                && session_absent(&cx, &turn.cwd, id).await;
                        if missing { mark_context_lost(&turn.state, &turn.key, &turn.run_id, id); }
                    }
                    return Err(error);
                }
            };
            let id = existing.native_session_id.clone().or_else(||session["sessionId"].as_str().map(str::to_string)).ok_or_else(acp::Error::invalid_params)?;
            let sid=SessionId::new(id.clone());
            *output_native.lock().unwrap()=Some(id.clone());
            let options=session_options(&session);
            store::mutate(&turn.state,&turn.key,None,|s|{
                if s.native_session_id.is_none() {s.native_created_at_ms=Some(current_time_millis()?);}
                s.restoring=false;
                s.native_session_id=Some(id);s.capabilities=caps.clone();s.update_catalog(options.clone());Ok(())
            }).map_err(|_|acp::Error::internal_error())?;
            if let Some(model) = existing.model_selection().or(turn.profile.default_model).or(existing.model.clone()) {
                let confirmed=configure_model(&cx,&sid,options,&model).await.map_err(|error| {
                    let _ = store::mutate(&turn.state,&turn.key,None,|s| {
                        s.interaction_error=Some("model_configuration_failed".into());Ok(())
                    });
                    error
                })?;
                store::mutate(&turn.state,&turn.key,None,|s|{s.update_configuration(confirmed);Ok(())}).map_err(|_|acp::Error::internal_error())?;
            } else {
                store::mutate(&turn.state,&turn.key,None,|s|{s.update_configuration(options);Ok(())}).map_err(|_|acp::Error::internal_error())?;
            }
            if turn.prompt.iter().any(|b|matches!(b,ContentBlock::Image(_))) && caps["promptCapabilities"]["image"]!=true { return Err(acp::Error::invalid_params()); }
            output_accepting.store(true,Ordering::Release);
            let mut prompt=Box::pin(cx.send_request(PromptRequest::new(sid.clone(),turn.prompt)).block_task());
            let mut cancelled=false;
            let mut cancel_at=None;
            let stop=loop {
                tokio::select! {
                    response=&mut prompt => break response?,
                    _=tokio::time::sleep(Duration::from_millis(100)) => {
                        let current=store::load(&turn.state,&turn.key).map_err(|_|acp::Error::internal_error())?;
                        if current.interaction_error.is_some() {*failure.lock().unwrap()=Some("question_failed".into());}
                        if !current.active_run(&turn.run_id) || current.stopping || failure.lock().unwrap().is_some() {
                            if !cancelled {
                                cancelled=true;cancel_at=Some(std::time::Instant::now());
                                cx.send_notification(CancelNotification::new(sid.clone()))?;
                            } else if cancel_at.unwrap().elapsed()>Duration::from_secs(10) {
                                // Returning drops the SDK process-group guard. No replacement
                                // task starts until the connection has been torn down.
                                output_accepting.store(false,Ordering::Release);
                                if failure.lock().unwrap().is_some() {return Err(acp::Error::internal_error());}
                                *output.lock().unwrap()=Some(TurnResult{text:String::new(),cancelled:true});
                                return Ok(());
                            }
                        }
                    }
                }
            };
            output_accepting.store(false,Ordering::Release);
            if let Some(error)=failure.lock().unwrap().as_ref() { let _=error; return Err(acp::Error::internal_error()); }
            if !cancelled && !matches!(stop.stop_reason,StopReason::EndTurn|StopReason::Cancelled) { return Err(acp::Error::internal_error()); }
            let current=store::load(&turn.state,&turn.key).map_err(|_|acp::Error::internal_error())?;
            if current.interaction_error.is_some() {return Err(acp::Error::internal_error());}
            if !cancelled && stop.stop_reason != StopReason::Cancelled && current.questions.iter().any(|q| q.response.is_none()) {
                // A client-side MCP timeout must not turn an unanswered question
                // into a successful model response or silently choose for a user.
                return Err(acp::Error::internal_error());
            }
            let text=current.text;
            *output.lock().unwrap()=Some(TurnResult{text,cancelled:cancelled || current.stopping || stop.stop_reason==StopReason::Cancelled});
            if caps["sessionCapabilities"]["close"].is_object() {
                let _=tokio::time::timeout(Duration::from_secs(5),cx.send_request(CloseSessionRequest::new(sid)).block_task()).await;
            }
            Ok(())
        });
    let hard_stop = async {
        let mut requested = None;
        loop {
            let current = store::load(&watchdog_state, &watchdog_key)?;
            if !current.active_run(&watchdog_run) || current.stopping {
                let since = requested.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() > Duration::from_secs(10) {
                    return Ok::<_, anyhow::Error>(());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        completed=tokio::time::timeout(Duration::from_secs(30*60),connection)=>{
            // Closing an unanswered question can make a native prompt return
            // an RPC error before it acknowledges session/cancel. Once the
            // connection/process group has ended, the accepted stop intent is
            // authoritative for this exact run, including that race.
            let current = store::load(&watchdog_state, &watchdog_key)?;
            if current.active_run(&watchdog_run) && current.stopping {
                return Ok(TurnResult{text:String::new(),cancelled:true});
            }
            let completed = completed.context("acp_turn_timeout")?;
            if completed.is_err() && startup_missing.load(Ordering::Acquire) {
                if let Some(id) = &prior.native_session_id { mark_context_lost(&watchdog_state, &watchdog_key, &watchdog_run, id); }
            }
            completed.map_err(|_|anyhow::anyhow!("acp_turn_failed"))?;
        }
        stopped=hard_stop=>{stopped?;return Ok(TurnResult{text:String::new(),cancelled:true});}
    }
    drop(question_tool);
    let outcome = result.lock().unwrap().take().context("acp_missing_result");
    outcome
}

pub(super) async fn await_answer_id(
    state: &DaemonState,
    key: &str,
    run: &str,
    request: Value,
    native_request_id: String,
) -> Result<Value> {
    super::questions::validate_schema(&request)?;
    let now = current_time_millis()?;
    let id = format!(
        "question-{}",
        store::session_key(key, run, &native_request_id)
    );
    store::mutate(state, key, None, |s| {
        if !s.active_run(run)
            || s.stopping
            || request["sessionId"].as_str() != s.native_session_id.as_deref()
        {
            bail!("stale_question");
        }
        if let Some(prior) = s.questions.iter().find(|q| q.id == id) {
            if prior.request != request {
                bail!("question_id_conflict")
            }
            return Ok(());
        }
        if s.questions.iter().filter(|q| q.pending()).count() >= 8 || s.questions.len() >= 100 {
            bail!("too_many_questions");
        }
        s.questions.push(Question {
            id: id.clone(),
            run_id: run.to_string(),
            expires_at_ms: now + 15 * 60 * 1000,
            interaction: Some(super::questions::QuestionInteraction::new(
                &request,
                native_request_id.starts_with("mcp:"),
            )?),
            request,
            response: None,
            end_reason: None,
        });
        Ok(())
    })?;
    loop {
        let s = store::load(state, key)?;
        let q = s
            .questions
            .iter()
            .find(|q| q.id == id)
            .context("stale_question")?;
        if let Some(answer) = &q.response {
            return Ok(answer.clone());
        }
        if !s.active_run(run) || s.stopping || q.end_reason.is_some() {
            bail!("question_closed");
        }
        if current_time_millis()? >= q.expires_at_ms {
            let answer = store::mutate(state, key, None, |s| {
                let q = s
                    .questions
                    .iter_mut()
                    .find(|q| q.id == id)
                    .context("stale_question")?;
                super::questions::expire_or_answer(q, current_time_millis()?)
            })?;
            if let Some(answer) = answer {
                return Ok(answer);
            }
            bail!("question_expired");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(all(test, unix))]
#[path = "client_tests.rs"]
mod tests;
