//! Common task admission, authorization and durable final delivery for ACP.
use crate::controller_scope::VerifiedControllerSender;
use crate::inbox::{
    route_controller_text_task, route_controller_text_task_with_verified_sender,
    ControllerTextMessage,
};
use crate::outbox::{
    RuntimeMessageSecurity, RuntimeMessageSend, RuntimeMessageTarget, RuntimeOutbox,
};
use crate::runtime::reply_payload::{
    group_did_from_conversation_id, structured_direct_reply, structured_group_reply,
    StructuredDirectReplyInput, StructuredGroupReplyInput,
};
use crate::runtime::{RuntimeAgentProfile, RuntimeLaunchOutcome, RuntimeRun, RuntimeTask};
use crate::security::runtime_token::{
    current_time_millis, issue_runtime_token, RpcMethod, RuntimeTokenScope,
};
use crate::state::{AuthorizedRuntimeContext, DaemonState, RuntimeFinalOutboxRecord};
use crate::DaemonConfig;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
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

pub fn run_controller_text_task_with_config(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    outbox: &impl RuntimeOutbox,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult> {
    let task = route_controller_text_task(profile, message)?;
    run_existing_runtime_task_with_config(config, state, profile, outbox, task)
}

pub fn run_controller_text_task_with_verified_sender_config(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    verified_sender: &VerifiedControllerSender,
    outbox: &impl RuntimeOutbox,
    message: ControllerTextMessage,
) -> Result<RuntimeTaskRunResult> {
    let task = route_controller_text_task_with_verified_sender(profile, verified_sender, message)?;
    run_existing_runtime_task_with_config(config, state, profile, outbox, task)
}

pub fn run_existing_runtime_task_with_config(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    outbox: &impl RuntimeOutbox,
    task: RuntimeTask,
) -> Result<RuntimeTaskRunResult> {
    let run_id = format!("run_{}", task.task_id);
    crate::acp::host::run(
        state,
        profile,
        outbox,
        task,
        run_id,
        Some(&config.local_socket_path),
    )
}

pub(crate) fn existing_runtime_run(
    state: &DaemonState,
    expected: &RuntimeRun,
) -> Result<Option<RuntimeTaskRunResult>> {
    let existing = match state.load_runtime_run(&expected.run_id) {
        Ok(run) => run,
        Err(error)
            if matches!(
                error.downcast_ref::<rusqlite::Error>(),
                Some(rusqlite::Error::QueryReturnedNoRows)
            ) =>
        {
            return Ok(None)
        }
        Err(error) => return Err(error),
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
    let delegated_outbox = crate::inbox::user_delegated::UserDelegatedRuntimeOutbox::new(state);
    for record in records {
        // The durable task, not whichever worker happens to flush the outbox,
        // owns the delivery audience. A missing task must fail closed.
        let task = match state.load_runtime_task_for_run(&record.run_id) {
            Ok(task) => task,
            Err(error)
                if matches!(
                    error.downcast_ref::<rusqlite::Error>(),
                    Some(rusqlite::Error::QueryReturnedNoRows)
                ) =>
            {
                state.mark_runtime_final_outbox_failed_terminal(
                    &record.idempotency_key,
                    "task_binding_unavailable",
                    "Task binding is unavailable; delivery is fenced",
                )?;
                state.fail_active_runtime_run(&record.run_id)?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if crate::acp::background::is_background(&task) {
            let binding = state
                .load_runtime_agent_profile(&record.agent_did)
                .and_then(|profile| crate::acp::background::binding(state, &profile, &task));
            if let Err(error) = binding {
                // Transient database failures must remain retryable. A
                // conclusively absent/disabled binding is a delivery fence.
                if error
                    .downcast_ref::<rusqlite::Error>()
                    .is_some_and(|error| !matches!(error, rusqlite::Error::QueryReturnedNoRows))
                {
                    return Err(error);
                }
                state.mark_runtime_final_outbox_failed_terminal(
                    &record.idempotency_key,
                    "personal_agent_binding_inactive",
                    "Personal agent binding is inactive; delivery is fenced",
                )?;
                state.fail_active_runtime_run(&record.run_id)?;
                continue;
            }
        }
        let outbox: &dyn RuntimeOutbox = if crate::acp::background::is_background(&task) {
            &delegated_outbox
        } else {
            outbox
        };
        if record.status != "pending" {
            continue;
        }
        if let Some(binding) = state.load_runtime_daemon_binding(&record.agent_did)? {
            if crate::agent_status::controller_identity_change_observed(
                state,
                &binding.daemon_agent_did,
            )? {
                if state.mark_runtime_final_outbox_failed_terminal(
                    &record.idempotency_key,
                    "controller_identity_changed",
                    "Controller identity changed; automatic delivery is fenced",
                )? {
                    state.fail_active_runtime_run(&record.run_id)?;
                }
                continue;
            }
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
                        "final_source": record.final_source,
                        "final_body_hash": record.final_body_hash,
                        "final_text_bytes": record.final_text.len(),
                    }),
                )?;
                sent_count += 1;
            }
            Err(error) => {
                let error_summary = sanitize_user_visible_error_summary(&error.to_string());
                let attempts = record.attempt_count + 1;
                if attempts >= MAX_RUNTIME_FINAL_OUTBOX_ATTEMPTS {
                    let failed_terminal = state.mark_runtime_final_outbox_failed_terminal(
                        &record.idempotency_key,
                        "final_delivery_failed",
                        &error_summary,
                    )?;
                    if failed_terminal {
                        let run = state.load_runtime_run(&record.run_id)?;
                        mark_runtime_run_failed_with_status(
                            state,
                            outbox,
                            &run,
                            "智能体回复发送失败",
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
                    }
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
    let task = match state.load_runtime_task_for_run(&record.run_id) {
        Ok(task) => task,
        Err(_) => return Ok(None),
    };
    let correlation = task.correlation();
    let source_message_id = correlation.source_message_id.as_str();
    let annotate = |mut payload: serde_json::Value| {
        if record.final_source == "acp" {
            payload["annotations"]["awiki_run_id"] = serde_json::json!(record.run_id);
        }
        payload
    };
    let is_group = record
        .conversation_id
        .as_deref()
        .and_then(group_did_from_conversation_id)
        .is_some();
    if is_group {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&task.text) {
            if payload.get("mention_context").is_some() {
                if let Some(sender_did) = payload
                    .get("source_sender_did")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    if let Some(reply) = structured_group_reply(StructuredGroupReplyInput {
                        run_id: &record.run_id,
                        agent_did: &record.agent_did,
                        requester_did: sender_did,
                        requester_full_handle: payload
                            .get("source_sender_full_handle")
                            .and_then(serde_json::Value::as_str),
                        source_message_id: Some(source_message_id),
                        reply_text: &record.final_text,
                    }) {
                        return Ok(Some(annotate(reply.payload)));
                    }
                }
            }
        }
    }
    Ok(structured_direct_reply(StructuredDirectReplyInput {
        agent_did: &record.agent_did,
        source_message_id,
        reply_text: &record.final_text,
    })
    .map(|reply| annotate(reply.payload)))
}

fn mark_runtime_final_delivered(
    state: &DaemonState,
    outbox: &(impl RuntimeOutbox + ?Sized),
    record: &RuntimeFinalOutboxRecord,
    message_id: Option<&str>,
) -> Result<()> {
    if !state.mark_runtime_final_outbox_sent(&record.idempotency_key, message_id)? {
        return Ok(());
    }
    let run = state.load_runtime_run(&record.run_id)?;
    let updated = state
        .finish_active_runtime_run(&record.run_id)
        .context("mark runtime run finished after final delivery")?;
    if updated {
        try_emit_runtime_status(
            state,
            outbox,
            &run,
            "succeeded",
            Some("Runtime response sent"),
            None,
            None,
        )?;
    }
    Ok(())
}

pub(crate) fn runtime_final_outbox_record(
    profile: &RuntimeAgentProfile,
    controller_did: &str,
    recipient_did: &str,
    run: &RuntimeRun,
    conversation_id: Option<&str>,
    final_text: &str,
    final_source: &str,
) -> Result<RuntimeFinalOutboxRecord> {
    let final_text = final_text.trim();
    if final_text.is_empty() {
        anyhow::bail!("runtime final text is empty");
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
        final_source: final_source.to_string(),
        final_body_hash: final_body_hash(final_text),
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

fn final_body_hash(final_text: &str) -> String {
    let digest = Sha256::digest(final_text.as_bytes());
    format!("sha256:{}", hex_lower(&digest))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn mark_runtime_run_failed_with_status(
    state: &DaemonState,
    outbox: &(impl RuntimeOutbox + ?Sized),
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

fn mark_active_runtime_run_failed(state: &DaemonState, run_id: &str) -> Result<bool> {
    state.fail_active_runtime_run(run_id)
}

fn emit_runtime_status(
    outbox: &(impl RuntimeOutbox + ?Sized),
    run: &RuntimeRun,
    status: &str,
    message: Option<&str>,
    last_error_code: Option<&str>,
    last_error_summary: Option<&str>,
) -> Result<()> {
    emit_runtime_status_with_metadata(
        outbox,
        run,
        status,
        message,
        last_error_code,
        last_error_summary,
        None,
    )
}

fn try_emit_runtime_status(
    state: &DaemonState,
    outbox: &(impl RuntimeOutbox + ?Sized),
    run: &RuntimeRun,
    status: &str,
    message: Option<&str>,
    last_error_code: Option<&str>,
    last_error_summary: Option<&str>,
) -> Result<()> {
    if let Err(error) = emit_runtime_status(
        outbox,
        run,
        status,
        message,
        last_error_code,
        last_error_summary,
    ) {
        state.insert_audit_event_json(
            "runtime.status.best_effort.failed",
            Some(&run.agent_did),
            Some(&run.runtime_profile_id),
            Some(&run.run_id),
            None,
            json!({
                "status": status,
                "error": sanitize_user_visible_error_summary(&error.to_string()),
            }),
        )?;
    }
    Ok(())
}

fn emit_runtime_status_with_metadata(
    outbox: &(impl RuntimeOutbox + ?Sized),
    run: &RuntimeRun,
    status: &str,
    message: Option<&str>,
    last_error_code: Option<&str>,
    last_error_summary: Option<&str>,
    metadata: Option<&Value>,
) -> Result<()> {
    let context = crate::state::AuthorizedRuntimeContext {
        token_id: "host-run-status".to_string(),
        agent_did: run.agent_did.clone(),
        runtime_profile_id: run.runtime_profile_id.clone(),
        run_id: run.run_id.clone(),
        method: RpcMethod::TaskStatus,
    };
    outbox.send_status_with_metadata(
        &context,
        status,
        message,
        last_error_code,
        last_error_summary,
        metadata,
    )?;
    Ok(())
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
        sanitized = "Agent execution failed".to_string();
    }
    if sanitized.chars().count() > 160 {
        sanitized = sanitized.chars().take(160).collect();
    }
    sanitized
}

fn runtime_recipient_policy(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    controller_did: &str,
) -> Result<RecipientPolicy> {
    if let Some(binding) =
        state.load_active_app_personal_agent_binding_by_runtime(&profile.agent_did)?
    {
        return Ok(RecipientPolicy::app_message_handler(&binding.user_did));
    }
    match state.load_cli_runtime_profile(&profile.runtime_profile_id) {
        Ok(cli_profile) => {
            RecipientPolicy::from_json(&cli_profile.recipient_policy_json, controller_did)
        }
        Err(_) => Ok(RecipientPolicy::controller_only(controller_did)),
    }
}

pub(crate) fn issue_acp_runtime_token(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    task: &RuntimeTask,
    run: &str,
) -> Result<crate::security::runtime_token::IssuedRuntimeToken> {
    let policy = runtime_recipient_policy(state, profile, &task.reply_recipient_did)?;
    let mut methods = vec![RpcMethod::RpcPing];
    if crate::acp::background::is_background(task) {
        crate::acp::background::binding(state, profile, task)?;
        methods.push(RpcMethod::AppActionRequest);
    } else if task.invocation_authority.can_send_outbound() {
        methods.extend([RpcMethod::MsgSend, RpcMethod::SendAttachment]);
    }
    let mut scope = RuntimeTokenScope::new(
        profile.agent_did.clone(),
        profile.runtime_profile_id.clone(),
        run.to_owned(),
        methods,
        Some(policy.allowed_recipients),
        Duration::from_secs(30 * 60),
    )?;
    scope.allowed_message_security = Some(policy.allowed_message_security);
    let issued = issue_runtime_token(scope)?;
    state.store_runtime_token(&issued)?;
    Ok(issued)
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
#[path = "host_tests.rs"]
mod tests;
