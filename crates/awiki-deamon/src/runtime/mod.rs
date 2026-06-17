use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::local_rpc::RuntimeRpcRequest;
use crate::security::runtime_token::RuntimeRpcToken;
use crate::workspace::{WorkspaceInstance, WorkspaceMode};

pub mod host;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAgentProfile {
    pub agent_did: String,
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
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub sender_did: String,
    pub requester_did: String,
    pub requester_full_handle: Option<String>,
    pub trigger_kind: RuntimeTaskTriggerKind,
    pub reply_recipient_did: String,
    pub conversation_id: Option<String>,
    pub text: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub runtime_temp_dir: Option<PathBuf>,
    pub runtime_rpc_token: RuntimeRpcToken,
    pub local_socket_path: Option<PathBuf>,
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
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
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
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
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
            .requester_full_handle
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("requester_full_handle must not be empty when present");
        }
        if self.reply_recipient_did.trim().is_empty() {
            bail!("reply_recipient_did must not be empty");
        }
        self.trigger_kind
            .validate_against_conversation(self.conversation_id.as_deref())?;
        if self.text.trim().is_empty() {
            bail!("runtime task text must not be empty");
        }
        Ok(())
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
        || task.controller_user_id != profile.controller_user_id
        || task.controller_full_handle != profile.controller_full_handle
        || task.controller_scope_key != profile.controller_scope_key
    {
        return false;
    }
    if task.controller_did != profile.controller_did {
        return false;
    }
    match task.trigger_kind {
        RuntimeTaskTriggerKind::ControllerDirect => {
            !is_group_conversation_id(task.conversation_id.as_deref())
                && task.sender_did == profile.controller_did
                && task.requester_did == profile.controller_did
                && task.reply_recipient_did == profile.controller_did
        }
        RuntimeTaskTriggerKind::ExternalDirect => {
            !is_group_conversation_id(task.conversation_id.as_deref())
                && task.requester_did == task.sender_did
                && task.reply_recipient_did == task.requester_did
                && task.requester_did != profile.controller_did
        }
        RuntimeTaskTriggerKind::GroupMention => {
            is_group_conversation_id(task.conversation_id.as_deref())
                && task.requester_did == task.sender_did
                && task.reply_recipient_did == task.requester_did
        }
        RuntimeTaskTriggerKind::DelegatedDirect => {
            task.requester_did == task.sender_did && task.reply_recipient_did == task.requester_did
        }
    }
}

impl RuntimeRunStatus {
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
