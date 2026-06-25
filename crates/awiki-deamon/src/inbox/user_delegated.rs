use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use im_core::messages::{
    InboxHistoryOptions, InboxQuery, InboxScope, Message, MessageBodyView, MessageDeliveryOptions,
    MessageDirection,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app_bridge::action::{APP_ACTION_RESULT_SCHEMA, APP_ACTION_SCHEMA, MVP_ALLOWED_ACTIONS};
use crate::app_bridge::bootstrap::{
    validate_user_delegated_identity_against_did_document, BootstrapDidDocumentResolver,
    DefaultBootstrapDidDocumentResolver,
};
use crate::app_bridge::message_agent::APP_MESSAGE_HANDLER_ROLE;
use crate::app_bridge::secret_store::normalize_delegated_private_key_pem;
use crate::im_core_adapter::ImCoreAdapter;
use crate::outbox::{
    ImCoreAgentOutbox, RuntimeAttachmentSend, RuntimeAttachmentSendResult, RuntimeMessageSend,
    RuntimeMessageSendResult, RuntimeOutbox,
};
use crate::plugins::generic_cli::{GenericCliDriverRegistry, GENERIC_CLI_RUNTIME_PLUGIN_ID};
use crate::plugins::hermes::{HermesRuntimePlugin, StdioHermesGateway, HERMES_RUNTIME_PLUGIN_ID};
use crate::runtime::host::run_existing_runtime_task_with_config;
use crate::runtime::{
    RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeRunStatus, RuntimeTask,
    RuntimeTaskTriggerKind,
};
use crate::security::runtime_token::current_time_millis;
use crate::state::{
    AppMessageAgentBindingRecord, AuthorizedRuntimeContext, DaemonState, InboxCursorRecord,
    MessageEventRecord, MessageSyncOutboxRecord, ProcessedMessageRecord,
    UserDelegatedIdentityRecord,
};
use crate::DaemonConfig;

const DEFAULT_SCOPE: &str = "default_plain";
const DEFAULT_LIMIT: u32 = 100;
const EVENT_SCHEMA_USER_MESSAGE: &str = "awiki.user_message.default_plain.v1";
const EVENT_SCHEMA_E2EE_OPAQUE: &str = "awiki.user_message.e2ee_opaque.v1";
const MESSAGE_SYNC_SCHEMA: &str = "awiki.message.sync.v1";
const MESSAGE_EVENT_STATUS_DISPATCHED: &str = "agent_dispatched";
const MESSAGE_EVENT_STATUS_SKIPPED_UNSUPPORTED: &str = "skipped_unsupported";
const PROCESSED_STATUS_DISPATCHED: &str = "dispatched";
const PROCESSED_STATUS_IGNORED_E2EE: &str = "ignored_e2ee_opaque";
const PROCESSED_STATUS_SKIPPED_APP_CONTROL: &str = "skipped_app_control";
const PROCESSED_STATUS_SKIPPED_UNSUPPORTED: &str = "skipped_unsupported";
const PROCESSED_STATUS_PROCESSING: &str = "processing";
const PROCESSED_STATUS_FAILED_RETRYABLE: &str = "failed_retryable";
const RETENTION_CLASS_SHORT_EXCERPT: &str = "short_excerpt";
const RETENTION_CLASS_OPAQUE_ONLY: &str = "opaque_no_plaintext";
const EXCERPT_MAX_CHARS: usize = 240;
const MAX_MESSAGE_SYNC_OUTBOX_ATTEMPTS: i64 = 5;
const MESSAGE_SYNC_OUTBOX_SENDING_STALE_MS: i64 = 5 * 60 * 1000;
const HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID: &str = "host-runtime-final-outbox";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessUserDelegatedInboxOutcome {
    pub binding_id: String,
    pub fetched_messages: usize,
    pub dispatched_messages: usize,
    pub ignored_e2ee_messages: usize,
    pub skipped_app_control_messages: usize,
    pub skipped_unsupported_messages: usize,
    pub skipped_processed_messages: usize,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatedInboxPage {
    pub messages: Vec<Message>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageEnvelope {
    pub schema: String,
    pub content_role: String,
    pub source_message_id: String,
    pub source_conversation_id: Option<String>,
    pub source_sender_did: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sender_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sender_full_handle: Option<String>,
    pub inbox_owner_did: String,
    pub message_kind: String,
    pub received_at: Option<String>,
    pub content_text: String,
    pub content_hash: String,
    pub allowed_actions: Vec<String>,
}

struct AgentDispatchContent {
    text: String,
    message_kind: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegatedMessageProcessOutcome {
    Dispatched,
    SkippedAppControl,
    SkippedUnsupported,
    IgnoredE2ee,
    SkippedProcessed,
}

pub trait UserDelegatedInboxClient {
    fn fetch_user_delegated_inbox(
        &self,
        identity: &UserDelegatedIdentityRecord,
        binding: &AppMessageAgentBindingRecord,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<DelegatedInboxPage>;
}

pub trait UserDelegatedMessageDispatcher {
    fn dispatch_user_message(
        &self,
        binding: &AppMessageAgentBindingRecord,
        task: RuntimeTask,
        envelope: &UserMessageEnvelope,
    ) -> Result<()>;
}

pub trait MessageSyncPayloadSender {
    fn send_message_sync_payload(
        &self,
        binding: &AppMessageAgentBindingRecord,
        idempotency_key: &str,
        payload: Value,
    ) -> Result<Option<String>>;
}

pub struct ImCoreMessageSyncPayloadSender<'a> {
    config: &'a DaemonConfig,
    state: &'a DaemonState,
    im_core: &'a ImCoreAdapter,
}

impl<'a> ImCoreMessageSyncPayloadSender<'a> {
    pub fn new(
        config: &'a DaemonConfig,
        state: &'a DaemonState,
        im_core: &'a ImCoreAdapter,
    ) -> Self {
        Self {
            config,
            state,
            im_core,
        }
    }
}

impl MessageSyncPayloadSender for ImCoreMessageSyncPayloadSender<'_> {
    fn send_message_sync_payload(
        &self,
        binding: &AppMessageAgentBindingRecord,
        idempotency_key: &str,
        payload: Value,
    ) -> Result<Option<String>> {
        let daemon_identity = self.state.load_agent_identity(&binding.daemon_agent_did)?;
        let jwt_token = self
            .state
            .load_agent_auth_token(&binding.daemon_agent_did)?;
        let client = self.im_core.client_for_agent_identity(
            self.config,
            &daemon_identity,
            jwt_token.as_deref(),
        )?;
        let outbox = ImCoreAgentOutbox::new(client);
        let result = outbox.send_payload_with_delivery(
            &binding.user_did,
            payload,
            MessageDeliveryOptions {
                idempotency_key: Some(idempotency_key.to_string()),
                wait_for_final_acceptance: false,
            },
        )?;
        Ok(Some(result.message.id.as_str().to_string()))
    }
}

pub struct RuntimeHostMessageDispatcher<'a> {
    config: &'a DaemonConfig,
    state: &'a DaemonState,
    hermes_gateway: StdioHermesGateway,
}

impl<'a> RuntimeHostMessageDispatcher<'a> {
    pub fn new(
        config: &'a DaemonConfig,
        state: &'a DaemonState,
        hermes_gateway: StdioHermesGateway,
    ) -> Self {
        Self {
            config,
            state,
            hermes_gateway,
        }
    }
}

impl UserDelegatedMessageDispatcher for RuntimeHostMessageDispatcher<'_> {
    fn dispatch_user_message(
        &self,
        _binding: &AppMessageAgentBindingRecord,
        task: RuntimeTask,
        _envelope: &UserMessageEnvelope,
    ) -> Result<()> {
        let profile = self.state.load_runtime_agent_profile(&task.agent_did)?;
        let run_id = delegated_runtime_run_id(self.state, &task.task_id)?;
        let outbox = UserDelegatedRuntimeOutbox::new(self.state);
        match profile.runtime_plugin_id.as_str() {
            HERMES_RUNTIME_PLUGIN_ID => {
                let hermes_profile = self.state.load_hermes_profile(&profile.agent_did)?;
                let plugin = HermesRuntimePlugin::with_state(
                    self.hermes_gateway.clone(),
                    hermes_profile,
                    self.state.clone(),
                );
                run_existing_runtime_task_with_config(
                    self.config,
                    self.state,
                    &profile,
                    &plugin,
                    &outbox,
                    task,
                    run_id,
                )?;
            }
            GENERIC_CLI_RUNTIME_PLUGIN_ID => {
                let cli_profile = self
                    .state
                    .load_cli_runtime_profile(&profile.runtime_profile_id)?;
                let plugin = GenericCliDriverRegistry::new(cli_profile);
                run_existing_runtime_task_with_config(
                    self.config,
                    self.state,
                    &profile,
                    &plugin,
                    &outbox,
                    task,
                    run_id,
                )?;
            }
            _ => {
                bail!(
                    "unsupported app message runtime plugin: {}",
                    profile.runtime_plugin_id
                );
            }
        }
        Ok(())
    }
}

fn delegated_runtime_run_id(state: &DaemonState, task_id: &str) -> Result<String> {
    let base = format!("run_{task_id}");
    match state.load_runtime_run(&base) {
        Ok(run) if run.status == RuntimeRunStatus::Failed => {}
        Ok(_) => return Ok(base),
        Err(_) => return Ok(base),
    }
    for attempt in 1..=5 {
        let candidate = format!("{base}_retry_{attempt}");
        match state.load_runtime_run(&candidate) {
            Ok(run) if run.status == RuntimeRunStatus::Failed => continue,
            Ok(_) => return Ok(candidate),
            Err(_) => return Ok(candidate),
        }
    }
    bail!("delegated message runtime retry attempts exhausted for task {task_id}")
}

pub struct ImCoreDelegatedInboxClient<'a> {
    config: &'a DaemonConfig,
    im_core: &'a ImCoreAdapter,
}

