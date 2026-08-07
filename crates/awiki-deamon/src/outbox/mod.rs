use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use im_core::attachments::{AttachmentInput, AttachmentSendRequest, AttachmentSendResult};
use im_core::ids::{GroupRef, PeerRef};
use im_core::messages::{
    MessageBody, MessageDeliveryOptions, MessageKind, MessageSecurityMode, MessageTarget,
    SendMessageRequest, SendMessageResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::state::AuthorizedRuntimeContext;

pub trait RuntimeOutbox {
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
    ) -> Result<()>;

    fn send_status_with_detail(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        _last_error_code: Option<&str>,
        _last_error_summary: Option<&str>,
    ) -> Result<()> {
        self.send_status(context, state, text)
    }

    fn send_status_with_metadata(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
        _metadata: Option<&Value>,
    ) -> Result<()> {
        self.send_status_with_detail(context, state, text, last_error_code, last_error_summary)
    }

    fn send_final(&self, context: &AuthorizedRuntimeContext, text: Option<&str>) -> Result<()>;

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult>;

    fn send_attachment(
        &self,
        context: &AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult>;
}

pub trait AgentManagementOutbox {
    fn send_agent_status(&self, response: &AgentStatusResponse) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusResponse {
    pub conversation_id: Option<String>,
    pub agent_did: String,
    pub recipient_did: String,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct ImCoreAgentOutbox {
    client: im_core::ImClient,
}

impl ImCoreAgentOutbox {
    pub fn new(client: im_core::ImClient) -> Self {
        Self { client }
    }

    pub async fn send_payload_async(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
    ) -> Result<SendMessageResult> {
        ensure_messaging_session(&self.client).await?;
        self.send_payload_with_security_async(
            recipient_did,
            payload,
            management_payload_security_mode(),
            MessageDeliveryOptions::default(),
        )
        .await
    }

    pub async fn send_payload_with_delivery_async(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
        delivery: MessageDeliveryOptions,
    ) -> Result<SendMessageResult> {
        ensure_messaging_session(&self.client).await?;
        self.send_payload_with_security_async(
            recipient_did,
            payload,
            management_payload_security_mode(),
            delivery,
        )
        .await
    }

    async fn send_payload_with_security_async(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
        security: MessageSecurityMode,
        delivery: MessageDeliveryOptions,
    ) -> Result<SendMessageResult> {
        let client_message_id = delivery
            .idempotency_key
            .as_deref()
            .map(im_core::ids::MessageId::parse)
            .transpose()?;
        let request = SendMessageRequest {
            target: MessageTarget::Direct(PeerRef::parse(recipient_did, "")?),
            body: MessageBody::Payload { payload },
            security,
            client_message_id,
            delivery,
            delegated_signing: None,
        };
        Ok(self.client.messages().send_async(request).await?)
    }

    pub async fn send_attachment_async(
        &self,
        recipient_did: &str,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<AttachmentSendResult> {
        ensure_messaging_session(&self.client).await?;
        let result = self
            .client
            .attachments()
            .send_async(
                MessageTarget::Direct(PeerRef::parse(recipient_did, "")?),
                AttachmentSendRequest {
                    input: AttachmentInput::LocalFile(attachment.file_path.clone()),
                    caption: attachment.caption.clone(),
                    mention_payload: None,
                    mime_type: None,
                    filename: attachment.display_filename.clone(),
                    delivery: MessageDeliveryOptions::default(),
                    security: MessageSecurityMode::DefaultPlain,
                },
            )
            .await?;
        Ok(result)
    }

    pub async fn send_text_async(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
    ) -> Result<SendMessageResult> {
        self.send_text_with_delivery_async(
            recipient_did,
            text,
            security,
            MessageDeliveryOptions::default(),
        )
        .await
    }

    pub async fn send_text_with_delivery_async(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
        delivery: MessageDeliveryOptions,
    ) -> Result<SendMessageResult> {
        ensure_messaging_session(&self.client).await?;
        let client_message_id = delivery
            .idempotency_key
            .as_deref()
            .map(im_core::ids::MessageId::parse)
            .transpose()?;
        let result = self
            .client
            .messages()
            .send_async(SendMessageRequest {
                target: MessageTarget::Direct(PeerRef::parse(recipient_did, "")?),
                body: MessageBody::Text {
                    text: text.to_string(),
                    kind: MessageKind::Text,
                },
                security: security.to_im_core_direct()?,
                client_message_id,
                delivery,
                delegated_signing: None,
            })
            .await?;
        Ok(result)
    }

    pub fn send_payload(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
    ) -> Result<SendMessageResult> {
        self.send_payload_with_delivery(recipient_did, payload, MessageDeliveryOptions::default())
    }

    pub fn send_payload_with_delivery(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
        delivery: MessageDeliveryOptions,
    ) -> Result<SendMessageResult> {
        let outbox = self.clone();
        let recipient_did = recipient_did.to_string();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-outbox-send".to_string())
                .spawn(move || block_on_payload_send(outbox, recipient_did, payload, delivery))?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("outbox send thread panicked"))?;
        }
        block_on_payload_send(outbox, recipient_did, payload, delivery)
    }

    pub fn send_text(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
    ) -> Result<SendMessageResult> {
        self.send_text_with_delivery(
            recipient_did,
            text,
            security,
            MessageDeliveryOptions::default(),
        )
    }

    pub fn send_text_with_delivery(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
        delivery: MessageDeliveryOptions,
    ) -> Result<SendMessageResult> {
        let outbox = self.clone();
        let recipient_did = recipient_did.to_string();
        let text = text.to_string();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-outbox-send".to_string())
                .spawn(move || {
                    block_on_text_send(outbox, recipient_did, text, security, delivery)
                })?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("outbox send thread panicked"))?;
        }
        block_on_text_send(outbox, recipient_did, text, security, delivery)
    }

    pub fn send_attachment(
        &self,
        recipient_did: &str,
        attachment: RuntimeAttachmentSend,
    ) -> Result<AttachmentSendResult> {
        let outbox = self.clone();
        let recipient_did = recipient_did.to_string();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-attachment-send".to_string())
                .spawn(move || block_on_attachment_send(outbox, recipient_did, attachment))?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("attachment send thread panicked"))?;
        }
        block_on_attachment_send(outbox, recipient_did, attachment)
    }

    pub fn send_runtime_message(&self, message: RuntimeMessageSend) -> Result<SendMessageResult> {
        let outbox = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-runtime-message-send".to_string())
                .spawn(move || block_on_runtime_message_send(outbox, message))?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("runtime message send thread panicked"))?;
        }
        block_on_runtime_message_send(outbox, message)
    }

    pub fn resolve_handle(&self, recipient: &str) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            return Ok(Some(recipient.to_string()));
        }
        let handle = im_core::ids::Handle::parse(recipient, "")?;
        let lookup = self.client.directory().lookup_handle(handle)?;
        if lookup.status.as_deref() != Some("active") {
            anyhow::bail!("handle_not_active");
        }
        Ok(Some(lookup.did.as_str().to_string()))
    }
}

