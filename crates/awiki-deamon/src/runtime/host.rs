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
    runtime_task_matches_profile_controller_scope, RuntimeAgentProfile, RuntimeLaunchContext,
    RuntimeLaunchOutcome, RuntimePlugin, RuntimeRun, RuntimeRunStatus, RuntimeTask,
};
use crate::security::runtime_token::{
    current_time_millis, issue_runtime_token, RpcMethod, RuntimeTokenScope,
    ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE, ANY_DIRECT_RECIPIENT_SCOPE, ANY_GROUP_RECIPIENT_SCOPE,
};
use crate::state::{
    canonical_cli_conversation_id, cli_route_session_key, AuthorizedRuntimeContext,
    CliDriverRunRecord, CreateCliRouteSession, DaemonState, RuntimeFinalOutboxRecord,
};
use crate::workspace::{
    prepare_workspace_instance, route_workspace_paths, WorkspaceBindingConfig, WorkspaceInstance,
    WorkspaceMode,
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
    let task_source_message_id = task
        .task_id
        .strip_prefix("task_")
        .unwrap_or(&task.task_id)
        .to_string();
    let recipient_policy = runtime_recipient_policy(state, profile, &task_reply_recipient_did)?;
    let mut scope = RuntimeTokenScope::new(
        profile.agent_did.clone(),
        profile.runtime_profile_id.clone(),
        run.run_id.clone(),
        vec![
            RpcMethod::RpcPing,
            RpcMethod::TaskStatus,
            RpcMethod::TaskFinish,
            RpcMethod::MsgSend,
            RpcMethod::SendAttachment,
            RpcMethod::ArtifactCreated,
            RpcMethod::AppActionRequest,
        ],
        Some(recipient_policy.allowed_recipients),
        Duration::from_secs(5 * 60),
    )?;
    scope.allowed_message_security = Some(recipient_policy.allowed_message_security);
    let issued = issue_runtime_token(scope)?;
    state.store_runtime_token(&issued)?;

    let install_status = plugin.check_install_status()?;
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
        }
        anyhow::bail!("runtime plugin {} is not installed", plugin.plugin_id());
    }
    let cli_route_session = if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        match prepare_generic_cli_route_session(state, profile, &task, &run) {
            Ok(session) => session,
            Err(error) => {
                state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
                emit_runtime_status(
                    outbox,
                    &run,
                    "failed",
                    Some("Runtime route session preparation failed"),
                    Some("route_session_preparation_failed"),
                    Some(&sanitize_user_visible_error_summary(&error.to_string())),
                )?;
                return Err(error).context("prepare generic-cli route session");
            }
        }
    } else {
        None
    };
    let mut cli_runtime_locks = CliRuntimeLeaseGuard::default();
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        match acquire_generic_cli_runtime_locks(state, profile, &run) {
            Ok(locks) => {
                cli_runtime_locks = locks;
            }
            Err(error) => {
                if let Some(route_session) = cli_route_session.as_ref() {
                    let _ = state.release_cli_route_session_lease(
                        &route_session.route_key,
                        &run.run_id,
                        "failed",
                        Some(&task_source_message_id),
                        Some(error.code),
                        Some(&error.summary),
                    );
                }
                state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
                emit_runtime_status(
                    outbox,
                    &run,
                    "failed",
                    Some(error.user_message()),
                    Some(error.code),
                    Some(&error.summary),
                )?;
                return Err(error.into_error()).context("acquire generic-cli runtime lock");
            }
        }
    }
    let workspace_instance = match (
        profile.workspace_id.as_ref(),
        profile.workspace_root.as_ref(),
        profile.workspace_mode,
        local_socket_path.is_some(),
        plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID,
    ) {
        (Some(workspace_id), Some(workspace_root), Some(workspace_mode), true, true) => Some({
            let run_or_route_id = cli_route_session
                .as_ref()
                .map(|session| session.route_key_hash.as_str())
                .unwrap_or(run.run_id.as_str());
            let prepared = prepare_workspace_instance(
                runtime_temp_dir
                    .as_ref()
                    .context("runtime temp dir is unavailable without daemon config")?,
                &WorkspaceBindingConfig {
                    workspace_id: workspace_id.clone(),
                    workspace_root: workspace_root.clone(),
                    workspace_mode,
                },
                run_or_route_id,
            );
            match prepared {
                Ok(instance) => instance,
                Err(error) => {
                    state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
                    if let Some(route_session) = cli_route_session.as_ref() {
                        let _ = state.release_cli_route_session_lease(
                            &route_session.route_key,
                            &run.run_id,
                            "failed",
                            Some(&task_source_message_id),
                            Some("workspace_preparation_failed"),
                            Some(&sanitize_user_visible_error_summary(&error.to_string())),
                        );
                    }
                    cli_runtime_locks.release_all();
                    emit_runtime_status(
                        outbox,
                        &run,
                        "failed",
                        Some("Runtime workspace preparation failed"),
                        Some("workspace_preparation_failed"),
                        Some(&sanitize_user_visible_error_summary(&error.to_string())),
                    )?;
                    return Err(error).context("prepare runtime workspace instance");
                }
            }
        }),
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
            if let Some(route_session) = cli_route_session.as_ref() {
                let _ = state.release_cli_route_session_lease(
                    &route_session.route_key,
                    &run.run_id,
                    "failed",
                    Some(&task_source_message_id),
                    Some("launch_failed"),
                    Some(&sanitize_user_visible_error_summary(&error.to_string())),
                );
            }
            cli_runtime_locks.release_all();
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
            }
            return Err(error).context("launch runtime run");
        }
    };
    for callback in launch_outcome.callbacks.iter().cloned() {
        execute_runtime_rpc_request_with_outbox(state, outbox, callback)
            .context("apply runtime callback")?;
    }
    if state.load_runtime_run(&run.run_id)?.status == RuntimeRunStatus::Pending {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Running)?;
        emit_runtime_status(outbox, &run, "running", Some("Runtime started"), None, None)?;
    }

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

    let mut fallback_final_source = None;
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID
        && launch_outcome.status == RuntimeRunStatus::Finished
        && state.load_runtime_run(&run.run_id)?.status != RuntimeRunStatus::Finished
    {
        if let Some(final_text) = fallback_final_text(&launch_outcome.metadata)? {
            let fallback_driver_id = launch_outcome
                .metadata
                .get("driver_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("generic-cli");
            let response = execute_runtime_rpc_request_with_outbox(
                state,
                outbox,
                CliWrapperRequest::task_finish(
                    issued.token.as_str().to_string(),
                    run.task_id.clone(),
                    final_text,
                )
                .into_rpc_request(),
            )
            .context("apply fallback final from CLI driver output")?;
            if response.ok {
                fallback_final_source = Some(format!("{fallback_driver_id}_output_last_message"));
            }
        }
    }

    if launch_outcome.status == RuntimeRunStatus::Failed
        && state.load_runtime_run(&run.run_id)?.status != RuntimeRunStatus::Failed
    {
        state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
        emit_runtime_status(
            outbox,
            &run,
            "failed",
            Some("Runtime failed"),
            Some("runtime_failed"),
            Some(&runtime_launch_failure_summary(&launch_outcome)),
        )?;
    }
    if plugin.plugin_id() == crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        if let Some(route_session) = cli_route_session.as_ref() {
            let native_session_id = launch_outcome
                .metadata
                .get("native_session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let final_status = state.load_runtime_run(&run.run_id)?.status;
            if final_status == RuntimeRunStatus::Finished
                && launch_outcome.status == RuntimeRunStatus::Finished
                && native_session_id.is_some()
            {
                let native_session_source = launch_outcome
                    .metadata
                    .get("native_session_source")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                state.update_cli_route_session_native_id_if_locked(
                    &route_session.route_key,
                    &run.run_id,
                    native_session_id,
                    native_session_source,
                    Some(&route_session.route_key),
                )?;
            }
        }
        persist_cli_driver_run(
            state,
            profile,
            &run,
            &launch_outcome,
            workspace_instance.as_ref(),
            fallback_final_source.as_deref(),
        )?;
        if let Some(route_session) = cli_route_session.as_ref() {
            let final_status = state.load_runtime_run(&run.run_id)?.status;
            let next_status = if final_status == RuntimeRunStatus::Failed
                || launch_outcome.status == RuntimeRunStatus::Failed
            {
                "failed"
            } else {
                "active"
            };
            let failure_summary = if next_status == "failed" {
                Some(runtime_launch_failure_summary(&launch_outcome))
            } else {
                None
            };
            state.release_cli_route_session_lease(
                &route_session.route_key,
                &run.run_id,
                next_status,
                Some(&task_source_message_id),
                if next_status == "failed" {
                    Some("runtime_failed")
                } else {
                    None
                },
                if next_status == "failed" {
                    failure_summary.as_deref()
                } else {
                    None
                },
            )?;
        }
        cli_runtime_locks.release_all();
    }

    Ok(RuntimeTaskRunResult {
        run: state.load_runtime_run(&run.run_id)?,
        launch_outcome,
        token_id: issued.token_id,
    })
}