impl<'a> ImCoreDelegatedInboxClient<'a> {
    pub fn new(
        config: &'a DaemonConfig,
        _state: &'a DaemonState,
        im_core: &'a ImCoreAdapter,
    ) -> Self {
        Self { config, im_core }
    }
}

impl UserDelegatedInboxClient for ImCoreDelegatedInboxClient<'_> {
    fn fetch_user_delegated_inbox(
        &self,
        identity: &UserDelegatedIdentityRecord,
        binding: &AppMessageAgentBindingRecord,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<DelegatedInboxPage> {
        let did_resolver = DefaultBootstrapDidDocumentResolver::new(self.config);
        let did_document = did_resolver
            .resolve_user_did_document(&identity.user_did)
            .with_context(|| {
                format!(
                    "resolve current DID Document for delegated inbox {}",
                    identity.user_did
                )
            })?;
        validate_user_delegated_identity_against_did_document(
            identity,
            &did_document,
            time::OffsetDateTime::now_utc(),
        )?;
        let key_ref = ensure_delegated_inbox_key_ref(self.config, identity)?;
        ensure_delegated_inbox_did_shadow(self.config, identity)?;
        let client = self.im_core.client_for_did(&identity.user_did)?;
        let auth_status = client
            .auth()
            .status()
            .context("inspect delegated inbox auth session")?;
        if !auth_status.has_session || auth_status.needs_refresh {
            client
                .auth()
                .refresh_session()
                .context("refresh delegated inbox auth session")?;
        }
        let page = client.messages().inbox_with_metadata(InboxQuery {
            scope: InboxScope::All,
            limit: im_core::ids::PageLimit::new(limit)?,
            cursor: cursor.map(im_core::ids::Cursor::parse).transpose()?,
            unread_only: false,
            inbox_history_options: Some(InboxHistoryOptions {
                inbox_owner_did: Some(identity.user_did.clone()),
                inbox_auth_verification_method: Some(
                    binding.inbox_auth_verification_method.clone(),
                ),
                inbox_auth_key_ref: Some(format!("file:{}", key_ref.display())),
                inbox_auth: None,
            }),
        })?;
        Ok(DelegatedInboxPage {
            messages: page.items,
            next_cursor: page.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: page.has_more,
        })
    }
}

struct UserDelegatedRuntimeOutbox<'a> {
    state: &'a DaemonState,
}

impl<'a> UserDelegatedRuntimeOutbox<'a> {
    fn new(state: &'a DaemonState) -> Self {
        Self { state }
    }

    #[cfg(test)]
    fn new_for_test(state: &'a DaemonState) -> Self {
        Self { state }
    }

