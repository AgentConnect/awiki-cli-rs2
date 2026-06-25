use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::local_rpc::RuntimeRpcRequest;
use crate::security::runtime_token::RuntimeRpcToken;
use crate::workspace::{WorkspaceInstance, WorkspaceMode};

pub mod host;

pub fn canonical_full_handle(value: &str) -> Result<String> {
    let value = value.trim().trim_start_matches('@').to_ascii_lowercase();
    if value.is_empty() {
        bail!("full handle must not be empty");
    }
    if value.starts_with("did:") || !value.contains('.') {
        bail!("full handle must include handle and domain");
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAgentProfile {
    pub agent_did: String,
    pub agent_handle: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub runtime_profile_id: String,
    pub runtime_plugin_id: String,
    pub display_name: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub workspace_mode: Option<WorkspaceMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTask {
    pub task_id: String,
    pub agent_did: String,
    pub agent_handle: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub sender_did: String,
    pub requester_did: String,
    pub requester_user_id: Option<String>,
    pub requester_full_handle: Option<String>,
    pub trigger_kind: RuntimeTaskTriggerKind,
    pub conversation_scope: RuntimeConversationScope,
    pub invocation_authority: RuntimeInvocationAuthority,
    pub reply_recipient_did: String,
    pub conversation_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeConversationScope {
    ControllerPrivate {
        controller_scope_key: String,
    },
    Direct {
        requester_user_id: String,
        requester_full_handle: String,
    },
    GroupVisible {
        group_key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeConversationScopeKind {
    ControllerPrivate,
    Direct,
    GroupVisible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInvocationAuthority {
    Controller,
    Requester,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskTriggerKind {
    ControllerDirect,
    ExternalDirect,
    GroupMention,
    DelegatedDirect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRun {
    pub run_id: String,
    pub task_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub runtime_plugin_id: String,
    pub workspace_id: Option<String>,
    pub status: RuntimeRunStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRunStatus {
    Pending,
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RuntimeLaunchContext {
    pub run: RuntimeRun,
    pub task: RuntimeTask,
    pub workspace_root: Option<PathBuf>,
    pub workspace_instance: Option<WorkspaceInstance>,
    pub cli_route_session: Option<GenericCliRouteSession>,
    pub runtime_temp_dir: Option<PathBuf>,
    pub runtime_rpc_token: RuntimeRpcToken,
    pub local_socket_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericCliRouteSession {
    pub route_key: String,
    pub route_key_hash: String,
    pub session_dir: PathBuf,
    pub last_run_id: Option<String>,
    pub last_message_id: Option<String>,
    pub native_session_id: Option<String>,
    pub synthetic_session_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLaunchOutcome {
    pub run_id: String,
    pub status: RuntimeRunStatus,
    pub exit_code: Option<i32>,
    pub callbacks: Vec<RuntimeRpcRequest>,
    pub metadata: Value,
}

pub trait RuntimePlugin {
    fn plugin_id(&self) -> &str;
    fn check_install_status(&self) -> Result<RuntimeInstallStatus>;
    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInstallStatus {
    pub installed: bool,
    pub detail: Option<String>,
}

impl RuntimeAgentProfile {
    pub fn validate(&self) -> Result<()> {
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.agent_handle.trim().is_empty() {
            bail!("agent_handle must not be empty");
        }
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
        }
        if canonical_full_handle(&self.controller_full_handle)? != self.controller_full_handle {
            bail!("controller_full_handle must be canonical");
        }
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if self.runtime_plugin_id.trim().is_empty() {
            bail!("runtime_plugin_id must not be empty");
        }
        if self.workspace_id.as_deref().is_some_and(str::is_empty) {
            bail!("workspace_id must not be empty when present");
        }
        let workspace_fields = [
            self.workspace_id.is_some(),
            self.workspace_root.is_some(),
            self.workspace_mode.is_some(),
        ];
        if workspace_fields.iter().any(|present| *present)
            && workspace_fields.iter().any(|present| !*present)
        {
            bail!("workspace_id, workspace_root, and workspace_mode must be provided together");
        }
        Ok(())
    }
}

impl RuntimeTask {
    pub fn validate(&self) -> Result<()> {
        if self.task_id.trim().is_empty() {
            bail!("task_id must not be empty");
        }
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.agent_handle.trim().is_empty() {
            bail!("agent_handle must not be empty");
        }
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
        }
        if canonical_full_handle(&self.controller_full_handle)? != self.controller_full_handle {
            bail!("controller_full_handle must be canonical");
        }
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.sender_did.trim().is_empty() {
            bail!("sender_did must not be empty");
        }
        if self.requester_did.trim().is_empty() {
            bail!("requester_did must not be empty");
        }
        if self
            .requester_user_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("requester_user_id must not be empty when present");
        }
        if self
            .requester_full_handle
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("requester_full_handle must not be empty when present");
        }
        if let Some(handle) = self.requester_full_handle.as_deref() {
            if canonical_full_handle(handle)? != handle {
                bail!("requester_full_handle must be canonical");
            }
        }
        if self.reply_recipient_did.trim().is_empty() {
            bail!("reply_recipient_did must not be empty");
        }
        self.trigger_kind
            .validate_against_conversation(self.conversation_id.as_deref())?;
        self.conversation_scope.validate()?;
        self.validate_scope_and_authority()?;
        if self.text.trim().is_empty() {
            bail!("runtime task text must not be empty");
        }
        Ok(())
    }

    fn validate_scope_and_authority(&self) -> Result<()> {
        match (&self.conversation_scope, self.trigger_kind) {
            (
                RuntimeConversationScope::ControllerPrivate {
                    controller_scope_key,
                },
                RuntimeTaskTriggerKind::ControllerDirect,
            ) => {
                if controller_scope_key != &self.controller_scope_key {
                    bail!("controller private scope does not match task controller scope");
                }
                if self.invocation_authority != RuntimeInvocationAuthority::Controller {
                    bail!("controller direct task requires controller authority");
                }
                if self.requester_did != self.controller_did
                    || self.sender_did != self.controller_did
                    || self.reply_recipient_did != self.controller_did
                {
                    bail!("controller direct task requires controller sender and reply recipient");
                }
            }
            (
                RuntimeConversationScope::Direct {
                    requester_user_id,
                    requester_full_handle,
                },
                RuntimeTaskTriggerKind::ExternalDirect | RuntimeTaskTriggerKind::DelegatedDirect,
            ) => {
                if self.invocation_authority != RuntimeInvocationAuthority::Requester {
                    bail!("direct requester task requires requester authority");
                }
                if requester_user_id == &self.controller_user_id
                    && requester_full_handle == &self.controller_full_handle
                {
                    bail!("direct requester task must not use controller identity");
                }
                if self.requester_user_id.as_deref() != Some(requester_user_id.as_str()) {
                    bail!("direct scope requester_user_id does not match task requester");
                }
                if self.requester_full_handle.as_deref() != Some(requester_full_handle.as_str()) {
                    bail!("direct scope requester_full_handle does not match task requester");
                }
                if self.requester_did != self.sender_did
                    || self.reply_recipient_did != self.requester_did
                {
                    bail!("direct requester task requires requester sender and reply recipient");
                }
            }
            (
                RuntimeConversationScope::GroupVisible { .. },
                RuntimeTaskTriggerKind::GroupMention,
            ) => {
                let requester_user_id = self
                    .requester_user_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("group mention requires requester_user_id"))?;
                let requester_full_handle =
                    self.requester_full_handle.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("group mention requires requester_full_handle")
                    })?;
                let requester_is_controller = requester_user_id == self.controller_user_id
                    && requester_full_handle == self.controller_full_handle;
                match (self.invocation_authority, requester_is_controller) {
                    (RuntimeInvocationAuthority::Controller, false) => {
                        bail!("group controller authority requires controller identity");
                    }
                    (RuntimeInvocationAuthority::Requester, true) => {
                        bail!("group controller identity requires controller authority");
                    }
                    _ => {}
                }
                if self.requester_did != self.sender_did
                    || self.reply_recipient_did != self.requester_did
                {
                    bail!("group mention task requires requester sender and reply recipient");
                }
            }
            _ => bail!("runtime task trigger does not match conversation scope"),
        }
        if self.invocation_authority == RuntimeInvocationAuthority::Requester
            && self.trigger_kind == RuntimeTaskTriggerKind::ControllerDirect
        {
            bail!("requester authority cannot use controller direct trigger");
        }
        Ok(())
    }
}

impl RuntimeConversationScope {
    pub fn controller_private(controller_scope_key: impl Into<String>) -> Self {
        Self::ControllerPrivate {
            controller_scope_key: controller_scope_key.into(),
        }
    }

    pub fn direct(
        requester_user_id: impl Into<String>,
        requester_full_handle: impl Into<String>,
    ) -> Result<Self> {
        let requester_user_id = requester_user_id.into();
        let requester_full_handle = canonical_full_handle(&requester_full_handle.into())?;
        Ok(Self::Direct {
            requester_user_id,
            requester_full_handle,
        })
    }

    pub fn group_visible(group_key: impl Into<String>) -> Self {
        Self::GroupVisible {
            group_key: group_key.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::ControllerPrivate {
                controller_scope_key,
            } => {
                if controller_scope_key.trim().is_empty() {
                    bail!("controller private scope key must not be empty");
                }
            }
            Self::Direct {
                requester_user_id,
                requester_full_handle,
            } => {
                if requester_user_id.trim().is_empty() {
                    bail!("direct scope requester_user_id must not be empty");
                }
                if requester_full_handle.trim().is_empty() {
                    bail!("direct scope requester_full_handle must not be empty");
                }
            }
            Self::GroupVisible { group_key } => {
                if group_key.trim().is_empty() {
                    bail!("group scope key must not be empty");
                }
            }
        }
        Ok(())
    }

    pub fn kind(&self) -> RuntimeConversationScopeKind {
        match self {
            Self::ControllerPrivate { .. } => RuntimeConversationScopeKind::ControllerPrivate,
            Self::Direct { .. } => RuntimeConversationScopeKind::Direct,
            Self::GroupVisible { .. } => RuntimeConversationScopeKind::GroupVisible,
        }
    }

    pub fn scope_key(&self) -> String {
        match self {
            Self::ControllerPrivate {
                controller_scope_key,
            } => format!("controller:{controller_scope_key}"),
            Self::Direct {
                requester_user_id,
                requester_full_handle,
            } => format!("user:{requester_user_id}:handle:{requester_full_handle}"),
            Self::GroupVisible { group_key } => format!("group:{group_key}"),
        }
    }

    pub fn kind_str(&self) -> &'static str {
        self.kind().as_str()
    }
}

impl RuntimeConversationScopeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ControllerPrivate => "controller_private",
            Self::Direct => "direct",
            Self::GroupVisible => "group_visible",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "controller_private" => Ok(Self::ControllerPrivate),
            "direct" => Ok(Self::Direct),
            "group_visible" => Ok(Self::GroupVisible),
            other => bail!("unsupported runtime conversation scope kind: {other}"),
        }
    }
}

impl RuntimeInvocationAuthority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Requester => "requester",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "controller" => Ok(Self::Controller),
            "requester" => Ok(Self::Requester),
            other => bail!("unsupported runtime invocation authority: {other}"),
        }
    }

    pub fn can_send_outbound(&self) -> bool {
        matches!(self, Self::Controller)
    }
}

