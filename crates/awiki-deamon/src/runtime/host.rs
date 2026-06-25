use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cli_wrapper::CliWrapperRequest;
use crate::controller_scope::VerifiedControllerSender;
use crate::inbox::{
    route_controller_text_task, route_controller_text_task_with_verified_sender,
    ControllerTextMessage,
};
use crate::local_rpc::execute_runtime_rpc_request_with_outbox;
use crate::outbox::{
    RuntimeMessageSecurity, RuntimeMessageSend, RuntimeMessageTarget, RuntimeOutbox,
};
use crate::plugins::hermes::HermesRuntimeEventKind;
use crate::runtime::{
    runtime_task_matches_profile_controller_scope, GenericCliRouteSession, RuntimeAgentProfile,
    RuntimeInvocationAuthority, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRun, RuntimeRunStatus, RuntimeTask,
};
use crate::security::runtime_token::{
    current_time_millis, issue_runtime_token, RpcMethod, RuntimeTokenScope,
    ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE, ANY_DIRECT_RECIPIENT_SCOPE, ANY_GROUP_RECIPIENT_SCOPE,
};
use crate::state::{
    AuthorizedRuntimeContext, CliDriverRunRecord, DaemonState, RuntimeFinalOutboxRecord,
};
use crate::workspace::{
    prepare_workspace_instance, WorkspaceBindingConfig, WorkspaceInstance, WorkspaceMode,
};
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecipientPolicy {
    allowed_recipients: Vec<String>,
    allowed_message_security: Vec<String>,
}

impl RecipientPolicy {
    fn controller_only(controller_did: &str) -> Self {
        Self {
            allowed_recipients: vec![controller_did.to_string()],
            allowed_message_security: vec!["default_plain".to_string()],
        }
    }

    fn hermes_default(controller_did: &str) -> Self {
        Self {
            allowed_recipients: vec![
                controller_did.to_string(),
                ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE.to_string(),
                ANY_DIRECT_RECIPIENT_SCOPE.to_string(),
                ANY_GROUP_RECIPIENT_SCOPE.to_string(),
            ],
            allowed_message_security: vec!["default_plain".to_string()],
        }
    }

    fn app_message_handler(user_did: &str) -> Self {
        Self {
            allowed_recipients: vec![user_did.to_string()],
            allowed_message_security: vec!["default_plain".to_string()],
        }
    }