    fn send_host_runtime_final_sync(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let binding = self
            .state
            .load_active_app_message_agent_binding_by_runtime(&context.agent_did)?
            .with_context(|| {
                format!(
                    "missing app message binding for runtime final outbox {}",
                    context.agent_did
                )
            })?;
        let resolved_did = message
            .resolved_did()
            .unwrap_or_else(|| message.resolved_recipient())
            .trim();
        if message.target_kind() != "direct" || resolved_did != binding.user_did {
            self.state.insert_audit_event_json(
                "user_delegated_inbox.runtime_final.host_sync.rejected",
                Some(&context.agent_did),
                Some(&context.runtime_profile_id),
                Some(&context.run_id),
                Some(&context.token_id),
                json!({
                    "target_kind": message.target_kind(),
                    "security": message.security.as_str(),
                    "reason": "host_runtime_final_must_target_controller_did",
                }),
            )?;
            bail!("host runtime final must target the delegated controller DID")
        }
        queue_runtime_final_sync(self.state, context, Some(message.text.as_str()))?;
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_final.host_sync",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "binding_id": &binding.binding_id,
                "app_instance_id": &binding.app_instance_id,
                "target_kind": message.target_kind(),
                "security": message.security.as_str(),
                "has_text": !message.text.trim().is_empty(),
                "text_hash": content_hash(&message.text),
            }),
        )?;
        Ok(RuntimeMessageSendResult {
            message_id: message.idempotency_key.clone(),
            raw_recipient: message.raw_recipient().to_string(),
            resolved_did: binding.user_did.clone(),
            target_kind: message.target_kind().to_string(),
            security: message.security,
        })
    }
}

impl RuntimeOutbox for UserDelegatedRuntimeOutbox<'_> {
    fn resolve_recipient_did(
        &self,
        _context: &AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            Ok(Some(recipient.to_string()))
        } else {
            Ok(None)
        }
    }

    fn send_status(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.send_status_with_detail(context, state, text, None, None)
    }

    fn send_status_with_detail(
        &self,
        context: &AuthorizedRuntimeContext,
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
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
        metadata: Option<&Value>,
    ) -> Result<()> {
        queue_runtime_status_sync(
            self.state,
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
            metadata,
        )?;
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_status",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "state": state,
                "has_text": text.is_some_and(|value| !value.trim().is_empty()),
                "last_error_code": last_error_code,
                "last_error_summary": last_error_summary.map(sanitize_error_message),
                "metadata": metadata,
            }),
        )?;
        Ok(())
    }

    fn send_final(&self, context: &AuthorizedRuntimeContext, text: Option<&str>) -> Result<()> {
        queue_runtime_final_sync(self.state, context, text)?;
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_final",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "has_text": text.is_some_and(|value| !value.trim().is_empty()),
                "text_hash": text.map(content_hash),
            }),
        )?;
        Ok(())
    }

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        if context.token_id == HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID {
            return self.send_host_runtime_final_sync(context, message);
        }
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_msg_send.rejected",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "target_kind": message.target_kind(),
                "security": message.security.as_str(),
                "reason": "user_delegated_message_agent_outbound_send_not_enabled_in_step_05",
            }),
        )?;
        bail!("user delegated message agent outbound send is not enabled in Step 05")
    }

    fn send_attachment(
        &self,
        context: &AuthorizedRuntimeContext,
        _attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_attachment_send.rejected",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "reason": "user_delegated_message_agent_attachment_send_not_enabled_in_step_05",
            }),
        )?;
        bail!("user delegated message agent attachment send is not enabled in Step 05")
    }
}

fn queue_runtime_status_sync(
    state: &DaemonState,
    context: &AuthorizedRuntimeContext,
    run_state: &str,
    text: Option<&str>,
    last_error_code: Option<&str>,
    last_error_summary: Option<&str>,
    metadata: Option<&Value>,
) -> Result<()> {
    let binding = state
        .load_active_app_message_agent_binding_by_runtime(&context.agent_did)?
        .with_context(|| {
            format!(
                "missing app message binding for runtime status {}",
                context.agent_did
            )
        })?;
    let text_hash = text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(content_hash);
    let source = runtime_task_message_source(state, &context.run_id);
    state.upsert_message_sync_outbox(&MessageSyncOutboxRecord {
        idempotency_key: format!(
            "message-sync:{}:runtime-status:{}:{}",
            binding.user_did, context.run_id, run_state
        ),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": MESSAGE_SYNC_SCHEMA,
            "sync_type": "runtime_status",
            "binding_id": binding.binding_id,
            "owner_did": binding.user_did,
            "app_instance_id": binding.app_instance_id,
            "runtime_agent_did": context.agent_did,
            "runtime_profile_id": context.runtime_profile_id,
            "run_id": context.run_id,
            "source_message_id": source.source_message_id,
            "source_conversation_id": source.source_conversation_id,
            "source_sender_did": source.source_sender_did,
            "source_content_hash": source.source_content_hash,
            "state": run_state,
            "has_text": text_hash.is_some(),
            "text_hash": text_hash,
            "last_error_code": last_error_code,
            "last_error_summary": last_error_summary.map(sanitize_error_message),
            "status_metadata": metadata,
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })?;
    Ok(())
}

fn queue_runtime_final_sync(
    state: &DaemonState,
    context: &AuthorizedRuntimeContext,
    text: Option<&str>,
) -> Result<()> {
    let binding = state
        .load_active_app_message_agent_binding_by_runtime(&context.agent_did)?
        .with_context(|| {
            format!(
                "missing app message binding for runtime final {}",
                context.agent_did
            )
        })?;
    let text_hash = text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(content_hash);
    let source = runtime_task_message_source(state, &context.run_id);
    state.upsert_message_sync_outbox(&MessageSyncOutboxRecord {
        idempotency_key: format!(
            "message-sync:{}:runtime-final:{}",
            binding.user_did, context.run_id
        ),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": MESSAGE_SYNC_SCHEMA,
            "sync_type": "runtime_final",
            "binding_id": binding.binding_id,
            "owner_did": binding.user_did,
            "app_instance_id": binding.app_instance_id,
            "runtime_agent_did": context.agent_did,
            "runtime_profile_id": context.runtime_profile_id,
            "run_id": context.run_id,
            "source_message_id": source.source_message_id,
            "source_conversation_id": source.source_conversation_id,
            "source_sender_did": source.source_sender_did,
            "source_content_hash": source.source_content_hash,
            "state": "finished",
            "has_text": text_hash.is_some(),
            "text_hash": text_hash,
            "retention_class": "hash_only",
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })?;
    Ok(())
}

#[derive(Default)]
struct RuntimeTaskMessageSource {
    source_message_id: Option<String>,
    source_conversation_id: Option<String>,
    source_sender_did: Option<String>,
    source_content_hash: Option<String>,
}