pub fn is_group_conversation_id(conversation_id: Option<&str>) -> bool {
    conversation_id
        .map(str::trim)
        .is_some_and(|value| value.starts_with("group:") && value.len() > "group:".len())
}

pub fn runtime_task_matches_profile_controller_scope(
    task: &RuntimeTask,
    profile: &RuntimeAgentProfile,
) -> bool {
    if task.agent_did != profile.agent_did
        || task.agent_handle != profile.agent_handle
        || task.controller_user_id != profile.controller_user_id
        || task.controller_full_handle != profile.controller_full_handle
        || task.controller_scope_key != profile.controller_scope_key
    {
        return false;
    }
    if task.controller_did != profile.controller_did {
        return false;
    }
    task.validate().is_ok()
}

impl RuntimeRunStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "finished" => Ok(Self::Finished),
            "failed" => Ok(Self::Failed),
            other => bail!("unsupported runtime run status: {other}"),
        }
    }
}

impl RuntimeTaskTriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ControllerDirect => "controller_direct",
            Self::ExternalDirect => "external_direct",
            Self::GroupMention => "group_mention",
            Self::DelegatedDirect => "delegated_direct",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "controller_direct" => Ok(Self::ControllerDirect),
            "external_direct" => Ok(Self::ExternalDirect),
            "group_mention" => Ok(Self::GroupMention),
            "delegated_direct" => Ok(Self::DelegatedDirect),
            other => bail!("unsupported runtime task trigger kind: {other}"),
        }
    }

    pub(crate) fn validate_against_conversation(
        &self,
        conversation_id: Option<&str>,
    ) -> Result<()> {
        let group = is_group_conversation_id(conversation_id);
        match (self, group) {
            (Self::GroupMention, false) => {
                bail!("group_mention trigger requires a group conversation")
            }
            (Self::ControllerDirect | Self::ExternalDirect, true) => {
                bail!("direct trigger must not use a group conversation")
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_full_handle_trims_at_prefix_and_lowercases() {
        assert_eq!(
            canonical_full_handle("  @Bob.Example.COM  ").unwrap(),
            "bob.example.com"
        );
    }

    #[test]
    fn canonical_full_handle_rejects_did_or_short_handle() {
        assert!(canonical_full_handle("did:wba:example.com:bob:e1_key").is_err());
        assert!(canonical_full_handle("bob").is_err());
    }

    #[test]
    fn direct_conversation_scope_uses_canonical_full_handle() {
        let scope = RuntimeConversationScope::direct("user-bob", " @Bob.Example.COM ").unwrap();

        assert_eq!(
            scope,
            RuntimeConversationScope::Direct {
                requester_user_id: "user-bob".to_string(),
                requester_full_handle: "bob.example.com".to_string()
            }
        );
        assert_eq!(scope.scope_key(), "user:user-bob:handle:bob.example.com");
    }
}
