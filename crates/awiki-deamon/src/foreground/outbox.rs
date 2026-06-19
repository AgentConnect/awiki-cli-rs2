use super::*;

#[derive(Clone)]
pub(super) struct ControllerRuntimeOutbox {
    message_sender: ControllerOutboxSender,
    status_sender: ControllerOutboxSender,
    daemon_agent_did: Option<String>,
    recipient_did: String,
    controller_did: Option<String>,
    task_id: String,
    conversation_id: Option<String>,
    status_source_message_id: Option<String>,
    status_mention_id: Option<String>,
    requester_did: Option<String>,
    requester_full_handle: Option<String>,
    trigger_kind: Option<String>,
    sent_counter: Arc<std::sync::atomic::AtomicUsize>,
    sent_message_ids: Arc<Mutex<Vec<String>>>,
}

impl ControllerRuntimeOutbox {
    pub(super) fn new(
        message_sender: ControllerOutboxSender,
        status_sender: ControllerOutboxSender,
        daemon_agent_did: Option<String>,
        recipient_did: impl Into<String>,
        controller_did: Option<String>,
        task_id: impl Into<String>,
        conversation_id: Option<String>,
        sent_counter: Arc<std::sync::atomic::AtomicUsize>,
        sent_message_ids: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self::with_status_correlation(
            message_sender,
            status_sender,
            daemon_agent_did,
            recipient_did,
            controller_did,
            task_id,
            conversation_id,
            None,
            None,
            None,
            None,
            None,
            sent_counter,
            sent_message_ids,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn with_status_correlation(
        message_sender: ControllerOutboxSender,
        status_sender: ControllerOutboxSender,
        daemon_agent_did: Option<String>,
        recipient_did: impl Into<String>,
        controller_did: Option<String>,
        task_id: impl Into<String>,
        conversation_id: Option<String>,
        status_source_message_id: Option<String>,
        status_mention_id: Option<String>,
        requester_did: Option<String>,
        requester_full_handle: Option<String>,
        trigger_kind: Option<String>,
        sent_counter: Arc<std::sync::atomic::AtomicUsize>,
        sent_message_ids: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            message_sender,
            status_sender,
            daemon_agent_did,
            recipient_did: recipient_did.into(),
            controller_did,
            task_id: task_id.into(),
            conversation_id,
            status_source_message_id,
            status_mention_id,
            requester_did,
            requester_full_handle,
            trigger_kind,
            sent_counter,
            sent_message_ids,
        }
    }

    fn send_status_payload(&self, recipient_did: &str, payload: Value) -> Result<()> {
        let message_id = self.status_sender.send_payload(recipient_did, payload)?;
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut ids) = self.sent_message_ids.lock() {
            ids.push(message_id);
        }
        Ok(())
    }

    fn should_send_controller_activity(&self) -> Option<&str> {
        let controller_did = self.controller_did.as_deref()?.trim();
        if controller_did.is_empty() || controller_did == self.recipient_did.trim() {
            return None;
        }
        Some(controller_did)
    }

    fn send_controller_activity_payload_best_effort(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        sent_at: &str,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) {
        let Some(controller_did) = self.should_send_controller_activity() else {
            return;
        };
        if self.send_status_payload(
            controller_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "event_id": format!("evt_activity_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
                "sent_at": sent_at,
                "daemon_agent_did": self.daemon_agent_did.clone(),
                "status_scope": "runtime_activity",
                "run_id": context.run_id.clone(),
                "state": state,
                "daemon": null,
                "runtimes": [],
                "runs": [{
                    "run_id": context.run_id.clone(),
                    "runtime_agent_did": context.agent_did.clone(),
                    "agent_did": context.agent_did.clone(),
                    "requester_did": self.requester_did.clone(),
                    "requester_full_handle": self.requester_full_handle.clone(),
                    "trigger_kind": self.trigger_kind.clone(),
                    "status": state,
                    "started_at": sent_at,
                    "updated_at": sent_at,
                    "last_error_code": last_error_code,
                    "last_error_summary": last_error_summary,
                }],
            }),
        )
        .is_err()
        {
            eprintln!(
                "warning: controller runtime activity status send failed for {}",
                context.agent_did
            );
        }
    }
}

#[derive(Clone)]
pub(super) enum ControllerOutboxSender {
    ImCore(ImCoreAgentOutbox),
    Mock,
    #[cfg(test)]
    Recording(ControllerOutboxRecorder),
}

impl ControllerOutboxSender {
    pub(super) fn send_payload(&self, recipient_did: &str, payload: Value) -> Result<String> {
        match self {
            Self::ImCore(outbox) => Ok(outbox
                .send_payload(recipient_did, payload)?
                .message
                .id
                .as_str()
                .to_string()),
            Self::Mock => Ok(format!(
                "mock-status-{}",
                payload
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("message")
            )),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_payload(recipient_did, payload),
        }
    }