impl RuntimeOutbox for ImCoreAgentOutbox {
    fn resolve_recipient_did(
        &self,
        _context: &AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        self.resolve_handle(recipient)
    }

    fn send_status(
        &self,
        _context: &AuthorizedRuntimeContext,
        _state: &str,
        _text: Option<&str>,
    ) -> Result<()> {
        anyhow::bail!("ImCoreAgentOutbox cannot send runtime status without controller context")
    }

    fn send_final(&self, _context: &AuthorizedRuntimeContext, _text: Option<&str>) -> Result<()> {
        anyhow::bail!("ImCoreAgentOutbox cannot send final status without controller context")
    }

    fn send_message(
        &self,
        _context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let result = self.send_runtime_message(message.clone())?;
        Ok(RuntimeMessageSendResult {
            message_id: Some(result.message.id.as_str().to_string()),
            raw_recipient: message.raw_recipient().to_string(),
            resolved_did: message.resolved_recipient().to_string(),
            target_kind: message.target_kind().to_string(),
            security: message.security,
        })
    }

    fn send_attachment(
        &self,
        context: &AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let task = attachment
            .target_did
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("attachment target DID is required"))?;
        let result = self.send_attachment(task, attachment.clone())?;
        Ok(RuntimeAttachmentSendResult {
            message_id: Some(result.message.message.id.as_str().to_string()),
            target: attachment.target.clone(),
            display_filename: attachment.display_filename.clone(),
            size_bytes: Some(result.attachment.size_bytes),
            agent_did: context.agent_did.clone(),
        })
    }
}

