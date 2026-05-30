use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::runtime::{RuntimeAgentProfile, RuntimeTask};

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
    if message.sender_did != profile.controller_did {
        bail!("message sender is not the configured controller_did");
    }
    if message.target_agent_did != profile.agent_did {
        bail!("message target does not match runtime agent");
    }
    if message.text.trim().is_empty() {
        bail!("controller text task must not be empty");
    }

    let task = RuntimeTask {
        task_id: format!("task_{}", message.message_id),
        agent_did: profile.agent_did.clone(),
        controller_did: profile.controller_did.clone(),
        sender_did: message.sender_did,
        conversation_id: message.conversation_id,
        text: message.text,
    };
    task.validate()?;
    Ok(task)
}
