use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::runtime::{is_group_conversation_id, RuntimeAgentProfile, RuntimeTask};

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

    let controller_did = if is_group_conversation_id(message.conversation_id.as_deref()) {
        profile.controller_did.clone()
    } else {
        message.sender_did.clone()
    };
    let task = RuntimeTask {
        task_id: format!("task_{}", message.message_id),
        agent_did: profile.agent_did.clone(),
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did,
        sender_did: message.sender_did,
        conversation_id: message.conversation_id,
        text: message.text,
    };
    task.validate()?;
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::runtime_task_matches_profile_controller_scope;

    fn profile() -> RuntimeAgentProfile {
        RuntimeAgentProfile {
            agent_did: "did:agent:hermes".to_string(),
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.awiki.info".to_string(),
            controller_scope_key: "controller-scope:user-alice".to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_profile_id: "rt_hermes".to_string(),
            runtime_plugin_id: "hermes".to_string(),
            display_name: None,
            workspace_id: None,
            workspace_root: None,
            workspace_mode: None,
        }
    }

    #[test]
    fn group_text_task_keeps_profile_controller_and_member_sender() {
        let profile = profile();

        let task = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "did:example:group:9".to_string(),
                conversation_id: Some("group:did:example:group".to_string()),
                sender_did: "did:human:bob".to_string(),
                target_agent_did: profile.agent_did.clone(),
                text: "hello group agent".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.controller_did, "did:human:alice");
        assert_eq!(task.sender_did, "did:human:bob");
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }

    #[test]
    fn direct_text_task_uses_verified_sender_as_controller() {
        let profile = profile();

        let task = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "msg_direct".to_string(),
                conversation_id: Some("direct:did:human:bob".to_string()),
                sender_did: "did:human:bob".to_string(),
                target_agent_did: profile.agent_did.clone(),
                text: "hello direct agent".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.controller_did, "did:human:bob");
        assert_eq!(task.sender_did, "did:human:bob");
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }
}
