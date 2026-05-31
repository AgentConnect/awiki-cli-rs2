use std::sync::{Arc, Mutex};

use anyhow::Result;
use im_core::ids::PeerRef;
use im_core::messages::{
    MessageBody, MessageDeliveryOptions, MessageSecurityMode, MessageTarget, SendMessageRequest,
    SendMessageResult,
};
use serde::{Deserialize, Serialize};

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
        recipient: Option<&str>,
        text: Option<&str>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxRecordKind {
    Status,
    Final,
    Message,
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
        });
        Ok(())
    }

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        recipient: Option<&str>,
        text: Option<&str>,
    ) -> Result<()> {
        self.push(OutboxRecord {
            run_id: context.run_id.clone(),
            agent_did: context.agent_did.clone(),
            kind: OutboxRecordKind::Message,
            state: None,
            recipient: recipient.map(str::to_string),
            text: text.map(str::to_string),
        });
        Ok(())
    }
}
