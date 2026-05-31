use std::sync::{Arc, Mutex};

use anyhow::Result;
use im_core::ids::PeerRef;
use im_core::messages::{
    MessageBody, MessageDeliveryOptions, MessageKind, MessageSecurityMode, MessageTarget,
    SendMessageRequest, SendMessageResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::AuthorizedRuntimeContext;

pub trait RuntimeOutbox {
    fn send_status(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()>;

    fn send_final(&self, context: &AuthorizedRuntimeContext, text: Option<&str>) -> Result<()>;

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<()>;
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
        let result = self
            .client
            .messages()
            .send_async(SendMessageRequest {
                target: MessageTarget::Direct(PeerRef::parse(recipient_did, "")?),
                body: MessageBody::Payload { payload },
                security: MessageSecurityMode::DefaultPlain,
                client_message_id: None,
                delivery: MessageDeliveryOptions::default(),
            })
            .await?;
        Ok(result)
    }

    pub async fn send_text_async(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
    ) -> Result<SendMessageResult> {
        ensure_messaging_session(&self.client).await?;
        let result = self
            .client
            .messages()
            .send_async(SendMessageRequest {
                target: MessageTarget::Direct(PeerRef::parse(recipient_did, "")?),
                body: MessageBody::Text {
                    text: text.to_string(),
                    kind: MessageKind::Text,
                },
                security: security.to_im_core(),
                client_message_id: None,
                delivery: MessageDeliveryOptions::default(),
            })
            .await?;
        Ok(result)
    }

    pub fn send_payload(
        &self,
        recipient_did: &str,
        payload: serde_json::Value,
    ) -> Result<SendMessageResult> {
        let outbox = self.clone();
        let recipient_did = recipient_did.to_string();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-outbox-send".to_string())
                .spawn(move || block_on_payload_send(outbox, recipient_did, payload))?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("outbox send thread panicked"))?;
        }
        block_on_payload_send(outbox, recipient_did, payload)
    }

    pub fn send_text(
        &self,
        recipient_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
    ) -> Result<SendMessageResult> {
        let outbox = self.clone();
        let recipient_did = recipient_did.to_string();
        let text = text.to_string();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-daemon-outbox-send".to_string())
                .spawn(move || block_on_text_send(outbox, recipient_did, text, security))?;
            return join
                .join()
                .map_err(|_| anyhow::anyhow!("outbox send thread panicked"))?;
        }
        block_on_text_send(outbox, recipient_did, text, security)
    }
}

fn block_on_payload_send(
    outbox: ImCoreAgentOutbox,
    recipient_did: String,
    payload: serde_json::Value,
) -> Result<SendMessageResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(outbox.send_payload_async(&recipient_did, payload))
}

fn block_on_text_send(
    outbox: ImCoreAgentOutbox,
    recipient_did: String,
    text: String,
    security: RuntimeMessageSecurity,
) -> Result<SendMessageResult> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(outbox.send_text_async(&recipient_did, &text, security))
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
    pub recipient: Option<String>,
    pub text: Option<String>,
    pub security: Option<RuntimeMessageSecurity>,
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
    DirectE2ee,
}

impl RuntimeMessageSecurity {
    pub fn parse(input: Option<&str>) -> Result<Self> {
        match input.unwrap_or("default_plain").trim() {
            "" | "default_plain" | "plain" => Ok(Self::DefaultPlain),
            "direct_e2ee" | "secure_direct" => Ok(Self::DirectE2ee),
            other => anyhow::bail!("unsupported msg.send security: {other}"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultPlain => "default_plain",
            Self::DirectE2ee => "direct_e2ee",
        }
    }

    fn to_im_core(self) -> MessageSecurityMode {
        match self {
            Self::DefaultPlain => MessageSecurityMode::DefaultPlain,
            Self::DirectE2ee => MessageSecurityMode::SecureDirect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMessageSend {
    pub recipient: String,
    pub text: String,
    pub security: RuntimeMessageSecurity,
}

impl RuntimeMessageSend {
    pub fn from_params(params: &Value) -> Result<Self> {
        let recipient = params
            .get("to")
            .or_else(|| params.get("recipient"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("msg.send recipient is required"))?;
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("msg.send text is required"))?;
        let security =
            RuntimeMessageSecurity::parse(params.get("security").and_then(Value::as_str))?;

        Ok(Self {
            recipient: recipient.to_string(),
            text: text.to_string(),
            security,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryRuntimeOutbox {
    records: Arc<Mutex<Vec<OutboxRecord>>>,
    agent_statuses: Arc<Mutex<Vec<AgentStatusResponse>>>,
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

impl RuntimeOutbox for MemoryRuntimeOutbox {
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
            recipient: None,
            text: text.map(str::to_string),
            security: None,
        });
        Ok(())
    }

    fn send_final(&self, context: &AuthorizedRuntimeContext, text: Option<&str>) -> Result<()> {
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Final,
            state: Some("finished".to_string()),
            recipient: None,
            text: text.map(str::to_string),
            security: None,
        });
        Ok(())
    }

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<()> {
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Message,
            state: None,
            recipient: Some(message.recipient.clone()),
            text: Some(message.text.clone()),
            security: Some(message.security),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn runtime_message_send_params_validate_and_map_security() {
        let plain = RuntimeMessageSend::from_params(&json!({
            "to": "did:human:alice",
            "text": "hello"
        }))
        .unwrap();
        assert_eq!(plain.recipient, "did:human:alice");
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.security, RuntimeMessageSecurity::DefaultPlain);
        assert_eq!(
            plain.security.to_im_core(),
            MessageSecurityMode::DefaultPlain
        );

        let secure = RuntimeMessageSend::from_params(&json!({
            "recipient": "did:human:bob",
            "text": "secure hello",
            "security": "direct_e2ee"
        }))
        .unwrap();
        assert_eq!(secure.security, RuntimeMessageSecurity::DirectE2ee);
        assert_eq!(
            secure.security.to_im_core(),
            MessageSecurityMode::SecureDirect
        );
    }
}