fn runtime_task_message_source(state: &DaemonState, run_id: &str) -> RuntimeTaskMessageSource {
    let Ok(task) = state.load_runtime_task_for_run(run_id) else {
        return RuntimeTaskMessageSource::default();
    };
    let Ok(payload) = serde_json::from_str::<Value>(&task.text) else {
        return RuntimeTaskMessageSource::default();
    };
    RuntimeTaskMessageSource {
        source_message_id: string_field(&payload, "source_message_id"),
        source_conversation_id: string_field(&payload, "source_conversation_id"),
        source_sender_did: string_field(&payload, "source_sender_did"),
        source_content_hash: string_field(&payload, "content_hash"),
    }
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn flush_message_sync_outbox(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    limit: usize,
) -> Result<usize> {
    let sender = ImCoreMessageSyncPayloadSender::new(config, state, im_core);
    flush_message_sync_outbox_with_sender(state, &sender, limit)
}

pub fn flush_message_sync_outbox_with_sender<S>(
    state: &DaemonState,
    sender: &S,
    limit: usize,
) -> Result<usize>
where
    S: MessageSyncPayloadSender,
{
    let now = current_time_millis()?;
    state.recover_stale_message_sync_outbox_sending(
        now - MESSAGE_SYNC_OUTBOX_SENDING_STALE_MS,
        now,
    )?;
    let records = state.list_due_message_sync_outbox(now, limit)?;
    let mut sent_count = 0usize;
    for record in records {
        if record.status != "pending" {
            continue;
        }
        if !state.mark_message_sync_outbox_sending(&record.idempotency_key)? {
            continue;
        }
        let binding = state
            .load_active_app_message_agent_binding(
                &record.owner_did,
                &record.app_instance_id,
                APP_MESSAGE_HANDLER_ROLE,
            )?
            .with_context(|| {
                format!(
                    "missing active app message binding for {} {}",
                    record.owner_did, record.app_instance_id
                )
            });
        let send_result = match binding {
            Ok(binding) => sender.send_message_sync_payload(
                &binding,
                &record.idempotency_key,
                record.payload_json.clone(),
            ),
            Err(error) => Err(error),
        };
        match send_result {
            Ok(message_id) => {
                state.mark_message_sync_outbox_sent(&record.idempotency_key)?;
                state.insert_audit_event_json(
                    "message_sync_outbox.sent",
                    None,
                    None,
                    record.payload_json.get("run_id").and_then(Value::as_str),
                    None,
                    json!({
                        "idempotency_key": record.idempotency_key,
                        "owner_did": record.owner_did,
                        "app_instance_id": record.app_instance_id,
                        "schema": record.payload_json.get("schema").and_then(Value::as_str),
                        "sync_type": record.payload_json.get("sync_type").and_then(Value::as_str),
                        "source_message_id": record.payload_json.get("source_message_id")
                            .or_else(|| record.payload_json.get("message_id"))
                            .and_then(Value::as_str),
                        "message_id": message_id,
                        "attempt_count": record.attempt_count + 1,
                    }),
                )?;
                sent_count += 1;
            }
            Err(error) => {
                let error_summary = sanitize_error_message(&error.to_string());
                let attempts = record.attempt_count + 1;
                if attempts >= MAX_MESSAGE_SYNC_OUTBOX_ATTEMPTS {
                    state.mark_message_sync_outbox_failed_terminal(
                        &record.idempotency_key,
                        "message_sync_delivery_failed",
                        &error_summary,
                    )?;
                    state.insert_audit_event_json(
                        "message_sync_outbox.failed_terminal",
                        None,
                        None,
                        record.payload_json.get("run_id").and_then(Value::as_str),
                        None,
                        json!({
                            "idempotency_key": record.idempotency_key,
                            "attempt_count": attempts,
                            "reason": error_summary,
                        }),
                    )?;
                } else {
                    let next_attempt_at_ms = now + message_sync_retry_delay_ms(attempts);
                    state.mark_message_sync_outbox_retry(
                        &record.idempotency_key,
                        next_attempt_at_ms,
                        "message_sync_delivery_retry",
                        &error_summary,
                    )?;
                    state.insert_audit_event_json(
                        "message_sync_outbox.retry_scheduled",
                        None,
                        None,
                        record.payload_json.get("run_id").and_then(Value::as_str),
                        None,
                        json!({
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

fn message_sync_retry_delay_ms(attempts: i64) -> i64 {
    match attempts {
        0 | 1 => 10_000,
        2 => 30_000,
        3 => 120_000,
        4 => 300_000,
        _ => 900_000,
    }
}

pub fn process_user_delegated_inbox_once(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: StdioHermesGateway,
) -> Result<usize> {
    let client = ImCoreDelegatedInboxClient::new(config, state, im_core);
    let dispatcher = RuntimeHostMessageDispatcher::new(config, state, hermes_gateway);
    process_user_delegated_inbox_once_with_client(state, &client, &dispatcher)
}

fn process_user_delegated_inbox_once_with_client<C, D>(
    state: &DaemonState,
    client: &C,
    dispatcher: &D,
) -> Result<usize>
where
    C: UserDelegatedInboxClient,
    D: UserDelegatedMessageDispatcher,
{
    let mut processed = 0usize;
    for binding in state
        .list_active_app_message_agent_bindings()?
        .into_iter()
        .filter(|binding| binding.role == APP_MESSAGE_HANDLER_ROLE)
    {
        match process_user_delegated_inbox_for_binding(state, client, dispatcher, &binding) {
            Ok(outcome) => {
                processed += outcome.dispatched_messages
                    + outcome.ignored_e2ee_messages
                    + outcome.skipped_app_control_messages
                    + outcome.skipped_unsupported_messages;
            }
            Err(error) => {
                state.insert_audit_event_json(
                    "user_delegated_inbox.sync.failed",
                    Some(&binding.daemon_agent_did),
                    Some(&binding.runtime_profile_id),
                    None,
                    None,
                    json!({
                        "binding_id": binding.binding_id,
                        "user_did": binding.user_did,
                        "error": sanitize_error_message(&error.to_string()),
                    }),
                )?;
            }
        }
    }
    Ok(processed)
}

pub fn process_user_delegated_inbox_for_binding<C, D>(
    state: &DaemonState,
    client: &C,
    dispatcher: &D,
    binding: &AppMessageAgentBindingRecord,
) -> Result<ProcessUserDelegatedInboxOutcome>
where
    C: UserDelegatedInboxClient,
    D: UserDelegatedMessageDispatcher,
{
    binding.validate()?;
    let identity = state
        .load_user_delegated_identity(&binding.inbox_auth_verification_method)?
        .with_context(|| {
            format!(
                "missing user delegated identity {}",
                binding.inbox_auth_verification_method
            )
        })?;
    identity.validate()?;
    if identity.user_did != binding.user_did {
        bail!("app message binding user_did does not match delegated identity");
    }
    if identity.status != "paired_key_received" {
        bail!("user delegated identity is not active for inbox sync");
    }
    let inbox_scope = inbox_scope_for_binding(binding);
    let cursor = state
        .load_inbox_cursor(&binding.user_did, &inbox_scope)?
        .and_then(|record| record.cursor);
    let page =
        client.fetch_user_delegated_inbox(&identity, binding, cursor.as_deref(), DEFAULT_LIMIT)?;
    let mut dispatched = 0usize;
    let mut ignored_e2ee = 0usize;
    let mut skipped_app_control = 0usize;
    let mut skipped_unsupported = 0usize;
    let mut skipped_processed = 0usize;
    let fetched = page.messages.len();
    for message in page.messages {
        if message.direction == MessageDirection::Outgoing {
            continue;
        }
        match process_user_delegated_message_for_binding(state, dispatcher, binding, &message)? {
            DelegatedMessageProcessOutcome::Dispatched => dispatched += 1,
            DelegatedMessageProcessOutcome::IgnoredE2ee => ignored_e2ee += 1,
            DelegatedMessageProcessOutcome::SkippedAppControl => skipped_app_control += 1,
            DelegatedMessageProcessOutcome::SkippedProcessed => skipped_processed += 1,
            DelegatedMessageProcessOutcome::SkippedUnsupported => skipped_unsupported += 1,
        }
    }
    state.upsert_inbox_cursor(&InboxCursorRecord {
        owner_did: binding.user_did.clone(),
        inbox_scope: inbox_scope.clone(),
        cursor: page.next_cursor.clone(),
        updated_at_ms: 0,
    })?;
    Ok(ProcessUserDelegatedInboxOutcome {
        binding_id: binding.binding_id.clone(),
        fetched_messages: fetched,
        dispatched_messages: dispatched,
        ignored_e2ee_messages: ignored_e2ee,
        skipped_app_control_messages: skipped_app_control,
        skipped_unsupported_messages: skipped_unsupported,
        skipped_processed_messages: skipped_processed,
        next_cursor: page.next_cursor,
    })
}

fn processed_message_is_terminal(
    state: &DaemonState,
    owner_did: &str,
    message_id: &str,
) -> Result<bool> {
    Ok(state
        .load_processed_message(owner_did, message_id)?
        .is_some_and(|record| {
            matches!(
                record.status.as_str(),
                PROCESSED_STATUS_DISPATCHED
                    | PROCESSED_STATUS_IGNORED_E2EE
                    | PROCESSED_STATUS_SKIPPED_APP_CONTROL
                    | PROCESSED_STATUS_SKIPPED_UNSUPPORTED
            )
        }))
}

fn process_user_delegated_message_for_binding<D>(
    state: &DaemonState,
    dispatcher: &D,
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Result<DelegatedMessageProcessOutcome>
where
    D: UserDelegatedMessageDispatcher,
{
    let source_message_id = message.id.as_str().to_string();
    if is_bound_agent_control_message(binding, message) || is_app_recovery_control_message(message)
    {
        return process_app_recovery_control_message(state, binding, message);
    }
    if is_e2ee_opaque_message(message) {
        let inserted = state.try_insert_processed_message(&ProcessedMessageRecord {
            owner_did: binding.user_did.clone(),
            message_id: source_message_id.clone(),
            schema: EVENT_SCHEMA_E2EE_OPAQUE.to_string(),
            processed_at_ms: 0,
            status: PROCESSED_STATUS_IGNORED_E2EE.to_string(),
        })?;
        if inserted {
            state.upsert_message_event(&ignored_e2ee_event(binding, message)?)?;
            return Ok(DelegatedMessageProcessOutcome::IgnoredE2ee);
        }
        return Ok(DelegatedMessageProcessOutcome::SkippedProcessed);
    }
    let Some(dispatch_content) = dispatch_content_for_agent(binding, message) else {
        return process_unsupported_message_for_binding(state, binding, message);
    };
    let processed_message_id =
        processed_message_id_for_dispatch(binding, &source_message_id, &dispatch_content);
    if processed_message_is_terminal(state, &binding.user_did, &processed_message_id)? {
        return Ok(DelegatedMessageProcessOutcome::SkippedProcessed);
    }
    let inserted = state.try_insert_processed_message(&processing_record(
        binding,
        &processed_message_id,
        EVENT_SCHEMA_USER_MESSAGE,
    ))?;
    if !inserted && processed_message_is_terminal(state, &binding.user_did, &processed_message_id)?
    {
        return Ok(DelegatedMessageProcessOutcome::SkippedProcessed);
    }
    if !inserted {
        state.mark_processed_message_status(
            &binding.user_did,
            &processed_message_id,
            PROCESSED_STATUS_PROCESSING,
        )?;
    }
    let envelope = user_message_envelope(binding, message, dispatch_content)?;
    let task = runtime_task_from_envelope(state, binding, &envelope)?;
    if let Err(error) = dispatcher.dispatch_user_message(binding, task, &envelope) {
        state.mark_processed_message_status(
            &binding.user_did,
            &processed_message_id,
            PROCESSED_STATUS_FAILED_RETRYABLE,
        )?;
        return Err(error);
    }
    let event = message_event_from_envelope(binding, message, &envelope)?;
    state.upsert_message_event(&event)?;
    state.mark_processed_message_status(
        &binding.user_did,
        &processed_message_id,
        PROCESSED_STATUS_DISPATCHED,
    )?;
    state.upsert_message_sync_outbox(&message_sync_outbox_record(binding, &envelope)?)?;
    Ok(DelegatedMessageProcessOutcome::Dispatched)
}

fn process_app_recovery_control_message(
    state: &DaemonState,
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Result<DelegatedMessageProcessOutcome> {
    let source_message_id = message.id.as_str().to_string();
    if processed_message_is_terminal(state, &binding.user_did, &source_message_id)? {
        return Ok(DelegatedMessageProcessOutcome::SkippedProcessed);
    }
    let inserted = state.try_insert_processed_message(&ProcessedMessageRecord {
        owner_did: binding.user_did.clone(),
        message_id: source_message_id.clone(),
        schema: app_recovery_control_schema(message)
            .unwrap_or("awiki.app_control.unknown.v1")
            .to_string(),
        processed_at_ms: 0,
        status: PROCESSED_STATUS_SKIPPED_APP_CONTROL.to_string(),
    })?;
    if !inserted {
        state.mark_processed_message_status(
            &binding.user_did,
            &source_message_id,
            PROCESSED_STATUS_SKIPPED_APP_CONTROL,
        )?;
    }
    Ok(DelegatedMessageProcessOutcome::SkippedAppControl)
}

fn process_unsupported_message_for_binding(
    state: &DaemonState,
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Result<DelegatedMessageProcessOutcome> {
    let source_message_id = message.id.as_str().to_string();
    if processed_message_is_terminal(state, &binding.user_did, &source_message_id)? {
        return Ok(DelegatedMessageProcessOutcome::SkippedProcessed);
    }
    let inserted = state.try_insert_processed_message(&ProcessedMessageRecord {
        owner_did: binding.user_did.clone(),
        message_id: source_message_id.clone(),
        schema: unsupported_message_schema(message),
        processed_at_ms: 0,
        status: PROCESSED_STATUS_SKIPPED_UNSUPPORTED.to_string(),
    })?;
    if !inserted {
        state.mark_processed_message_status(
            &binding.user_did,
            &source_message_id,
            PROCESSED_STATUS_SKIPPED_UNSUPPORTED,
        )?;
    }
    let reason = unsupported_reason(message);
    state.upsert_message_event(&unsupported_message_event(binding, message, reason)?)?;
    state.upsert_message_sync_outbox(&unsupported_message_sync_outbox_record(
        binding, message, reason,
    )?)?;
    Ok(DelegatedMessageProcessOutcome::SkippedUnsupported)
}

fn processing_record(
    binding: &AppMessageAgentBindingRecord,
    message_id: &str,
    schema: &str,
) -> ProcessedMessageRecord {
    ProcessedMessageRecord {
        owner_did: binding.user_did.clone(),
        message_id: message_id.to_string(),
        schema: schema.to_string(),
        processed_at_ms: 0,
        status: PROCESSED_STATUS_PROCESSING.to_string(),
    }
}

fn user_message_envelope(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
    dispatch_content: AgentDispatchContent,
) -> Result<UserMessageEnvelope> {
    let content_hash = content_hash(&dispatch_content.text);
    Ok(UserMessageEnvelope {
        schema: EVENT_SCHEMA_USER_MESSAGE.to_string(),
        content_role: "user_message_untrusted".to_string(),
        source_message_id: message.id.as_str().to_string(),
        source_conversation_id: conversation_id(message),
        source_sender_did: message.sender.as_str().to_string(),
        source_sender_user_id: message_metadata_attribute(message, "source_sender_user_id")
            .or_else(|| message_metadata_attribute(message, "sender_user_id"))
            .or_else(|| message_metadata_attribute(message, "peer_user_id")),
        source_sender_full_handle: message_metadata_attribute(message, "source_sender_full_handle")
            .or_else(|| message_metadata_attribute(message, "sender_full_handle"))
            .or_else(|| message_metadata_attribute(message, "peer_full_handle")),
        inbox_owner_did: binding.user_did.clone(),
        message_kind: dispatch_content.message_kind.to_string(),
        received_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone()),
        content_text: dispatch_content.text,
        content_hash,
        allowed_actions: allowed_actions(binding),
    })
}

fn runtime_task_from_envelope(
    state: &DaemonState,
    binding: &AppMessageAgentBindingRecord,
    envelope: &UserMessageEnvelope,
) -> Result<RuntimeTask> {
    let profile = state.load_runtime_agent_profile(&binding.runtime_agent_did)?;
    let payload = json!({
        "schema": "awiki.runtime.user_message_task.v1",
        "content_role": envelope.content_role,
        "source_message_id": envelope.source_message_id,
        "source_conversation_id": envelope.source_conversation_id,
        "source_sender_did": envelope.source_sender_did,
        "source_sender_user_id": envelope.source_sender_user_id,
        "source_sender_full_handle": envelope.source_sender_full_handle,
        "inbox_owner_did": envelope.inbox_owner_did,
        "message_kind": envelope.message_kind,
        "received_at": envelope.received_at,
        "content_text": envelope.content_text,
        "content_hash": envelope.content_hash,
        "allowed_actions": envelope.allowed_actions,
    });
    let text = serde_json::to_string(&payload)?;
    let controller_did = profile.controller_did.clone();
    let requester_did = envelope.source_sender_did.clone();
    let requester_user_id = envelope
        .source_sender_user_id
        .clone()
        .with_context(|| "delegated direct message is missing source_sender_user_id")?;
    let requester_full_handle = envelope
        .source_sender_full_handle
        .clone()
        .with_context(|| "delegated direct message is missing source_sender_full_handle")?;
    let task_key = format!("{}:{}", binding.user_did, envelope.source_message_id);
    let task = RuntimeTask {
        task_id: format!("task_user_msg_{}", stable_id_suffix(&task_key)),
        agent_did: binding.runtime_agent_did.clone(),
        agent_handle: profile.agent_handle,
        controller_user_id: profile.controller_user_id,
        controller_full_handle: profile.controller_full_handle,
        controller_scope_key: profile.controller_scope_key,
        controller_did,
        sender_did: requester_did.clone(),
        requester_did: requester_did.clone(),
        requester_user_id: Some(requester_user_id.clone()),
        requester_full_handle: Some(requester_full_handle.clone()),
        trigger_kind: RuntimeTaskTriggerKind::DelegatedDirect,
        conversation_scope: RuntimeConversationScope::direct(
            requester_user_id,
            requester_full_handle,
        )?,
        invocation_authority: RuntimeInvocationAuthority::Requester,
        reply_recipient_did: binding.user_did.clone(),
        conversation_id: envelope.source_conversation_id.clone(),
        text,
    };
    task.validate()?;
    Ok(task)
}

fn message_event_from_envelope(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
    envelope: &UserMessageEnvelope,
) -> Result<MessageEventRecord> {
    let record = MessageEventRecord {
        event_id: event_id(&binding.user_did, &envelope.source_message_id),
        owner_did: binding.user_did.clone(),
        conversation_id: envelope.source_conversation_id.clone(),
        message_id: envelope.source_message_id.clone(),
        message_kind: envelope.message_kind.clone(),
        sender_did: envelope.source_sender_did.clone(),
        received_at: envelope.received_at.clone(),
        plain_text_ref_or_excerpt: Some(short_excerpt(&envelope.content_text)),
        content_hash: envelope.content_hash.clone(),
        schema: EVENT_SCHEMA_USER_MESSAGE.to_string(),
        processing_status: MESSAGE_EVENT_STATUS_DISPATCHED.to_string(),
        retention_class: RETENTION_CLASS_SHORT_EXCERPT.to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    record
        .validate()
        .with_context(|| format!("build message event for {}", message.id.as_str()))?;
    Ok(record)
}

fn ignored_e2ee_event(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Result<MessageEventRecord> {
    let message_id = message.id.as_str().to_string();
    Ok(MessageEventRecord {
        event_id: event_id(&binding.user_did, &message_id),
        owner_did: binding.user_did.clone(),
        conversation_id: conversation_id(message),
        message_id,
        message_kind: "e2ee_opaque".to_string(),
        sender_did: message.sender.as_str().to_string(),
        received_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone()),
        plain_text_ref_or_excerpt: None,
        content_hash: content_hash("e2ee_opaque"),
        schema: EVENT_SCHEMA_E2EE_OPAQUE.to_string(),
        processing_status: PROCESSED_STATUS_IGNORED_E2EE.to_string(),
        retention_class: RETENTION_CLASS_OPAQUE_ONLY.to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    })
}

fn unsupported_message_event(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
    reason: &str,
) -> Result<MessageEventRecord> {
    let message_id = message.id.as_str().to_string();
    Ok(MessageEventRecord {
        event_id: event_id(&binding.user_did, &message_id),
        owner_did: binding.user_did.clone(),
        conversation_id: conversation_id(message),
        message_id,
        message_kind: reason.to_string(),
        sender_did: message.sender.as_str().to_string(),
        received_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone()),
        plain_text_ref_or_excerpt: None,
        content_hash: content_hash(reason),
        schema: unsupported_message_schema(message),
        processing_status: MESSAGE_EVENT_STATUS_SKIPPED_UNSUPPORTED.to_string(),
        retention_class: RETENTION_CLASS_OPAQUE_ONLY.to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    })
}

fn message_sync_outbox_record(
    binding: &AppMessageAgentBindingRecord,
    envelope: &UserMessageEnvelope,
) -> Result<MessageSyncOutboxRecord> {
    Ok(MessageSyncOutboxRecord {
        idempotency_key: message_sync_idempotency_key(binding, &envelope.source_message_id),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": MESSAGE_SYNC_SCHEMA,
            "message_id": envelope.source_message_id,
            "conversation_id": envelope.source_conversation_id,
            "sender_did": envelope.source_sender_did,
            "sender_user_id": envelope.source_sender_user_id,
            "sender_full_handle": envelope.source_sender_full_handle,
            "owner_did": envelope.inbox_owner_did,
            "processing_status": MESSAGE_EVENT_STATUS_DISPATCHED,
            "content_role": envelope.content_role,
            "content_hash": envelope.content_hash,
            "retention_class": RETENTION_CLASS_SHORT_EXCERPT,
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })
}

fn unsupported_message_sync_outbox_record(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
    reason: &str,
) -> Result<MessageSyncOutboxRecord> {
    let message_id = message.id.as_str().to_string();
    Ok(MessageSyncOutboxRecord {
        idempotency_key: message_sync_idempotency_key(binding, &message_id),
        owner_did: binding.user_did.clone(),
        app_instance_id: binding.app_instance_id.clone(),
        payload_json: json!({
            "schema": MESSAGE_SYNC_SCHEMA,
            "message_id": message_id,
            "conversation_id": conversation_id(message),
            "sender_did": message.sender.as_str(),
            "owner_did": binding.user_did,
            "processing_status": MESSAGE_EVENT_STATUS_SKIPPED_UNSUPPORTED,
            "unsupported_reason": reason,
            "retention_class": RETENTION_CLASS_OPAQUE_ONLY,
            "content_hash": content_hash(reason),
        }),
        status: "pending".to_string(),
        attempt_count: 0,
        next_attempt_at_ms: 0,
        last_error_code: None,
        last_error_summary: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        sent_at_ms: None,
    })
}

fn dispatch_content_for_agent(
    _binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Option<AgentDispatchContent> {
    if is_group_message(message) {
        return None;
    }
    match &message.body {
        MessageBodyView::Text { text, .. }
            if !text.trim().is_empty() && !is_group_message(message) =>
        {
            Some(AgentDispatchContent {
                text: text.clone(),
                message_kind: "text",
            })
        }
        MessageBodyView::Payload { payload } if is_system_control_payload(payload) => None,
        MessageBodyView::Payload { .. } => None,
        MessageBodyView::Unsupported { .. } => None,
        _ => None,
    }
}

fn processed_message_id_for_dispatch(
    _binding: &AppMessageAgentBindingRecord,
    source_message_id: &str,
    _dispatch_content: &AgentDispatchContent,
) -> String {
    source_message_id.to_string()
}

fn is_group_message(message: &Message) -> bool {
    matches!(message.thread, im_core::messages::ThreadRef::Group(_)) || message.group.is_some()
}

fn message_sync_idempotency_key(
    binding: &AppMessageAgentBindingRecord,
    source_message_id: &str,
) -> String {
    format!(
        "message-sync:{}:{}:{}",
        binding.user_did,
        stable_id_suffix(&binding.runtime_agent_did),
        source_message_id
    )
}

fn inbox_scope_for_binding(binding: &AppMessageAgentBindingRecord) -> String {
    format!(
        "{}:binding:{}:runtime:{}",
        DEFAULT_SCOPE,
        stable_id_suffix(&binding.binding_id),
        stable_id_suffix(&binding.runtime_agent_did),
    )
}

fn is_system_control_payload(payload: &Value) -> bool {
    payload
        .get("schema")
        .and_then(Value::as_str)
        .is_some_and(|schema| schema.starts_with("awiki."))
}

fn is_bound_agent_control_message(
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> bool {
    let sender = message.sender.as_str();
    sender == binding.daemon_agent_did || sender == binding.runtime_agent_did
}

fn is_app_recovery_control_message(message: &Message) -> bool {
    match &message.body {
        MessageBodyView::Payload { payload } => is_app_recovery_control_payload(payload),
        _ => false,
    }
}

fn is_app_recovery_control_payload(payload: &Value) -> bool {
    matches!(
        payload.get("schema").and_then(Value::as_str),
        Some(MESSAGE_SYNC_SCHEMA | APP_ACTION_SCHEMA | APP_ACTION_RESULT_SCHEMA)
    )
}

fn app_recovery_control_schema(message: &Message) -> Option<&str> {
    match &message.body {
        MessageBodyView::Payload { payload } => payload.get("schema").and_then(Value::as_str),
        _ => None,
    }
}

fn unsupported_reason(message: &Message) -> &'static str {
    match &message.body {
        MessageBodyView::Payload { payload } if is_system_control_payload(payload) => {
            "system_control_payload"
        }
        _ if is_group_message(message) => "group_message",
        MessageBodyView::Payload { .. } => "structured_payload",
        MessageBodyView::Unsupported { .. } => "unsupported_body",
        MessageBodyView::Text { .. } => "empty_text",
    }
}

fn unsupported_message_schema(message: &Message) -> String {
    format!("awiki.user_message.{}.v1", unsupported_reason(message))
}

fn is_e2ee_opaque_message(message: &Message) -> bool {
    let content_type = message.metadata.content_type.as_deref().unwrap_or_default();
    if content_type.contains("e2ee")
        || content_type.contains("cipher")
        || content_type == "application/anp-direct-cipher+json"
        || content_type == "application/anp-direct-init+json"
    {
        return true;
    }
    message.metadata.attributes.iter().any(|attribute| {
        let key = attribute.key.to_ascii_lowercase();
        let value = attribute.value.to_ascii_lowercase();
        (key == "security" || key == "security_profile" || key == "message_security_profile")
            && (value.contains("e2ee") || value.contains("secure-direct"))
    })
}

fn message_metadata_attribute(message: &Message, key: &str) -> Option<String> {
    message
        .metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn conversation_id(message: &Message) -> Option<String> {
    match &message.thread {
        im_core::messages::ThreadRef::Direct(peer) => Some(format!("direct:{}", peer.as_str())),
        im_core::messages::ThreadRef::Group(group) => Some(format!("group:{}", group.as_str())),
        im_core::messages::ThreadRef::Thread(thread) => Some(thread.as_str().to_string()),
    }
}

fn allowed_actions(binding: &AppMessageAgentBindingRecord) -> Vec<String> {
    let has_explicit_capability_policy = binding
        .capability_policy_json
        .get("schema")
        .and_then(Value::as_str)
        == Some(crate::app_bridge::action::APP_CAPABILITIES_SCHEMA);
    let configured = if has_explicit_capability_policy {
        binding
            .capability_policy_json
            .get("capabilities")
            .and_then(Value::as_array)
            .or_else(|| {
                binding
                    .capability_policy_json
                    .get("allowed_actions")
                    .and_then(Value::as_array)
            })
    } else {
        binding
            .desired_agent_json
            .get("allowed_actions")
            .and_then(Value::as_array)
    };
    configured
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| MVP_ALLOWED_ACTIONS.contains(value))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn ensure_delegated_inbox_key_ref(
    config: &DaemonConfig,
    identity: &UserDelegatedIdentityRecord,
) -> Result<PathBuf> {
    let dir = delegated_identity_dir(config, &identity.user_did);
    fs::create_dir_all(&dir)?;
    let path = dir.join("daemon-key-1.pem");
    let private_key_pem = normalize_delegated_private_key_pem(&identity.private_key_material)?;
    fs::write(&path, private_key_pem.as_bytes())?;
    set_private_key_file_permissions(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn set_private_key_file_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict delegated key permissions at {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_key_file_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn ensure_delegated_inbox_did_shadow(
    config: &DaemonConfig,
    identity: &UserDelegatedIdentityRecord,
) -> Result<()> {
    let alias = delegated_identity_alias(&identity.user_did);
    let identity_dir = config.identity_root_dir.join(&alias);
    fs::create_dir_all(&identity_dir)?;
    let did_document_path = identity_dir.join("did.json");
    fs::write(
        &did_document_path,
        serde_json::to_vec_pretty(&minimal_user_did_document(identity))?,
    )?;
    let private_key_pem = normalize_delegated_private_key_pem(&identity.private_key_material)?;
    let private_key_path = identity_dir.join("private.key");
    fs::write(&private_key_path, private_key_pem.as_bytes())?;
    set_private_key_file_permissions(&private_key_path)?;
    let auth_path = identity_dir.join("auth.json");
    if !auth_path.exists() {
        fs::write(&auth_path, "{}")?;
    }
    let mut registry = read_identity_registry(config)?;
    let identities = registry
        .as_object_mut()
        .and_then(|object| object.get_mut("identities"))
        .and_then(Value::as_array_mut)
        .context("identity registry must contain identities array")?;
    let exists = identities.iter().any(|entry| {
        entry
            .get("did")
            .and_then(Value::as_str)
            .is_some_and(|did| did == identity.user_did)
    });
    if !exists {
        identities.push(json!({
            "id": alias,
            "did": identity.user_did,
            "dir_name": alias,
            "local_alias": alias,
            "ready_for_auth": true,
            "ready_for_messaging": true,
            "missing": []
        }));
        if let Some(parent) = config.identity_registry_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &config.identity_registry_path,
            serde_json::to_vec_pretty(&registry)?,
        )?;
    }
    Ok(())
}

fn read_identity_registry(config: &DaemonConfig) -> Result<Value> {
    if !config.identity_registry_path.exists() {
        return Ok(json!({
            "default_identity": "",
            "identities": []
        }));
    }
    let raw = fs::read(&config.identity_registry_path)?;
    let mut value: Value = serde_json::from_slice(&raw)?;
    if !value.is_object() {
        value = json!({});
    }
    if value.get("identities").is_none() {
        value["identities"] = json!([]);
    }
    Ok(value)
}

fn minimal_user_did_document(identity: &UserDelegatedIdentityRecord) -> Value {
    json!({
        "id": identity.user_did,
        "verificationMethod": [{
            "id": identity.verification_method,
            "type": "Multikey",
            "controller": identity.user_did,
            "publicKeyMultibase": identity.public_key_multibase
        }],
        "authentication": [identity.verification_method]
    })
}

fn delegated_identity_dir(config: &DaemonConfig, user_did: &str) -> PathBuf {
    config
        .runtime_cache_dir
        .join("delegated-inbox")
        .join(stable_id_suffix(user_did))
}

fn delegated_identity_alias(user_did: &str) -> String {
    format!("delegated-inbox-{}", stable_id_suffix(user_did))
}

fn event_id(owner_did: &str, message_id: &str) -> String {
    format!(
        "evt_{}",
        stable_id_suffix(&format!("{owner_did}:{message_id}"))
    )
}

fn content_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("sha256:{digest:x}")
}

fn stable_id_suffix(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn short_excerpt(text: &str) -> String {
    text.chars().take(EXCERPT_MAX_CHARS).collect()
}

fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
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
    if sanitized.chars().count() > 240 {
        sanitized = sanitized.chars().take(240).collect();
    }
    sanitized
}

#[cfg(test)]
mod tests;