    fn send_runtime_message(&self, message: &RuntimeMessageSend) -> Result<String> {
        match self {
            Self::ImCore(outbox) => Ok(outbox
                .send_runtime_message(message.clone())?
                .message
                .id
                .as_str()
                .to_string()),
            Self::Mock => Ok(format!("mock-message-{}", message.security.as_str())),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_message(message),
        }
    }

    fn send_runtime_attachment(
        &self,
        recipient_did: &str,
        attachment: RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        match self {
            Self::ImCore(outbox) => {
                let result = outbox.send_attachment(recipient_did, attachment.clone())?;
                Ok(RuntimeAttachmentSendResult {
                    message_id: Some(result.message.message.id.as_str().to_string()),
                    target: attachment.target,
                    display_filename: attachment.display_filename,
                    size_bytes: Some(result.attachment.size_bytes),
                    agent_did: String::new(),
                })
            }
            Self::Mock => Ok(RuntimeAttachmentSendResult {
                message_id: Some("mock-attachment".to_string()),
                target: attachment.target,
                display_filename: attachment.display_filename,
                size_bytes: std::fs::metadata(&attachment.file_path)
                    .ok()
                    .map(|metadata| metadata.len()),
                agent_did: String::new(),
            }),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_attachment(recipient_did, attachment),
        }
    }

    fn resolve_recipient_did(&self, recipient: &str) -> Result<Option<String>> {
        match self {
            Self::ImCore(outbox) => outbox.resolve_handle(recipient),
            Self::Mock => {
                let recipient = recipient.trim();
                if recipient.starts_with("did:") {
                    Ok(Some(recipient.to_string()))
                } else {
                    Ok(None)
                }
            }
            #[cfg(test)]
            Self::Recording(recorder) => recorder.resolve_recipient_did(recipient),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(super) struct ControllerOutboxRecorder {
    pub(super) sender_id: String,
    pub(super) calls: Arc<Mutex<Vec<ControllerOutboxCall>>>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ControllerOutboxCall {
    pub(super) sender_id: String,
    pub(super) kind: &'static str,
    pub(super) recipient_did: String,
    pub(super) state: Option<String>,
    pub(super) text: Option<String>,
    pub(super) security: Option<RuntimeMessageSecurity>,
    pub(super) payload: Option<Value>,
}

#[cfg(test)]
impl ControllerOutboxRecorder {
    pub(super) fn new(
        sender_id: impl Into<String>,
        calls: Arc<Mutex<Vec<ControllerOutboxCall>>>,
    ) -> Self {
        Self {
            sender_id: sender_id.into(),
            calls,
        }
    }

    fn record_payload(&self, recipient_did: &str, payload: Value) -> Result<String> {
        self.push(
            "payload",
            recipient_did,
            payload
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_string),
            payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string),
            None,
            Some(payload),
        )
    }

    fn record_message(&self, message: &RuntimeMessageSend) -> Result<String> {
        self.push(
            "message",
            message.resolved_recipient(),
            None,
            Some(message.text.clone()),
            Some(message.security),
            None,
        )
    }

    fn record_attachment(
        &self,
        recipient_did: &str,
        attachment: RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let message_id = self.push(
            "attachment",
            recipient_did,
            None,
            attachment.caption.clone(),
            Some(RuntimeMessageSecurity::DefaultPlain),
            None,
        )?;
        Ok(RuntimeAttachmentSendResult {
            message_id: Some(message_id),
            target: attachment.target,
            display_filename: attachment.display_filename,
            size_bytes: std::fs::metadata(&attachment.file_path)
                .ok()
                .map(|metadata| metadata.len()),
            agent_did: String::new(),
        })
    }

    fn resolve_recipient_did(&self, recipient: &str) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            Ok(Some(recipient.to_string()))
        } else {
            Ok(None)
        }
    }

    fn push(
        &self,
        kind: &'static str,
        recipient_did: &str,
        state: Option<String>,
        text: Option<String>,
        security: Option<RuntimeMessageSecurity>,
        payload: Option<Value>,
    ) -> Result<String> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|_| anyhow::anyhow!("controller outbox recorder lock poisoned"))?;
        let next = calls.len() + 1;
        calls.push(ControllerOutboxCall {
            sender_id: self.sender_id.clone(),
            kind,
            recipient_did: recipient_did.to_string(),
            state,
            text,
            security,
            payload,
        });
        Ok(format!("recording-{}-{kind}-{next}", self.sender_id))
    }
}