#[derive(Default)]
struct CliRuntimeLeaseGuard {
    state: Option<DaemonState>,
    runtime_profile_id: Option<String>,
    host_home_driver_id: Option<String>,
    run_id: Option<String>,
}

impl CliRuntimeLeaseGuard {
    fn profile(state: DaemonState, runtime_profile_id: String, run_id: String) -> Self {
        Self {
            state: Some(state),
            runtime_profile_id: Some(runtime_profile_id),
            host_home_driver_id: None,
            run_id: Some(run_id),
        }
    }

    fn mark_host_home(&mut self, driver_id: String) {
        self.host_home_driver_id = Some(driver_id);
    }

    fn release_profile(&mut self) {
        if let (Some(state), Some(runtime_profile_id), Some(run_id)) = (
            self.state.as_ref(),
            self.runtime_profile_id.take(),
            self.run_id.as_deref(),
        ) {
            let _ = state.release_cli_runtime_profile_lock(&runtime_profile_id, run_id);
        }
    }

    fn release_host_home(&mut self) {
        if let (Some(state), Some(driver_id), Some(run_id)) = (
            self.state.as_ref(),
            self.host_home_driver_id.take(),
            self.run_id.as_deref(),
        ) {
            let _ = state.release_cli_host_home_lock(&driver_id, run_id);
        }
    }