    fn from_json(value: &Value, controller_did: &str) -> Result<Self> {
        let Some(object) = value.as_object() else {
            anyhow::bail!("recipient_policy_json must be a JSON object");
        };
        let allow_controller = object
            .get("allow_controller")
            .and_then(Value::as_bool)
            .or_else(|| {
                object
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(|mode| mode == "controller-only")
            })
            .unwrap_or(false);
        let mut allowed_recipients = Vec::new();
        if allow_controller {
            allowed_recipients.push(controller_did.to_string());
        }
        collect_string_array(object.get("allowed_dids"), &mut allowed_recipients)?;
        collect_string_array(object.get("allowed_handles"), &mut allowed_recipients)?;
        collect_string_array(object.get("allow"), &mut allowed_recipients)?;
        let mut allowed_message_security = Vec::new();
        collect_string_array(
            object.get("allowed_security"),
            &mut allowed_message_security,
        )?;
        if allowed_message_security.is_empty() {
            allowed_message_security.push("default_plain".to_string());
        }
        if allowed_recipients.is_empty() {
            anyhow::bail!("recipient policy must allow at least one recipient");
        }
        Ok(Self {
            allowed_recipients,
            allowed_message_security,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTaskRunResult {
    pub run: RuntimeRun,
    pub launch_outcome: RuntimeLaunchOutcome,
    pub token_id: String,
}

pub fn run_controller_text_task<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_controller_text_task_with_socket(state, profile, plugin, outbox, message, None, None)
}

pub fn run_controller_text_task_with_config<P, O>(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_controller_text_task_with_socket(
        state,
        profile,
        plugin,
        outbox,
        message,
        Some(config.local_socket_path.clone()),
        Some(config.runtime_temp_dir.clone()),
    )
}

pub fn run_controller_text_task_with_verified_sender_config<P, O>(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    verified_sender: &VerifiedControllerSender,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_controller_text_task_with_verified_sender_socket(
        state,
        profile,
        verified_sender,
        plugin,
        outbox,
        message,
        Some(config.local_socket_path.clone()),
        Some(config.runtime_temp_dir.clone()),
    )
}

fn run_controller_text_task_with_socket<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
    local_socket_path: Option<std::path::PathBuf>,
    runtime_temp_dir: Option<std::path::PathBuf>,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    profile.validate()?;
    let task = route_controller_text_task(profile, message)?;
    let run_id = format!("run_{}", task.task_id);
    run_existing_runtime_task_with_socket(
        state,
        profile,
        plugin,
        outbox,
        task,
        run_id,
        local_socket_path,
        runtime_temp_dir,
    )
}

fn run_controller_text_task_with_verified_sender_socket<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    verified_sender: &VerifiedControllerSender,
    plugin: &P,
    outbox: &O,
    message: ControllerTextMessage,
    local_socket_path: Option<std::path::PathBuf>,
    runtime_temp_dir: Option<std::path::PathBuf>,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    profile.validate()?;
    let task = route_controller_text_task_with_verified_sender(profile, verified_sender, message)?;
    let run_id = format!("run_{}", task.task_id);
    run_existing_runtime_task_with_socket(
        state,
        profile,
        plugin,
        outbox,
        task,
        run_id,
        local_socket_path,
        runtime_temp_dir,
    )
}

pub fn run_existing_runtime_task_with_config<P, O>(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    task: RuntimeTask,
    run_id: impl Into<String>,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    run_existing_runtime_task_with_socket(
        state,
        profile,
        plugin,
        outbox,
        task,
        run_id.into(),
        Some(config.local_socket_path.clone()),
        Some(config.runtime_temp_dir.clone()),
    )
}

fn run_existing_runtime_task_with_socket<P, O>(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    plugin: &P,
    outbox: &O,
    task: RuntimeTask,
    run_id: String,
    local_socket_path: Option<std::path::PathBuf>,
    runtime_temp_dir: Option<std::path::PathBuf>,
) -> Result<RuntimeTaskRunResult>
where
    P: RuntimePlugin,
    O: RuntimeOutbox,
{
    profile.validate()?;
    task.validate()?;
    if !runtime_task_matches_profile_controller_scope(&task, profile) {
        anyhow::bail!("runtime task does not match profile controller scope");
    }
    if run_id.trim().is_empty() {
        anyhow::bail!("run_id must not be empty");
    }

    let run = RuntimeRun {
        run_id,
        task_id: task.task_id.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        workspace_id: profile.workspace_id.clone(),
        status: RuntimeRunStatus::Pending,
    };
    if let Some(existing) = existing_runtime_run(state, &run)? {
        return Ok(existing);
    }
    state.upsert_runtime_agent_profile(profile)?;
    state.insert_runtime_task(&task)?;
    if !state.try_insert_runtime_run(&run)? {
        if let Some(existing) = existing_runtime_run(state, &run)? {
            return Ok(existing);
        }
    }
    let task_conversation_id = task.conversation_id.clone();
    let task_controller_did = task.controller_did.clone();
    let task_reply_recipient_did = task.reply_recipient_did.clone();
    let recipient_policy = runtime_recipient_policy(
        state,
        profile,
        &task_reply_recipient_did,
        task.invocation_authority,
    )?;
    let allowed_methods = runtime_allowed_methods(task.invocation_authority);
    let mut scope = RuntimeTokenScope::new(
        profile.agent_did.clone(),
        profile.runtime_profile_id.clone(),
        run.run_id.clone(),
        allowed_methods,
        Some(recipient_policy.allowed_recipients),
        Duration::from_secs(5 * 60),
    )?;
    scope.allowed_message_security = Some(recipient_policy.allowed_message_security);
    let issued = issue_runtime_token(scope)?;
    state.store_runtime_token(&issued)?;

    let install_status = match plugin.check_install_status() {
        Ok(status) => status,
        Err(error) => {
            state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
            let error_summary = error.to_string();
            if plugin.plugin_id() == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
                let (error_code, error_summary) = hermes_launch_error_detail(&error_summary);
                emit_hermes_failure_outputs(
                    state,
                    outbox,
                    profile,
                    &task_reply_recipient_did,
                    &run,
                    error_code,
                    error_summary.as_str(),
                )?;
            } else {
                emit_runtime_setup_failed_status(
                    outbox,
                    &run,
                    "runtime_install_probe_failed",
                    &error_summary,
                )?;
            }
            return Err(error).context("check runtime plugin install status");
        }
    };
    if !install_status.installed {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
        if plugin.plugin_id() == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
            let detail = install_status
                .detail
                .as_deref()
                .unwrap_or("Hermes gateway command is not configured");
            let (error_code, error_summary) = hermes_launch_error_detail(detail);
            emit_hermes_failure_outputs(
                state,
                outbox,
                profile,
                &task_reply_recipient_did,
                &run,
                error_code,
                error_summary.as_str(),
            )?;
        } else {
            let detail = install_status
                .detail
                .as_deref()
                .unwrap_or("runtime plugin is not installed");
            emit_runtime_setup_failed_status(outbox, &run, "runtime_not_installed", detail)?;
        }
        anyhow::bail!("runtime plugin {} is not installed", plugin.plugin_id());
    }
    let cli_route_session = if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        Some(prepare_generic_cli_route_session(
            state,
            profile,
            &task_conversation_id,
            runtime_temp_dir.as_ref(),
        )?)
    } else {
        None
    };
    let workspace_instance_key = cli_route_session
        .as_ref()
        .filter(|_| profile.workspace_mode == Some(WorkspaceMode::RouteRoot))
        .map(|session| session.route_key_hash.as_str())
        .unwrap_or(run.run_id.as_str());
    let workspace_instance = match (
        profile.workspace_id.as_ref(),
        profile.workspace_root.as_ref(),
        profile.workspace_mode,
        local_socket_path.is_some(),
        plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID,
    ) {
        (Some(workspace_id), Some(workspace_root), Some(workspace_mode), true, true) => Some(
            prepare_workspace_instance(
                runtime_temp_dir
                    .as_ref()
                    .context("runtime temp dir is unavailable without daemon config")?,
                &WorkspaceBindingConfig {
                    workspace_id: workspace_id.clone(),
                    workspace_root: workspace_root.clone(),
                    workspace_mode,
                },
                workspace_instance_key,
            )
            .context("prepare runtime workspace instance")?,
        ),
        _ => None,
    };

    let launch_context = RuntimeLaunchContext {
        run: run.clone(),
        task,
        workspace_root: profile.workspace_root.clone(),
        workspace_instance: workspace_instance.clone(),
        cli_route_session: cli_route_session.clone(),
        runtime_temp_dir,
        runtime_rpc_token: issued.token.clone(),
        local_socket_path,
    };
    let launch_outcome = match plugin.launch_run(launch_context) {
        Ok(outcome) => outcome,
        Err(error) => {
            state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
            if plugin.plugin_id() == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
                let (error_code, error_summary) = hermes_launch_error_detail(&error.to_string());
                emit_hermes_failure_outputs(
                    state,
                    outbox,
                    profile,
                    &task_reply_recipient_did,
                    &run,
                    error_code,
                    error_summary.as_str(),
                )?;
            } else {
                emit_runtime_launch_failed_status(outbox, &run, &error.to_string())?;
            }
            return Err(error).context("launch runtime run");
        }
    };
    for callback in launch_outcome.callbacks.iter().cloned() {
        if let Err(error) = execute_runtime_rpc_request_with_outbox(state, outbox, callback) {
            let error_summary = error.root_cause().to_string();
            mark_runtime_run_failed_with_status(
                state,
                outbox,
                &run,
                "Runtime callback failed",
                "runtime_callback_failed",
                &error_summary,
            )?;
            return Err(error).context("apply runtime callback");
        }
    }
    mark_pending_run_as_running(state, outbox, &run)?;

    if plugin.plugin_id() == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
        let hermes_failed = hermes_has_error(&launch_outcome.metadata)?;
        if hermes_failed {
            let error = hermes_structured_error(&launch_outcome.metadata);
            let error_code = error
                .as_ref()
                .map(|error| error.0.clone())
                .unwrap_or_else(|| "hermes_error".to_string());
            let error_summary = error
                .as_ref()
                .map(|error| error.1.clone())
                .or_else(|| hermes_error_summary(&launch_outcome.metadata))
                .unwrap_or_else(|| "Hermes run failed".to_string());
            emit_hermes_failure_outputs(
                state,
                outbox,
                profile,
                &task_reply_recipient_did,
                &run,
                &error_code,
                &error_summary,
            )?;
        } else {
            let final_text = hermes_final_text(&launch_outcome.metadata)?;
            if let Some(final_text) = final_text.as_deref() {
                let final_record = runtime_final_outbox_record(
                    profile,
                    &task_controller_did,
                    &task_reply_recipient_did,
                    &run,
                    task_conversation_id.as_deref(),
                    final_text,
                )?;
                state.upsert_runtime_final_outbox_pending(&final_record)?;
                flush_runtime_final_outbox(state, outbox, 8)
                    .context("send Hermes final text as runtime message")?;
                let refreshed = state
                    .load_runtime_final_outbox_by_run(&run.run_id)?
                    .context("Hermes final outbox record missing after flush")?;
                if refreshed.status != "sent" {
                    emit_runtime_status(
                        outbox,
                        &run,
                        "running",
                        Some("Hermes response is ready; delivery is retrying"),
                        refreshed.last_error_code.as_deref(),
                        refreshed.last_error_summary.as_deref(),
                    )?;
                }
            } else {
                let error_summary = "Hermes run completed without final text";
                emit_hermes_failure_outputs(
                    state,
                    outbox,
                    profile,
                    &task_reply_recipient_did,
                    &run,
                    "final_text_missing",
                    error_summary,
                )?;
                anyhow::bail!("{error_summary}");
            }
        }
    }

    let fallback_final_source = if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID
    {
        match apply_generic_cli_final_fallback(
            state,
            outbox,
            &run,
            &launch_outcome,
            issued.token.as_str(),
        ) {
            Ok(source) => source,
            Err(error) => {
                let error_summary = error.root_cause().to_string();
                mark_runtime_run_failed_with_status(
                    state,
                    outbox,
                    &run,
                    "Runtime final fallback failed",
                    "runtime_fallback_final_failed",
                    &error_summary,
                )?;
                return Err(error).context("apply generic CLI final fallback");
            }
        }
    } else {
        None
    };
    apply_terminal_failure_fallback(state, outbox, &run, &launch_outcome)?;
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        persist_cli_driver_run(
            state,
            profile,
            &run,
            &launch_outcome,
            workspace_instance.as_ref(),
            fallback_final_source.as_deref(),
            cli_route_session.as_ref(),
        )?;
    }

    Ok(RuntimeTaskRunResult {
        run: state.load_runtime_run(&run.run_id)?,
        launch_outcome,
        token_id: issued.token_id,
    })
}

