use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBindingConfig {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub workspace_mode: WorkspaceMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceMode {
    SharedRoot,
    WorktreePerTask,
    Container,
    Sandbox,
}

impl WorkspaceBindingConfig {
    pub fn validate(&self) -> Result<()> {
        if self.workspace_id.trim().is_empty() {
            bail!("workspace_id must not be empty");
        }
        if self.workspace_root.as_os_str().is_empty() {
            bail!("workspace_root must not be empty");
        }
        Ok(())
    }
}

impl WorkspaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SharedRoot => "shared-root",
            Self::WorktreePerTask => "worktree-per-task",
            Self::Container => "container",
            Self::Sandbox => "sandbox",
        }
    }

    pub fn is_security_boundary(self) -> bool {
        matches!(self, Self::Container | Self::Sandbox)
    }

    pub fn isolation_note(self) -> &'static str {
        match self {
            Self::SharedRoot => "不是硬隔离，只适合个人低风险或读任务",
            Self::WorktreePerTask => "只隔离代码变更，不防系统凭据读取",
            Self::Container => "可作为安全边界，依赖容器配置",
            Self::Sandbox => "可作为安全边界，依赖 sandbox profile",
        }
    }
}
