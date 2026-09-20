//! Delegated inbox policy. Execution remains in the shared ACP host.
use anyhow::{bail, Context, Result};

use crate::{
    app_bridge::personal_agent::{
        APP_MESSAGE_HANDLER_ROLE, APP_PERSONAL_AGENT_STATUS_ACTIVE, APP_PERSONAL_AGENT_STATUS_READY,
    },
    runtime::{
        RuntimeAgentProfile, RuntimeInvocationAuthority, RuntimeTask, RuntimeTaskTriggerKind,
    },
    state::AppPersonalAgentBindingRecord,
    DaemonState,
};

pub(crate) fn is_background(task: &RuntimeTask) -> bool {
    task.trigger_kind == RuntimeTaskTriggerKind::DelegatedDirect
}

/// Never infer delegated authority from message text, a profile name, or an
/// inactive binding. Recheck this policy during execution and before delivery.
pub(crate) fn binding(
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    task: &RuntimeTask,
) -> Result<AppPersonalAgentBindingRecord> {
    let binding = state
        .load_active_app_personal_agent_binding_by_runtime(&profile.agent_did)?
        .context("personal_agent_binding_inactive")?;
    let driver = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
    if !is_background(task)
        || profile.runtime_plugin_id != super::PLUGIN_ID
        || driver.driver_id != "hermes"
        || task.agent_did != profile.agent_did
        || task.controller_scope_key != profile.controller_scope_key
        || task.invocation_authority != RuntimeInvocationAuthority::Requester
        || task.conversation_scope.kind() != crate::runtime::RuntimeConversationScopeKind::Direct
        || binding.runtime_agent_did != profile.agent_did
        || binding.runtime_profile_id != profile.runtime_profile_id
        || binding.user_did != task.controller_did
        || binding.user_did != task.reply_recipient_did
        || binding.role != APP_MESSAGE_HANDLER_ROLE
        || !matches!(
            binding.status.as_str(),
            APP_PERSONAL_AGENT_STATUS_ACTIVE | APP_PERSONAL_AGENT_STATUS_READY
        )
    {
        bail!("personal_agent_binding_mismatch");
    }
    Ok(binding)
}

pub(crate) const PROMPT: &str = r#"
[Background personal assistant]
This is a noninteractive delegated inbox task for the owner APP.
Never ask questions, request human input, or wait for approval within this run.
If information is missing, deliver a useful partial summary or draft with the
missing information clearly stated. If no useful result is possible, explain
that manual handling is needed and end the task. Never invent an answer.
Deliver useful summaries/drafts through the existing app.action.request API:
write a JSON request to stdin of "$AWIKI_DAEMON_EXECUTABLE" __runtime-wrapper app-action.
Request fields are action, source_message_id, conversation_id, and args.
Use args.text for a message.summarize_plain summary, or args.draft_text for a
message.create_draft suggestion. Keep the original source message and conversation.
Allowed action names come from the delegated envelope and current APP policy;
the host validates them and the APP retains confirmation for write actions.
Do not claim an action was executed merely because it was queued for the APP.
Do not send any message or file directly to the original sender or third parties.
The final answer closes this background task; it does not send a chat message.
Never print credentials or put them in command arguments.
"#;