fn existing_runtime_run(
    state: &DaemonState,
    expected: &RuntimeRun,
) -> Result<Option<RuntimeTaskRunResult>> {
    let existing = match state.load_runtime_run(&expected.run_id) {
        Ok(run) => run,
        Err(_) => return Ok(None),
    };
    if existing.task_id != expected.task_id
        || existing.agent_did != expected.agent_did
        || existing.runtime_profile_id != expected.runtime_profile_id
        || existing.runtime_plugin_id != expected.runtime_plugin_id
        || existing.workspace_id != expected.workspace_id
    {
        anyhow::bail!(
            "runtime run id collision for {} does not match expected binding",
            expected.run_id
        );
    }
    Ok(Some(RuntimeTaskRunResult {
        launch_outcome: RuntimeLaunchOutcome {
            run_id: existing.run_id.clone(),
            status: existing.status.clone(),
            exit_code: None,
            callbacks: Vec::new(),
            metadata: serde_json::json!({
                "deduplicated": true,
                "reason": "runtime_run_already_exists",
            }),
        },
        run: existing,
        token_id: String::new(),
    }))
}

pub fn flush_runtime_final_outbox(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    limit: usize,
) -> Result<usize> {
    let now = current_time_millis()?;
    state.recover_stale_runtime_final_outbox_sending(
        now - RUNTIME_FINAL_OUTBOX_SENDING_STALE_MS,
        now,
    )?;
    let records = state.list_due_runtime_final_outbox(now, limit)?;
    let mut sent_count = 0;
    for record in records {
        if record.status != "pending" {
            continue;
        }
        if !state.mark_runtime_final_outbox_sending(&record.idempotency_key)? {
            continue;
        }
        let context = AuthorizedRuntimeContext {
            token_id: "host-runtime-final-outbox".to_string(),
            agent_did: record.agent_did.clone(),
            runtime_profile_id: record.runtime_profile_id.clone(),
            run_id: record.run_id.clone(),
            method: RpcMethod::MsgSend,
        };
        let security = RuntimeMessageSecurity::parse(Some(record.security.as_str()))?;
        let message = RuntimeMessageSend {
            target: runtime_final_message_target(&record)?,
            text: record.final_text.clone(),
            payload: runtime_final_payload(state, &record)?,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: Some(record.idempotency_key.clone()),
            security,
        };
        match outbox.send_message(&context, &message) {
            Ok(result) => {
                mark_runtime_final_delivered(state, outbox, &record, result.message_id.as_deref())?;
                state.insert_audit_event_json(
                    "runtime.final_outbox.sent",
                    Some(&record.agent_did),
                    Some(&record.runtime_profile_id),
                    Some(&record.run_id),
                    None,
                    serde_json::json!({
                        "idempotency_key": record.idempotency_key,
                        "message_id": result.message_id,
                        "attempt_count": record.attempt_count + 1,
                    }),
                )?;
                sent_count += 1;
            }
            Err(error) => {
                let error_summary = sanitize_user_visible_error_summary(&error.to_string());
                let attempts = record.attempt_count + 1;
                if attempts >= MAX_RUNTIME_FINAL_OUTBOX_ATTEMPTS {
                    state.mark_runtime_final_outbox_failed_terminal(
                        &record.idempotency_key,
                        "final_delivery_failed",
                        &error_summary,
                    )?;
                    let run = state.load_runtime_run(&record.run_id)?;
                    mark_runtime_run_failed_with_status(
                        state,
                        outbox,
                        &run,
                        "Hermes response delivery failed",
                        "final_delivery_failed",
                        &error_summary,
                    )?;
                    state.insert_audit_event_json(
                        "runtime.final_outbox.failed_terminal",
                        Some(&record.agent_did),
                        Some(&record.runtime_profile_id),
                        Some(&record.run_id),
                        None,
                        serde_json::json!({
                            "idempotency_key": record.idempotency_key,
                            "attempt_count": attempts,
                            "reason": error_summary,
                        }),
                    )?;
                } else {
                    let next_attempt_at_ms = now + runtime_final_retry_delay_ms(attempts);
                    state.mark_runtime_final_outbox_retry(
                        &record.idempotency_key,
                        next_attempt_at_ms,
                        "final_delivery_retry",
                        &error_summary,
                    )?;
                    state.insert_audit_event_json(
                        "runtime.final_outbox.retry_scheduled",
                        Some(&record.agent_did),
                        Some(&record.runtime_profile_id),
                        Some(&record.run_id),
                        None,
                        serde_json::json!({
                            "idempotency_key": record.idempotency_key,
                            "attempt_count": attempts,
                            "next_attempt_at_ms": next_attempt_at_ms,
                            "reason": error_summary,
                        }),
                    )?;
                }
            }
        }
    }
    Ok(sent_count)
}