fn outbox_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    agent_did: &str,
) -> Result<ControllerOutboxSender> {
    let identity = state.load_agent_identity(agent_did)?;
    let jwt_token = state.load_agent_auth_token(agent_did)?;
    let client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    Ok(ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(
        client,
    )))
}

pub(super) fn runtime_message_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    runtime_agent_did: &str,
) -> Result<ControllerOutboxSender> {
    outbox_sender_for_agent(config, state, im_core, runtime_agent_did)
        .with_context(|| format!("load runtime message sender for {runtime_agent_did}"))
}

pub(super) struct RuntimeStatusSender {
    pub(super) daemon_agent_did: String,
    pub(super) sender: ControllerOutboxSender,
}

pub(super) fn runtime_status_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    runtime_agent_did: &str,
) -> Result<RuntimeStatusSender> {
    let binding = state
        .load_runtime_daemon_binding(runtime_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {runtime_agent_did}"))?;
    let sender = outbox_sender_for_agent(config, state, im_core, &binding.daemon_agent_did)
        .with_context(|| {
            format!(
                "load daemon status sender {} for runtime {runtime_agent_did}",
                binding.daemon_agent_did
            )
        })?;
    Ok(RuntimeStatusSender {
        daemon_agent_did: binding.daemon_agent_did,
        sender,
    })
}

impl RuntimeOutbox for ControllerRuntimeOutbox {
    fn resolve_recipient_did(
        &self,
        _context: &crate::state::AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        self.message_sender.resolve_recipient_did(recipient)
    }

    fn send_status(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.send_status_with_detail(context, state, text, None, None)
    }

    fn send_status_with_detail(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> Result<()> {
        self.send_status_with_metadata(
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
            None,
        )
    }

    fn send_status_with_metadata(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
        metadata: Option<&Value>,
    ) -> Result<()> {
        let sent_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        let mut run_payload = json!({
            "run_id": context.run_id.clone(),
            "message_id": self.task_id.clone(),
            "source_message_id": self.status_source_message_id.clone(),
            "mention_id": self.status_mention_id.clone(),
            "runtime_agent_did": context.agent_did.clone(),
            "agent_did": context.agent_did.clone(),
            "requester_did": self.requester_did.clone(),
            "requester_full_handle": self.requester_full_handle.clone(),
            "trigger_kind": self.trigger_kind.clone(),
            "conversation_id": self.conversation_id.clone(),
            "status": state,
            "started_at": sent_at,
            "updated_at": sent_at,
            "last_error_code": last_error_code,
            "last_error_summary": last_error_summary,
        });
        if let (Some(metadata), Some(run_object)) = (metadata, run_payload.as_object_mut()) {
            if let Some(metadata_object) = metadata.as_object() {
                for (key, value) in metadata_object {
                    run_object.insert(key.clone(), value.clone());
                }
            }
        }
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
                "sent_at": sent_at,
                "daemon_agent_did": self.daemon_agent_did.clone(),
                "status_scope": "run",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "conversation_id": self.conversation_id.clone(),
                "state": state,
                "message": text,
                "daemon": null,
                "runtimes": [],
                "runs": [run_payload],
            }),
        )?;
        self.send_controller_activity_payload_best_effort(
            context,
            state,
            &sent_at,
            last_error_code,
            last_error_summary,
        );
        Ok(())
    }

    fn send_final(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> Result<()> {
        let sent_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
                "sent_at": sent_at,
                "daemon_agent_did": self.daemon_agent_did.clone(),
                "status_scope": "run",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "conversation_id": self.conversation_id.clone(),
                "state": "finished",
                "message": text,
                "daemon": null,
                "runtimes": [],
                "runs": [{
                    "run_id": context.run_id.clone(),
                    "message_id": self.task_id.clone(),
                    "source_message_id": self.status_source_message_id.clone(),
                    "mention_id": self.status_mention_id.clone(),
                    "runtime_agent_did": context.agent_did.clone(),
                    "agent_did": context.agent_did.clone(),
                    "requester_did": self.requester_did.clone(),
                    "requester_full_handle": self.requester_full_handle.clone(),
                    "trigger_kind": self.trigger_kind.clone(),
                    "conversation_id": self.conversation_id.clone(),
                    "status": "finished",
                    "started_at": sent_at,
                    "updated_at": sent_at,
                    "last_error_code": null,
                    "last_error_summary": null,
                }],
                "result": {
                    "type": "text",
                    "content": text.unwrap_or_default(),
                },
            }),
        )?;
        self.send_controller_activity_payload_best_effort(
            context, "finished", &sent_at, None, None,
        );
        Ok(())
    }

    fn send_message(
        &self,
        _context: &crate::state::AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let message_id = self.message_sender.send_runtime_message(message)?;
        Ok(RuntimeMessageSendResult {
            message_id: Some(message_id),
            raw_recipient: message.raw_recipient().to_string(),
            resolved_did: message.resolved_recipient().to_string(),
            target_kind: message.target_kind().to_string(),
            security: message.security,
        })
    }

    fn send_attachment(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let recipient_did = attachment
            .target_did
            .as_deref()
            .unwrap_or(self.recipient_did.as_str());
        let mut result = self
            .message_sender
            .send_runtime_attachment(recipient_did, attachment.clone())?;
        result.agent_did = context.agent_did.clone();
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(message_id) = result.message_id.as_ref() {
            if let Ok(mut ids) = self.sent_message_ids.lock() {
                ids.push(message_id.clone());
            }
        }
        Ok(result)
    }
}

