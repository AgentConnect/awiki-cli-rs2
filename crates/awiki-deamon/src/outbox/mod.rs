use std::sync::{Arc, Mutex};

use anyhow::Result;
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
}

impl MemoryRuntimeOutbox {
    pub fn records(&self) -> Vec<OutboxRecord> {
        self.records.lock().expect("outbox lock poisoned").clone()
    }

    fn push(&self, record: OutboxRecord) {
        self.records
            .lock()
            .expect("outbox lock poisoned")
            .push(record);
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
