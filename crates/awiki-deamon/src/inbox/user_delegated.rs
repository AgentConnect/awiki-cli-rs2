use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use im_core::messages::{
    parse_message_mention_payload, InboxHistoryOptions, InboxQuery, InboxScope, Message,
    MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageMention,
    MessageMentionPayload, MessageMentionRole, MessageMentionSelector, MessageMentionTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app_bridge::action::MVP_ALLOWED_ACTIONS;
use crate::app_bridge::bootstrap::{
    validate_user_delegated_identity_against_did_document, BootstrapDidDocumentResolver,
    DefaultBootstrapDidDocumentResolver,
};
use crate::app_bridge::message_agent::APP_MESSAGE_HANDLER_ROLE;
use crate::app_bridge::secret_store::normalize_delegated_private_key_pem;
use crate::controller_scope::daemon_auth_material;
use crate::im_core_adapter::ImCoreAdapter;
use crate::outbox::{
    ImCoreAgentOutbox, RuntimeAttachmentSend, RuntimeAttachmentSendResult, RuntimeMessageSend,
    RuntimeMessageSendResult, RuntimeOutbox,
};
use crate::plugins::generic_cli::{GenericCliDriverRegistry, GENERIC_CLI_RUNTIME_PLUGIN_ID};
use crate::plugins::hermes::{HermesRuntimePlugin, StdioHermesGateway, HERMES_RUNTIME_PLUGIN_ID};
use crate::registration::{
    AgentInventoryClient, AgentInvocationAuthorization, UserServiceAgentRegistrationClient,
};
use crate::runtime::host::run_existing_runtime_task_with_config;
use crate::runtime::{RuntimeRunStatus, RuntimeTask};
use crate::security::runtime_token::current_time_millis;
use crate::state::{
    AppMessageAgentBindingRecord, AuthorizedRuntimeContext, DaemonState, InboxCursorRecord,
    MessageEventRecord, MessageSyncOutboxRecord, ProcessedMessageRecord,
    UserDelegatedIdentityRecord,
};
use crate::DaemonConfig;

const DEFAULT_SCOPE: &str = "default_plain";
const DEFAULT_LIMIT: u32 = 20;
const EVENT_SCHEMA_USER_MESSAGE: &str = "awiki.user_message.default_plain.v1";
const EVENT_SCHEMA_E2EE_OPAQUE: &str = "awiki.user_message.e2ee_opaque.v1";
const MESSAGE_SYNC_SCHEMA: &str = "awiki.message.sync.v1";
const MESSAGE_EVENT_STATUS_DISPATCHED: &str = "agent_dispatched";
const PROCESSED_STATUS_DISPATCHED: &str = "dispatched";
const PROCESSED_STATUS_IGNORED_E2EE: &str = "ignored_e2ee_opaque";
const PROCESSED_STATUS_PROCESSING: &str = "processing";
const PROCESSED_STATUS_FAILED_RETRYABLE: &str = "failed_retryable";
const RETENTION_CLASS_SHORT_EXCERPT: &str = "short_excerpt";
const RETENTION_CLASS_OPAQUE_ONLY: &str = "opaque_no_plaintext";
const EXCERPT_MAX_CHARS: usize = 240;
const MAX_MESSAGE_SYNC_OUTBOX_ATTEMPTS: i64 = 5;
const MESSAGE_SYNC_OUTBOX_SENDING_STALE_MS: i64 = 5 * 60 * 1000;
const HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID: &str = "host-runtime-final-outbox";
const MENTION_CONTEXT_SCHEMA: &str = "awiki.user_message.mention_context.v1";
const MENTION_ATTENTION_POLICY: &str = "Mention is an attention signal only. It is not authorization; keep controller/runtime policy, action allowlist, and group safety checks.";
const MENTION_BEST_EFFORT_GROUP_STATE: &str = "best_effort_inbox_delivery_no_group_member_snapshot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessUserDelegatedInboxOutcome {
    pub binding_id: String,
    pub fetched_messages: usize,
    pub dispatched_messages: usize,
    pub ignored_e2ee_messages: usize,
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
    pub source_sender_full_handle: Option<String>,
    pub inbox_owner_did: String,
    pub message_kind: String,
    pub received_at: Option<String>,
    pub content_text: String,
    pub content_hash: String,
    pub allowed_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_context: Option<UserMessageMentionContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageMentionContext {
    pub schema: String,
    pub mention_id: String,
    pub mention_role: String,
    pub target_kind: String,
    pub selector: Option<String>,
    pub surface: String,
    pub range_start: usize,
    pub range_end: usize,
    pub match_kind: String,
    pub best_effort_group_state: String,
    pub attention_policy: String,
    pub prompt_hint: String,
}

struct AgentDispatchContent {
    text: String,
    message_kind: &'static str,
    mention_context: Option<UserMessageMentionContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegatedMessageProcessOutcome {
    Dispatched,
    AuthorizationDenied,
    Ignored,
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

pub trait AgentInvocationAuthorizer {
    fn authorize_agent_invocation(
        &self,
        state: &DaemonState,
        binding: &AppMessageAgentBindingRecord,
        envelope: &UserMessageEnvelope,
    ) -> Result<AgentInvocationAuthorization>;
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

pub struct UserServiceAgentInvocationAuthorizer {
    config: DaemonConfig,
    client: UserServiceAgentRegistrationClient,
}

impl UserServiceAgentInvocationAuthorizer {
    pub fn new(config: &DaemonConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            client: UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?,
        })
    }
}

impl AgentInvocationAuthorizer for UserServiceAgentInvocationAuthorizer {
    fn authorize_agent_invocation(
        &self,
        state: &DaemonState,
        binding: &AppMessageAgentBindingRecord,
        envelope: &UserMessageEnvelope,
    ) -> Result<AgentInvocationAuthorization> {
        let daemon = state.load_agent_definition(&binding.daemon_agent_did)?;
        let auth = daemon_auth_material(&self.config, state, &daemon)?;
        self.client.authorize_agent_invocation(
            &binding.daemon_agent_did,
            &binding.runtime_agent_did,
            &envelope.source_sender_did,
            envelope.source_conversation_id.as_deref(),
            Some(&envelope.source_message_id),
            &auth,
        )
    }
}

pub struct RuntimeHostMessageDispatcher<'a> {
    config: &'a DaemonConfig,
    state: &'a DaemonState,
    im_core: &'a ImCoreAdapter,
}

impl<'a> RuntimeHostMessageDispatcher<'a> {
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

impl UserDelegatedMessageDispatcher for RuntimeHostMessageDispatcher<'_> {
    fn dispatch_user_message(
        &self,
        _binding: &AppMessageAgentBindingRecord,
        task: RuntimeTask,
        _envelope: &UserMessageEnvelope,
    ) -> Result<()> {
        let profile = self.state.load_runtime_agent_profile(&task.agent_did)?;
        let run_id = delegated_runtime_run_id(self.state, &task.task_id)?;
        let outbox = UserDelegatedRuntimeOutbox::new(self.config, self.state, self.im_core);
        match profile.runtime_plugin_id.as_str() {
            HERMES_RUNTIME_PLUGIN_ID => {
                let hermes_profile = self.state.load_hermes_profile(&profile.agent_did)?;
                let plugin = HermesRuntimePlugin::with_state(
                    StdioHermesGateway::from_config(self.config),
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
    config: &'a DaemonConfig,
    state: &'a DaemonState,
    im_core: Option<&'a ImCoreAdapter>,
}

impl<'a> UserDelegatedRuntimeOutbox<'a> {
    fn new(config: &'a DaemonConfig, state: &'a DaemonState, im_core: &'a ImCoreAdapter) -> Self {
        Self {
            config,
            state,
            im_core: Some(im_core),
        }
    }

    #[cfg(test)]
    fn new_for_test(config: &'a DaemonConfig, state: &'a DaemonState) -> Self {
        Self {
            config,
            state,
            im_core: None,
        }
    }

    fn send_host_runtime_final_sync(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        if message.target_kind() == "group" {
            return self.send_group_runtime_message(context, message);
        }
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

    fn send_group_runtime_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let binding = self
            .state
            .load_active_app_message_agent_binding_by_runtime(&context.agent_did)?
            .with_context(|| {
                format!(
                    "missing app message binding for group runtime final {}",
                    context.agent_did
                )
            })?;
        let daemon_identity = self.state.load_agent_identity(&binding.daemon_agent_did)?;
        let jwt_token = self
            .state
            .load_agent_auth_token(&binding.daemon_agent_did)?;
        let im_core = self
            .im_core
            .context("group runtime outbox requires im-core adapter")?;
        let client = im_core.client_for_agent_identity(
            self.config,
            &daemon_identity,
            jwt_token.as_deref(),
        )?;
        let outbox = ImCoreAgentOutbox::new(client);
        let result = outbox.send_runtime_message(message.clone())?;
        self.state.insert_audit_event_json(
            "user_delegated_inbox.runtime_final.group_sent",
            Some(&context.agent_did),
            Some(&context.runtime_profile_id),
            Some(&context.run_id),
            Some(&context.token_id),
            json!({
                "binding_id": &binding.binding_id,
                "app_instance_id": &binding.app_instance_id,
                "target_kind": message.target_kind(),
                "group": message.resolved_recipient(),
                "security": message.security.as_str(),
                "has_payload": message.payload.is_some(),
                "has_text": !message.text.trim().is_empty(),
                "text_hash": content_hash(&message.text),
                "message_id": result.message.id.as_str(),
            }),
        )?;
        Ok(RuntimeMessageSendResult {
            message_id: Some(result.message.id.as_str().to_string()),
            raw_recipient: message.raw_recipient().to_string(),
            resolved_did: message.resolved_recipient().to_string(),
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
        queue_runtime_status_sync(
            self.state,
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
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
) -> Result<usize> {
    let client = ImCoreDelegatedInboxClient::new(config, state, im_core);
    let dispatcher = RuntimeHostMessageDispatcher::new(config, state, im_core);
    let authorizer = UserServiceAgentInvocationAuthorizer::new(config)?;
    process_user_delegated_inbox_once_with_authorizer(state, &client, &dispatcher, &authorizer)
}

fn process_user_delegated_inbox_once_with_authorizer<C, D, A>(
    state: &DaemonState,
    client: &C,
    dispatcher: &D,
    authorizer: &A,
) -> Result<usize>
where
    C: UserDelegatedInboxClient,
    D: UserDelegatedMessageDispatcher,
    A: AgentInvocationAuthorizer,
{
    let mut processed = 0usize;
    for binding in state
        .list_active_app_message_agent_bindings()?
        .into_iter()
        .filter(|binding| binding.role == APP_MESSAGE_HANDLER_ROLE)
    {
        match process_user_delegated_inbox_for_binding_with_authorizer(
            state, client, dispatcher, authorizer, &binding,
        ) {
            Ok(outcome) => {
                processed += outcome.dispatched_messages + outcome.ignored_e2ee_messages;
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
    let authorizer = AllowAllAgentInvocationAuthorizer;
    process_user_delegated_inbox_for_binding_with_authorizer(
        state,
        client,
        dispatcher,
        &authorizer,
        binding,
    )
}

pub fn process_user_delegated_inbox_for_binding_with_authorizer<C, D, A>(
    state: &DaemonState,
    client: &C,
    dispatcher: &D,
    authorizer: &A,
    binding: &AppMessageAgentBindingRecord,
) -> Result<ProcessUserDelegatedInboxOutcome>
where
    C: UserDelegatedInboxClient,
    D: UserDelegatedMessageDispatcher,
    A: AgentInvocationAuthorizer,
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
    let mut skipped_processed = 0usize;
    let fetched = page.messages.len();
    for message in page.messages {
        if message.direction == MessageDirection::Outgoing {
            continue;
        }
        match process_user_delegated_message_for_binding_with_authorizer(
            state, dispatcher, authorizer, binding, &message,
        )? {
            DelegatedMessageProcessOutcome::Dispatched => dispatched += 1,
            DelegatedMessageProcessOutcome::IgnoredE2ee => ignored_e2ee += 1,
            DelegatedMessageProcessOutcome::SkippedProcessed => skipped_processed += 1,
            DelegatedMessageProcessOutcome::AuthorizationDenied
            | DelegatedMessageProcessOutcome::Ignored => {}
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
        skipped_processed_messages: skipped_processed,
        next_cursor: page.next_cursor,
    })
}

pub fn process_user_delegated_group_message_for_runtime<D>(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent_did: &str,
    message: &Message,
    dispatcher: &D,
) -> Result<bool>
where
    D: UserDelegatedMessageDispatcher,
{
    let authorizer = UserServiceAgentInvocationAuthorizer::new(config)?;
    process_user_delegated_group_message_for_runtime_with_authorizer(
        state,
        runtime_agent_did,
        message,
        dispatcher,
        &authorizer,
    )
}

fn process_user_delegated_group_message_for_runtime_with_authorizer<D, A>(
    state: &DaemonState,
    runtime_agent_did: &str,
    message: &Message,
    dispatcher: &D,
    authorizer: &A,
) -> Result<bool>
where
    D: UserDelegatedMessageDispatcher,
    A: AgentInvocationAuthorizer,
{
    if !is_group_message(message) || message.direction == MessageDirection::Outgoing {
        return Ok(false);
    }
    let Some(binding) =
        state.load_active_app_message_agent_binding_by_runtime(runtime_agent_did)?
    else {
        return Ok(false);
    };
    if binding.role != APP_MESSAGE_HANDLER_ROLE {
        return Ok(false);
    }
    let outcome = process_user_delegated_message_for_binding_with_authorizer(
        state, dispatcher, authorizer, &binding, message,
    )?;
    Ok(matches!(
        outcome,
        DelegatedMessageProcessOutcome::Dispatched
            | DelegatedMessageProcessOutcome::AuthorizationDenied
            | DelegatedMessageProcessOutcome::IgnoredE2ee
            | DelegatedMessageProcessOutcome::SkippedProcessed
    ))
}

struct AllowAllAgentInvocationAuthorizer;

impl AgentInvocationAuthorizer for AllowAllAgentInvocationAuthorizer {
    fn authorize_agent_invocation(
        &self,
        _state: &DaemonState,
        binding: &AppMessageAgentBindingRecord,
        envelope: &UserMessageEnvelope,
    ) -> Result<AgentInvocationAuthorization> {
        Ok(AgentInvocationAuthorization {
            allowed: true,
            reason: "test_allow_all".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            sender_did: envelope.source_sender_did.clone(),
            sender_full_handle: None,
            active_mode: "whitelist".to_string(),
        })
    }
}

fn authorize_group_invocation<A>(
    state: &DaemonState,
    authorizer: &A,
    binding: &AppMessageAgentBindingRecord,
    envelope: &UserMessageEnvelope,
) -> Result<AgentInvocationAuthorization>
where
    A: AgentInvocationAuthorizer,
{
    authorizer.authorize_agent_invocation(state, binding, envelope)
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
                PROCESSED_STATUS_DISPATCHED | PROCESSED_STATUS_IGNORED_E2EE
            )
        }))
}

fn process_user_delegated_message_for_binding_with_authorizer<D, A>(
    state: &DaemonState,
    dispatcher: &D,
    authorizer: &A,
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Result<DelegatedMessageProcessOutcome>
where
    D: UserDelegatedMessageDispatcher,
    A: AgentInvocationAuthorizer,
{
    let source_message_id = message.id.as_str().to_string();
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
        return Ok(DelegatedMessageProcessOutcome::Ignored);
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
    let mut envelope = user_message_envelope(binding, message, dispatch_content)?;
    if envelope.mention_context.is_some() {
        match authorize_group_invocation(state, authorizer, binding, &envelope) {
            Ok(authorization) if authorization.allowed => {
                envelope.source_sender_full_handle = authorization.sender_full_handle;
            }
            Ok(authorization) => {
                state.insert_audit_event_json(
                    "user_delegated_inbox.mention_authorization_denied",
                    Some(&binding.runtime_agent_did),
                    Some(&binding.runtime_profile_id),
                    None,
                    None,
                    json!({
                        "binding_id": binding.binding_id,
                        "owner_did": binding.user_did,
                        "runtime_agent_did": binding.runtime_agent_did,
                        "source_message_id": envelope.source_message_id,
                        "source_conversation_id": envelope.source_conversation_id,
                        "source_sender_did": envelope.source_sender_did,
                        "reason": authorization.reason,
                        "active_mode": authorization.active_mode,
                        "sender_full_handle": authorization.sender_full_handle,
                    }),
                )?;
                state.mark_processed_message_status(
                    &binding.user_did,
                    &processed_message_id,
                    PROCESSED_STATUS_DISPATCHED,
                )?;
                return Ok(DelegatedMessageProcessOutcome::AuthorizationDenied);
            }
            Err(error) => {
                state.insert_audit_event_json(
                    "user_delegated_inbox.mention_authorization_failed",
                    Some(&binding.runtime_agent_did),
                    Some(&binding.runtime_profile_id),
                    None,
                    None,
                    json!({
                        "binding_id": binding.binding_id,
                        "owner_did": binding.user_did,
                        "runtime_agent_did": binding.runtime_agent_did,
                        "source_message_id": envelope.source_message_id,
                        "source_conversation_id": envelope.source_conversation_id,
                        "source_sender_did": envelope.source_sender_did,
                        "error": sanitize_error_message(&error.to_string()),
                    }),
                )?;
                state.mark_processed_message_status(
                    &binding.user_did,
                    &processed_message_id,
                    PROCESSED_STATUS_FAILED_RETRYABLE,
                )?;
                return Err(error);
            }
        }
    }
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
    if let Some(mention_context) = &envelope.mention_context {
        state.insert_audit_event_json(
            "user_delegated_inbox.mention_matched",
            Some(&binding.runtime_agent_did),
            Some(&binding.runtime_profile_id),
            None,
            None,
            json!({
                "binding_id": binding.binding_id,
                "owner_did": binding.user_did,
                "runtime_agent_did": binding.runtime_agent_did,
                "source_message_id": envelope.source_message_id,
                "source_conversation_id": envelope.source_conversation_id,
                "source_sender_did": envelope.source_sender_did,
                "mention_id": mention_context.mention_id,
                "mention_role": mention_context.mention_role,
                "target_kind": mention_context.target_kind,
                "selector": mention_context.selector,
                "match_kind": mention_context.match_kind,
                "best_effort_group_state": mention_context.best_effort_group_state,
            }),
        )?;
    }
    Ok(DelegatedMessageProcessOutcome::Dispatched)
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
        source_sender_full_handle: None,
        inbox_owner_did: binding.user_did.clone(),
        message_kind: dispatch_content.message_kind.to_string(),
        received_at: message
            .received_at
            .clone()
            .or_else(|| message.sent_at.clone()),
        content_text: dispatch_content.text,
        content_hash,
        allowed_actions: allowed_actions(binding),
        mention_context: dispatch_content.mention_context,
    })
}

fn runtime_task_from_envelope(
    state: &DaemonState,
    binding: &AppMessageAgentBindingRecord,
    envelope: &UserMessageEnvelope,
) -> Result<RuntimeTask> {
    let profile = state.load_runtime_agent_profile(&binding.runtime_agent_did)?;
    let mut payload = json!({
        "schema": "awiki.runtime.user_message_task.v1",
        "content_role": envelope.content_role,
        "source_message_id": envelope.source_message_id,
        "source_conversation_id": envelope.source_conversation_id,
        "source_sender_did": envelope.source_sender_did,
        "source_sender_full_handle": envelope.source_sender_full_handle,
        "inbox_owner_did": envelope.inbox_owner_did,
        "message_kind": envelope.message_kind,
        "received_at": envelope.received_at,
        "content_text": envelope.content_text,
        "content_hash": envelope.content_hash,
        "allowed_actions": envelope.allowed_actions,
    });
    if let Some(mention_context) = &envelope.mention_context {
        payload["mention_context"] = serde_json::to_value(mention_context)?;
        payload["attention_policy"] = json!(mention_context.attention_policy);
    }
    let text = serde_json::to_string(&payload)?;
    let controller_did = profile.controller_did.clone();
    let task_key = match &envelope.mention_context {
        Some(mention_context) => format!(
            "{}:{}:{}:{}",
            binding.user_did,
            binding.runtime_agent_did,
            envelope.source_message_id,
            mention_context.mention_id
        ),
        None => format!("{}:{}", binding.user_did, envelope.source_message_id),
    };
    let sender_did = if envelope.mention_context.is_some()
        && is_group_conversation_id(envelope.source_conversation_id.as_deref())
    {
        envelope.source_sender_did.clone()
    } else {
        profile.controller_did.clone()
    };
    let task = RuntimeTask {
        task_id: format!("task_user_msg_{}", stable_id_suffix(&task_key)),
        agent_did: binding.runtime_agent_did.clone(),
        controller_user_id: profile.controller_user_id,
        controller_full_handle: profile.controller_full_handle,
        controller_scope_key: profile.controller_scope_key,
        controller_did,
        sender_did,
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
            "sender_full_handle": envelope.source_sender_full_handle,
            "owner_did": envelope.inbox_owner_did,
            "processing_status": MESSAGE_EVENT_STATUS_DISPATCHED,
            "content_role": envelope.content_role,
            "content_hash": envelope.content_hash,
            "retention_class": RETENTION_CLASS_SHORT_EXCERPT,
            "mention_context": envelope.mention_context,
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
    binding: &AppMessageAgentBindingRecord,
    message: &Message,
) -> Option<AgentDispatchContent> {
    if is_group_message(message) && is_agent_sender(message) {
        return None;
    }
    match &message.body {
        MessageBodyView::Text { text, .. }
            if !text.trim().is_empty() && !is_group_message(message) =>
        {
            Some(AgentDispatchContent {
                text: text.clone(),
                message_kind: "text",
                mention_context: None,
            })
        }
        MessageBodyView::Payload { payload } if is_system_control_payload(payload) => None,
        MessageBodyView::Payload { payload } if is_group_message(message) => {
            let mention_payload = parse_message_mention_payload(payload).ok()?;
            let mention_context = mention_context_for_runtime(binding, &mention_payload)?;
            Some(AgentDispatchContent {
                text: mention_payload.text,
                message_kind: "group_mention",
                mention_context: Some(mention_context),
            })
        }
        MessageBodyView::Payload { .. } => None,
        MessageBodyView::Unsupported { .. } => None,
        _ => None,
    }
}

fn mention_context_for_runtime(
    binding: &AppMessageAgentBindingRecord,
    payload: &MessageMentionPayload,
) -> Option<UserMessageMentionContext> {
    payload
        .mentions
        .iter()
        .filter_map(|mention| mention_context_for_mention(binding, payload, mention))
        .max_by_key(mention_context_priority)
}

fn mention_context_for_mention(
    binding: &AppMessageAgentBindingRecord,
    payload: &MessageMentionPayload,
    mention: &MessageMention,
) -> Option<UserMessageMentionContext> {
    let (target_kind, selector, match_kind) = match &mention.target {
        MessageMentionTarget::Agent { did, .. } if did == &binding.runtime_agent_did => {
            ("agent", None, "agent_did")
        }
        MessageMentionTarget::GroupSelector {
            selector: MessageMentionSelector::All,
        } if binding.status == "message_agent_ready" => {
            ("group_selector", Some("all"), "selector_all_best_effort")
        }
        MessageMentionTarget::GroupSelector {
            selector: MessageMentionSelector::Agents,
        } if binding.status == "message_agent_ready"
            && is_agent_did_like(&binding.runtime_agent_did) =>
        {
            (
                "group_selector",
                Some("agents"),
                "selector_agents_best_effort",
            )
        }
        MessageMentionTarget::GroupSelector {
            selector: MessageMentionSelector::Humans,
        }
        | MessageMentionTarget::Human { .. } => return None,
        MessageMentionTarget::Agent { .. } | MessageMentionTarget::GroupSelector { .. } => {
            return None;
        }
    };
    let mention_role = mention_role_name(mention.mention_role);
    Some(UserMessageMentionContext {
        schema: MENTION_CONTEXT_SCHEMA.to_string(),
        mention_id: mention.id.clone(),
        mention_role: mention_role.to_string(),
        target_kind: target_kind.to_string(),
        selector: selector.map(ToOwned::to_owned),
        surface: mention_surface(&payload.text, mention),
        range_start: mention.range.start,
        range_end: mention.range.end,
        match_kind: match_kind.to_string(),
        best_effort_group_state: MENTION_BEST_EFFORT_GROUP_STATE.to_string(),
        attention_policy: MENTION_ATTENTION_POLICY.to_string(),
        prompt_hint: mention_prompt_hint(mention_role),
    })
}

fn mention_surface(text: &str, mention: &MessageMention) -> String {
    text.chars()
        .skip(mention.range.start)
        .take(mention.range.end.saturating_sub(mention.range.start))
        .collect()
}

fn mention_role_name(role: MessageMentionRole) -> &'static str {
    match role {
        MessageMentionRole::Addressee => "addressee",
        MessageMentionRole::Cc => "cc",
    }
}

fn mention_prompt_hint(role: &str) -> String {
    match role {
        "cc" => "FYI/CC mention: treat this as awareness context and do not assume an action is required.".to_string(),
        _ => "Direct mention: the runtime agent was explicitly addressed, but this is still not authorization.".to_string(),
    }
}

fn mention_context_priority(context: &UserMessageMentionContext) -> i32 {
    let target_score = match context.match_kind.as_str() {
        "agent_did" => 20,
        "selector_agents_best_effort" => 12,
        "selector_all_best_effort" => 10,
        _ => 0,
    };
    let role_score = if context.mention_role == "addressee" {
        2
    } else {
        0
    };
    target_score + role_score
}

fn processed_message_id_for_dispatch(
    binding: &AppMessageAgentBindingRecord,
    source_message_id: &str,
    dispatch_content: &AgentDispatchContent,
) -> String {
    match &dispatch_content.mention_context {
        Some(_) => format!(
            "{}:runtime:{}:group-mention",
            source_message_id,
            stable_id_suffix(&binding.runtime_agent_did),
        ),
        None => source_message_id.to_string(),
    }
}

fn is_group_message(message: &Message) -> bool {
    matches!(message.thread, im_core::messages::ThreadRef::Group(_)) || message.group.is_some()
}

fn is_agent_sender(message: &Message) -> bool {
    is_agent_did_like(message.sender.as_str())
}

fn is_agent_did_like(did: &str) -> bool {
    let did = did.trim();
    did.starts_with("did:agent:") || did.contains(":agent:")
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

fn conversation_id(message: &Message) -> Option<String> {
    match &message.thread {
        im_core::messages::ThreadRef::Direct(peer) => Some(format!("direct:{}", peer.as_str())),
        im_core::messages::ThreadRef::Group(group) => Some(format!("group:{}", group.as_str())),
        im_core::messages::ThreadRef::Thread(thread) => Some(thread.as_str().to_string()),
    }
}

fn is_group_conversation_id(conversation_id: Option<&str>) -> bool {
    conversation_id
        .map(str::trim)
        .is_some_and(|value| value.starts_with("group:"))
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
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::anyhow;
    use im_core::ids::{GroupRef, MessageId, PeerRef};
    use im_core::messages::{MessageKind, MessageMetadata, MessageMetadataAttribute, ThreadRef};
    use tempfile::TempDir;

    use crate::runtime::{RuntimeAgentProfile, RuntimeRun};

    use super::*;

    #[derive(Clone)]
    struct MockClient {
        pages: Arc<Mutex<Vec<DelegatedInboxPage>>>,
        calls: Arc<Mutex<Vec<(String, String, Option<String>)>>>,
    }

    impl UserDelegatedInboxClient for MockClient {
        fn fetch_user_delegated_inbox(
            &self,
            identity: &UserDelegatedIdentityRecord,
            binding: &AppMessageAgentBindingRecord,
            cursor: Option<&str>,
            _limit: u32,
        ) -> Result<DelegatedInboxPage> {
            self.calls.lock().unwrap().push((
                identity.user_did.clone(),
                binding.inbox_auth_verification_method.clone(),
                cursor.map(ToOwned::to_owned),
            ));
            let mut pages = self.pages.lock().unwrap();
            if pages.is_empty() {
                return Ok(DelegatedInboxPage {
                    messages: Vec::new(),
                    next_cursor: None,
                    has_more: false,
                });
            }
            Ok(pages.remove(0))
        }
    }

    #[derive(Default)]
    struct RecordingDispatcher {
        dispatched: Arc<Mutex<Vec<(RuntimeTask, UserMessageEnvelope)>>>,
        fail_once: Arc<Mutex<bool>>,
    }

    impl UserDelegatedMessageDispatcher for RecordingDispatcher {
        fn dispatch_user_message(
            &self,
            _binding: &AppMessageAgentBindingRecord,
            task: RuntimeTask,
            envelope: &UserMessageEnvelope,
        ) -> Result<()> {
            let mut fail_once = self.fail_once.lock().unwrap();
            if *fail_once {
                *fail_once = false;
                return Err(anyhow!("simulated dispatcher failure"));
            }
            drop(fail_once);
            self.dispatched
                .lock()
                .unwrap()
                .push((task, envelope.clone()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingAuthorizer {
        allowed: bool,
        sender_full_handle: Option<String>,
        calls: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    impl RecordingAuthorizer {
        fn allow(sender_full_handle: Option<&str>) -> Self {
            Self {
                allowed: true,
                sender_full_handle: sender_full_handle.map(ToOwned::to_owned),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn deny() -> Self {
            Self {
                allowed: false,
                sender_full_handle: Some("bob.anpclaw.com".to_string()),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl AgentInvocationAuthorizer for RecordingAuthorizer {
        fn authorize_agent_invocation(
            &self,
            _state: &DaemonState,
            binding: &AppMessageAgentBindingRecord,
            envelope: &UserMessageEnvelope,
        ) -> Result<AgentInvocationAuthorization> {
            self.calls.lock().unwrap().push((
                binding.runtime_agent_did.clone(),
                envelope.source_sender_did.clone(),
                envelope.source_message_id.clone(),
            ));
            Ok(AgentInvocationAuthorization {
                allowed: self.allowed,
                reason: if self.allowed {
                    "test_allowed".to_string()
                } else {
                    "test_denied".to_string()
                },
                agent_did: binding.runtime_agent_did.clone(),
                sender_did: envelope.source_sender_did.clone(),
                sender_full_handle: self.sender_full_handle.clone(),
                active_mode: "whitelist".to_string(),
            })
        }
    }

    #[derive(Default)]
    struct RecordingMessageSyncSender {
        sent: Arc<Mutex<Vec<(String, String, Value)>>>,
        fail_once: Arc<Mutex<bool>>,
    }

    impl MessageSyncPayloadSender for RecordingMessageSyncSender {
        fn send_message_sync_payload(
            &self,
            binding: &AppMessageAgentBindingRecord,
            idempotency_key: &str,
            payload: Value,
        ) -> Result<Option<String>> {
            let mut fail_once = self.fail_once.lock().unwrap();
            if *fail_once {
                *fail_once = false;
                return Err(anyhow!("simulated message sync send failure"));
            }
            drop(fail_once);
            self.sent.lock().unwrap().push((
                binding.user_did.clone(),
                idempotency_key.to_string(),
                payload,
            ));
            Ok(Some(format!("sent_{idempotency_key}")))
        }
    }

    #[test]
    fn delegated_inbox_dispatches_plain_message_as_untrusted_envelope() {
        let fixture = fixture();
        let state = &fixture.state;
        let identity = &fixture.identity;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![plain_message("msg_1", "did:human:bob", "hello agent")],
                next_cursor: Some("cursor_2".to_string()),
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.dispatched_messages, 1);
        assert_eq!(outcome.next_cursor.as_deref(), Some("cursor_2"));
        let calls = client.calls.lock().unwrap();
        assert_eq!(calls[0].0, identity.user_did);
        assert_eq!(calls[0].1, identity.verification_method);
        assert_eq!(calls[0].2, None);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        assert_eq!(dispatched.len(), 1);
        let (task, envelope) = &dispatched[0];
        assert_eq!(task.agent_did, binding.runtime_agent_did);
        assert_eq!(task.controller_did, binding.user_did);
        assert_eq!(task.sender_did, binding.user_did);
        assert_eq!(envelope.content_role, "user_message_untrusted");
        assert_eq!(envelope.source_sender_did, "did:human:bob");
        assert_eq!(envelope.content_text, "hello agent");
        assert_eq!(envelope.message_kind, "text");
        assert!(envelope.mention_context.is_none());
        assert_eq!(
            envelope.allowed_actions,
            vec![
                "message.summarize_plain".to_string(),
                "message.create_draft".to_string()
            ]
        );
        assert!(!task.text.contains("system"));

        let event = state
            .load_message_event(&event_id(&binding.user_did, "msg_1"))
            .unwrap()
            .unwrap();
        assert_eq!(event.processing_status, MESSAGE_EVENT_STATUS_DISPATCHED);
        assert_eq!(event.retention_class, RETENTION_CLASS_SHORT_EXCERPT);
        assert_eq!(
            event.plain_text_ref_or_excerpt.as_deref(),
            Some("hello agent")
        );
        let sync = state
            .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_1"))
            .unwrap()
            .unwrap();
        assert_eq!(sync.payload_json["content_role"], "user_message_untrusted");

        let sync_sender = RecordingMessageSyncSender::default();
        let sent = flush_message_sync_outbox_with_sender(state, &sync_sender, 10).unwrap();
        assert_eq!(sent, 1);
        let sent_payloads = sync_sender.sent.lock().unwrap();
        assert_eq!(sent_payloads[0].0, binding.user_did);
        assert_eq!(
            sent_payloads[0].1,
            message_sync_idempotency_key(binding, "msg_1")
        );
        assert_eq!(sent_payloads[0].2["message_id"], "msg_1");
        let delivered = state
            .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_1"))
            .unwrap()
            .unwrap();
        assert_eq!(delivered.status, "sent");
    }

    #[test]
    fn explicit_empty_capabilities_project_empty_allowed_actions() {
        let fixture = fixture();
        let mut binding = fixture.binding.clone();
        binding.capability_policy_json = json!({
            "schema": crate::app_bridge::action::APP_CAPABILITIES_SCHEMA,
            "capabilities": []
        });
        let message = plain_message("msg_1", "did:human:bob", "hello agent");

        let envelope =
            user_message_envelope(&binding, &message, plain_dispatch("hello agent")).unwrap();

        assert!(envelope.allowed_actions.is_empty());
    }

    #[test]
    fn delegated_inbox_uses_cursor_and_skips_processed_replay() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        state
            .upsert_inbox_cursor(&InboxCursorRecord {
                owner_did: binding.user_did.clone(),
                inbox_scope: inbox_scope_for_binding(binding),
                cursor: Some("cursor_1".to_string()),
                updated_at_ms: 0,
            })
            .unwrap();
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![
                DelegatedInboxPage {
                    messages: vec![plain_message("msg_1", "did:human:bob", "hello")],
                    next_cursor: Some("cursor_2".to_string()),
                    has_more: false,
                },
                DelegatedInboxPage {
                    messages: vec![plain_message("msg_1", "did:human:bob", "hello")],
                    next_cursor: Some("cursor_3".to_string()),
                    has_more: false,
                },
            ])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();
        let second =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(second.dispatched_messages, 0);
        assert_eq!(second.skipped_processed_messages, 1);
        let calls = client.calls.lock().unwrap();
        assert_eq!(calls[0].2.as_deref(), Some("cursor_1"));
        assert_eq!(calls[1].2.as_deref(), Some("cursor_2"));
        assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);
        let cursor = state
            .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
            .unwrap()
            .unwrap();
        assert_eq!(cursor.cursor.as_deref(), Some("cursor_3"));
    }

    #[test]
    fn delegated_inbox_retries_retryable_processing_after_dispatch_failure() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![
                DelegatedInboxPage {
                    messages: vec![plain_message("msg_retry", "did:human:bob", "retry me")],
                    next_cursor: Some("cursor_after_failure".to_string()),
                    has_more: false,
                },
                DelegatedInboxPage {
                    messages: vec![plain_message("msg_retry", "did:human:bob", "retry me")],
                    next_cursor: Some("cursor_after_success".to_string()),
                    has_more: false,
                },
            ])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher {
            fail_once: Arc::new(Mutex::new(true)),
            ..RecordingDispatcher::default()
        };

        let first = process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding);

        assert!(first.is_err());
        let processed = state
            .load_processed_message(&binding.user_did, "msg_retry")
            .unwrap()
            .unwrap();
        assert_eq!(processed.status, PROCESSED_STATUS_FAILED_RETRYABLE);
        assert!(state
            .load_message_event(&event_id(&binding.user_did, "msg_retry"))
            .unwrap()
            .is_none());
        assert!(state
            .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
            .unwrap()
            .is_none());

        let second =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(second.dispatched_messages, 1);
        assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);
        let processed = state
            .load_processed_message(&binding.user_did, "msg_retry")
            .unwrap()
            .unwrap();
        assert_eq!(processed.status, PROCESSED_STATUS_DISPATCHED);
        let cursor = state
            .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
            .unwrap()
            .unwrap();
        assert_eq!(cursor.cursor.as_deref(), Some("cursor_after_success"));
    }

    #[test]
    fn delegated_runtime_run_id_skips_failed_prior_attempt() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let failed = crate::runtime::RuntimeRun {
            run_id: "run_task_user_msg_failed".to_string(),
            task_id: "task_user_msg_failed".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            runtime_plugin_id: "hermes".to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Failed,
        };
        state.try_insert_runtime_run(&failed).unwrap();

        let run_id = delegated_runtime_run_id(state, "task_user_msg_failed").unwrap();

        assert_eq!(run_id, "run_task_user_msg_failed_retry_1");
    }

    #[test]
    fn delegated_inbox_ignores_e2ee_without_plaintext_event_or_dispatch() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![e2ee_message("msg_e2ee", "did:human:bob")],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.ignored_e2ee_messages, 1);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
        let event = state
            .load_message_event(&event_id(&binding.user_did, "msg_e2ee"))
            .unwrap()
            .unwrap();
        assert_eq!(event.processing_status, PROCESSED_STATUS_IGNORED_E2EE);
        assert_eq!(event.retention_class, RETENTION_CLASS_OPAQUE_ONLY);
        assert!(event.plain_text_ref_or_excerpt.is_none());
    }

    #[test]
    fn delegated_inbox_does_not_dispatch_system_payload_as_user_text() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![system_payload_message("msg_control", "did:human:bob")],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.dispatched_messages, 0);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    }

    #[test]
    fn delegated_group_runtime_ignores_plain_group_text() {
        let fixture = fixture();
        let state = &fixture.state;
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::allow(Some("bob.anpclaw.com"));
        let handled = process_user_delegated_group_message_for_runtime_with_authorizer(
            state,
            &fixture.binding.runtime_agent_did,
            &group_plain_message("msg_group_plain", "did:human:bob", "@Hermes hello"),
            &dispatcher,
            &authorizer,
        )
        .unwrap();

        assert!(!handled);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
        assert!(authorizer.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn delegated_group_runtime_dispatches_structured_agent_mention() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes hello",
            "mentions": [{
                "id": "men_group_runtime",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": binding.runtime_agent_did}
            }]
        });
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::allow(Some("bob.anpclaw.com"));
        let handled = process_user_delegated_group_message_for_runtime_with_authorizer(
            state,
            &binding.runtime_agent_did,
            &mention_payload_message("msg_group_runtime", "did:human:bob", payload),
            &dispatcher,
            &authorizer,
        )
        .unwrap();

        assert!(handled);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(
            dispatched[0].0.conversation_id.as_deref(),
            Some("group:group_1")
        );
        assert_eq!(dispatched[0].1.message_kind, "group_mention");
        assert_eq!(
            dispatched[0].1.mention_context.as_ref().unwrap().mention_id,
            "men_group_runtime"
        );
        assert_eq!(authorizer.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn delegated_inbox_dispatches_group_agent_did_mention_with_attention_context() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes please summarize",
            "mentions": [{
                "id": "men_agent",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {
                    "kind": "agent",
                    "did": binding.runtime_agent_did,
                    "display_name": "Hermes display ignored"
                },
                "mention_role": "addressee"
            }]
        });
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![mention_payload_message(
                    "msg_agent",
                    "did:human:bob",
                    payload,
                )],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.dispatched_messages, 1);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        assert_eq!(dispatched.len(), 1);
        let (task, envelope) = &dispatched[0];
        assert_eq!(task.conversation_id.as_deref(), Some("group:group_1"));
        assert_eq!(task.sender_did, "did:human:bob");
        assert_eq!(task.controller_did, binding.user_did);
        assert_eq!(envelope.message_kind, "group_mention");
        assert_eq!(envelope.content_text, "@Hermes please summarize");
        let mention_context = envelope.mention_context.as_ref().unwrap();
        assert_eq!(mention_context.mention_id, "men_agent");
        assert_eq!(mention_context.target_kind, "agent");
        assert_eq!(mention_context.selector, None);
        assert_eq!(mention_context.surface, "@Hermes");
        assert_eq!(mention_context.match_kind, "agent_did");
        assert!(mention_context
            .attention_policy
            .contains("not authorization"));

        let task_payload = task_payload(task);
        assert_eq!(
            task_payload["mention_context"]["mention_id"],
            json!("men_agent")
        );
        assert_eq!(
            task_payload["mention_context"]["match_kind"],
            json!("agent_did")
        );
        assert!(task_payload["attention_policy"]
            .as_str()
            .unwrap()
            .contains("not authorization"));
        let sync = state
            .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_agent"))
            .unwrap()
            .unwrap();
        assert_eq!(
            sync.payload_json["mention_context"]["mention_id"],
            "men_agent"
        );
        assert!(state
            .audit_event_exists(
                "user_delegated_inbox.mention_matched",
                Some(&binding.runtime_agent_did),
                Some("men_agent"),
            )
            .unwrap());
    }

    #[test]
    fn delegated_inbox_authorization_denial_skips_group_mention_dispatch() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes please summarize",
            "mentions": [{
                "id": "men_agent_denied",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": binding.runtime_agent_did}
            }]
        });
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![mention_payload_message(
                    "msg_denied",
                    "did:human:bob",
                    payload,
                )],
                next_cursor: Some("cursor_denied".to_string()),
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::deny();

        let outcome = process_user_delegated_inbox_for_binding_with_authorizer(
            state,
            &client,
            &dispatcher,
            &authorizer,
            binding,
        )
        .unwrap();

        assert_eq!(outcome.dispatched_messages, 0);
        assert_eq!(authorizer.calls.lock().unwrap().len(), 1);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
        let processed_id = format!(
            "msg_denied:runtime:{}:group-mention",
            stable_id_suffix(&binding.runtime_agent_did)
        );
        let processed = state
            .load_processed_message(&binding.user_did, &processed_id)
            .unwrap()
            .unwrap();
        assert_eq!(processed.status, PROCESSED_STATUS_DISPATCHED);
        assert!(state
            .audit_event_exists(
                "user_delegated_inbox.mention_authorization_denied",
                Some(&binding.runtime_agent_did),
                Some("test_denied"),
            )
            .unwrap());
    }

    #[test]
    fn delegated_inbox_authorization_handle_is_carried_into_group_task() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes hello",
            "mentions": [{
                "id": "men_agent_allowed",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": binding.runtime_agent_did}
            }]
        });
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![mention_payload_message(
                    "msg_allowed",
                    "did:human:bob",
                    payload,
                )],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::allow(Some("bob.anpclaw.com"));

        let outcome = process_user_delegated_inbox_for_binding_with_authorizer(
            state,
            &client,
            &dispatcher,
            &authorizer,
            binding,
        )
        .unwrap();

        assert_eq!(outcome.dispatched_messages, 1);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        let task_payload = task_payload(&dispatched[0].0);
        assert_eq!(
            task_payload["source_sender_full_handle"],
            json!("bob.anpclaw.com")
        );
        assert_eq!(
            dispatched[0].1.source_sender_full_handle.as_deref(),
            Some("bob.anpclaw.com")
        );
        let sync = state
            .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_allowed"))
            .unwrap()
            .unwrap();
        assert_eq!(sync.payload_json["sender_full_handle"], "bob.anpclaw.com");
    }

    #[test]
    fn delegated_inbox_ignores_agent_sent_group_mentions_to_prevent_loops() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes follow up",
            "mentions": [{
                "id": "men_loop",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": binding.runtime_agent_did}
            }]
        });
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![mention_payload_message(
                    "msg_loop",
                    "did:wba:example.com:agent:other",
                    payload,
                )],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::allow(Some("other-agent.example.com"));

        let outcome = process_user_delegated_inbox_for_binding_with_authorizer(
            state,
            &client,
            &dispatcher,
            &authorizer,
            binding,
        )
        .unwrap();

        assert_eq!(outcome.dispatched_messages, 0);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
        assert!(authorizer.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn delegated_inbox_deduplicates_exact_and_selector_mentions_for_same_agent() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let payload = json!({
            "text": "@Hermes @agents please check",
            "mentions": [
                {
                    "id": "men_agent_exact",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": binding.runtime_agent_did}
                },
                {
                    "id": "men_agents_selector",
                    "range": {"start": 8, "end": 15, "unit": "unicode_code_point"},
                    "target": {"kind": "group_selector", "selector": "agents"}
                }
            ]
        });
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![mention_payload_message(
                    "msg_multi_mention",
                    "did:human:bob",
                    payload,
                )],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();
        let authorizer = RecordingAuthorizer::allow(Some("bob.anpclaw.com"));

        let outcome = process_user_delegated_inbox_for_binding_with_authorizer(
            state,
            &client,
            &dispatcher,
            &authorizer,
            binding,
        )
        .unwrap();

        assert_eq!(outcome.dispatched_messages, 1);
        assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);
        assert_eq!(authorizer.calls.lock().unwrap().len(), 1);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        let mention_context = dispatched[0].1.mention_context.as_ref().unwrap();
        assert_eq!(mention_context.mention_id, "men_agent_exact");
        assert_eq!(mention_context.match_kind, "agent_did");
    }

    #[test]
    fn delegated_inbox_dispatches_group_selector_mentions_with_cc_fyi_context() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![
                    mention_payload_message(
                        "msg_agents",
                        "did:human:bob",
                        json!({
                            "text": "@agents FYI only",
                            "mentions": [{
                                "id": "men_agents",
                                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                                "target": {"kind": "group_selector", "selector": "agents"},
                                "mention_role": "cc"
                            }]
                        }),
                    ),
                    mention_payload_message(
                        "msg_all",
                        "did:human:carol",
                        json!({
                            "text": "@all please look",
                            "mentions": [{
                                "id": "men_all",
                                "range": {"start": 0, "end": 4, "unit": "unicode_code_point"},
                                "target": {"kind": "group_selector", "selector": "all"}
                            }]
                        }),
                    ),
                ],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.dispatched_messages, 2);
        let dispatched = dispatcher.dispatched.lock().unwrap();
        let (_, agents_envelope) = &dispatched[0];
        let agents_context = agents_envelope.mention_context.as_ref().unwrap();
        assert_eq!(agents_context.selector.as_deref(), Some("agents"));
        assert_eq!(agents_context.mention_role, "cc");
        assert_eq!(agents_context.match_kind, "selector_agents_best_effort");
        assert!(agents_context.prompt_hint.contains("FYI/CC"));
        let agents_task_payload = task_payload(&dispatched[0].0);
        assert_eq!(
            agents_task_payload["mention_context"]["prompt_hint"],
            json!(agents_context.prompt_hint)
        );

        let (_, all_envelope) = &dispatched[1];
        let all_context = all_envelope.mention_context.as_ref().unwrap();
        assert_eq!(all_context.selector.as_deref(), Some("all"));
        assert_eq!(all_context.mention_role, "addressee");
        assert_eq!(all_context.match_kind, "selector_all_best_effort");
        assert!(all_context.best_effort_group_state.contains("best_effort"));
    }

    #[test]
    fn delegated_inbox_does_not_dispatch_group_plain_text_humans_or_invalid_mentions() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![
                    group_plain_message("msg_plain_at", "did:human:bob", "@Hermes hello"),
                    mention_payload_message(
                        "msg_humans",
                        "did:human:bob",
                        json!({
                            "text": "@humans please look",
                            "mentions": [{
                                "id": "men_humans",
                                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                                "target": {"kind": "group_selector", "selector": "humans"}
                            }]
                        }),
                    ),
                    mention_payload_message(
                        "msg_invalid",
                        "did:human:bob",
                        json!({
                            "text": "@agents broken range",
                            "mentions": [{
                                "id": "men_invalid",
                                "range": {"start": 0, "end": 99, "unit": "unicode_code_point"},
                                "target": {"kind": "group_selector", "selector": "agents"}
                            }]
                        }),
                    ),
                ],
                next_cursor: Some("cursor_after_skips".to_string()),
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.dispatched_messages, 0);
        assert_eq!(outcome.next_cursor.as_deref(), Some("cursor_after_skips"));
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    }

    #[test]
    fn delegated_inbox_ignores_group_e2ee_mention_cipher_without_dispatch() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let client = MockClient {
            pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
                messages: vec![group_e2ee_mention_cipher_message(
                    "msg_group_e2ee",
                    "did:human:bob",
                    binding,
                )],
                next_cursor: None,
                has_more: false,
            }])),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let dispatcher = RecordingDispatcher::default();

        let outcome =
            process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

        assert_eq!(outcome.ignored_e2ee_messages, 1);
        assert!(dispatcher.dispatched.lock().unwrap().is_empty());
        let event = state
            .load_message_event(&event_id(&binding.user_did, "msg_group_e2ee"))
            .unwrap()
            .unwrap();
        assert_eq!(event.message_kind, "e2ee_opaque");
        assert_eq!(event.retention_class, RETENTION_CLASS_OPAQUE_ONLY);
        assert!(event.plain_text_ref_or_excerpt.is_none());
    }

    #[test]
    fn delegated_runtime_status_and_final_are_queued_without_plaintext_final() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let task = RuntimeTask {
            task_id: "task_user_msg_1".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
            controller_did: binding.daemon_agent_did.clone(),
            sender_did: binding.daemon_agent_did.clone(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            text: serde_json::to_string(&json!({
                "schema": "awiki.runtime.user_message_task.v1",
                "source_message_id": "msg_user_1",
                "source_conversation_id": "direct:did:human:bob",
                "source_sender_did": "did:human:bob",
                "content_hash": "sha256:test",
                "content_text": "hello agent"
            }))
            .unwrap(),
        };
        state.insert_runtime_task(&task).unwrap();
        state
            .insert_runtime_run(&RuntimeRun {
                run_id: "run_user_msg_1".to_string(),
                task_id: task.task_id.clone(),
                agent_did: binding.runtime_agent_did.clone(),
                runtime_profile_id: binding.runtime_profile_id.clone(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                workspace_id: None,
                status: RuntimeRunStatus::Running,
            })
            .unwrap();
        let outbox = UserDelegatedRuntimeOutbox::new_for_test(&fixture.config, state);
        let context = AuthorizedRuntimeContext {
            token_id: "token_1".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            run_id: "run_user_msg_1".to_string(),
            method: crate::security::runtime_token::RpcMethod::TaskStatus,
        };

        outbox
            .send_status(&context, "running", Some("working on it"))
            .unwrap();
        outbox
            .send_final(&context, Some("full final text should not be stored"))
            .unwrap();

        let status = state
            .load_message_sync_outbox(
                "message-sync:did:human:alice:runtime-status:run_user_msg_1:running",
            )
            .unwrap()
            .unwrap();
        assert_eq!(status.payload_json["sync_type"], "runtime_status");
        assert_eq!(status.payload_json["source_message_id"], "msg_user_1");
        assert_eq!(status.payload_json["has_text"], true);
        assert!(status.payload_json["text_hash"].as_str().is_some());

        let final_sync = state
            .load_message_sync_outbox("message-sync:did:human:alice:runtime-final:run_user_msg_1")
            .unwrap()
            .unwrap();
        assert_eq!(final_sync.payload_json["sync_type"], "runtime_final");
        assert_eq!(final_sync.payload_json["source_message_id"], "msg_user_1");
        assert_eq!(
            final_sync.payload_json["source_conversation_id"],
            "direct:did:human:bob"
        );
        assert_eq!(final_sync.payload_json["retention_class"], "hash_only");
        assert!(final_sync.payload_json["text_hash"].as_str().is_some());
        assert!(!final_sync
            .payload_json
            .to_string()
            .contains("full final text should not be stored"));

        let sync_sender = RecordingMessageSyncSender::default();
        let sent = flush_message_sync_outbox_with_sender(state, &sync_sender, 10).unwrap();
        assert_eq!(sent, 2);
        let sent_payloads = sync_sender.sent.lock().unwrap();
        assert!(sent_payloads
            .iter()
            .any(|(_, _, payload)| payload["sync_type"] == "runtime_final"
                && payload["source_message_id"] == "msg_user_1"));
    }

    #[test]
    fn delegated_runtime_host_final_message_is_converted_to_message_sync() {
        let fixture = fixture();
        let state = &fixture.state;
        let binding = &fixture.binding;
        let task = RuntimeTask {
            task_id: "task_user_msg_host_final".to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
            controller_did: binding.user_did.clone(),
            sender_did: binding.user_did.clone(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            text: serde_json::to_string(&json!({
                "schema": "awiki.runtime.user_message_task.v1",
                "source_message_id": "msg_user_host_final",
                "source_conversation_id": "direct:did:human:bob",
                "source_sender_did": "did:human:bob",
                "content_hash": "sha256:test",
                "content_text": "hello agent"
            }))
            .unwrap(),
        };
        state.insert_runtime_task(&task).unwrap();
        state
            .insert_runtime_run(&RuntimeRun {
                run_id: "run_user_msg_host_final".to_string(),
                task_id: task.task_id.clone(),
                agent_did: binding.runtime_agent_did.clone(),
                runtime_profile_id: binding.runtime_profile_id.clone(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                workspace_id: None,
                status: RuntimeRunStatus::Running,
            })
            .unwrap();
        let outbox = UserDelegatedRuntimeOutbox::new_for_test(&fixture.config, state);
        let context = AuthorizedRuntimeContext {
            token_id: HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID.to_string(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            run_id: "run_user_msg_host_final".to_string(),
            method: crate::security::runtime_token::RpcMethod::MsgSend,
        };
        let result = outbox
            .send_message(
                &context,
                &RuntimeMessageSend {
                    target: crate::outbox::RuntimeMessageTarget::Direct {
                        recipient: binding.user_did.clone(),
                        raw_recipient: binding.user_did.clone(),
                        resolved_did: Some(binding.user_did.clone()),
                    },
                    text: "final summary text".to_string(),
                    payload: None,
                    file_path: None,
                    display_filename: None,
                    mime_type: None,
                    idempotency_key: Some("runtime-final:test".to_string()),
                    security: crate::outbox::RuntimeMessageSecurity::DefaultPlain,
                },
            )
            .unwrap();

        assert_eq!(result.message_id.as_deref(), Some("runtime-final:test"));
        let final_sync = state
            .load_message_sync_outbox(
                "message-sync:did:human:alice:runtime-final:run_user_msg_host_final",
            )
            .unwrap()
            .unwrap();
        assert_eq!(final_sync.payload_json["sync_type"], "runtime_final");
        assert_eq!(
            final_sync.payload_json["source_message_id"],
            "msg_user_host_final"
        );
        assert_eq!(final_sync.payload_json["has_text"], true);
        assert!(!final_sync
            .payload_json
            .to_string()
            .contains("final summary text"));
    }

    struct TestFixture {
        _root: TempDir,
        config: DaemonConfig,
        state: DaemonState,
        identity: UserDelegatedIdentityRecord,
        binding: AppMessageAgentBindingRecord,
    }

    fn fixture() -> TestFixture {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let identity = UserDelegatedIdentityRecord {
            user_did: "did:human:alice".to_string(),
            verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            controller_did: "did:human:alice".to_string(),
            daemon_agent_did: "did:agent:daemon".to_string(),
            public_key_multibase: "z-public".to_string(),
            private_key_material: "pem-private".to_string(),
            allowed_scopes_json: json!(["message.inbox.read.plain"]),
            status: "paired_key_received".to_string(),
            expires_at: None,
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "bootstrap:1".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let replay = crate::state::BootstrapReplayRecord {
            bootstrap_id: identity.bootstrap_id.clone(),
            idempotency_key: identity.idempotency_key.clone(),
            payload_hash: "hash".to_string(),
            user_did: identity.user_did.clone(),
            verification_method: identity.verification_method.clone(),
            app_instance_id: identity.app_instance_id.clone(),
            daemon_agent_did: identity.daemon_agent_did.clone(),
            status: identity.status.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        state.store_bootstrap_state(&identity, &replay).unwrap();
        state
            .upsert_runtime_agent_profile(&RuntimeAgentProfile {
                agent_did: "did:agent:hermes".to_string(),
                controller_user_id: "user-alice".to_string(),
                controller_full_handle: "alice.anpclaw.com".to_string(),
                controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com"
                    .to_string(),
                controller_did: identity.user_did.clone(),
                runtime_profile_id: "profile_hermes".to_string(),
                runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
                display_name: Some("Hermes".to_string()),
                workspace_id: None,
                workspace_root: None,
                workspace_mode: None,
            })
            .unwrap();
        let binding = AppMessageAgentBindingRecord {
            binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
            user_did: identity.user_did.clone(),
            inbox_auth_verification_method: identity.verification_method.clone(),
            app_instance_id: identity.app_instance_id.clone(),
            bootstrap_id: identity.bootstrap_id.clone(),
            idempotency_key: identity.idempotency_key.clone(),
            daemon_agent_did: identity.daemon_agent_did.clone(),
            runtime_agent_did: "did:agent:hermes".to_string(),
            runtime_profile_id: "profile_hermes".to_string(),
            role: APP_MESSAGE_HANDLER_ROLE.to_string(),
            desired_agent_json: json!({
                "role": APP_MESSAGE_HANDLER_ROLE,
                "allowed_actions": ["message.summarize_plain", "message.create_draft"]
            }),
            capability_policy_json: json!({
                "schema": crate::app_bridge::action::APP_CAPABILITIES_SCHEMA,
                "capabilities": ["message.summarize_plain", "message.create_draft"],
                "require_confirmation_for_write_actions": true
            }),
            status: "message_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        };
        state.upsert_app_message_agent_binding(&binding).unwrap();
        TestFixture {
            _root: root,
            config,
            state,
            identity,
            binding,
        }
    }

    fn plain_message(id: &str, sender: &str, text: &str) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse(sender, "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: Some(PeerRef::parse("did:human:alice", "").unwrap()),
            group: None,
            body: MessageBodyView::Text {
                text: text.to_string(),
                kind: MessageKind::Text,
            },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("text/plain".to_string()),
                ..MessageMetadata::default()
            },
        }
    }

    fn e2ee_message(id: &str, sender: &str) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse(sender, "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: Some(PeerRef::parse("did:human:alice", "").unwrap()),
            group: None,
            body: MessageBodyView::Unsupported {
                content_type: Some("application/anp-direct-cipher+json".to_string()),
            },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("application/anp-direct-cipher+json".to_string()),
                attributes: vec![MessageMetadataAttribute {
                    key: "security".to_string(),
                    value: "direct-e2ee".to_string(),
                }],
                ..MessageMetadata::default()
            },
        }
    }

    fn system_payload_message(id: &str, sender: &str) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: None,
            group: Some(GroupRef::parse("group_1").unwrap()),
            body: MessageBodyView::Payload {
                payload: json!({
                    "schema": "awiki.daemon.bootstrap.v1",
                    "private_key_multibase": "should-not-dispatch"
                }),
            },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("application/json".to_string()),
                ..MessageMetadata::default()
            },
        }
    }

    fn mention_payload_message(id: &str, sender: &str, payload: Value) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: None,
            group: Some(GroupRef::parse("group_1").unwrap()),
            body: MessageBodyView::Payload { payload },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("application/json".to_string()),
                ..MessageMetadata::default()
            },
        }
    }

    fn group_plain_message(id: &str, sender: &str, text: &str) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: None,
            group: Some(GroupRef::parse("group_1").unwrap()),
            body: MessageBodyView::Text {
                text: text.to_string(),
                kind: MessageKind::Text,
            },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("text/plain".to_string()),
                ..MessageMetadata::default()
            },
        }
    }

    fn group_e2ee_mention_cipher_message(
        id: &str,
        sender: &str,
        binding: &AppMessageAgentBindingRecord,
    ) -> Message {
        Message {
            id: MessageId::parse(id).unwrap(),
            thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse(sender, "").unwrap(),
            receiver: None,
            group: Some(GroupRef::parse("group_1").unwrap()),
            body: MessageBodyView::Payload {
                payload: json!({
                    "text": "@Hermes encrypted",
                    "mentions": [{
                        "id": "men_e2ee",
                        "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                        "target": {"kind": "agent", "did": binding.runtime_agent_did}
                    }]
                }),
            },
            sent_at: Some("2026-06-09T00:00:00Z".to_string()),
            received_at: Some("2026-06-09T00:00:01Z".to_string()),
            metadata: MessageMetadata {
                content_type: Some("application/anp-group-cipher+json".to_string()),
                attributes: vec![MessageMetadataAttribute {
                    key: "security".to_string(),
                    value: "group-e2ee".to_string(),
                }],
                ..MessageMetadata::default()
            },
        }
    }

    fn plain_dispatch(text: &str) -> AgentDispatchContent {
        AgentDispatchContent {
            text: text.to_string(),
            message_kind: "text",
            mention_context: None,
        }
    }

    fn task_payload(task: &RuntimeTask) -> Value {
        serde_json::from_str(&task.text).unwrap()
    }
}