#[derive(Clone)]
pub(super) struct RuntimeCallbackOutbox {
    config: DaemonConfig,
    state: DaemonState,
    im_core: ImCoreAdapter,
    sent_counter: Arc<std::sync::atomic::AtomicUsize>,
    sent_message_ids: Arc<Mutex<Vec<String>>>,
    mock_status_outbox: bool,
}

impl RuntimeCallbackOutbox {
    pub(super) fn new(
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
        mock_status_outbox: bool,
    ) -> Self {
        Self {
            config,
            state,
            im_core,
            sent_counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            sent_message_ids: Arc::new(Mutex::new(Vec::new())),
            mock_status_outbox,
        }
    }

    pub(super) fn sent_messages(&self) -> usize {
        self.sent_counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn status_message_ids(&self) -> Vec<String> {
        self.sent_message_ids
            .lock()
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }

    fn controller_outbox(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
    ) -> Result<ControllerRuntimeOutbox> {
        let task = self.state.load_runtime_task_for_run(&context.run_id)?;
        let (message_sender, daemon_agent_did, status_sender) = if self.mock_status_outbox {
            (
                ControllerOutboxSender::Mock,
                None,
                ControllerOutboxSender::Mock,
            )
        } else {
            let status_sender = runtime_status_sender_for_agent(
                &self.config,
                &self.state,
                &self.im_core,
                &context.agent_did,
            )?;
            (
                runtime_message_sender_for_agent(
                    &self.config,
                    &self.state,
                    &self.im_core,
                    &context.agent_did,
                )?,
                Some(status_sender.daemon_agent_did),
                status_sender.sender,
            )
        };
        let (status_source_message_id, status_mention_id) = runtime_task_status_correlation(&task);
        Ok(ControllerRuntimeOutbox::with_status_correlation(
            message_sender,
            status_sender,
            daemon_agent_did,
            task.reply_recipient_did,
            Some(task.controller_did),
            task.task_id,
            task.conversation_id,
            status_source_message_id,
            status_mention_id,
            Some(task.requester_did),
            task.requester_full_handle,
            Some(task.trigger_kind.as_str().to_string()),
            Arc::clone(&self.sent_counter),
            Arc::clone(&self.sent_message_ids),
        ))
    }
}

impl RuntimeOutbox for RuntimeCallbackOutbox {
    fn resolve_recipient_did(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            return Ok(Some(recipient.to_string()));
        }
        if self.mock_status_outbox {
            return Ok(None);
        }
        let message_sender = runtime_message_sender_for_agent(
            &self.config,
            &self.state,
            &self.im_core,
            &context.agent_did,
        )?;
        message_sender.resolve_recipient_did(recipient)
    }

    fn send_status(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.controller_outbox(context)?
            .send_status(context, state, text)
    }

    fn send_status_with_metadata(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
        metadata: Option<&Value>,
    ) -> Result<()> {
        self.controller_outbox(context)?.send_status_with_metadata(
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
            metadata,
        )
    }

    fn send_final(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> Result<()> {
        self.controller_outbox(context)?.send_final(context, text)
    }

    fn send_message(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        self.controller_outbox(context)?
            .send_message(context, message)
    }

    fn send_attachment(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        self.controller_outbox(context)?
            .send_attachment(context, attachment)
    }
}

impl AgentManagementOutbox for RuntimeCallbackOutbox {
    fn send_agent_status(&self, response: &AgentStatusResponse) -> Result<()> {
        let inner = if self.mock_status_outbox {
            ControllerOutboxSender::Mock
        } else {
            let identity = self.state.load_agent_identity(&response.agent_did)?;
            let jwt_token = self.state.load_agent_auth_token(&response.agent_did)?;
            let client = self.im_core.client_for_agent_identity(
                &self.config,
                &identity,
                jwt_token.as_deref(),
            )?;
            ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(client))
        };
        let message_id = inner.send_payload(&response.recipient_did, response.payload.clone())?;
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut ids) = self.sent_message_ids.lock() {
            ids.push(message_id);
        }
        Ok(())
    }
}