fn block_on_payload_send(
    outbox: ImCoreAgentOutbox,
    recipient_did: String,
    payload: serde_json::Value,
    delivery: MessageDeliveryOptions,
) -> Result<SendMessageResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(outbox.send_payload_with_delivery_async(&recipient_did, payload, delivery))
}

fn block_on_text_send(
    outbox: ImCoreAgentOutbox,
    recipient_did: String,
    text: String,
    security: RuntimeMessageSecurity,
    delivery: MessageDeliveryOptions,
) -> Result<SendMessageResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(outbox.send_text_with_delivery_async(
        &recipient_did,
        &text,
        security,
        delivery,
    ))
}

fn block_on_attachment_send(
    outbox: ImCoreAgentOutbox,
    recipient_did: String,
    attachment: RuntimeAttachmentSend,
) -> Result<AttachmentSendResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(outbox.send_attachment_async(&recipient_did, &attachment))
}

fn block_on_runtime_message_send(
    outbox: ImCoreAgentOutbox,
    message: RuntimeMessageSend,
) -> Result<SendMessageResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        ensure_messaging_session(&outbox.client).await?;
        let request = message.into_im_core_request()?;
        Ok(outbox.client.messages().send_async(request).await?)
    })
}

fn management_payload_security_mode() -> MessageSecurityMode {
    MessageSecurityMode::DefaultPlain
}