fn runtime_final_message_target(record: &RuntimeFinalOutboxRecord) -> Result<RuntimeMessageTarget> {
    if let Some(group_did) = record
        .conversation_id
        .as_deref()
        .and_then(group_did_from_conversation_id)
    {
        return Ok(RuntimeMessageTarget::Group {
            group: group_did.to_string(),
        });
    }
    Ok(RuntimeMessageTarget::Direct {
        recipient: record.recipient_did.clone(),
        raw_recipient: record.recipient_did.clone(),
        resolved_did: Some(record.recipient_did.clone()),
    })
}

fn runtime_final_payload(
    state: &DaemonState,
    record: &RuntimeFinalOutboxRecord,
) -> Result<Option<serde_json::Value>> {
    let Some(_) = record
        .conversation_id
        .as_deref()
        .and_then(group_did_from_conversation_id)
    else {
        return Ok(None);
    };
    let task = match state.load_runtime_task_for_run(&record.run_id) {
        Ok(task) => task,
        Err(_) => return Ok(None),
    };
    let payload = match serde_json::from_str::<serde_json::Value>(&task.text) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };
    if payload.get("mention_context").is_none() {
        return Ok(None);
    }
    let Some(sender_did) = payload
        .get("source_sender_did")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let mention_surface = mention_surface_for_sender(&payload, sender_did);
    let text = format!("{} {}", mention_surface, record.final_text.trim());
    let mention_end = mention_surface.chars().count();
    Ok(Some(serde_json::json!({
        "text": text,
        "mentions": [{
            "id": format!("reply_{}", stable_id_suffix(&format!("{}:{}", record.run_id, sender_did))),
            "range": {
                "start": 0,
                "end": mention_end,
                "unit": "unicode_code_point"
            },
            "target": {
                "kind": "human",
                "did": sender_did,
                "display_name": mention_surface.trim_start_matches('@')
            },
            "mention_role": "addressee"
        }],
        "annotations": {
            "awiki_reply_to_message_id": payload.get("source_message_id").cloned().unwrap_or(serde_json::Value::Null),
            "awiki_reply_from_agent_did": record.agent_did
        }
    })))
}