    fn release_all(&mut self) {
        self.release_host_home();
        self.release_profile();
    }
}

impl Drop for CliRuntimeLeaseGuard {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[derive(Debug, Clone)]
struct GenericCliRuntimeLockError {
    code: &'static str,
    message: String,
    summary: String,
}

impl GenericCliRuntimeLockError {
    fn profile_busy(runtime_profile_id: &str) -> Self {
        Self::new(
            "profile_busy",
            format!("generic-cli runtime profile is busy: {runtime_profile_id}"),
        )
    }

    fn host_home_busy(driver_id: &str) -> Self {
        Self::new(
            "host_home_busy",
            format!("generic-cli host home is busy: {driver_id}"),
        )
    }

    fn new(code: &'static str, message: String) -> Self {
        let summary = sanitize_user_visible_error_summary(&message);
        Self {
            code,
            message,
            summary,
        }
    }

    fn user_message(&self) -> &'static str {
        match self.code {
            "host_home_busy" => "Runtime host home concurrency limit is busy",
            "runtime_lock_unavailable" => "Runtime concurrency lock is unavailable",
            _ => "Runtime profile concurrency limit is busy",
        }
    }

    fn into_error(self) -> anyhow::Error {
        anyhow::anyhow!(self.message)
    }
}

impl From<anyhow::Error> for GenericCliRuntimeLockError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(
            "runtime_lock_unavailable",
            format!("generic-cli runtime lock unavailable: {error}"),
        )
    }
}