fn final_body_hash(text: Option<&str>) -> Option<String> {
    let text = text?;
    let digest = Sha256::digest(text.as_bytes());
    Some(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

async fn ensure_messaging_session(client: &im_core::ImClient) -> Result<()> {
    match client
        .auth()
        .ensure_session_async(im_core::auth::AuthScope::Messaging)
        .await
    {
        Ok(_) => Ok(()),
        Err(_) => {
            client.auth().refresh_session_async().await?;
            client
                .auth()
                .ensure_session_async(im_core::auth::AuthScope::Messaging)
                .await?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRecord {
    pub run_id: String,
    pub agent_did: String,
    pub kind: OutboxRecordKind,
    pub state: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub metadata: Option<Value>,
    pub recipient: Option<String>,
    pub raw_recipient: Option<String>,
    pub resolved_did: Option<String>,
    pub message_id: Option<String>,
    pub text: Option<String>,
    pub security: Option<RuntimeMessageSecurity>,
    pub file_path: Option<PathBuf>,
    pub display_filename: Option<String>,
    pub mime_type: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxRecordKind {
    Status,
    Final,
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMessageSecurity {
    DefaultPlain,
}

impl RuntimeMessageSecurity {
    pub fn parse(input: Option<&str>) -> Result<Self> {
        match input.unwrap_or("default_plain").trim() {
            "" | "default_plain" | "plain" => Ok(Self::DefaultPlain),
            "direct_e2ee" | "secure_direct" | "group_e2ee" | "group-e2ee" | "secure_group" => {
                anyhow::bail!("unsupported msg.send security: only default_plain is supported")
            }
            other => anyhow::bail!("unsupported msg.send security: {other}"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultPlain => "default_plain",
        }
    }

    fn to_im_core(self, _target: &RuntimeMessageTarget) -> Result<MessageSecurityMode> {
        Ok(MessageSecurityMode::DefaultPlain)
    }

    fn to_im_core_direct(self) -> Result<MessageSecurityMode> {
        Ok(MessageSecurityMode::DefaultPlain)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMessageSend {
    pub target: RuntimeMessageTarget,
    pub text: String,
    pub payload: Option<Value>,
    pub file_path: Option<PathBuf>,
    pub display_filename: Option<String>,
    pub mime_type: Option<String>,
    pub idempotency_key: Option<String>,
    pub security: RuntimeMessageSecurity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMessageTarget {
    Direct {
        recipient: String,
        raw_recipient: String,
        resolved_did: Option<String>,
    },
    Group {
        group: String,
    },
}

impl RuntimeMessageSend {
    pub fn from_params(params: &Value) -> Result<Self> {
        let to = params
            .get("to")
            .or_else(|| params.get("to_handle"))
            .or_else(|| params.get("recipient"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let group = params
            .get("group")
            .or_else(|| params.get("group_did"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let target = match (to, group) {
            (Some(recipient), None) => RuntimeMessageTarget::Direct {
                recipient: recipient.to_string(),
                raw_recipient: recipient.to_string(),
                resolved_did: None,
            },
            (None, Some(group)) => RuntimeMessageTarget::Group {
                group: group.to_string(),
            },
            (None, None) => anyhow::bail!("msg.send requires either to or group"),
            (Some(_), Some(_)) => {
                anyhow::bail!("msg.send accepts either to or group, but not both")
            }
        };
        let text = params
            .get("text")
            .or_else(|| params.get("caption"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("msg.send text is required"))?;
        validate_message_text(text)?;
        let file_path = params
            .get("file_path")
            .or_else(|| params.get("file"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        if let Some(file_path) = file_path.as_ref() {
            validate_attachment_path(file_path)?;
        }
        let display_filename = params
            .get("display_filename")
            .or_else(|| params.get("filename"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mime_type = params
            .get("mime_type")
            .or_else(|| params.get("mime-type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(display_filename) = display_filename.as_deref() {
            validate_attachment_text_field("display_filename", display_filename)?;
        }
        if let Some(mime_type) = mime_type.as_deref() {
            validate_attachment_text_field("mime_type", mime_type)?;
        }
        let security =
            RuntimeMessageSecurity::parse(params.get("security").and_then(Value::as_str))?;
        security.to_im_core(&target)?;
        let idempotency_key = params
            .get("idempotency_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(idempotency_key) = idempotency_key.as_deref() {
            validate_idempotency_key(idempotency_key)?;
        }

        Ok(Self {
            target,
            text: text.to_string(),
            payload: None,
            file_path,
            display_filename,
            mime_type,
            idempotency_key,
            security,
        })
    }

    pub fn with_resolved_recipient(mut self, resolved_did: impl Into<String>) -> Self {
        let resolved_did = resolved_did.into();
        if let RuntimeMessageTarget::Direct {
            recipient,
            raw_recipient,
            resolved_did: current_resolved_did,
        } = &mut self.target
        {
            *raw_recipient = std::mem::take(recipient);
            *recipient = resolved_did.clone();
            *current_resolved_did = Some(resolved_did);
        }
        self
    }

    pub fn recipient_candidates(&self) -> Vec<&str> {
        match &self.target {
            RuntimeMessageTarget::Direct {
                recipient,
                raw_recipient,
                resolved_did,
            } => {
                let mut candidates = vec![raw_recipient.as_str(), recipient.as_str()];
                if let Some(resolved_did) = resolved_did.as_deref() {
                    candidates.push(resolved_did);
                }
                candidates
            }
            RuntimeMessageTarget::Group { group } => vec![group.as_str()],
        }
    }

    pub fn raw_recipient(&self) -> &str {
        match &self.target {
            RuntimeMessageTarget::Direct { raw_recipient, .. } => raw_recipient,
            RuntimeMessageTarget::Group { group } => group,
        }
    }

    pub fn resolved_recipient(&self) -> &str {
        match &self.target {
            RuntimeMessageTarget::Direct { recipient, .. } => recipient,
            RuntimeMessageTarget::Group { group } => group,
        }
    }

    pub fn resolved_did(&self) -> Option<&str> {
        match &self.target {
            RuntimeMessageTarget::Direct { resolved_did, .. } => resolved_did.as_deref(),
            RuntimeMessageTarget::Group { .. } => None,
        }
    }

    pub fn target_kind(&self) -> &'static str {
        match &self.target {
            RuntimeMessageTarget::Direct { .. } => "direct",
            RuntimeMessageTarget::Group { .. } => "group",
        }
    }

    fn into_im_core_request(self) -> Result<SendMessageRequest> {
        let target = match &self.target {
            RuntimeMessageTarget::Direct { recipient, .. } => {
                MessageTarget::Direct(PeerRef::parse(recipient, "")?)
            }
            RuntimeMessageTarget::Group { group } => MessageTarget::Group(GroupRef::parse(group)?),
        };
        let security = self.security.to_im_core(&self.target)?;
        let body = if let Some(payload) = self.payload {
            MessageBody::Payload { payload }
        } else if let Some(file_path) = self.file_path {
            MessageBody::Attachment {
                input: AttachmentInput::LocalFile(file_path),
                caption: Some(self.text).filter(|value| !value.trim().is_empty()),
                mention_payload: None,
                mime_type: self.mime_type,
                filename: self.display_filename,
            }
        } else {
            MessageBody::Text {
                text: self.text,
                kind: MessageKind::Text,
            }
        };
        Ok(SendMessageRequest {
            target,
            body,
            security,
            client_message_id: self
                .idempotency_key
                .as_deref()
                .map(im_core::ids::MessageId::parse)
                .transpose()?,
            delivery: MessageDeliveryOptions {
                idempotency_key: self.idempotency_key,
                wait_for_final_acceptance: false,
            },
            delegated_signing: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMessageSendResult {
    pub message_id: Option<String>,
    pub raw_recipient: String,
    pub resolved_did: String,
    pub target_kind: String,
    pub security: RuntimeMessageSecurity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttachmentSend {
    pub target: String,
    pub target_did: Option<String>,
    pub file_path: PathBuf,
    pub display_filename: Option<String>,
    pub caption: Option<String>,
}

impl RuntimeAttachmentSend {
    pub fn from_params(params: &Value, current_target_did: Option<&str>) -> Result<Self> {
        let target = params
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("current_conversation")
            .trim();
        if target != "current_conversation" {
            anyhow::bail!("attachment.send target must be current_conversation");
        }
        let file_path = params
            .get("file_path")
            .or_else(|| params.get("file"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("attachment.send file_path is required"))?;
        validate_attachment_path(&file_path)?;
        let display_filename = params
            .get("display_filename")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let caption = params
            .get("caption")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(display_filename) = display_filename.as_deref() {
            validate_attachment_text_field("display_filename", display_filename)?;
        }
        if let Some(caption) = caption.as_deref() {
            validate_attachment_text_field("caption", caption)?;
        }
        Ok(Self {
            target: target.to_string(),
            target_did: current_target_did.map(str::to_string),
            file_path,
            display_filename,
            caption,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttachmentSendResult {
    pub message_id: Option<String>,
    pub target: String,
    pub display_filename: Option<String>,
    pub size_bytes: Option<u64>,
    pub agent_did: String,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryRuntimeOutbox {
    records: Arc<Mutex<Vec<OutboxRecord>>>,
    agent_statuses: Arc<Mutex<Vec<AgentStatusResponse>>>,
    handle_resolutions: Arc<Mutex<BTreeMap<String, String>>>,
    handle_statuses: Arc<Mutex<BTreeMap<String, String>>>,
}

impl MemoryRuntimeOutbox {
    pub fn records(&self) -> Vec<OutboxRecord> {
        self.records.lock().expect("outbox lock poisoned").clone()
    }

    pub fn agent_statuses(&self) -> Vec<AgentStatusResponse> {
        self.agent_statuses
            .lock()
            .expect("outbox lock poisoned")
            .clone()
    }

    pub fn with_handle_resolution(self, handle: impl Into<String>, did: impl Into<String>) -> Self {
        self.handle_resolutions
            .lock()
            .expect("outbox lock poisoned")
            .insert(normalize_handle_candidate(&handle.into()), did.into());
        self
    }

    pub fn with_handle_resolution_status(
        self,
        handle: impl Into<String>,
        did: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        let handle = normalize_handle_candidate(&handle.into());
        self.handle_resolutions
            .lock()
            .expect("outbox lock poisoned")
            .insert(handle.clone(), did.into());
        self.handle_statuses
            .lock()
            .expect("outbox lock poisoned")
            .insert(handle, status.into());
        self
    }

    fn push(&self, record: OutboxRecord) {
        self.records
            .lock()
            .expect("outbox lock poisoned")
            .push(record);
    }
}

impl AgentManagementOutbox for MemoryRuntimeOutbox {
    fn send_agent_status(&self, response: &AgentStatusResponse) -> Result<()> {
        self.agent_statuses
            .lock()
            .expect("outbox lock poisoned")
            .push(response.clone());
        Ok(())
    }
}

impl AgentManagementOutbox for ImCoreAgentOutbox {
    fn send_agent_status(&self, response: &AgentStatusResponse) -> Result<()> {
        self.send_payload(&response.recipient_did, response.payload.clone())?;
        Ok(())
    }
}

fn validate_attachment_path(path: &PathBuf) -> Result<()> {
    let metadata =
        std::fs::metadata(path).map_err(|_| anyhow::anyhow!("attachment file not found"))?;
    if !metadata.is_file() {
        anyhow::bail!("attachment path must be a regular file");
    }
    if metadata.len() > 100 * 1024 * 1024 {
        anyhow::bail!("attachment file is too large");
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        ".env" | "id_rsa" | "id_ed25519" | "private.key" | "auth.json"
    ) || name.contains("token")
        || name.contains("private")
        || name.contains("secret")
    {
        anyhow::bail!("attachment file is not allowed");
    }
    Ok(())
}

fn validate_attachment_text_field(field: &str, value: &str) -> Result<()> {
    if contains_sensitive_content(value) {
        anyhow::bail!("attachment.send {field} contains sensitive content");
    }
    if looks_like_local_absolute_path(value) {
        anyhow::bail!("attachment.send {field} must not contain local file paths");
    }
    Ok(())
}

fn validate_message_text(text: &str) -> Result<()> {
    if contains_sensitive_content(text) {
        anyhow::bail!("msg.send text contains sensitive content");
    }
    if looks_like_local_absolute_path(text) {
        anyhow::bail!("msg.send text must not contain local file paths");
    }
    Ok(())
}

fn validate_idempotency_key(value: &str) -> Result<()> {
    if value.len() > 256 {
        anyhow::bail!("msg.send idempotency_key is too long");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.'))
    {
        anyhow::bail!("msg.send idempotency_key contains unsupported characters");
    }
    Ok(())
}

fn contains_sensitive_content(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("rtok_")
        || lower.contains("registration_token")
        || lower.contains("jwt")
        || lower.contains("private key")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("secret")
        || lower.contains("begin private key")
        || lower.contains(".env")
}

fn looks_like_local_absolute_path(text: &str) -> bool {
    text.split_whitespace().any(|part| {
        let trimmed = part.trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '`' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}'
            )
        });
        trimmed.starts_with("/Users/")
            || trimmed.starts_with("/home/")
            || trimmed.starts_with("/tmp/")
            || trimmed.starts_with("/var/log/")
            || trimmed.starts_with("C:\\")
    })
}

impl RuntimeOutbox for MemoryRuntimeOutbox {
    fn resolve_recipient_did(
        &self,
        _context: &AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            return Ok(Some(recipient.to_string()));
        }
        let normalized = normalize_handle_candidate(recipient);
        if let Some(status) = self
            .handle_statuses
            .lock()
            .expect("outbox lock poisoned")
            .get(&normalized)
            .cloned()
        {
            if status != "active" {
                anyhow::bail!("handle_not_active");
            }
        }
        Ok(self
            .handle_resolutions
            .lock()
            .expect("outbox lock poisoned")
            .get(&normalized)
            .cloned())
    }

    fn send_status(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Status,
            state: Some(state.to_string()),
            last_error_code: None,
            last_error_summary: None,
            metadata: None,
            recipient: None,
            raw_recipient: None,
            resolved_did: None,
            message_id: None,
            text: text.map(str::to_string),
            security: None,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: None,
        });
        Ok(())
    }

    fn send_status_with_detail(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> Result<()> {
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Status,
            state: Some(state.to_string()),
            last_error_code: last_error_code.map(str::to_string),
            last_error_summary: last_error_summary.map(str::to_string),
            metadata: None,
            recipient: None,
            raw_recipient: None,
            resolved_did: None,
            message_id: None,
            text: text.map(str::to_string),
            security: None,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: None,
        });
        Ok(())
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
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Status,
            state: Some(state.to_string()),
            last_error_code: last_error_code.map(str::to_string),
            last_error_summary: last_error_summary.map(str::to_string),
            metadata: metadata.cloned(),
            recipient: None,
            raw_recipient: None,
            resolved_did: None,
            message_id: None,
            text: text.map(str::to_string),
            security: None,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: None,
        });
        Ok(())
    }

    fn send_final(&self, context: &AuthorizedRuntimeContext, text: Option<&str>) -> Result<()> {
        let metadata = serde_json::json!({
            "final_source": "task_finish_callback",
            "final_body_hash": final_body_hash(text),
            "final_text_bytes": text.map(str::len).unwrap_or(0),
        });
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Final,
            state: Some("finished".to_string()),
            last_error_code: None,
            last_error_summary: None,
            metadata: Some(metadata),
            recipient: None,
            raw_recipient: None,
            resolved_did: None,
            message_id: None,
            text: text.map(str::to_string),
            security: None,
            file_path: None,
            display_filename: None,
            mime_type: None,
            idempotency_key: None,
        });
        Ok(())
    }

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let message_id = format!(
            "memory-message-{}",
            self.records.lock().expect("outbox lock poisoned").len() + 1
        );
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Message,
            state: None,
            last_error_code: None,
            last_error_summary: None,
            metadata: None,
            recipient: Some(message.resolved_recipient().to_string()),
            raw_recipient: Some(message.raw_recipient().to_string()),
            resolved_did: message.resolved_did().map(str::to_string),
            message_id: Some(message_id.clone()),
            text: Some(message.text.clone()),
            security: Some(message.security),
            file_path: message.file_path.clone(),
            display_filename: message.display_filename.clone(),
            mime_type: message.mime_type.clone(),
            idempotency_key: message.idempotency_key.clone(),
        });
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
        context: &AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let message_id = format!(
            "memory-attachment-{}",
            self.records.lock().expect("outbox lock poisoned").len() + 1
        );
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Message,
            state: None,
            last_error_code: None,
            last_error_summary: None,
            metadata: None,
            recipient: attachment.target_did.clone(),
            raw_recipient: Some(attachment.target.clone()),
            resolved_did: attachment.target_did.clone(),
            message_id: Some(message_id.clone()),
            text: attachment.caption.clone(),
            security: Some(RuntimeMessageSecurity::DefaultPlain),
            file_path: Some(attachment.file_path.clone()),
            display_filename: attachment.display_filename.clone(),
            mime_type: None,
            idempotency_key: None,
        });
        Ok(RuntimeAttachmentSendResult {
            message_id: Some(message_id),
            target: attachment.target.clone(),
            display_filename: attachment.display_filename.clone(),
            size_bytes: std::fs::metadata(&attachment.file_path)
                .ok()
                .map(|metadata| metadata.len()),
            agent_did: context.agent_did.clone(),
        })
    }
}

fn normalize_handle_candidate(input: &str) -> String {
    let value = input.trim().to_ascii_lowercase();
    if value.starts_with('@') {
        value
    } else {
        format!("@{value}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn runtime_message_send_params_validate_plain_security_only() {
        let plain = RuntimeMessageSend::from_params(&json!({
            "to": "did:human:alice",
            "text": "  hello  "
        }))
        .unwrap();
        assert_eq!(plain.resolved_recipient(), "did:human:alice");
        assert_eq!(plain.raw_recipient(), "did:human:alice");
        assert_eq!(plain.resolved_did(), None);
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.security, RuntimeMessageSecurity::DefaultPlain);
        assert_eq!(
            plain.security.to_im_core(&plain.target).unwrap(),
            MessageSecurityMode::DefaultPlain
        );

        let secure = RuntimeMessageSend::from_params(&json!({
            "recipient": "did:human:bob",
            "text": "secure hello",
            "security": "direct_e2ee"
        }))
        .unwrap_err();
        assert!(secure
            .to_string()
            .contains("only default_plain is supported"));

        let group = RuntimeMessageSend::from_params(&json!({
            "group": "did:group:team",
            "text": "group hello",
            "security": "group_e2ee"
        }))
        .unwrap_err();
        assert!(group
            .to_string()
            .contains("only default_plain is supported"));
    }

    #[test]
    fn agent_management_payloads_use_plain_direct_control_channel() {
        assert_eq!(
            management_payload_security_mode(),
            MessageSecurityMode::DefaultPlain
        );
    }

    #[test]
    fn final_body_hash_records_empty_body_but_not_missing_body() {
        assert_eq!(final_body_hash(None), None);
        assert_eq!(
            final_body_hash(Some("")),
            Some(
                "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .to_string()
            )
        );
        assert_eq!(
            final_body_hash(Some("abc")),
            Some(
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    .to_string()
            )
        );
    }

    #[test]
    fn runtime_message_send_rejects_sensitive_text_and_paths() {
        for text in [
            "token rtok_sensitive_secret_value_123456789",
            "Authorization: Bearer abc.def.ghi",
            "read /Users/alice/.awiki/logs/full.log",
            "private key BEGIN PRIVATE KEY",
            "load .env",
        ] {
            let error = RuntimeMessageSend::from_params(&json!({
                "to": "did:human:alice",
                "text": text
            }))
            .unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("sensitive") || message.contains("local file paths"),
                "{message}"
            );
        }
    }

    #[test]
    fn runtime_attachment_send_rejects_sensitive_display_fields() {
        let root = tempfile::tempdir().unwrap();
        let file_path = root.path().join("report.txt");
        std::fs::write(&file_path, "report").unwrap();

        for (field, value) in [
            ("display_filename", "secret-token.txt"),
            ("caption", "Authorization: Bearer abc.def.ghi"),
            ("caption", "see /Users/alice/.awiki/logs/full.log"),
        ] {
            let mut params = json!({
                "target": "current_conversation",
                "file_path": file_path,
            });
            params[field] = Value::String(value.to_string());
            let error =
                RuntimeAttachmentSend::from_params(&params, Some("did:human:alice")).unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("sensitive") || message.contains("local file paths"),
                "{message}"
            );
        }
    }
}