fn mention_surface_for_sender(payload: &serde_json::Value, sender_did: &str) -> String {
    if let Some(handle) = payload
        .get("source_sender_full_handle")
        .and_then(serde_json::Value::as_str)
        .and_then(short_handle)
    {
        return format!("@{handle}");
    }
    if let Some(handle) = short_handle_from_wba_did(sender_did) {
        return format!("@{handle}");
    }
    let compact = if sender_did.len() <= 18 {
        sender_did.to_string()
    } else {
        format!(
            "{}…{}",
            &sender_did[..10],
            &sender_did[sender_did.len().saturating_sub(6)..]
        )
    };
    format!("@{compact}")
}

fn short_handle(value: &str) -> Option<String> {
    let mut trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("did:") {
        return None;
    }
    while let Some(rest) = trimmed.strip_prefix('@') {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("wba://") {
        trimmed = rest.trim_start();
    }
    let handle = match trimmed.find('.') {
        Some(index) if index > 0 => &trimmed[..index],
        _ => trimmed,
    }
    .trim();
    if handle.is_empty() {
        None
    } else {
        Some(handle.to_string())
    }
}

fn short_handle_from_wba_did(did: &str) -> Option<String> {
    let parts = did.trim().split(':').collect::<Vec<_>>();
    if parts.len() >= 6 && parts[0] == "did" && parts[1] == "wba" {
        if parts[3] == "user" {
            return short_handle(parts[4]);
        }
        if parts[3] == "agent" && parts.len() >= 7 {
            return short_handle(parts[5]);
        }
        return short_handle(parts[3]);
    }
    None
}

fn group_did_from_conversation_id(conversation_id: &str) -> Option<&str> {
    conversation_id
        .trim()
        .strip_prefix("group:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn stable_id_suffix(input: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn mark_runtime_final_delivered(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    record: &RuntimeFinalOutboxRecord,
    message_id: Option<&str>,
) -> Result<()> {
    state.mark_runtime_final_outbox_sent(&record.idempotency_key, message_id)?;
    let run = state.load_runtime_run(&record.run_id)?;
    let updated = state
        .update_runtime_run_status_if_status_in(
            &record.run_id,
            &[RuntimeRunStatus::Pending, RuntimeRunStatus::Running],
            RuntimeRunStatus::Finished,
        )
        .context("mark Hermes run finished after final delivery")?;
    if updated {
        emit_runtime_status(
            outbox,
            &run,
            "succeeded",
            Some("Hermes response sent"),
            None,
            None,
        )?;
    }
    Ok(())
}

fn runtime_final_outbox_record(
    profile: &RuntimeAgentProfile,
    controller_did: &str,
    recipient_did: &str,
    run: &RuntimeRun,
    conversation_id: Option<&str>,
    final_text: &str,
) -> Result<RuntimeFinalOutboxRecord> {
    let final_text = final_text.trim();
    if final_text.is_empty() {
        anyhow::bail!("Hermes final text is empty");
    }
    let now = current_time_millis()?;
    Ok(RuntimeFinalOutboxRecord {
        idempotency_key: runtime_final_idempotency_key(
            &profile.agent_did,
            &run.run_id,
            &profile.controller_scope_key,
        ),
        run_id: run.run_id.clone(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: controller_did.to_string(),
        recipient_did: recipient_did.to_string(),
        conversation_id: conversation_id.map(str::to_string),
        final_text: final_text.to_string(),
        security: "default_plain".to_string(),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: now,
        last_error_code: None,
        last_error_summary: None,
        message_id: None,
        created_at_ms: now,
        updated_at_ms: now,
        sent_at_ms: None,
    })
}

fn runtime_final_idempotency_key(
    runtime_agent_did: &str,
    run_id: &str,
    controller_scope_key: &str,
) -> String {
    format!("runtime-final:{runtime_agent_did}:{run_id}:{controller_scope_key}")
}

const MAX_RUNTIME_FINAL_OUTBOX_ATTEMPTS: i64 = 5;
const RUNTIME_FINAL_OUTBOX_SENDING_STALE_MS: i64 = 5 * 60 * 1000;

fn runtime_final_retry_delay_ms(attempts: i64) -> i64 {
    match attempts {
        0 | 1 => 10_000,
        2 => 30_000,
        3 => 120_000,
        4 => 300_000,
        _ => 900_000,
    }
}

fn hermes_launch_error_detail(error: &str) -> (&'static str, String) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("awiki_hermes_gateway_cmd is required")
        || lower.contains("hermes gateway command is not configured")
        || lower.contains("awiki_hermes_bin is not set")
    {
        return (
            "gateway_command_missing",
            "Hermes gateway command is not configured".to_string(),
        );
    }
    if lower.contains("spawn awiki_hermes_gateway_cmd")
        || lower.contains("awiki_hermes_gateway_cmd has an unclosed quote")
        || lower.contains("awiki_hermes_gateway_cmd must not be empty")
        || lower.contains("gateway_command_unavailable")
    {
        return (
            "gateway_command_unavailable",
            "Hermes gateway command is unavailable".to_string(),
        );
    }
    if lower.contains("gateway.ready")
        || lower.contains("gateway not ready")
        || lower.contains("gateway timed out")
        || lower.contains("gateway exited")
    {
        return (
            "gateway_not_ready",
            "Hermes gateway did not become ready".to_string(),
        );
    }
    ("launch_failed", error.to_string())
}

fn emit_hermes_failure_outputs(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    profile: &RuntimeAgentProfile,
    controller_did: &str,
    run: &RuntimeRun,
    error_code: &str,
    error_summary: &str,
) -> Result<()> {
    let sanitized = sanitize_user_visible_error_summary(error_summary);
    let context = runtime_output_context(state, profile, run, RpcMethod::MsgSend)?;
    let failure_text = format!("Hermes 运行失败：{sanitized}");
    let _ = outbox.send_message(
        &context,
        &crate::outbox::RuntimeMessageSend {
            target: crate::outbox::RuntimeMessageTarget::Direct {
                recipient: controller_did.to_string(),
                raw_recipient: controller_did.to_string(),
                resolved_did: Some(controller_did.to_string()),
            },
            text: failure_text,
            payload: None,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: None,
            security: RuntimeMessageSecurity::DefaultPlain,
        },
    );
    if mark_active_runtime_run_failed(state, &run.run_id)? {
        emit_runtime_status(
            outbox,
            run,
            "failed",
            Some("Hermes run failed"),
            Some(error_code),
            Some(&sanitized),
        )?;
    }
    Ok(())
}

fn mark_pending_run_as_running(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
) -> Result<()> {
    if state.update_runtime_run_status_if_status_in(
        &run.run_id,
        &[RuntimeRunStatus::Pending],
        RuntimeRunStatus::Running,
    )? {
        emit_runtime_status(outbox, run, "running", Some("Runtime started"), None, None)?;
    }
    Ok(())
}

fn emit_runtime_setup_failed_status(
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    error_code: &str,
    error_summary: &str,
) -> Result<()> {
    emit_runtime_status(
        outbox,
        run,
        "failed",
        Some("Runtime setup failed"),
        Some(error_code),
        Some(&sanitize_user_visible_error_summary(error_summary)),
    )
}

fn emit_runtime_launch_failed_status(
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    error_summary: &str,
) -> Result<()> {
    emit_runtime_status(
        outbox,
        run,
        "failed",
        Some("Runtime launch failed"),
        Some("runtime_launch_failed"),
        Some(&sanitize_user_visible_error_summary(error_summary)),
    )
}

fn mark_runtime_run_failed_with_status(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    message: &str,
    error_code: &str,
    error_summary: &str,
) -> Result<()> {
    if mark_active_runtime_run_failed(state, &run.run_id)? {
        emit_runtime_status(
            outbox,
            run,
            "failed",
            Some(message),
            Some(error_code),
            Some(&sanitize_user_visible_error_summary(error_summary)),
        )?;
    }
    Ok(())
}

fn apply_generic_cli_final_fallback(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    launch_outcome: &RuntimeLaunchOutcome,
    runtime_rpc_token: &str,
) -> Result<Option<&'static str>> {
    let stored_status = state.load_runtime_run(&run.run_id)?.status;
    if launch_outcome.status != RuntimeRunStatus::Finished
        || !matches!(
            stored_status,
            RuntimeRunStatus::Pending | RuntimeRunStatus::Running
        )
    {
        return Ok(None);
    }
    let Some(final_text) = fallback_final_text(&launch_outcome.metadata)? else {
        return Ok(None);
    };
    let response = execute_runtime_rpc_request_with_outbox(
        state,
        outbox,
        CliWrapperRequest::task_finish(
            runtime_rpc_token.to_string(),
            run.task_id.clone(),
            final_text,
        )
        .into_rpc_request(),
    )
    .context("apply fallback final from generic CLI driver output")?;
    if response.ok {
        Ok(Some("generic_cli_final_output"))
    } else {
        let error_summary = response
            .error
            .as_ref()
            .map(|error| format!("{}: {}", error.code, error.message))
            .unwrap_or_else(|| "runtime RPC rejected fallback final".to_string());
        anyhow::bail!("runtime RPC rejected fallback final: {error_summary}")
    }
}

fn apply_terminal_failure_fallback(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    launch_outcome: &RuntimeLaunchOutcome,
) -> Result<()> {
    if launch_outcome.status != RuntimeRunStatus::Failed {
        return Ok(());
    }
    if mark_active_runtime_run_failed(state, &run.run_id)? {
        emit_runtime_status(
            outbox,
            run,
            "failed",
            Some("Runtime failed"),
            Some("runtime_failed"),
            Some(&runtime_launch_failure_summary(launch_outcome)),
        )?;
    }
    Ok(())
}

fn mark_active_runtime_run_failed(state: &DaemonState, run_id: &str) -> Result<bool> {
    state.update_runtime_run_status_if_status_in(
        run_id,
        &[RuntimeRunStatus::Pending, RuntimeRunStatus::Running],
        RuntimeRunStatus::Failed,
    )
}

fn emit_runtime_status(
    outbox: &impl RuntimeOutbox,
    run: &RuntimeRun,
    status: &str,
    message: Option<&str>,
    last_error_code: Option<&str>,
    last_error_summary: Option<&str>,
) -> Result<()> {
    let context = crate::state::AuthorizedRuntimeContext {
        token_id: "host-run-status".to_string(),
        agent_did: run.agent_did.clone(),
        runtime_profile_id: run.runtime_profile_id.clone(),
        run_id: run.run_id.clone(),
        method: RpcMethod::TaskStatus,
    };
    outbox.send_status_with_detail(
        &context,
        status,
        message,
        last_error_code,
        last_error_summary,
    )?;
    Ok(())
}

fn runtime_launch_failure_summary(launch_outcome: &RuntimeLaunchOutcome) -> String {
    launch_outcome
        .exit_code
        .map(|code| format!("Runtime exited with status {code}"))
        .unwrap_or_else(|| "Runtime failed".to_string())
}

fn runtime_output_context(
    _state: &DaemonState,
    profile: &RuntimeAgentProfile,
    run: &RuntimeRun,
    method: RpcMethod,
) -> Result<crate::state::AuthorizedRuntimeContext> {
    Ok(crate::state::AuthorizedRuntimeContext {
        token_id: "host-runtime-output".to_string(),
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        run_id: run.run_id.clone(),
        method,
    })
}

fn sanitize_user_visible_error_summary(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
                || lower.contains("bearer")
            {
                "<redacted>"
            } else if part.starts_with('/') || part.starts_with("file://") {
                "<path>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.trim().is_empty() {
        sanitized = "Hermes run failed".to_string();
    }
    if sanitized.chars().count() > 160 {
        sanitized = sanitized.chars().take(160).collect();
    }
    sanitized
}

fn hermes_error_summary(metadata: &Value) -> Option<String> {
    metadata
        .get("events")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .find_map(|event| {
            let is_error = event
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    kind == serde_json::to_value(HermesRuntimeEventKind::Error)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_string))
                        .as_deref()
                        .unwrap_or("error")
                });
            if !is_error {
                return None;
            }
            event
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
}