fn acquire_generic_cli_runtime_locks(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    run: &RuntimeRun,
) -> std::result::Result<CliRuntimeLeaseGuard, GenericCliRuntimeLockError> {
    let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    let expires_at_ms = current_time_millis()? + 10 * 60 * 1000;
    let acquired_profile = state.try_acquire_cli_runtime_profile_lock(
        &profile.runtime_profile_id,
        &cli_profile.driver_id,
        &run.run_id,
        "runtime.host",
        expires_at_ms,
    )?;
    if !acquired_profile {
        return Err(GenericCliRuntimeLockError::profile_busy(
            &profile.runtime_profile_id,
        ));
    }

    let mut guard = CliRuntimeLeaseGuard::profile(
        state.clone(),
        profile.runtime_profile_id.clone(),
        run.run_id.clone(),
    );
    let needs_host_home_lock =
        cli_profile.driver_id == "claude-code" && cli_profile.config_home.is_none();
    if needs_host_home_lock {
        let acquired_host_home = state.try_acquire_cli_host_home_lock(
            &cli_profile.driver_id,
            &run.run_id,
            "runtime.host",
            expires_at_ms,
        )?;
        if !acquired_host_home {
            guard.release_profile();
            return Err(GenericCliRuntimeLockError::host_home_busy(
                &cli_profile.driver_id,
            ));
        }
        guard.mark_host_home(cli_profile.driver_id);
    }
    Ok(guard)
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

fn prepare_generic_cli_route_session(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    task: &RuntimeTask,
    run: &RuntimeRun,
) -> Result<Option<crate::state::CliRouteSessionRecord>> {
    if profile.workspace_mode != Some(WorkspaceMode::RouteRoot) {
        return Ok(None);
    }
    let conversation_id = task
        .conversation_id
        .as_deref()
        .context("generic-cli RouteRoot requires conversation_id")?;
    let conversation_id = canonical_cli_conversation_id(conversation_id)?;
    let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    let workspace_root = profile
        .workspace_root
        .as_ref()
        .context("generic-cli RouteRoot requires workspace_root")?;
    let route_key = cli_route_session_key(
        &profile.agent_did,
        &profile.controller_scope_key,
        &conversation_id,
    )?;
    let route_key_hash = crate::state::cli_route_key_hash(&route_key)?;
    let session_root = workspace_root
        .parent()
        .map(|runtime_workspaces_root| {
            runtime_workspaces_root
                .parent()
                .unwrap_or(runtime_workspaces_root)
                .join("sessions")
                .join(&profile.runtime_profile_id)
        })
        .unwrap_or_else(|| workspace_root.join("sessions"));
    let paths = route_workspace_paths(workspace_root, &session_root, &route_key_hash)?;
    let session = state.get_or_create_cli_route_session(CreateCliRouteSession {
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        driver_id: cli_profile.driver_id,
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: task.controller_did.clone(),
        conversation_id,
        workspace_path: paths.workspace_path,
        session_dir: paths.session_dir,
    })?;
    if let Err(error) = std::fs::create_dir_all(&session.workspace_path).with_context(|| {
        format!(
            "create generic-cli route workspace {}",
            session.workspace_path.display()
        )
    }) {
        let summary = sanitize_user_visible_error_summary(&error.to_string());
        let _ = state.mark_cli_route_session_failed(
            &session.route_key,
            Some(&run.run_id),
            "route_workspace_create_failed",
            &summary,
        );
        return Err(error);
    }
    if let Err(error) = std::fs::create_dir_all(&session.session_dir).with_context(|| {
        format!(
            "create generic-cli route session dir {}",
            session.session_dir.display()
        )
    }) {
        let summary = sanitize_user_visible_error_summary(&error.to_string());
        let _ = state.mark_cli_route_session_failed(
            &session.route_key,
            Some(&run.run_id),
            "route_session_dir_create_failed",
            &summary,
        );
        return Err(error);
    }
    if !state.try_acquire_cli_route_session_lease(
        &session.route_key,
        &run.run_id,
        "runtime.host",
        current_time_millis()? + 10 * 60 * 1000,
    )? {
        anyhow::bail!(
            "generic-cli route session is busy: {}",
            session.route_key_hash
        );
    }
    Ok(Some(session))
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
    state
        .update_runtime_run_status(&record.run_id, RuntimeRunStatus::Finished)
        .context("mark Hermes run finished after final delivery")?;
    emit_runtime_status(
        outbox,
        &run,
        "succeeded",
        Some("Hermes response sent"),
        None,
        None,
    )?;
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
    state.update_runtime_run_status(&run.run_id, RuntimeRunStatus::Failed)?;
    emit_runtime_status(
        outbox,
        run,
        "failed",
        Some("Hermes run failed"),
        Some(error_code),
        Some(&sanitized),
    )?;
    Ok(())
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
    if let Some(summary) = launch_outcome
        .metadata
        .get("error_summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return sanitize_user_visible_error_summary(summary);
    }
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
    let route_key = if profile.workspace_mode == Some(WorkspaceMode::RouteRoot) {
        let conversation_id = task
            .conversation_id
            .as_deref()
            .context("generic-cli RouteRoot run record requires conversation_id")?;
        let conversation_id = canonical_cli_conversation_id(conversation_id)?;
        cli_route_session_key(
            &run.agent_did,
            &profile.controller_scope_key,
            &conversation_id,
        )?
    } else {
        generic_cli_route_key(
            &run.agent_did,
            &profile.controller_scope_key,
            task.conversation_id.as_deref(),
        )
    };
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
        route_key: route_key.clone(),
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
        native_session_id: launch_outcome
            .metadata
            .get("native_session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        synthetic_session_id: Some(route_key),
        status: launch_outcome.status.as_str().to_string(),
        fallback_final_source: fallback_final_source.map(str::to_string),
    })?;
    Ok(())
}

fn canonicalize_optional_path(path: Option<&std::path::PathBuf>) -> Option<std::path::PathBuf> {
    path.map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
}

fn generic_cli_route_key(
    agent_did: &str,
    controller_scope_key: &str,
    conversation_id: Option<&str>,
) -> String {
    format!(
        "cli:{agent_did}:{controller_scope_key}:{}:message-run",
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
) -> Result<RecipientPolicy> {
    if let Some(binding) =
        state.load_active_app_message_agent_binding_by_runtime(&profile.agent_did)?
    {
        return Ok(RecipientPolicy::app_message_handler(&binding.user_did));
    }
    if profile.runtime_plugin_id == crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID {
        return Ok(RecipientPolicy::hermes_default(controller_did));
    }
    match state.load_cli_runtime_profile(&profile.runtime_profile_id) {
        Ok(cli_profile) => {
            RecipientPolicy::from_json(&cli_profile.recipient_policy_json, controller_did)
        }
        Err(_) => Ok(RecipientPolicy::controller_only(controller_did)),
    }
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
}
