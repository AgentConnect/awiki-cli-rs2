use std::fmt;

use serde::{Deserialize, Serialize};

use crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID;
use crate::runtime::{RuntimeRun, RuntimeTask};
use crate::state::HermesProfileRecord;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesPromptWrapper {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub runtime_plugin_id: String,
    pub controller_did: String,
    pub sender_did: String,
    pub controller_verified: bool,
    pub message_id: String,
    pub run_id: String,
    pub conversation_id: Option<String>,
    pub content_type: String,
    pub allowed_actions: Vec<String>,
    pub user_message: String,
}

impl fmt::Debug for HermesPromptWrapper {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HermesPromptWrapper")
            .field("agent_did", &self.agent_did)
            .field("runtime_profile_id", &self.runtime_profile_id)
            .field("runtime_plugin_id", &self.runtime_plugin_id)
            .field("controller_did", &self.controller_did)
            .field("sender_did", &self.sender_did)
            .field("controller_verified", &self.controller_verified)
            .field("message_id", &self.message_id)
            .field("run_id", &self.run_id)
            .field("conversation_id", &self.conversation_id)
            .field("content_type", &self.content_type)
            .field("allowed_actions", &self.allowed_actions)
            .field("user_message", &"<redacted>")
            .finish()
    }
}

impl HermesPromptWrapper {
    pub fn new(profile: &HermesProfileRecord, run: &RuntimeRun, task: &RuntimeTask) -> Self {
        Self {
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            controller_did: task.controller_did.clone(),
            sender_did: task.sender_did.clone(),
            controller_verified: task.sender_did == task.controller_did,
            message_id: task.task_id.trim_start_matches("task_").to_string(),
            run_id: run.run_id.clone(),
            conversation_id: task.conversation_id.clone(),
            content_type: "text/plain".to_string(),
            allowed_actions: vec![
                "report-status".to_string(),
                "finish-message".to_string(),
                "send-message".to_string(),
            ],
            user_message: task.text.clone(),
        }
    }

    pub fn to_prompt_text(&self) -> String {
        format!(
            r#"You are Awiki Hermes Runtime Agent.

This prompt wrapper was constructed by awiki daemon after controller verification.

agent:
  agent_did: {agent_did}
  runtime_profile_id: {runtime_profile_id}
  runtime_plugin_id: {runtime_plugin_id}

controller:
  controller_did: {controller_did}
  sender_did: {sender_did}
  controller_verified: {controller_verified}

message:
  message_id: {message_id}
  run_id: {run_id}
  conversation_id: {conversation_id}
  content_type: text/plain

allowed_actions:
  - report-status
  - finish-message
  - send-message

rules:
  - Use message/run semantics, not product task workflow.
  - Use daemon wrapper/local RPC for Awiki capabilities.
  - Do not connect to message-service directly.
  - Do not claim a message was sent unless daemon wrapper reports success.
  - Streaming message.complete is observation only; successful final must go through finish-message.
  - Failed execution should report failed status; do not call success final for failures.

user_message:
{user_message}
"#,
            agent_did = self.agent_did,
            runtime_profile_id = self.runtime_profile_id,
            runtime_plugin_id = self.runtime_plugin_id,
            controller_did = self.controller_did,
            sender_did = self.sender_did,
            controller_verified = self.controller_verified,
            message_id = self.message_id,
            run_id = self.run_id,
            conversation_id = self.conversation_id.as_deref().unwrap_or(""),
            user_message = self.user_message,
        )
    }
}