fn hermes_structured_error(metadata: &Value) -> Option<(String, String)> {
    let error = metadata.get("error")?;
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty())?;
    let summary = error
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .unwrap_or("Hermes run failed");
    Some((code.to_string(), summary.to_string()))
}

fn hermes_has_error(metadata: &Value) -> Result<bool> {
    if metadata.get("error").is_some_and(|value| !value.is_null()) {
        return Ok(true);
    }
    let Some(events) = metadata.get("events").and_then(Value::as_array) else {
        return Ok(false);
    };
    let error_kind = serde_json::to_value(HermesRuntimeEventKind::Error)?
        .as_str()
        .unwrap_or("error")
        .to_string();
    Ok(events.iter().any(|event| {
        event
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == error_kind)
    }))
}

fn hermes_final_text(metadata: &Value) -> Result<Option<String>> {
    if let Some(text) = metadata
        .get("final_text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
    {
        return Ok(Some(text));
    }
    let Some(events) = metadata.get("events").and_then(Value::as_array) else {
        return Ok(None);
    };
    for event in events.iter().rev() {
        let Some(kind) = event.get("kind").and_then(Value::as_str) else {
            continue;
        };
        if kind
            != serde_json::to_value(HermesRuntimeEventKind::MessageComplete)?
                .as_str()
                .unwrap_or("message_complete")
        {
            continue;
        }
        let text = event
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        if text.is_some() {
            return Ok(text);
        }
    }
    Ok(None)
}

fn persist_cli_driver_run(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    run: &RuntimeRun,
    launch_outcome: &RuntimeLaunchOutcome,
    workspace_instance: Option<&WorkspaceInstance>,
    fallback_final_source: Option<&str>,
    route_session: Option<&GenericCliRouteSession>,
) -> Result<()> {
    let task = state.load_runtime_task(&run.task_id)?;
    let Some(driver_id) = state
        .load_cli_runtime_profile(&profile.runtime_profile_id)
        .map(|profile| profile.driver_id)
        .or_else(|_| {
            launch_outcome
                .metadata
                .get("driver_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .context("generic-cli run metadata does not include driver_id")
        })
        .ok()
    else {
        return Ok(());
    };
    let output_json = launch_outcome
        .metadata
        .get("output")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let command_json = launch_outcome
        .metadata
        .get("command")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let native_session_id = launch_outcome
        .metadata
        .get("native_session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| route_session.and_then(|session| session.native_session_id.clone()));
    let route_key = route_session
        .map(|session| session.route_key.clone())
        .unwrap_or_else(|| {
            generic_cli_route_key(
                &profile.agent_handle,
                &profile.controller_scope_key,
                task.conversation_id.as_deref(),
            )
        });
    let synthetic_session_id = route_session
        .and_then(|session| session.synthetic_session_id.clone())
        .or_else(|| Some(route_key.clone()));
    state.upsert_cli_driver_run(&CliDriverRunRecord {
        run_id: run.run_id.clone(),
        agent_did: run.agent_did.clone(),
        runtime_profile_id: run.runtime_profile_id.clone(),
        driver_id,
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: task.controller_did.clone(),
        conversation_id: task.conversation_id.clone(),
        route_key,
        workspace_id: profile.workspace_id.clone(),
        workspace_root: workspace_instance
            .map(|instance| instance.workspace_root.clone())
            .or_else(|| canonicalize_optional_path(profile.workspace_root.as_ref())),
        workspace_instance_path: workspace_instance
            .map(|instance| instance.workspace_instance_path.clone())
            .or_else(|| canonicalize_optional_path(profile.workspace_root.as_ref())),
        workspace_mode: workspace_instance
            .map(|instance| instance.workspace_mode)
            .or(profile.workspace_mode),
        is_security_boundary: workspace_instance
            .map(|instance| instance.is_security_boundary)
            .unwrap_or(false),
        command_json,
        output_json,
        final_output_path: launch_outcome
            .metadata
            .get("final_output_path")
            .and_then(Value::as_str)
            .map(std::path::PathBuf::from),
        native_session_id,
        synthetic_session_id,
        status: launch_outcome.status.as_str().to_string(),
        fallback_final_source: fallback_final_source.map(str::to_string),
    })?;
    Ok(())
}

fn prepare_generic_cli_route_session(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    conversation_id: &Option<String>,
    runtime_temp_dir: Option<&std::path::PathBuf>,
) -> Result<GenericCliRouteSession> {
    let route_key = generic_cli_route_key(
        &profile.agent_handle,
        &profile.controller_scope_key,
        conversation_id.as_deref(),
    );
    let route_key_hash = stable_id_suffix(&route_key);
    let session_dir = runtime_temp_dir
        .cloned()
        .unwrap_or_else(std::env::temp_dir)
        .join("awiki-deamon")
        .join("generic-cli")
        .join("sessions")
        .join(&route_key_hash);
    let previous = state.load_latest_cli_driver_run_by_route(&route_key)?;
    Ok(GenericCliRouteSession {
        route_key,
        route_key_hash: route_key_hash.clone(),
        session_dir,
        last_run_id: previous.as_ref().map(|record| record.run_id.clone()),
        last_message_id: previous
            .as_ref()
            .and_then(|record| state.load_runtime_task_for_run(&record.run_id).ok())
            .map(|task| task.task_id.trim_start_matches("task_").to_string()),
        native_session_id: previous
            .as_ref()
            .and_then(|record| record.native_session_id.clone()),
        synthetic_session_id: previous
            .as_ref()
            .and_then(|record| record.synthetic_session_id.clone())
            .or_else(|| Some(route_key_hash.clone())),
        status: previous
            .as_ref()
            .map(|record| record.status.clone())
            .unwrap_or_else(|| "new".to_string()),
    })
}

fn canonicalize_optional_path(path: Option<&std::path::PathBuf>) -> Option<std::path::PathBuf> {
    path.map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
}

fn generic_cli_route_key(
    agent_handle: &str,
    controller_scope_key: &str,
    conversation_id: Option<&str>,
) -> String {
    format!(
        "cli:{agent_handle}:{controller_scope_key}:{}:message-run",
        conversation_id.unwrap_or("no-conversation")
    )
}

fn fallback_final_text(metadata: &Value) -> Result<Option<String>> {
    let Some(path) = metadata
        .get("final_output_path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
    else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read fallback final output {}", path.display()))?;
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text.to_string()))
    }
}

fn runtime_recipient_policy(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    controller_did: &str,
    authority: RuntimeInvocationAuthority,
) -> Result<RecipientPolicy> {
    if let Some(binding) =
        state.load_active_app_message_agent_binding_by_runtime(&profile.agent_did)?
    {
        return Ok(RecipientPolicy::app_message_handler(&binding.user_did));
    }
    if profile.runtime_plugin_id == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
        return match authority {
            RuntimeInvocationAuthority::Controller => {
                Ok(RecipientPolicy::hermes_default(controller_did))
            }
            RuntimeInvocationAuthority::Requester => {
                Ok(RecipientPolicy::controller_only(controller_did))
            }
        };
    }
    match state.load_cli_runtime_profile(&profile.runtime_profile_id) {
        Ok(cli_profile) => {
            RecipientPolicy::from_json(&cli_profile.recipient_policy_json, controller_did)
        }
        Err(_) => Ok(RecipientPolicy::controller_only(controller_did)),
    }
}

