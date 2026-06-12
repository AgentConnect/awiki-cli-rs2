use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::runtime::{RuntimeAgentProfile, RuntimeTask};

pub mod user_delegated;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTextMessage {
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender_did: String,
    pub target_agent_did: String,
    pub text: String,
}

pub fn route_controller_text_task(
    profile: &RuntimeAgentProfile,
    message: ControllerTextMessage,
) -> Result<RuntimeTask> {
    profile.validate()?;
    if message.target_agent_did != profile.agent_did {
        bail!("message target does not match runtime agent");
    }
    if message.sender_did.trim().is_empty() {
        bail!("message sender_did must not be empty");
    }
    if message.text.trim().is_empty() {
        bail!("controller text task must not be empty");
    }

    let task = RuntimeTask {
        task_id: format!("task_{}", message.message_id),
        agent_did: profile.agent_did.clone(),
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: message.sender_did.clone(),
        sender_did: message.sender_did,
        conversation_id: message.conversation_id,
        text: message.text,
    };
    task.validate()?;
    Ok(task)
}
