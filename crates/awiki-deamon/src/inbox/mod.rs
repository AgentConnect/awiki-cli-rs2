use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::controller_scope::VerifiedControllerSender;
use crate::runtime::{
    canonical_full_handle, RuntimeAgentProfile, RuntimeConversationScope,
    RuntimeInvocationAuthority, RuntimeTask, RuntimeTaskTriggerKind,
};

pub mod user_delegated;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTextMessage {
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender_did: String,
    pub requester_user_id: Option<String>,
    pub requester_full_handle: Option<String>,
    pub trigger_kind: RuntimeTaskTriggerKind,
    pub invocation_authority: RuntimeInvocationAuthority,
    pub target_agent_did: String,
    pub text: String,
}

impl ControllerTextMessage {
    pub fn controller_direct(
        message_id: impl Into<String>,
        conversation_id: Option<String>,
        sender_did: impl Into<String>,
        target_agent_did: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            message_id: message_id.into(),
            conversation_id,
            sender_did: sender_did.into(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: RuntimeInvocationAuthority::Controller,
            target_agent_did: target_agent_did.into(),
            text: text.into(),
        }
    }
}

pub fn route_controller_text_task(
    profile: &RuntimeAgentProfile,
    message: ControllerTextMessage,
) -> Result<RuntimeTask> {
    route_text_task_for_profile(profile, None, message)
}

pub fn route_controller_text_task_with_verified_sender(
    profile: &RuntimeAgentProfile,
    verified_sender: &VerifiedControllerSender,
    message: ControllerTextMessage,
) -> Result<RuntimeTask> {
    route_text_task_for_profile(profile, Some(verified_sender), message)
}

fn route_text_task_for_profile(
    profile: &RuntimeAgentProfile,
    verified_sender: Option<&VerifiedControllerSender>,
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
    message
        .trigger_kind
        .validate_against_conversation(message.conversation_id.as_deref())?;
    let controller_did = verified_sender
        .map(|sender| sender.controller_did.clone())
        .unwrap_or_else(|| profile.controller_did.clone());
    if let Some(verified) = verified_sender {
        if verified.controller_user_id != profile.controller_user_id
            || verified.controller_full_handle != profile.controller_full_handle
            || verified.controller_scope_key != profile.controller_scope_key
        {
            bail!("verified controller scope does not match runtime profile");
        }
    }
    let requester_did = message.sender_did.clone();
    let requester_user_id_for_task = message.requester_user_id;
    let mut requester_full_handle_for_task = message.requester_full_handle;
    let (reply_recipient_did, conversation_scope) = match message.trigger_kind {
        RuntimeTaskTriggerKind::ControllerDirect => {
            if requester_did != controller_did {
                bail!("controller_direct runtime task requires controller sender");
            }
            if message.invocation_authority != RuntimeInvocationAuthority::Controller {
                bail!("controller_direct runtime task requires controller authority");
            }
            (
                controller_did.clone(),
                RuntimeConversationScope::controller_private(profile.controller_scope_key.clone()),
            )
        }
        RuntimeTaskTriggerKind::ExternalDirect | RuntimeTaskTriggerKind::DelegatedDirect => {
            if requester_did == controller_did
                && message.trigger_kind == RuntimeTaskTriggerKind::ExternalDirect
            {
                bail!("external_direct runtime task requires non-controller requester");
            }
            let requester_user_id = requester_user_id_for_task
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("direct runtime task requires requester_user_id"))?;
            let requester_full_handle = requester_full_handle_for_task
                .as_deref()
                .map(canonical_full_handle)
                .transpose()?
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("direct runtime task requires requester_full_handle")
                })?;
            requester_full_handle_for_task = Some(requester_full_handle.clone());
            (
                requester_did.clone(),
                RuntimeConversationScope::direct(
                    requester_user_id.to_string(),
                    requester_full_handle,
                )?,
            )
        }
        RuntimeTaskTriggerKind::GroupMention => {
            let requester_full_handle = requester_full_handle_for_task
                .as_deref()
                .map(canonical_full_handle)
                .transpose()?
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("group mention requires requester_full_handle"))?;
            requester_full_handle_for_task = Some(requester_full_handle);
            (
                requester_did.clone(),
                RuntimeConversationScope::group_visible(
                    message
                        .conversation_id
                        .as_deref()
                        .and_then(group_key_from_conversation_id)
                        .ok_or_else(|| {
                            anyhow::anyhow!("group mention requires group conversation id")
                        })?,
                ),
            )
        }
    };
    let task = RuntimeTask {
        task_id: format!("task_{}", message.message_id),
        agent_did: profile.agent_did.clone(),
        agent_handle: profile.agent_handle.clone(),
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did,
        sender_did: message.sender_did,
        requester_did,
        requester_user_id: requester_user_id_for_task,
        requester_full_handle: requester_full_handle_for_task,
        trigger_kind: message.trigger_kind,
        conversation_scope,
        invocation_authority: message.invocation_authority,
        reply_recipient_did,
        conversation_id: message.conversation_id,
        text: message.text,
    };
    task.validate()?;
    Ok(task)
}