fn runtime_allowed_methods(authority: RuntimeInvocationAuthority) -> Vec<RpcMethod> {
    let mut methods = vec![
        RpcMethod::RpcPing,
        RpcMethod::TaskStatus,
        RpcMethod::TaskFinish,
        RpcMethod::ArtifactCreated,
        RpcMethod::AppActionRequest,
    ];
    if authority.can_send_outbound() {
        methods.push(RpcMethod::MsgSend);
        methods.push(RpcMethod::SendAttachment);
    }
    methods
}

fn collect_string_array(value: Option<&Value>, output: &mut Vec<String>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        anyhow::bail!("recipient policy entries must be arrays");
    };
    for item in items {
        let item = item
            .as_str()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .context("recipient policy entries must be non-empty strings")?;
        output.push(item.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn mention_surface_for_sender_uses_short_handle_from_full_handle() {
        let payload = json!({
            "source_sender_full_handle": "bob.anpclaw.com"
        });

        assert_eq!(
            mention_surface_for_sender(&payload, "did:human:bob"),
            "@bob"
        );
    }

    #[test]
    fn mention_surface_for_sender_derives_short_handle_from_wba_did() {
        let payload = json!({});

        assert_eq!(
            mention_surface_for_sender(&payload, "did:wba:awiki.info:user:alice:e1_sender"),
            "@alice"
        );
    }

    #[test]
    fn runtime_requester_authority_cannot_call_outbound_send_methods() {
        let methods = runtime_allowed_methods(RuntimeInvocationAuthority::Requester);

        assert!(methods.contains(&RpcMethod::RpcPing));
        assert!(methods.contains(&RpcMethod::TaskStatus));
        assert!(methods.contains(&RpcMethod::TaskFinish));
        assert!(!methods.contains(&RpcMethod::MsgSend));
        assert!(!methods.contains(&RpcMethod::SendAttachment));
    }

    #[test]
    fn runtime_controller_authority_can_call_outbound_send_methods() {
        let methods = runtime_allowed_methods(RuntimeInvocationAuthority::Controller);

        assert!(methods.contains(&RpcMethod::MsgSend));
        assert!(methods.contains(&RpcMethod::SendAttachment));
    }
}