fn group_key_from_conversation_id(conversation_id: &str) -> Option<String> {
    conversation_id
        .trim()
        .strip_prefix("group:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::runtime_task_matches_profile_controller_scope;

    fn profile() -> RuntimeAgentProfile {
        RuntimeAgentProfile {
            agent_did: "did:agent:hermes".to_string(),
            agent_handle: "alice-hermes".to_string(),
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
                requester_user_id: Some("user-bob".to_string()),
                requester_full_handle: Some("bob.example.com".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::GroupMention,
                invocation_authority: RuntimeInvocationAuthority::Requester,
                target_agent_did: profile.agent_did.clone(),
                text: "hello group agent".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.controller_did, "did:human:alice");
        assert_eq!(task.sender_did, "did:human:bob");
        assert_eq!(task.requester_did, "did:human:bob");
        assert_eq!(task.reply_recipient_did, "did:human:bob");
        assert_eq!(task.trigger_kind, RuntimeTaskTriggerKind::GroupMention);
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }

    #[test]
    fn group_text_task_can_use_controller_authority_without_private_scope() {
        let profile = profile();

        let task = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "did:example:group:controller".to_string(),
                conversation_id: Some("group:did:example:group".to_string()),
                sender_did: "did:human:alice".to_string(),
                requester_user_id: Some("user-alice".to_string()),
                requester_full_handle: Some("alice.awiki.info".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::GroupMention,
                invocation_authority: RuntimeInvocationAuthority::Controller,
                target_agent_did: profile.agent_did.clone(),
                text: "ask my agent in group to send something".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            task.invocation_authority,
            RuntimeInvocationAuthority::Controller
        );
        assert_eq!(
            task.conversation_scope,
            RuntimeConversationScope::group_visible("did:example:group")
        );
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }

    #[test]
    fn group_text_task_rejects_controller_authority_for_non_controller_requester() {
        let profile = profile();

        let error = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "did:example:group:not-controller".to_string(),
                conversation_id: Some("group:did:example:group".to_string()),
                sender_did: "did:human:bob".to_string(),
                requester_user_id: Some("user-bob".to_string()),
                requester_full_handle: Some("bob.example.com".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::GroupMention,
                invocation_authority: RuntimeInvocationAuthority::Controller,
                target_agent_did: profile.agent_did.clone(),
                text: "pretend bob controls alice's agent".to_string(),
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("group controller authority requires controller identity"));
    }

    #[test]
    fn group_text_task_rejects_controller_identity_without_controller_authority() {
        let profile = profile();

        let error = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "did:example:group:controller-as-requester".to_string(),
                conversation_id: Some("group:did:example:group".to_string()),
                sender_did: "did:human:alice".to_string(),
                requester_user_id: Some("user-alice".to_string()),
                requester_full_handle: Some("alice.awiki.info".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::GroupMention,
                invocation_authority: RuntimeInvocationAuthority::Requester,
                target_agent_did: profile.agent_did.clone(),
                text: "alice should not be downgraded in her own agent group mention".to_string(),
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("group controller identity requires controller authority"));
    }

    #[test]
    fn direct_text_task_uses_verified_sender_as_controller() {
        let profile = profile();

        let task = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "msg_direct".to_string(),
                conversation_id: Some("direct:did:human:bob".to_string()),
                sender_did: "did:human:alice".to_string(),
                requester_user_id: Some("user-alice".to_string()),
                requester_full_handle: None,
                trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
                invocation_authority: RuntimeInvocationAuthority::Controller,
                target_agent_did: profile.agent_did.clone(),
                text: "hello direct agent".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.controller_did, "did:human:alice");
        assert_eq!(task.sender_did, "did:human:alice");
        assert_eq!(task.requester_did, "did:human:alice");
        assert_eq!(task.reply_recipient_did, "did:human:alice");
        assert_eq!(task.trigger_kind, RuntimeTaskTriggerKind::ControllerDirect);
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }

    #[test]
    fn external_direct_text_task_keeps_profile_controller_and_replies_to_requester() {
        let profile = profile();

        let task = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "msg_external".to_string(),
                conversation_id: Some("direct:did:human:bob".to_string()),
                sender_did: "did:human:bob".to_string(),
                requester_user_id: Some("user-bob".to_string()),
                requester_full_handle: Some("bob.example.com".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::ExternalDirect,
                invocation_authority: RuntimeInvocationAuthority::Requester,
                target_agent_did: profile.agent_did.clone(),
                text: "hello external agent".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.controller_did, "did:human:alice");
        assert_eq!(task.sender_did, "did:human:bob");
        assert_eq!(task.requester_did, "did:human:bob");
        assert_eq!(
            task.requester_full_handle.as_deref(),
            Some("bob.example.com")
        );
        assert_eq!(task.reply_recipient_did, "did:human:bob");
        assert_eq!(task.trigger_kind, RuntimeTaskTriggerKind::ExternalDirect);
        assert!(runtime_task_matches_profile_controller_scope(
            &task, &profile
        ));
    }

    #[test]
    fn external_direct_text_task_rejects_controller_authority() {
        let profile = profile();

        let error = route_controller_text_task(
            &profile,
            ControllerTextMessage {
                message_id: "msg_external_controller_authority".to_string(),
                conversation_id: Some("direct:did:human:bob".to_string()),
                sender_did: "did:human:bob".to_string(),
                requester_user_id: Some("user-bob".to_string()),
                requester_full_handle: Some("bob.example.com".to_string()),
                trigger_kind: RuntimeTaskTriggerKind::ExternalDirect,
                invocation_authority: RuntimeInvocationAuthority::Controller,
                target_agent_did: profile.agent_did.clone(),
                text: "bob should not get controller authority in direct requester path"
                    .to_string(),
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("direct requester task requires requester authority"));
    }
}
